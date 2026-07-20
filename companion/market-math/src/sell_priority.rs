//! Sell-priority scoring — the "what to sell right now" ranking.
//!
//! Unlike the rest of this crate (a 1:1 port of the Python pipeline), this
//! module is a faithful Rust mirror of the CLIENT scoring in
//! `prototype/src/lib/sell-priority.ts` — the canonical sell ranking the SPA
//! table already uses. It lives here so the desktop tray + post-scan
//! notification rank with the SAME formula, giving one Rust source of truth for
//! both desktop consumers instead of a second, drifting heuristic.
//!
//! A shared-fixture parity test (`tests/fixtures/sell-priority/cases.json`,
//! checked from BOTH the Rust consumer and Vitest) guards against silent
//! divergence: change the score in one language and the golden order in the
//! fixture must change too, which then fails the other side until it matches.
//!
//! Purity is preserved (no I/O, no deps, no clocks) like everything here. The
//! one subtlety is JS truthiness: `sell-priority.ts` reads every field as
//! `Number(x) || 0`, so `0`, `NaN`, and a missing key all collapse to `0` and
//! fall through to the next fallback. [`truthy`] mirrors that exactly.

/// Below this many closed trades / 48 h the book is too thin to trust its ask
/// as a forecast — the ask clamp and the trend badges share it. Mirrors
/// `LIQUID_VOL` in sell-priority.ts.
pub const LIQUID_VOL: f64 = 5.0;
/// At/under this 48 h volume a listing barely moves — flagged `patience`.
/// Mirrors `PATIENCE_VOL` in sell-priority.ts.
const PATIENCE_VOL: f64 = 3.0;

/// The market fields the score reads — the Rust counterpart of TS's
/// `Pick<MarketItemEntry, 'vol' | 'low_sell' | 'avg' | 'median_now' |
/// 'median_90d'>`. A missing field in JS reads as `0`, so `Default` (all
/// zeroes) is the "no data" entry.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PricedEntry {
    pub vol: f64,
    pub low_sell: f64,
    pub avg: f64,
    pub median_now: f64,
    pub median_90d: f64,
}

/// Output of [`score_row`]: the liquidity-discounted expected daily take, plus
/// the `patience` flag for barely-moving books.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SellScore {
    pub sell_score: f64,
    pub patience: bool,
}

/// Mirror JS `Number(x) || 0`: `0`, `-0`, and non-finite (`NaN`/`Inf`, the
/// stand-ins for a missing/garbage field) all collapse to `0.0`; a normal
/// value passes through. Prices/volumes are never negative in the snapshot, so
/// negatives (truthy in JS) don't arise and aren't special-cased.
fn truthy(x: f64) -> f64 {
    if x.is_finite() && x != 0.0 {
        x
    } else {
        0.0
    }
}

/// How many copies are actually listable: owned minus whichever holds back
/// more, the user's reserve or the count of leveled (XP > 0, untradeable)
/// copies — they don't stack. Clamped at 0. 1:1 with `sellableQty` in
/// sell-priority.ts.
pub fn sellable_qty(count: i64, reserve: i64, leveled: i64) -> i64 {
    (count - reserve.max(leveled)).max(0)
}

/// What a listing would realistically clear at — the sanity-clamped ask. 1:1
/// with `clearingPrice` in sell-priority.ts:
///  - no live ask → median, else avg, floor 1;
///  - a troll undercut (ask < median/3) → the median;
///  - a thin book (vol < LIQUID_VOL) with an aspirational ask (> 1.5× median)
///    → 1.5× median;
///  - otherwise the live ask (on a liquid book the ask is real information).
pub fn clearing_price(m: &PricedEntry) -> f64 {
    let low_sell = truthy(m.low_sell);
    // `median_now || median_90d` in TS: a literal 0 (thin item, no recent
    // trade) falls through to the 90d baseline.
    let median = {
        let now = truthy(m.median_now);
        if now != 0.0 {
            now
        } else {
            truthy(m.median_90d)
        }
    };
    let avg = truthy(m.avg);
    let vol = truthy(m.vol);
    if low_sell <= 0.0 {
        return if median > 0.0 { median } else { avg.max(1.0) };
    }
    if median > 0.0 {
        if low_sell * 3.0 < median {
            return median;
        }
        if vol < LIQUID_VOL && low_sell > median * 1.5 {
            return median * 1.5;
        }
    }
    low_sell
}

