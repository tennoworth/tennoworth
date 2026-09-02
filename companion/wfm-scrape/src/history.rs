//! Long price history from relics.run - `history.json`, one year of daily
//! (median, volume) per item, beyond WFM's 90-day statistics window.
//!
//! relics.run publishes one file per day since 2021-09-12:
//! `https://relics.run/history/price_history_YYYY-MM-DD.json` (~3.9 MB,
//! keyed by WFM display name; each value is WFM's own per-(rank, subtype)
//! daily statistics rows with `order_type` closed / sell / buy). We keep only
//! the `closed` rows, narrow each item to ONE tier the same way the scraper
//! does (rank 0; one relic refinement, chosen by volume over the whole
//! window rather than per day, so a lone radiant day can't flip a series),
//! and store two dense arrays per slug.
//!
//! The artifact IS the state: the run reads the existing `history.json`,
//! fetches only the days after the last one it holds (normally one), trims
//! to `days`, and rewrites atomically. A missing prior bootstraps up to
//! `bootstrap_days` (a one-off ~1.4 GB pull on the production box). Any single
//! day failing to fetch is a `null` column and a warning, never an abort:
//! history is a bonus surface.

use std::collections::HashMap;

use chrono::{Duration, NaiveDate};
use market_math::{canonical_subtype, drop_poisoned_rows, rank0_rows, subtype_rows, StatsDay};
use serde::{Deserialize, Serialize};

use crate::fetch::Http;

pub const HISTORY_URL_BASE: &str = "https://relics.run/history/price_history_";
/// Series length kept.
pub const DEFAULT_DAYS: usize = 365;
/// Polite spacing between relics.run fetches (a hobbyist mirror, no stated limit).
pub const FETCH_SPACING_MS: u64 = 1000;

/// The on-disk / served shape. `median[i]` / `volume[i]` describe day
/// `start + i`; `null` median = no closed trades that day (or that day's file
/// was unavailable). Arrays, not per-day objects: ~2 M numbers for a year of
/// ~2.7 k items is ~6 MB raw / ~1 MB gzipped, which is the budget.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct History {
    pub generated_at: String,
    /// ISO date of index 0.
    pub start: String,
    pub days: usize,
    /// ISO date of the last day that holds data. The window's nominal end is
    /// `start + days - 1`, but relics.run publishes with lag, so the newest
    /// column(s) can be legitimately empty; the next run resumes from here.
    #[serde(default)]
    pub through: Option<String>,
    pub items: HashMap<String, Series>,
    /// Days in the window whose relics.run file could not be fetched (their
    /// columns are null everywhere) - surfaced so a gap reads as "unavailable",
    /// not "no trades".
    #[serde(default)]
    pub missing_days: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Series {
    pub median: Vec<Option<f64>>,
    pub volume: Vec<u32>,
    /// The subtype tier this series tracks (relic refinement, gem size…),
    /// decided once when the series is first built and kept thereafter - an
    /// incremental day must not re-decide it from one file's worth of trades.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtype: Option<String>,
}

impl History {
    pub fn empty(start: NaiveDate, days: usize, generated_at: &str) -> Self {
        History {
            generated_at: generated_at.into(),
            start: start.to_string(),
            days,
            through: None,
            items: HashMap::new(),
            missing_days: Vec::new(),
        }
    }
    pub fn through_date(&self) -> Option<NaiveDate> {
        self.through.as_deref().and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
    }
    pub fn start_date(&self) -> Option<NaiveDate> {
        NaiveDate::parse_from_str(&self.start, "%Y-%m-%d").ok()
    }
    /// The last calendar day the window covers (inclusive).
    pub fn end_date(&self) -> Option<NaiveDate> {
        self.start_date().map(|s| s + Duration::days(self.days as i64 - 1))
    }
}

/// One item's closed rows for one day, as parsed from a relics.run file.
#[derive(Debug, Clone, Default)]
pub struct DayRows {
    pub rows: Vec<StatsDay>,
}

