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
use wfm_scrape::{de, de_extract};
use wfm_scrape::reconcile::{reconcile, Observation};
use wfm_scrape::render::{self, assemble_snapshot, CatalogItemMeta};

fn valid_compact_usage(rows: &HashMap<String, serde_json::Value>) -> bool {
    !rows.is_empty() && rows.values().all(|row| {
        row.get("name").and_then(|v| v.as_str()).is_some_and(|v| !v.is_empty())
            && row.get("category").and_then(|v| v.as_str()).is_some_and(|v| !v.is_empty())
            && row.get("share").and_then(|v| v.as_f64()).is_some_and(|v| v.is_finite() && v >= 0.0)
            && row.get("by_mr").is_none()
    })
}

fn compact_prior_usage(
    rows: &HashMap<String, serde_json::Value>,
) -> Option<(u16, HashMap<String, serde_json::Value>)> {
    let year = rows.values().next()?.get("year")?.as_u64()? as u16;
    if !de::DE_USAGE_YEARS.contains(&year) {
        return None;
    }
    let mut compact = HashMap::new();
    for (slug, row) in rows {
        if row.get("year").and_then(|v| v.as_u64()) != Some(year as u64) {
            return None;
        }
        let name = row.get("name").and_then(|v| v.as_str())?;
        let category = row.get("category").and_then(|v| v.as_str())?;
        let share = row.get("share").and_then(|v| v.as_f64())?;
        if name.is_empty() || category.is_empty() || !share.is_finite() || share < 0.0 {
            return None;
        }
        compact.insert(slug.clone(), serde_json::json!({
            "name": name,
            "category": category,
            "share": share,
        }));
    }
    valid_compact_usage(&compact).then_some((year, compact))
}

