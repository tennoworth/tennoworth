use market_math::sell_priority::{clearing_price, PricedEntry, LIQUID_VOL};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use ts_rs::TS;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, TS, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum SessionMode {
    Fast,
    PerTrade,
    Clear,
    Max,
}
#[derive(Clone, Debug, Deserialize, Serialize, TS, PartialEq)]
pub struct SessionCandidate {
    pub key: String,
    pub slug: String,
    pub name: String,
    pub owned: f64,
    pub sellable: f64,
    pub leveled: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub subtype: Option<String>,
    #[serde(rename = "type")]
    pub item_type: String,
    pub hold: bool,
    pub bulk: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub supported: Option<bool>,
    #[ts(type = "unknown")]
    pub market: Value,
}
#[derive(Clone, Debug, Deserialize, Serialize, TS, PartialEq)]
pub struct SessionRequest {
    pub candidates: Vec<SessionCandidate>,
    pub mode: SessionMode,
    pub budget: f64,
    pub target: Option<f64>,
}
#[derive(Clone, Debug, Deserialize, Serialize, TS, PartialEq)]
pub struct SessionRow {
    #[serde(flatten)]
    pub candidate: SessionCandidate,
    pub quantity: f64,
    pub per_trade: f64,
    pub platinum: f64,
    pub trades: u32,
    pub reason: String,
    pub bid: Option<f64>,
}
#[derive(Clone, Debug, Deserialize, Serialize, TS, PartialEq)]
pub struct SessionExclusion {
    pub name: String,
    pub reason: String,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, TS, PartialEq)]
pub struct SessionPlan {
    pub rows: Vec<SessionRow>,
    pub trades: u32,
    pub total: f64,
    pub excluded: Vec<SessionExclusion>,
    pub target: Option<f64>,
    pub shortfall: Option<f64>,
}
fn number(v: &Value, key: &str) -> f64 {
    v.get(key)
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite())
        .unwrap_or(0.0)
}
fn safe(n: f64) -> bool {
    n.is_finite() && n.fract() == 0.0 && n.abs() <= 9_007_199_254_740_991.0
}
pub fn valid_session_lot(quantity: f64, lot: f64, bulk: bool) -> bool {
    safe(quantity)
        && quantity > 0.0
        && safe(lot)
        && (1.0..=6.0).contains(&lot)
        && quantity % lot == 0.0
        && (bulk || lot == 1.0)
}
pub fn select_session(request: SessionRequest) -> SessionPlan {
    let limits: Value = serde_json::from_str(include_str!("../../../tests/fixtures/limits.json"))
        .unwrap_or(Value::Null);
    let maximum = number(&limits, "max_platinum");
    let cap = if safe(request.budget) {
        request.budget.clamp(0.0, number(&limits, "max_plan_items")) as u32
    } else {
        0
    };
    let goal = request.target.filter(|n| n.is_finite() && *n > 0.0);
    let mut result = SessionPlan {
        target: goal,
        ..Default::default()
    };
    let mut counts = BTreeMap::new();
    for row in &request.candidates {
        *counts.entry(row.slug.clone()).or_insert(0_u32) += 1;
    }
    let mut eligible = Vec::new();
    for candidate in request.candidates {
        let m = &candidate.market;
        let priced = PricedEntry {
            vol: number(m, "vol"),
            low_sell: number(m, "low_sell"),
            avg: number(m, "avg"),
            median_now: number(m, "median_now"),
            median_90d: number(m, "median_90d"),
        };
        let price = clearing_price(&priced).ceil();
        let volume = priced.vol.max(0.0);
        let reason = if candidate.supported == Some(false)
            || candidate.subtype.as_ref().is_some_and(|s| !s.is_empty())
            || candidate.slug.ends_with("_set")
            || candidate.item_type.to_lowercase().contains("riven")
            || counts.get(&candidate.slug) != Some(&1)
        {
            Some("This item identity is not supported in Trade Session yet.")
        } else if !safe(candidate.sellable)
            || candidate.sellable <= 0.0
            || !safe(candidate.owned)
            || candidate.sellable > candidate.owned
        {
            Some("No confirmed sellable copies after protection and trade checks.")
        } else if ![
            priced.low_sell,
            priced.avg,
            priced.median_now,
            priced.median_90d,
        ]
        .iter()
        .any(|n| *n > 0.0)
            || !price.is_finite()
            || price < number(&limits, "min_platinum")
            || price > maximum
        {
            Some("No credible price within the listing limits.")
        } else if matches!(request.mode, SessionMode::Fast | SessionMode::Clear)
            && volume < LIQUID_VOL
        {
            Some("Not enough reported trading volume for this mode.")
        } else {
            None
        };
        if let Some(reason) = reason {
            result.excluded.push(SessionExclusion {
                name: candidate.name,
                reason: reason.into(),
            });
            continue;
        }
        let mut lot = if request.mode == SessionMode::Fast || !candidate.bulk {
            1.0
        } else {
            6.0_f64
                .min(candidate.sellable)
                .min((maximum / price).floor())
        };
        if request.mode == SessionMode::Clear {
            while candidate.sellable % lot != 0.0 {
                lot -= 1.0;
            }
        }
        let bid = Some(number(m, "top_buy")).filter(|n| *n > 0.0);
        let weight = if candidate.hold { 0.6 } else { 1.0 };
        let value = price * lot;
        let score = match request.mode {
            SessionMode::Fast => vec![
                volume
                    * (1.0
                        + bid
                            .filter(|n| *n <= price)
                            .map(|n| n / price)
                            .unwrap_or(0.0))
                    * weight,
                price,
            ],
            SessionMode::Clear => vec![
                f64::from(u8::from(candidate.sellable / lot <= f64::from(cap))) * weight,
                lot * weight,
                volume,
            ],
            SessionMode::Max => vec![
                value * f64::from(cap).min((candidate.sellable / lot).floor()) * weight,
                volume,
            ],
            SessionMode::PerTrade => vec![value * weight, volume],
        };
        eligible.push((
            SessionRow {
                candidate,
                quantity: 0.0,
                per_trade: lot,
                platinum: price,
                trades: 0,
                reason: String::new(),
                bid,
            },
            score,
            volume,
        ));
    }
    eligible.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                a.0.candidate
                    .key
                    .encode_utf16()
                    .cmp(b.0.candidate.key.encode_utf16())
            })
    });
    let add = |row: &mut SessionRow, result: &mut SessionPlan| {
        if result.trades >= cap
            || goal.is_some_and(|g| result.total >= g)
            || row.quantity + row.per_trade > row.candidate.sellable
        {
            return false;
        }
        row.quantity += row.per_trade;
        row.trades += 1;
        result.trades += 1;
        result.total += row.platinum * row.per_trade;
        true
    };
    if matches!(request.mode, SessionMode::PerTrade | SessionMode::Fast) {
        loop {
            let mut changed = false;
            for (row, _, _) in &mut eligible {
                changed |= add(row, &mut result);
            }
            if !changed {
                break;
            }
        }
    } else {
        for (row, _, _) in &mut eligible {
            while add(row, &mut result) {}
        }
    }
    for (mut row, _, volume) in eligible {
        if row.quantity == 0.0 {
            continue;
        }
        let mut reasons = vec![match request.mode {
            SessionMode::Fast => "Liquid singles; match credible asks.".into(),
            SessionMode::PerTrade => {
                format!("{}p per suggested exchange.", row.platinum * row.per_trade)
            }
            SessionMode::Clear if row.quantity == row.candidate.sellable => {
                "Clears this safe spare stack.".into()
            }
            _ => "Prioritizes listing value within the budget.".into(),
        }];
        if volume < LIQUID_VOL {
            reasons.push("Thin market: patience may be needed.".into());
        }
        if row.candidate.hold {
            reasons.push("Hold advice lowers priority; protected copies remain excluded.".into());
        }
        row.reason = reasons.join(" ");
        result.rows.push(row);
    }
    result.shortfall = goal.map(|g| (g - result.total).max(0.0));
    result
}

