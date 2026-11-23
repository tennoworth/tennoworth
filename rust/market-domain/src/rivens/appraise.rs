//! Riven appraisal, and the reroll decision.
//!
//! WHAT THIS DELIBERATELY DOES NOT DO: guess what a riven is worth from its
//! stats. That needs per-stat roll ranges and a model of which stats buyers
//! want, and we have neither - DE publishes dispositions, not roll ranges. It
//! is why no riven price checker is trusted.
//!
//! WHAT IT ALSO NO LONGER DOES: fit a normal distribution to DE's summary
//! statistics. The first version of this rule did, and it was wrong in the
//! way that matters. Riven prices are bounded below at zero and heavily
//! right-skewed - a weapon with 90 sales at 10p and 10 at 1,000p has a mean of
//! 109 and a standard deviation of 297 - and a normal fit to those two numbers
//! reported a 10p offer as the 37th percentile with a 63% chance that a reroll
//! beats it, and priced the losing outcomes at MINUS 194p. Every one of those
//! numbers is impossible or backwards. Five summary statistics do not identify
//! a distribution this skewed, and no parametric fit rescues that.
//!
//! So this reports only what those five numbers actually support: where an
//! offer sits against the landmarks DE gives us (min, median, average, max),
//! how skewed the market is, and what rerolling costs. The price still comes
//! from the USER - an offer they received, or their own estimate.
//!
//! Parity with the TypeScript rules this replaced: tests/fixtures/rivens/appraise.json.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// DE's `pop` is a relative popularity score on a 0-100 scale, not a count of
/// trades: each week exactly one row is 100, and rows like AX-52 rolled
/// (pop 5, max 3,800p, average 182p) could not exist if it counted sales. It
/// says a band is thin, not how thin. The old gate read it as a count and hid
/// any band under 12, which on the 2026-10-08 PC feed was 98% of them. At or
/// under this score (the floor, held by over half the rows) the band is shown
/// with a warning instead, because no count exists to gate on. The threshold is
/// a product judgement.
pub const LOW_POPULARITY: f64 = 1.0;

/// Kuva per reroll. Climbs with the reroll count and caps.
///
/// In-game values, and the reason "just reroll it" is not free advice: a riven
/// at cap costs 3,500 kuva a spin.
pub const REROLL_KUVA: [u32; 10] = [900, 1000, 1200, 1400, 1700, 2000, 2350, 2750, 3150, 3500];

/// Update 44.0: cycling with a manually locked trait costs double. A spliced
/// trait is also fixed in place but does not double the cost.
pub const LOCKED_REROLL_FACTOR: u32 = 2;

/// Ratio of mean to median at or above which the average is not a typical
/// price and the median is the number to quote. Riven markets are routinely
/// 3x or worse. A product judgement - see [`LOWBALL_FRACTION`].
pub const SKEWED: f64 = 1.5;

/// How far under the median an offer has to sit before it is a lowball rather
/// than ordinary haggling.
///
/// A PRODUCT judgement, not something DE's statistics imply - stated plainly
/// because the rest of this module is careful about that distinction. Same goes
/// for [`SKEWED`]. Both are tuneable opinions about presentation; neither is
/// dressed up as a derived quantity.
pub const LOWBALL_FRACTION: f64 = 0.75;

const DAY_MS: f64 = 86_400_000.0;

/// One DE weekly band as the page reads it: each field as `Number(x) || 0`.
#[derive(Debug, Clone, Copy, Default, Deserialize, TS)]
pub struct RivenTier {
    pub avg: f64,
    pub median: f64,
    pub min: f64,
    pub max: f64,
    pub stddev: f64,
    pub pop: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, TS)]
pub struct Distribution {
    pub median: f64,
    pub avg: f64,
    pub min: f64,
    pub max: f64,
    pub stddev: f64,
    /// DE's 0-100 relative popularity - see [`LOW_POPULARITY`].
    pub pop: f64,
}

