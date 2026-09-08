use crate::inventory::OwnedItem;
use market_math::sell_priority::{self, PricedEntry};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use ts_rs::TS;

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
pub struct ScoringMarketEntry {
    #[serde(default)]
    pub vol: Option<f64>,
    #[serde(default)]
    pub low_sell: Option<f64>,
    #[serde(default)]
    pub avg: Option<f64>,
    #[serde(default)]
    pub median_now: Option<f64>,
    #[serde(default)]
    pub median_90d: Option<f64>,
    #[serde(default)]
    pub low5_avg: Option<f64>,
    #[serde(default)]
    pub ducats: Option<f64>,
    #[serde(default)]
    pub donch_top_90d: Option<f64>,
    #[serde(default)]
    pub donch_bot_90d: Option<f64>,
    #[serde(default)]
    pub top_buy: Option<f64>,
    #[serde(default)]
    pub medians_7d: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct UsageSetPart {
    pub slug: String,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
pub struct UsageSet {
    #[serde(default)]
    pub parts: Vec<UsageSetPart>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
pub struct ScoringMarket {
    pub items: BTreeMap<String, ScoringMarketEntry>,
    #[serde(default)]
    #[ts(type = "Record<string, unknown>")]
    pub usage: BTreeMap<String, Value>,
    #[serde(default)]
    pub set_to_parts: BTreeMap<String, UsageSet>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ScoreInventoryRequest {
    pub owned: Vec<(String, OwnedItem)>,
    pub market: ScoringMarket,
    pub reserve_copies: f64,
    pub spares_only: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct UsageEntry {
    pub name: String,
    pub category: String,
    pub year: f64,
    pub share: f64,
    pub peak_mr: f64,
    pub by_mr: Vec<f64>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct MasteryBand {
    pub from: usize,
    pub to: usize,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum Liquidity {
    SellsToday,
    Underpriced,
    Slow,
    Thin,
    Unknown,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct DemandRead {
    pub usage: Option<UsageEntry>,
    pub inherited: bool,
    pub liquidity: Liquidity,
    pub band: Option<MasteryBand>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum Timing {
    Hold,
    Peak,
    Neutral,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct ScoredInventoryFact {
    pub key: String,
    pub sellable: f64,
    pub clearing_price: f64,
    pub sell_score: f64,
    pub patience: bool,
    pub ducats: Option<f64>,
    pub plat_per_100d: Option<f64>,
    pub potential_plat: f64,
    pub raw_value: f64,
    pub medians_7d: Vec<f64>,
    pub median_90d: Option<f64>,
    pub delta_90d_pct: Option<f64>,
    pub timing: Timing,
    pub demand: DemandRead,
}

pub fn valid_usage(value: &Value) -> Option<UsageEntry> {
    let row: UsageEntry = serde_json::from_value(value.clone()).ok()?;
    (!row.name.is_empty()
        && !row.category.is_empty()
        && row.year > 0.0
        && row.year.fract() == 0.0
        && row.share.is_finite()
        && row.share >= 0.0
        && row.peak_mr.is_finite()
        && row.peak_mr >= 0.0
        && !row.by_mr.is_empty()
        && row.by_mr.iter().all(|v| v.is_finite() && *v >= 0.0))
    .then_some(row)
}

fn parent_index(sets: &BTreeMap<String, UsageSet>) -> BTreeMap<String, Option<String>> {
    let mut index = BTreeMap::<String, Option<String>>::new();
    for (parent, set) in sets {
        for part in &set.parts {
            if part.slug.is_empty() {
                continue;
            }
            match index.get(&part.slug) {
                None => {
                    index.insert(part.slug.clone(), Some(parent.clone()));
                }
                Some(Some(existing)) if existing != parent => {
                    index.insert(part.slug.clone(), None);
                }
                _ => {}
            }
        }
    }
    index
}

pub fn mastery_band(by_mr: &[f64]) -> Option<MasteryBand> {
    let total: f64 = by_mr.iter().sum();
    if total <= 0.0 {
        return None;
    }
    let mut peak = 0;
    let mut highest = *by_mr.first()?;
    for (index, value) in by_mr.iter().enumerate().skip(1) {
        if *value > highest {
            peak = index;
            highest = *value;
        }
    }
    let (mut from, mut to, mut acc) = (peak, peak, highest);
    while acc / total < 0.6 && (from > 0 || to < by_mr.len().saturating_sub(1)) {
        let left = from
            .checked_sub(1)
            .and_then(|i| by_mr.get(i))
            .copied()
            .unwrap_or(-1.0);
        let right = by_mr.get(to + 1).copied().unwrap_or(-1.0);
        if right > left {
            to += 1;
            acc += right;
        } else {
            from = from.saturating_sub(1);
            acc += left;
        }
    }
    Some(MasteryBand { from, to })
}

fn timing(price: f64, m: &ScoringMarketEntry) -> Timing {
    let top = m.donch_top_90d.unwrap_or(0.0);
    let bot = m.donch_bot_90d.unwrap_or(0.0);
    if price <= 0.0 || top <= 0.0 || bot <= 0.0 || top <= bot {
        return Timing::Neutral;
    }
    let position = (price - bot) / (top - bot);
    if position <= 0.2 {
        return Timing::Hold;
    }
    let ask = m.low_sell.unwrap_or(0.0);
    let buy = m.top_buy.unwrap_or(0.0);
    if position >= 0.8 && ((ask > 0.0 && price <= ask * 2.0) || (buy > 0.0 && price <= buy * 2.0)) {
        Timing::Peak
    } else {
        Timing::Neutral
    }
}

pub fn score_inventory(request: ScoreInventoryRequest) -> Result<Vec<ScoredInventoryFact>, String> {
    if !request.reserve_copies.is_finite() || request.reserve_copies < 0.0 {
        return Err("Reserve must be a nonnegative number".into());
    }
    let parents = parent_index(&request.market.set_to_parts);
    let mut out = Vec::new();
    for (key, owned) in request.owned {
        if !owned.count.is_finite()
            || owned.count < 0.0
            || !owned.leveled.is_finite()
            || owned.leveled < 0.0
        {
            return Err("Owned quantities must be nonnegative numbers".into());
        }
        let Some(m) = request.market.items.get(&owned.slug) else {
            continue;
        };
        let priced = PricedEntry {
            vol: m.vol.unwrap_or(0.0),
            low_sell: m.low_sell.unwrap_or(0.0),
            avg: m.avg.unwrap_or(0.0),
            median_now: m.median_now.unwrap_or(0.0),
            median_90d: m.median_90d.unwrap_or(0.0),
        };
        let sellable = if request.spares_only {
            let tradeable = (owned.count - owned.leveled).max(0.0);
            if owned.kept_lvl.is_some_and(|v| v > 0.0) {
                tradeable
            } else {
                (tradeable - 1.0).max(0.0)
            }
        } else {
            (owned.count - request.reserve_copies.max(owned.leveled)).max(0.0)
        };
        let clearing_price = sell_priority::clearing_price(&priced);
        let (usage, inherited) = if let Some(value) = request.market.usage.get(&owned.slug) {
            (valid_usage(value), false)
        } else {
            let usage = parents
                .get(&owned.slug)
                .and_then(|p| p.as_ref())
                .and_then(|parent| request.market.usage.get(parent))
                .and_then(valid_usage);
            let inherited = usage.is_some();
            (usage, inherited)
        };
        let liquidity = if priced.vol < 5.0 {
            Liquidity::Thin
        } else if let Some(usage) = &usage {
            if usage.share < sell_priority::DEAD_SHARE {
                Liquidity::Slow
            } else if priced.median_90d > 0.0 && clearing_price < priced.median_90d * 0.9 {
                Liquidity::Underpriced
            } else {
                Liquidity::SellsToday
            }
        } else {
            Liquidity::Unknown
        };
        let score =
            sell_priority::score_row_weighted(sellable, &priced, usage.as_ref().map(|u| u.share));
        let band = usage.as_ref().and_then(|u| mastery_band(&u.by_mr));
        let demand = DemandRead {
            usage,
            inherited,
            liquidity,
            band,
        };
        let median_now = if priced.median_now != 0.0 {
            priced.median_now
        } else {
            priced.median_90d
        };
        let median_90d = (priced.median_90d > 0.0).then_some(priced.median_90d);
        let delta_90d_pct = median_90d
            .filter(|_| median_now != 0.0 && priced.vol >= sell_priority::LIQUID_VOL)
            .map(|baseline| ((median_now - baseline) / baseline) * 100.0);
        let ducats = if owned.subtype.as_ref().is_some_and(|s| !s.is_empty()) {
            None
        } else {
            m.ducats
        };
        out.push(ScoredInventoryFact {
            key,
            sellable,
            clearing_price,
            sell_score: score.sell_score,
            patience: score.patience,
            ducats,
            plat_per_100d: ducats
                .filter(|d| *d > 0.0 && clearing_price > 0.0)
                .map(|d| clearing_price * 100.0 / d),
            potential_plat: sellable * priced.avg,
            raw_value: sellable * m.low5_avg.filter(|v| *v > 0.0).unwrap_or(priced.avg),
            medians_7d: m.medians_7d.iter().copied().filter(|v| *v > 0.0).collect(),
            median_90d,
            delta_90d_pct,
            timing: timing(median_now, m),
            demand,
        });
    }
    if out.iter().any(|r| {
        !r.sell_score.is_finite()
            || !r.raw_value.is_finite()
            || !r.potential_plat.is_finite()
            || !r.clearing_price.is_finite()
            || r.plat_per_100d.is_some_and(|n| !n.is_finite())
            || r.delta_90d_pct.is_some_and(|n| !n.is_finite())
    }) {
        return Err("Scoring exceeded finite numeric range".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Deserialize)]
    struct Case {
        name: String,
        request: ScoreInventoryRequest,
        expected: Vec<ScoredInventoryFact>,
    }
    #[derive(Deserialize)]
    struct Fixture {
        cases: Vec<Case>,
    }
    #[test]
    fn malformed_quantities_and_overflow_reject_the_batch() {
        let fixture: Fixture = serde_json::from_str(include_str!(
            "../../../tests/fixtures/inventory-scoring/cases.json"
        ))
        .unwrap();
        let mut request = fixture.cases.into_iter().next().unwrap().request;
        request.reserve_copies = -1.0;
        assert!(score_inventory(request.clone()).is_err());
        request.reserve_copies = 0.0;
        request.owned.first_mut().unwrap().1.count = -1.0;
        assert!(score_inventory(request.clone()).is_err());
        request.owned.first_mut().unwrap().1.count = f64::MAX;
        request.market.items.get_mut("item").unwrap().avg = Some(f64::MAX);
        assert!(score_inventory(request).is_err());
    }

    #[test]
    fn matches_shared_frontend_fixture() {
        let fixture: Fixture = serde_json::from_str(include_str!(
            "../../../tests/fixtures/inventory-scoring/cases.json"
        ))
        .unwrap();
        for case in fixture.cases {
            assert_eq!(
                score_inventory(case.request).unwrap(),
                case.expected,
                "{}",
                case.name
            );
        }
    }
}
