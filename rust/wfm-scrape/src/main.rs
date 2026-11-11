//! `tennoworth-pipeline` binary (crate `wfm-scrape`) - the host-only market pipeline.
//!
//! Subcommands:
//! - `build`: reads `wfm_results.csv`, fetches upstreams, reconciles with the
//!   prior snapshot, and writes `market.json` + `wfstat-catalog.json`.
//! - `scrape`: the full WFM scrape to `wfm_results.csv`.
//! - `art`: download new or changed item pictures from the Warframe wiki into
//!   `<publish>/item-art/` and write `<publish>/item-art.json`.
//! - `stamp <market.json>`: print the snapshot's `updated_at` as Unix seconds,
//!   or fail when the file breaks the market contract. The host check judges
//!   freshness with it: the box has no other JSON parser.
//!
//! Flags:
//! - `--fixtures-dir <DIR>`: run offline using frozen fixture files.
//!   Expects `<DIR>/fixture_responses.json` (URL→JSON map) and
//!   `<DIR>/wfm_results.csv`. Writes output to `<DIR>/market.json`.
//! - `--now <ISO>`: pin the injected clock (e.g. `2026-07-01T00:00:00Z`).

use chrono::Utc;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use wfm_scrape::pipeline::{build, find_root, Dirs};
use wfm_scrape::{clock, ingest::LiveHttp};

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let Some(command) = args.get(1) else {
        eprintln!("usage: tennoworth-pipeline build|scrape|history|art|stamp [--fixtures-dir <DIR>] [--data-dir <DIR>] [--publish-dir <DIR>] [--now <ISO>]");
        return std::process::ExitCode::FAILURE;
    };
    let live = extract_flag(&args, "--fixtures-dir").is_none();
    let dirs = if live && matches!(command.as_str(), "build" | "art") {
        match Dirs::resolve(
            extract_flag(&args, "--data-dir").map(PathBuf::from),
            extract_flag(&args, "--publish-dir").map(PathBuf::from),
        ) {
            Ok(dirs) => Some(dirs),
            Err(error) => {
                eprintln!("error: {error}");
                return std::process::ExitCode::FAILURE;
            }
        }
    } else {
        None
    };
    // The lock directory also holds the WFM cooldown and policy state, so
    // `scrape` and `build` must name the same one: the CSV's directory.
    let _output_lock = if live && matches!(command.as_str(), "scrape" | "build" | "art") {
        let directory = match &dirs {
            Some(dirs) => dirs.data.clone(),
            None => PathBuf::from(
                extract_flag(&args, "--out").unwrap_or_else(|| "wfm_results.csv".into()),
            )
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
            .to_path_buf(),
        };
        // The art timer can land while a sweep holds the directory: it waits
        // for the sweep instead of failing, and leaves the day to the next run
        // if the sweep is still busy.
        let wait = if command == "art" {
            ART_LOCK_WAIT
        } else {
            std::time::Duration::ZERO
        };
        match acquire_output(&directory, wait) {
            Ok(Some(file)) => Some(file),
            Ok(None) if command == "art" => {
                eprintln!(
                    "art: a scrape still holds the output directory; the next run tries again"
                );
                return std::process::ExitCode::SUCCESS;
            }
            Ok(None) => {
                eprintln!("Another scraper is using this output directory.");
                return std::process::ExitCode::FAILURE;
            }
            Err(error) => {
                eprintln!("{error}");
                return std::process::ExitCode::FAILURE;
            }
        }
    } else {
        None
    };
    let result = match command.as_str() {
        "history" => run_history_cmd(&args),
        "build" => {
            let fixtures_dir = extract_flag(&args, "--fixtures-dir");
            let now_arg = extract_flag(&args, "--now");
            let fixtures_path = fixtures_dir.as_deref().map(std::path::Path::new);
            build(fixtures_path, now_arg.as_deref(), dirs.as_ref())
        }
        "scrape" => run_scrape_cmd(&args),
        "art" => run_art_cmd(&args, dirs.as_ref()),
        "stamp" => run_stamp_cmd(&args),
        _ => {
            eprintln!("unknown subcommand: {command}");
            return std::process::ExitCode::FAILURE;
        }
    };
    // One line per invocation, success or failure: the counters are the only
    // record of what this process asked WFM for, and a failed run is when that
    // matters most - it will be repeated. Only scrape and build touch WFM.
    if matches!(command.as_str(), "scrape" | "build") {
        let published = if command == "scrape" {
            extract_flag(&args, "--out").unwrap_or_else(|| "wfm_results.csv".into())
        } else {
            dirs.as_ref().map_or_else(
                || "frontend/public/market.json".to_string(),
                |dirs| dirs.publish.join("market.json").display().to_string(),
            )
        };
        eprintln!(
            "{}",
            wfm_scrape::scrape::http::metrics().line(wfm_scrape::scrape::http::snapshot_age_s(
                std::path::Path::new(&published)
            ))
        );
    }
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// How long `art` waits for a sweep to release the output directory. Well inside
/// the art unit's hour-long start timeout, with room for its own ten minutes.
const ART_LOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(30 * 60);
const LOCK_POLL: std::time::Duration = std::time::Duration::from_secs(1);

