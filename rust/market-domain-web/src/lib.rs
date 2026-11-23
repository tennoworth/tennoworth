//! The view rules of `market_domain`, as a wasm32 module the site and the
//! desktop webview instantiate once before they render, so every call is
//! synchronous. No bindgen: the boundary is plain numbers, plus JSON in wasm
//! memory for rules over snapshot objects - the page copies a request's UTF-8
//! into a buffer from [`web_alloc`], calls [`web_call`], reads
//! [`web_result_len`] bytes of JSON at the returned pointer and frees both.
//! `frontend/src/domain/native.ts` is that page side.

use market_domain::math::sell_priority;
use market_domain::rivens::{self, appraise};
use market_domain::web::WebRequest;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Answer one [`WebRequest`]: `{"ok": <response>}` or `{"error": "<message>"}`.
pub fn call_json(request: &str) -> String {
    let outcome = serde_json::from_str::<WebRequest>(request)
        .map_err(|e| format!("Invalid view request: {e}"))
        .and_then(WebRequest::execute)
        .and_then(|response| serde_json::to_string(&response).map_err(|e| e.to_string()));
    match outcome {
        Ok(result) => format!(r#"{{"ok":{result}}}"#),
        Err(error) => error_json(&error),
    }
}

fn error_json(message: &str) -> String {
    let quoted = serde_json::to_string(message).unwrap_or_else(|_| String::from(r#""""#));
    format!(r#"{{"error":{quoted}}}"#)
}

/// Copies to sell after the reserve and leveled copies.
#[no_mangle]
pub extern "C" fn web_sellable_qty(count: f64, reserve: f64, leveled: f64) -> f64 {
    sell_priority::sellable(count, reserve, leveled)
}

/// Spare copies; a non-finite `kept_lvl` is "no individual copy".
#[no_mangle]
pub extern "C" fn web_spare_qty(count: f64, kept_lvl: f64, leveled: f64) -> f64 {
    sell_priority::spare_qty(count, kept_lvl.is_finite().then_some(kept_lvl), leveled)
}

/// What a listing would realistically clear at (`sell_priority::clearing_price`).
/// The page passes its fields as `Number(x) || 0` would read them.
#[no_mangle]
pub extern "C" fn web_clearing_price(
    vol: f64,
    low_sell: f64,
    avg: f64,
    median_now: f64,
    median_90d: f64,
) -> f64 {
    sell_priority::clearing_price(&sell_priority::PricedEntry {
        vol,
        low_sell,
        avg,
        median_now,
        median_90d,
    })
}

static RESULT_LEN: AtomicUsize = AtomicUsize::new(0);

/// A zeroed buffer of exactly `len` bytes, owned by the caller until [`web_free`].
#[no_mangle]
pub extern "C" fn web_alloc(len: usize) -> *mut u8 {
    Box::into_raw(vec![0_u8; len].into_boxed_slice()).cast()
}

/// Release a buffer from [`web_alloc`] or [`web_call`].
///
/// # Safety
/// `ptr` and `len` must describe one such buffer, not yet freed.
#[no_mangle]
pub unsafe extern "C" fn web_free(ptr: *mut u8, len: usize) {
    drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len)));
}

/// Answer the UTF-8 request at `ptr..ptr+len`; its length is [`web_result_len`].
///
/// # Safety
/// `ptr` and `len` must describe readable memory, such as a buffer from
/// [`web_alloc`].
#[no_mangle]
pub unsafe extern "C" fn web_call(ptr: *const u8, len: usize) -> *mut u8 {
    let input = std::slice::from_raw_parts(ptr, len);
    let answer = match std::str::from_utf8(input) {
        Ok(request) => call_json(request),
        Err(_) => error_json("The view request is not UTF-8."),
    };
    let bytes = answer.into_bytes().into_boxed_slice();
    RESULT_LEN.store(bytes.len(), Ordering::SeqCst);
    Box::into_raw(bytes).cast()
}

/// Length of the answer the last [`web_call`] returned.
#[no_mangle]
pub extern "C" fn web_result_len() -> usize {
    RESULT_LEN.load(Ordering::SeqCst)
}

/// The DE band a riven reads (`rivens::band_tier`): 0 none, 1 unrolled,
/// 2 rolled. The flags are whether DE published each tier; integers rather
/// than `bool`, which the page could hand over as any number.
#[no_mangle]
pub extern "C" fn web_riven_band(rerolls: f64, has_unrolled: u32, has_rolled: u32) -> u32 {
    match rivens::band_tier(rerolls, has_unrolled != 0, has_rolled != 0) {
        None => 0,
        Some(rivens::BandTier::Unrolled) => 1,
        Some(rivens::BandTier::Rolled) => 2,
    }
}

/// `appraise::rerolled_discount` of the two bands' medians (0 for a missing
/// band); NaN when either has no price.
#[no_mangle]
pub extern "C" fn web_riven_rerolled_discount(unrolled_median: f64, rolled_median: f64) -> f64 {
    appraise::rerolled_discount(unrolled_median, rolled_median).unwrap_or(f64::NAN)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    #[test]
    fn answers_and_rejects_as_an_envelope() {
        let ok: Value = serde_json::from_str(&call_json(
            r#"{"op":"select_picks","input":{"rows":[{"sellable":1,"patience":false,"hold":false,"score":20}]}}"#,
        ))
        .unwrap();
        assert_eq!(ok, json!({ "ok": { "op": "select_picks", "result": [0] } }));
        let error: Value =
            serde_json::from_str(&call_json(r#"{"op":"place_order","input":{}}"#)).unwrap();
        assert!(error["error"]
            .as_str()
            .unwrap()
            .starts_with("Invalid view request"));
    }

    #[test]
    fn the_scalar_exports_are_the_domain_rules() {
        assert_eq!(web_sellable_qty(5.0, 1.0, 2.0), 3.0);
        assert_eq!(web_spare_qty(4.0, 5.0, 0.0), 4.0);
        assert_eq!(web_spare_qty(4.0, f64::NAN, 0.0), 3.0);
        assert_eq!(web_clearing_price(20.0, 1.0, 0.0, 38.0, 0.0), 38.0);
    }

    #[test]
    fn the_riven_exports_are_the_domain_rules() {
        assert_eq!(web_riven_band(3.0, 1, 1), 2);
        assert_eq!(web_riven_band(3.0, 1, 0), 1);
        assert_eq!(web_riven_band(0.0, 0, 7), 2);
        assert_eq!(web_riven_band(0.0, 0, 0), 0);
        assert!((web_riven_rerolled_discount(200.0, 176.0) - 0.12).abs() < 1e-12);
        assert!(web_riven_rerolled_discount(200.0, 0.0).is_nan());
    }

    #[test]
    fn requests_read_the_pages_floats_exactly() {
        // A ratio as JSON.stringify prints it: the default parser reads this
        // one a unit in the last place off.
        let answer: Value = serde_json::from_str(&call_json(
            r#"{"op":"appraise_riven","input":{"price":null,"tier":{"avg":0.9090909090909091,"median":1.0000000000000002,"min":0,"max":0,"stddev":0,"pop":0},"rerolls":0,"locked":false}}"#,
        ))
        .unwrap();
        let dist = &answer["ok"]["result"]["dist"];
        assert_eq!(dist["avg"].as_f64(), Some(0.909_090_909_090_909_1));
        assert_eq!(dist["median"].as_f64(), Some(1.0000000000000002));
    }
}
