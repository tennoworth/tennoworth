//! The rules My orders and the listing review apply to the account's own sell
//! listings: lot and unit prices, the player's price rule, the live-ask read
//! that skips a troll undercut, listing health against the live book and the
//! last scan, snapshot drift, and what an interrupted batch has left. The
//! views call them synchronously through `market-domain-web`; the
//! TypeScript in `frontend/src/domain/` keeps the call shape.
//!
//! Inputs arrive as the page holds them, so a number can be absent, `null`
//! (JSON's spelling of `NaN`) or fractional where an integer is expected;
//! each rule reads them as the page always did.

use crate::limits::MAX_PER_TRADE;
use crate::math::sell_priority::{self, PricedEntry, LIQUID_VOL, TROLL_UNDERCUT_RATIO};
use crate::orders::OrderSide;
use crate::page::{self, deserialize_by_reading};
use serde::Serialize;
use serde_json::Value;
use ts_rs::TS;

/// How far off the reference price a listing must be before it is worth a
/// reprice. Below this, repricing costs a market write and a trip through the
/// rate limiter to chase noise.
pub const DRIFT_PCT: f64 = 0.2;

/// ...and it must also move by at least this much platinum: 20% of a 5p item
/// is 1p, which is churn, not advice. Both gates must pass.
pub const MIN_DRIFT_PLAT: f64 = 3.0;

/// How many drifted listings My orders shows by default.
pub const DRIFT_LIMIT: usize = 25;

const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

fn is_safe_integer(x: f64) -> bool {
    x.is_finite() && x.trunc() == x && x.abs() <= MAX_SAFE_INTEGER
}

/// `Math.max`: unlike `f64::max`, a `NaN` operand wins.
fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.max(b)
    }
}

fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.min(b)
    }
}

/// `Math.round`: halves round up, toward +inf, where `f64::round` rounds
/// them away from zero.
fn js_round(x: f64) -> f64 {
    let floor = x.floor();
    if x - floor >= 0.5 {
        floor + 1.0
    } else {
        floor
    }
}

/// `Number(x) || 0`.
fn num(x: Option<f64>) -> f64 {
    x.filter(|v| v.is_finite() && *v != 0.0).unwrap_or(0.0)
}

/// A figure a rule may use: snapshots write `0` for a statistic they do not
/// carry, so `0`, `null` and absence all mean "unknown".
fn known(value: Option<f64>) -> Option<f64> {
    value.filter(|v| v.is_finite() && *v > 0.0)
}

/// A stable insertion sort: `before(a, b)` puts `a` ahead of `b`, and equal
/// rows keep their order, as `Array.prototype.sort` keeps them. The lists are
/// a book's few buyers or an account's listings; std's sorts each cost the
/// view module several kilobytes per element type.
pub(crate) fn sort_stably<T>(rows: &mut [T], before: impl Fn(&T, &T) -> bool) {
    for i in 1..rows.len() {
        let mut j = i;
        while j > 0
            && rows
                .get(j)
                .zip(rows.get(j - 1))
                .is_some_and(|(a, b)| before(a, b))
        {
            rows.swap(j, j - 1);
            j -= 1;
        }
    }
}

fn lot_ok(lot: f64) -> bool {
    is_safe_integer(lot) && lot >= 1.0 && lot <= f64::from(MAX_PER_TRADE)
}

/// The per-unit price of a listing whose `platinum` buys one lot of `lot`.
pub fn unit_price(platinum: f64, lot: f64) -> Option<f64> {
    (platinum.is_finite() && platinum > 0.0 && lot_ok(lot)).then(|| platinum / lot)
}

/// The whole-platinum lot total for a per-unit price.
pub fn lot_price(unit: f64, lot: f64) -> Option<f64> {
    (unit.is_finite() && unit > 0.0 && lot_ok(lot)).then(|| (unit * lot).ceil())
}

/// Which market figure a suggested ask starts from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, TS)]
pub enum PriceBasis {
    #[ts(rename = "lowest-ask")]
    LowestAsk,
    #[ts(rename = "low5-avg")]
    Low5Avg,
    #[ts(rename = "low20-avg")]
    Low20Avg,
    #[ts(rename = "avg-48h")]
    Avg48h,
    #[ts(rename = "median-90d")]
    Median90d,
}

impl PriceBasis {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "lowest-ask" => Self::LowestAsk,
            "low5-avg" => Self::Low5Avg,
            "low20-avg" => Self::Low20Avg,
            "avg-48h" => Self::Avg48h,
            "median-90d" => Self::Median90d,
            _ => return None,
        })
    }
}

/// A player's pricing rule: a basis plus a platinum offset. It only changes
/// what is suggested; nothing reprices on its own.
#[derive(Debug, Clone, Copy, PartialEq, TS)]
pub struct PriceRule {
    /// `None` for a basis this build does not know, which prices nothing.
    #[ts(as = "PriceBasis")]
    pub basis: Option<PriceBasis>,
    pub offset: f64,
}

impl PriceRule {
    fn read(v: &Value) -> Self {
        Self {
            basis: v
                .get("basis")
                .and_then(Value::as_str)
                .and_then(PriceBasis::parse),
            offset: page::number(v, "offset").unwrap_or(f64::NAN),
        }
    }
}

/// The figures a basis can read.
#[derive(Debug, Clone, Default, PartialEq, TS)]
#[ts(rename_all = "camelCase", optional_fields = nullable)]
pub struct PriceEvidence {
    /// Lowest other online ask (own order excluded), or the troll-clamped
    /// clearing price when no live check has run.
    pub lowest_ask: Option<f64>,
    pub low5_avg: Option<f64>,
    /// Snapshot only: a live check returns at most five asks.
    pub low20_avg: Option<f64>,
    #[ts(rename = "avg48h")]
    pub avg_48h: Option<f64>,
    #[ts(rename = "median90d")]
    pub median_90d: Option<f64>,
}

impl PriceEvidence {
    fn read(v: &Value) -> Self {
        Self {
            lowest_ask: page::number(v, "lowestAsk"),
            low5_avg: page::number(v, "low5Avg"),
            low20_avg: page::number(v, "low20Avg"),
            avg_48h: page::number(v, "avg48h"),
            median_90d: page::number(v, "median90d"),
        }
    }
}

