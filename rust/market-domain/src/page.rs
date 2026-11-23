//! Reading a view request the way the page wrote it. Request types parse
//! through one `serde_json::Value` and these readers rather than a derived
//! `Deserialize` each: the derives cost the view module, which every surface
//! downloads before it renders, several times the rules they fed. A field of
//! the wrong type reads as absent, as the page's own checks treated it, and
//! JSON's `null` for the page's `NaN` reads as absent too.

use serde_json::Value;

pub(crate) fn number(v: &Value, key: &str) -> Option<f64> {
    v.get(key).and_then(Value::as_f64)
}

pub(crate) fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(String::from)
}

pub(crate) fn flag(v: &Value, key: &str) -> Option<bool> {
    v.get(key).and_then(Value::as_bool)
}

/// A nested object; `null`, absence or any other value is none.
pub(crate) fn object<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    v.get(key).filter(|o| o.is_object())
}

pub(crate) fn list<'a>(v: &'a Value, key: &str) -> Option<&'a [Value]> {
    v.get(key).and_then(Value::as_array).map(Vec::as_slice)
}

/// A list of numbers, each non-number as `NaN`; a missing list is empty.
pub(crate) fn numbers(v: &Value, key: &str) -> Vec<f64> {
    list(v, key)
        .unwrap_or_default()
        .iter()
        .map(|n| n.as_f64().unwrap_or(f64::NAN))
        .collect()
}

/// `Deserialize` for request types that have `fn read(&Value) -> Self`.
macro_rules! deserialize_by_reading {
    ($($ty:ty),+ $(,)?) => {$(
        impl<'de> serde::Deserialize<'de> for $ty {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                Ok(Self::read(&<serde_json::Value as serde::Deserialize>::deserialize(d)?))
            }
        }
    )+};
}
pub(crate) use deserialize_by_reading;
