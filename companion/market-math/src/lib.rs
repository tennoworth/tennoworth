//! Pure market-data heuristics, ported 1:1 from `wfm_demand.py` +
//! `scripts/csv_to_market_json.py` (phase 2 of the Python→Rust pipeline
//! consolidation — see the rust-consolidation-plan memory / repo docs).
//!
//! RULES FOR THIS CRATE:
//! - No I/O, no HTTP, no clocks, no dependencies. Pure functions only —
//!   that's what lets every function be pinned against the frozen Python
//!   test fixtures (tests/test_wfm_demand.py) and, later, lets the
//!   wfm-scrape binary be validated by semantic snapshot diffing.
//! - Where Python semantics are quirky (ties-to-even rounding, upper-middle
//!   baseline, insertion-order tie-breaks), we preserve them DELIBERATELY
//!   and say so at the site. Divergence is a decision, not an accident.

/// Wash-trade cap: WFM clamps prices at 99,999p; a daily median or max at
/// (near) the cap is manipulation, not trade.
pub const PLAT_CAP: f64 = 99_999.0;
/// A day > 50× the cleaned baseline is almost certainly faked…
pub const OUTLIER_FACTOR: f64 = 50.0;
/// …but only when the outlier is also expensive in absolute terms. A real
/// balance-patch repricing of a 1p junk item to 60p is >50× yet honest;
/// wash-trade pumps worth doing are large (Mawfish printed 300p on a 2p fish).
pub const OUTLIER_ABS_MIN: f64 = 200.0;

/// One day-row from WFM's closed-trade statistics.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StatsDay {
    pub median: f64,
    pub max_price: f64,
    pub volume: f64,
    pub avg_price: f64,
    pub subtype: Option<String>,
    /// Tri-state, faithfully mirroring the Python dict semantics:
    /// - `None`          — key absent: an untiered item (weapon/set/relic)
    /// - `Some(None)`    — key present but null: counts as rank-0 AND marks
    ///                     the item as tiered (Python: `"mod_rank" in d` is
    ///                     true, `(d.get("mod_rank") or 0) == 0` keeps it)
    /// - `Some(Some(n))` — a real rank tier
    pub mod_rank: Option<Option<i64>>,
}

/// One live order from WFM's v2 order book.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LiveOrder {
    pub platinum: f64,
    /// v2 sends the key with null on untiered items, so absent-vs-null
    /// collapse into `None` here — any `Some(rank)` marks the item tiered.
    pub rank: Option<i64>,
    pub subtype: Option<String>,
}

/// Anything that can be narrowed to one subtype tier (stats days and live
/// orders both carry a subtype; Python reuses one function for both).
pub trait HasSubtype {
    fn subtype(&self) -> Option<&str>;
}
impl HasSubtype for StatsDay {
    fn subtype(&self) -> Option<&str> {
        self.subtype.as_deref()
    }
}
impl HasSubtype for LiveOrder {
    fn subtype(&self) -> Option<&str> {
        self.subtype.as_deref()
    }
}

/// Keep the unranked tier of a closed-stats list.
///
/// WFM emits one row per (day, mod_rank) for mods — an unranked AND a
/// max-rank tier. Baro sells mods unranked and that's the tier players
/// resell, so stats must describe rank 0. Items without rank tiers lack the
/// key entirely, so ABSENCE OF METADATA is the only correct fallback
/// condition: an empty result here means every trade in the window was
/// max-rank, and the honest output is "no rank-0 activity" (vol 0), not
/// max-rank prices wearing a rank-0 label.
pub fn rank0_rows(rows: &[StatsDay]) -> Vec<StatsDay> {
    if rows.iter().all(|d| d.mod_rank.is_none()) {
        return rows.to_vec();
    }
    rows.iter()
        .filter(|d| matches!(d.mod_rank, Some(None) | Some(Some(0)) | None))
        .cloned()
        .collect()
}

/// Keep the unranked tier of a live order list — the order-book counterpart
/// of [`rank0_rows`], with the same honest empty fallback. (tempo_royale: a
/// rank-3 ask at 30p under a 35p rank-0 book read as "sell at 30";
/// arcane_velocity: a rank-5 buy at 160p sat next to a 7p rank-0 price.)
pub fn rank0_orders(orders: &[LiveOrder]) -> Vec<LiveOrder> {
    if orders.iter().all(|o| o.rank.is_none()) {
        return orders.to_vec();
    }
    orders
        .iter()
        .filter(|o| o.rank.unwrap_or(0) == 0)
        .cloned()
        .collect()
}