fn rich_prior_usage_year(rows: &HashMap<String, serde_json::Value>) -> Option<u16> {
    let mut common_year = None;
    if rows.is_empty() {
        return None;
    }
    for row in rows.values() {
        let name = row.get("name").and_then(|value| value.as_str())?;
        let category = row.get("category").and_then(|value| value.as_str())?;
        let year = u16::try_from(row.get("year").and_then(|value| value.as_u64())?).ok()?;
        let share = row.get("share").and_then(|value| value.as_f64())?;
        let peak_mr = row.get("peak_mr").and_then(|value| value.as_f64())?;
        let by_mr = row.get("by_mr").and_then(|value| value.as_array())?;
        if name.is_empty()
            || category.is_empty()
            || !de::DE_USAGE_YEARS.contains(&year)
            || !share.is_finite()
            || share < 0.0
            || !peak_mr.is_finite()
            || peak_mr < 0.0
            || by_mr.is_empty()
            || !by_mr.iter().all(|value| {
                value
                    .as_f64()
                    .is_some_and(|value| value.is_finite() && value >= 0.0)
            })
        {
            return None;
        }
        if common_year.replace(year).is_some_and(|prior| prior != year) {
            return None;
        }
    }
    common_year
}

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
            let fixtures_path = fixtures_dir.as_deref().map(std::path::Path::new);
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
    let (catalog, mut meta_by_slug) = match fetch::fetch_catalog_wfm(http.as_ref(), "https://api.warframe.market/v2/items") {
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

    // ---- Digital Extremes first-party ingest -------------------------------
    // Runs after path_to_info because every DE join resolves `/Lotus/...`
    // through it. Costs 490 bytes on a cycle where nothing changed: the export
    // index is content-hashed, so unchanged manifests are skipped entirely.
    let prior_de_hashes: std::collections::BTreeMap<String, String> = prior
        .get("de")
        .and_then(|d| d.get("hashes"))
        .and_then(|h| h.as_object())
        .map(|m| m.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string()))).collect())
        .unwrap_or_default();

    eprintln!("Fetching DE Public Export index...");
    let de_snap = de::fetch_export(http.as_ref(), &prior_de_hashes);
    if de_snap.hashes.is_empty() {
        eprintln!("  unavailable — DE surfaces fall back to the prior snapshot");
    } else {
        // Fetched vs skipped, counted from what we actually hold — the old
        // line derived "skipped" from the changed count, which reported the
        // always-fetch manifests as skipped when they had just been pulled.
        let skipped = de::WANTED_MANIFESTS.iter().filter(|n| de_snap.skipped(n)).count();
        eprintln!(
            "  {} manifests indexed · {} fetched ({}) · {} skipped as unchanged",
            de_snap.hashes.len(),
            de_snap.manifests.len(),
            if de_snap.changed.is_empty() {
                "unchanged but always-fetch".to_string()
            } else {
                de_snap.changed.join(", ")
            },
            skipped,
        );
    }

    eprintln!("Fetching DE worldState...");
    let de_world_observation = de::fetch_world_state(http.as_ref());
    let de_world = match &de_world_observation {
        Observation::Usable { data, .. } => Some(data),
        _ => None,
    };

    // Ducats, first-party. `primeSellingPrice` keys on the recipe (the
    // blueprint you trade), so it resolves through path_to_info the same way
    // relic rewards do. Only applied when the manifest actually came through
    // this cycle — a skipped manifest must not blank a good value.
    // ALWAYS_FETCH guarantees we ATTEMPT this manifest every cycle. It does not
    // guarantee we get it: the index can answer while the manifest itself 500s.
    // Ducats are an override on `meta_by_slug`, which is rebuilt from
    // warframe.market every cycle, so reconcile has nothing to carry and a
    // failed fetch would silently drop the whole catalogue back to WFM's
    // values. The failure path therefore re-applies the last known-good
    // override from the prior snapshot.
    let de_recipes = de_snap.manifests.get("ExportRecipes_en.json");
    // Slugs DE set, this cycle or carried from the last one. Written into the
    // snapshot as provenance so a later failure knows which values were ours.
    let mut de_ducats: std::collections::BTreeMap<String, i64> = std::collections::BTreeMap::new();

    // Built first so the same "did it actually produce anything" test that
    // governs relics governs this too — a manifest can parse and resolve
    // nothing, and treating arrival as success is precisely the mistake that
    // took four review rounds to stop making.
    let fresh_ducats: HashMap<String, i64> = de_recipes
        .map(|recipes| {
            let by_unique = de_extract::ducats_from_recipes(recipes);
            let alias = de_extract::recipe_alias(recipes);
            by_unique
                .iter()
                .filter_map(|(unique, ducats)| {
                    let info = de_extract::resolve_path(unique, &path_to_info, &alias)?;
                    let slug = info.get("slug").and_then(|s| s.as_str())?;
                    Some((slug.to_string(), *ducats))
                })
                .collect()
        })
        .unwrap_or_default();

    if fresh_ducats.is_empty() {
        // Carry ONLY the values DE previously set. Copying every prior ducat
        // would stamp stale numbers over fresh, legitimately-corrected WFM
        // ones — trading a known bug for a subtler one.
        let prior_de_ducats: std::collections::BTreeMap<String, i64> = prior
            .get("de")
            .and_then(|d| d.get("ducats"))
            .and_then(|m| m.as_object())
            .map(|m| m.iter().filter_map(|(k, v)| v.as_i64().map(|n| (k.clone(), n))).collect())
            .unwrap_or_default();
        let mut carried = 0usize;
        for (slug, ducats) in &prior_de_ducats {
            if let Some(meta) = meta_by_slug.get_mut(slug) {
                meta.ducats = Some(*ducats);
                carried += 1;
            }
        }
        de_ducats = prior_de_ducats;
        eprintln!(
            "  ducats: no DE values this cycle — carried {carried} from the prior snapshot"
        );
    } else {
        let mut applied = 0usize;
        let mut disagreed = 0usize;
        for (slug, ducats) in &fresh_ducats {
            if let Some(meta) = meta_by_slug.get_mut(slug) {
                if meta.ducats.is_some_and(|d| d != *ducats && d != 0) {
                    disagreed += 1;
                }
                meta.ducats = Some(*ducats);
                de_ducats.insert(slug.clone(), *ducats);
                applied += 1;
            }
        }
        eprintln!("  ducats: {applied} slugs from DE ({disagreed} disagreed with WFM's value)");
    }

    // Build costs, keyed by the item produced. Only available when the recipes
    // manifest came through this cycle; reconcile preserves it otherwise.
    // An empty map here is correct on a skipped cycle: reconcile carries the
    // prior recipes forward, and there is no legacy source to be tempted by.
    let (recipes_surface, recipe_collisions) = de_recipes
        .map(|r| de_extract::recipes_from_export(r, &path_to_info))
        .unwrap_or_default();
    let recipes_observation = if !recipes_surface.is_empty() {
        Observation::usable(recipes_surface)
    } else {
        match de_snap.outcome("ExportRecipes_en.json") {
            de::ManifestOutcome::Unchanged => Observation::Unchanged,
            de::ManifestOutcome::Unavailable => Observation::Unavailable,
            de::ManifestOutcome::Invalid | de::ManifestOutcome::Usable => Observation::Invalid,
        }
    };
    if let Observation::Usable { data: recipes, .. } = &recipes_observation {
        eprintln!("  recipes: {} buildable items costed", recipes.len());
        if recipe_collisions > 0 {
            eprintln!(
                "  warning: {recipe_collisions} recipes collided on an already-taken slug \
                 (first kept) — path_to_info is not one-to-one"
            );
        }
    }

    // Annual usage history is immutable. Keep every valid prior year, request
    // only missing published candidates, and leave failed years absent so the
    // next cycle retries them. DE publishes these files in arrears; never
    // manufacture a current-year candidate.
    let usage_old: Option<HashMap<String, serde_json::Value>> =
        prior.get("usage").and_then(|s| serde_json::from_value(s.clone()).ok());
    let mut usage_history: render::UsageHistorySurface = prior
        .get("usage_history")
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .unwrap_or_default();
    usage_history.by_year.retain(|year, rows| {
        de::DE_USAGE_YEARS.contains(year) && valid_compact_usage(rows)
    });
    if let Some((year, compact)) = usage_old.as_ref().and_then(compact_prior_usage) {
        usage_history.by_year.entry(year).or_insert(compact);
    }

    let prior_usage_year = usage_old
        .as_ref()
        .and_then(rich_prior_usage_year);
    let rich_repair_year = usage_history
        .by_year
        .keys()
        .next_back()
        .copied()
        .filter(|year| prior_usage_year != Some(*year));
    let mut fresh_rich_usage = std::collections::BTreeMap::new();
    for year in de::DE_USAGE_YEARS {
        let has_compact = usage_history.by_year.contains_key(year);
        if has_compact && rich_repair_year != Some(*year) {
            eprintln!("Usage telemetry: {year} already in the snapshot — not refetched");
            continue;
        }
        if has_compact {
            eprintln!("Fetching DE usage telemetry ({year}) to repair rich usage...");
        } else {
            eprintln!("Fetching DE usage telemetry ({year})...");
        }
        match http.get_json(&de::usage_url(*year)) {
            Ok(doc) => {
                let (compact, accepted, unmatched) =
                    de_extract::usage_history_from_export(&doc, &catalog);
                eprintln!(
                    "  {year}: {} joined · {unmatched} unmatched · {accepted} valid rows",
                    compact.len()
                );
                if valid_compact_usage(&compact) {
                    let (rich, _) = de_extract::usage_from_export(&doc, *year, &catalog);
                    if !has_compact {
                        usage_history.by_year.insert(*year, compact);
                    }
                    fresh_rich_usage.insert(*year, rich);
                } else {
                    eprintln!("  warning: {year} usage shape had no joinable valid rows");
                }
            }
            Err(e) => eprintln!("  warning: {year}: {e}"),
        }
    }
    usage_history.years = usage_history.by_year.keys().copied().collect();
    let newest_usage_year = usage_history.years.last().copied().unwrap_or(0);
    let usage_observation = if let Some(rich) = fresh_rich_usage.remove(&newest_usage_year) {
        Observation::usable(rich)
    } else if prior_usage_year == Some(newest_usage_year) {
        Observation::Unchanged
    } else {
        Observation::Unavailable
    };

    // Relic rewards. DE's table beats the drop-table scrape on two counts: all
    // four refinements instead of intact only, and correct rarity labels (the
    // old source calls 25.33% drops "Uncommon").
    //
    // The three cases below are NOT interchangeable, and conflating two of
    // them was a real bug: when the manifest is skipped as unchanged, falling
    // back to the legacy scrape overwrites a good four-refinement surface with
    // an intact-only one on every warm cycle. A skip must emit EMPTY so
    // reconcile carries the DE-derived surface forward; only an unreachable DE
    // justifies the fallback.
    //
    // The fallback fires ONLY when DE is unreachable outright. Any other
    // shortfall — a skipped manifest, or one that failed to fetch or parse —
    // emits EMPTY so reconcile carries the DE-derived surface forward.
    // Reaching for the legacy source in those cases produces a NON-EMPTY
    // intact-only table, and a non-empty surface is exactly what stops
    // reconcile preserving the good one.
    // Relic rewards, as ONE policy rather than a chain of special cases.
    //
    // Four rounds of review found four adjacent holes here, each because a
    // condition covered the state it was written for and not its neighbours.
    // The inputs are: did DE's manifests arrive, did they yield rows, and is
    // there a prior surface to fall back on. Those collapse to three outcomes,
    // in strict priority:
    //
    //   1. Fresh DE rows          → publish them.
    //   2. Otherwise, prior rows  → publish EMPTY, so reconcile carries them.
    //   3. Otherwise              → the legacy scrape; something beats nothing.
    //
    // The invariant that ties it together: **never publish an empty relic
    // surface while any source could produce one.** Empty is only ever a
    // deliberate instruction to reconcile, never an outcome.
    //
    // Note that "DE's manifests arrived" is not the same as "DE produced
    // rows": a manifest can parse cleanly and still resolve nothing usable,
    // which is the case that slipped through the previous fix.
    let prior_has_relics = prior
        .get("relic_rewards")
        .and_then(|r| r.as_object())
        .is_some_and(|r| !r.is_empty());

    let fresh_relics = match (
        de_snap.manifests.get("ExportRelicArcane_en.json"),
        de_recipes,
    ) {
        (Some(relics), Some(recipes)) => {
            eprintln!("Building relic tables from DE Public Export...");
            let build = de_extract::relic_rewards_from_de(relics, recipes, &path_to_info);
            eprintln!(
                "  {} relics · {} reward refs resolved · {} unresolved{}",
                build.rewards.len(),
                build.resolved,
                build.unresolved,
                if build.samples.is_empty() {
                    String::new()
                } else {
                    format!(" (e.g. {})", build.samples.join(", "))
                }
            );
            build.rewards
        }
        _ => HashMap::new(),
    };

    let (relic_observation, relic_source) = if !fresh_relics.is_empty() {
        (Observation::usable(fresh_relics), "de_public_export")
    } else if prior_has_relics {
        eprintln!("Relic tables not rebuilt this cycle — carrying the prior DE surface");
        let state = match (
            de_snap.outcome("ExportRelicArcane_en.json"),
            de_snap.outcome("ExportRecipes_en.json"),
        ) {
            (de::ManifestOutcome::Invalid, _) | (_, de::ManifestOutcome::Invalid) => Observation::Invalid,
            (de::ManifestOutcome::Unavailable, _) | (_, de::ManifestOutcome::Unavailable) => Observation::Unavailable,
            (de::ManifestOutcome::Unchanged, de::ManifestOutcome::Usable | de::ManifestOutcome::Unchanged) => Observation::Unchanged,
            _ => Observation::Invalid,
        };
        (state, "de_public_export")
    } else {
        // No DE rows and nothing to carry. The old drop-table scrape stays as
        // the fallback rather than being deleted: it is the only other source
        // of a relic table, and losing relics entirely would be a worse
        // regression than intact-only odds. It is off the happy path, so its
        // rarity mislabelling and intact-only coverage stop being what users
        // normally see — and it can only ever overwrite a DE surface when
        // there is no DE surface to protect.
        eprintln!("Fetching relic drop tables (fallback — no DE data available)...");
        let r = fetch::fetch_relic_rewards(http.as_ref(), &catalog);
        eprintln!("  {} relics with reward data (intact only)", r.len());
        if r.is_empty() {
            (Observation::Unavailable, "legacy_drop_table")
        } else {
            (Observation::usable(r), "legacy_drop_table")
        }
    };

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
    // worldState carries the manifest from ANNOUNCEMENT, so his stock is known
    // days before he lands — the old source returned an empty list between
    // visits and published no schedule at all.
    let de_alias = de_recipes.map(de_extract::recipe_alias).unwrap_or_default();
    let de_baro = de_world
        .as_ref()
        .map(|w| de_extract::baro_from_world(w, &path_to_info, &de_alias, |ms| clock::iso_z(clock::from_millis(ms))))
        .filter(|b| !b.is_empty());
    let (baro, baro_source) = de_baro.map(|b| (b, "de_world_state")).unwrap_or_else(|| {
            eprintln!("  worldState had no trader — falling back to warframestat");
            (fetch::fetch_baro(http.as_ref()), "warframestat")
        });
    eprintln!(
        "  baro: {} · {} items",
        baro.get("location").and_then(|l| l.as_str()).unwrap_or("unavailable"),
        baro.get("inventory").and_then(|i| i.as_array()).map(|a| a.len()).unwrap_or(0)
    );

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
    let riven_stats_old: Option<HashMap<String, serde_json::Value>> = prior.get("riven_stats").and_then(|s| serde_json::from_value(s.clone()).ok());
    let recipes_old: Option<HashMap<String, serde_json::Value>> = prior.get("recipes").and_then(|s| serde_json::from_value(s.clone()).ok());
    let prior_de = prior.get("de");
    let vault_rotation_old: Option<Vec<serde_json::Value>> = prior_de
        .and_then(|d| d.get("vault_rotation"))
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    let deals_old: Option<Vec<serde_json::Value>> = prior_de
        .and_then(|d| d.get("deals"))
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    let goals_old: Option<std::collections::BTreeMap<String, serde_json::Value>> = prior
        .get("event_rewards").and_then(|v| v.get("goals"))
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    let events_old: Option<std::collections::BTreeMap<String, serde_json::Value>> = prior
        .get("event_rewards").and_then(|v| v.get("events"))
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    let prior_child_stamps: std::collections::BTreeMap<String, String> = prior_de
        .and_then(|d| d.get("child_fetched_at"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    eprintln!("Fetching prime release/vault dates + Resurgence rotations...");
    let calendar = fetch::fetch_calendar(http.as_ref(), wfstat_raw.as_ref(), &catalog);
    eprintln!(
        "  {} primes dated · {} resurgence rotations",
        calendar.get("primes").and_then(|p| p.as_object()).map(|p| p.len()).unwrap_or(0),
        calendar.get("resurgence").and_then(|r| r.as_array()).map(|r| r.len()).unwrap_or(0)
    );

    eprintln!("Fetching riven dispositions...");
    let mut rivens = fetch::fetch_rivens(http.as_ref());
    // DE is the authority on dispositions; warframe.market mirrors them and
    // lags. Overriding here (rather than replacing the fetch) keeps WFM's
    // group/riven_type/req_mr metadata, which DE does not publish, and lets
    // the existing 90-day change log diff against the authoritative value.
    // Applied only when the weapons manifest actually came through this cycle.
    // Same rule again: a weapons manifest that parses but yields no
    // dispositions must not silently leave every value at warframe.market's
    // lagging mirror. When there is nothing fresh, re-apply the prior
    // snapshot's — which were DE's — so the override survives.
    let de_dispos = de_snap
        .manifests
        .get("ExportWeapons_en.json")
        .map(de_extract::dispositions_from_weapons)
        .unwrap_or_default();
    let prior_de_dispositions: std::collections::BTreeMap<String, f64> = prior
        .get("de")
        .and_then(|d| d.get("dispositions"))
        .and_then(|m| m.as_object())
        .map(|m| m.iter().filter_map(|(k, v)| v.as_f64().map(|n| (k.clone(), n))).collect())
        .unwrap_or_default();
    let mut joined_dispositions = std::collections::BTreeMap::new();
    if let Some(map) = rivens.get("weapons").and_then(|w| w.as_object()) {
        for (slug, row) in map {
            let Some(name) = row.get("name").and_then(|n| n.as_str()).map(|n| n.to_lowercase()) else { continue };
            if let Some(value) = de_dispos.get(&name) {
                joined_dispositions.insert(slug.clone(), *value);
            }
        }
    }
    let prior_coverage_ok = prior_de_dispositions.is_empty()
        || joined_dispositions.len() * 100 >= prior_de_dispositions.len() * 80;
    let joined_usable = !de_dispos.is_empty()
        && !joined_dispositions.is_empty()
        && joined_dispositions.len() * 100 >= de_dispos.len() * 80
        && prior_coverage_ok;
    let disposition_state = match de_snap.outcome("ExportWeapons_en.json") {
        de::ManifestOutcome::Usable if joined_usable => wfm_scrape::reconcile::Disposition::PublishedFresh,
        de::ManifestOutcome::Usable | de::ManifestOutcome::Invalid => wfm_scrape::reconcile::Disposition::PreservedInvalid,
        de::ManifestOutcome::Unchanged => wfm_scrape::reconcile::Disposition::PreservedUnchanged,
        de::ManifestOutcome::Unavailable => wfm_scrape::reconcile::Disposition::PreservedUnavailable,
    };
    if !joined_usable && !de_dispos.is_empty() {
        eprintln!("  warning: {}/{} DE dispositions joined (prior exact set {}) — preserving prior exact provenance", joined_dispositions.len(), de_dispos.len(), prior_de_dispositions.len());
    }
    let mut de_dispositions = if joined_usable { joined_dispositions } else { std::collections::BTreeMap::new() };
    if !joined_usable {
        let mut carried = 0usize;
        if let Some(map) = rivens.get_mut("weapons").and_then(|w| w.as_object_mut()) {
            for (slug, prior_dispo) in &prior_de_dispositions {
                if let Some(row) = map.get_mut(slug) {
                    if let Some(obj) = row.as_object_mut() {
                        obj.insert("disposition".into(), serde_json::json!(*prior_dispo));
                        de_dispositions.insert(slug.clone(), *prior_dispo);
                        carried += 1;
                    }
                }
            }
        }
        eprintln!("  dispositions: none from DE this cycle — carried {carried} from the prior snapshot");
    } else {
        let mut moved = 0usize;
        if let Some(map) = rivens.get_mut("weapons").and_then(|w| w.as_object_mut()) {
            for (slug, de_dispo) in &de_dispositions {
                let Some(row) = map.get_mut(slug) else { continue };
                let was = row.get("disposition").and_then(|d| d.as_f64());
                if was != Some(*de_dispo) {
                    moved += 1;
                }
                if let Some(obj) = row.as_object_mut() {
                    obj.insert("disposition".into(), serde_json::json!(de_dispo));
                }
            }
        }
        eprintln!("  dispositions: {} matched to DE ({moved} differed from WFM's mirror)", de_dispositions.len());
    }
    // The change log LAST, diffing what we are about to publish against what we
    // published before. Computing it inside the fetch made it diff WFM's
    // mirror against DE's stored values, which logged a phantom change every
    // time the mirror lagged and missed every real one, because the override
    // lands after the fetch.
    if let Some(weapons) = rivens.get("weapons").and_then(|w| w.as_object()).cloned() {
        let changes = fetch::riven_change_log(&weapons, rivens_old.as_ref(), now);
        if !changes.is_empty() {
            rivens.insert("changes".into(), serde_json::Value::Array(changes));
        }
    }
    let rivens = rivens;
    if let Some(ch) = rivens.get("changes").and_then(|c| c.as_array()) {
        let today = ch.iter().filter(|c| c.get("seen_at").and_then(|s| s.as_str()) == Some(clock::iso_z(now).as_str())).count();
        eprintln!("  {} weapons · {} changes in log ({} new this run)",
            rivens.get("weapons").and_then(|w| w.as_object()).map(|w| w.len()).unwrap_or(0), ch.len(), today);
    }

    eprintln!("Fetching DE weekly riven stats...");
    // DE names the weapons by display name; the riven-weapons manifest already
    // fetched above maps those to slugs — no second request.
    let weapons_by_name: HashMap<String, String> = rivens
        .get("weapons")
        .and_then(|w| w.as_object())
        .map(|w| {
            w.iter()
                .filter_map(|(slug, row)| {
                    row.get("name")
                        .and_then(|n| n.as_str())
                        .map(|n| (n.to_lowercase(), slug.clone()))
                })
                .collect()
        })
        .unwrap_or_default();
    let (mut riven_stats, unmatched_stats, riven_stats_children) =
        fetch::fetch_riven_stats(http.as_ref(), &weapons_by_name);
    let pc_riven_stats_state = riven_stats_children
        .get("pc")
        .copied()
        .unwrap_or(fetch::RivenChildOutcome::Unavailable);
    if pc_riven_stats_state == fetch::RivenChildOutcome::Usable {
        fetch::carry_failed_riven_platforms(&mut riven_stats, riven_stats_old.as_ref(), &riven_stats_children);
    }
    eprintln!("  {} weapons · {unmatched_stats} DE rows without a WFM slug", riven_stats.len());

    let path_to_info_for_de = path_to_info.clone();
    let observation = |data: HashMap<String, serde_json::Value>, complete| {
        if data.is_empty() { Observation::Unavailable } else if complete { Observation::usable(data) } else { Observation::partial(data) }
    };
    let r_p2i = reconcile("path_to_info", observation(path_to_info, parents_complete), p2i_old.as_ref(), prior_stamps.get("path_to_info").map(|s| s.as_str()), now, STALE_DAYS);
    let r_s2p = reconcile("set_to_parts", observation(set_to_parts, parents_complete), s2p_old.as_ref(), prior_stamps.get("set_to_parts").map(|s| s.as_str()), now, STALE_DAYS);
    let r_rr = reconcile("relic_rewards", relic_observation, rr_old.as_ref(), prior_stamps.get("relic_rewards").map(|s| s.as_str()), now, STALE_DAYS);
    let vault_observation = if vault_status.is_empty() { Observation::Unavailable } else if vault_complete { Observation::usable(vault_status) } else { Observation::partial(vault_status) };
    let r_vs = reconcile("vault_status", vault_observation, vs_old.as_ref(), prior_stamps.get("vault_status").map(|s| s.as_str()), now, STALE_DAYS);
    // Before reconcile, not after: reconcile only falls back to the prior value
    // when the whole surface is empty, and Baro's never is (schedule fields keep
    // arriving between visits). His inventory is capturable only during the 48h
    // he is present, so it has to be carried across explicitly.
    let mut baro = baro;
    fetch::carry_baro_inventory(&mut baro, baro_old.as_ref());
    let baro_observation = if baro.is_empty() { Observation::Unavailable } else { Observation::usable(baro) };
    let r_baro = reconcile("baro", baro_observation, baro_old.as_ref(), prior_stamps.get("baro").map(|s| s.as_str()), now, STALE_DAYS);
    let r_rivens = reconcile("rivens", observation(rivens, true), rivens_old.as_ref(), prior_stamps.get("rivens").map(|s| s.as_str()), now, STALE_DAYS);
    let r_calendar = reconcile("calendar", observation(calendar, true), calendar_old.as_ref(), prior_stamps.get("calendar").map(|s| s.as_str()), now, STALE_DAYS);
    let riven_stats_observation = match pc_riven_stats_state {
        fetch::RivenChildOutcome::Unavailable => Observation::Unavailable,
        fetch::RivenChildOutcome::Invalid => Observation::Invalid,
        fetch::RivenChildOutcome::AuthoritativeEmpty => Observation::AuthoritativeEmpty,
        fetch::RivenChildOutcome::Usable => Observation::usable(riven_stats),
    };
    let r_riven_stats = reconcile("riven_stats", riven_stats_observation, riven_stats_old.as_ref(), prior_stamps.get("riven_stats").map(|s| s.as_str()), now, STALE_DAYS);
    let r_recipes = reconcile("recipes", recipes_observation, recipes_old.as_ref(), prior_stamps.get("recipes").map(|s| s.as_str()), now, STALE_DAYS);
    let r_usage = reconcile("usage", usage_observation, usage_old.as_ref(), prior_stamps.get("usage").map(|s| s.as_str()), now, STALE_DAYS);
    let fresh_vault_rotation = de_world
        .as_ref()
        .map(|w| de_extract::vault_rotation_from_world(w, |ms| clock::iso_z(clock::from_millis(ms))))
        .unwrap_or_default();
    let fresh_deals = de_world
        .as_ref()
        .map(|w| de_extract::deals_from_world(w, &path_to_info_for_de, &de_alias, |ms| clock::iso_z(clock::from_millis(ms))))
        .unwrap_or_default();
    let world_vault_observation = match &de_world_observation {
        Observation::Unavailable => Observation::Unavailable,
        Observation::Invalid => Observation::Invalid,
        Observation::Usable { .. } => de::world_array_observation(de_world, "PrimeVaultTraders", fresh_vault_rotation),
        Observation::Unchanged | Observation::AuthoritativeEmpty => Observation::Invalid,
    };
    let world_deals_observation = match &de_world_observation {
        Observation::Unavailable => Observation::Unavailable,
        Observation::Invalid => Observation::Invalid,
        Observation::Usable { .. } => de::world_array_observation(de_world, "DailyDeals", fresh_deals),
        Observation::Unchanged | Observation::AuthoritativeEmpty => Observation::Invalid,
    };
    let r_world_vault = reconcile(
        "world.vault_rotation",
        world_vault_observation,
        vault_rotation_old.as_ref(),
        prior_child_stamps.get("world.vault_rotation").map(|s| s.as_str()),
        now,
        STALE_DAYS,
    );
    let r_world_deals = reconcile(
        "world.deals",
        world_deals_observation,
        deals_old.as_ref(),
        prior_child_stamps.get("world.deals").map(|s| s.as_str()),
        now,
        STALE_DAYS,
    );
    let goal_build = de_world
        .map(|world| de_extract::event_rewards_from_world_child(
            world, "Goals", &path_to_info_for_de, &de_alias,
            |ms| clock::iso_z(clock::from_millis(ms)),
        ))
        .unwrap_or_default();
    let event_build = de_world
        .map(|world| de_extract::event_rewards_from_world_child(
            world, "Events", &path_to_info_for_de, &de_alias,
            |ms| clock::iso_z(clock::from_millis(ms)),
        ))
        .unwrap_or_default();
    let child_observation = |key: &str, build: de_extract::EventRewardBuild| {
        match &de_world_observation {
            Observation::Unavailable => Observation::Unavailable,
            Observation::Invalid => Observation::Invalid,
            Observation::Usable { data: world, .. } => match world.get(key).and_then(|v| v.as_array()) {
                None => Observation::Invalid,
                Some(rows) if rows.is_empty() => Observation::AuthoritativeEmpty,
                Some(_) if build.rows.is_empty() => Observation::Invalid,
                // A child is the freshness unit. Stamping retained rows fresh
                // after any malformed sibling would overstate what this poll
                // established, so mixed payloads preserve the whole prior.
                Some(_) if build.invalid_rows > 0 => Observation::Invalid,
                Some(_) => Observation::usable(build.rows),
            },
            Observation::Unchanged | Observation::AuthoritativeEmpty => Observation::Invalid,
        }
    };
    let r_world_goals = reconcile(
        "world.goals", child_observation("Goals", goal_build), goals_old.as_ref(),
        prior_child_stamps.get("world.goals").map(|s| s.as_str()), now, STALE_DAYS,
    );
    let r_world_events = reconcile(
        "world.events", child_observation("Events", event_build), events_old.as_ref(),
        prior_child_stamps.get("world.events").map(|s| s.as_str()), now, STALE_DAYS,
    );

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
    if let Some(w) = &r_riven_stats.stale_warning {
        eprintln!("{}", w.format());
    }
    if let Some(w) = &r_recipes.stale_warning {
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
    surface_fetched_at.insert("riven_stats".into(), r_riven_stats.fetched_at.clone());
    surface_fetched_at.insert("recipes".into(), r_recipes.fetched_at.clone());
    surface_fetched_at.insert("usage".into(), r_usage.fetched_at.clone());

    let mut surface_provenance = HashMap::new();
    macro_rules! provenance {
        ($name:literal, $result:expr) => {
            surface_provenance.insert(
                $name.to_string(),
                render::SurfaceProvenance {
                    disposition: $result.disposition,
                    attempted_at: $result.attempted_at.clone(),
                    data_fetched_at: $result.fetched_at.clone(),
                    source: None,
                },
            );
        };
    }
    provenance!("path_to_info", r_p2i);
    provenance!("set_to_parts", r_s2p);
    provenance!("relic_rewards", r_rr);
    provenance!("vault_status", r_vs);
    provenance!("baro", r_baro);
    provenance!("rivens", r_rivens);
    provenance!("calendar", r_calendar);
    provenance!("riven_stats", r_riven_stats);
    provenance!("recipes", r_recipes);
    provenance!("usage", r_usage);
    provenance!("world.vault_rotation", r_world_vault);
    provenance!("world.deals", r_world_deals);
    provenance!("world.goals", r_world_goals);
    provenance!("world.events", r_world_events);
    if let Some(p) = surface_provenance.get_mut("relic_rewards") {
        p.source = Some(relic_source.to_string());
    }
    if let Some(p) = surface_provenance.get_mut("baro") {
        p.source = Some(baro_source.to_string());
    }
    let prior_disposition_stamp = prior_child_stamps
        .get("de.dispositions")
        .cloned()
        .unwrap_or_else(|| clock::iso_z(now));
    let disposition_fetched_at = if disposition_state == wfm_scrape::reconcile::Disposition::PublishedFresh {
        clock::iso_z(now)
    } else {
        prior_disposition_stamp
    };
    surface_provenance.insert("de.dispositions".into(), render::SurfaceProvenance {
        disposition: disposition_state,
        attempted_at: clock::iso_z(now),
        data_fetched_at: disposition_fetched_at.clone(),
        source: Some("de_public_export".into()),
    });

    eprintln!("Rendering {} CSV rows...", csv_path.display());
    let rows = csvin::read_csv_rows(&csv_path)?;
    let items = render::render_items(&rows, &meta_by_slug);

    // The DE provenance block. `hashes` is what makes the next cycle cheap —
    // it is compared against the fresh index so unchanged manifests are never
    // refetched. Carried forward verbatim when DE was unreachable, so an
    // outage does not force a full re-download on recovery.
    //
    // The worldState-derived rows are CARRIED on an outage rather than
    // emptied. This block is assigned after `assemble_snapshot`, so it never
    // passes through reconcile and nothing else would preserve it — a single
    // failed poll would silently drop an announced vault rotation, which is
    // exactly the event the feature exists to warn about. `world_ok: false`
    // tells the UI the rows are stale.
    let de_surface = render::DeSurface {
        hashes: if de_snap.hashes.is_empty() {
            prior_de_hashes.clone()
        } else {
            de_snap.hashes.clone()
        },
        changed: de_snap.changed.clone(),
        world_ok: matches!(de_world_observation, Observation::Usable { .. }),
        child_fetched_at: {
            let mut stamps = prior_child_stamps;
            stamps.insert("world.vault_rotation".into(), r_world_vault.fetched_at.clone());
            stamps.insert("world.deals".into(), r_world_deals.fetched_at.clone());
            stamps.insert("world.goals".into(), r_world_goals.fetched_at.clone());
            stamps.insert("world.events".into(), r_world_events.fetched_at.clone());
            stamps.insert("de.dispositions".into(), disposition_fetched_at);
            for (child, outcome) in &riven_stats_children {
                if matches!(outcome, fetch::RivenChildOutcome::Usable | fetch::RivenChildOutcome::AuthoritativeEmpty) {
                    stamps.insert(format!("riven_stats.{child}"), clock::iso_z(now));
                }
            }
            stamps
        },
        vault_rotation: r_world_vault.data,
        deals: r_world_deals.data,
        ducats: de_ducats,
        dispositions: de_dispositions,
    };

    let mut snapshot = assemble_snapshot(
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
        r_riven_stats.data,
        r_recipes.data,
        r_usage.data,
        usage_history,
        surface_fetched_at,
    );
    snapshot.de = Some(de_surface);
    snapshot.surface_provenance = surface_provenance;
    snapshot.event_rewards = render::EventRewardsSurface {
        goals: r_world_goals.data,
        events: r_world_events.data,
    };

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