/// Parse one relics.run daily file into `slug → closed rows`, joining display
/// names to slugs through the WFM catalog (`name_lower → slug`). Names the
/// catalog doesn't know are counted in the returned `unmatched`.
pub fn parse_day_file(
    body: &serde_json::Value,
    name_to_slug: &HashMap<String, String>,
) -> (HashMap<String, DayRows>, usize) {
    let mut out: HashMap<String, DayRows> = HashMap::new();
    let mut unmatched = 0usize;
    let Some(obj) = body.as_object() else { return (out, 0) };
    for (name, rows) in obj {
        let Some(slug) = name_to_slug.get(&name.to_lowercase()) else {
            unmatched += 1;
            continue;
        };
        let Some(arr) = rows.as_array() else { continue };
        let mut day = DayRows::default();
        for r in arr {
            if r.get("order_type").and_then(|t| t.as_str()) != Some("closed") {
                continue;
            }
            let num = |k: &str| r.get(k).and_then(|v| v.as_f64()).unwrap_or(0.0);
            // relics.run mirrors WFM's tri-state: key absent = untiered,
            // present-null = rank-0 tier, number = that rank.
            let mod_rank = match r.get("mod_rank") {
                None => None,
                Some(serde_json::Value::Null) => Some(None),
                Some(v) => Some(v.as_i64()),
            };
            day.rows.push(StatsDay {
                median: num("median"),
                max_price: num("max_price"),
                volume: num("volume"),
                avg_price: num("avg_price"),
                subtype: r.get("subtype").and_then(|s| s.as_str()).map(String::from),
                mod_rank,
            });
        }
        if !day.rows.is_empty() {
            out.insert(slug.clone(), day);
        }
    }
    (out, unmatched)
}

/// Reduce one item's rows for one day to (median, volume) on its canonical
/// tier. `pick` is the item's subtype chosen over the whole window.
pub fn reduce_day(rows: &[StatsDay], pick: Option<&str>) -> (Option<f64>, u32) {
    let tier = drop_poisoned_rows(&subtype_rows(&rank0_rows(rows), pick));
    if tier.is_empty() {
        return (None, 0);
    }
    // Normally exactly one row survives (one rank-0 tier per day). If WFM
    // ever emits two (absent-vs-null mod_rank on the same item), take the
    // busier one for the median and sum the volume.
    let Some(best) = tier
        .iter()
        .max_by(|a, b| a.volume.total_cmp(&b.volume))
    else {
        return (None, 0);
    };
    let vol: f64 = tier.iter().map(|d| d.volume).sum();
    (Some(best.median), vol.round().max(0.0) as u32)
}

