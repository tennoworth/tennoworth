//! Scrape orchestration — the `wfm_demand.py` loop.
//!
//! Fetch the master item list, filter/exclude/limit it, then per item fetch the
//! order book + closed stats (paced + retried through [`crate::http`]), compose
//! the row entirely from [`market_math`] heuristics, gate on `--min-volume`,
//! and write the CSV atomically. The scoring/filtering math lives in
//! `market-math`; this module only wires it together.

use std::path::{Path, PathBuf};

use serde_json::Value;

use market_math::{
    avg_lowest_asks, canonical_subtype, clamp_low5, demand_ratio, drop_poisoned_rows, low_sell,
    rank0_orders, rank0_rows, round_dp, score, series_stats, spread, subtype_rows, top_buy,
    weighted_avg_48h, StatsDay,
};

use crate::coerce::{Coercions, DEFAULT_MAX_COERCIONS};
use crate::http::{fetch_json, ScrapeHttp, Sleeper, REQUEST_DELAY};
use crate::orders::{live_orders, parse_orders};
use crate::stats::parse_stats;

const API_ROOT: &str = "https://api.warframe.market";

/// The CSV column order — the keys of `analyze_item`'s dict, in insertion
/// order, which is also what `csvin::CsvRow` reads back.
const HEADER: [&str; 19] = [
    "url_name", "name", "tags", "ducats", "live_buys", "live_sells", "buy_sell_ratio",
    "top_buy_price", "low_sell_price", "low5_avg", "spread", "volume_48h", "avg_price_48h",
    "median_now", "median_90d", "medians_7d", "donch_top_90d", "donch_bot_90d", "score",
];

/// A master-catalog item — the fields `analyze_item` reads off the raw
/// `/v2/items` entry (slug, display name, tags, ducats).
#[derive(Debug, Clone)]
pub struct CatalogItem {
    pub slug: String,
    pub name: String,
    pub tags: Vec<String>,
    pub ducats: Option<i64>,
}

/// One analyzed row, in the exact shape `analyze_item` returns.
#[derive(Debug, Clone)]
pub struct AnalyzedRow {
    pub url_name: String,
    pub name: String,
    pub tags: Vec<String>,
    pub ducats: Option<i64>,
    pub live_buys: i64,
    pub live_sells: i64,
    pub buy_sell_ratio: f64,
    pub top_buy_price: f64,
    pub low_sell_price: f64,
    pub low5_avg: f64,
    pub spread: f64,
    pub volume_48h: f64,
    pub avg_price_48h: f64,
    pub median_now: f64,
    pub median_90d: f64,
    pub medians_7d: Vec<f64>,
    pub donch_top_90d: f64,
    pub donch_bot_90d: f64,
    pub score: f64,
}

/// Parse the `/v2/items` master list into [`CatalogItem`]s. Entries without a
/// slug are skipped (Python indexes `item["slug"]`, so a slug is mandatory).
pub fn parse_items(items: &Value) -> Vec<CatalogItem> {
    let arr = match items.as_array() {
        Some(a) => a,
        None => return vec![],
    };
    let mut out = Vec::with_capacity(arr.len());
    for it in arr {
        let slug = match it.get("slug").and_then(|s| s.as_str()) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => continue,
        };
        out.push(CatalogItem {
            name: item_name(it, &slug),
            slug,
            tags: it
                .get("tags")
                .and_then(|t| t.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                .unwrap_or_default(),
            ducats: it.get("ducats").and_then(|d| d.as_i64()),
        });
    }
    out
}

