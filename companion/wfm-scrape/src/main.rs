//! `wfm-scrape` binary — host-only market pipeline (the only one; Python
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

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::Utc;

use wfm_scrape::clock;

/// Days after which a preserved-from-prior surface triggers a "this surface
/// is stale" warning.
const STALE_DAYS: i64 = 7;
use wfm_scrape::csvin;
use wfm_scrape::fetch::{self, FixtureHttp, Http, LiveHttp};
use wfm_scrape::reconcile::reconcile;
use wfm_scrape::render::{self, assemble_snapshot, CatalogItemMeta};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: wfm-scrape build|scrape|history [--fixtures-dir <DIR>] [--now <ISO>]");
        std::process::exit(1);
    }
    match args[1].as_str() {
        "history" => {
            if let Err(e) = run_history_cmd(&args) {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
        "build" => {
            let fixtures_dir = extract_flag(&args, "--fixtures-dir");
            let now_arg = extract_flag(&args, "--now");
            let fixtures_path = fixtures_dir.as_deref().map(|s| std::path::Path::new(s));
            if let Err(e) = run_build(fixtures_path, now_arg.as_deref()) {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
        "scrape" => {
            if let Err(e) = run_scrape_cmd(&args) {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
        _ => {
            eprintln!("unknown subcommand: {}", args[1]);
            std::process::exit(1);
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
/// inert here — the scrape CSV carries no timestamp.
fn run_scrape_cmd(args: &[String]) -> Result<(), String> {
    use wfm_scrape::http::{FixtureScrapeHttp, LiveScrapeHttp, NoopSleeper, RealSleeper, ScrapeHttp, Sleeper};
    use wfm_scrape::scrape::{run_scrape, ScrapeConfig};

    let parse_usize = |s: String, what: &str| -> Result<usize, String> {
        s.parse::<usize>().map_err(|_| format!("invalid {what}: {s}"))
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
        limit: extract_flag(args, "--limit").map(|s| parse_usize(s, "--limit")).transpose()?.unwrap_or(0),
        min_volume: extract_flag(args, "--min-volume").map(|s| parse_i64(s, "--min-volume")).transpose()?.unwrap_or(5),
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
        let raw = std::fs::read_to_string(&resp_path).map_err(|e| format!("read {resp_path:?}: {e}"))?;
        let responses: HashMap<String, serde_json::Value> =
            serde_json::from_str(&raw).map_err(|e| format!("parse {resp_path:?}: {e}"))?;
        (Box::new(FixtureScrapeHttp::new(responses)), Box::new(NoopSleeper))
    } else {
        let client = wfm_client::build_client(30).map_err(|e| format!("build HTTP client: {e}"))?;
        (Box::new(LiveScrapeHttp { client, platform }), Box::new(RealSleeper))
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
    let public = root.as_ref().map(|r| r.join("prototype").join("public"));
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
    let market_raw = std::fs::read_to_string(&market_path).map_err(|e| format!("read {}: {e}", market_path.display()))?;
    let market: serde_json::Value = serde_json::from_str(&market_raw).map_err(|e| format!("parse {}: {e}", market_path.display()))?;
    let name_to_slug: HashMap<String, String> = market
        .get("catalog")
        .and_then(|c| c.as_object())
        .map(|c| c.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.to_lowercase(), s.to_string()))).collect())
        .unwrap_or_default();
    if name_to_slug.is_empty() {
        return Err(format!("{} has no catalog — run `wfm-scrape build` first", market_path.display()));
    }

    let prior: Option<History> = std::fs::read_to_string(&out)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok());
    match &prior {
        Some(p) => eprintln!("history: prior {} → {} ({} items)", p.start, p.through.as_deref().unwrap_or("-"), p.items.len()),
        None => eprintln!("history: no prior — bootstrapping up to {bootstrap_days} days (one relics.run file per day, ~4 MB each)"),
    }

    let client = reqwest::blocking::Client::builder()
        .user_agent(wfm_client::user_agent("wfm-scrape", env!("CARGO_PKG_VERSION")))
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
        // Nothing new (or only not-yet-published days) — the artifact is
        // unchanged, so don't rewrite it and don't bump generated_at.
        eprintln!("history: nothing new — not rewritten");
        return Ok(());
    }
    let tmp = out.with_extension("json.tmp");
    let json = serde_json::to_string(&hist).map_err(|e| format!("serialize: {e}"))?;
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
    }
    std::fs::write(&tmp, &json).map_err(|e| format!("write tmp: {e}"))?;
    std::fs::rename(&tmp, &out).map_err(|e| format!("rename: {e}"))?;
    eprintln!("Wrote {} ({} bytes, window {} → {}, data through {})", out.display(), json.len(), hist.start, hist.end_date().map(|d| d.to_string()).unwrap_or_default(), hist.through.as_deref().unwrap_or("-"));
    Ok(())
}

fn run_build(fixtures_dir: Option<&Path>, now_arg: Option<&str>) -> Result<(), String> {
    let now = now_arg
        .map(|s| clock::parse_stamp(s).ok_or_else(|| format!("invalid --now stamp: {s}")))
        .unwrap_or_else(|| Ok(Utc::now()))?;

    let (http, csv_path, json_out, catalog_out, prior): (
        Box<dyn Http>,
        PathBuf,
        PathBuf,
        PathBuf,
        serde_json::Value,
    ) = if let Some(fd) = fixtures_dir {
        let resp_path = fd.join("fixture_responses.json");
        let raw = std::fs::read_to_string(&resp_path).map_err(|e| format!("read {resp_path:?}: {e}"))?;
        let responses: HashMap<String, serde_json::Value> =
            serde_json::from_str(&raw).map_err(|e| format!("parse {resp_path:?}: {e}"))?;
        let http = FixtureHttp { responses };
        let csv = fd.join("wfm_results.csv");
        let out = fd.join("market.json");
        let cat = fd.join("wfstat-catalog.json");
        let prior_path = fd.join("prior-market.json");
        let prior = if prior_path.exists() {
            let s = std::fs::read_to_string(&prior_path).map_err(|e| format!("read prior: {e}"))?;
            serde_json::from_str(&s).unwrap_or(serde_json::Value::Object(serde_json::Map::new()))
        } else {
            serde_json::Value::Object(serde_json::Map::new())
        };
        let prior_catalog = fd.join("prior-catalog.json");
        if prior_catalog.exists() && !cat.exists() {
            eprintln!("  preserving prior wfstat-catalog");
        }
        (Box::new(http), csv, out, cat, prior)
    } else {
        let root = find_root()?;
        let csv = root.join("wfm_results.csv");
        let out = root.join("prototype").join("public").join("market.json");
        let cat = root.join("prototype").join("public").join("wfstat-catalog.json");

        let client = reqwest::blocking::Client::builder()
            .user_agent(wfm_client::user_agent("wfm-scrape", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| format!("build HTTP client: {e}"))?;
        let http = LiveHttp { client };

        let prior = if out.exists() {
            let s = std::fs::read_to_string(&out).map_err(|e| format!("read prior: {e}"))?;
            serde_json::from_str(&s).unwrap_or(serde_json::Value::Object(serde_json::Map::new()))
        } else {
            serde_json::Value::Object(serde_json::Map::new())
        };

        (Box::new(http), csv, out, cat, prior)
    };

    if !csv_path.exists() {
        return Err(format!("{} not found — run `wfm-scrape scrape` first.", csv_path.display()));
    }

    let prior_stamps: HashMap<String, String> = prior
        .get("surface_fetched_at")
        .and_then(|s| s.as_object())
        .map(|m| m.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string())).collect())
        .unwrap_or_default();

    eprintln!("Fetching warframe.market master catalog...");
    let (catalog, meta_by_slug) = match fetch::fetch_catalog_wfm(http.as_ref(), "https://api.warframe.market/v2/items") {
        Ok(v) => v,
        Err(e) => {
            let Some(prior_catalog) = prior.get("catalog").and_then(|c| c.as_object()) else {
                return Err(format!("{e} — and no prior snapshot to fall back on."));
            };
            eprintln!("  {e} — reusing the prior snapshot's catalog");
            let cat: HashMap<String, String> = prior_catalog.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string())).collect();
            let items_meta: HashMap<String, CatalogItemMeta> = prior
                .get("items")
                .and_then(|i| i.as_object())
                .map(|items| {
                    items.iter().map(|(slug, it)| {
                        (slug.clone(), CatalogItemMeta {
                            tags: it.get("tags").and_then(|t| t.as_array())
                                .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                                .unwrap_or_default(),
                            ducats: it.get("ducats").and_then(|d| d.as_i64()),
                            max_rank: None,
                            subtypes: vec![],
                        })
                    }).collect()
                })
                .unwrap_or_default();
            (cat, items_meta)
        }
    };
    eprintln!("  {} items", catalog.len());

    // Fetched BEFORE the parent walk, and only once: sentinel parents now come
    // out of this payload (their own endpoint 404s since ~2026-07-31) and the
    // resolver catalog is reduced from the same copy further down. Two
    // consumers, one ~44 MB download — a second one would add minutes to a
    // scrape that already runs close to its systemd timeout.
    eprintln!("Fetching warframestat bulk item catalog...");
    let wfstat_raw: Option<serde_json::Value> = if fixtures_dir.is_none() {
        fetch::fetch_wfstat_raw()
            .map_err(|e| eprintln!("  warning: {e}"))
            .ok()
    } else {
        http.get_json(fetch::WFSTAT_ITEMS_URL)
            .map_err(|e| eprintln!("  warning: {e}"))
            .ok()
    };

    eprintln!("Fetching warframestat component path map + sets...");
    let (path_to_info, set_to_parts, parents_complete) =
        fetch::fetch_parent_data(http.as_ref(), &catalog, wfstat_raw.as_ref());
    eprintln!("  {} component paths · {} prime sets", path_to_info.len(), set_to_parts.len());

    eprintln!("Fetching relic drop tables (Intact)...");
    let relic_rewards = fetch::fetch_relic_rewards(http.as_ref(), &catalog);
    eprintln!("  {} relics with reward data", relic_rewards.len());

    eprintln!("Fetching prime vault status...");
    let (vault_status, vault_complete) = fetch::fetch_vault_status(http.as_ref(), &catalog, now);
    {
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for v in vault_status.values() {
            *counts.entry(v.as_str()).or_default() += 1;
        }
        eprintln!("  {} slugs tagged · {:?}", vault_status.len(), counts);
    }

    eprintln!("Fetching Baro Ki'Teer schedule...");
    let baro = fetch::fetch_baro(http.as_ref());
    eprintln!("  baro: {}", baro.get("location").and_then(|l| l.as_str()).unwrap_or("unavailable"));

    // ORDERING INVARIANT (see the market.json write below): wfstat-catalog.json
    // is written FIRST, market.json LAST. The two files are each individually
    // atomic (tmp+rename) but the PAIR is not — keep the catalog write ahead of
    // the snapshot write so a reader that catches the gap sees new-catalog +
    // old-market, never the reverse.
    // Reduced from the payload already in hand — no second request.
    let wfstat_slim = wfstat_raw
        .as_ref()
        .and_then(|v| {
            fetch::slim_wfstat_items(v, fetch::WFSTAT_ITEMS_URL)
                .map_err(|e| eprintln!("  warning: {e}"))
                .ok()
        })
        .unwrap_or_default();
    if wfstat_slim.is_empty() && catalog_out.exists() {
        eprintln!("  fetch empty — keeping existing {}", catalog_out.file_name().unwrap_or_default().to_string_lossy());
    } else if !wfstat_slim.is_empty() {
        let tmp = catalog_out.with_extension("json.tmp");
        let slim_json = serde_json::to_string(&wfstat_slim).map_err(|e| format!("serialize: {e}"))?;
        std::fs::create_dir_all(catalog_out.parent().unwrap_or(std::path::Path::new(".")))
            .map_err(|e| format!("mkdir: {e}"))?;
        std::fs::write(&tmp, &slim_json).map_err(|e| format!("write tmp: {e}"))?;
        std::fs::rename(&tmp, &catalog_out).map_err(|e| format!("rename: {e}"))?;
        eprintln!("  {} entries → {}", wfstat_slim.len(), catalog_out.file_name().unwrap_or_default().to_string_lossy());
    }

    let p2i_old: Option<HashMap<String, serde_json::Value>> = prior.get("path_to_info").and_then(|s| serde_json::from_value(s.clone()).ok());
    let s2p_old: Option<HashMap<String, serde_json::Value>> = prior.get("set_to_parts").and_then(|s| serde_json::from_value(s.clone()).ok());
    let rr_old: Option<HashMap<String, serde_json::Value>> = prior.get("relic_rewards").and_then(|s| serde_json::from_value(s.clone()).ok());
    let vs_old: Option<HashMap<String, String>> = prior.get("vault_status").and_then(|s| serde_json::from_value(s.clone()).ok());
    let baro_old: Option<HashMap<String, serde_json::Value>> = prior.get("baro").and_then(|s| serde_json::from_value(s.clone()).ok());
    let rivens_old: Option<HashMap<String, serde_json::Value>> = prior.get("rivens").and_then(|s| serde_json::from_value(s.clone()).ok());
    let calendar_old: Option<HashMap<String, serde_json::Value>> = prior.get("calendar").and_then(|s| serde_json::from_value(s.clone()).ok());

    eprintln!("Fetching prime release/vault dates + Resurgence rotations...");
    let calendar = fetch::fetch_calendar(http.as_ref(), wfstat_raw.as_ref(), &catalog);
    eprintln!(
        "  {} primes dated · {} resurgence rotations",
        calendar.get("primes").and_then(|p| p.as_object()).map(|p| p.len()).unwrap_or(0),
        calendar.get("resurgence").and_then(|r| r.as_array()).map(|r| r.len()).unwrap_or(0)
    );

    eprintln!("Fetching riven dispositions...");
    let rivens = fetch::fetch_rivens(http.as_ref(), rivens_old.as_ref(), now);
    if let Some(ch) = rivens.get("changes").and_then(|c| c.as_array()) {
        let today = ch.iter().filter(|c| c.get("seen_at").and_then(|s| s.as_str()) == Some(clock::iso_z(now).as_str())).count();
        eprintln!("  {} weapons · {} changes in log ({} new this run)",
            rivens.get("weapons").and_then(|w| w.as_object()).map(|w| w.len()).unwrap_or(0), ch.len(), today);
    }

    let r_p2i = reconcile("path_to_info", path_to_info, p2i_old.as_ref(), prior_stamps.get("path_to_info").map(|s| s.as_str()), now, parents_complete, STALE_DAYS);
    let r_s2p = reconcile("set_to_parts", set_to_parts, s2p_old.as_ref(), prior_stamps.get("set_to_parts").map(|s| s.as_str()), now, parents_complete, STALE_DAYS);
    let r_rr = reconcile("relic_rewards", relic_rewards, rr_old.as_ref(), prior_stamps.get("relic_rewards").map(|s| s.as_str()), now, true, STALE_DAYS);
    let r_vs = reconcile("vault_status", vault_status, vs_old.as_ref(), prior_stamps.get("vault_status").map(|s| s.as_str()), now, vault_complete, STALE_DAYS);
    // Before reconcile, not after: reconcile only falls back to the prior value
    // when the whole surface is empty, and Baro's never is (schedule fields keep
    // arriving between visits). His inventory is capturable only during the 48h
    // he is present, so it has to be carried across explicitly.
    let mut baro = baro;
    fetch::carry_baro_inventory(&mut baro, baro_old.as_ref());
    let r_baro = reconcile("baro", baro, baro_old.as_ref(), prior_stamps.get("baro").map(|s| s.as_str()), now, true, STALE_DAYS);
    let r_rivens = reconcile("rivens", rivens, rivens_old.as_ref(), prior_stamps.get("rivens").map(|s| s.as_str()), now, true, STALE_DAYS);
    let r_calendar = reconcile("calendar", calendar, calendar_old.as_ref(), prior_stamps.get("calendar").map(|s| s.as_str()), now, true, STALE_DAYS);

    for r in [&r_p2i, &r_s2p, &r_rr] {
        if let Some(w) = &r.stale_warning {
            eprintln!("{}", w.format());
        }
    }
    if let Some(w) = &r_vs.stale_warning {
        eprintln!("{}", w.format());
    }
    if let Some(w) = &r_baro.stale_warning {
        eprintln!("{}", w.format());
    }
    if let Some(w) = &r_rivens.stale_warning {
        eprintln!("{}", w.format());
    }
    if let Some(w) = &r_calendar.stale_warning {
        eprintln!("{}", w.format());
    }

    let mut surface_fetched_at: HashMap<String, String> = HashMap::new();
    surface_fetched_at.insert("path_to_info".into(), r_p2i.fetched_at.clone());
    surface_fetched_at.insert("set_to_parts".into(), r_s2p.fetched_at.clone());
    surface_fetched_at.insert("relic_rewards".into(), r_rr.fetched_at.clone());
    surface_fetched_at.insert("vault_status".into(), r_vs.fetched_at.clone());
    surface_fetched_at.insert("baro".into(), r_baro.fetched_at.clone());
    surface_fetched_at.insert("rivens".into(), r_rivens.fetched_at.clone());
    surface_fetched_at.insert("calendar".into(), r_calendar.fetched_at.clone());

    eprintln!("Rendering {} CSV rows...", csv_path.display());
    let rows = csvin::read_csv_rows(&csv_path)?;
    let items = render::render_items(&rows, &meta_by_slug);

    let snapshot = assemble_snapshot(
        now,
        catalog,
        items,
        r_p2i.data,
        r_s2p.data,
        r_rr.data,
        r_vs.data,
        r_baro.data,
        r_rivens.data,
        r_calendar.data,
        surface_fetched_at,
    );

    // market.json is written LAST — it's the generation anchor the browser app
    // joins everything through (items[slug], catalog, path_to_info, baro), while
    // wfstat-catalog.json (written above) is only a fallback resolver that
    // resolvePath() consults AFTER market.path_to_info and that the browser
    // caches in IndexedDB for 24h. A torn read of the non-atomic pair is then
    // always new-catalog + old-market (benign: a superset resolver over a
    // self-consistent older snapshot) rather than new-market + old-catalog
    // (which could leave fresh snapshot rows unresolvable until the catalog
    // lands).
    let tmp = json_out.with_extension("json.tmp");
    let json_str = serde_json::to_string(&snapshot).map_err(|e| format!("serialize: {e}"))?;
    let parent = json_out.parent().unwrap_or(std::path::Path::new("."));
    std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
    std::fs::write(&tmp, &json_str).map_err(|e| format!("write tmp: {e}"))?;
    std::fs::rename(&tmp, &json_out).map_err(|e| format!("rename: {e}"))?;
    let meta = std::fs::metadata(&json_out).map_err(|e| format!("stat: {e}"))?;
    eprintln!("Wrote {} ({} bytes)", json_out.display(), meta.len());

    Ok(())
}

fn find_root() -> Result<PathBuf, String> {
    let mut dir = std::env::current_dir().map_err(|e| format!("cwd: {e}"))?;
    loop {
        if dir.join("prototype").join("public").is_dir() && dir.join("wfm_results.csv").exists() {
            return Ok(dir);
        }
        if dir.join(".git").is_dir() && dir.join("prototype").join("public").is_dir() {
            return Ok(dir);
        }
        match dir.parent() {
            Some(p) => dir = p.to_path_buf(),
            None => break,
        }
    }
    Err("Cannot find project root (looked for prototype/public/ + wfm_results.csv)".into())
}
