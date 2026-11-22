//! Sell-priority scoring - the "what to sell right now" ranking.
//!
//! The only implementation of the score: the desktop table, tray and planners
//! read it, and the host pipeline scores the homepage's hand-off sample with it.
//! The views call the clearing price, quantities and pick rule synchronously
//! through `market-domain-web`; `frontend/src/domain/sell-priority.ts` keeps
//! only the call shape and the thresholds it prints, pinned to the values here
//! by shared fixtures.
//!
//! No I/O and no clock, like everything here. Snapshot fields arrive as the
//! page read them (`Number(x) || 0`), so `0`, `NaN` and a missing key all
//! mean "no figure" and fall through to the next fallback; [`truthy`] keeps
//! that reading for callers that pass raw numbers.

/// Below this many closed trades / 48 h the book is too thin to trust its ask
/// as a forecast - the ask clamp, the trend badges and the thin liquidity
/// class share it. Pinned against `LIQUID_VOL` in sell-priority.ts by
/// `tests/fixtures/sell-priority/liquidity-boundary.json`, checked from
/// market-domain so this crate stays dependency-free.
pub const LIQUID_VOL: f64 = 5.0;
/// The priority a row needs to be a Top pick; the evening digest uses the same
/// bar so both name the same items. Pinned against `MIN_PICK_SCORE` in
/// sell-priority.ts by `tests/fixtures/sell-priority/pick-floor.json`.
pub const MIN_PICK_SCORE: f64 = 20.0;
/// How many Top picks, and digest lines, at most.
pub const MAX_PICKS: usize = 5;

/// Whether a row is a Top pick: copies to sell, a book that moves, not a dip
/// to wait out (`hold`; a flooded drop still counts), and the bar cleared.
/// The Sell view's `selectPicks` applies the same rule, pinned by
/// `tests/fixtures/sell-priority/pick-floor.json`.
/// One row as Top picks and the evening digest read it.
#[derive(Debug, Clone, Copy)]
pub struct PickCandidate {
    pub sellable: f64,
    pub patience: bool,
    pub hold: bool,
    pub score: f64,
}

/// Indices of the first `cap` rows that are picks, in the given order: rows
/// arrive best-first, and re-sorting here would hide a caller that stopped
/// guaranteeing it.
pub fn select_picks(rows: &[PickCandidate], cap: usize) -> Vec<usize> {
    rows.iter()
        .enumerate()
        .filter(|(_, r)| is_pick(r.sellable, r.patience, r.hold, r.score))
        .map(|(i, _)| i)
        .take(cap)
        .collect()
}

pub fn is_pick(sellable: f64, patience: bool, hold: bool, score: f64) -> bool {
    sellable > 0.0 && !patience && !hold && score >= MIN_PICK_SCORE
}
/// At/under this 48 h volume a listing barely moves - flagged `patience`.
/// The patience bar has no TypeScript copy: the score that carries it is native.
const PATIENCE_VOL: f64 = 3.0;
pub const DEAD_SHARE: f64 = 0.15;
pub const POPULAR_SHARE: f64 = 1.0;

pub fn liquidity_weight(usage_share: Option<f64>) -> f64 {
    let Some(share) = usage_share.filter(|share| share.is_finite() && *share >= 0.0) else {
        return 1.0;
    };
    if share < DEAD_SHARE {
        0.75
    } else if share >= POPULAR_SHARE {
        1.25
    } else {
        0.75 + ((share - DEAD_SHARE) / (POPULAR_SHARE - DEAD_SHARE)) * 0.5
    }
}

pub fn usage_weight_tier(usage_share: Option<f64>) -> &'static str {
    let Some(share) = usage_share.filter(|share| share.is_finite() && *share >= 0.0) else {
        return "neutral";
    };
    if share < DEAD_SHARE {
        "low"
    } else if share >= POPULAR_SHARE {
        "popular"
    } else {
        "typical"
    }
}

/// The market fields the score reads - the Rust counterpart of TS's
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

/// Output of [`score_row`]: the sell prioritization score plus the `patience`
/// flag for barely-moving books.
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
/// more, the user's "Keep copies" reserve or the leveled copies. Warframe
/// trades only unranked gear, so a leveled (XP > 0) copy already satisfies the
/// "don't sell my only copy" intent the reserve exists for: they don't stack.
/// Clamped at 0, so a stale or edited reserve can never go negative.
pub fn sellable_qty(count: i64, reserve: i64, leveled: i64) -> i64 {
    (count - reserve.max(leveled)).max(0)
}

/// [`sellable_qty`] over the snapshot's JSON numbers.
pub fn sellable(count: f64, reserve: f64, leveled: f64) -> f64 {
    (count - reserve.max(leveled)).max(0.0)
}

/// The Spares preset's notion of sellable: copies you would otherwise dissolve
/// for endo. Owning a ranked copy (`kept_lvl` > 0, the one you use) makes every
/// tradeable copy a spare; otherwise one is kept to rank later. Leveled copies
/// never count. `None` is "no individual copy recorded".
pub fn spare_qty(count: f64, kept_lvl: Option<f64>, leveled: f64) -> f64 {
    let tradeable = (count - leveled).max(0.0);
    if kept_lvl.is_some_and(|level| level > 0.0) {
        tradeable
    } else {
        (tradeable - 1.0).max(0.0)
    }
}

