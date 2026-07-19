//! Scrape transport: a status-aware GET trait, Python-faithful retry/backoff,
//! and injectable pacing.
//!
//! The converter's [`crate::fetch::Http`] collapses every non-2xx into an
//! error string, which cannot express the 429-vs-5xx-vs-transport distinction
//! `wfm_demand.py`'s `fetch_json` acts on. This trait keeps the status so the
//! retry loop can reproduce that behavior exactly, and stays fixture-driven so
//! backoff, pacing, and exhaustion are all testable with zero real sleeps.

use std::time::Duration;

use serde_json::Value;

/// Fixed post-request delay — Python's `REQUEST_DELAY = 0.34`. The scraper
/// sleeps this after EACH request (two per item), giving ~2.9 req/s. This is a
/// fixed spacing, not a token bucket: `wfm-client`'s `RateLimiter` models a
/// bursty 3/s window instead, so it is deliberately not used here — matching
/// Python's actual sleep logic is the fidelity requirement.
pub const REQUEST_DELAY: Duration = Duration::from_millis(340);

/// Number of attempts per request — Python's `fetch_json(retries=3)`.
pub const RETRIES: u32 = 3;

/// Outcome of a single GET, preserving enough status to drive Python's retry.
pub enum HttpOutcome {
    /// 2xx with a successfully-parsed JSON body.
    Ok(Value),
    /// HTTP 429 — Cloudflare/WFM rate limit. Backs off and retries.
    RateLimited,
    /// Any other non-2xx (4xx/5xx). Python's `raise_for_status()` path.
    HttpError(u16),
    /// Connection/timeout/read/parse failure. Python's other
    /// `RequestException` path (a `r.json()` decode error lands here too).
    Transport(String),
}

/// Status-aware GET. Every scrape endpoint goes through this so a fixture can
/// stand in for the network.
pub trait ScrapeHttp {
    fn get(&self, url: &str) -> HttpOutcome;
}

/// Injected sleeper so backoff + pacing are deterministic in tests.
pub trait Sleeper {
    fn sleep(&self, dur: Duration);
}

/// Real time — the production sleeper.
pub struct RealSleeper;
impl Sleeper for RealSleeper {
    fn sleep(&self, dur: Duration) {
        std::thread::sleep(dur);
    }
}

/// Never sleeps — used in fixture mode so the parity subprocess is instant.
pub struct NoopSleeper;
impl Sleeper for NoopSleeper {
    fn sleep(&self, _dur: Duration) {}
}

/// Records requested sleeps instead of sleeping — lets tests assert the exact
/// backoff/pacing schedule.
pub struct RecordingSleeper {
    pub sleeps: std::cell::RefCell<Vec<Duration>>,
}
impl RecordingSleeper {
    pub fn new() -> Self {
        RecordingSleeper {
            sleeps: std::cell::RefCell::new(Vec::new()),
        }
    }
    pub fn recorded(&self) -> Vec<Duration> {
        self.sleeps.borrow().clone()
    }
}
impl Default for RecordingSleeper {
    fn default() -> Self {
        Self::new()
    }
}
impl Sleeper for RecordingSleeper {
    fn sleep(&self, dur: Duration) {
        self.sleeps.borrow_mut().push(dur);
    }
}

/// Exponential backoff, Python's `2 ** attempt`: 1s, 2s, 4s.
fn backoff(attempt: u32) -> Duration {
    Duration::from_secs(1u64 << attempt)
}

/// WFM's envelope, unwrapped Python-order: `payload` first, then `data`, else
/// the bare body. (`wfm_client::unwrap_envelope` prefers `data` — the opposite
/// — so it is intentionally not reused; `fetch_json` in the scraper checks
/// `payload` first.)
fn unwrap_payload_first(body: Value) -> Value {
    if let Value::Object(mut m) = body {
        if let Some(p) = m.remove("payload") {
            return p;
        }
        if let Some(d) = m.remove("data") {
            return d;
        }
        return Value::Object(m);
    }
    body
}

