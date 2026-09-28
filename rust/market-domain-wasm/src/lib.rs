//! The native decision contracts for the browser development preview.
//!
//! `?preview-desktop` has no Tauri host, so its `evaluate_domain` command runs
//! this module instead: the same `DomainRequest::execute` the desktop runs,
//! compiled to wasm32. The preview therefore shows native results, and there is
//! no second implementation to keep in step.
//!
//! The boundary is plain wasm memory - no bindgen - so building it needs only
//! the `wasm32-unknown-unknown` target. The page copies the request's UTF-8 into
//! a buffer from [`domain_alloc`], calls [`domain_evaluate`], reads
//! [`domain_result_len`] bytes of JSON at the returned pointer, and hands both
//! buffers back to [`domain_free`].

use market_domain::dispatch::DomainRequest;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Evaluate one request. The answer is `{"ok": <response>}` or
/// `{"error": "<message>"}` - what the desktop command resolves or rejects with.
pub fn evaluate_json(request: &str) -> String {
    let outcome = serde_json::from_str::<DomainRequest>(request)
        .map_err(|e| format!("Invalid calculation request: {e}"))
        .and_then(DomainRequest::execute)
        .and_then(|response| serde_json::to_value(response).map_err(|e| e.to_string()));
    let envelope = match outcome {
        Ok(result) => serde_json::json!({ "ok": result }),
        Err(error) => serde_json::json!({ "error": error }),
    };
    envelope.to_string()
}

static RESULT_LEN: AtomicUsize = AtomicUsize::new(0);

/// A zeroed buffer of exactly `len` bytes, owned by the caller until
/// [`domain_free`].
#[no_mangle]
pub extern "C" fn domain_alloc(len: usize) -> *mut u8 {
    Box::into_raw(vec![0_u8; len].into_boxed_slice()).cast()
}

/// Release a buffer from [`domain_alloc`] or [`domain_evaluate`].
///
/// # Safety
/// `ptr` and `len` must describe one such buffer, not yet freed.
#[no_mangle]
pub unsafe extern "C" fn domain_free(ptr: *mut u8, len: usize) {
    drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len)));
}

/// Evaluate the UTF-8 request at `ptr..ptr+len`. Returns the answer's buffer;
/// its length is [`domain_result_len`].
///
/// # Safety
/// `ptr` and `len` must describe readable memory, such as a buffer from
/// [`domain_alloc`].
#[no_mangle]
pub unsafe extern "C" fn domain_evaluate(ptr: *const u8, len: usize) -> *mut u8 {
    let input = std::slice::from_raw_parts(ptr, len);
    let answer = match std::str::from_utf8(input) {
        Ok(request) => evaluate_json(request),
        Err(_) => serde_json::json!({ "error": "The calculation request is not UTF-8." }).to_string(),
    };
    let bytes = answer.into_bytes().into_boxed_slice();
    RESULT_LEN.store(bytes.len(), Ordering::SeqCst);
    Box::into_raw(bytes).cast()
}

/// Length of the answer the last [`domain_evaluate`] returned.
#[no_mangle]
pub extern "C" fn domain_result_len() -> usize {
    RESULT_LEN.load(Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    /// serde_json keeps `5` and `5.0` apart; JSON consumers do not.
    fn numbers_as_f64(value: Value) -> Value {
        match value {
            Value::Number(n) => n.as_f64().map_or(Value::Number(n), |f| json!(f)),
            Value::Array(items) => Value::Array(items.into_iter().map(numbers_as_f64).collect()),
            Value::Object(map) => {
                Value::Object(map.into_iter().map(|(k, v)| (k, numbers_as_f64(v))).collect())
            }
            other => other,
        }
    }

    fn fixture(path: &str) -> Value {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
        serde_json::from_str(&std::fs::read_to_string(root.join(path)).unwrap()).unwrap()
    }

    /// Returns whether the case was answered (rather than rejected).
    fn assert_answers(operation: &str, input: &Value, expected: &Value, name: &Value) -> bool {
        let request = json!({ "operation": operation, "input": input }).to_string();
        let answer: Value = serde_json::from_str(&evaluate_json(&request)).unwrap();
        // The planner fixtures call the planners directly; the desktop's
        // dispatch validates first and rejects some of those inputs. The
        // preview must reject exactly what the desktop rejects.
        if let Some(error) = answer.get("error") {
            let native = serde_json::from_str::<DomainRequest>(&request).unwrap().execute();
            assert_eq!(native.err().as_deref(), error.as_str(), "{operation}: {name}");
            return false;
        }
        assert_eq!(
            numbers_as_f64(answer),
            numbers_as_f64(json!({ "ok": { "operation": operation, "result": expected } })),
            "{operation}: {name}"
        );
        true
    }

    #[test]
    fn every_shared_fixture_answers_as_the_desktop_would() {
        for (operation, path, list) in [
            ("advisor", "advisor/verdicts.json", None),
            ("history", "advisor/history.json", None),
            ("trade_session", "trade-session/selector.json", None),
            ("normalize_inventory", "inventory-normalization/cases.json", Some("cases")),
            ("score_inventory", "inventory-scoring/cases.json", Some("cases")),
        ] {
            let cases = fixture(path);
            let cases = list.map_or(&cases, |key| &cases[key]);
            let answered = cases.as_array().unwrap().iter()
                .filter(|case| assert_answers(operation, &case["request"], &case["expected"], &case["name"]))
                .count();
            assert!(answered > 0, "{operation}: every case was rejected");
        }
        let mut answered = 0;
        for case in fixture("planners/cases.json").as_array().unwrap() {
            let operation = match case["operation"].as_str().unwrap() {
                "sets" => "set_recos",
                "relic" => "relic_plan",
                "ducat" => "ducat_plan",
                "build" => "build_plan",
                other => panic!("unmapped planner operation {other}"),
            };
            answered += usize::from(assert_answers(operation, &case["input"], &case["expected"], &case["name"]));
        }
        assert!(answered > 0, "every planner case was rejected");
    }

    #[test]
    fn a_rejected_request_answers_with_the_native_error() {
        let answer: Value = serde_json::from_str(&evaluate_json("{\"operation\":\"nope\",\"input\":{}}")).unwrap();
        assert!(answer["error"].as_str().unwrap().starts_with("Invalid calculation request"));
        let too_far = json!({ "operation": "history", "input": {
            "series": { "start": "2026-01-01", "median": [1.0] }, "buckets": 10_001 } });
        let answer: Value = serde_json::from_str(&evaluate_json(&too_far.to_string())).unwrap();
        assert_eq!(answer, json!({ "error": "History bucket count is too large." }));
    }

    #[test]
    fn the_memory_boundary_round_trips_a_request() {
        let request = json!({ "operation": "history", "input": {
            "series": { "start": "2026-01-01", "median": [10.0, 12.0] } } })
        .to_string();
        let buffer = domain_alloc(request.len());
        unsafe {
            std::ptr::copy_nonoverlapping(request.as_ptr(), buffer, request.len());
            let answer = domain_evaluate(buffer, request.len());
            let len = domain_result_len();
            let text = std::str::from_utf8(std::slice::from_raw_parts(answer, len)).unwrap().to_owned();
            domain_free(buffer, request.len());
            domain_free(answer, len);
            assert_eq!(text, evaluate_json(&request));
        }
    }
}