/// What a listing would realistically clear at - the sanity-clamped ask. The
/// lowest live ask is usually the honest answer, but any account can set it
/// for free, so it is checked against the closed-trade median:
///  - no live ask → median, else avg, floor 1;
///  - a troll undercut (ask < median/3) → the median. One 1p ask under a
///    stable 38p median once cratered akbolto receiver's score, killed its
///    peak pill and fired a Baro deal badge; a lone listing absorbs one sale;
///  - a thin book (vol < LIQUID_VOL) with an aspirational ask (> 1.5× median)
///    → 1.5× median. A vol-2 scene with one 100p ask over a 10p median once
///    ranked #7; on a thin book the closed trades are the evidence;
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

/// The neutral sell score for one owned row. `score_row_weighted` applies the
/// bounded usage multiplier used by product ranking. `owned` is the sellable
/// quantity (post-reserve). A row with no market entry is the caller's to
/// skip.
pub fn score_row(owned: f64, m: &PricedEntry) -> SellScore {
    score_row_weighted(owned, m, None)
}

pub fn score_row_weighted(owned: f64, m: &PricedEntry, usage_share: Option<f64>) -> SellScore {
    let vol = truthy(m.vol);
    let price = clearing_price(m);
    let daily_sales = (vol / 2.0).max(0.05);
    let units_today = owned.min(daily_sales);
    SellScore {
        sell_score: units_today * price * liquidity_weight(usage_share),
        patience: vol < PATIENCE_VOL,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pick_floor_matches_the_typescript_side() {
        let floor: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/sell-priority/pick-floor.json"
        ))
        .unwrap();
        assert_eq!(floor["min_pick_score"].as_f64(), Some(MIN_PICK_SCORE));
        assert_eq!(floor["max_picks"].as_u64(), Some(MAX_PICKS as u64));
        for case in floor["cases"].as_array().unwrap() {
            assert_eq!(
                is_pick(
                    case["sellable"].as_f64().unwrap(),
                    case["patience"].as_bool().unwrap(),
                    case["timing"] == "hold",
                    case["score"].as_f64().unwrap(),
                ),
                case["pick"].as_bool().unwrap(),
                "{}",
                case["name"]
            );
        }
    }

    fn m(vol: f64, low_sell: f64, avg: f64, median_now: f64, median_90d: f64) -> PricedEntry {
        PricedEntry {
            vol,
            low_sell,
            avg,
            median_now,
            median_90d,
        }
    }

    #[test]
    fn invalid_usage_is_neutral() {
        for share in [Some(-1.0), Some(f64::NAN), Some(f64::INFINITY), None] {
            assert_eq!(liquidity_weight(share), 1.0);
        }
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
        // owned 0 on an empty entry: no score, and a barely-moving book
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

    #[test]
    fn sellable_over_json_numbers_matches_the_integer_rule() {
        for (count, reserve, leveled) in [(5, 1, 2), (5, 3, 1), (4, 0, 4), (2, 5, 0), (3, 0, 5)] {
            assert_eq!(
                sellable(f64::from(count), f64::from(reserve), f64::from(leveled)),
                sellable_qty(count.into(), reserve.into(), leveled.into()) as f64
            );
        }
    }

    #[test]
    fn spares_are_every_copy_beside_a_ranked_one_else_all_but_one() {
        assert_eq!(spare_qty(4.0, Some(5.0), 0.0), 4.0);
        assert_eq!(spare_qty(4.0, Some(1.0), 0.0), 4.0);
        assert_eq!(spare_qty(4.0, None, 0.0), 3.0);
        assert_eq!(spare_qty(4.0, Some(0.0), 0.0), 3.0);
        assert_eq!(spare_qty(1.0, None, 0.0), 0.0);
        assert_eq!(spare_qty(0.0, None, 0.0), 0.0);
        assert_eq!(spare_qty(3.0, Some(5.0), 3.0), 0.0);
        assert_eq!(spare_qty(3.0, Some(5.0), 1.0), 2.0);
        assert_eq!(spare_qty(3.0, None, 1.0), 1.0);
    }

    #[test]
    fn picks_filter_and_truncate_without_resorting() {
        let row = |score: f64| PickCandidate {
            sellable: 5.0,
            patience: false,
            hold: false,
            score,
        };
        let rows = [
            row(30.0),
            row(90.0),
            PickCandidate {
                hold: true,
                ..row(500.0)
            },
            PickCandidate {
                patience: true,
                ..row(500.0)
            },
            PickCandidate {
                sellable: 0.0,
                ..row(500.0)
            },
            row(MIN_PICK_SCORE - 0.01),
            row(MIN_PICK_SCORE),
        ];
        assert_eq!(select_picks(&rows, MAX_PICKS), vec![0, 1, 6]);
        assert_eq!(select_picks(&rows, 1), vec![0]);
        assert!(select_picks(&[], MAX_PICKS).is_empty());
    }
}