/// Reject malformed snapshot numbers instead of changing their legacy coerced price.
pub fn validate_session_request(request: &SessionRequest) -> Result<(), String> {
    for row in &request.candidates {
        if !row.market.is_object() {
            return Err("Trade Session market data must be an object.".into());
        }
        for key in [
            "avg",
            "low_sell",
            "median_now",
            "median_90d",
            "vol",
            "top_buy",
        ] {
            if row
                .market
                .get(key)
                .is_some_and(|n| !n.is_null() && !n.is_number())
            {
                return Err(
                    "Trade Session market prices and volumes must be numbers or null.".into(),
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selector_shared_contract() {
        let cases: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/trade-session/selector.json"
        ))
        .unwrap();
        for case in cases.as_array().unwrap() {
            let request: SessionRequest = serde_json::from_value(case["request"].clone()).unwrap();
            validate_session_request(&request).unwrap();
            let actual = select_session(request);
            let expected: SessionPlan = serde_json::from_value(case["expected"].clone()).unwrap();
            assert_eq!(actual, expected, "{}", case["name"]);
        }
    }
    #[test]
    fn lot_shared_contract() {
        let cases: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/trade-session/lots.json"
        ))
        .unwrap();
        for case in cases.as_array().unwrap() {
            if let Some(lot) = case["per_trade"].as_f64() {
                assert_eq!(
                    valid_session_lot(
                        case["quantity"].as_f64().unwrap(),
                        lot,
                        case["bulk_tradable"].as_bool().unwrap()
                    ),
                    case["valid"].as_bool().unwrap()
                );
            }
        }
    }
    #[test]
    fn nonfinite_budget_cannot_allocate() {
        for budget in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let request = SessionRequest {
                candidates: vec![],
                mode: SessionMode::Max,
                budget,
                target: Some(f64::NAN),
            };
            let plan = select_session(request);
            assert_eq!(plan.trades, 0);
            assert_eq!(plan.target, None);
        }
    }
}
