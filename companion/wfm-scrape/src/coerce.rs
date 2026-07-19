//! Permissive numeric-field parsing with coercion counting.
//!
//! Python's scraper reads stat fields with an *accidental* contract:
//! `d.get("median", 0) or 0`. That silently maps null / missing / 0 to 0, but
//! a numeric-string (`"33"`) survives as a truthy string and later blows up
//! `statistics.median` with a `TypeError` — a hard crash of the whole run with
//! an opaque traceback and no field context.
//!
//! This module makes the contract explicit and improves on it, per the
//! June "permissive parsing pattern":
//!   - `null`, missing, `""`  → `0.0`               (the tolerated absent case)
//!   - a JSON number          → that number         (the happy path)
//!   - a numeric-string        → parse it AND count  (tolerated, but tracked)
//!   - object / bool / array   → hard error + field path (contract violation)
//!   - non-finite (NaN / ±Inf) → hard error + field path
//!
//! The *count* of numeric-string coercions is accumulated across a whole
//! scrape; if it exceeds [`DEFAULT_MAX_COERCIONS`] the run is failed loudly
//! (a systemic upstream shape drift — WFM sending strings for every field —
//! would otherwise silently reshape the snapshot). A hard type error fails the
//! run immediately with the offending field's path.

use serde_json::Value;

/// Anomaly budget for numeric-string coercions across one full scrape.
///
/// A healthy scrape reads on the order of hundreds of thousands of numeric
/// fields, essentially all of which are JSON numbers — a numeric-string is an
/// anomaly, not the norm. 100 is generous enough that a handful of transient
/// oddities on individual items never abort a 45-minute run, yet a systemic
/// "WFM now sends everything as strings" drift (which would produce thousands)
/// trips it on the first fraction of items. It is an absolute anomaly budget,
/// not a rate — the type-drift analogue of run-scrape.sh's `MIN_ROWS` floor.
pub const DEFAULT_MAX_COERCIONS: u64 = 100;

/// Running tally of tolerated numeric-string coercions for a scrape.
#[derive(Debug, Default, Clone)]
pub struct Coercions {
    pub count: u64,
}

impl Coercions {
    pub fn new() -> Self {
        Coercions { count: 0 }
    }

    /// True once the coercion tally has passed `max` — the run should fail.
    pub fn exceeds(&self, max: u64) -> bool {
        self.count > max
    }
}

/// Coerce one JSON value at `field_path` to an `f64`, following the contract
/// documented on this module. `field_path` is threaded purely for a legible
/// error (e.g. `primed_continuity.90days[3].median`).
pub fn coerce_number(v: &Value, field_path: &str, coercions: &mut Coercions) -> Result<f64, String> {
    match v {
        Value::Null => Ok(0.0),
        Value::Number(n) => {
            let f = n
                .as_f64()
                .ok_or_else(|| format!("{field_path}: number not representable as f64"))?;
            if !f.is_finite() {
                return Err(format!("{field_path}: non-finite number"));
            }
            Ok(f)
        }
        Value::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                return Ok(0.0);
            }
            let f: f64 = t
                .parse()
                .map_err(|_| format!("{field_path}: non-numeric string {s:?}"))?;
            if !f.is_finite() {
                return Err(format!("{field_path}: non-finite numeric string {s:?}"));
            }
            coercions.count += 1;
            Ok(f)
        }
        Value::Bool(_) => Err(format!("{field_path}: boolean where a number was expected")),
        Value::Object(_) => Err(format!("{field_path}: object where a number was expected")),
        Value::Array(_) => Err(format!("{field_path}: array where a number was expected")),
    }
}

/// Coerce a named field of a JSON object, treating a missing key exactly like
/// `null` (Python's `d.get(key, 0)`).
pub fn coerce_field(
    obj: &Value,
    key: &str,
    field_path: &str,
    coercions: &mut Coercions,
) -> Result<f64, String> {
    match obj.get(key) {
        Some(v) => coerce_number(v, field_path, coercions),
        None => Ok(0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn c() -> Coercions {
        Coercions::new()
    }

    #[test]
    fn number_is_the_happy_path_no_count() {
        let mut co = c();
        assert_eq!(coerce_number(&json!(33), "f", &mut co).unwrap(), 33.0);
        assert_eq!(coerce_number(&json!(12.5), "f", &mut co).unwrap(), 12.5);
        assert_eq!(co.count, 0);
    }

    #[test]
    fn null_and_empty_string_are_zero_uncounted() {
        let mut co = c();
        assert_eq!(coerce_number(&json!(null), "f", &mut co).unwrap(), 0.0);
        assert_eq!(coerce_number(&json!(""), "f", &mut co).unwrap(), 0.0);
        assert_eq!(coerce_number(&json!("   "), "f", &mut co).unwrap(), 0.0);
        assert_eq!(co.count, 0);
    }

    #[test]
    fn missing_field_is_zero_like_python_get_default() {
        let mut co = c();
        let obj = json!({"median": 5});
        assert_eq!(coerce_field(&obj, "volume", "x.volume", &mut co).unwrap(), 0.0);
        assert_eq!(co.count, 0);
    }

    #[test]
    fn numeric_string_is_parsed_and_counted() {
        let mut co = c();
        assert_eq!(coerce_number(&json!("42"), "f", &mut co).unwrap(), 42.0);
        assert_eq!(coerce_number(&json!("42.5"), "f", &mut co).unwrap(), 42.5);
        assert_eq!(co.count, 2); // both counted
    }

    #[test]
    fn object_bool_array_are_hard_errors_with_path() {
        let mut co = c();
        let e = coerce_number(&json!({"a": 1}), "goopolla.median", &mut co).unwrap_err();
        assert!(e.contains("goopolla.median"), "{e}");
        assert!(coerce_number(&json!(true), "f", &mut co).is_err());
        assert!(coerce_number(&json!([1, 2]), "f", &mut co).is_err());
        assert_eq!(co.count, 0); // hard errors never count as coercions
    }

    #[test]
    fn non_numeric_string_is_a_hard_error() {
        let mut co = c();
        let e = coerce_number(&json!("soon"), "f.median", &mut co).unwrap_err();
        assert!(e.contains("f.median"), "{e}");
        assert!(e.contains("soon"), "{e}");
        assert_eq!(co.count, 0);
    }

    #[test]
    fn non_finite_numeric_string_is_rejected() {
        let mut co = c();
        assert!(coerce_number(&json!("inf"), "f", &mut co).is_err());
        assert!(coerce_number(&json!("NaN"), "f", &mut co).is_err());
        assert!(coerce_number(&json!("1e400"), "f", &mut co).is_err()); // overflows to +Inf
    }

    #[test]
    fn exceeds_fires_only_past_the_budget() {
        let mut co = c();
        co.count = 100;
        assert!(!co.exceeds(100));
        co.count = 101;
        assert!(co.exceeds(100));
    }
}