/// Where an offer sits against the landmarks DE actually publishes. No model,
/// no interpolation - each of these is directly checkable against the feed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum Placement {
    /// Under the cheapest sale DE saw.
    BelowObserved,
    /// Between the minimum and the median.
    Bottom,
    /// Between the median and the average.
    Middle,
    /// Above the average, still within the observed range.
    Upper,
    /// Over the dearest sale DE saw.
    AboveObserved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum PriceVerdict {
    Below,
    Fair,
    Above,
    Outlier,
}

/// What can be said about rerolling, which is less than you would like.
///
/// There is no probability here and no direction either. An earlier version
/// returned a "lean" - likely gains / likely loses / coin flip - and that was
/// still a probability claim wearing different clothes: a reroll draws from the
/// set of POSSIBLE rolls while DE publishes the SOLD ones, so no comparison
/// against the sold median establishes what a new roll will do. The band width
/// that separated the three cases was arbitrary on top of that.
///
/// So this reports two facts and stops: what the spin costs, and where the
/// offer sits relative to the weapon's median. The reader draws the inference.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RerollRead {
    /// Kuva this spin costs.
    pub kuva: u32,
    /// The weapon's median trade.
    pub median: f64,
    /// Whether the offer is above that median. A fact about two published
    /// numbers, not a forecast.
    pub above_median: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum Unavailable {
    NoData,
}

/// The limits of the model, worded by the view: `band` (a weapon band, not
/// this roll; popularity is a score, not a trade count), `desirability`,
/// `spliced` (the band mixes spliced and classic rivens since Update 44.1),
/// `rare` (few trades), `skewed` (average far above the median).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum AppraisalCaveat {
    NoData,
    Band,
    Desirability,
    Spliced,
    Rare,
    Skewed,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
pub struct Appraisal {
    pub dist: Option<Distribution>,
    /// Set when DE published no usable price for this tier.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub unavailable: Option<Unavailable>,
    pub placement: Option<Placement>,
    pub verdict: Option<PriceVerdict>,
    pub reroll: Option<RerollRead>,
    /// True when the average is dragged well above the median - the average is
    /// then not a typical price and the UI must lead with the median.
    pub skewed: bool,
    /// Notes the UI must show alongside any number - the limits of the model.
    pub caveats: Vec<AppraisalCaveat>,
}

/// Kuva for the next spin. A fractional count reads as the spins completed,
/// and a negative one as a fresh riven.
pub fn reroll_cost(rerolls: f64, locked: bool) -> u32 {
    let last = REROLL_KUVA.len() - 1;
    let step = rerolls.floor().max(0.0).min(last as f64) as usize;
    let base = REROLL_KUVA.get(step).or(REROLL_KUVA.last()).copied().unwrap_or_default();
    if locked {
        base * LOCKED_REROLL_FACTOR
    } else {
        base
    }
}

/// A usable distribution, or None when DE published no price for the tier.
pub fn distribution_of(tier: Option<&RivenTier>) -> Option<Distribution> {
    let tier = tier?;
    if tier.median <= 0.0 {
        return None;
    }
    Some(Distribution {
        median: tier.median,
        avg: tier.avg,
        min: tier.min,
        max: tier.max,
        stddev: tier.stddev,
        pop: tier.pop,
    })
}

/// Ratio of mean to median - how far a few large sales drag the average.
pub fn skew_of(dist: &Distribution) -> Option<f64> {
    (dist.median > 0.0 && dist.avg > 0.0).then(|| dist.avg / dist.median)
}

pub fn placement_of(price: f64, dist: &Distribution) -> Placement {
    if dist.min > 0.0 && price < dist.min {
        return Placement::BelowObserved;
    }
    if dist.max > 0.0 && price > dist.max {
        return Placement::AboveObserved;
    }
    if price < dist.median {
        return Placement::Bottom;
    }
    // An offer AT the median is the typical price by definition. It has to be
    // tested before the skew branch: on a flat market where the average equals
    // the median, falling through would call the most ordinary price on the
    // weapon "above the average", which is both wrong and the opposite of
    // useful.
    if price == dist.median {
        return Placement::Middle;
    }
    // On a skewed market the average sits above the median, so "between them" is
    // a real band. On a flat or left-skewed one there is no such band and
    // anything past the median is the upper part of the range.
    if dist.avg > dist.median && price < dist.avg {
        return Placement::Middle;
    }
    Placement::Upper
}

