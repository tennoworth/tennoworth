use market_math::sell_priority::{clearing_price, PricedEntry};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;
mod build;
mod ducat;
mod relic;
mod sets;
pub use build::*;
pub use ducat::*;
pub use relic::*;
pub use sets::*;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct PlannerOwned {
    pub slug: String,
    pub name: String,
    pub count: f64,
    #[serde(default)]
    pub subtype: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct PlannerRequest {
    pub owned: Vec<PlannerOwned>,
    #[ts(type = "unknown")]
    pub market: Value,
    pub limit: Option<i64>,
}
fn num(value: &Value) -> f64 {
    match value {
        Value::Number(n) => n.as_f64().unwrap_or(0.0),
        Value::String(s) => s
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
            .unwrap_or(0.0),
        Value::Bool(true) => 1.0,
        Value::Bool(false) => 0.0,
        _ => 0.0,
    }
}
fn field(value: &Value, key: &str) -> f64 {
    num(&value[key])
}
fn name(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or_default().to_owned()
}
fn entry<'a>(market: &'a Value, slug: &str) -> &'a Value {
    market
        .get("items")
        .and_then(|items| items.get(slug))
        .unwrap_or(&Value::Null)
}
fn price(market: &Value, slug: &str) -> Option<f64> {
    let m = entry(market, slug);
    let p = PricedEntry {
        low_sell: field(m, "low_sell"),
        median_now: field(m, "median_now"),
        median_90d: field(m, "median_90d"),
        avg: field(m, "avg"),
        vol: field(m, "vol"),
    };
    [p.low_sell, p.median_now, p.median_90d, p.avg]
        .iter()
        .any(|n| *n > 0.0)
        .then(|| clearing_price(&p))
}
fn count(owned: &[PlannerOwned], slug: &str) -> f64 {
    owned
        .iter()
        .filter(|r| r.slug == slug)
        .map(|r| r.count)
        .sum()
}
fn cap<T>(mut rows: Vec<T>, limit: i64) -> Vec<T> {
    let size = if limit < 0 {
        rows.len().saturating_sub(limit.unsigned_abs() as usize)
    } else {
        limit as usize
    };
    rows.truncate(size);
    rows
}

const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

pub fn validate_owned(owned: &[PlannerOwned]) -> Result<(), String> {
    let mut totals = std::collections::HashMap::<&str, f64>::new();
    for row in owned {
        if !row.count.is_finite()
            || row.count < 0.0
            || row.count > MAX_SAFE_INTEGER
            || row.count.fract() != 0.0
        {
            return Err("Owned quantities must be nonnegative safe integers.".to_owned());
        }
        let total = totals.entry(&row.slug).or_default();
        *total += row.count;
        if *total > MAX_SAFE_INTEGER {
            return Err("Combined copies of an item exceed the supported quantity.".to_owned());
        }
    }
    Ok(())
}

fn validate_numeric_field(value: &Value) -> Result<(), String> {
    let number = match value {
        Value::Number(value) => value.as_f64(),
        Value::String(value) => value.trim().parse::<f64>().ok(),
        _ => None,
    };
    if number.is_some_and(|number| !number.is_finite() || number.abs() > MAX_SAFE_INTEGER) {
        return Err("Planner market numbers exceed the supported numeric range.".to_owned());
    }
    Ok(())
}

/// Legacy snapshots can carry numeric strings, which bypass a JSON-number-only boundary check.
pub fn validate_planner_market(market: &Value) -> Result<(), String> {
    if let Some(items) = market.get("items").and_then(Value::as_object) {
        for item in items.values() {
            for key in [
                "low_sell",
                "median_now",
                "median_90d",
                "avg",
                "vol",
                "ducats",
                "top_buy",
            ] {
                if let Some(value) = item.get(key) {
                    validate_numeric_field(value)?;
                }
            }
        }
    }
    if let Some(sets) = market.get("set_to_parts").and_then(Value::as_object) {
        for set in sets.values() {
            if let Some(parts) = set.get("parts").and_then(Value::as_array) {
                for part in parts {
                    if let Some(value) = part.get("quantity") {
                        validate_numeric_field(value)?;
                    }
                }
            }
        }
    }
    if let Some(relics) = market.get("relic_rewards").and_then(Value::as_object) {
        for rewards in relics.values().filter_map(Value::as_array) {
            for reward in rewards {
                for key in ["chance", "item_count"] {
                    if let Some(value) = reward.get(key) {
                        validate_numeric_field(value)?;
                    }
                }
                if let Some(chances) = reward.get("chances").and_then(Value::as_object) {
                    for chance in chances.values() {
                        validate_numeric_field(chance)?;
                    }
                }
            }
        }
    }
    Ok(())
}

pub fn validate_ducat_request(request: &DucatRequest) -> Result<(), String> {
    for value in [Some(request.target), request.keep_above]
        .into_iter()
        .flatten()
    {
        if !value.is_finite() || value.abs() > MAX_SAFE_INTEGER {
            return Err(
                "Ducat target and price threshold must be finite supported numbers.".to_owned(),
            );
        }
    }
    for row in &request.owned {
        let ducats = field(entry(&request.market, &row.slug), "ducats");
        if ducats <= 0.0 {
            continue;
        }
        if let Some(plat) = price(&request.market, &row.slug) {
            if !(ducats / plat).is_finite() {
                return Err("A market price is too small to calculate its ducat ratio.".to_owned());
            }
        }
    }
    Ok(())
}
