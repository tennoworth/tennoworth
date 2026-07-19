//! Permissive number parsing to match Python's `float(x) or 0` semantics.
//!
//! Upstream CSV fields and WFM JSON responses carry shapes Python's
//! `float()/int()` silently absorb (numeric strings, empty strings → 0,
//! null → 0). Rust must be equally permissive but LOUD on schema drift:
//! every string→number coercion increments a counter, and the caller gates
//! on a small baseline — permissive to known slop, fail above threshold.

use serde_json::Value;

#[derive(Debug, Default)]
pub struct Coercions {
    pub string_to_number: u64,
}

impl Coercions {
    pub fn total(&self) -> u64 {
        self.string_to_number
    }
}

pub fn coerce_f64(v: &Value, path: &str, c: &mut Coercions) -> Result<f64, String> {
    match v {
        Value::Null => Ok(0.0),
        Value::String(s) if s.is_empty() => Ok(0.0),
        Value::String(s) => {
            let n: f64 = s
                .parse()
                .map_err(|_| format!("{path}: non-numeric string {s:?}"))?;
            if n.is_nan() || n.is_infinite() {
                return Err(format!("{path}: non-finite value {n}"));
            }
            c.string_to_number += 1;
            Ok(n)
        }
        Value::Number(n) => {
            let x = n.as_f64().ok_or_else(|| format!("{path}: unrepresentable number {n}"))?;
            if x.is_nan() || x.is_infinite() {
                return Err(format!("{path}: non-finite value {x}"));
            }
            Ok(x)
        }
        Value::Bool(_) => Err(format!("{path}: boolean where number expected")),
        Value::Array(_) => Err(format!("{path}: array where number expected")),
        Value::Object(_) => Err(format!("{path}: object where number expected")),
    }
}

pub fn coerce_i64(v: &Value, path: &str, c: &mut Coercions) -> Result<i64, String> {
    match v {
        Value::Null => Ok(0),
        Value::String(s) if s.is_empty() => Ok(0),
        Value::String(s) => {
            let n: i64 = s
                .parse()
                .map_err(|_| format!("{path}: non-integer string {s:?}"))?;
            c.string_to_number += 1;
            Ok(n)
        }
        Value::Number(n) => n
            .as_i64()
            .ok_or_else(|| format!("{path}: non-integer number {n}")),
        Value::Bool(_) => Err(format!("{path}: boolean where integer expected")),
        Value::Array(_) => Err(format!("{path}: array where integer expected")),
        Value::Object(_) => Err(format!("{path}: object where integer expected")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn f64_null_and_empty_string_are_zero() {
        let mut c = Coercions::default();
        assert_eq!(coerce_f64(&Value::Null, "x", &mut c), Ok(0.0));
        assert_eq!(coerce_f64(&json!(""), "x", &mut c), Ok(0.0));
        assert_eq!(c.string_to_number, 0);
    }

    #[test]
    fn f64_numeric_string_is_parsed_and_counted() {
        let mut c = Coercions::default();
        assert_eq!(coerce_f64(&json!("3.14"), "x", &mut c), Ok(3.14));
        assert_eq!(coerce_f64(&json!("42"), "x", &mut c), Ok(42.0));
        assert_eq!(c.string_to_number, 2);
    }

    #[test]
    fn f64_rejects_non_numeric_strings() {
        let mut c = Coercions::default();
        assert!(coerce_f64(&json!("bad"), "x", &mut c).is_err());
    }

    #[test]
    fn f64_rejects_nan_and_inf() {
        let mut c = Coercions::default();
        assert!(coerce_f64(&json!("NaN"), "x", &mut c).is_err());
        assert!(coerce_f64(&json!("Infinity"), "x", &mut c).is_err());
        assert!(coerce_f64(&json!("-Infinity"), "x", &mut c).is_err());
    }

    #[test]
    fn f64_rejects_bool_array_object() {
        let mut c = Coercions::default();
        assert!(coerce_f64(&json!(true), "x", &mut c).is_err());
        assert!(coerce_f64(&json!([]), "x", &mut c).is_err());
        assert!(coerce_f64(&json!({}), "x", &mut c).is_err());
    }

    #[test]
    fn i64_null_and_empty_are_zero() {
        let mut c = Coercions::default();
        assert_eq!(coerce_i64(&Value::Null, "x", &mut c), Ok(0));
        assert_eq!(coerce_i64(&json!(""), "x", &mut c), Ok(0));
        assert_eq!(c.string_to_number, 0);
    }

    #[test]
    fn i64_numeric_string_is_parsed_and_counted() {
        let mut c = Coercions::default();
        assert_eq!(coerce_i64(&json!("42"), "x", &mut c), Ok(42));
        assert_eq!(c.string_to_number, 1);
    }

    #[test]
    fn i64_rejects_float_as_integer() {
        let mut c = Coercions::default();
        assert!(coerce_i64(&json!(3.14), "x", &mut c).is_err());
    }

    #[test]
    fn i64_rejects_bool() {
        let mut c = Coercions::default();
        assert!(coerce_i64(&json!(false), "x", &mut c).is_err());
        assert!(coerce_i64(&json!(true), "x", &mut c).is_err());
    }
}
