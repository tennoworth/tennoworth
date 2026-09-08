use chrono::{DateTime, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use ts_rs::TS;

#[derive(Clone, Debug, Deserialize, Serialize, TS, PartialEq)]
pub struct HistorySeries {
    pub median: Vec<Option<f64>>,
    #[serde(default)]
    pub volume: Vec<f64>,
    pub subtype: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, TS, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct YearStats {
    pub latest: f64,
    pub latest_idx: usize,
    pub baseline: f64,
    pub delta_pct: Option<f64>,
    pub high: f64,
    pub low: f64,
    pub traded_days: usize,
}
#[derive(Clone, Debug, Deserialize, Serialize, TS, PartialEq)]
pub struct HistoryRequest {
    pub series: HistorySeries,
    pub min_days: Option<usize>,
    pub buckets: Option<usize>,
}
#[derive(Clone, Debug, Deserialize, Serialize, TS, PartialEq)]
pub struct HistoryAnalysis {
    pub points: Vec<(usize, f64)>,
    pub stats: Option<YearStats>,
    pub weekly: Vec<f64>,
    pub slope30: Option<f64>,
}
fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let mid = sorted.len() / 2;
    let upper = sorted.get(mid).copied().unwrap_or(0.0);
    if sorted.len() % 2 == 1 {
        upper
    } else {
        (sorted.get(mid.saturating_sub(1)).copied().unwrap_or(0.0) + upper) / 2.0
    }
}
pub fn points(series: &HistorySeries) -> Vec<(usize, f64)> {
    series
        .median
        .iter()
        .enumerate()
        .filter_map(|(i, m)| m.filter(|v| v.is_finite()).map(|v| (i, v)))
        .collect()
}
pub fn year_stats(series: &HistorySeries, min_days: usize) -> Option<YearStats> {
    let pts = points(series);
    if pts.len() < min_days {
        return None;
    }
    let &(latest_idx, latest) = pts.last()?;
    let vals: Vec<_> = pts.iter().map(|p| p.1).collect();
    let head: Vec<_> = vals
        .iter()
        .take(30.min((vals.len() / 3).max(1)))
        .copied()
        .collect();
    let baseline = median(&head);
    Some(YearStats {
        latest,
        latest_idx,
        baseline,
        delta_pct: if baseline > 0.0 && head.len() >= 5 {
            Some((latest - baseline) / baseline * 100.0)
        } else {
            None
        },
        high: vals.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        low: vals.iter().copied().fold(f64::INFINITY, f64::min),
        traded_days: pts.len(),
    })
}
pub fn weekly(series: &HistorySeries, buckets: usize) -> Vec<f64> {
    let size = if buckets == 0 {
        series.median.len().max(1)
    } else {
        series.median.len().div_ceil(buckets).max(1)
    };
    series
        .median
        .chunks(size)
        .filter_map(|chunk| {
            let vals: Vec<_> = chunk
                .iter()
                .filter_map(|m| m.filter(|n| n.is_finite()))
                .collect();
            if vals.is_empty() {
                None
            } else {
                Some(median(&vals))
            }
        })
        .collect()
}
fn slice_median(values: &[Option<f64>], from: i64, to: i64) -> Option<f64> {
    let mut vals: Vec<_> = values
        .iter()
        .enumerate()
        .filter(|(i, _)| (*i as i64) >= from.max(0) && (*i as i64) < to)
        .filter_map(|(_, m)| m.filter(|n| n.is_finite() && *n > 0.0))
        .collect();
    if vals.len() < 5 {
        return None;
    }
    vals.sort_by(f64::total_cmp);
    vals.get(vals.len() / 2).copied()
}
pub fn slope30(values: &[Option<f64>]) -> Option<f64> {
    let n = values.len() as i64;
    Some(slice_median(values, n - 15, n)? / slice_median(values, n - 30, n - 15)? - 1.0)
}
fn date_ms(date: &str) -> Option<f64> {
    if date.len() == 10 {
        NaiveDate::parse_from_str(date, "%Y-%m-%d")
            .ok()?
            .and_hms_opt(0, 0, 0)
            .map(|d| d.and_utc().timestamp_millis() as f64)
    } else {
        DateTime::parse_from_rfc3339(date)
            .ok()
            .map(|d| d.timestamp_millis() as f64)
    }
}
pub fn pre_vault_median(start: &str, values: &[Option<f64>], vault: &str) -> Option<f64> {
    let index = ((date_ms(vault)? - date_ms(start)?) / 86_400_000.0).floor() as i64;
    if index <= 0 {
        None
    } else {
        slice_median(values, index - 30, index)
    }
}
pub fn analyze_history(request: HistoryRequest) -> HistoryAnalysis {
    HistoryAnalysis {
        points: points(&request.series),
        stats: year_stats(&request.series, request.min_days.unwrap_or(20)),
        weekly: weekly(&request.series, request.buckets.unwrap_or(52)),
        slope30: slope30(&request.series.median),
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, TS, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Advice {
    SellNow,
    Hold,
    Neutral,
}
#[derive(Clone, Debug, Deserialize, Serialize, TS, PartialEq)]
pub struct Verdict {
    pub advice: Advice,
    pub reasons: Vec<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, TS, PartialEq)]
pub struct AdvisorRequest {
    pub slugs: Vec<String>,
    #[ts(type = "unknown")]
    pub market: Value,
    #[ts(type = "unknown")]
    pub history: Option<Value>,
    pub now_ms: f64,
}
fn string<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}
fn number(v: &Value, key: &str) -> Option<f64> {
    v.get(key)
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite() && *n != 0.0)
}
fn days_since(date: Option<&str>, now: f64) -> Option<f64> {
    Some(((now - date_ms(date?)?) / 86_400_000.0).floor())
}
fn rounded(n: f64) -> String {
    if n == f64::INFINITY {
        return "Infinity".into();
    }
    if n == f64::NEG_INFINITY {
        return "-Infinity".into();
    }
    if n.is_nan() {
        return "NaN".into();
    }
    let floor = n.floor();
    let rounded = if n - floor < 0.5 { floor } else { floor + 1.0 };
    if rounded.abs() >= 1e21 {
        format!("{rounded:e}").replace('e', "e+")
    } else {
        rounded.to_string()
    }
}
fn fixed_one(n: f64) -> String {
    if n.abs() >= 1e21 {
        return format!("{n:e}").replace('e', "e+");
    }
    // Decimal half ties that are exact in binary occur at quarters; JS toFixed
    // rounds their magnitudes upward, whereas Rust formatting uses ties-even.
    if (n * 4.0).fract() == 0.0 && (n.abs() * 10.0).fract() == 0.5 {
        format!("{:.1}", (n.abs() * 10.0).ceil() / 10.0 * n.signum())
    } else {
        format!("{n:.1}")
    }
}
fn pct(n: f64) -> String {
    format!("{}{}%", if n > 0.0 { "+" } else { "" }, rounded(n * 100.0))
}
fn slope_reason(reasons: &mut Vec<String>, slope: Option<f64>) {
    if let Some(s) = slope {
        reasons.push(format!("30 d move {}", pct(s)));
    }
}
fn advise(slug: &str, set: &str, request: &AdvisorRequest) -> Option<Verdict> {
    let calendar = request.market.get("calendar")?;
    let cal = calendar.get("primes")?.get(set)?;
    if !cal.is_object() {
        return None;
    }
    let market = request
        .market
        .get("items")
        .and_then(|m| m.get(slug))
        .unwrap_or(&Value::Null);
    let now = number(market, "median_now").or_else(|| number(market, "median_90d"));
    let history = request.history.as_ref();
    let series: Option<HistorySeries> = history
        .and_then(|h| h.get("items"))
        .and_then(|i| i.get(slug))
        .and_then(|v| v.get("median"))
        .and_then(Value::as_array)
        .map(|values| HistorySeries {
            median: values.iter().map(Value::as_f64).collect(),
            volume: Vec::new(),
            subtype: None,
        });
    let stats = series.as_ref().and_then(|s| year_stats(s, 20));
    let slope = series.as_ref().and_then(|s| slope30(&s.median));
    let since_release = days_since(string(cal, "released"), request.now_ms);
    if let Some(days) = since_release.filter(|d| *d >= 0.0 && *d < 45.0) {
        let mut reasons = vec![format!(
            "released {days} d ago - new-prime prices decay toward a floor over the first weeks"
        )];
        slope_reason(&mut reasons, slope);
        return Some(Verdict {
            advice: Advice::SellNow,
            reasons,
        });
    }
    if let Some(rc) = calendar.get("resurgence_current") {
        let active = rc
            .get("frames")
            .and_then(Value::as_array)
            .is_some_and(|frames| frames.iter().any(|f| f.as_str() == Some(set)))
            && string(rc, "from")
                .and_then(date_ms)
                .is_some_and(|d| d <= request.now_ms)
            && string(rc, "to")
                .and_then(date_ms)
                .is_some_and(|d| request.now_ms <= d);
        if active {
            let end = string(rc, "to")
                .and_then(date_ms)
                .and_then(|ms| DateTime::from_timestamp_millis(ms as i64))
                .map(|d| d.format("%Y-%m-%d").to_string())?;
            let mut reasons = vec![format!("Prime Resurgence is reprinting it until {end}")];
            let falling = slope.is_some_and(|s| s <= -0.05);
            reasons.push(match slope {
                Some(s) if falling => format!("price falling: {} over 30 d", pct(s)),
                Some(s) => format!("price holding so far ({} / 30 d)", pct(s)),
                None => "no price series yet".into(),
            });
            return Some(Verdict {
                advice: if falling {
                    Advice::SellNow
                } else {
                    Advice::Neutral
                },
                reasons,
            });
        }
    }
    let vaulted = cal.get("vaulted").and_then(Value::as_bool).unwrap_or(false);
    let vault = string(cal, "vault_date");
    let since_vault = if vaulted {
        days_since(vault, request.now_ms)
    } else {
        None
    };
    if let Some(days) = since_vault.filter(|d| *d >= 0.0 && *d < 270.0) {
        let pre = series.as_ref().and_then(|s| {
            pre_vault_median(history.and_then(|h| string(h, "start"))?, &s.median, vault?)
        });
        let ramp = pre.zip(now).filter(|(p, _)| *p > 0.0).map(|(p, n)| n / p);
        let below = ramp.map(|r| r < 1.6).unwrap_or_else(|| {
            stats
                .as_ref()
                .zip(now)
                .map(|(s, n)| n < s.high * 0.8)
                .unwrap_or(true)
        });
        if below {
            let mut reasons = vec![format!(
                "vaulted {days} d ago - the post-vault ramp typically runs for months"
            )];
            if let Some(r) = ramp {
                reasons.push(format!("now ×{} its pre-vault price", fixed_one(r)));
            }
            slope_reason(&mut reasons, slope);
            return Some(Verdict {
                advice: Advice::Hold,
                reasons,
            });
        }
    }
    if let Some((stats, now)) = stats
        .as_ref()
        .zip(now)
        .filter(|(s, n)| s.high >= s.low * 1.3 && *n >= s.high * 0.9)
    {
        let mut reasons = vec![format!(
            "at {}% of its 1-year high ({}p)",
            rounded(now / stats.high * 100.0),
            rounded(stats.high)
        )];
        slope_reason(&mut reasons, slope);
        return Some(Verdict {
            advice: Advice::SellNow,
            reasons,
        });
    }
    let to_vault = if vaulted {
        None
    } else {
        days_since(string(cal, "est_vault_date"), request.now_ms)
    };
    if let Some(days) = to_vault.filter(|d| *d < 0.0 && -*d <= 90.0) {
        return Some(Verdict {
            advice: Advice::Hold,
            reasons: vec![format!(
                "vault expected ~{} ({} d) - prices typically climb once relics stop dropping",
                string(cal, "est_vault_date").unwrap_or_default(),
                -days
            )],
        });
    }
    let mut reasons = vec![since_vault
        .map(|d| format!("vaulted {d} d ago"))
        .unwrap_or_else(|| "not vaulted".into())];
    if let Some((s, n)) = stats.as_ref().zip(now) {
        reasons.push(format!(
            "at {}% of its 1-year high",
            rounded(n / s.high * 100.0)
        ));
    }
    slope_reason(&mut reasons, slope);
    Some(Verdict {
        advice: Advice::Neutral,
        reasons,
    })
}
fn part_to_set(market: &Value) -> BTreeMap<String, String> {
    let mut mapping = BTreeMap::new();
    if let Some(sets) = market.get("set_to_parts").and_then(Value::as_object) {
        for (set, entry) in sets {
            mapping.insert(set.clone(), set.clone());
            if let Some(parts) = entry.get("parts").and_then(Value::as_array) {
                for part in parts {
                    if let Some(slug) = string(part, "slug") {
                        mapping.insert(slug.to_string(), set.clone());
                    }
                }
            }
        }
    }
    mapping
}
pub fn advise_owned(request: AdvisorRequest) -> BTreeMap<String, Verdict> {
    let mapping = part_to_set(&request.market);
    let mut out = BTreeMap::new();
    if !request.now_ms.is_finite() {
        return out;
    }
    for slug in request
        .slugs
        .iter()
        .collect::<std::collections::BTreeSet<_>>()
    {
        if let Some(verdict) = mapping
            .get(slug)
            .and_then(|set| advise(slug, set, &request))
        {
            out.insert(slug.clone(), verdict);
        }
    }
    out
}