/// The sell score for one owned row: `min(owned, vol/2 floored at 0.05) ×
/// clearing_price`. `owned` is the *sellable* quantity (post-reserve). 1:1
/// with `scoreRow` in sell-priority.ts — the no-market-entry case (TS `!m`
/// → `{0, false}`) is the caller's job here (callers skip unresolvable rows).
pub fn score_row(owned: f64, m: &PricedEntry) -> SellScore {
    let vol = truthy(m.vol);
    let price = clearing_price(m);
    let daily_sales = (vol / 2.0).max(0.05);
    let units_today = owned.min(daily_sales);
    SellScore {
        sell_score: units_today * price,
        patience: vol < PATIENCE_VOL,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Ported 1:1 from prototype/src/lib/sell-priority.test.ts so the two
    // suites move together.
    fn m(vol: f64, low_sell: f64, avg: f64, median_now: f64, median_90d: f64) -> PricedEntry {
        PricedEntry { vol, low_sell, avg, median_now, median_90d }
    }

    // ---- score_row --------------------------------------------------------
    #[test]
    fn score_uses_low_sell_when_available() {
        let r = score_row(10.0, &m(20.0, 30.0, 45.0, 0.0, 0.0));
        assert_eq!(r.sell_score, 300.0); // min(10, 10) × 30
        assert!(!r.patience);
    }

    #[test]
    fn score_falls_back_to_avg_when_low_sell_missing() {
        let r = score_row(1.0, &m(10.0, 0.0, 50.0, 0.0, 0.0));
        assert_eq!(r.sell_score, 50.0); // min(1, 5) × 50
    }

    #[test]
    fn score_caps_units_at_market_absorption() {
        let r = score_row(100.0, &m(4.0, 100.0, 110.0, 0.0, 0.0));
        assert_eq!(r.sell_score, 200.0); // dailySales 2 × 100p
    }

    #[test]
    fn score_caps_at_what_you_own() {
        let r = score_row(1.0, &m(100.0, 20.0, 25.0, 0.0, 0.0));
        assert_eq!(r.sell_score, 20.0); // min(1, 50) × 20
    }

    #[test]
    fn score_patience_flag_boundary() {
        assert!(score_row(5.0, &m(1.0, 200.0, 220.0, 0.0, 0.0)).patience);
        assert!(score_row(5.0, &m(0.0, 200.0, 220.0, 0.0, 0.0)).patience);
        assert!(score_row(5.0, &m(2.0, 200.0, 220.0, 0.0, 0.0)).patience);
        assert!(!score_row(5.0, &m(3.0, 200.0, 220.0, 0.0, 0.0)).patience);
    }

    #[test]
    fn score_dead_item_still_ranks_low() {
        let r = score_row(1.0, &m(0.0, 100.0, 100.0, 0.0, 0.0));
        assert!((r.sell_score - 5.0).abs() < 1e-9); // 0.05 floor × 100
        assert!(r.patience);
    }

    #[test]
    fn score_missing_fields_default_to_zero() {
        // TS `scoreRow({owned:0, m:{}})` → {0, patience:true}
        let r = score_row(0.0, &PricedEntry::default());
        assert_eq!(r.sell_score, 0.0);
        assert!(r.patience);
    }

    #[test]
    fn score_fantasy_ask_does_not_top_the_sort() {
        // corpus_void_key: vol 1, 2999p ask, ~200p real trades.
        let r = score_row(3.0, &m(1.0, 2999.0, 204.0, 0.0, 200.0));
        assert!((r.sell_score - 150.0).abs() < 1e-9); // 0.5 × (200 × 1.5)
    }

    #[test]
    fn score_vol2_ask_does_not_dodge_the_clamp() {
        let r = score_row(1.0, &m(2.0, 100.0, 10.0, 10.0, 0.0));
        assert!((r.sell_score - 15.0).abs() < 1e-9); // min(1,1) × (10 × 1.5)
        assert!(r.patience);
    }

    // ---- clearing_price ---------------------------------------------------
    #[test]
    fn clearing_uses_live_ask_when_it_agrees() {
        assert_eq!(clearing_price(&m(20.0, 60.0, 62.0, 65.0, 0.0)), 60.0);
    }

    #[test]
    fn clearing_clamps_troll_undercut_up_to_median() {
        assert_eq!(clearing_price(&m(54.0, 1.0, 30.0, 38.0, 0.0)), 38.0);
    }

    #[test]
    fn clearing_clamps_thin_aspirational_ask_to_1_5x() {
        assert_eq!(clearing_price(&m(1.0, 2999.0, 204.0, 204.0, 0.0)), 306.0);
        assert_eq!(clearing_price(&m(2.0, 100.0, 10.0, 10.0, 0.0)), 15.0);
        assert_eq!(clearing_price(&m(4.0, 100.0, 10.0, 10.0, 0.0)), 15.0);
    }

    #[test]
    fn clearing_keeps_thin_ask_within_1_5x() {
        assert_eq!(clearing_price(&m(2.0, 14.0, 11.0, 10.0, 0.0)), 14.0);
    }

    #[test]
    fn clearing_keeps_liquid_high_ask() {
        assert_eq!(clearing_price(&m(30.0, 700.0, 250.0, 200.0, 0.0)), 700.0);
        assert_eq!(clearing_price(&m(5.0, 100.0, 10.0, 10.0, 0.0)), 100.0); // vol 5 = liquid
    }

    #[test]
    fn clearing_falls_back_median_avg_one() {
        assert_eq!(clearing_price(&m(5.0, 0.0, 50.0, 42.0, 0.0)), 42.0);
        assert_eq!(clearing_price(&m(5.0, 0.0, 50.0, 0.0, 0.0)), 50.0);
        assert_eq!(clearing_price(&m(5.0, 0.0, 0.0, 0.0, 0.0)), 1.0);
    }

    #[test]
    fn clearing_uses_median_90d_when_now_absent() {
        assert_eq!(clearing_price(&m(54.0, 1.0, 30.0, 0.0, 38.0)), 38.0);
    }

    // ---- sellable_qty -----------------------------------------------------
    #[test]
    fn sellable_passthrough_with_zero_reserve() {
        assert_eq!(sellable_qty(5, 0, 0), 5);
        assert_eq!(sellable_qty(0, 0, 0), 0);
    }

    #[test]
    fn sellable_subtracts_reserve() {
        assert_eq!(sellable_qty(5, 1, 0), 4);
        assert_eq!(sellable_qty(5, 5, 0), 0);
    }

    #[test]
    fn sellable_clamps_at_zero() {
        assert_eq!(sellable_qty(2, 5, 0), 0);
        assert_eq!(sellable_qty(0, 3, 0), 0);
        assert_eq!(sellable_qty(3, 0, 5), 0);
    }

    #[test]
    fn sellable_leveled_and_reserve_do_not_stack() {
        assert_eq!(sellable_qty(5, 1, 2), 3); // leveled wins
        assert_eq!(sellable_qty(5, 3, 1), 2); // reserve wins
        assert_eq!(sellable_qty(4, 0, 4), 0); // all leveled
    }
}