/// How an offer reads against the weapon's own market.
pub fn judge_offer(price: f64, dist: &Distribution) -> PriceVerdict {
    match placement_of(price, dist) {
        Placement::BelowObserved => PriceVerdict::Below,
        Placement::Bottom if price < dist.median * LOWBALL_FRACTION => PriceVerdict::Below,
        Placement::Bottom | Placement::Middle => PriceVerdict::Fair,
        Placement::Upper => PriceVerdict::Above,
        Placement::AboveObserved => PriceVerdict::Outlier,
    }
}

pub fn reroll_read(price: f64, dist: &Distribution, rerolls: f64, locked: bool) -> RerollRead {
    RerollRead {
        kuva: reroll_cost(rerolls, locked),
        median: dist.median,
        above_median: price > dist.median,
    }
}

/// How much less a rerolled riven fetches than an unrolled one on this weapon,
/// from the two bands' medians.
///
/// Straight out of DE's feed, which splits `rerolled` - and nothing in the
/// ecosystem surfaces it. Buyers pay for reroll headroom, so an unrolled riven
/// of the same apparent quality is usually worth more. Medians, not means,
/// because these markets are skewed. None when either side has no price.
pub fn rerolled_discount(unrolled_median: f64, rolled_median: f64) -> Option<f64> {
    if unrolled_median <= 0.0 || rolled_median <= 0.0 {
        return None;
    }
    Some((unrolled_median - rolled_median) / unrolled_median)
}

/// Appraise an offer against a weapon's distribution.
///
/// `price` is supplied by the user. We do not invent it, and the caveats say so.
pub fn appraise(price: Option<f64>, tier: Option<&RivenTier>, rerolls: f64, locked: bool) -> Appraisal {
    let Some(dist) = distribution_of(tier) else {
        return Appraisal {
            dist: None,
            unavailable: Some(Unavailable::NoData),
            placement: None,
            verdict: None,
            reroll: None,
            skewed: false,
            caveats: vec![AppraisalCaveat::NoData],
        };
    };
    let skewed = skew_of(&dist).is_some_and(|skew| skew >= SKEWED);
    let mut caveats = vec![AppraisalCaveat::Band, AppraisalCaveat::Desirability, AppraisalCaveat::Spliced];
    if dist.pop <= LOW_POPULARITY {
        caveats.push(AppraisalCaveat::Rare);
    }
    if skewed {
        caveats.push(AppraisalCaveat::Skewed);
    }
    let offer = price.filter(|p| *p > 0.0);
    Appraisal {
        dist: Some(dist),
        unavailable: None,
        placement: offer.map(|p| placement_of(p, &dist)),
        verdict: offer.map(|p| judge_offer(p, &dist)),
        reroll: offer.map(|p| reroll_read(p, &dist, rerolls, locked)),
        skewed,
        caveats,
    }
}

#[derive(Debug, Clone, Deserialize, TS)]
pub struct AppraiseRequest {
    #[serde(default)]
    pub price: Option<f64>,
    #[serde(default)]
    pub tier: Option<RivenTier>,
    /// Null reads as a fresh riven.
    #[serde(default)]
    pub rerolls: Option<f64>,
    #[serde(default)]
    pub locked: bool,
}

impl AppraiseRequest {
    pub fn run(&self) -> Appraisal {
        appraise(self.price, self.tier.as_ref(), self.rerolls.unwrap_or(0.0), self.locked)
    }
}

/// One auction of a comps sample. The page reads `created` with `Date.parse`, as
/// the listing-age column beside it does, and sends the instant; null when it
/// is missing or malformed.
#[derive(Debug, Clone, Deserialize, TS)]
pub struct CompAsk {
    #[serde(default)]
    pub owner_status: Option<String>,
    #[serde(default)]
    pub created_ms: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, TS)]
pub struct DescribeCompsRequest {
    pub comps: Vec<CompAsk>,
    #[serde(default)]
    pub now_ms: Option<f64>,
}