/// Fold a day's rows into `hist` at `date`. Grows the window forward when the
/// date is past the current end (dropping the oldest columns) and ignores
/// dates before the start. `picks` is the per-slug subtype decision.
pub fn apply_day(hist: &mut History, date: NaiveDate, day: &HashMap<String, DayRows>, picks: &HashMap<String, Option<String>>) {
    let Some(start) = hist.start_date() else { return };
    let days = hist.days;
    let mut idx = (date - start).num_days();
    if idx < 0 {
        return;
    }
    if idx as usize >= days {
        // Shift the window so `date` is the last column.
        let shift = idx as usize + 1 - days;
        let new_start = start + Duration::days(shift as i64);
        for s in hist.items.values_mut() {
            if shift >= s.median.len() {
                s.median.clear();
                s.volume.clear();
            } else {
                s.median.drain(..shift);
                s.volume.drain(..shift);
            }
            s.median.resize(days, None);
            s.volume.resize(days, 0);
        }
        // Series that slid entirely out of the window carry nothing - drop them.
        hist.items.retain(|_, s| s.median.iter().any(|m| m.is_some()));
        hist.missing_days.retain(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").map(|d| d >= new_start).unwrap_or(false));
        hist.start = new_start.to_string();
        idx = days as i64 - 1;
    }
    let idx = idx as usize;
    if !day.is_empty() && hist.through_date().is_none_or(|t| date > t) {
        hist.through = Some(date.to_string());
    }
    for (slug, rows) in day {
        let s = hist.items.entry(slug.clone()).or_default();
        // An existing series keeps its tier; a new one takes this run's pick.
        if s.subtype.is_none() && s.median.iter().all(|m| m.is_none()) {
            s.subtype = picks.get(slug).cloned().flatten();
        }
        let (median, volume) = reduce_day(&rows.rows, s.subtype.as_deref());
        s.median.resize(days, None);
        s.volume.resize(days, 0);
        if let Some(slot) = s.median.get_mut(idx) {
            *slot = median;
        }
        if let Some(slot) = s.volume.get_mut(idx) {
            *slot = volume;
        }
    }
    // A day can create a series that has no closed trade on its tier; keep the
    // artifact to items with at least one real value.
    hist.items.retain(|_, s| s.median.iter().any(|m| m.is_some()));
}

/// Which subtype each slug's series should track, decided over EVERY day's
/// rows we are about to apply (volume-weighted; prefers intact for relics -
/// `canonical_subtype`'s rule). Items with no subtyped rows get `None`.
pub fn decide_picks(days: &[(NaiveDate, HashMap<String, DayRows>)]) -> HashMap<String, Option<String>> {
    let mut all: HashMap<String, Vec<StatsDay>> = HashMap::new();
    for (_, day) in days {
        for (slug, rows) in day {
            all.entry(slug.clone()).or_default().extend(rows.rows.iter().cloned());
        }
    }
    all.into_iter()
        .map(|(slug, rows)| (slug, canonical_subtype(&rows, |d: &StatsDay| d.volume)))
        .collect()
}

/// The dates to fetch for this run: the day after the last day WITH DATA up to
/// `yesterday` (relics.run publishes a day once it's over, with some lag), or
/// a bootstrap window when there is no prior. Never more than `days` dates.
pub fn dates_to_fetch(prior: Option<&History>, yesterday: NaiveDate, days: usize, bootstrap_days: usize) -> Vec<NaiveDate> {
    let first = match prior.and_then(|p| p.through_date()) {
        Some(through) => through + Duration::days(1),
        None => yesterday - Duration::days(bootstrap_days.min(days).saturating_sub(1) as i64),
    };
    let mut out = Vec::new();
    let mut d = first;
    let floor = yesterday - Duration::days(days as i64 - 1);
    if d < floor {
        d = floor;
    }
    while d <= yesterday {
        out.push(d);
        d += Duration::days(1);
    }
    out
}

pub fn day_url(date: NaiveDate) -> String {
    format!("{HISTORY_URL_BASE}{date}.json")
}

/// What one run did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct HistorySummary {
    pub fetched: usize,
    pub failed: usize,
    pub items: usize,
    pub unmatched_names: usize,
}