/// Takes the output directory, waiting up to `wait` for another holder.
/// `None` means it was still held when the wait ran out.
fn acquire_output(
    directory: &Path,
    wait: std::time::Duration,
) -> Result<Option<std::fs::File>, String> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(directory.join(".wfm-output.lock"))
        .map_err(|e| format!("Opening scrape lock: {e}"))?;
    let deadline = std::time::Instant::now() + wait;
    loop {
        match file.try_lock() {
            Ok(()) => break,
            Err(std::fs::TryLockError::WouldBlock) => {
                let left = deadline.saturating_duration_since(std::time::Instant::now());
                if left.is_zero() {
                    return Ok(None);
                }
                std::thread::sleep(left.min(LOCK_POLL));
            }
            Err(std::fs::TryLockError::Error(e)) => return Err(format!("Taking scrape lock: {e}")),
        }
    }
    wfm_client::policy::start(directory, wfm_client::policy::Component::Scraper)
        .map_err(|e| e.to_string())?;
    Ok(Some(file))
}

/// The art timer fires once a day somewhere in a 60-minute window, so two runs
/// can be under 24h apart; a 24h minimum skipped every other day. 20h still
/// keeps a manual run from being repeated the same day.
const ART_INTERVAL_HOURS: i64 = 20;

fn ran_recently(last_run: Option<&str>, now: chrono::DateTime<Utc>, hours: i64) -> bool {
    last_run
        .and_then(clock::parse_stamp)
        .is_some_and(|t| now - t < chrono::Duration::hours(hours))
}

/// Wire the `art` subcommand. Runs at most once per `--interval-hours`
/// (default 20) however often it is called.
fn run_art_cmd(args: &[String], dirs: Option<&Dirs>) -> Result<(), String> {
    use wfm_scrape::art;
    let dirs = dirs.ok_or("art needs --data-dir and --publish-dir, or a checkout")?;
    let now = extract_flag(args, "--now")
        .map(|s| clock::parse_stamp(&s).ok_or_else(|| format!("invalid --now stamp: {s}")))
        .unwrap_or_else(|| Ok(Utc::now()))?;
    let hours: i64 = extract_flag(args, "--interval-hours")
        .map(|s| s.parse().map_err(|_| format!("bad --interval-hours: {s}")))
        .transpose()?
        .unwrap_or(ART_INTERVAL_HOURS);
    let mut opts = art::Options::default();
    if let Some(b) = extract_flag(args, "--budget") {
        opts.budget = b.parse().map_err(|_| format!("bad --budget: {b}"))?;
    }

    let state_path = dirs.data.join(art::STATE_FILE);
    let mut state: art::State = match std::fs::read(&state_path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| {
            format!(
                "{}: {e} - move it aside to start over",
                state_path.display()
            )
        })?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => art::State::default(),
        Err(e) => return Err(format!("read {}: {e}", state_path.display())),
    };
    let recent = ran_recently(state.last_run.as_deref(), now, hours);
    if recent && !args.iter().any(|a| a == "--force") {
        eprintln!("art: last run under {hours}h ago; nothing to do");
        return Ok(());
    }

    let read_json = |p: &Path| -> Result<serde_json::Value, String> {
        let bytes = std::fs::read(p).map_err(|e| format!("read {}: {e}", p.display()))?;
        serde_json::from_slice(&bytes).map_err(|e| format!("parse {}: {e}", p.display()))
    };
    let market = read_json(&dirs.publish.join("market.json"))?;
    let sources: Vec<art::Source> =
        serde_json::from_value(read_json(&dirs.data.join(art::SOURCES_FILE))?)
            .map_err(|e| format!("{}: {e} - run `build` first", art::SOURCES_FILE))?;
    let wanted = art::resolve(&market, &sources);
    eprintln!("art: {} items have a wiki picture", wanted.len());

    let fetch = art::LiveFetch::new()?;
    let summary = art::run(
        &wanted,
        &mut state,
        &fetch,
        &dirs.publish,
        now,
        &opts,
        &|d| std::thread::sleep(d),
    )?;
    let body = serde_json::to_vec(&state).map_err(|e| e.to_string())?;
    let tmp = state_path.with_extension("json.tmp");
    std::fs::write(&tmp, body).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &state_path).map_err(|e| format!("rename: {e}"))?;
    eprintln!(
        "art: {} requested · {} stored · {} unchanged · {} missing · {} published",
        summary.requested, summary.stored, summary.unchanged, summary.missing, summary.published
    );
    if let Some(why) = summary.stopped {
        eprintln!("art: stopped early ({why}); the next run resumes");
    }
    Ok(())
}