/// The suggested per-unit ask, or `None` when the rule's basis is unknown for
/// this item: an absent statistic must not turn into a plausible-looking price.
///
/// Not rounded: bulk listings compare fractional unit prices, and rounding here
/// hid a real overpricing (6.66p against a 6.5p ask became "fine" against 7p).
/// Callers round when they build the lot total. A negative offset cannot take
/// the price below 1p, or below the basis itself when that is already under 1p.
pub fn price_from_rule(rule: PriceRule, evidence: &PriceEvidence) -> Option<f64> {
    let base = known(match rule.basis? {
        PriceBasis::LowestAsk => evidence.lowest_ask,
        PriceBasis::Low5Avg => evidence.low5_avg,
        PriceBasis::Low20Avg => evidence.low20_avg,
        PriceBasis::Avg48h => evidence.avg_48h,
        PriceBasis::Median90d => evidence.median_90d,
    })?;
    Some(js_max(js_min(base, 1.0), base + rule.offset))
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, TS)]
pub struct LiveAskRead {
    /// The cheapest ask worth competing with, or `null` with no asks.
    pub ask: Option<f64>,
    /// The cheapest ask, when it was skipped as an outlier.
    pub ignored: Option<f64>,
}

/// The cheapest live ask "within reason". Any account can list for 1p, so an
/// ask under [`TROLL_UNDERCUT_RATIO`] times below the median is skipped for the
/// next one, as the snapshot's clearing price does. When every ask is that low
/// the book has moved, not been trolled (a live check returns five asks at
/// most), so the cheapest stands; without a median there is nothing to judge
/// against. `asks` is the live check's list, cheapest first.
pub fn reasonable_lowest_ask(asks: &[f64], median: Option<f64>) -> LiveAskRead {
    let cheapest = asks.first().copied();
    let (Some(lowest), Some(baseline)) = (cheapest, known(median)) else {
        return LiveAskRead {
            ask: cheapest,
            ignored: None,
        };
    };
    match asks
        .iter()
        .position(|ask| ask * TROLL_UNDERCUT_RATIO >= baseline)
    {
        Some(first) if first > 0 => LiveAskRead {
            ask: asks.get(first).copied(),
            ignored: Some(lowest),
        },
        _ => LiveAskRead {
            ask: Some(lowest),
            ignored: None,
        },
    }
}

/// A field whose presence matters even when its value is unknown, as it does
/// under object spread: `Given(None)` is a key holding `null`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum Stated {
    #[default]
    Absent,
    Given(Option<f64>),
}

/// What a health verdict says about one sell listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum HealthKind {
    /// The ask is above the price rule's target, by default the lowest OTHER
    /// online ask: it won't sell first.
    Overpriced,
    /// A live buyer bids MORE than the ask: platinum left on the table.
    Underbid,
    /// The ask sits well under the price rule's target. It still sells, for
    /// less than it could; the gap must clear the drift gates, because a small
    /// undercut is how most players sell.
    Underpriced,
    /// The listing's quantity exceeds what the last scan says is owned.
    ExcessQty,
    /// The last scan says none is owned: the listing is a ghost.
    NotOwned,
}

impl HealthKind {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "overpriced" => Self::Overpriced,
            "underbid" => Self::Underbid,
            "underpriced" => Self::Underpriced,
            "excess-qty" => Self::ExcessQty,
            "not-owned" => Self::NotOwned,
            _ => return None,
        })
    }
}

fn side(v: &Value) -> Option<OrderSide> {
    match v.get("type").and_then(Value::as_str) {
        Some("sell") => Some(OrderSide::Sell),
        Some("buy") => Some(OrderSide::Buy),
        _ => None,
    }
}

/// The live top-of-book of one listing's tier, as a live check reports it.
#[derive(Debug, Clone, Default, TS)]
#[ts(optional_fields = nullable)]
pub struct LiveBook {
    /// The best online asks, cheapest first, the listing's own excluded.
    #[ts(as = "Option<Vec<Option<f64>>>", optional = nullable)]
    pub sells: Vec<f64>,
    pub low_sell: Option<f64>,
    pub top_buy: Option<f64>,
    /// Set when this lookup failed; the row has no live data.
    pub error: Option<String>,
}

impl LiveBook {
    fn read(v: &Value) -> Self {
        Self {
            sells: page::numbers(v, "sells"),
            low_sell: page::number(v, "low_sell"),
            top_buy: page::number(v, "top_buy"),
            error: page::text(v, "error"),
        }
    }
}

/// Snapshot statistics of the listing's rank, for the rule bases a live check
/// does not report.
#[derive(Debug, Clone, Default, TS)]
#[ts(rename_all = "camelCase", optional_fields = nullable)]
pub struct HealthStats {
    /// A key that is present stands even when it is `null`; only an absent one
    /// lets the live asks stand in, on a rank the snapshot does not describe.
    #[ts(as = "Option<f64>", optional = nullable)]
    pub low5_avg: Stated,
    pub low20_avg: Option<f64>,
    #[ts(rename = "avg48h")]
    pub avg_48h: Option<f64>,
    #[ts(rename = "median90d")]
    pub median_90d: Option<f64>,
    /// Today's median, else the 90-day baseline: what the live-ask read
    /// judges outliers against.
    pub median: Option<f64>,
}

impl HealthStats {
    fn read(v: &Value) -> Self {
        Self {
            low5_avg: v
                .get("low5Avg")
                .map_or(Stated::Absent, |avg| Stated::Given(avg.as_f64())),
            low20_avg: page::number(v, "low20Avg"),
            avg_48h: page::number(v, "avg48h"),
            median_90d: page::number(v, "median90d"),
            median: page::number(v, "median"),
        }
    }
}

/// One of the account's live listings, with what the panel knows about it.
#[derive(Debug, Clone, TS)]
#[ts(rename_all = "camelCase", optional_fields = nullable)]
pub struct HealthInput {
    pub id: String,
    pub slug: String,
    pub name: String,
    /// The per-unit price of the listing's lot.
    #[ts(type = "number")]
    pub platinum: Option<f64>,
    #[ts(type = "number")]
    pub quantity: Option<f64>,
    /// Only a sell listing is assessed.
    #[ts(rename = "type", as = "OrderSide")]
    pub side: Option<OrderSide>,
    /// Items per trade.
    pub per_trade: Option<f64>,
    /// Snapshot 48-hour volume of the listing's rank, for the thin-book mark;
    /// `null` when the snapshot does not describe that rank.
    pub vol: Option<f64>,
    /// Live top-of-book for this listing's tier; `null` = not checked.
    pub live: Option<LiveBook>,
    /// Tradeable copies owned per the latest scan; `null` = unknown.
    pub owned: Option<f64>,
    pub stats: Option<HealthStats>,
}

