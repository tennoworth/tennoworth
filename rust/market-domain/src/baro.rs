//! Baro stock decisions and inventory yield use the same snapshot on every surface.
use crate::planners::{validate_owned, PlannerOwned};
use market_math::sell_priority::LIQUID_VOL;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

// Ducats cost farming time; keep the board's conservative value floor.
const SKIP_PLAT_PER_DUCAT: f64 = 0.16;
// A small move above baseline is noise, rather than evidence to sell into.
const FLIP_PREMIUM: f64 = 1.05;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct BaroStock {
    pub item: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub slug: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub unique: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub ducats: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub credits: Option<f64>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum BaroVerdict {
    Flip,
    Hold,
    Thin,
    Skip,
    Unpriced,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BaroRow {
    #[serde(flatten)]
    pub stock: BaroStock,
    pub price: Option<f64>,
    pub baseline: Option<f64>,
    pub vol: Option<f64>,
    pub plat_per_ducat: Option<f64>,
    pub verdict: BaroVerdict,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BaroPick {
    pub name: String,
    pub plat_per_ducat: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BaroValue {
    pub rows: Vec<BaroRow>,
    pub best: Vec<BaroPick>,
    pub more_tradeable: usize,
    pub fodder_ducats: Option<f64>,
    pub fodder_items: Option<f64>,
    pub cheap_fodder: Option<f64>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct BaroRequest {
    pub stock: Vec<BaroStock>,
    #[ts(type = "unknown")]
    pub market: Value,
    /// None means inventory evidence is unavailable, rather than an empty scan.
    pub owned: Option<Vec<PlannerOwned>>,
}
fn number(entry: &Value, field: &str) -> Option<f64> {
    entry
        .get(field)
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite() && *n >= 0.0)
}
fn positive(entry: &Value, field: &str) -> Option<f64> {
    number(entry, field).filter(|n| *n > 0.0)
}
fn current_price(entry: &Value) -> Option<f64> {
    positive(entry, "low5_avg").or_else(|| positive(entry, "low_sell"))
}
fn entry<'a>(market: &'a Value, slug: &str) -> &'a Value {
    market
        .get("items")
        .and_then(|items| items.get(slug))
        .unwrap_or(&Value::Null)
}
pub fn baro_value(request: &BaroRequest) -> Result<BaroValue, String> {
    let mut rows: Vec<_> = request
        .stock
        .iter()
        .map(|stock| {
            let item = entry(&request.market, stock.slug.as_deref().unwrap_or(""));
            let price = current_price(item);
            let baseline = positive(item, "median_90d");
            let vol = number(item, "vol");
            let plat_per_ducat = price
                .zip(stock.ducats.filter(|d| d.is_finite() && *d > 0.0))
                .map(|(p, d)| p / d)
                .filter(|ratio| ratio.is_finite());
            let verdict = if stock.slug.as_deref().is_none_or(str::is_empty) || price.is_none() {
                BaroVerdict::Unpriced
            } else if vol.is_some_and(|v| v < LIQUID_VOL) {
                BaroVerdict::Thin
            } else if plat_per_ducat.is_some_and(|r| r < SKIP_PLAT_PER_DUCAT) {
                BaroVerdict::Skip
            } else if price
                .zip(baseline)
                .is_some_and(|(p, b)| p >= b * FLIP_PREMIUM)
            {
                BaroVerdict::Flip
            } else {
                BaroVerdict::Hold
            };
            BaroRow {
                stock: stock.clone(),
                price,
                baseline,
                vol,
                plat_per_ducat,
                verdict,
            }
        })
        .collect();
    rows.sort_by(|a, b| {
        b.plat_per_ducat
            .unwrap_or(-1.0)
            .total_cmp(&a.plat_per_ducat.unwrap_or(-1.0))
            .then_with(|| {
                a.stock
                    .item
                    .to_lowercase()
                    .cmp(&b.stock.item.to_lowercase())
            })
    });
    let best: Vec<_> = rows
        .iter()
        .filter(|r| matches!(r.verdict, BaroVerdict::Flip | BaroVerdict::Hold))
        .filter_map(|r| {
            r.plat_per_ducat.map(|ratio| BaroPick {
                name: r.stock.item.clone(),
                plat_per_ducat: ratio,
            })
        })
        .take(2)
        .collect();
    let tradeable = rows
        .iter()
        .filter(|r| r.verdict != BaroVerdict::Unpriced && r.plat_per_ducat.is_some())
        .count();
    let (mut ducats, mut count, mut cheap) = (0.0, 0.0, 0.0);
    if let Some(owned) = &request.owned {
        validate_owned(owned)?;
        for rec in owned
            .iter()
            .filter(|r| r.subtype.as_deref().is_none_or(str::is_empty))
        {
            let item = entry(&request.market, &rec.slug);
            if let Some(value) = positive(item, "ducats") {
                ducats += value * rec.count;
                count += rec.count;
                if current_price(item).is_some_and(|p| p < 4.0) {
                    cheap += rec.count;
                }
            }
        }
        if [ducats, count, cheap]
            .iter()
            .any(|v| !v.is_finite() || *v > 9_007_199_254_740_991.0)
        {
            return Err("Ducat totals exceed the supported quantity.".into());
        }
    }
    Ok(BaroValue {
        more_tradeable: tradeable.saturating_sub(best.len()),
        best,
        rows,
        fodder_ducats: request.owned.as_ref().map(|_| ducats),
        fodder_items: request.owned.as_ref().map(|_| count),
        cheap_fodder: request.owned.as_ref().map(|_| cheap),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn value(stock: Value, items: Value, owned: Value) -> BaroValue {
        let request: BaroRequest =
            serde_json::from_value(json!({"stock":stock,"market":{"items":items},"owned":owned}))
                .unwrap();
        baro_value(&request).unwrap()
    }
    #[test]
    fn empty_unpriced_and_untradeable_stock_have_no_picks() {
        for stock in [
            json!([]),
            json!([{"item":"Missing","slug":"missing","ducats":350}]),
            json!([{"item":"Cosmetic","ducats":200}]),
        ] {
            let result = value(stock, json!({}), json!([]));
            assert!(result.best.is_empty());
            assert_eq!(result.more_tradeable, 0);
        }
        let result = value(
            json!([{"item":"Missing cost","slug":"priced"},{"item":"Zero cost","slug":"priced","ducats":0}]),
            json!({"priced":{"low_sell":20,"vol":40}}),
            Value::Null,
        );
        assert!(result.best.is_empty());
        assert_eq!(result.fodder_ducats, None);
        let subnormal = value(
            json!([{"item":"Invalid cost","slug":"priced","ducats":5e-324}]),
            json!({"priced":{"low_sell":20,"vol":40}}),
            Value::Null,
        );
        assert!(subnormal.best.is_empty());
        assert_eq!(subnormal.rows[0].plat_per_ducat, None);
    }
    #[test]
    fn board_prices_verdicts_and_order_are_preserved() {
        let result = value(
            json!([
                {"item":"Cosmetic","ducats":200},
                {"item":"Prisma Angstrum","slug":"angstrum","ducats":400},
                {"item":"Prisma Skana","slug":"skana","ducats":400},
                {"item":"Primed Reload Speed","slug":"reload","ducats":375},
                {"item":"Primed Fury","slug":"fury","ducats":350}
            ]),
            json!({
                "fury":{"low5_avg":142,"low_sell":5,"median_90d":128,"vol":41},
                "reload":{"low_sell":96,"median_90d":104,"vol":22},
                "skana":{"low5_avg":72,"median_90d":75,"vol":3},
                "angstrum":{"low5_avg":44,"median_90d":47,"vol":30}
            }),
            Value::Null,
        );
        assert_eq!(
            result
                .rows
                .iter()
                .map(|r| r.stock.item.as_str())
                .collect::<Vec<_>>(),
            vec![
                "Primed Fury",
                "Primed Reload Speed",
                "Prisma Skana",
                "Prisma Angstrum",
                "Cosmetic"
            ]
        );
        assert_eq!(
            result.rows.iter().map(|r| &r.verdict).collect::<Vec<_>>(),
            vec![
                &BaroVerdict::Flip,
                &BaroVerdict::Hold,
                &BaroVerdict::Thin,
                &BaroVerdict::Skip,
                &BaroVerdict::Unpriced
            ]
        );
        assert!((result.best[0].plat_per_ducat - 142.0 / 350.0).abs() < 1e-9);
        assert_eq!(result.best[1].name, "Primed Reload Speed");
        assert_eq!(result.more_tradeable, 2);
        assert_eq!(result.rows[4].price, None);
    }
    #[test]
    fn tied_picks_use_names_and_thin_or_poor_values_are_not_recommended() {
        let result = value(
            json!([
                {"item":"Zeta","slug":"good","ducats":100},
                {"item":"Alpha","slug":"good","ducats":100},
                {"item":"Beta","slug":"good","ducats":100},
                {"item":"Thin","slug":"thin","ducats":100},
                {"item":"Poor","slug":"poor","ducats":100}
            ]),
            json!({"good":{"low_sell":30,"vol":5},"thin":{"low_sell":100,"vol":4},"poor":{"low_sell":15,"vol":10}}),
            Value::Null,
        );
        assert_eq!(
            result
                .best
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Alpha", "Beta"]
        );
        assert_eq!(result.more_tradeable, 3);
    }
    #[test]
    fn fodder_counts_copies_once_even_when_held_item_is_also_in_stock() {
        let result = value(
            json!([{"item":"Part","slug":"part","ducats":400}]),
            json!({
                "part":{"ducats":45,"low5_avg":3,"low_sell":1,"vol":30},
                "valuable":{"ducats":100,"low_sell":30},
                "unknown":{"ducats":15},
                "relic":{"ducats":100,"low_sell":1}
            }),
            json!([
                {"slug":"part","name":"Part","count":3},
                {"slug":"part","name":"Part","count":1},
                {"slug":"valuable","name":"Valuable","count":2},
                {"slug":"unknown","name":"Unknown","count":1},
                {"slug":"absent","name":"Absent","count":9},
                {"slug":"relic","name":"Relic","count":8,"subtype":"Radiant"}
            ]),
        );
        assert_eq!(result.fodder_ducats, Some(395.0));
        assert_eq!(result.fodder_items, Some(7.0));
        assert_eq!(result.cheap_fodder, Some(4.0));
    }
    #[test]
    fn invalid_prices_do_not_become_zero_and_unsafe_totals_are_refused() {
        assert_eq!(
            current_price(&json!({"low5_avg":"invalid","low_sell":null})),
            None
        );
        assert_eq!(
            current_price(&json!({"low5_avg":0,"low_sell":30})),
            Some(30.0)
        );
        assert_eq!(current_price(&json!({"low5_avg":-2,"low_sell":0})), None);
        let request: BaroRequest = serde_json::from_value(json!({"stock":[],"market":{"items":{"part":{"ducats":100}}},"owned":[{"slug":"part","name":"Part","count":9_007_199_254_740_991_u64}]})).unwrap();
        assert!(baro_value(&request).is_err());
    }
}
