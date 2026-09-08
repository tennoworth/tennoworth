//! `wfm-scrape` binary - host-only market pipeline (the only one; Python
//! retired 2026-08).
//!
//! Subcommands:
//! - `build`: reads `wfm_results.csv`, fetches upstreams, reconciles with the
//!   prior snapshot, and writes `market.json` + `wfstat-catalog.json`.
//! - `scrape`: the full WFM scrape to `wfm_results.csv`.
//!
//! Flags:
//! - `--fixtures-dir <DIR>`: run offline using frozen fixture files.
//!   Expects `<DIR>/fixture_responses.json` (URL→JSON map) and
//!   `<DIR>/wfm_results.csv`. Writes output to `<DIR>/market.json`.
//! - `--now <ISO>`: pin the injected clock (e.g. `2026-07-01T00:00:00Z`).

use chrono::Utc;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use wfm_scrape::pipeline::{build, find_root};
use wfm_scrape::{clock, ingest::LiveHttp};

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let Some(command) = args.get(1) else {
        eprintln!("usage: wfm-scrape build|scrape|history [--fixtures-dir <DIR>] [--now <ISO>]");
        return std::process::ExitCode::FAILURE;
    };
    let result = match command.as_str() {
        "history" => run_history_cmd(&args),
        "build" => {
            let fixtures_dir = extract_flag(&args, "--fixtures-dir");
            let now_arg = extract_flag(&args, "--now");
            let fixtures_path = fixtures_dir.as_deref().map(std::path::Path::new);
            build(fixtures_path, now_arg.as_deref())
        }
        "scrape" => run_scrape_cmd(&args),
        _ => {
            eprintln!("unknown subcommand: {command}");
            return std::process::ExitCode::FAILURE;
        }
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn extract_flag(args: &[String], flag: &str) -> Option<String> {
    let idx = args.iter().position(|a| a == flag)?;
    args.get(idx + 1).cloned()
}

/// Wire the `scrape` subcommand.
///
/// Accepts the exact flags run-scrape.sh passes (`--filter --exclude
/// --min-volume --out`) plus the rest of the argparse surface
/// (`--platform --limit --checkpoint-every`), all with matching defaults.
/// `--fixtures-dir <DIR>` swaps live HTTP for a frozen `fixture_responses.json`
/// (URL→body) and disables real sleeps, so the fixture regression tests run
/// offline and instantly. `--now` is accepted for symmetry with `build` but is
/// inert here - the scrape CSV carries no timestamp.
fn run_scrape_cmd(args: &[String]) -> Result<(), String> {
    use wfm_scrape::http::{
        FixtureScrapeHttp, LiveScrapeHttp, NoopSleeper, RealSleeper, ScrapeHttp, Sleeper,
    };
    use wfm_scrape::scrape::{run_scrape, ScrapeConfig};

    let parse_usize = |s: String, what: &str| -> Result<usize, String> {
        s.parse::<usize>()
            .map_err(|_| format!("invalid {what}: {s}"))
    };
    let parse_i64 = |s: String, what: &str| -> Result<i64, String> {
        s.parse::<i64>().map_err(|_| format!("invalid {what}: {s}"))
    };

    let fixtures_dir = extract_flag(args, "--fixtures-dir");
    let platform = extract_flag(args, "--platform").unwrap_or_else(|| "pc".into());
    wfm_client::validate_platform(&platform)?;

    let mut cfg = ScrapeConfig {
        filter: extract_flag(args, "--filter").unwrap_or_else(|| "prime".into()),
        exclude: extract_flag(args, "--exclude").unwrap_or_else(|| "set".into()),
        platform: platform.clone(),
        limit: extract_flag(args, "--limit")
            .map(|s| parse_usize(s, "--limit"))
            .transpose()?
            .unwrap_or(0),
        min_volume: extract_flag(args, "--min-volume")
            .map(|s| parse_i64(s, "--min-volume"))
            .transpose()?
            .unwrap_or(5),
        out: PathBuf::from(extract_flag(args, "--out").unwrap_or_else(|| "wfm_results.csv".into())),
        checkpoint_every: extract_flag(args, "--checkpoint-every")
            .map(|s| parse_usize(s, "--checkpoint-every"))
            .transpose()?
            .unwrap_or(100),
        max_coercions: wfm_scrape::coerce::DEFAULT_MAX_COERCIONS,
    };

    let (http, sleeper): (Box<dyn ScrapeHttp>, Box<dyn Sleeper>) = if let Some(fd) = &fixtures_dir {
        let fd = Path::new(fd);
        // Default the output into the fixtures dir when --out was not given, so
        // a fixture run never scribbles into the cwd.
        if extract_flag(args, "--out").is_none() {
            cfg.out = fd.join("wfm_results.csv");
        }
        let resp_path = fd.join("fixture_responses.json");
        let raw =
            std::fs::read_to_string(&resp_path).map_err(|e| format!("read {resp_path:?}: {e}"))?;
        let responses: HashMap<String, serde_json::Value> =
            serde_json::from_str(&raw).map_err(|e| format!("parse {resp_path:?}: {e}"))?;
        (
            Box::new(FixtureScrapeHttp::new(responses)),
            Box::new(NoopSleeper),
        )
    } else {
        let client = wfm_client::build_client(30).map_err(|e| format!("build HTTP client: {e}"))?;
        (
            Box::new(LiveScrapeHttp { client, platform }),
            Box::new(RealSleeper),
        )
    };

    let summary = run_scrape(http.as_ref(), sleeper.as_ref(), &cfg)?;
    eprintln!(
        "scrape complete: scanned {}, kept {}, coercions {} → {}",
        summary.scanned,
        summary.kept,
        summary.coercions,
        cfg.out.display()
    );
    if summary.kept == 0 {
        eprintln!("No items matched your criteria. Try lowering --min-volume.");
    }
    Ok(())
}

/// `wfm-scrape history [--out <history.json>] [--market <market.json>]
///                    [--days N] [--bootstrap-days N] [--now <ISO>]`
///
/// Box-only, run AFTER `build` (it joins relics.run's display names through
/// the freshly written market.json catalog). The output file is also the
/// state: only the days after the last stored one are fetched. See
/// `history.rs` for the shape and the reasoning.
fn run_history_cmd(args: &[String]) -> Result<(), String> {
    use wfm_scrape::history::{update_history, History, DEFAULT_DAYS};

    let now = extract_flag(args, "--now")
        .map(|s| clock::parse_stamp(&s).ok_or_else(|| format!("invalid --now stamp: {s}")))
        .unwrap_or_else(|| Ok(Utc::now()))?;
    let root = find_root().ok();
    let public = root.as_ref().map(|r| r.join("frontend").join("public"));
    let out = extract_flag(args, "--out")
        .map(PathBuf::from)
        .or_else(|| public.as_ref().map(|p| p.join("history.json")))
        .ok_or("--out is required outside the repo")?;
    let market_path = extract_flag(args, "--market")
        .map(PathBuf::from)
        .or_else(|| public.as_ref().map(|p| p.join("market.json")))
        .ok_or("--market is required outside the repo")?;
    let days: usize = extract_flag(args, "--days")
        .map(|s| s.parse().map_err(|_| format!("bad --days: {s}")))
        .transpose()?
        .unwrap_or(DEFAULT_DAYS);
    let bootstrap_days: usize = extract_flag(args, "--bootstrap-days")
        .map(|s| s.parse().map_err(|_| format!("bad --bootstrap-days: {s}")))
        .transpose()?
        .unwrap_or(days);

    // Display name → slug from the market snapshot's catalog (name_lower → slug).
    let market_raw = std::fs::read_to_string(&market_path)
        .map_err(|e| format!("read {}: {e}", market_path.display()))?;
    let market: serde_json::Value = serde_json::from_str(&market_raw)
        .map_err(|e| format!("parse {}: {e}", market_path.display()))?;
    let name_to_slug: HashMap<String, String> = market
        .get("catalog")
        .and_then(|c| c.as_object())
        .map(|c| {
            c.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.to_lowercase(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();
    if name_to_slug.is_empty() {
        return Err(format!(
            "{} has no catalog - run `wfm-scrape build` first",
            market_path.display()
        ));
    }

    let prior: Option<History> = std::fs::read_to_string(&out)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok());
    match &prior {
        Some(p) => eprintln!("history: prior {} → {} ({} items)", p.start, p.through.as_deref().unwrap_or("-"), p.items.len()),
        None => eprintln!("history: no prior - bootstrapping up to {bootstrap_days} days (one relics.run file per day, ~4 MB each)"),
    }

    let client = reqwest::blocking::Client::builder()
        .user_agent(wfm_client::user_agent(
            "wfm-scrape",
            env!("CARGO_PKG_VERSION"),
        ))
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| format!("build HTTP client: {e}"))?;
    let http = LiveHttp { client };
    let yesterday = now.date_naive() - chrono::Duration::days(1);
    let (hist, summary) = update_history(
        &http,
        prior,
        &name_to_slug,
        yesterday,
        &clock::iso_z(now),
        days,
        bootstrap_days,
        &|ms| std::thread::sleep(std::time::Duration::from_millis(ms)),
    );
    eprintln!(
        "history: fetched {} day(s), {} failed, {} items, {} relics.run names not in the catalog",
        summary.fetched, summary.failed, summary.items, summary.unmatched_names
    );
    if summary.fetched == 0 {
        // Nothing new (or only not-yet-published days) - the artifact is
        // unchanged, so don't rewrite it and don't bump generated_at.
        eprintln!("history: nothing new - not rewritten");
        return Ok(());
    }
    let tmp = out.with_extension("json.tmp");
    let json = serde_json::to_string(&hist).map_err(|e| format!("serialize: {e}"))?;
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
    }
    std::fs::write(&tmp, &json).map_err(|e| format!("write tmp: {e}"))?;
    std::fs::rename(&tmp, &out).map_err(|e| format!("rename: {e}"))?;
    eprintln!(
        "Wrote {} ({} bytes, window {} → {}, data through {})",
        out.display(),
        json.len(),
        hist.start,
        hist.end_date().map(|d| d.to_string()).unwrap_or_default(),
        hist.through.as_deref().unwrap_or("-")
    );
    Ok(())
}
