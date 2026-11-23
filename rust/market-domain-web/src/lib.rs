//! The view rules of `market_domain`, as a wasm32 module the site and the
//! desktop webview instantiate once before they render, so every call is
//! synchronous. No bindgen: the boundary is plain numbers, plus JSON in wasm
//! memory for rules over snapshot objects - the page copies a request's UTF-8
//! into a buffer from [`web_alloc`], calls [`web_call`], reads
//! [`web_result_len`] bytes of JSON at the returned pointer and frees both.
//! `frontend/src/domain/native.ts` is that page side.

use market_domain::calendar;
use market_domain::listing;
use market_domain::math::sell_priority;
use market_domain::trade_session;
use market_domain::web::WebRequest;
use std::sync::atomic::{AtomicUsize, Ordering};

use market_domain::browse::{self, BargainKind};
use market_domain::math::book_depth;

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

/// Whether a book is a queue at one price (`book_depth::crowded_book`); the
/// page passes its fields as `Number(x) || 0` would read them.
#[no_mangle]
pub extern "C" fn web_crowded_book(low5: f64, low20: f64, sells: f64) -> bool {
    book_depth::crowded_book(low5, low20, sells)
}

/// Move vs the 90-day baseline in percent; `NaN` in either is "not a number"
/// and `NaN` out is "no move to show".
#[no_mangle]
pub extern "C" fn web_delta_pct(median_now: f64, median_90d: f64) -> f64 {
    let read = |v: f64| (!v.is_nan()).then_some(v);
    browse::delta_pct(read(median_now), read(median_90d)).unwrap_or(f64::NAN)
}

/// Whether a bargain still holds on the live book; a `NaN` bid is no bid.
#[no_mangle]
pub extern "C" fn web_bargain_holds(flip: bool, seen_ask: f64, ask: f64, bid: f64) -> bool {
    let kind = if flip {
        BargainKind::Flip
    } else {
        BargainKind::Cheap
    };
    browse::bargain_holds(kind, seen_ask, ask, (!bid.is_nan()).then_some(bid))
}

/// The per-unit price of a listing whose `platinum` buys one lot; `NaN` for
/// none. The page passes an absent lot as `1` and a non-number as `NaN`.
#[no_mangle]
pub extern "C" fn web_order_unit_price(platinum: f64, lot: f64) -> f64 {
    listing::unit_price(platinum, lot).unwrap_or(f64::NAN)
}

/// The whole-platinum lot total for a per-unit price; `NaN` for none.
#[no_mangle]
pub extern "C" fn web_order_lot_price(unit: f64, lot: f64) -> f64 {
    listing::lot_price(unit, lot).unwrap_or(f64::NAN)
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

/// 0 for no nudge, 1 to list before the weekend, 2 during it; `weekday` is
/// `Date.prototype.getDay` in the reader's zone (`calendar::weekend_nudge`).
#[no_mangle]
pub extern "C" fn web_weekend_nudge(weekday: f64) -> u32 {
    match calendar::weekend_nudge(weekday) {
        None => 0,
        Some(calendar::WeekendNudge::Before) => 1,
        Some(calendar::WeekendNudge::During) => 2,
    }
}
/// Whether `quantity` copies can trade `lot` at a time; 1 or 0, as is `bulk`.
#[no_mangle]
pub extern "C" fn web_valid_session_lot(quantity: f64, lot: f64, bulk: i32) -> i32 {
    i32::from(trade_session::valid_session_lot(quantity, lot, bulk != 0))
}

/// The fewest trades that sell `quantity` copies; `bulk` is 1 or 0.
#[no_mangle]
pub extern "C" fn web_trades_to_sell(quantity: f64, bulk: i32) -> f64 {
    trade_session::trades_to_sell(quantity, bulk != 0)
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
        assert_eq!(web_clearing_price(20.0, 1.0, 0.0, 38.0, 0.0), 38.0);
        assert!(web_crowded_book(69.0, 70.0, 50.0));
        assert!(!web_crowded_book(50.0, 55.5, 40.0));
        assert_eq!(web_delta_pct(60.0, 50.0), 20.0);
        assert!(web_delta_pct(f64::NAN, 50.0).is_nan());
        assert!(web_delta_pct(25.0, 0.0).is_nan());
        assert!(web_bargain_holds(true, 30.0, 31.0, 38.0));
        assert!(!web_bargain_holds(true, 30.0, 31.0, f64::NAN));
        assert!(web_bargain_holds(false, 14.0, 14.0, f64::NAN));
        assert_eq!(web_order_unit_price(36.0, 5.0), 7.2);
        assert!(web_order_unit_price(12.0, 7.0).is_nan());
        assert_eq!(web_order_lot_price(7.2, 5.0), 36.0);
        assert!(web_order_lot_price(0.0, 1.0).is_nan());
        assert_eq!(
            [
                web_weekend_nudge(0.0),
                web_weekend_nudge(1.0),
                web_weekend_nudge(4.0),
                web_weekend_nudge(6.0)
            ],
            [2, 0, 1, 2]
        );
        assert_eq!(web_valid_session_lot(12.0, 6.0, 1), 1);
        assert_eq!(web_valid_session_lot(12.0, 6.0, 0), 0);
        assert_eq!(web_trades_to_sell(9.0, 1), 2.0);
        assert_eq!(web_trades_to_sell(9.0, 0), 9.0);
    }
}