fn run_stamp_cmd(args: &[String]) -> Result<(), String> {
    let path = args
        .get(2)
        .ok_or("usage: tennoworth-pipeline stamp <market.json>")?;
    let body = std::fs::read_to_string(path).map_err(|e| format!("read {path}: {e}"))?;
    let stamp = contract_stamp(&body)
        .ok_or_else(|| format!("{path} is not a market snapshot with an RFC 3339 updated_at"))?;
    println!("{}", stamp.timestamp());
    Ok(())
}

/// The stamp of a body that meets the runtime market contract every reader
/// applies (`tests/fixtures/market-contract/cases.json`): an RFC 3339
/// `updated_at` and a non-empty `items` object. A snapshot no reader accepts
/// is no fresher than a missing one.
fn contract_stamp(body: &str) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    let root = value.as_object()?;
    let stamp = chrono::DateTime::parse_from_rfc3339(root.get("updated_at")?.as_str()?).ok()?;
    if root.get("items")?.as_object()?.is_empty() {
        return None;
    }
    Some(stamp)
}

fn extract_flag(args: &[String], flag: &str) -> Option<String> {
    let idx = args.iter().position(|a| a == flag)?;
    args.get(idx + 1).cloned()
}

/// Wire the `scrape` subcommand.
///
/// Accepts the exact flags run-scrape.sh passes (`--filter --exclude
/// --min-volume --out`) plus the rest of the argparse
/// surface (`--platform --limit --checkpoint-every --now`), all with matching
/// defaults.
/// `--fixtures-dir <DIR>` swaps live HTTP for a frozen `fixture_responses.json`
/// (URL→body) and disables real sleeps, so the fixture regression tests run
/// offline and instantly. `--now` is accepted for symmetry with `build` but is
/// inert here - the scrape CSV carries no timestamp.
fn run_scrape_cmd(args: &[String]) -> Result<(), String> {
    use wfm_scrape::scrape::http::{
        FixtureScrapeHttp, LiveScrapeHttp, NoopSleeper, RealSleeper, ScrapeHttp, Sleeper,
    };
    use wfm_scrape::scrape::{run_scrape, ScrapeConfig, StatsCacheConfig};

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
        stats_cache: match (
            extract_flag(args, "--stats-cache"),
            extract_flag(args, "--stats-interval"),
        ) {
            (Some(path), Some(interval)) => Some(StatsCacheConfig {
                path: PathBuf::from(path),
                interval: wfm_scrape::scrape::stats_cache::parse_interval(&interval)?,
            }),
            (None, None) => None,
            _ => return Err("--stats-cache and --stats-interval go together".into()),
        },
        now: extract_flag(args, "--now")
            .map(|s| {
                wfm_scrape::clock::parse_stamp(&s)
                    .ok_or_else(|| format!("invalid --now stamp: {s}"))
            })
            .transpose()?
            .unwrap_or_else(Utc::now),
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
        "scrape complete: scanned {}, kept {}, coercions {}, workers {}, statistics fetched {} reused {} → {}",
        summary.scanned,
        summary.kept,
        summary.coercions,
        summary.workers,
        summary.statistics_fetched,
        summary.statistics_reused,
        cfg.out.display()
    );
    Ok(())
}