/// GET with retry, a 1:1 port of `wfm_demand.py`'s `fetch_json`:
///   - 2xx → unwrap the envelope and return it.
///   - 429 → sleep `2**attempt` and retry — INCLUDING after the final attempt,
///     after which it returns `None` (Python sleeps then falls out of the loop).
///   - 4xx/5xx/transport → return `None` immediately on the last attempt,
///     otherwise sleep `2**attempt` and retry (no sleep on the last attempt).
///
/// Returns `None` once attempts are exhausted — the caller skips the item, the
/// exact "truncated but exit 0" behavior run-scrape.sh's row-floor guards.
pub fn fetch_json(http: &dyn ScrapeHttp, sleeper: &dyn Sleeper, url: &str) -> Option<Value> {
    for attempt in 0..RETRIES {
        match http.get(url) {
            HttpOutcome::Ok(body) => return Some(unwrap_payload_first(body)),
            HttpOutcome::RateLimited => {
                // 429 backs off on every attempt, the last one included.
                sleeper.sleep(backoff(attempt));
            }
            HttpOutcome::HttpError(_) | HttpOutcome::Transport(_) => {
                if attempt + 1 == RETRIES {
                    return None;
                }
                sleeper.sleep(backoff(attempt));
            }
        }
    }
    None
}

/// Live transport over `wfm_client`'s browser-UA client. Sends the EXACT header
/// set `wfm_demand.py` sends — `User-Agent` (via the client), `Platform`,
/// `Language` — and deliberately NOT `Crossplay` (the Python scraper omits it;
/// `wfm_client::wfm_headers` would add it, so it is not used here).
pub struct LiveScrapeHttp {
    pub client: reqwest::blocking::Client,
    pub platform: String,
}

impl ScrapeHttp for LiveScrapeHttp {
    fn get(&self, url: &str) -> HttpOutcome {
        let resp = self
            .client
            .get(url)
            .header("Platform", &self.platform)
            .header("Language", "en")
            .send();
        match resp {
            Ok(r) => {
                let status = r.status();
                if status.as_u16() == 429 {
                    return HttpOutcome::RateLimited;
                }
                if !status.is_success() {
                    return HttpOutcome::HttpError(status.as_u16());
                }
                match r.text() {
                    Ok(body) => match serde_json::from_str(&body) {
                        Ok(v) => HttpOutcome::Ok(v),
                        Err(e) => HttpOutcome::Transport(format!("{url}: JSON parse: {e}")),
                    },
                    Err(e) => HttpOutcome::Transport(format!("{url}: read body: {e}")),
                }
            }
            Err(e) => HttpOutcome::Transport(format!("{url}: {e}")),
        }
    }
}

/// Fixture transport for `--fixtures-dir` mode: a URL→response map. A URL absent
/// from the map is a transport error (which, after retries, makes the caller
/// skip that item — never a panic).
///
/// FIXTURE RESPONSE FORMAT (kept byte-identical to the Python parity fake in
/// `tests/test_scrape_parity.py`, so both scrapers see the same scripted world):
///   - a bare JSON body (object)          → HTTP 200 with that body,
///   - `{"status": <int>, "body": <json>}` → that status (429 → rate-limited,
///     other non-2xx → HttpError), the given body on 2xx,
///   - a JSON ARRAY                        → a scripted SEQUENCE, one element
///     consumed per GET to the same URL (for retry scripting: `[429, 429, 200]`),
///     each element itself a bare body or a `{status, body}`; once exhausted the
///     LAST element sticks. WFM bodies are always envelope objects, never bare
///     arrays, so a top-level array is unambiguously a sequence.
pub struct FixtureScrapeHttp {
    pub responses: std::collections::HashMap<String, Value>,
    cursors: std::cell::RefCell<std::collections::HashMap<String, usize>>,
}

impl FixtureScrapeHttp {
    pub fn new(responses: std::collections::HashMap<String, Value>) -> Self {
        FixtureScrapeHttp {
            responses,
            cursors: std::cell::RefCell::new(std::collections::HashMap::new()),
        }
    }
}

/// One scripted response `(status, body)` from a fixture entry: a bare body is
/// 200; a `{status, body}` object is taken verbatim.
fn interpret_one(v: &Value) -> (u16, Value) {
    if let Value::Object(m) = v {
        if let Some(status) = m.get("status").and_then(|s| s.as_u64()) {
            return (status as u16, m.get("body").cloned().unwrap_or(Value::Null));
        }
    }
    (200, v.clone())
}

