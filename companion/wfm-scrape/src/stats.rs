//! Closed-trade statistics parsing — the v1 `/v1/items/{slug}/statistics`
//! payload's `statistics_closed.48hours` / `.90days` day-rows.
//!
//! Each row becomes a [`market_math::StatsDay`]. Numeric fields flow through
//! [`crate::coerce`]; `mod_rank` keeps the absent / null / number tri-state the
//! market-math tier filter depends on; non-object entries are skipped exactly
//! like Python's `if isinstance(d, dict)`.
//!
//! FIELD-READ AUDIT (the orders.rs coercion-safety fix's sibling check): unlike
//! orders — where liveness is a cheap local predicate we can apply before
//! coercing — the stats rank-0 / subtype narrowing lives in `market-math`,
//! which is a pure, dependency-free crate and therefore cannot thread the
//! coercion counter. So every window row is coerced up front, before the
//! rank0/subtype filters run. `volume` matches Python exactly (Python reads it
//! on ALL rows via `canonical_subtype`); `median` / `max_price` / `avg_price`
//! are read by Python only on rank-0 + right-subtype rows, so we over-coerce
//! them on ranked / wrong-subtype rows. In healthy data those are plain numbers
//! → identical output; a divergence needs junk confined to a filtered-out row
//! (a partial schema drift) — a tiny surface that fixing would cost a breach of
//! the market-math purity boundary, so it is accepted and documented, not fixed.

use serde_json::Value;

use market_math::StatsDay;

use crate::coerce::{coerce_field, Coercions};

/// Parse the (envelope-unwrapped) v1 statistics payload into
/// `(48h_rows, 90d_rows)`. A missing `statistics_closed` or window yields an
/// empty vector, matching Python's `.get(..., {})` / `.get(..., [])` chain.
pub fn parse_stats(
    payload: &Value,
    url_name: &str,
    co: &mut Coercions,
) -> Result<(Vec<StatsDay>, Vec<StatsDay>), String> {
    let closed = payload.get("statistics_closed");
    let recent = parse_window(closed, "48hours", url_name, co)?;
    let nineties = parse_window(closed, "90days", url_name, co)?;
    Ok((recent, nineties))
}

fn parse_window(
    closed: Option<&Value>,
    window: &str,
    url_name: &str,
    co: &mut Coercions,
) -> Result<Vec<StatsDay>, String> {
    let arr = match closed.and_then(|c| c.get(window)).and_then(|w| w.as_array()) {
        Some(a) => a,
        None => return Ok(vec![]),
    };
    let mut out = Vec::with_capacity(arr.len());
    for (i, d) in arr.iter().enumerate() {
        if !d.is_object() {
            continue; // Python: if isinstance(d, dict)
        }
        let path = format!("{url_name}.{window}[{i}]");
        out.push(StatsDay {
            median: coerce_field(d, "median", &format!("{path}.median"), co)?,
            max_price: coerce_field(d, "max_price", &format!("{path}.max_price"), co)?,
            volume: coerce_field(d, "volume", &format!("{path}.volume"), co)?,
            avg_price: coerce_field(d, "avg_price", &format!("{path}.avg_price"), co)?,
            subtype: d.get("subtype").and_then(|s| s.as_str()).map(|s| s.to_string()),
            mod_rank: parse_mod_rank(d.get("mod_rank")),
        });
    }
    Ok(out)
}

/// The stats `mod_rank` tri-state:
///   - key absent          → `None`         (untiered item: weapon/set/relic)
///   - key present, `null` → `Some(None)`   (tiered; counts as the rank-0 tier)
///   - key present, number → `Some(Some(n))`
///
/// A present-but-non-numeric value (never seen from WFM) is treated as the
/// rank-0 tier so the item is still marked tiered rather than silently untiered.
fn parse_mod_rank(v: Option<&Value>) -> Option<Option<i64>> {
    match v {
        None => None,
        Some(Value::Null) => Some(None),
        Some(x) => Some(Some(x.as_i64().or_else(|| x.as_f64().map(|f| f as i64)).unwrap_or(0))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(payload: &Value) -> (Vec<StatsDay>, Vec<StatsDay>) {
        let mut co = Coercions::new();
        parse_stats(payload, "slug", &mut co).unwrap()
    }

    #[test]
    fn extracts_both_windows_and_coerces_numbers() {
        let payload = json!({
            "statistics_closed": {
                "48hours": [{"median": 42, "max_price": 50, "volume": 8, "avg_price": 43.2}],
                "90days": [{"median": 33.0, "volume": 5}]
            }
        });
        let (recent, nineties) = parse(&payload);
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].median, 42.0);
        assert_eq!(recent[0].avg_price, 43.2);
        assert_eq!(nineties[0].median, 33.0);
        assert_eq!(nineties[0].max_price, 0.0); // missing → 0
    }

    #[test]
    fn missing_statistics_closed_yields_empty() {
        let (r, n) = parse(&json!({}));
        assert!(r.is_empty() && n.is_empty());
    }

    #[test]
    fn non_object_day_rows_are_skipped() {
        let payload = json!({"statistics_closed": {"48hours": [null, 5, {"median": 10, "volume": 1}]}});
        let (recent, _) = parse(&payload);
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].median, 10.0);
    }

    #[test]
    fn mod_rank_tri_state() {
        let payload = json!({"statistics_closed": {"90days": [
            {"median": 10},                 // absent → untiered
            {"median": 20, "mod_rank": null}, // null → Some(None)
            {"median": 30, "mod_rank": 0},    // Some(Some(0))
            {"median": 40, "mod_rank": 10}    // Some(Some(10))
        ]}});
        let (_, n) = parse(&payload);
        assert_eq!(n[0].mod_rank, None);
        assert_eq!(n[1].mod_rank, Some(None));
        assert_eq!(n[2].mod_rank, Some(Some(0)));
        assert_eq!(n[3].mod_rank, Some(Some(10)));
    }

    #[test]
    fn subtype_carried_and_null_becomes_none() {
        let payload = json!({"statistics_closed": {"48hours": [
            {"median": 5, "subtype": "intact"},
            {"median": 6, "subtype": null}
        ]}});
        let (r, _) = parse(&payload);
        assert_eq!(r[0].subtype.as_deref(), Some("intact"));
        assert_eq!(r[1].subtype, None);
    }

    #[test]
    fn numeric_string_field_is_counted() {
        let mut co = Coercions::new();
        let payload = json!({"statistics_closed": {"48hours": [{"median": "33", "volume": 2}]}});
        let (r, _) = parse_stats(&payload, "slug", &mut co).unwrap();
        assert_eq!(r[0].median, 33.0);
        assert_eq!(co.count, 1);
    }

    #[test]
    fn object_valued_field_errors_with_path() {
        let mut co = Coercions::new();
        let payload = json!({"statistics_closed": {"90days": [{"median": {"nested": 1}}]}});
        let err = parse_stats(&payload, "goopolla", &mut co).unwrap_err();
        assert!(err.contains("goopolla.90days[0].median"), "{err}");
    }
}