/// `tennoworth-pipeline history [--publish-dir <DIR>] [--out <history.json>]
///                    [--market <market.json>] [--days N] [--bootstrap-days N]
///                    [--now <ISO>]`
///
/// Box-only, run AFTER `build` (it joins relics.run's display names through
/// the freshly written market.json catalog). The output file is also the
/// state: only the days after the last stored one are fetched. See
/// `history.rs` for the shape and the reasoning.
fn run_history_cmd(args: &[String]) -> Result<(), String> {
    use wfm_scrape::pipeline::history::{read_prior, update_history, DEFAULT_DAYS};

    let now = extract_flag(args, "--now")
        .map(|s| clock::parse_stamp(&s).ok_or_else(|| format!("invalid --now stamp: {s}")))
        .unwrap_or_else(|| Ok(Utc::now()))?;
    let public = match extract_flag(args, "--publish-dir") {
        Some(dir) => Some(PathBuf::from(dir)),
        None => find_root()
            .ok()
            .map(|root| root.join("frontend").join("public")),
    };
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
            "{} has no catalog - run `tennoworth-pipeline build` first",
            market_path.display()
        ));
    }

    let prior = read_prior(&out)?;
    match &prior {
        Some(p) => eprintln!("history: prior {} → {} ({} items)", p.start, p.through.as_deref().unwrap_or("-"), p.items.len()),
        None => eprintln!("history: no prior - bootstrapping up to {bootstrap_days} days (one relics.run file per day, ~4 MB each)"),
    }

    let client = reqwest::blocking::Client::builder()
        .retry(reqwest::retry::never())
        .redirect(wfm_client::redirect_policy())
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn the_stamp_follows_the_shared_market_contract() {
        let cases: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/market-contract/cases.json"
        ))
        .unwrap();
        for case in cases.as_array().unwrap() {
            let body = serde_json::to_string(&case["body"]).unwrap();
            assert_eq!(
                contract_stamp(&body).is_some(),
                case["valid"].as_bool().unwrap(),
                "{}",
                case["name"]
            );
        }
        // The host check compares instants, so an offset is resolved, not dropped.
        let offset = r#"{"updated_at":"2026-10-13T13:00:00+02:00","items":{"a":{}}}"#;
        assert_eq!(contract_stamp(offset).unwrap().timestamp(), 1_791_889_200);
    }

    #[test]
    fn a_daily_timer_that_fires_a_little_early_still_runs() {
        let now = clock::parse_stamp("2026-11-07T04:26:55Z").unwrap();
        // The production case: the previous run started 23h51m earlier.
        assert!(!ran_recently(
            Some("2026-11-06T04:35:56Z"),
            now,
            ART_INTERVAL_HOURS
        ));
        // A manual run earlier the same day still holds the next one back.
        assert!(ran_recently(
            Some("2026-11-06T20:32:43Z"),
            now,
            ART_INTERVAL_HOURS
        ));
        assert!(!ran_recently(None, now, ART_INTERVAL_HOURS));
    }

    struct Dir(PathBuf);
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn dir(name: &str) -> Dir {
        let p = std::env::temp_dir().join(format!("wfm-lock-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&p).unwrap();
        Dir(p)
    }

    #[test]
    fn a_busy_directory_fails_at_once_without_a_wait() {
        let d = dir("busy");
        let held = acquire_output(&d.0, Duration::ZERO).unwrap();
        assert!(held.is_some());
        assert!(acquire_output(&d.0, Duration::ZERO).unwrap().is_none());
    }

    #[test]
    fn a_waiting_caller_takes_the_directory_once_it_is_released() {
        let d = dir("wait");
        let held = acquire_output(&d.0, Duration::ZERO).unwrap().unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            drop(held);
        });
        let started = std::time::Instant::now();
        assert!(acquire_output(&d.0, Duration::from_secs(30))
            .unwrap()
            .is_some());
        assert!(started.elapsed() < Duration::from_secs(5));
        release.join().unwrap();
    }
}
