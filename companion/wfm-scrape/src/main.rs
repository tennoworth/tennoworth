//! `wfm-scrape` binary — host-only market pipeline.
//!
//! Subcommands:
//! - `build`: mirrors `scripts/csv_to_market_json.py` (phase 3).
//!   Reads `wfm_results.csv`, fetches upstreams, reconciles with the
//!   prior snapshot, and writes `market.json` + `wfstat-catalog.json`.
//! - `scrape`: will mirror `wfm_demand.py` (phase 4, not yet ported).
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
use wfm_scrape::csvin;
use wfm_scrape::fetch::{self, FixtureHttp, Http, LiveHttp};
use wfm_scrape::reconcile::reconcile;
use wfm_scrape::render::{self, assemble_snapshot, CatalogItemMeta};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: wfm-scrape build|scrape [--fixtures-dir <DIR>] [--now <ISO>]");
        std::process::exit(1);
    }
    match args[1].as_str() {
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
            eprintln!("scrape subcommand not yet ported (phase 4)");
            std::process::exit(1);
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
            .user_agent(wfm_client::BROWSER_UA)
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
        return Err(format!("{} not found — run wfm_demand.py first.", csv_path.display()));
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
            let prior_catalog = prior.get("catalog").and_then(|c| c.as_object());
            if prior_catalog.is_none() {
                return Err(format!("{e} — and no prior snapshot to fall back on."));
            }
            eprintln!("  {e} — reusing the prior snapshot's catalog");
            let cat: HashMap<String, String> = prior_catalog.unwrap().iter().map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string())).collect();
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

    eprintln!("Fetching warframestat component path map + sets...");
    let (path_to_info, set_to_parts, parents_complete) = fetch::fetch_parent_data(http.as_ref(), &catalog);
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
    eprintln!("Fetching warframestat bulk item catalog (resolver data)...");
    let wfstat_slim = if fixtures_dir.is_none() {
        fetch::fetch_wfstat_slim().unwrap_or_default()
    } else {
        // In fixture mode, use Http trait (can't use fetch_wfstat_slim's custom client)
        fetch_catalog_slim_via_http(http.as_ref()).unwrap_or_default()
    };
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

    let r_p2i = reconcile("path_to_info", path_to_info, p2i_old.as_ref(), prior_stamps.get("path_to_info").map(|s| s.as_str()), now, parents_complete, 7);
    let r_s2p = reconcile("set_to_parts", set_to_parts, s2p_old.as_ref(), prior_stamps.get("set_to_parts").map(|s| s.as_str()), now, parents_complete, 7);
    let r_rr = reconcile("relic_rewards", relic_rewards, rr_old.as_ref(), prior_stamps.get("relic_rewards").map(|s| s.as_str()), now, true, 7);
    let r_vs = reconcile("vault_status", vault_status, vs_old.as_ref(), prior_stamps.get("vault_status").map(|s| s.as_str()), now, vault_complete, 7);
    let r_baro = reconcile("baro", baro, baro_old.as_ref(), prior_stamps.get("baro").map(|s| s.as_str()), now, true, 7);

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

    let mut surface_fetched_at: HashMap<String, String> = HashMap::new();
    surface_fetched_at.insert("path_to_info".into(), r_p2i.fetched_at.clone());
    surface_fetched_at.insert("set_to_parts".into(), r_s2p.fetched_at.clone());
    surface_fetched_at.insert("relic_rewards".into(), r_rr.fetched_at.clone());
    surface_fetched_at.insert("vault_status".into(), r_vs.fetched_at.clone());
    surface_fetched_at.insert("baro".into(), r_baro.fetched_at.clone());

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
    // lands). Mirrors csv_to_market_json.py's CATALOG_OUT-before-JSON_OUT order.
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

fn fetch_catalog_slim_via_http(http: &dyn Http) -> Result<Vec<serde_json::Value>, String> {
    let url = "https://api.warframestat.us/items/";
    let arr = http.get_json(url)?;
    let items = arr.as_array().ok_or_else(|| format!("{url}: not an array"))?;
    let slim: Vec<serde_json::Value> = items
        .iter()
        .filter(|it| it.get("uniqueName").is_some() && it.get("name").is_some())
        .map(|it| {
            serde_json::json!([it["uniqueName"], {"name": it["name"], "category": it.get("category")}])
        })
        .collect();
    Ok(slim)
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