fn validate_series(series: &HistorySeries) -> Result<(), String> {
    if series.median.iter().flatten().any(|n| !n.is_finite()) {
        return Err("History medians must be finite numbers or null.".into());
    }
    if slope30(&series.median).is_some_and(|n| !(n * 100.0).is_finite()) {
        return Err("History price movement exceeds the supported numeric range.".into());
    }
    Ok(())
}
pub fn validate_history_request(request: &HistoryRequest) -> Result<(), String> {
    validate_series(&request.series)?;
    if year_stats(&request.series, request.min_days.unwrap_or(20))
        .and_then(|stats| stats.delta_pct)
        .is_some_and(|n| !n.is_finite())
    {
        return Err("History price change exceeds the supported numeric range.".into());
    }
    Ok(())
}
pub fn validate_advisor_request(request: &AdvisorRequest) -> Result<(), String> {
    let mapping = part_to_set(&request.market);
    for slug in request
        .slugs
        .iter()
        .collect::<std::collections::BTreeSet<_>>()
    {
        let item = request
            .market
            .get("items")
            .and_then(|items| items.get(slug));
        for key in ["median_now", "median_90d"] {
            if item
                .and_then(|item| item.get(key))
                .is_some_and(|n| !n.is_null() && !n.is_number())
            {
                return Err("Advisor market prices must be numbers or null.".into());
            }
        }
        let medians = request
            .history
            .as_ref()
            .and_then(|h| h.get("items"))
            .and_then(|items| items.get(slug))
            .and_then(|series| series.get("median"));
        let Some(medians) = medians else {
            continue;
        };
        let values = medians
            .as_array()
            .ok_or("History medians must be an array.")?;
        if values.iter().any(|n| !n.is_null() && !n.is_number()) {
            return Err("History medians must be finite numbers or null.".into());
        }
        let series = HistorySeries {
            median: values.iter().map(Value::as_f64).collect(),
            volume: Vec::new(),
            subtype: None,
        };
        validate_series(&series)?;
        let now = item.and_then(|m| number(m, "median_now").or_else(|| number(m, "median_90d")));
        if let Some(now) = now {
            // A literal zero baseline already has a defined legacy display
            // (Infinity%). Tiny nonzero baselines must not overflow silently.
            if let Some(stats) = year_stats(&series, 20) {
                if stats.high != 0.0 && !(now / stats.high * 100.0).is_finite() {
                    return Err(
                        "Advisor price comparison exceeds the supported numeric range.".into(),
                    );
                }
            }
            let set = mapping.get(slug);
            let vault = set
                .and_then(|set| {
                    request
                        .market
                        .get("calendar")
                        .and_then(|c| c.get("primes"))
                        .and_then(|p| p.get(set))
                })
                .and_then(|cal| string(cal, "vault_date"));
            let start = request.history.as_ref().and_then(|h| string(h, "start"));
            if let Some(pre) = start
                .zip(vault)
                .and_then(|(start, vault)| pre_vault_median(start, &series.median, vault))
            {
                if !(now / pre).is_finite() {
                    return Err(
                        "Advisor vault comparison exceeds the supported numeric range.".into(),
                    );
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn numeric_boundary_contract() {
        let cases: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/advisor/numeric-boundaries.json"
        ))
        .unwrap();
        for case in cases.as_array().unwrap() {
            let validation = match case["operation"].as_str().unwrap() {
                "advisor" => validate_advisor_request(
                    &serde_json::from_value(case["input"].clone()).unwrap(),
                ),
                "history" => validate_history_request(
                    &serde_json::from_value(case["input"].clone()).unwrap(),
                ),
                "trade_session" => crate::trade_session::validate_session_request(
                    &serde_json::from_value(case["input"].clone()).unwrap(),
                ),
                _ => panic!("unknown fixture operation"),
            };
            if let Some(error) = case["error"].as_str() {
                assert_eq!(validation, Err(error.into()), "{}", case["name"]);
            } else {
                validation.unwrap();
                let result = advise_owned(serde_json::from_value(case["input"].clone()).unwrap());
                assert_eq!(
                    serde_json::to_value(result).unwrap(),
                    case["legacy"]["result"]
                );
            }
        }
    }
    #[test]
    fn history_shared_contract() {
        let cases: Value =
            serde_json::from_str(include_str!("../../../tests/fixtures/advisor/history.json"))
                .unwrap();
        for case in cases.as_array().unwrap() {
            let request = serde_json::from_value(case["request"].clone()).unwrap();
            validate_history_request(&request).unwrap();
            let actual = analyze_history(request);
            let expected: HistoryAnalysis =
                serde_json::from_value(case["expected"].clone()).unwrap();
            assert_eq!(actual, expected, "{}", case["name"]);
        }
    }
    #[test]
    fn advice_shared_contract() {
        let cases: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/advisor/verdicts.json"
        ))
        .unwrap();
        for case in cases.as_array().unwrap() {
            let request = serde_json::from_value(case["request"].clone()).unwrap();
            validate_advisor_request(&request).unwrap();
            let actual = serde_json::to_value(advise_owned(request)).unwrap();
            assert_eq!(actual, case["expected"], "{}", case["name"]);
        }
    }
    #[test]
    fn malformed_history_is_rejected_before_calculation() {
        let request = AdvisorRequest {
            slugs: vec!["x_set".into()],
            market: serde_json::json!({"calendar":{"primes":{"x_set":{"released":"bad"}}},"set_to_parts":{"x_set":{}}}),
            history: Some(serde_json::json!({"items":{"x_set":{"median":["bad"]}}})),
            now_ms: 0.0,
        };
        assert_eq!(
            validate_advisor_request(&request),
            Err("History medians must be finite numbers or null.".into())
        );
    }
}
