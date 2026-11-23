//! JavaScript number semantics the view rules mirror. A rule that moved from
//! TypeScript keeps its answers, so where JS and Rust disagree (`Math.round`
//! on a negative half, `Math.min` with NaN, what JSON can carry) the JS
//! reading is the contract.

use serde::de::{self, Deserializer, Visitor};
use serde::Deserialize;
use std::fmt;
use ts_rs::TS;

/// A JS number as the page holds it. JSON has no NaN or infinities, so the
/// page spells those as strings (`jsNumber` in frontend/src/domain/native.ts),
/// after `Number(x)` - the reading a JS comparison or arithmetic gives `x`.
#[derive(Clone, Copy, Debug, PartialEq, TS)]
#[ts(type = "number | \"NaN\" | \"Infinity\" | \"-Infinity\"")]
pub struct JsNumber(pub f64);

impl<'de> Deserialize<'de> for JsNumber {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Number;
        impl Visitor<'_> for Number {
            type Value = JsNumber;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a number, \"NaN\", \"Infinity\" or \"-Infinity\"")
            }
            fn visit_f64<E>(self, v: f64) -> Result<JsNumber, E> {
                Ok(JsNumber(v))
            }
            #[allow(clippy::cast_precision_loss, reason = "a JS number is an f64")]
            fn visit_i64<E>(self, v: i64) -> Result<JsNumber, E> {
                Ok(JsNumber(v as f64))
            }
            #[allow(clippy::cast_precision_loss, reason = "a JS number is an f64")]
            fn visit_u64<E>(self, v: u64) -> Result<JsNumber, E> {
                Ok(JsNumber(v as f64))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<JsNumber, E> {
                match v {
                    "NaN" => Ok(JsNumber(f64::NAN)),
                    "Infinity" => Ok(JsNumber(f64::INFINITY)),
                    "-Infinity" => Ok(JsNumber(f64::NEG_INFINITY)),
                    _ => Err(E::invalid_value(de::Unexpected::Str(v), &self)),
                }
            }
        }
        deserializer.deserialize_any(Number)
    }
}

/// `Math.round`: halves round up, so -2.5 is -2 where `f64::round` gives -3.
pub fn round(x: f64) -> f64 {
    let floor = x.floor();
    if x - floor >= 0.5 {
        floor + 1.0
    } else {
        floor
    }
}

/// `Math.min` of two: NaN wins, and -0 is below 0.
pub fn min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == b {
        if a.is_sign_negative() {
            a
        } else {
            b
        }
    } else {
        a.min(b)
    }
}

/// `Math.max` of two: NaN wins, and 0 is above -0.
pub fn max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == b {
        if a.is_sign_positive() {
            a
        } else {
            b
        }
    } else {
        a.max(b)
    }
}

/// `x || 0` for a number: NaN and -0 read as 0.
pub fn or_zero(x: f64) -> f64 {
    if x.is_nan() || x == 0.0 {
        0.0
    } else {
        x
    }
}

/// Where `array.slice(at)` starts on a list of `len` (and `slice(0, at)` ends).
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "clamped to 0..=len first"
)]
pub fn slice_index(at: f64, len: usize) -> usize {
    let len_f = len as f64;
    let at = if at.is_nan() { 0.0 } else { at.trunc() };
    let index = if at < 0.0 {
        (len_f + at).max(0.0)
    } else {
        at.min(len_f)
    };
    index as usize
}

/// The characters JS `trim()` and `\s` match: WhiteSpace and LineTerminator.
/// Not `char::is_whitespace`, which adds U+0085 and drops U+FEFF.
pub fn is_space(c: char) -> bool {
    matches!(
        c,
        '\u{9}'..='\u{d}'
            | ' '
            | '\u{a0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200a}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{3000}'
            | '\u{feff}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_read_as_js_spells_them() {
        let read = |s: &str| serde_json::from_str::<JsNumber>(s).map(|n| n.0);
        assert_eq!(read("5").unwrap(), 5.0);
        assert_eq!(read("-1.5").unwrap(), -1.5);
        assert!(read("\"NaN\"").unwrap().is_nan());
        assert_eq!(read("\"Infinity\"").unwrap(), f64::INFINITY);
        assert_eq!(read("\"-Infinity\"").unwrap(), f64::NEG_INFINITY);
        assert!(read("null").is_err());
        assert!(read("\"5\"").is_err());
    }

    #[test]
    fn rounding_and_extremes_follow_math() {
        assert_eq!(round(2.5), 3.0);
        assert_eq!(round(-2.5), -2.0);
        assert_eq!(round(0.499_999_999_999_999_94), 0.0);
        assert_eq!(round(-0.4), -0.0);
        assert!(min(1.0, f64::NAN).is_nan());
        assert!(max(f64::NAN, 1.0).is_nan());
        assert!(min(0.0, -0.0).is_sign_negative());
        assert!(max(-0.0, 0.0).is_sign_positive());
        assert_eq!(or_zero(f64::NAN), 0.0);
        assert_eq!(or_zero(3.0), 3.0);
    }

    #[test]
    fn slice_indices_clamp_like_array_slice() {
        assert_eq!(slice_index(2.0, 4), 2);
        assert_eq!(slice_index(8.0, 4), 4);
        assert_eq!(slice_index(-1.0, 4), 3);
        assert_eq!(slice_index(-9.0, 4), 0);
        assert_eq!(slice_index(f64::NAN, 4), 0);
        assert_eq!(slice_index(1.7, 4), 1);
        assert_eq!(slice_index(f64::INFINITY, 4), 4);
    }

    #[test]
    fn spaces_are_the_js_set() {
        assert!(is_space('\u{feff}'));
        assert!(!is_space('\u{85}'));
        assert!(is_space('\n'));
    }
}