impl HealthInput {
    fn read(v: &Value) -> Self {
        Self {
            id: page::text(v, "id").unwrap_or_default(),
            slug: page::text(v, "slug").unwrap_or_default(),
            name: page::text(v, "name").unwrap_or_default(),
            platinum: page::number(v, "platinum"),
            quantity: page::number(v, "quantity"),
            side: side(v),
            per_trade: page::number(v, "perTrade"),
            vol: page::number(v, "vol"),
            live: page::object(v, "live").map(LiveBook::read),
            owned: page::number(v, "owned"),
            stats: page::object(v, "stats").map(HealthStats::read),
        }
    }
}

/// What the player asked the health check to flag.
#[derive(Debug, Clone, TS)]
pub struct HealthPrefs {
    pub rule: PriceRule,
    /// Platinum an ask may sit above the rule's target before it is flagged.
    pub tolerance: f64,
    /// A kind this build does not know mutes nothing.
    #[ts(as = "Vec<HealthKind>")]
    pub muted: Vec<HealthKind>,
}

impl HealthPrefs {
    fn read(v: &Value) -> Self {
        Self {
            rule: PriceRule::read(v.get("rule").unwrap_or(&Value::Null)),
            tolerance: page::number(v, "tolerance").unwrap_or(f64::NAN),
            muted: page::list(v, "muted")
                .unwrap_or_default()
                .iter()
                .filter_map(|kind| kind.as_str().and_then(HealthKind::parse))
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HealthIssue {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub kind: HealthKind,
    /// What the listing says now: the unit price for price kinds, the
    /// quantity for quantity kinds.
    pub current: f64,
    /// What to change it to. `0` on `not-owned` means "delete". Price kinds
    /// give the per-unit price of `suggestedLot`.
    pub suggested: f64,
    /// Price kinds only: the whole-platinum lot price a reprice sends.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub suggested_lot: Option<f64>,
    /// `underpriced` only: the book is too thin for the target to be firm.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub thin: Option<bool>,
    /// `overpriced` only: the cheapest live ask, skipped as an outlier.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub ignored_ask: Option<f64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HealthSummary {
    pub overpriced: usize,
    pub underbid: usize,
    pub underpriced: usize,
    pub excess_qty: usize,
    pub not_owned: usize,
    pub total: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
pub struct HealthReport {
    pub issues: Vec<HealthIssue>,
    pub summary: HealthSummary,
}

fn lot_size(per_trade: Option<f64>) -> f64 {
    per_trade
        .filter(|lot| is_safe_integer(*lot) && *lot >= 1.0)
        .unwrap_or(1.0)
}

/// Whole platinum per lot, after rounding to six places as the page did
/// (`Number((unit * lot).toFixed(6))`), so float noise
/// (6.5 x 6 = 39.000000000000004) cannot add a platinum. Only a fraction of at
/// most half a millionth rounds down to the whole number, and no double sits
/// exactly at half, so comparing the fraction against the nearest double to
/// 5e-7 (just under it) decides the ceiling as the rounding would, without
/// formatting a float.
fn whole_lot(unit: f64, lot: f64) -> f64 {
    const HALF_MILLIONTH: f64 = 5e-7;
    let x = unit * lot;
    let floor = x.floor();
    let fraction = x - floor;
    let ceiling = if !x.is_finite() || fraction > HALF_MILLIONTH {
        x.ceil()
    } else if fraction == 0.0 {
        x
    } else {
        floor
    };
    js_max(1.0, ceiling)
}

/// The health verdicts for one listing. Only sell listings are assessed: a buy
/// order is the player's own bid, and "someone asks less than your bid" is a
/// purchase, not a problem.
pub fn assess_listing(o: &HealthInput, prefs: &HealthPrefs) -> Vec<HealthIssue> {
    let mut out = Vec::new();
    if o.side != Some(OrderSide::Sell) {
        return out;
    }
    let platinum = o.platinum.unwrap_or(f64::NAN);
    let issue = |kind, current, suggested| HealthIssue {
        id: o.id.clone(),
        slug: o.slug.clone(),
        name: o.name.clone(),
        kind,
        current,
        suggested,
        suggested_lot: None,
        thin: None,
        ignored_ask: None,
    };

    if let Some(t) = o
        .live
        .as_ref()
        .filter(|t| t.error.as_deref().is_none_or(str::is_empty))
    {
        let stats = o.stats.clone().unwrap_or_default();
        let lot = lot_size(o.per_trade);
        let listed_lot = js_round(platinum * lot);
        let sells = t.sells.as_slice();
        let read = if sells.is_empty() {
            LiveAskRead {
                ask: t.low_sell,
                ignored: None,
            }
        } else {
            reasonable_lowest_ask(sells, stats.median)
        };
        let believable: &[f64] = match read.ask {
            None => &[],
            Some(ask) => {
                let from = sells.iter().position(|s| *s == ask).unwrap_or(0);
                sells.get(from..).unwrap_or_default()
            }
        };
        let live_low5 = (!believable.is_empty())
            .then(|| believable.iter().fold(0.0, |a, b| a + b) / believable.len() as f64);
        let evidence = PriceEvidence {
            lowest_ask: read.ask,
            low5_avg: match stats.low5_avg {
                Stated::Absent => live_low5,
                Stated::Given(value) => value,
            },
            low20_avg: stats.low20_avg,
            avg_48h: stats.avg_48h,
            median_90d: stats.median_90d,
        };
        // A market lot is whole platinum: comparing against the fractional
        // target flagged a 16p ask against 15.4p, and Reprice then sent 16p.
        let target_lot =
            price_from_rule(prefs.rule, &evidence).map(|target| whole_lot(target, lot));
        let priced = |kind, lot_price: f64| HealthIssue {
            suggested_lot: Some(lot_price),
            ..issue(kind, platinum, lot_price / lot)
        };
        match (target_lot, t.top_buy) {
            (Some(target), _) if listed_lot > target + prefs.tolerance * lot => {
                out.push(HealthIssue {
                    ignored_ask: read.ignored,
                    ..priced(HealthKind::Overpriced, target)
                })
            }
            (_, Some(bid)) if platinum < bid => {
                out.push(priced(HealthKind::Underbid, whole_lot(bid, lot)))
            }
            (Some(target), _) => {
                let gap = target / lot - platinum;
                if gap >= MIN_DRIFT_PLAT && gap / platinum >= DRIFT_PCT {
                    out.push(HealthIssue {
                        thin: o.vol.is_some_and(|vol| vol < LIQUID_VOL).then_some(true),
                        ..priced(HealthKind::Underpriced, target)
                    });
                }
            }
            _ => {}
        }
    }

    if let Some(owned) = o.owned {
        let quantity = o.quantity.unwrap_or(f64::NAN);
        if owned <= 0.0 {
            out.push(issue(HealthKind::NotOwned, quantity, 0.0));
        } else if quantity > owned {
            out.push(issue(HealthKind::ExcessQty, quantity, owned));
        }
    }
    out.retain(|i| !prefs.muted.contains(&i.kind));
    out
}

pub fn summarize(issues: &[HealthIssue]) -> HealthSummary {
    let mut s = HealthSummary {
        total: issues.len(),
        ..HealthSummary::default()
    };
    for i in issues {
        let count = match i.kind {
            HealthKind::Overpriced => &mut s.overpriced,
            HealthKind::Underbid => &mut s.underbid,
            HealthKind::Underpriced => &mut s.underpriced,
            HealthKind::ExcessQty => &mut s.excess_qty,
            HealthKind::NotOwned => &mut s.not_owned,
        };
        *count += 1;
    }
    s
}

/// Every listing's verdicts, in listing order, and their tally.
pub fn assess_listings(orders: &[HealthInput], prefs: &HealthPrefs) -> HealthReport {
    let issues: Vec<HealthIssue> = orders
        .iter()
        .flat_map(|o| assess_listing(o, prefs))
        .collect();
    HealthReport {
        summary: summarize(&issues),
        issues,
    }
}

/// The snapshot fields the clearing price reads.
#[derive(Debug, Clone, Default, TS)]
#[ts(optional_fields = nullable)]
pub struct DriftMarket {
    pub vol: Option<f64>,
    pub low_sell: Option<f64>,
    pub avg: Option<f64>,
    pub median_now: Option<f64>,
    pub median_90d: Option<f64>,
}

#[derive(Debug, Clone, TS)]
#[ts(optional_fields = nullable)]
pub struct DriftInput {
    pub id: String,
    pub slug: String,
    pub name: String,
    #[ts(type = "number")]
    pub platinum: Option<f64>,
    /// Only sell listings are analysed; see [`select_drifted`].
    #[ts(rename = "type")]
    pub side: Option<OrderSide>,
    pub m: Option<DriftMarket>,
}

impl DriftInput {
    fn read(v: &Value) -> Self {
        Self {
            id: page::text(v, "id").unwrap_or_default(),
            slug: page::text(v, "slug").unwrap_or_default(),
            name: page::text(v, "name").unwrap_or_default(),
            platinum: page::number(v, "platinum"),
            side: side(v),
            m: page::object(v, "m").map(|m| DriftMarket {
                vol: page::number(m, "vol"),
                low_sell: page::number(m, "low_sell"),
                avg: page::number(m, "avg"),
                median_now: page::number(m, "median_now"),
                median_90d: page::number(m, "median_90d"),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum DriftKind {
    Overpriced,
    Underpriced,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
pub struct DriftRow {
    pub id: String,
    pub slug: String,
    pub name: String,
    /// What the listing asks now.
    pub listed: f64,
    /// What it would be repriced to.
    pub suggested: f64,
    /// Signed difference, suggested - listed.
    pub delta: f64,
    /// Magnitude of the move as a fraction of the listed price.
    pub delta_pct: f64,
    pub kind: DriftKind,
    /// Below the liquidity floor the book is thin enough that the suggestion
    /// is weak evidence: the view de-emphasises rather than hides, as the sell
    /// picks treat the same threshold.
    pub thin: bool,
}

/// Listings whose price has drifted far enough from the snapshot to be worth
/// repricing, biggest platinum swing first, which is the order in which
/// repricing pays.
///
/// The reference is the clearing price, the same clamp the sell score uses,
/// so drift inherits its refusal to chase a troll 1p ask or trust a lone
/// aspirational one on a thin book. The snapshot cannot tell whose order is
/// whose, so `low_sell` includes the listing itself: the lowest ask reports no
/// drift, which is right, but as a consequence of the data. Buy orders are
/// skipped rather than inverted: every reference here describes what a SELLER
/// clears at.
pub fn select_drifted(orders: &[DriftInput], limit: usize) -> Vec<DriftRow> {
    let mut rows: Vec<DriftRow> = orders
        .iter()
        .filter(|o| o.side != Some(OrderSide::Buy))
        .filter_map(|o| {
            let listed = num(o.platinum);
            let m = o.m.as_ref().filter(|_| listed > 0.0)?;
            let suggested = sell_priority::clearing_price(&PricedEntry {
                vol: num(m.vol),
                low_sell: num(m.low_sell),
                avg: num(m.avg),
                median_now: num(m.median_now),
                median_90d: num(m.median_90d),
            })
            .ceil();
            if suggested <= 0.0 {
                return None;
            }
            let delta = suggested - listed;
            let delta_pct = delta.abs() / listed;
            // Written as the negated pass test so a NaN share passes, as it did.
            if delta.abs() < MIN_DRIFT_PLAT || delta_pct < DRIFT_PCT {
                return None;
            }
            Some(DriftRow {
                id: o.id.clone(),
                slug: o.slug.clone(),
                name: o.name.clone(),
                listed,
                suggested,
                delta,
                delta_pct,
                kind: if delta < 0.0 {
                    DriftKind::Overpriced
                } else {
                    DriftKind::Underpriced
                },
                thin: num(m.vol) < LIQUID_VOL,
            })
        })
        .collect();
    sort_stably(&mut rows, |a, b| a.delta.abs() > b.delta.abs());
    rows.truncate(limit);
    rows
}

/// What an interrupted listing batch should say for itself. A `pending` item
/// was never sent, so Resume sends it; an `uncertain_mutation` item may already
/// have reached the market, so Resume checks current orders before it does
/// anything with one. A batch can hold both, and says so.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
pub struct InterruptedBatch {
    pub pending: usize,
    pub uncertain: usize,
    pub done: usize,
    /// Whether Resume may be offered: unsent work, or an unknown outcome a
    /// check of current orders may resolve.
    pub resumable: bool,
}

/// The batch's leftovers, or `None` when nothing needs saying: a plan whose
/// items all finished has no interrupted work, and an empty banner is worse
/// than none. A status this build does not know counts as nothing.
pub fn interrupted_batch<S: AsRef<str>>(statuses: &[S]) -> Option<InterruptedBatch> {
    let count = |wanted: &str| statuses.iter().filter(|s| s.as_ref() == wanted).count();
    let (pending, uncertain) = (count("pending"), count("uncertain_mutation"));
    (pending > 0 || uncertain > 0).then(|| InterruptedBatch {
        pending,
        uncertain,
        done: count("ok"),
        resumable: pending > 0 || uncertain > 0,
    })
}

fn each<T>(v: &Value, key: &str, read: impl Fn(&Value) -> T) -> Vec<T> {
    page::list(v, key)
        .unwrap_or_default()
        .iter()
        .map(read)
        .collect()
}

#[derive(Debug, Clone, TS)]
pub struct PriceRuleRequest {
    pub rule: PriceRule,
    pub evidence: PriceEvidence,
}

impl PriceRuleRequest {
    fn read(v: &Value) -> Self {
        Self {
            rule: PriceRule::read(v.get("rule").unwrap_or(&Value::Null)),
            evidence: PriceEvidence::read(v.get("evidence").unwrap_or(&Value::Null)),
        }
    }
}

#[derive(Debug, Clone, TS)]
pub struct LiveAskRequest {
    #[ts(as = "Vec<Option<f64>>")]
    pub asks: Vec<f64>,
    #[ts(optional = nullable)]
    pub median: Option<f64>,
}

impl LiveAskRequest {
    fn read(v: &Value) -> Self {
        Self {
            asks: page::numbers(v, "asks"),
            median: page::number(v, "median"),
        }
    }
}

#[derive(Debug, Clone, TS)]
pub struct HealthRequest {
    pub orders: Vec<HealthInput>,
    pub prefs: HealthPrefs,
}

impl HealthRequest {
    fn read(v: &Value) -> Self {
        Self {
            orders: each(v, "orders", HealthInput::read),
            prefs: HealthPrefs::read(v.get("prefs").unwrap_or(&Value::Null)),
        }
    }
}

#[derive(Debug, Clone, TS)]
pub struct DriftRequest {
    pub orders: Vec<DriftInput>,
    /// Defaults to [`DRIFT_LIMIT`].
    #[ts(optional)]
    pub limit: Option<usize>,
}

impl DriftRequest {
    fn read(v: &Value) -> Self {
        Self {
            orders: each(v, "orders", DriftInput::read),
            limit: v
                .get("limit")
                .and_then(Value::as_u64)
                .and_then(|n| usize::try_from(n).ok()),
        }
    }
}

#[derive(Debug, Clone, TS)]
pub struct BatchRequest {
    /// Each item's journal status; a status this build does not know counts
    /// as nothing.
    pub statuses: Vec<String>,
}

impl BatchRequest {
    fn read(v: &Value) -> Self {
        Self {
            statuses: each(v, "statuses", |s| s.as_str().unwrap_or_default().to_owned()),
        }
    }
}

deserialize_by_reading!(
    PriceRuleRequest,
    LiveAskRequest,
    HealthRequest,
    DriftRequest,
    BatchRequest
);

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    #[test]
    fn unit_and_lot_prices_match_the_shared_fixture() {
        let cases: Vec<Value> = serde_json::from_str(include_str!(
            "../../../tests/fixtures/trade-session/prices.json"
        ))
        .unwrap();
        for case in cases {
            // The page reads an absent lot as one, and anything but a number as no lot.
            let lot = match &case["per_trade"] {
                Value::Null => 1.0,
                v => v.as_f64().unwrap_or(f64::NAN),
            };
            let platinum = case["platinum"].as_f64().unwrap();
            assert_eq!(unit_price(platinum, lot), case["unit"].as_f64(), "{case}");
            if let Some(unit) = case["unit"].as_f64() {
                assert_eq!(lot_price(unit, lot), Some(platinum), "{case}");
            }
        }
        assert_eq!(lot_price(6.5, 6.0), Some(39.0));
        assert_eq!(lot_price(f64::NAN, 1.0), None);
        assert_eq!(unit_price(f64::INFINITY, 1.0), None);
    }

    #[test]
    fn js_rounding_rounds_halves_up() {
        assert_eq!(js_round(2.5), 3.0);
        assert_eq!(js_round(-2.5), -2.0);
        assert_eq!(js_round(0.499_999_999_999_999_94), 0.0);
        assert!(js_round(f64::NAN).is_nan());
        assert!(js_max(1.0, f64::NAN).is_nan());
    }

    #[test]
    fn a_whole_lot_rounds_to_six_places_before_the_ceiling() {
        assert_eq!(whole_lot(6.5, 6.0), 39.0);
        assert_eq!(whole_lot(2.2 - 1.0, 5.0), 6.0);
        assert_eq!(whole_lot(15.4, 1.0), 16.0);
        assert_eq!(whole_lot(1.000_000_4, 1.0), 1.0);
        assert_eq!(whole_lot(2.000_000_6, 1.0), 3.0);
        assert_eq!(whole_lot(0.2, 1.0), 1.0);
        assert!(whole_lot(f64::NAN, 1.0).is_nan());
        assert_eq!(whole_lot(f64::INFINITY, 1.0), f64::INFINITY);
    }

    fn rule(basis: PriceBasis, offset: f64) -> PriceRule {
        PriceRule {
            basis: Some(basis),
            offset,
        }
    }

    fn evidence() -> PriceEvidence {
        PriceEvidence {
            lowest_ask: Some(20.0),
            low5_avg: Some(22.4),
            low20_avg: Some(27.0),
            avg_48h: Some(25.2),
            median_90d: Some(30.0),
        }
    }

    #[test]
    fn a_rule_adds_its_offset_to_the_basis_unrounded() {
        use PriceBasis::*;
        assert_eq!(
            price_from_rule(rule(Low20Avg, 1.0), &evidence()),
            Some(28.0)
        );
        assert_eq!(
            price_from_rule(rule(LowestAsk, 0.0), &evidence()),
            Some(20.0)
        );
        assert!((price_from_rule(rule(Low5Avg, 1.0), &evidence()).unwrap() - 23.4).abs() < 1e-9);
        assert!((price_from_rule(rule(Avg48h, -2.0), &evidence()).unwrap() - 23.2).abs() < 1e-9);
        assert_eq!(
            price_from_rule(rule(Median90d, 1.0), &evidence()),
            Some(31.0)
        );
    }

    #[test]
    fn a_rule_never_goes_under_1p_or_under_a_sub_1p_basis() {
        let ask = |v| PriceEvidence {
            lowest_ask: Some(v),
            ..PriceEvidence::default()
        };
        assert_eq!(
            price_from_rule(rule(PriceBasis::LowestAsk, -50.0), &ask(3.0)),
            Some(1.0)
        );
        assert_eq!(
            price_from_rule(rule(PriceBasis::LowestAsk, -2.0), &ask(0.5)),
            Some(0.5)
        );
    }

    #[test]
    fn a_rule_suggests_nothing_on_an_unknown_basis() {
        let only_ask = PriceEvidence {
            lowest_ask: Some(20.0),
            ..PriceEvidence::default()
        };
        assert_eq!(
            price_from_rule(rule(PriceBasis::Median90d, 1.0), &only_ask),
            None
        );
        let zero = PriceEvidence {
            low5_avg: Some(0.0),
            ..PriceEvidence::default()
        };
        assert_eq!(price_from_rule(rule(PriceBasis::Low5Avg, 1.0), &zero), None);
        let nan = PriceEvidence {
            avg_48h: Some(f64::NAN),
            ..PriceEvidence::default()
        };
        assert_eq!(price_from_rule(rule(PriceBasis::Avg48h, 0.0), &nan), None);
        assert_eq!(
            price_from_rule(rule(PriceBasis::LowestAsk, 0.0), &PriceEvidence::default()),
            None
        );
    }

    fn read(ask: Option<f64>, ignored: Option<f64>) -> LiveAskRead {
        LiveAskRead { ask, ignored }
    }

    #[test]
    fn the_live_ask_read_skips_a_troll_undercut() {
        assert_eq!(
            reasonable_lowest_ask(&[2.0, 14.0, 15.0], Some(18.0)),
            read(Some(14.0), Some(2.0))
        );
        assert_eq!(
            reasonable_lowest_ask(&[1.0, 2.0, 14.0], Some(18.0)),
            read(Some(14.0), Some(1.0))
        );
        // Believable, at the third exactly too.
        assert_eq!(
            reasonable_lowest_ask(&[12.0, 14.0], Some(18.0)),
            read(Some(12.0), None)
        );
        assert_eq!(
            reasonable_lowest_ask(&[6.0, 14.0], Some(18.0)),
            read(Some(6.0), None)
        );
        // Low all the way up is a move, not a troll.
        assert_eq!(
            reasonable_lowest_ask(&[3.0, 4.0, 5.0], Some(30.0)),
            read(Some(3.0), None)
        );
        assert_eq!(
            reasonable_lowest_ask(&[1.0, 14.0], None),
            read(Some(1.0), None)
        );
        assert_eq!(
            reasonable_lowest_ask(&[1.0, 14.0], Some(0.0)),
            read(Some(1.0), None)
        );
        assert_eq!(reasonable_lowest_ask(&[], Some(18.0)), read(None, None));
    }

    // The listing-health cases are the page's own tests, in its JSON shapes,
    // so they also cover how each field is read off the wire.
    fn listing(over: Value) -> HealthInput {
        let mut base = json!({ "id": "o1", "slug": "x", "name": "X", "platinum": 20, "quantity": 1, "type": "sell", "live": null, "owned": null });
        for (k, v) in over.as_object().unwrap() {
            base[k] = v.clone();
        }
        HealthInput::read(&base)
    }

    fn live(over: Value) -> Value {
        let mut base = json!({ "slug": "x", "sells": [], "buys": [], "low_sell": null, "top_buy": null, "own_ask": null, "own_bid": null, "error": null });
        for (k, v) in over.as_object().unwrap() {
            base[k] = v.clone();
        }
        base
    }

    fn prefs(value: Value) -> HealthPrefs {
        let mut base =
            json!({ "rule": { "basis": "lowest-ask", "offset": 0 }, "tolerance": 0, "muted": [] });
        for (k, v) in value.as_object().unwrap() {
            base[k] = v.clone();
        }
        HealthPrefs::read(&base)
    }

    fn assess(over: Value) -> Value {
        assess_with(over, json!({}))
    }

    fn assess_with(over: Value, p: Value) -> Value {
        serde_json::to_value(assess_listing(&listing(over), &prefs(p))).unwrap()
    }

    fn kinds(issues: &Value) -> Vec<&str> {
        issues
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["kind"].as_str().unwrap())
            .collect()
    }

    #[test]
    fn health_flags_an_ask_above_the_lowest_other_ask_and_matches_it() {
        let issues = assess(
            json!({ "platinum": 20, "live": live(json!({ "low_sell": 15, "sells": [15, 18] })) }),
        );
        assert_eq!(issues[0]["kind"], "overpriced");
        assert_eq!(issues[0]["suggested"], 15.0);
    }

    #[test]
    fn health_flags_a_bid_above_the_ask() {
        let issues = assess(
            json!({ "platinum": 10, "live": live(json!({ "low_sell": 12, "top_buy": 11 })) }),
        );
        assert_eq!(issues[0]["kind"], "underbid");
        assert_eq!(issues[0]["suggested"], 11.0);
    }

    #[test]
    fn health_is_quiet_on_a_competitive_ask_a_failed_check_and_a_buy_order() {
        assert_eq!(
            assess(
                json!({ "platinum": 12, "live": live(json!({ "low_sell": 12, "top_buy": 9 })) })
            ),
            json!([])
        );
        assert_eq!(
            assess(
                json!({ "platinum": 12, "live": live(json!({ "low_sell": 14, "top_buy": 9 })) })
            ),
            json!([])
        );
        assert_eq!(
            assess(
                json!({ "platinum": 99, "live": live(json!({ "low_sell": 1, "error": "HTTP 404" })) })
            ),
            json!([])
        );
        assert_eq!(
            assess(
                json!({ "type": "buy", "platinum": 1, "live": live(json!({ "low_sell": 5, "top_buy": 4 })) })
            ),
            json!([])
        );
    }

    #[test]
    fn health_checks_quantity_against_the_scan() {
        let q = assess(json!({ "quantity": 5, "owned": 2 }));
        assert_eq!(
            (q[0]["kind"].as_str(), q[0]["suggested"].as_f64()),
            (Some("excess-qty"), Some(2.0))
        );
        let g = assess(json!({ "quantity": 1, "owned": 0 }));
        assert_eq!(
            (g[0]["kind"].as_str(), g[0]["suggested"].as_f64()),
            (Some("not-owned"), Some(0.0))
        );
        assert_eq!(g[0].get("suggestedLot"), None);
        assert_eq!(assess(json!({ "quantity": 1, "owned": null })), json!([]));
        assert_eq!(assess(json!({ "quantity": 2, "owned": 2 })), json!([]));
        let both = assess(
            json!({ "platinum": 20, "quantity": 3, "owned": 1, "live": live(json!({ "low_sell": 15 })) }),
        );
        assert_eq!(kinds(&both), ["overpriced", "excess-qty"]);
    }

    #[test]
    fn health_summary_counts_by_kind() {
        let orders: Vec<HealthInput> = [
            json!({ "id": "a", "platinum": 20, "live": live(json!({ "low_sell": 15 })) }),
            json!({ "id": "b", "platinum": 5, "live": live(json!({ "low_sell": 9, "top_buy": 7 })) }),
            json!({ "id": "c", "quantity": 3, "owned": 0 }),
        ]
        .into_iter()
        .map(listing)
        .collect();
        let report = assess_listings(&orders, &prefs(json!({})));
        assert_eq!(
            serde_json::to_value(report.summary).unwrap(),
            json!({ "overpriced": 1, "underbid": 1, "underpriced": 0, "excessQty": 0, "notOwned": 1, "total": 3 })
        );
    }

    #[test]
    fn health_targets_the_players_rule() {
        let median = json!({ "rule": { "basis": "median-90d", "offset": 1 } });
        let issues = assess_with(
            json!({ "platinum": 40, "live": live(json!({ "low_sell": 30 })), "stats": { "median90d": 34 } }),
            median.clone(),
        );
        assert_eq!(
            (issues[0]["kind"].as_str(), issues[0]["suggested"].as_f64()),
            (Some("overpriced"), Some(35.0))
        );
        assert_eq!(
            assess_with(
                json!({ "platinum": 35, "live": live(json!({ "low_sell": 30 })), "stats": { "median90d": 34 } }),
                median.clone()
            ),
            json!([])
        );
        assert_eq!(
            assess_with(
                json!({ "platinum": 99, "live": live(json!({ "low_sell": 30 })), "stats": {} }),
                median
            ),
            json!([])
        );
    }

    #[test]
    fn health_tolerance_and_mutes() {
        let o = json!({ "platinum": 17, "live": live(json!({ "low_sell": 15 })) });
        assert_eq!(assess_with(o.clone(), json!({ "tolerance": 2 })), json!([]));
        assert_eq!(
            assess_with(o, json!({ "tolerance": 1 }))[0]["kind"],
            "overpriced"
        );
        let both = json!({ "quantity": 5, "owned": 2, "platinum": 20, "live": live(json!({ "low_sell": 15 })) });
        assert_eq!(
            kinds(&assess_with(both, json!({ "muted": ["overpriced"] }))),
            ["excess-qty"]
        );
    }

    #[test]
    fn health_skips_a_troll_undercut_on_the_live_book() {
        let troll = live(json!({ "low_sell": 2, "sells": [2, 14, 15] }));
        let issues = assess(json!({ "platinum": 20, "stats": { "median": 18 }, "live": troll }));
        assert_eq!(
            (
                issues[0]["kind"].as_str(),
                issues[0]["suggested"].as_f64(),
                issues[0]["ignoredAsk"].as_f64()
            ),
            (Some("overpriced"), Some(14.0), Some(2.0))
        );
        let above = live(json!({ "low_sell": 2, "sells": [2, 14] }));
        assert_eq!(
            assess(json!({ "platinum": 14, "stats": { "median": 18 }, "live": above.clone() })),
            json!([])
        );
        let under = assess_with(
            json!({ "platinum": 20, "stats": { "median": 18 }, "live": above }),
            json!({ "rule": { "basis": "lowest-ask", "offset": -1 } }),
        );
        assert_eq!(
            (
                under[0]["suggested"].as_f64(),
                under[0]["ignoredAsk"].as_f64()
            ),
            (Some(13.0), Some(2.0))
        );
    }

    #[test]
    fn health_compares_whole_platinum_lots() {
        let low5 = json!({ "rule": { "basis": "low5-avg", "offset": 0 } });
        let book = live(json!({ "low_sell": 15, "sells": [15, 16] }));
        assert_eq!(
            assess_with(
                json!({ "platinum": 16, "stats": { "low5Avg": 15.4 }, "live": book.clone() }),
                low5.clone()
            ),
            json!([])
        );
        let i = assess_with(
            json!({ "platinum": 17, "stats": { "low5Avg": 15.4 }, "live": book }),
            low5,
        );
        assert_eq!(
            (
                i[0]["kind"].as_str(),
                i[0]["suggested"].as_f64(),
                i[0]["suggestedLot"].as_f64()
            ),
            (Some("overpriced"), Some(16.0), Some(16.0))
        );
        let bulk = live(json!({ "low_sell": 6.5, "sells": [6.5] }));
        let j = assess(json!({ "platinum": 40.0 / 6.0, "perTrade": 6, "live": bulk.clone() }));
        assert_eq!(
            (
                j[0]["kind"].as_str(),
                j[0]["suggestedLot"].as_f64(),
                j[0]["suggested"].as_f64()
            ),
            (Some("overpriced"), Some(39.0), Some(6.5))
        );
        assert_eq!(
            assess(json!({ "platinum": 39.0 / 6.0, "perTrade": 6, "live": bulk })),
            json!([])
        );
        // (2.2 - 1) x 5 is 6.000000000000001 in floating point.
        let noisy = json!({ "rule": { "basis": "low5-avg", "offset": -1 } });
        let k = assess_with(
            json!({ "platinum": 7.0 / 5.0, "perTrade": 5, "stats": { "low5Avg": 2.2 }, "live": live(json!({ "low_sell": 2, "sells": [2] })) }),
            noisy,
        );
        assert_eq!(
            (k[0]["kind"].as_str(), k[0]["suggestedLot"].as_f64()),
            (Some("overpriced"), Some(6.0))
        );
    }

    #[test]
    fn health_on_a_rank_the_snapshot_does_not_describe() {
        let arcane = |platinum: f64, stats: Value| json!({ "platinum": platinum, "stats": stats, "live": live(json!({ "low_sell": 20, "sells": [20, 22, 24] })) });
        let low5 = json!({ "rule": { "basis": "low5-avg", "offset": 0 } });
        assert_eq!(
            assess_with(arcane(21.0, json!({ "median": 2 })), low5.clone()),
            json!([])
        );
        let i = assess_with(arcane(30.0, json!({ "median": 2 })), low5.clone());
        assert_eq!(
            (i[0]["kind"].as_str(), i[0]["suggested"].as_f64()),
            (Some("overpriced"), Some(22.0))
        );
        let median = json!({ "rule": { "basis": "median-90d", "offset": 0 } });
        assert_eq!(
            assess_with(arcane(21.0, json!({ "median": 2 })), median.clone()),
            json!([])
        );
        let described = json!({ "low5Avg": 26, "median90d": 27, "median": 25 });
        let j = assess_with(arcane(21.0, described.clone()), low5);
        assert_eq!(
            (j[0]["kind"].as_str(), j[0]["suggested"].as_f64()),
            (Some("underpriced"), Some(26.0))
        );
        let k = assess_with(arcane(21.0, described), median);
        assert_eq!(
            (k[0]["kind"].as_str(), k[0]["suggested"].as_f64()),
            (Some("underpriced"), Some(27.0))
        );
    }

    #[test]
    fn health_keeps_a_present_but_unknown_snapshot_average() {
        // Object spread: a stats key that is there, even unknown, replaces the
        // live average; only an absent one lets the live asks stand in.
        let low5 = json!({ "rule": { "basis": "low5-avg", "offset": 0 } });
        let book = live(json!({ "low_sell": 20, "sells": [20, 22, 24] }));
        assert_eq!(
            assess_with(
                json!({ "platinum": 30, "stats": { "low5Avg": null }, "live": book.clone() }),
                low5.clone()
            ),
            json!([])
        );
        assert_eq!(
            kinds(&assess_with(
                json!({ "platinum": 30, "stats": {}, "live": book }),
                low5
            )),
            ["overpriced"]
        );
    }

    #[test]
    fn health_flags_an_ask_well_under_the_target() {
        let book =
            |low: f64, bid: Value| live(json!({ "low_sell": low, "sells": [low], "top_buy": bid }));
        let i = assess(json!({ "platinum": 10, "live": book(25.0, Value::Null) }));
        assert_eq!(
            i,
            json!([{ "id": "o1", "slug": "x", "name": "X", "kind": "underpriced", "current": 10.0, "suggested": 25.0, "suggestedLot": 25.0 }])
        );
        assert_eq!(
            assess(json!({ "platinum": 23, "live": book(25.0, Value::Null) })),
            json!([])
        );
        assert_eq!(
            assess(json!({ "platinum": 100, "live": book(110.0, Value::Null) })),
            json!([])
        );
        assert_eq!(
            kinds(&assess(
                json!({ "platinum": 10, "live": book(25.0, json!(12)) })
            )),
            ["underbid"]
        );
        assert_eq!(
            assess(json!({ "platinum": 10, "vol": 2, "live": book(25.0, Value::Null) }))[0]["thin"],
            true
        );
        assert_eq!(
            assess(json!({ "platinum": 10, "vol": null, "live": book(25.0, Value::Null) }))[0]
                .get("thin"),
            None
        );
        assert_eq!(
            assess_with(
                json!({ "platinum": 10, "live": book(25.0, Value::Null) }),
                json!({ "muted": ["underpriced"] })
            ),
            json!([])
        );
    }

    fn liquid(price: f64) -> Value {
        json!({ "vol": 20, "low_sell": price, "avg": price, "median_now": price, "median_90d": price })
    }

    fn drift(orders: &[Value], limit: usize) -> Vec<DriftRow> {
        let orders: Vec<DriftInput> = orders
            .iter()
            .map(|over| {
                let mut base = json!({ "id": "o1", "slug": "mag_prime_set", "name": "Mag Prime Set", "platinum": 100, "type": "sell", "m": liquid(100.0) });
                for (k, v) in over.as_object().unwrap() {
                    base[k] = v.clone();
                }
                DriftInput::read(&base)
            })
            .collect();
        select_drifted(&orders, limit)
    }

    #[test]
    fn drift_reports_a_listing_the_market_moved_away_from() {
        assert_eq!(drift(&[json!({})], DRIFT_LIMIT), []);
        let over = drift(&[json!({ "m": liquid(60.0) })], DRIFT_LIMIT);
        assert_eq!(
            (
                over[0].kind,
                over[0].listed,
                over[0].suggested,
                over[0].delta
            ),
            (DriftKind::Overpriced, 100.0, 60.0, -40.0)
        );
        assert!((over[0].delta_pct - 0.4).abs() < 1e-9);
        let under = drift(&[json!({ "platinum": 50 })], DRIFT_LIMIT);
        assert_eq!(
            (under[0].kind, under[0].suggested, under[0].delta),
            (DriftKind::Underpriced, 100.0, 50.0)
        );
    }

    #[test]
    fn drift_needs_both_gates() {
        assert_eq!(drift(&[json!({ "m": liquid(110.0) })], DRIFT_LIMIT), []);
        assert_eq!(
            drift(&[json!({ "platinum": 5, "m": liquid(7.0) })], DRIFT_LIMIT),
            []
        );
    }

    #[test]
    fn drift_skips_buy_orders_trolls_and_nonsense() {
        assert_eq!(
            drift(&[json!({ "type": "buy", "m": liquid(60.0) })], DRIFT_LIMIT),
            []
        );
        let troll =
            json!({ "vol": 20, "low_sell": 1, "avg": 38, "median_now": 38, "median_90d": 38 });
        assert_eq!(
            drift(&[json!({ "platinum": 40, "m": troll })], DRIFT_LIMIT),
            []
        );
        assert_eq!(drift(&[json!({ "m": null })], DRIFT_LIMIT), []);
        assert_eq!(drift(&[json!({ "platinum": 0 })], DRIFT_LIMIT), []);
        assert_eq!(
            drift(
                &[json!({ "platinum": null, "m": liquid(60.0) })],
                DRIFT_LIMIT
            ),
            []
        );
        assert_eq!(drift(&[], DRIFT_LIMIT), []);
    }

    #[test]
    fn drift_marks_a_thin_book_and_sorts_by_the_platinum_swing() {
        let thin =
            json!({ "vol": 1, "low_sell": 60, "avg": 60, "median_now": 60, "median_90d": 60 });
        assert!(drift(&[json!({ "m": thin })], DRIFT_LIMIT)[0].thin);
        let rows = drift(
            &[
                json!({ "id": "small", "platinum": 10, "m": liquid(20.0) }),
                json!({ "id": "big", "platinum": 400, "m": liquid(200.0) }),
            ],
            DRIFT_LIMIT,
        );
        assert_eq!(
            rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["big", "small"]
        );
        let many: Vec<Value> = (0..10)
            .map(|i| json!({ "id": format!("o{i}"), "m": liquid(50.0) }))
            .collect();
        let first = drift(&many, 3);
        assert_eq!(
            first.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["o0", "o1", "o2"]
        );
    }

    #[test]
    fn an_interrupted_batch_reports_what_is_left() {
        assert_eq!(interrupted_batch::<&str>(&[]), None);
        assert_eq!(interrupted_batch(&["ok", "ok"]), None);
        assert_eq!(interrupted_batch(&["unexpected"]), None);
        assert_eq!(
            interrupted_batch(&["ok", "pending"]),
            Some(InterruptedBatch {
                pending: 1,
                uncertain: 0,
                done: 1,
                resumable: true
            })
        );
        assert_eq!(
            interrupted_batch(&["ok", "uncertain_mutation", "uncertain_mutation"]),
            Some(InterruptedBatch {
                pending: 0,
                uncertain: 2,
                done: 1,
                resumable: true
            })
        );
        assert_eq!(
            interrupted_batch(&["pending", "uncertain_mutation"]),
            Some(InterruptedBatch {
                pending: 1,
                uncertain: 1,
                done: 0,
                resumable: true
            })
        );
    }
}