/// Run the incremental update: `prior` (the existing artifact, if any) +
/// relics.run → a new [`History`]. `sleep` is injected so tests don't wait.
#[allow(clippy::too_many_arguments)]
pub fn update_history(
    http: &dyn Http,
    prior: Option<History>,
    name_to_slug: &HashMap<String, String>,
    yesterday: NaiveDate,
    generated_at: &str,
    days: usize,
    bootstrap_days: usize,
    sleep: &dyn Fn(u64),
) -> (History, HistorySummary) {
    // A prior with a different window length is rebuilt rather than resized in
    // place (only happens on a config change), so it also doesn't decide what
    // to fetch.
    let usable_prior = prior.filter(|p| p.days == days);
    let dates = dates_to_fetch(usable_prior.as_ref(), yesterday, days, bootstrap_days);
    let mut hist = match usable_prior {
        Some(mut p) => {
            p.generated_at = generated_at.into();
            p
        }
        None => History::empty(yesterday - Duration::days(days as i64 - 1), days, generated_at),
    };
    let mut summary = HistorySummary::default();
    let mut fetched_days: Vec<(NaiveDate, HashMap<String, DayRows>)> = Vec::new();
    let mut failed_dates: Vec<NaiveDate> = Vec::new();
    for (i, date) in dates.iter().enumerate() {
        if i > 0 {
            sleep(FETCH_SPACING_MS);
        }
        match http.get_json(&day_url(*date)) {
            Ok(body) => {
                let (rows, unmatched) = parse_day_file(&body, name_to_slug);
                summary.fetched += 1;
                summary.unmatched_names = summary.unmatched_names.max(unmatched);
                fetched_days.push((*date, rows));
            }
            Err(e) => {
                eprintln!("  warning: history {date}: {e}");
                summary.failed += 1;
                failed_dates.push(*date);
            }
        }
    }
    let picks = decide_picks(&fetched_days);
    for (date, rows) in &fetched_days {
        apply_day(&mut hist, *date, rows, &picks);
    }
    // A failed day BEFORE a later successful one is a real gap: it advances the
    // window (so it isn't retried forever) and is recorded as missing. Failed
    // days at the TAIL are left alone - relics.run publishes a day with some
    // lag, so "yesterday" is often just not there yet; the window end stays at
    // the last success and the next run simply tries again.
    let last_ok = fetched_days.iter().map(|(d, _)| *d).max();
    for date in failed_dates {
        if last_ok.is_some_and(|ok| date < ok) {
            apply_day(&mut hist, date, &HashMap::new(), &picks);
            let s = date.to_string();
            if !hist.missing_days.contains(&s) {
                hist.missing_days.push(s);
            }
        }
    }
    hist.missing_days.sort();
    summary.items = hist.items.len();
    (hist, summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fetch::FixtureHttp;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }
    fn catalog() -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert("primed flow".into(), "primed_flow".into());
        m.insert("lith c5 relic".into(), "lith_c5_relic".into());
        m.insert("volt prime set".into(), "volt_prime_set".into());
        m
    }
    fn closed(median: f64, volume: f64, mod_rank: Option<Option<i64>>, subtype: Option<&str>) -> serde_json::Value {
        let mut o = serde_json::json!({"order_type": "closed", "median": median, "volume": volume, "max_price": median * 1.2, "avg_price": median});
        match mod_rank {
            None => {}
            Some(None) => { o["mod_rank"] = serde_json::Value::Null; }
            Some(Some(r)) => { o["mod_rank"] = serde_json::json!(r); }
        }
        if let Some(s) = subtype {
            o["subtype"] = serde_json::json!(s);
        }
        o
    }
    fn day_body(pf_r0: f64, pf_r10: f64, relic: &[(&str, f64, f64)]) -> serde_json::Value {
        let relic_rows: Vec<serde_json::Value> = relic.iter().map(|(st, m, v)| closed(*m, *v, None, Some(st))).collect();
        serde_json::json!({
            "Primed Flow": [
                closed(pf_r10, 57.0, Some(Some(10)), None),
                closed(pf_r0, 86.0, Some(Some(0)), None),
                {"order_type": "sell", "median": 49, "volume": 4123, "mod_rank": 0},
            ],
            "Lith C5 Relic": relic_rows,
            "Unknown Thing": [closed(1.0, 1.0, None, None)],
        })
    }
    fn http(days: &[(&str, serde_json::Value)]) -> FixtureHttp {
        let mut r = HashMap::new();
        for (date, body) in days {
            r.insert(day_url(d(date)), body.clone());
        }
        FixtureHttp { responses: r }
    }
    fn no_sleep(_: u64) {}

    #[test]
    fn a_day_file_reduces_to_rank0_and_the_volume_dominant_subtype() {
        let (rows, unmatched) = parse_day_file(&day_body(20.0, 80.0, &[("intact", 8.0, 30.0), ("radiant", 40.0, 5.0)]), &catalog());
        assert_eq!(unmatched, 1, "Unknown Thing");
        assert_eq!(rows["primed_flow"].rows.len(), 2, "sell rows dropped");
        let picks = decide_picks(&[(d("2026-08-15"), rows.clone())]);
        assert_eq!(picks["lith_c5_relic"].as_deref(), Some("intact"));
        assert_eq!(picks["primed_flow"], None);
        assert_eq!(reduce_day(&rows["primed_flow"].rows, None), (Some(20.0), 86));
        assert_eq!(reduce_day(&rows["lith_c5_relic"].rows, Some("intact")), (Some(8.0), 30));
    }

    #[test]
    fn bootstrap_then_incremental_append_and_window_shift() {
        // Bootstrap 3 days ending 08-15 into a 3-day window.
        let h = http(&[
            ("2026-08-13", day_body(18.0, 80.0, &[("intact", 7.0, 10.0)])),
            ("2026-08-14", day_body(19.0, 80.0, &[("intact", 8.0, 10.0)])),
            ("2026-08-15", day_body(20.0, 80.0, &[("intact", 9.0, 10.0)])),
        ]);
        let (hist, sum) = update_history(&h, None, &catalog(), d("2026-08-15"), "t0", 3, 3, &no_sleep);
        assert_eq!(sum, HistorySummary { fetched: 3, failed: 0, items: 2, unmatched_names: 1 });
        assert_eq!(hist.start, "2026-08-13");
        assert_eq!(hist.items["primed_flow"].median, vec![Some(18.0), Some(19.0), Some(20.0)]);
        assert_eq!(hist.items["lith_c5_relic"].volume, vec![10, 10, 10]);

        // Next day: only 08-16 is fetched; the window slides, 08-13 falls off.
        let h2 = http(&[("2026-08-16", day_body(21.0, 80.0, &[("intact", 10.0, 10.0)]))]);
        let (hist2, sum2) = update_history(&h2, Some(hist), &catalog(), d("2026-08-16"), "t1", 3, 3, &no_sleep);
        assert_eq!(sum2.fetched, 1);
        assert_eq!(hist2.start, "2026-08-14");
        assert_eq!(hist2.items["primed_flow"].median, vec![Some(19.0), Some(20.0), Some(21.0)]);
        assert_eq!(hist2.generated_at, "t1");
    }

    #[test]
    fn an_incremental_day_keeps_the_series_tier_it_was_built_with() {
        let h = http(&[
            ("2026-08-14", day_body(19.0, 80.0, &[("intact", 8.0, 30.0)])),
            ("2026-08-15", day_body(20.0, 80.0, &[("intact", 9.0, 30.0)])),
        ]);
        let (hist, _) = update_history(&h, None, &catalog(), d("2026-08-15"), "t0", 3, 2, &no_sleep);
        assert_eq!(hist.items["lith_c5_relic"].subtype.as_deref(), Some("intact"));
        // Next day only radiant traded, at 40p - the intact series shows "no
        // trades", not a 40p spike.
        let h2 = http(&[("2026-08-16", day_body(21.0, 80.0, &[("radiant", 40.0, 6.0)]))]);
        let (hist2, _) = update_history(&h2, Some(hist), &catalog(), d("2026-08-16"), "t1", 3, 2, &no_sleep);
        assert_eq!(hist2.items["lith_c5_relic"].subtype.as_deref(), Some("intact"));
        assert_eq!(hist2.items["lith_c5_relic"].median, vec![Some(8.0), Some(9.0), None]);
    }

    #[test]
    fn a_missing_interior_day_is_a_null_column_and_recorded_not_an_abort() {
        let h = http(&[
            ("2026-08-13", day_body(18.0, 80.0, &[])),
            // 08-14 absent from the fixture → fetch error, but 08-15 exists
            ("2026-08-15", day_body(20.0, 80.0, &[])),
        ]);
        let (hist, sum) = update_history(&h, None, &catalog(), d("2026-08-15"), "t", 3, 3, &no_sleep);
        assert_eq!(sum.fetched, 2);
        assert_eq!(sum.failed, 1);
        assert_eq!(hist.items["primed_flow"].median, vec![Some(18.0), None, Some(20.0)]);
        assert_eq!(hist.missing_days, vec!["2026-08-14".to_string()]);
    }

    #[test]
    fn a_not_yet_published_trailing_day_is_retried_next_run_not_recorded() {
        let h = http(&[("2026-08-14", day_body(19.0, 80.0, &[]))]);
        // "yesterday" = 08-15 but relics.run hasn't published it yet
        let (hist, sum) = update_history(&h, None, &catalog(), d("2026-08-15"), "t", 3, 2, &no_sleep);
        assert_eq!((sum.fetched, sum.failed), (1, 1));
        assert!(hist.missing_days.is_empty());
        assert_eq!(hist.through.as_deref(), Some("2026-08-14"));
        // data ends at 08-14 → next run asks for 08-15 again
        assert_eq!(hist.items["primed_flow"].median, vec![None, Some(19.0), None]);
        // (start is yesterday-2 = 08-13, so 08-14 is index 1 and 08-15 is still empty)
        let h2 = http(&[("2026-08-15", day_body(20.0, 80.0, &[]))]);
        let (hist2, _) = update_history(&h2, Some(hist), &catalog(), d("2026-08-15"), "t2", 3, 3, &no_sleep);
        assert_eq!(hist2.items["primed_flow"].median, vec![None, Some(19.0), Some(20.0)]);
    }

    #[test]
    fn items_with_no_closed_trade_on_their_tier_get_no_series() {
        let h = http(&[("2026-08-15", day_body(20.0, 80.0, &[("radiant", 40.0, 6.0)]))]);
        let (hist, _) = update_history(&h, None, &catalog(), d("2026-08-15"), "t", 2, 1, &no_sleep);
        // relic traded only radiant → intact-preferring pick? canonical_subtype
        // prefers intact only when present; here radiant is the sole tier, so
        // it IS the pick and the series exists.
        assert!(hist.items.contains_key("lith_c5_relic"));
        assert!(!hist.items.contains_key("volt_prime_set"), "never in any day file");
    }

    #[test]
    fn nothing_to_fetch_when_already_current() {
        let prior = History { generated_at: "x".into(), start: "2026-08-13".into(), days: 3, through: Some("2026-08-15".into()), items: HashMap::new(), missing_days: vec![] };
        assert!(dates_to_fetch(Some(&prior), d("2026-08-15"), 3, 3).is_empty());
        assert_eq!(dates_to_fetch(Some(&prior), d("2026-08-17"), 3, 3), vec![d("2026-08-16"), d("2026-08-17")]);
        // a very stale prior only refetches what still fits the window
        assert_eq!(dates_to_fetch(Some(&prior), d("2026-09-30"), 3, 3), vec![d("2026-09-28"), d("2026-09-29"), d("2026-09-30")]);
        // bootstrap is capped by the window
        assert_eq!(dates_to_fetch(None, d("2026-08-15"), 3, 365).len(), 3);
    }

    #[test]
    fn a_prior_with_a_different_window_length_is_rebuilt() {
        let prior = History { generated_at: "x".into(), start: "2026-08-13".into(), days: 3, through: Some("2026-08-15".into()), items: HashMap::new(), missing_days: vec![] };
        let h = http(&[("2026-08-15", day_body(20.0, 80.0, &[]))]);
        let (hist, _) = update_history(&h, Some(prior), &catalog(), d("2026-08-15"), "t", 2, 1, &no_sleep);
        assert_eq!(hist.days, 2);
        assert_eq!(hist.start, "2026-08-14");
        assert_eq!(hist.items["primed_flow"].median, vec![None, Some(20.0)]);
    }
}