/// The single subtype tier this item's stats should describe, or `None`.
///
/// Relics have the same per-(day, subtype) duality mods have with rank: one
/// radiant day at 120p blending into 5-14p intact days fabricated a +991%
/// trend on meso_l1_relic. Prefer `intact`; for items whose subtypes don't
/// include intact (gems, fish), use the dominant-by-volume tier.
///
/// Tie-break preserved from Python: `max(dict, key=…)` returns the
/// FIRST-INSERTED max, so equal-volume tiers resolve to the one seen first.
pub fn canonical_subtype<T: HasSubtype>(rows: &[T], volume_of: impl Fn(&T) -> f64) -> Option<String> {
    let mut vols: Vec<(String, f64)> = Vec::new();
    for r in rows {
        if let Some(s) = r.subtype() {
            match vols.iter_mut().find(|(k, _)| k == s) {
                Some((_, v)) => *v += volume_of(r),
                None => vols.push((s.to_string(), volume_of(r))),
            }
        }
    }
    if vols.is_empty() {
        return None;
    }
    if vols.iter().any(|(k, _)| k == "intact") {
        return Some("intact".to_string());
    }
    let mut best = &vols[0];
    for kv in &vols[1..] {
        if kv.1 > best.1 {
            best = kv; // strictly greater → first-seen wins ties, like Python
        }
    }
    Some(best.0.clone())
}

/// Filter a list to one subtype tier (`None` pick = no-op). Rows without a
/// subtype are generic — keep them.
pub fn subtype_rows<T: HasSubtype + Clone>(rows: &[T], pick: Option<&str>) -> Vec<T> {
    match pick {
        None => rows.to_vec(),
        Some(p) => rows
            .iter()
            .filter(|r| r.subtype().unwrap_or(p) == p)
            .cloned()
            .collect(),
    }
}

/// Average of the n cheapest live asks — a depth-aware "current price".
///
/// `low_sell` alone is ONE number any account can set for free (a 1p troll
/// listing); the mean of the cheapest five is what the sell wall actually
/// looks like. 0.0 when there are no live asks. Rounded to 1 decimal with
/// ties-to-even, matching Python's `round(x, 1)`.
pub fn avg_lowest_asks(orders: &[LiveOrder], n: usize) -> f64 {
    let mut prices: Vec<f64> = orders.iter().map(|o| o.platinum).filter(|&p| p > 0.0).collect();
    prices.sort_by(f64::total_cmp);
    prices.truncate(n);
    if prices.is_empty() {
        return 0.0;
    }
    let mean = prices.iter().sum::<f64>() / prices.len() as f64;
    (mean * 10.0).round_ties_even() / 10.0
}