/// Python's `_item_name`: `i18n.en.name`, else the slug.
fn item_name(it: &Value, slug: &str) -> String {
    it.get("i18n")
        .and_then(|i| i.get("en"))
        .and_then(|e| e.get("name"))
        .and_then(|n| n.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(slug)
        .to_string()
}

/// JSON truthiness — Python's `not stats_payload` test (an empty object is
/// falsy, so a stats payload with no content skips the item).
fn json_truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// Analyze one item into a row, or `Ok(None)` to skip it (missing orders or an
/// empty stats payload — Python's `if orders is None or not stats_payload`).
/// A hard coercion error (object/bool/non-finite field) propagates as `Err`.
pub fn analyze_item(
    item: &CatalogItem,
    orders: Option<Value>,
    stats: Option<Value>,
    co: &mut Coercions,
) -> Result<Option<AnalyzedRow>, String> {
    let orders = match orders {
        Some(v) => v,
        None => return Ok(None),
    };
    let stats = match stats {
        Some(v) if json_truthy(&v) => v,
        _ => return Ok(None),
    };

    let (recent_all, nineties_all) = parse_stats(&stats, &item.slug, co)?;

    // The single tier both windows and the live book are narrowed to, computed
    // from the raw (pre-filter) stats — rank 0 for mods, one subtype for relics.
    let mut combined = recent_all.clone();
    combined.extend(nineties_all.clone());
    let sub_pick = canonical_subtype(&combined, |d: &StatsDay| d.volume);
    let pick = sub_pick.as_deref();

    let parsed = parse_orders(&orders, &item.slug, co)?;
    let live_buys = rank0_orders(&subtype_rows(&live_orders(&parsed, "buy"), pick));
    let live_sells = rank0_orders(&subtype_rows(&live_orders(&parsed, "sell"), pick));

    let recent = drop_poisoned_rows(&subtype_rows(&rank0_rows(&recent_all), pick));
    let nineties = drop_poisoned_rows(&subtype_rows(&rank0_rows(&nineties_all), pick));

    let (median_now, median_90d, medians_7d, donch_top, donch_bot) = series_stats(&nineties);

    let volume_48h: f64 = recent.iter().map(|d| d.volume).sum();
    let avg_price_48h = weighted_avg_48h(&recent, median_90d);

    let top = top_buy(&live_buys);
    let low = low_sell(&live_sells);
    let low5 = clamp_low5(avg_lowest_asks(&live_sells, 5), median_90d, low);
    let ratio = demand_ratio(live_buys.len(), live_sells.len());
    let sc = score(volume_48h, avg_price_48h, ratio);

    Ok(Some(AnalyzedRow {
        url_name: item.slug.clone(),
        name: item.name.clone(),
        tags: item.tags.clone(),
        ducats: item.ducats,
        live_buys: live_buys.len() as i64,
        live_sells: live_sells.len() as i64,
        buy_sell_ratio: round_dp(ratio, 2),
        top_buy_price: top,
        low_sell_price: low,
        low5_avg: low5,
        spread: spread(low, top),
        volume_48h,
        avg_price_48h: round_dp(avg_price_48h, 1),
        median_now: round_dp(median_now, 1),
        median_90d: round_dp(median_90d, 1),
        medians_7d,
        donch_top_90d: donch_top,
        donch_bot_90d: donch_bot,
        score: round_dp(sc, 1),
    }))
}

/// Format a numeric CSV cell. Rust's `Display` already drops a trailing `.0`
/// (8.0 → "8"), matching the int/float split Python's `csv` writer produces;
/// the parity comparator parses numerically regardless.
fn fmt_num(x: f64) -> String {
    format!("{x}")
}

/// Python `str()` of a list of strings: `['a', 'b']`, `[]` when empty.
fn fmt_str_list(items: &[String]) -> String {
    let inner: Vec<String> = items.iter().map(|s| format!("'{s}'")).collect();
    format!("[{}]", inner.join(", "))
}

/// Python `str()` of a numeric list: `[33, 36, 42]`, `[]` when empty.
fn fmt_num_list(items: &[f64]) -> String {
    let inner: Vec<String> = items.iter().map(|x| fmt_num(*x)).collect();
    format!("[{}]", inner.join(", "))
}

impl AnalyzedRow {
    fn to_fields(&self) -> Vec<String> {
        vec![
            self.url_name.clone(),
            self.name.clone(),
            fmt_str_list(&self.tags),
            self.ducats.map(|d| d.to_string()).unwrap_or_default(),
            self.live_buys.to_string(),
            self.live_sells.to_string(),
            fmt_num(self.buy_sell_ratio),
            fmt_num(self.top_buy_price),
            fmt_num(self.low_sell_price),
            fmt_num(self.low5_avg),
            fmt_num(self.spread),
            fmt_num(self.volume_48h),
            fmt_num(self.avg_price_48h),
            fmt_num(self.median_now),
            fmt_num(self.median_90d),
            fmt_num_list(&self.medians_7d),
            fmt_num(self.donch_top_90d),
            fmt_num(self.donch_bot_90d),
            fmt_num(self.score),
        ]
    }
}

/// Serialize rows to CSV bytes, sorted by score descending (stable, so score
/// ties keep insertion order — Python's `sorted(..., reverse=True)`).
pub fn rows_to_csv(rows: &[AnalyzedRow]) -> Result<Vec<u8>, String> {
    let mut sorted: Vec<&AnalyzedRow> = rows.iter().collect();
    sorted.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    let mut wtr = csv::WriterBuilder::new().from_writer(Vec::new());
    wtr.write_record(HEADER).map_err(|e| format!("CSV header: {e}"))?;
    for r in &sorted {
        wtr.write_record(r.to_fields()).map_err(|e| format!("CSV row: {e}"))?;
    }
    wtr.into_inner().map_err(|e| format!("CSV flush: {e}"))
}

/// Write the CSV atomically to `path` — `path.tmp` then rename, Python's
/// `os.replace`. Callers pass either the mid-run `<out>.partial` checkpoint path
/// or the final `--out` path; each write is torn-file-free on its own path.
/// Empty results write nothing (Python's `write_snapshot` returns early), so a
/// throttled run never overwrites a healthy CSV with a header-only file.
pub fn write_csv(rows: &[AnalyzedRow], path: &Path) -> Result<(), String> {
    if rows.is_empty() {
        return Ok(());
    }
    let data = rows_to_csv(rows)?;
    let tmp = PathBuf::from(format!("{}.tmp", path.display()));
    std::fs::write(&tmp, &data).map_err(|e| format!("write {tmp:?}: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("rename {tmp:?} → {path:?}: {e}"))?;
    Ok(())
}

/// Scrape configuration — the CLI flags, with `wfm_demand.py`'s argparse
/// defaults.
#[derive(Debug, Clone)]
pub struct ScrapeConfig {
    pub filter: String,
    pub exclude: String,
    pub platform: String,
    pub limit: usize,
    pub min_volume: i64,
    pub out: PathBuf,
    pub checkpoint_every: usize,
    pub max_coercions: u64,
}

impl Default for ScrapeConfig {
    fn default() -> Self {
        ScrapeConfig {
            filter: "prime".into(),
            exclude: "set".into(),
            platform: "pc".into(),
            limit: 0,
            min_volume: 5,
            out: PathBuf::from("wfm_results.csv"),
            checkpoint_every: 100,
            max_coercions: DEFAULT_MAX_COERCIONS,
        }
    }
}

/// Outcome of a scrape run.
#[derive(Debug, Clone)]
pub struct ScrapeSummary {
    pub scanned: usize,
    pub kept: usize,
    pub coercions: u64,
}

/// Run the full scrape. `http` and `sleeper` are injected so fixture mode and
/// tests never touch the network or sleep.
pub fn run_scrape(
    http: &dyn ScrapeHttp,
    sleeper: &dyn Sleeper,
    cfg: &ScrapeConfig,
) -> Result<ScrapeSummary, String> {
    let items_val = fetch_json(http, sleeper, &format!("{API_ROOT}/v2/items"))
        .ok_or_else(|| "Failed to fetch item list. Network problem?".to_string())?;
    let mut items = parse_items(&items_val);
    if items.is_empty() {
        return Err("Failed to fetch item list. Network problem?".to_string());
    }

    // Empty filter/exclude are no-ops (Python guards them with `if args.filter`).
    if !cfg.filter.is_empty() {
        let f = cfg.filter.to_lowercase();
        items.retain(|i| i.slug.to_lowercase().contains(&f));
    }
    if !cfg.exclude.is_empty() {
        let x = cfg.exclude.to_lowercase();
        items.retain(|i| !i.slug.to_lowercase().contains(&x));
    }
    if cfg.limit > 0 {
        items.truncate(cfg.limit);
    }

    let total = items.len();
    let mut results: Vec<AnalyzedRow> = Vec::new();
    let mut co = Coercions::new();

    // CHECKPOINT SAFETY — a DELIBERATE DIVERGENCE from Python: Python's
    // checkpoints atomic-replace `--out` itself mid-run, so an abort past a
    // checkpoint leaves `--out` as a partial file. Here checkpoints write to
    // `<out>.partial` and `--out` is replaced exactly ONCE, at successful
    // completion. On ANY failure `--out` is untouched and `<out>.partial`
    // survives for post-mortem. Strictly safer; run-scrape.sh's row-count floor
    // still gates on the completed `--out`, so a truncated `.partial` can never
    // masquerade as a finished snapshot.
    let partial = PathBuf::from(format!("{}.partial", cfg.out.display()));

    for (idx, item) in items.iter().enumerate() {
        let orders = fetch_json(http, sleeper, &format!("{API_ROOT}/v2/orders/item/{}", item.slug));
        sleeper.sleep(REQUEST_DELAY);
        let stats = fetch_json(http, sleeper, &format!("{API_ROOT}/v1/items/{}/statistics", item.slug));
        sleeper.sleep(REQUEST_DELAY);

        let row = analyze_item(item, orders, stats, &mut co)?;

        // The permissive-parsing budget, enforced INCREMENTALLY (Python only
        // checks equivalently at the end). A systemic upstream shape drift (WFM
        // sending numeric fields as strings) trips the budget within the first
        // fraction of items; aborting here — before the volume gate, the
        // checkpoint write, or any further fetch — fails the run loudly and
        // promptly, leaving `--out` untouched, rather than promoting a
        // silently-reshaped snapshot.
        if co.exceeds(cfg.max_coercions) {
            return Err(format!(
                "aborting: {} numeric-string coercions exceed the budget of {} — WFM field types look drifted",
                co.count, cfg.max_coercions
            ));
        }

        if let Some(row) = row {
            if row.volume_48h >= cfg.min_volume as f64 {
                results.push(row);
            }
        }

        if cfg.checkpoint_every > 0 && (idx + 1) % cfg.checkpoint_every == 0 {
            write_csv(&results, &partial)?;
        }
    }

    // The single, final replacement of `--out` — reached only when every item
    // was scanned without tripping the coercion budget.
    if !results.is_empty() {
        write_csv(&results, &cfg.out)?;
    }

    Ok(ScrapeSummary {
        scanned: total,
        kept: results.len(),
        coercions: co.count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{FixtureScrapeHttp, NoopSleeper, RecordingSleeper};
    use serde_json::json;
    use std::collections::HashMap;

    fn ing(kind: &str, plat: i64) -> Value {
        json!({"type": kind, "platinum": plat, "visible": true, "user": {"status": "ingame"}})
    }

    fn stats_days(window: &str, days: Value) -> Value {
        json!({"payload": {"statistics_closed": {window: days}}})
    }

    // A small two-item catalog + per-item orders/stats, all always-200.
    fn fixture() -> FixtureScrapeHttp {
        let mut r: HashMap<String, Value> = HashMap::new();
        r.insert(
            format!("{API_ROOT}/v2/items"),
            json!({"data": [
                {"slug": "volt_prime_barrel", "i18n": {"en": {"name": "Volt Prime Barrel"}}, "tags": ["prime"], "ducats": 45},
                {"slug": "thin_item", "i18n": {"en": {"name": "Thin Item"}}, "tags": [], "ducats": null}
            ]}),
        );
        // Healthy item: buys+sells, 8 vol over 48h.
        r.insert(
            format!("{API_ROOT}/v2/orders/item/volt_prime_barrel"),
            json!({"data": [ing("buy", 20), ing("buy", 18), ing("sell", 25), ing("sell", 27)]}),
        );
        r.insert(
            format!("{API_ROOT}/v1/items/volt_prime_barrel/statistics"),
            stats_days("48hours", json!([{"median": 26, "max_price": 30, "volume": 8, "avg_price": 26.0}])),
        );
        // Thin item: only 0 volume in 48h → dropped by min_volume gate.
        r.insert(
            format!("{API_ROOT}/v2/orders/item/thin_item"),
            json!({"data": []}),
        );
        r.insert(
            format!("{API_ROOT}/v1/items/thin_item/statistics"),
            stats_days("48hours", json!([{"median": 5, "volume": 0, "avg_price": 5.0}])),
        );
        FixtureScrapeHttp::new(r)
    }

    fn cfg(out: &Path) -> ScrapeConfig {
        ScrapeConfig {
            filter: String::new(),
            exclude: String::new(),
            min_volume: 1,
            out: out.to_path_buf(),
            checkpoint_every: 0,
            ..Default::default()
        }
    }

    #[test]
    fn run_keeps_only_items_over_min_volume_and_writes_csv() {
        let dir = std::env::temp_dir().join(format!("wfmscrape_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("run.csv");
        let summary = run_scrape(&fixture(), &NoopSleeper, &cfg(&out)).unwrap();
        assert_eq!(summary.scanned, 2);
        assert_eq!(summary.kept, 1); // thin_item dropped at the volume gate
        let text = std::fs::read_to_string(&out).unwrap();
        assert!(text.starts_with(&HEADER.join(",")));
        assert!(text.contains("volt_prime_barrel"));
        assert!(!text.contains("thin_item"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn paces_two_request_delays_per_item_and_no_retry_sleeps_on_clean_fixtures() {
        // Orchestration-level pacing evidence: exactly two REQUEST_DELAY pauses
        // per scanned item (one after the order fetch, one after stats). The
        // always-200 base fixtures never trigger a retry/backoff sleep, so the
        // schedule is entirely those fixed spacings.
        let dir = std::env::temp_dir().join(format!("wfmscrape_pace_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("run.csv");
        let sl = RecordingSleeper::new();
        let summary = run_scrape(&fixture(), &sl, &cfg(&out)).unwrap();
        let sleeps = sl.recorded();
        assert_eq!(sleeps.len(), summary.scanned * 2, "two spacings per scanned item");
        assert!(sleeps.iter().all(|d| *d == REQUEST_DELAY), "every sleep is the fixed spacing");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn checkpoints_hit_partial_not_out_until_completion() {
        // NEW SEMANTICS: mid-run checkpoints land on `<out>.partial`; `--out`
        // itself is written exactly once, at completion. Generate >checkpoint
        // items so a checkpoint fires, then assert `.partial` exists (proof it
        // fired) and `--out` carries the FULL result set (not the checkpoint).
        let n = 105usize;
        let mut r: HashMap<String, Value> = HashMap::new();
        let mut items = Vec::new();
        for i in 0..n {
            let slug = format!("item_{i:04}");
            items.push(json!({"slug": slug, "i18n": {"en": {"name": slug}}, "tags": [], "ducats": null}));
            r.insert(
                format!("{API_ROOT}/v2/orders/item/{slug}"),
                json!({"data": [ing("buy", 20), ing("sell", 25)]}),
            );
            r.insert(
                format!("{API_ROOT}/v1/items/{slug}/statistics"),
                stats_days("48hours", json!([{"median": 25, "max_price": 30, "volume": 5, "avg_price": 25.0}])),
            );
        }
        r.insert(format!("{API_ROOT}/v2/items"), json!({"data": items}));

        let dir = std::env::temp_dir().join(format!("wfmscrape_ckpt_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("run.csv");
        // Default checkpoint_every (100) fires once at item 100.
        let c = ScrapeConfig {
            filter: String::new(),
            exclude: String::new(),
            min_volume: 1,
            out: out.clone(),
            ..Default::default()
        };
        let summary = run_scrape(&FixtureScrapeHttp::new(r), &NoopSleeper, &c).unwrap();
        assert_eq!(summary.kept, n);

        let partial = out.with_extension("csv.partial");
        assert!(partial.exists(), "a checkpoint must have written <out>.partial");
        let partial_rows = std::fs::read_to_string(&partial).unwrap().lines().count() - 1; // minus header
        assert_eq!(partial_rows, 100, "the checkpoint captured the first 100 items");

        let out_rows = std::fs::read_to_string(&out).unwrap().lines().count() - 1;
        assert_eq!(out_rows, n, "--out holds the full result set, written once at completion");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn abort_leaves_out_untouched_and_partial_for_post_mortem() {
        // A coercion overflow mid-run must NOT replace `--out`; the last
        // `.partial` checkpoint stays behind for diagnosis.
        let n = 150usize;
        let mut r: HashMap<String, Value> = HashMap::new();
        let mut items = Vec::new();
        for i in 0..n {
            let slug = format!("drift_{i:04}");
            items.push(json!({"slug": slug, "i18n": {"en": {"name": slug}}}));
            r.insert(format!("{API_ROOT}/v2/orders/item/{slug}"), json!({"data": []}));
            // Numeric-string fields — every item adds coercions, so the budget
            // trips well before the scan finishes.
            r.insert(
                format!("{API_ROOT}/v1/items/{slug}/statistics"),
                stats_days("48hours", json!([{"median": "5", "volume": "5", "avg_price": "5", "max_price": "9"}])),
            );
        }
        r.insert(format!("{API_ROOT}/v2/items"), json!({"data": items}));

        let dir = std::env::temp_dir().join(format!("wfmscrape_abort_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("run.csv");
        // Budget 500 with 4 coercions/item trips at item 126 — AFTER the
        // checkpoint at item 100 has already written `.partial`.
        let c = ScrapeConfig {
            filter: String::new(),
            exclude: String::new(),
            min_volume: 1,
            out: out.clone(),
            max_coercions: 500,
            ..Default::default() // checkpoint_every 100
        };
        let err = run_scrape(&FixtureScrapeHttp::new(r), &NoopSleeper, &c).unwrap_err();
        assert!(err.contains("coercions exceed the budget"), "{err}");
        assert!(!out.exists(), "--out is never replaced on an aborted run");
        assert!(out.with_extension("csv.partial").exists(), ".partial checkpoint survives for post-mortem");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_stats_url_skips_the_item_but_run_completes() {
        // Truncated/partial behavior: an item whose stats never arrive is
        // skipped (fetch_json → None), the run finishes with the survivors.
        let mut fx = fixture();
        fx.responses.remove(&format!("{API_ROOT}/v1/items/volt_prime_barrel/statistics"));
        let dir = std::env::temp_dir().join(format!("wfmscrape_trunc_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("run.csv");
        let summary = run_scrape(&fx, &NoopSleeper, &cfg(&out)).unwrap();
        assert_eq!(summary.scanned, 2);
        assert_eq!(summary.kept, 0); // both items now drop out
        assert!(!out.exists()); // empty results → no CSV promoted
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn filter_and_exclude_narrow_the_item_set() {
        let mut items = parse_items(&json!([
            {"slug": "volt_prime_set", "i18n": {"en": {"name": "Volt Prime Set"}}},
            {"slug": "volt_prime_barrel", "i18n": {"en": {"name": "Volt Prime Barrel"}}},
            {"slug": "rubico", "i18n": {"en": {"name": "Rubico"}}}
        ]));
        // filter "prime"
        items.retain(|i| i.slug.to_lowercase().contains("prime"));
        assert_eq!(items.len(), 2);
        // exclude "set"
        items.retain(|i| !i.slug.to_lowercase().contains("set"));
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].slug, "volt_prime_barrel");
    }

    #[test]
    fn coercion_budget_overflow_fails_the_run() {
        // Feed numeric-string fields and a budget of 1 → the run aborts.
        let mut r: HashMap<String, Value> = HashMap::new();
        r.insert(
            format!("{API_ROOT}/v2/items"),
            json!({"data": [{"slug": "s", "i18n": {"en": {"name": "S"}}}]}),
        );
        r.insert(format!("{API_ROOT}/v2/orders/item/s"), json!({"data": []}));
        r.insert(
            format!("{API_ROOT}/v1/items/s/statistics"),
            stats_days("48hours", json!([{"median": "1", "volume": "2", "avg_price": "3", "max_price": "4"}])),
        );
        let http = FixtureScrapeHttp::new(r);
        let dir = std::env::temp_dir().join(format!("wfmscrape_coerce_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut c = cfg(&dir.join("run.csv"));
        c.max_coercions = 1;
        let err = run_scrape(&http, &NoopSleeper, &c).unwrap_err();
        assert!(err.contains("coercions exceed the budget"), "{err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn object_valued_field_aborts_with_field_path() {
        let item = CatalogItem { slug: "goopolla".into(), name: "Goopolla".into(), tags: vec![], ducats: None };
        // analyze_item receives the already-unwrapped payload (fetch_json strips
        // the `payload` envelope before handing it over).
        let stats = json!({"statistics_closed": {"90days": [{"median": {"x": 1}}]}});
        let mut co = Coercions::new();
        let err = analyze_item(&item, Some(json!([])), Some(stats), &mut co).unwrap_err();
        assert!(err.contains("goopolla.90days[0].median"), "{err}");
    }

    #[test]
    fn skips_item_when_orders_missing_or_stats_empty() {
        let item = CatalogItem { slug: "s".into(), name: "S".into(), tags: vec![], ducats: None };
        let mut co = Coercions::new();
        // orders None
        assert!(analyze_item(&item, None, Some(json!({"statistics_closed": {}})), &mut co).unwrap().is_none());
        // stats empty object (falsy)
        assert!(analyze_item(&item, Some(json!([])), Some(json!({})), &mut co).unwrap().is_none());
    }

    #[test]
    fn csv_cells_format_lists_and_none_ducats() {
        let row = AnalyzedRow {
            url_name: "x".into(),
            name: "X".into(),
            tags: vec!["mod".into(), "prime".into()],
            ducats: None,
            live_buys: 2,
            live_sells: 3,
            buy_sell_ratio: 0.67,
            top_buy_price: 35.0,
            low_sell_price: 42.0,
            low5_avg: 42.6,
            spread: 7.0,
            volume_48h: 8.0,
            avg_price_48h: 26.0,
            median_now: 26.0,
            median_90d: 25.0,
            medians_7d: vec![25.0, 26.0],
            donch_top_90d: 30.0,
            donch_bot_90d: 20.0,
            score: 100.0,
        };
        let f = row.to_fields();
        assert_eq!(f[2], "['mod', 'prime']"); // tags
        assert_eq!(f[3], ""); // ducats None → empty
        assert_eq!(f[7], "35"); // integral float → no trailing .0
        assert_eq!(f[15], "[25, 26]"); // medians_7d
    }
}