/// Owner-status buckets. Anything unrecognized stays `unknown` rather than
/// being folded into `offline`, which would be a claim WFM did not make.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, TS)]
pub struct CompsStatusCounts {
    pub online: usize,
    pub ingame: usize,
    pub offline: usize,
    pub unknown: usize,
}

/// What the comps sample on screen actually is.
///
/// The sample is the cheapest eligible auctions for one weapon, not
/// attribute-matched comparables and not a record of completed sales. Listing
/// age is not time-to-sale, and an update is not evidence that a seller is
/// reachable - so this describes the sample and stops there: no verdict, no
/// liquidity score, no "how fast this sells".
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CompsSampleRead {
    pub size: usize,
    pub dated: usize,
    pub oldest_ask_days: Option<f64>,
    pub newest_ask_days: Option<f64>,
    pub status: CompsStatusCounts,
}

pub fn describe_comps(comps: &[CompAsk], now_ms: Option<f64>) -> CompsSampleRead {
    let mut status = CompsStatusCounts::default();
    let mut ages: Vec<f64> = Vec::new();
    let now = now_ms.filter(|n| n.is_finite());
    for comp in comps {
        match comp.owner_status.as_deref() {
            Some("online") => status.online += 1,
            Some("ingame") => status.ingame += 1,
            Some("offline") => status.offline += 1,
            _ => status.unknown += 1,
        }
        let (Some(now), Some(created)) = (now, comp.created_ms.filter(|c| c.is_finite())) else {
            continue;
        };
        let days = ((now - created) / DAY_MS).floor();
        if days >= 0.0 {
            ages.push(days);
        }
    }
    CompsSampleRead {
        size: comps.len(),
        dated: ages.len(),
        oldest_ask_days: ages.iter().copied().reduce(f64::max),
        newest_ask_days: ages.iter().copied().reduce(f64::min),
        status,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rivens::tests::same;
    use serde_json::{json, Value};

    fn fixture() -> Value {
        serde_json::from_str(include_str!("../../../../tests/fixtures/rivens/appraise.json")).unwrap()
    }

    /// The page sends each tier field as `Number(x) || 0`.
    fn tier_of(v: &Value) -> Option<RivenTier> {
        let o = v.as_object()?;
        let n = |k: &str| o.get(k).and_then(Value::as_f64).unwrap_or(0.0);
        Some(RivenTier { avg: n("avg"), median: n("median"), min: n("min"), max: n("max"), stddev: n("stddev"), pop: n("pop") })
    }

    fn base(over: Value) -> RivenTier {
        let mut t = json!({ "avg": 210, "median": 195, "min": 7, "max": 980, "stddev": 120, "pop": 37 });
        for (k, v) in over.as_object().unwrap() {
            t[k] = v.clone();
        }
        tier_of(&t).unwrap()
    }

    #[test]
    fn appraisals_match_the_shared_fixture() {
        let cases = fixture()["appraise"].as_array().unwrap().clone();
        assert!(cases.len() > 300);
        for c in cases {
            let tier = tier_of(&c["tier"]);
            let got = appraise(c["price"].as_f64(), tier.as_ref(), c["rerolls"].as_f64().unwrap(), c["locked"].as_bool().unwrap());
            same(&serde_json::to_value(&got).unwrap(), &c["expected"], c["name"].as_str().unwrap());
        }
    }

    #[test]
    fn rerolled_discounts_match_the_shared_fixture() {
        for c in fixture()["rerolled_discount"].as_array().unwrap() {
            let median = |k: &str| c.get(k).and_then(|t| t.get("median")).and_then(Value::as_f64).unwrap_or(0.0);
            let got = rerolled_discount(median("unrolled"), median("rolled"));
            assert_eq!(got, c["expected"].as_f64(), "{}", c["name"]);
        }
    }

    #[test]
    fn comps_reads_match_the_shared_fixture() {
        for c in fixture()["describe_comps"].as_array().unwrap() {
            let comps: Vec<CompAsk> = serde_json::from_value(c["comps"].clone()).unwrap();
            let got = describe_comps(&comps, c["now_ms"].as_f64());
            same(&serde_json::to_value(&got).unwrap(), &c["expected"], c["name"].as_str().unwrap());
        }
    }

    #[test]
    fn pop_is_a_score_so_a_thin_band_still_places() {
        assert_eq!(distribution_of(Some(&base(json!({ "pop": 0 })))).map(|d| d.median), Some(195.0));
        let rare = appraise(Some(200.0), Some(&base(json!({ "pop": LOW_POPULARITY }))), 0.0, false);
        assert!(rare.placement.is_some() && rare.caveats.contains(&AppraisalCaveat::Rare));
        let thin = appraise(Some(200.0), Some(&base(json!({ "pop": LOW_POPULARITY + 1.0 }))), 0.0, false);
        assert!(!thin.caveats.contains(&AppraisalCaveat::Rare));
    }

    #[test]
    fn the_skewed_market_never_yields_an_impossible_price() {
        // 90 sales at 10p and 10 at 1,000p: the distribution a normal fit got backwards.
        let skewed = base(json!({ "avg": 109, "median": 10, "min": 10, "max": 1000, "stddev": 297, "pop": 100 }));
        let d = distribution_of(Some(&skewed)).unwrap();
        assert!((skew_of(&d).unwrap() - 10.9).abs() < 0.05);
        assert_eq!(placement_of(10.0, &d), Placement::Middle);
        assert_eq!(placement_of(9.0, &d), Placement::BelowObserved);
        let a = appraise(Some(10.0), Some(&skewed), 0.0, false);
        assert!(a.skewed && a.caveats.contains(&AppraisalCaveat::Skewed));
        let reroll = a.reroll.unwrap();
        for n in [d.median, d.avg, d.min, reroll.median, f64::from(reroll.kuva)] {
            assert!(n >= 0.0);
        }
    }

    #[test]
    fn an_offer_at_the_median_is_typical_on_any_market() {
        let flat = distribution_of(Some(&base(json!({ "avg": 100, "median": 100, "min": 90, "max": 110 })))).unwrap();
        assert_eq!(placement_of(100.0, &flat), Placement::Middle);
        assert_eq!(judge_offer(100.0, &flat), PriceVerdict::Fair);
        assert_eq!(placement_of(105.0, &flat), Placement::Upper);
        let left = distribution_of(Some(&base(json!({ "avg": 80, "median": 100, "min": 40, "max": 120 })))).unwrap();
        assert_eq!(placement_of(100.0, &left), Placement::Middle);
    }

    #[test]
    fn a_lowball_sits_a_quarter_under_the_median() {
        let d = distribution_of(Some(&base(json!({})))).unwrap();
        assert_eq!(judge_offer(146.25, &d), PriceVerdict::Fair);
        assert_eq!(judge_offer(146.0, &d), PriceVerdict::Below);
        assert_eq!(judge_offer(180.0, &d), PriceVerdict::Fair);
        assert_eq!(judge_offer(400.0, &d), PriceVerdict::Above);
        assert_eq!(judge_offer(1500.0, &d), PriceVerdict::Outlier);
    }

    #[test]
    fn reroll_cost_climbs_caps_and_doubles_when_locked() {
        assert_eq!([0.0, 4.0, 9.0, 50.0, -3.0, 2.7].map(|r| reroll_cost(r, false)), [900, 1700, 3500, 3500, 900, 1200]);
        assert_eq!(reroll_cost(0.0, true), 1800);
        assert_eq!(reroll_cost(50.0, true), 7000);
    }

    #[test]
    fn no_price_leaves_the_band_without_a_verdict() {
        let a = appraise(None, Some(&base(json!({}))), 0.0, false);
        assert!(a.dist.is_some() && a.verdict.is_none() && a.reroll.is_none());
        let nonsense = appraise(Some(0.0), Some(&base(json!({}))), 0.0, false);
        assert!(nonsense.placement.is_none());
        let none = appraise(Some(200.0), Some(&base(json!({ "median": 0 }))), 0.0, false);
        assert_eq!(none.unavailable, Some(Unavailable::NoData));
    }
}