/// Resolve a fixture entry to the response for call number `i` — indexing into a
/// sequence (sticky-last past its end), or the single response otherwise.
fn response_at(value: &Value, i: usize) -> (u16, Value) {
    if let Value::Array(seq) = value {
        if seq.is_empty() {
            return (200, Value::Null);
        }
        return interpret_one(&seq[i.min(seq.len() - 1)]);
    }
    interpret_one(value)
}

/// Map a scripted HTTP status onto the retry-relevant outcome — the same split
/// `fetch_json` acts on: 2xx is a body, 429 rate-limits, any other is an error.
fn outcome_for(status: u16, body: Value) -> HttpOutcome {
    match status {
        200..=299 => HttpOutcome::Ok(body),
        429 => HttpOutcome::RateLimited,
        other => HttpOutcome::HttpError(other),
    }
}

impl ScrapeHttp for FixtureScrapeHttp {
    fn get(&self, url: &str) -> HttpOutcome {
        let value = match self.responses.get(url) {
            Some(v) => v,
            None => return HttpOutcome::Transport(format!("{url}: not in fixture set")),
        };
        let i = {
            let mut cursors = self.cursors.borrow_mut();
            let n = cursors.entry(url.to_string()).or_insert(0);
            let cur = *n;
            *n += 1;
            cur
        };
        let (status, body) = response_at(value, i);
        outcome_for(status, body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::RefCell;
    use std::collections::{HashMap, VecDeque};

    /// Per-URL scripted outcomes — pop one per call to drive retry sequences.
    struct ScriptedHttp {
        scripts: RefCell<HashMap<String, VecDeque<HttpOutcome>>>,
    }
    impl ScriptedHttp {
        fn new(url: &str, seq: Vec<HttpOutcome>) -> Self {
            let mut m = HashMap::new();
            m.insert(url.to_string(), seq.into_iter().collect());
            ScriptedHttp {
                scripts: RefCell::new(m),
            }
        }
    }
    impl ScrapeHttp for ScriptedHttp {
        fn get(&self, url: &str) -> HttpOutcome {
            self.scripts
                .borrow_mut()
                .get_mut(url)
                .and_then(|q| q.pop_front())
                .unwrap_or_else(|| HttpOutcome::Transport("exhausted".into()))
        }
    }

    const URL: &str = "https://api.warframe.market/v2/items";

    #[test]
    fn ok_first_try_no_sleep_payload_unwrapped() {
        let http = ScriptedHttp::new(URL, vec![HttpOutcome::Ok(json!({"payload": [1, 2]}))]);
        let sl = RecordingSleeper::new();
        assert_eq!(fetch_json(&http, &sl, URL), Some(json!([1, 2])));
        assert!(sl.recorded().is_empty());
    }

    #[test]
    fn data_envelope_unwrapped_when_no_payload() {
        let http = ScriptedHttp::new(URL, vec![HttpOutcome::Ok(json!({"data": [3, 4]}))]);
        let sl = RecordingSleeper::new();
        assert_eq!(fetch_json(&http, &sl, URL), Some(json!([3, 4])));
    }

    #[test]
    fn bare_body_returned_when_no_envelope() {
        let http = ScriptedHttp::new(URL, vec![HttpOutcome::Ok(json!([5, 6]))]);
        let sl = RecordingSleeper::new();
        assert_eq!(fetch_json(&http, &sl, URL), Some(json!([5, 6])));
    }

    #[test]
    fn retries_429_then_succeeds_with_growing_backoff() {
        let http = ScriptedHttp::new(
            URL,
            vec![
                HttpOutcome::RateLimited,
                HttpOutcome::RateLimited,
                HttpOutcome::Ok(json!({"data": 1})),
            ],
        );
        let sl = RecordingSleeper::new();
        assert_eq!(fetch_json(&http, &sl, URL), Some(json!(1)));
        // 2**0, 2**1 before the 3rd (successful) attempt.
        assert_eq!(sl.recorded(), vec![Duration::from_secs(1), Duration::from_secs(2)]);
    }

    #[test]
    fn exhausts_429_and_backs_off_on_the_final_attempt_too() {
        let http = ScriptedHttp::new(
            URL,
            vec![
                HttpOutcome::RateLimited,
                HttpOutcome::RateLimited,
                HttpOutcome::RateLimited,
            ],
        );
        let sl = RecordingSleeper::new();
        assert_eq!(fetch_json(&http, &sl, URL), None);
        // 429 sleeps after every attempt, including the last: 1s, 2s, 4s.
        assert_eq!(
            sl.recorded(),
            vec![Duration::from_secs(1), Duration::from_secs(2), Duration::from_secs(4)]
        );
    }

    #[test]
    fn retries_5xx_then_succeeds() {
        let http = ScriptedHttp::new(
            URL,
            vec![HttpOutcome::HttpError(503), HttpOutcome::Ok(json!({"data": 9}))],
        );
        let sl = RecordingSleeper::new();
        assert_eq!(fetch_json(&http, &sl, URL), Some(json!(9)));
        assert_eq!(sl.recorded(), vec![Duration::from_secs(1)]);
    }

    #[test]
    fn exhausts_5xx_without_sleeping_on_the_last_attempt() {
        let http = ScriptedHttp::new(
            URL,
            vec![
                HttpOutcome::HttpError(500),
                HttpOutcome::HttpError(500),
                HttpOutcome::HttpError(500),
            ],
        );
        let sl = RecordingSleeper::new();
        assert_eq!(fetch_json(&http, &sl, URL), None);
        // Non-429 errors do NOT sleep on the final attempt: 1s, 2s only.
        assert_eq!(sl.recorded(), vec![Duration::from_secs(1), Duration::from_secs(2)]);
    }

    #[test]
    fn transport_error_retries_like_a_request_exception() {
        let http = ScriptedHttp::new(
            URL,
            vec![
                HttpOutcome::Transport("conn reset".into()),
                HttpOutcome::Ok(json!({"data": 7})),
            ],
        );
        let sl = RecordingSleeper::new();
        assert_eq!(fetch_json(&http, &sl, URL), Some(json!(7)));
        assert_eq!(sl.recorded(), vec![Duration::from_secs(1)]);
    }

    #[test]
    fn fixture_http_reports_missing_urls_as_transport() {
        let http = FixtureScrapeHttp::new(HashMap::new());
        assert!(matches!(http.get("https://x"), HttpOutcome::Transport(_)));
    }

    #[test]
    fn fixture_bare_body_is_a_repeating_200() {
        let mut r = HashMap::new();
        r.insert(URL.to_string(), json!({"data": [1]}));
        let http = FixtureScrapeHttp::new(r);
        // A single body serves every call — the always-200 base fixtures rely
        // on this (each item's URL is fetched once, but retries could re-hit it).
        assert!(matches!(http.get(URL), HttpOutcome::Ok(_)));
        assert!(matches!(http.get(URL), HttpOutcome::Ok(_)));
    }

    #[test]
    fn fixture_status_object_maps_to_outcome() {
        let mut r = HashMap::new();
        r.insert(URL.to_string(), json!({"status": 500, "body": {}}));
        let http = FixtureScrapeHttp::new(r);
        assert!(matches!(http.get(URL), HttpOutcome::HttpError(500)));
    }

    #[test]
    fn fixture_sequence_is_consumed_per_call_then_sticks() {
        let mut r = HashMap::new();
        r.insert(
            URL.to_string(),
            json!([{"status": 429, "body": {}}, {"status": 429, "body": {}}, {"data": 7}]),
        );
        let http = FixtureScrapeHttp::new(r);
        assert!(matches!(http.get(URL), HttpOutcome::RateLimited));
        assert!(matches!(http.get(URL), HttpOutcome::RateLimited));
        assert!(matches!(http.get(URL), HttpOutcome::Ok(_)));
        // Sticky last: a 4th call keeps returning the final element.
        assert!(matches!(http.get(URL), HttpOutcome::Ok(_)));
    }

    #[test]
    fn fixture_429_sequence_drives_fetch_json_to_recovery() {
        // The end-to-end shape parity case (i) leans on: 429 twice then 200 must
        // resolve to the body through fetch_json's retry loop.
        let mut r = HashMap::new();
        r.insert(
            URL.to_string(),
            json!([{"status": 429, "body": {}}, {"status": 429, "body": {}}, {"payload": [1, 2]}]),
        );
        let http = FixtureScrapeHttp::new(r);
        let sl = RecordingSleeper::new();
        assert_eq!(fetch_json(&http, &sl, URL), Some(json!([1, 2])));
        assert_eq!(sl.recorded(), vec![Duration::from_secs(1), Duration::from_secs(2)]);
    }
}
