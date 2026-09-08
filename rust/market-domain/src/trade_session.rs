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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub components: Option<BTreeMap<String, f64>>,
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
    pub component_limits: BTreeMap<String, f64>,
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
    let mut available: BTreeMap<String, f64> = request
        .candidates
        .iter()
        .filter(|row| {
            row.components.is_none()
                && row.subtype.as_ref().is_none_or(|s| s.is_empty())
                && !row.slug.ends_with("_set")
                && counts.get(&row.slug) == Some(&1)
        })
        .map(|row| {
            (
                row.slug.clone(),
                if safe(row.sellable) && row.sellable >= 0.0 {
                    row.sellable
                } else {
                    0.0
                },
            )
        })
        .collect();
    let component_limits = available.clone();
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
        let valid_set = candidate.slug.ends_with("_set")
            && candidate.components.as_ref().is_some_and(|parts| {
                !parts.is_empty()
                    && parts.iter().all(|(slug, count)| {
                        slug != &candidate.slug && safe(*count) && *count > 0.0
                    })
                    && parts.values().sum::<f64>() <= 6.0
            });
        let reason = if candidate.supported == Some(false)
            || candidate.subtype.as_ref().is_some_and(|s| !s.is_empty())
            || ((candidate.components.is_some() || candidate.slug.ends_with("_set")) && !valid_set)
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
        let mut lot = if valid_set || request.mode == SessionMode::Fast || !candidate.bulk {
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
                component_limits: candidate
                    .components
                    .as_ref()
                    .map(|parts| {
                        parts
                            .keys()
                            .map(|slug| {
                                (
                                    slug.clone(),
                                    component_limits.get(slug).copied().unwrap_or(0.0),
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_else(|| {
                        BTreeMap::from([(
                            candidate.slug.clone(),
                            component_limits
                                .get(&candidate.slug)
                                .copied()
                                .unwrap_or(0.0),
                        )])
                    }),
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
    let mut add = |row: &mut SessionRow, result: &mut SessionPlan| {
        if result.trades >= cap
            || goal.is_some_and(|g| result.total >= g)
            || row.quantity + row.per_trade > row.candidate.sellable
        {
            return false;
        }
        let components = row
            .candidate
            .components
            .clone()
            .unwrap_or_else(|| BTreeMap::from([(row.candidate.slug.clone(), 1.0)]));
        if components.iter().any(|(slug, count)| {
            available.get(slug).copied().unwrap_or(0.0) < count * row.per_trade
        }) {
            return false;
        }
        for (slug, count) in components {
            if let Some(left) = available.get_mut(&slug) {
                *left -= count * row.per_trade;
            }
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
        if row.candidate.components.is_some() {
            reasons
                .push("Complete owned set; its components are allocated within this batch.".into());
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
    fn sets_share_the_component_pool() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/trade-session/sets.json"
        ))
        .unwrap();
        let mut candidates: Vec<SessionCandidate> = fixture["owned"].as_object().unwrap().iter().map(|(slug, count)| {
            serde_json::from_value(serde_json::json!({"key":slug,"slug":slug,"name":slug,"owned":count,"sellable":count,
                "leveled":0,"type":"Prime","hold":false,"bulk":false,
                "market":{"low_sell":fixture["part_prices"][slug],"median_now":fixture["part_prices"][slug],"vol":30}})).unwrap()
        }).collect();
        let set: SessionCandidate = serde_json::from_value(serde_json::json!({"key":"example_set","slug":"example_set","name":"Example Set",
            "owned":2,"sellable":2,"leveled":0,"type":"Set","hold":false,"bulk":true,"components":fixture["parts"],
            "market":{"low_sell":fixture["set_price"],"median_now":fixture["set_price"],"vol":30}})).unwrap();
        candidates.push(set);
        for mode in [
            SessionMode::Fast,
            SessionMode::PerTrade,
            SessionMode::Clear,
            SessionMode::Max,
        ] {
            let plan = select_session(SessionRequest {
                candidates: candidates.clone(),
                mode,
                budget: 2.0,
                target: None,
            });
            let mut consumed = BTreeMap::<String, f64>::new();
            for row in &plan.rows {
                for (slug, count) in row
                    .candidate
                    .components
                    .clone()
                    .unwrap_or_else(|| BTreeMap::from([(row.candidate.slug.clone(), 1.0)]))
                {
                    *consumed.entry(slug).or_default() += count * row.quantity;
                }
                if row.candidate.components.is_some() {
                    assert_eq!(row.per_trade, 1.0);
                }
            }
            for (slug, count) in consumed {
                assert!(count <= fixture["owned"][slug].as_f64().unwrap());
            }
            if mode == SessionMode::Max {
                assert_eq!(plan.rows.len(), 1);
                assert_eq!(plan.rows[0].candidate.slug, "example_set");
                assert_eq!(
                    plan.rows[0].quantity,
                    fixture["expected_sets"].as_f64().unwrap()
                );
            }
        }
        candidates.last_mut().unwrap().components = Some(BTreeMap::from([("barrel".into(), 7.0)]));
        assert!(select_session(SessionRequest {
            candidates,
            mode: SessionMode::Max,
            budget: 2.0,
            target: None
        })
        .rows
        .iter()
        .all(|row| row.candidate.components.is_none()));
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