/// Python's `statistics.median`: sort, odd length → middle, even length →
/// mean of the two middles.
fn stat_median(values: &[f64]) -> f64 {
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n == 0 {
        return 0.0;
    }
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

/// `(median_now, median_90d, medians_7d, donch_top, donch_bot)` from an
/// already tier-narrowed, poison-filtered 90-day series.
///
/// median_now = the latest day's median — "what it trades at today".
/// median_90d = the median OF the daily medians — the 90-day baseline.
/// The band is recomputed from the filtered daily medians, not WFM's
/// precomputed donch values — those still reflect a cap-pinned day even
/// after we drop it from the series.
pub fn series_stats(nineties: &[StatsDay]) -> (f64, f64, Vec<f64>, f64, f64) {
    let daily: Vec<f64> = nineties.iter().map(|d| d.median).collect();
    let medians_7d: Vec<f64> = daily.iter().rev().take(7).rev().copied().collect();
    if nineties.is_empty() {
        return (0.0, 0.0, medians_7d, 0.0, 0.0);
    }
    let median_now = nineties.last().map(|d| d.median).unwrap_or(0.0);
    let nonzero: Vec<f64> = daily.iter().copied().filter(|&m| m > 0.0).collect();
    let median_90d = if nonzero.is_empty() { median_now } else { stat_median(&nonzero) };
    let band: &[f64] = if nonzero.is_empty() { &[median_now] } else { &nonzero };
    let top = band.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let bot = band.iter().copied().fold(f64::INFINITY, f64::min);
    (median_now, median_90d, medians_7d, top, bot)
}

/// Strip wash-trade / cap-pinned daily rows from a closed-stats list.
///
/// When the cap filter removes EVERYTHING, the item's only activity is
/// manipulation — return the empty list so volume sums to 0 (an earlier
/// `or rows` fallback handed the fabricated 99,999p average back in exactly
/// that case). Baseline preserved from Python: `meds[len // 2]` — the UPPER
/// middle of the sorted positive medians, not statistics.median.
pub fn drop_poisoned_rows(rows: &[StatsDay]) -> Vec<StatsDay> {
    let clean: Vec<StatsDay> = rows
        .iter()
        .filter(|d| d.median < PLAT_CAP * 0.9 && d.max_price < PLAT_CAP * 0.9)
        .cloned()
        .collect();
    let mut meds: Vec<f64> = clean.iter().map(|d| d.median).filter(|&m| m > 0.0).collect();
    meds.sort_by(f64::total_cmp);
    if let Some(&baseline) = meds.get(meds.len() / 2) {
        if baseline > 0.0 {
            return clean
                .into_iter()
                .filter(|d| d.median <= baseline * OUTLIER_FACTOR || d.median < OUTLIER_ABS_MIN)
                .collect();
        }
    }
    clean
}

/// 48h volume-weighted average with the outlier clamp. `vol` stays the
/// honest closed-trade count elsewhere, but a single fat-finger/scam sale on
/// a thin item otherwise sets `avg` arbitrarily (deimos_heart_scene: one
/// 500p sale against a 9p 90d median read avg=500). Weight only day-rows
/// whose avg_price sits within 3× of the 90d baseline median; if every trade
/// in the window was an outlier, fall back to that median.
pub fn weighted_avg_48h(recent: &[StatsDay], median_90d: f64) -> f64 {
    let sane: Vec<&StatsDay> = if median_90d > 0.0 {
        recent
            .iter()
            .filter(|d| d.avg_price >= median_90d / 3.0 && d.avg_price <= median_90d * 3.0)
            .collect()
    } else {
        recent.iter().collect()
    };
    let sane_vol: f64 = sane.iter().map(|d| d.volume).sum();
    if sane_vol > 0.0 {
        sane.iter().map(|d| d.avg_price * d.volume).sum::<f64>() / sane_vol
    } else if median_90d > 0.0 {
        median_90d
    } else {
        0.0
    }
}

/// Rebuild-path mirror of the weighted-avg clamp (from csv_to_market_json's
/// `_clamp_avg`): when a stored 48h avg falls outside 3× of the 90d baseline
/// median, quote the median instead. avg 0 (no recent trades) stays 0.
pub fn clamp_avg(avg: f64, median_90d: f64) -> f64 {
    if avg > 0.0 && median_90d > 0.0 && !(median_90d / 3.0 <= avg && avg <= median_90d * 3.0) {
        median_90d
    } else {
        avg
    }
}

/// Rebuild-path mirror of `_clamp_low5`: on a thin/steep book one cheap ask
/// sets low_sell while the next asks cliff upward, so the 5-ask mean
/// overstates. Clamp the HIGH side to the 90d median; the cliff test
/// (`low_sell <= median*3`) leaves a real across-the-board rise untouched.
pub fn clamp_low5(low5: f64, median_90d: f64, low_sell: f64) -> f64 {
    if low5 > 0.0 && median_90d > 0.0 && low5 > median_90d * 3.0 && low_sell <= median_90d * 3.0 {
        median_90d
    } else {
        low5
    }
}

/// Highest live buy price, or 0 when there are no buys — Python's
/// `max((o["platinum"] for o in live_buys), default=0)`.
pub fn top_buy(buys: &[LiveOrder]) -> f64 {
    buys.iter().map(|o| o.platinum).reduce(f64::max).unwrap_or(0.0)
}

/// Lowest live sell price, or 0 when there are no sells — Python's
/// `min((o["platinum"] for o in live_sells), default=0)`.
pub fn low_sell(sells: &[LiveOrder]) -> f64 {
    sells.iter().map(|o| o.platinum).reduce(f64::min).unwrap_or(0.0)
}

/// Bid/ask spread, mirroring Python's truthy guard:
/// `(low_sell - top_buy) if (low_sell and top_buy) else 0`. A zero on either
/// side (no book) yields 0, not a negative "spread" against a phantom price.
pub fn spread(low_sell: f64, top_buy: f64) -> f64 {
    if low_sell != 0.0 && top_buy != 0.0 {
        low_sell - top_buy
    } else {
        0.0
    }
}

/// Live buy/sell pressure ratio. With sellers present it's `buys / sells`;
/// with buyers but no sellers Python treats it as very high demand pressure
/// (`buys * 10`), an arbitrary boost for "no competition". No sellers AND no
/// buyers → 0.
pub fn demand_ratio(buys: usize, sells: usize) -> f64 {
    if sells > 0 {
        buys as f64 / sells as f64
    } else {
        buys as f64 * 10.0
    }
}

/// The composite "worth farming right now" score — the SCORE() at the bottom
/// of `analyze_item`: `volume_48h * avg_price_48h * (1 + ratio)`.
pub fn score(volume_48h: f64, avg_price_48h: f64, ratio: f64) -> f64 {
    volume_48h * avg_price_48h * (1.0 + ratio)
}

/// Python's `round(x, dp)` — round half to EVEN (banker's rounding) on the
/// *true* value of the float. A naive `(x * 10^dp).round_ties_even()` would
/// diverge whenever the scaling multiply lands the value exactly on a tie the
/// unscaled value wasn't on (2.675 → 267.5, rounding up to 2.68 where Python
/// keeps 2.67). Rust's `{:.dp}` formatter is correctly-rounded to the true
/// value with the same ties-to-even rule CPython uses, so round-trip through it
/// matches Python bit-for-bit across the pipeline's value range. Non-finite
/// passes through unchanged.
pub fn round_dp(x: f64, dp: usize) -> f64 {
    if !x.is_finite() {
        return x;
    }
    format!("{x:.dp$}").parse().unwrap_or(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Fixture helpers mirroring tests/test_wfm_demand.py's _day().
    fn day(median: f64) -> StatsDay {
        StatsDay { median, ..Default::default() }
    }
    fn day_rank(median: f64, rank: Option<Option<i64>>) -> StatsDay {
        StatsDay { median, mod_rank: rank, ..Default::default() }
    }
    fn day_sub(median: f64, volume: f64, subtype: &str) -> StatsDay {
        StatsDay { median, volume, subtype: Some(subtype.into()), ..Default::default() }
    }
    fn ask(platinum: f64) -> LiveOrder {
        LiveOrder { platinum, ..Default::default() }
    }
    fn ask_rank(platinum: f64, rank: Option<i64>) -> LiveOrder {
        LiveOrder { platinum, rank, ..Default::default() }
    }

    // ---- drop_poisoned_rows -------------------------------------------
    #[test]
    fn poison_drops_cap_pinned_median() {
        let rows = [day(1.0), day(PLAT_CAP)];
        assert_eq!(drop_poisoned_rows(&rows), vec![rows[0].clone()]);
    }

    #[test]
    fn poison_drops_cap_pinned_max_price_even_with_sane_median() {
        let bad = StatsDay { median: 12.0, max_price: PLAT_CAP, ..Default::default() };
        let rows = [day(10.0), bad];
        assert_eq!(drop_poisoned_rows(&rows), vec![rows[0].clone()]);
    }

    #[test]
    fn poison_drops_extreme_outlier_vs_baseline() {
        let rows = [day(1.0), day(1.0), day(1.0), day(300.0)];
        assert_eq!(drop_poisoned_rows(&rows), rows[..3].to_vec());
    }

    #[test]
    fn poison_keeps_cheap_outliers_below_absolute_floor() {
        // A real balance-patch repricing (1p junk → 60p) is >50x but honest.
        let rows = [day(1.0), day(1.0), day(1.0), day(60.0)];
        assert_eq!(drop_poisoned_rows(&rows), rows.to_vec());
    }

    #[test]
    fn poison_keeps_legit_spike_below_factor() {
        let rows = [day(10.0), day(12.0), day(400.0)];
        assert_eq!(drop_poisoned_rows(&rows), rows.to_vec()); // 400 < 50x baseline(12)
    }

    #[test]
    fn poison_returns_empty_when_everything_is_manipulated() {
        // The Goopolla regression: pure wash-trade series must yield vol 0.
        let rows = [day(PLAT_CAP), day(PLAT_CAP)];
        assert_eq!(drop_poisoned_rows(&rows), vec![]);
    }

    #[test]
    fn poison_empty_input_is_empty() {
        assert_eq!(drop_poisoned_rows(&[]), vec![]);
    }

    // ---- rank0_rows ----------------------------------------------------
    #[test]
    fn rank0_passthrough_when_no_rank_metadata() {
        let rows = [day(10.0), day(12.0)];
        assert_eq!(rank0_rows(&rows), rows.to_vec());
    }

    #[test]
    fn rank0_filters_max_rank_tier() {
        let rows = [day_rank(60.0, Some(Some(0))), day_rank(160.0, Some(Some(10)))];
        assert_eq!(rank0_rows(&rows), vec![rows[0].clone()]);
    }

    #[test]
    fn rank0_all_max_rank_returns_empty_not_inflated() {
        // The Primed-mod regression: only-max-rank window = "no rank-0
        // activity", not max-rank prices wearing a rank-0 label.
        let rows = [day_rank(160.0, Some(Some(10))), day_rank(179.0, Some(Some(10)))];
        assert_eq!(rank0_rows(&rows), vec![]);
    }

    #[test]
    fn rank0_treats_null_rank_as_unranked() {
        let rows = [day_rank(60.0, Some(None)), day_rank(160.0, Some(Some(10)))];
        assert_eq!(rank0_rows(&rows), vec![rows[0].clone()]);
    }

    // ---- rank0_orders --------------------------------------------------
    #[test]
    fn rank0_orders_drops_ranked_tiers() {
        let book = [ask_rank(30.0, Some(3)), ask_rank(35.0, Some(0)), ask_rank(40.0, Some(0))];
        let kept: Vec<f64> = rank0_orders(&book).iter().map(|o| o.platinum).collect();
        assert_eq!(kept, vec![35.0, 40.0]);
    }

    #[test]
    fn rank0_orders_keeps_untiered_items_whole() {
        let book = [ask(10.0), ask(12.0), ask(15.0)];
        assert_eq!(rank0_orders(&book), book.to_vec());
    }

    #[test]
    fn rank0_orders_all_maxed_means_empty_not_maxed_prices() {
        let book = [ask_rank(120.0, Some(5)), ask_rank(140.0, Some(5))];
        assert_eq!(rank0_orders(&book), vec![]);
    }

    // ---- canonical_subtype / subtype_rows -------------------------------
    #[test]
    fn subtype_none_for_untiered_items() {
        let rows = [day(10.0), day(12.0)];
        assert_eq!(canonical_subtype(&rows, |d| d.volume), None);
        assert_eq!(subtype_rows(&rows, None), rows.to_vec());
    }

    #[test]
    fn subtype_prefers_intact_for_relics() {
        // meso_l1_relic regression: one radiant day at 120p must not blend
        // into the intact baseline.
        let rows = [
            day_sub(5.0, 2.0, "intact"),
            day_sub(14.0, 18.0, "intact"),
            day_sub(120.0, 12.0, "radiant"),
        ];
        let pick = canonical_subtype(&rows, |d| d.volume);
        assert_eq!(pick.as_deref(), Some("intact"));
        assert_eq!(subtype_rows(&rows, pick.as_deref()), rows[..2].to_vec());
    }

    #[test]
    fn subtype_falls_back_to_dominant_volume_without_intact() {
        let rows = [day_sub(3.0, 40.0, "raw"), day_sub(9.0, 2.0, "cut")];
        assert_eq!(canonical_subtype(&rows, |d| d.volume).as_deref(), Some("raw"));
    }

    #[test]
    fn subtype_tie_resolves_to_first_seen_like_python() {
        let rows = [day_sub(3.0, 10.0, "raw"), day_sub(9.0, 10.0, "cut")];
        assert_eq!(canonical_subtype(&rows, |d| d.volume).as_deref(), Some("raw"));
    }

    #[test]
    fn subtype_keeps_generic_rows_under_a_pick() {
        let rows = [day_sub(5.0, 0.0, "intact"), day(6.0)];
        assert_eq!(subtype_rows(&rows, Some("intact")), rows.to_vec());
    }

    // ---- avg_lowest_asks -------------------------------------------------
    #[test]
    fn avg_lowest_asks_takes_cheapest_n() {
        let orders: Vec<LiveOrder> =
            [30.0, 12.0, 50.0, 14.0, 11.0, 200.0, 13.0].iter().map(|&p| ask(p)).collect();
        assert_eq!(avg_lowest_asks(&orders, 5), 16.0); // 11,12,13,14,30
    }

    #[test]
    fn avg_lowest_asks_dilutes_a_single_troll_listing() {
        let mut orders = vec![ask(1.0)];
        orders.extend(std::iter::repeat_with(|| ask(38.0)).take(6));
        assert_eq!(avg_lowest_asks(&orders, 5), 30.6); // (1 + 38*4) / 5
    }

    #[test]
    fn avg_lowest_asks_handles_thin_and_empty_books() {
        assert_eq!(avg_lowest_asks(&[ask(20.0), ask(24.0)], 5), 22.0);
        assert_eq!(avg_lowest_asks(&[], 5), 0.0);
        assert_eq!(avg_lowest_asks(&[ask(0.0)], 5), 0.0);
    }

    #[test]
    fn avg_lowest_asks_rounds_ties_to_even_like_python() {
        // mean 2.25 (exactly representable) → 2.2 under Python's round(x, 1).
        assert_eq!(avg_lowest_asks(&[ask(2.0), ask(2.5)], 5), 2.2);
    }

    // ---- series_stats ------------------------------------------------------
    #[test]
    fn series_stats_empty() {
        assert_eq!(series_stats(&[]), (0.0, 0.0, vec![], 0.0, 0.0));
    }

    #[test]
    fn series_stats_splits_now_from_baseline_and_recomputes_band() {
        let rows: Vec<StatsDay> = [10.0, 12.0, 8.0, 14.0, 11.0].iter().map(|&m| day(m)).collect();
        let (now, base, sevens, top, bot) = series_stats(&rows);
        assert_eq!(now, 11.0); // latest day, not the baseline
        assert_eq!(base, 11.0); // median of [8,10,11,12,14]
        assert_eq!(sevens, vec![10.0, 12.0, 8.0, 14.0, 11.0]);
        assert_eq!((top, bot), (14.0, 8.0)); // from the series, not WFM's donch
    }

    #[test]
    fn series_stats_ignores_zero_median_days_in_baseline() {
        let rows = [day(0.0), day(10.0), day(20.0)];
        let (now, base, _, top, bot) = series_stats(&rows);
        assert_eq!(now, 20.0);
        assert_eq!(base, 15.0); // even-length median: mean of the two middles
        assert_eq!((top, bot), (20.0, 10.0));
    }

    // ---- weighted_avg_48h ------------------------------------------------
    #[test]
    fn weighted_avg_ignores_outlier_days() {
        // deimos_heart_scene: one 500p sale against a 9p baseline.
        let recent = [
            StatsDay { avg_price: 500.0, volume: 1.0, ..Default::default() },
            StatsDay { avg_price: 10.0, volume: 4.0, ..Default::default() },
        ];
        assert_eq!(weighted_avg_48h(&recent, 9.0), 10.0);
    }

    #[test]
    fn weighted_avg_falls_back_to_median_when_all_outliers() {
        let recent = [StatsDay { avg_price: 500.0, volume: 1.0, ..Default::default() }];
        assert_eq!(weighted_avg_48h(&recent, 9.0), 9.0);
    }

    #[test]
    fn weighted_avg_no_baseline_weights_everything() {
        let recent = [
            StatsDay { avg_price: 10.0, volume: 1.0, ..Default::default() },
            StatsDay { avg_price: 20.0, volume: 3.0, ..Default::default() },
        ];
        assert_eq!(weighted_avg_48h(&recent, 0.0), 17.5);
    }

    // ---- clamp_avg / clamp_low5 -------------------------------------------
    #[test]
    fn clamp_avg_pulls_high_outlier_to_median() {
        assert_eq!(clamp_avg(500.0, 9.0), 9.0);
    }

    #[test]
    fn clamp_avg_pulls_low_outlier_up_to_median() {
        assert_eq!(clamp_avg(26.0, 80.0), 80.0);
    }

    #[test]
    fn clamp_avg_leaves_in_range_value_untouched() {
        assert_eq!(clamp_avg(12.0, 9.0), 12.0);
        assert_eq!(clamp_avg(27.0, 9.0), 27.0); // exactly 3x
    }

    #[test]
    fn clamp_avg_keeps_zero_and_handles_missing_median() {
        assert_eq!(clamp_avg(0.0, 9.0), 0.0);
        assert_eq!(clamp_avg(500.0, 0.0), 500.0);
    }

    #[test]
    fn clamp_low5_pulls_down_a_cliff_book() {
        assert_eq!(clamp_low5(35.2, 5.0, 5.0), 5.0);
    }

    #[test]
    fn clamp_low5_preserves_a_real_across_the_board_high() {
        assert_eq!(clamp_low5(72.0, 17.0, 59.0), 72.0);
    }

    #[test]
    fn clamp_low5_leaves_in_range_and_zero_untouched() {
        assert_eq!(clamp_low5(12.0, 9.0, 8.0), 12.0);
        assert_eq!(clamp_low5(0.0, 5.0, 5.0), 0.0);
        assert_eq!(clamp_low5(35.0, 0.0, 5.0), 35.0);
    }

    // ---- top_buy / low_sell / spread --------------------------------------
    #[test]
    fn top_buy_is_the_dearest_bid_or_zero() {
        let book = [ask(10.0), ask(35.0), ask(22.0)];
        assert_eq!(top_buy(&book), 35.0);
        assert_eq!(top_buy(&[]), 0.0); // Python's max(..., default=0)
    }

    #[test]
    fn low_sell_is_the_cheapest_ask_or_zero() {
        let book = [ask(42.0), ask(40.0), ask(55.0)];
        assert_eq!(low_sell(&book), 40.0);
        assert_eq!(low_sell(&[]), 0.0); // Python's min(..., default=0)
    }

    #[test]
    fn spread_zeroes_when_either_side_is_missing() {
        assert_eq!(spread(40.0, 35.0), 5.0);
        assert_eq!(spread(40.0, 0.0), 0.0); // no bid → no spread
        assert_eq!(spread(0.0, 35.0), 0.0); // no ask → no spread
    }

    // ---- demand_ratio -----------------------------------------------------
    #[test]
    fn demand_ratio_is_buys_over_sells_when_sellers_exist() {
        assert_eq!(demand_ratio(12, 18), 12.0 / 18.0);
        assert_eq!(demand_ratio(0, 5), 0.0);
    }

    #[test]
    fn demand_ratio_boosts_when_no_sellers() {
        assert_eq!(demand_ratio(3, 0), 30.0); // buyers, no competition
        assert_eq!(demand_ratio(0, 0), 0.0); // dead book
    }

    // ---- score ------------------------------------------------------------
    #[test]
    fn score_is_volume_times_avg_times_one_plus_ratio() {
        assert_eq!(score(384.0, 43.2, 0.67), 384.0 * 43.2 * 1.67);
        assert_eq!(score(0.0, 43.2, 2.0), 0.0);
    }

    // ---- round_dp (Python round, ties-to-even) ----------------------------
    #[test]
    fn round_dp_matches_python_banker_rounding() {
        assert_eq!(round_dp(0.6666666, 2), 0.67);
        assert_eq!(round_dp(2.25, 1), 2.2); // ties to even (down)
        assert_eq!(round_dp(2.35, 1), 2.4); // ties to even (up)
        assert_eq!(round_dp(2.675, 2), 2.67); // float repr is 2.67499… → down
        assert_eq!(round_dp(11.0, 1), 11.0);
    }

    #[test]
    fn round_dp_passes_non_finite_through() {
        assert!(round_dp(f64::NAN, 1).is_nan());
        assert_eq!(round_dp(f64::INFINITY, 1), f64::INFINITY);
    }
}
