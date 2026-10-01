//! Scrape transport: a status-aware GET trait, retry/backoff, and injectable
//! pacing.
//!
//! The converter's [`crate::ingest::Http`] collapses every non-2xx into an
//! error string, which cannot express the 429-vs-5xx-vs-transport distinction
//! the scrape's fetch path acts on. This trait keeps the status so the
//! retry loop can reproduce that behavior exactly, and stays fixture-driven so
//! backoff, pacing, and exhaustion are all testable with zero real sleeps.

use std::sync::Mutex;
use std::time::Duration;

use serde_json::Value;

/// Number of attempts per request - Python's `fetch_json(retries=3)`.
pub const RETRIES: u32 = 3;

/// Longest single cooldown the sweep waits out instead of abandoning the run. A
/// `Retry-After` at or below this is WFM saying when to come back; waiting keeps
/// a two-hour tick from being lost to a momentary throttle.
const MAX_COOLDOWN_WAIT_MS: u64 = 5_000;
/// Total patience per sweep, and how many separate waits it may be spent on. A
/// client that keeps getting throttled wants to send less, not to wait longer.
const MAX_COOLDOWN_WAIT_TOTAL_MS: u64 = 20_000;
const MAX_COOLDOWN_WAITS: u64 = 4;

/// Patience for a request that failed in transit or with a server error after
/// the transport's own quick attempts. Those span about six seconds, and a
/// short WFM blip outlasts them: before this, one such statistics read among
/// ~3,900 aborted the whole two-hour sweep. The budget is per sweep so a real
/// outage still stops the run within a few minutes instead of waiting on every
/// remaining item.
const RECOVERY_BACKOFF_MS: [u64; 2] = [30_000, 90_000];
const MAX_RECOVERY_WAIT_TOTAL_MS: u64 = 300_000;
const MAX_RECOVERY_WAITS: u64 = 6;

/// Outcome of a single GET, preserving enough status to drive Python's retry.
pub enum HttpOutcome {
    /// 2xx with a successfully-parsed JSON body.
    Ok(Value),
    /// HTTP 429/509 - Cloudflare/WFM rate limit, carrying the cooldown deadline
    /// recorded for it (unix ms, 0 when none could be read). [`fetch_json`]
    /// waits out a short one; an unknown one stops the sweep, because retrying a
    /// throttle with no deadline is how a client gets blocked.
    RateLimited { cooldown_until_ms: u64 },
    /// Any other non-2xx (4xx/5xx). Python's `raise_for_status()` path.
    HttpError(u16),
    /// Connection/timeout/read/parse failure. Python's other
    /// `RequestException` path (a `r.json()` decode error lands here too).
    Transport(String),
    Access(String),
}

/// Status-aware GET. Every scrape endpoint goes through this so a fixture can
/// stand in for the network.
///
/// `Send + Sync` because the sweep runs one worker thread per request the
/// governor allows in flight, and they share one transport and one sleeper.
pub trait ScrapeHttp: Send + Sync {
    fn get(&self, url: &str) -> HttpOutcome;
}

/// Injected sleeper so backoff + pacing are deterministic in tests.
pub trait Sleeper: Send + Sync {
    fn sleep(&self, dur: Duration);
}

/// A poisoned lock means a worker panicked; the data behind it is a counter, so
/// reading the last value beats propagating the panic into the abort path.
fn lock<T>(value: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Real time - the production sleeper.
pub struct RealSleeper;
impl Sleeper for RealSleeper {
    fn sleep(&self, dur: Duration) {
        std::thread::sleep(dur);
    }
}

/// Never sleeps - used in fixture mode so the parity subprocess is instant.
pub struct NoopSleeper;
impl Sleeper for NoopSleeper {
    fn sleep(&self, _dur: Duration) {}
}

/// Records requested sleeps instead of sleeping - lets tests assert the exact
/// backoff/pacing schedule.
pub struct RecordingSleeper {
    pub sleeps: std::sync::Mutex<Vec<Duration>>,
}
impl RecordingSleeper {
    pub fn new() -> Self {
        RecordingSleeper {
            sleeps: std::sync::Mutex::new(Vec::new()),
        }
    }
    pub fn recorded(&self) -> Vec<Duration> {
        lock(&self.sleeps).clone()
    }
}
impl Default for RecordingSleeper {
    fn default() -> Self {
        Self::new()
    }
}
impl Sleeper for RecordingSleeper {
    fn sleep(&self, dur: Duration) {
        lock(&self.sleeps).push(dur);
    }
}

/// The wait before retrying a transient failure on `attempt`, or None once the
/// request has had its retries.
fn recovery_backoff_ms(attempt: u32) -> Option<u64> {
    RECOVERY_BACKOFF_MS.get(attempt as usize).copied()
}

/// WFM's envelope, unwrapped Python-order: `payload` first, then `data`, else
/// the bare body. `wfm_client::unwrap_envelope` prefers `data`, so it is
/// intentionally not reused; `fetch_json` in the scraper checks `payload`
/// first.
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

// ---- sweep metrics ---------------------------------------------------------
//
// One process is one sweep, so these counters are process-global rather than
// threaded through every call site. They are the only record of what we asked
// WFM for, and a failed sweep must report them too - it will be repeated.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Instant, SystemTime};

/// Endpoint family, so the footprint reads per class rather than as one number.
#[derive(Clone, Copy)]
pub enum RequestClass {
    Catalog,
    Statistics,
    Orders,
    Riven,
    Other,
}

impl RequestClass {
    const ALL: [RequestClass; 5] = [
        RequestClass::Catalog,
        RequestClass::Statistics,
        RequestClass::Orders,
        RequestClass::Riven,
        RequestClass::Other,
    ];

    fn label(self) -> &'static str {
        match self {
            RequestClass::Catalog => "catalog",
            RequestClass::Statistics => "statistics",
            RequestClass::Orders => "orders",
            RequestClass::Riven => "riven",
            RequestClass::Other => "other",
        }
    }
}

/// Classify a request URL. The scrape's endpoints are fixed, so this is a
/// prefix match rather than a lookup table.
pub fn classify_url(url: &str) -> RequestClass {
    if url.ends_with("/v2/items") {
        RequestClass::Catalog
    } else if url.contains("/statistics") {
        RequestClass::Statistics
    } else if url.contains("/v2/orders/item/") {
        RequestClass::Orders
    } else if url.contains("/v2/riven/") {
        RequestClass::Riven
    } else {
        RequestClass::Other
    }
}

#[derive(Default)]
struct ClassCounters {
    attempts: AtomicU64,
    retries: AtomicU64,
    ok: AtomicU64,
    failed: AtomicU64,
    elapsed_ms: AtomicU64,
    decoded_bytes: AtomicU64,
}

/// One kind of sweep patience: throttle cooldowns or transient-failure
/// recovery. Both figures move together, so they are reserved under one lock:
/// separate counters let two workers each pass the last check and overspend a
/// limit that is documented as a limit.
#[derive(Default)]
struct WaitBudget {
    waits: u64,
    wait_ms: u64,
}
impl WaitBudget {
    fn admit(&mut self, wait_ms: u64, max_waits: u64, max_total_ms: u64) -> bool {
        if self.waits >= max_waits || self.wait_ms.saturating_add(wait_ms) > max_total_ms {
            return false;
        }
        self.waits += 1;
        self.wait_ms += wait_ms;
        true
    }
}

/// What one sweep spent, by class. Named fields rather than an array: the
/// workspace denies indexing, and a class is a fixed set.
#[derive(Default)]
pub struct SweepMetrics {
    catalog: ClassCounters,
    statistics: ClassCounters,
    orders: ClassCounters,
    riven: ClassCounters,
    other: ClassCounters,
    cooldown: Mutex<WaitBudget>,
    recovery: Mutex<WaitBudget>,
}

impl SweepMetrics {
    fn class(&self, class: RequestClass) -> &ClassCounters {
        match class {
            RequestClass::Catalog => &self.catalog,
            RequestClass::Statistics => &self.statistics,
            RequestClass::Orders => &self.orders,
            RequestClass::Riven => &self.riven,
            RequestClass::Other => &self.other,
        }
    }

    pub fn record_attempt(&self, class: RequestClass) {
        self.class(class).attempts.fetch_add(1, Ordering::Relaxed);
    }
    pub fn record_retry(&self, class: RequestClass) {
        self.class(class).retries.fetch_add(1, Ordering::Relaxed);
    }
    pub fn record_ok(&self, class: RequestClass) {
        self.class(class).ok.fetch_add(1, Ordering::Relaxed);
    }
    pub fn record_failed(&self, class: RequestClass) {
        self.class(class).failed.fetch_add(1, Ordering::Relaxed);
    }
    pub fn record_elapsed(&self, class: RequestClass, elapsed: Duration) {
        self.class(class)
            .elapsed_ms
            .fetch_add(elapsed.as_millis() as u64, Ordering::Relaxed);
    }
    /// Payload size as parsed, which is the only size WFM lets us see: it
    /// answers gzip-compressed and sends no Content-Length, so a wire counter
    /// would read zero on every production request.
    pub fn record_bytes(&self, class: RequestClass, decoded: u64) {
        self.class(class)
            .decoded_bytes
            .fetch_add(decoded, Ordering::Relaxed);
    }

    /// Admit one wait of `wait_ms` against the sweep's throttle patience, or
    /// refuse it because the wait alone is too long or the patience is spent.
    /// The reservation is atomic, so workers sharing the sweep cannot both spend
    /// the last of it.
    fn admit_cooldown_wait(&self, wait_ms: u64) -> bool {
        lock(&self.cooldown).admit(wait_ms, MAX_COOLDOWN_WAITS, MAX_COOLDOWN_WAIT_TOTAL_MS)
    }

    /// The same reservation against the sweep's transient-failure patience.
    fn admit_recovery_wait(&self, wait_ms: u64) -> bool {
        lock(&self.recovery).admit(wait_ms, MAX_RECOVERY_WAITS, MAX_RECOVERY_WAIT_TOTAL_MS)
    }

    fn recovery_waits(&self) -> u64 {
        lock(&self.recovery).waits
    }

    fn recovery_wait_ms(&self) -> u64 {
        lock(&self.recovery).wait_ms
    }

    fn cooldown_waits(&self) -> u64 {
        lock(&self.cooldown).waits
    }

    fn cooldown_wait_ms(&self) -> u64 {
        lock(&self.cooldown).wait_ms
    }

    /// One line for the sweep log. `snapshot_age_s` is how old the CSV on disk
    /// is when the sweep ends, or None when there is no snapshot yet.
    pub fn line(&self, snapshot_age_s: Option<u64>) -> String {
        let mut attempts = String::new();
        let mut ok = String::new();
        let mut failed = String::new();
        let (mut retries, mut elapsed, mut decoded) = (0, 0, 0);
        for class in RequestClass::ALL {
            let counters = self.class(class);
            if !attempts.is_empty() {
                attempts.push(' ');
                ok.push(' ');
                failed.push(' ');
            }
            attempts.push_str(&format!(
                "{}={}",
                class.label(),
                counters.attempts.load(Ordering::Relaxed)
            ));
            ok.push_str(&format!(
                "{}={}",
                class.label(),
                counters.ok.load(Ordering::Relaxed)
            ));
            failed.push_str(&format!(
                "{}={}",
                class.label(),
                counters.failed.load(Ordering::Relaxed)
            ));
            retries += counters.retries.load(Ordering::Relaxed);
            elapsed += counters.elapsed_ms.load(Ordering::Relaxed);
            decoded += counters.decoded_bytes.load(Ordering::Relaxed);
        }
        let governor = wfm_client::governor::process().status();
        format!(
            "sweep metrics: attempts[{attempts}] ok[{ok}] failed[{failed}] retries={retries} \
             wire_requests={} throttles={} cooldown_waits={} cooldown_wait_ms={} recovery_waits={} \
             recovery_wait_ms={} decoded_bytes={decoded} elapsed_ms={elapsed} snapshot_age_s={}",
            governor.requests,
            governor.throttles,
            self.cooldown_waits(),
            self.cooldown_wait_ms(),
            self.recovery_waits(),
            self.recovery_wait_ms(),
            snapshot_age_s.map_or_else(|| "none".to_string(), |s| s.to_string())
        )
    }
}

/// Process-wide counters - a running process is a sweep.
pub fn metrics() -> &'static SweepMetrics {
    static METRICS: OnceLock<SweepMetrics> = OnceLock::new();
    METRICS.get_or_init(SweepMetrics::default)
}

/// Age of the snapshot on disk, for the sweep log's freshness term.
pub fn snapshot_age_s(path: &std::path::Path) -> Option<u64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    Some(SystemTime::now().duration_since(modified).ok()?.as_secs())
}

/// The wait to spend on a throttle cooling down until `cooldown_until_ms`, or
/// None when the sweep should stop instead. The deadline comes from the
/// transport, which already recorded this response's `Retry-After` with the
/// governor: calling `throttled()` again here would only double one throttle's
/// backoff, so this reads the stored deadline rather than re-reporting it.
fn admitted_cooldown_wait(sweep: &SweepMetrics, cooldown_until_ms: u64, now_ms: u64) -> Option<u64> {
    let wait = cooldown_until_ms.saturating_sub(now_ms);
    if wait == 0 || wait > MAX_COOLDOWN_WAIT_MS || !sweep.admit_cooldown_wait(wait) {
        return None;
    }
    Some(wait)
}

/// A throttle is not a generic access failure: the governor has stored the
/// deadline the response asked for, and the sweep can sometimes wait it out.
/// A transport failure is the momentary kind [`fetch_json`] retries. Everything
/// else - a pause, a cancellation, a policy refusal - stops the run.
fn transport_error_outcome(error: &wfm_client::governor::AccessError) -> HttpOutcome {
    use wfm_client::governor::AccessError;
    match error {
        AccessError::Cooldown(deadline) => HttpOutcome::RateLimited {
            cooldown_until_ms: *deadline,
        },
        AccessError::Transport => HttpOutcome::Transport(error.to_string()),
        other => HttpOutcome::Access(other.to_string()),
    }
}

/// Retry transient reads; policy blocks and ordinary client errors abort.
pub fn fetch_json(http: &dyn ScrapeHttp, sleeper: &dyn Sleeper, url: &str) -> Option<Value> {
    fetch_json_in(metrics(), http, sleeper, url)
}

/// [`fetch_json`] against an explicit sweep, so a test's patience is its own.
fn fetch_json_in(
    sweep: &SweepMetrics,
    http: &dyn ScrapeHttp,
    sleeper: &dyn Sleeper,
    url: &str,
) -> Option<Value> {
    let class = classify_url(url);
    for attempt in 0..RETRIES {
        if attempt > 0 {
            sweep.record_retry(class);
        }
        sweep.record_attempt(class);
        let started = Instant::now();
        let outcome = http.get(url);
        sweep.record_elapsed(class, started.elapsed());
        match outcome {
            HttpOutcome::Ok(body) => {
                sweep.record_ok(class);
                return Some(unwrap_payload_first(body));
            }
            HttpOutcome::RateLimited { cooldown_until_ms } => {
                sweep.record_failed(class);
                let now = wfm_client::governor::unix_ms();
                match admitted_cooldown_wait(sweep, cooldown_until_ms, now) {
                    Some(wait) => {
                        eprintln!("WFM throttled the request; waiting {wait} ms for the cooldown to end.");
                        sleeper.sleep(Duration::from_millis(wait));
                    }
                    None => {
                        eprintln!("WFM throttled the request; publication is stopped. Retry after market access recovers.");
                        return None;
                    }
                }
            }
            HttpOutcome::Access(reason) => {
                sweep.record_failed(class);
                eprintln!("WFM request stopped: {reason}");
                return None;
            }
            HttpOutcome::HttpError(status) if status < 500 || status == 509 => {
                sweep.record_failed(class);
                eprintln!("WFM request stopped: WFM HTTP {status}");
                return None;
            }
            HttpOutcome::HttpError(status) => {
                if !wait_to_retry(sweep, sleeper, class, attempt, &format!("WFM HTTP {status}")) {
                    return None;
                }
            }
            HttpOutcome::Transport(reason) => {
                if !wait_to_retry(sweep, sleeper, class, attempt, &reason) {
                    return None;
                }
            }
        }
    }
    None
}

/// Wait before retrying a transient failure, or record the request as failed
/// when it has had its retries or the sweep's recovery patience is spent.
fn wait_to_retry(
    sweep: &SweepMetrics,
    sleeper: &dyn Sleeper,
    class: RequestClass,
    attempt: u32,
    reason: &str,
) -> bool {
    let wait = recovery_backoff_ms(attempt)
        .filter(|_| attempt + 1 < RETRIES)
        .filter(|wait| sweep.admit_recovery_wait(*wait));
    let Some(wait) = wait else {
        sweep.record_failed(class);
        eprintln!("WFM request stopped: {reason}; retries exhausted");
        return false;
    };
    eprintln!("WFM request failed: {reason}; retrying in {} s.", wait / 1000);
    sleeper.sleep(Duration::from_millis(wait));
    true
}

/// Live transport through the shared request governor. Sends the EXACT header
/// set the scraper needs - `User-Agent` (via the client), `Platform`,
/// `Language` - and deliberately NOT `Crossplay` (the scraper omits it;
/// `wfm_client::wfm_headers` would add it, so it is not used here).
pub struct LiveScrapeHttp {
    pub client: reqwest::blocking::Client,
    pub platform: String,
}

impl ScrapeHttp for LiveScrapeHttp {
    fn get(&self, url: &str) -> HttpOutcome {
        use wfm_client::transport::GovernedRequest;
        let resp = self
            .client
            .get(url)
            .header("Platform", &self.platform)
            .header("Language", "en")
            .send_governed(wfm_client::governor::Kind::Read);
        match resp {
            Ok(r) => {
                let status = r.status();
                let class = classify_url(url);
                if status.as_u16() == 429 {
                    return HttpOutcome::RateLimited {
                        cooldown_until_ms: wfm_client::governor::process()
                            .status()
                            .cooldown_until_ms,
                    };
                }
                if !status.is_success() {
                    return HttpOutcome::HttpError(status.as_u16());
                }
                match r.text() {
                    Ok(body) => {
                        metrics().record_bytes(class, body.len() as u64);
                        match serde_json::from_str(&body) {
                            Ok(v) => HttpOutcome::Ok(v),
                            Err(e) => HttpOutcome::Access(format!("{url}: JSON parse: {e}")),
                        }
                    }
                    // A body cut off mid-read is a dropped connection, not a
                    // verdict on the request.
                    Err(e) => HttpOutcome::Transport(format!("{url}: read body: {e}")),
                }
            }
            Err(error) => transport_error_outcome(&error),
        }
    }
}

/// Fixture transport for `--fixtures-dir` mode: a URL→response map. A URL absent
/// from the map is a transport error (which, after retries, makes the caller
/// skip that item - never a panic).
///
/// FIXTURE RESPONSE FORMAT:
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
    cursors: std::sync::Mutex<std::collections::HashMap<String, usize>>,
}

impl FixtureScrapeHttp {
    pub fn new(responses: std::collections::HashMap<String, Value>) -> Self {
        FixtureScrapeHttp {
            responses,
            cursors: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// Whether `url` has been requested at least once (fixture-present or not).
    #[cfg(test)]
    pub fn was_fetched(&self, url: &str) -> bool {
        lock(&self.cursors).contains_key(url)
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

/// Resolve a fixture entry to the response for call number `i` - indexing into a
/// sequence (sticky-last past its end), or the single response otherwise.
fn response_at(value: &Value, i: usize) -> (u16, Value) {
    if let Value::Array(seq) = value {
        if seq.is_empty() {
            return (200, Value::Null);
        }
        return interpret_one(
            seq.get(i.min(seq.len().saturating_sub(1)))
                .unwrap_or(&Value::Null),
        );
    }
    interpret_one(value)
}

/// Map a scripted HTTP status onto the retry-relevant outcome - the same split
/// `fetch_json` acts on: 2xx is a body, 429 rate-limits, any other is an error.
fn outcome_for(status: u16, body: Value) -> HttpOutcome {
    match status {
        200..=299 => HttpOutcome::Ok(body),
        // A fixture has no deadline to carry, so a scripted 429 stops the sweep
        // exactly as an unreadable Retry-After would.
        429 | 509 => HttpOutcome::RateLimited {
            cooldown_until_ms: 0,
        },
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
            let mut cursors = lock(&self.cursors);
            let n = cursors.entry(url.to_string()).or_insert(0);
            let cur = *n;
            *n += 1;
            cur
        };
        let (status, body) = response_at(value, i);
        let size = serde_json::to_vec(&body).map(|b| b.len() as u64).unwrap_or(0);
        metrics().record_bytes(classify_url(url), size);
        outcome_for(status, body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::{HashMap, VecDeque};

    /// Per-URL scripted outcomes - pop one per call to drive retry sequences.
    struct ScriptedHttp {
        scripts: std::sync::Mutex<HashMap<String, VecDeque<HttpOutcome>>>,
    }
    impl ScriptedHttp {
        fn new(url: &str, seq: Vec<HttpOutcome>) -> Self {
            let mut m = HashMap::new();
            m.insert(url.to_string(), seq.into_iter().collect());
            ScriptedHttp {
                scripts: std::sync::Mutex::new(m),
            }
        }
    }
    impl ScrapeHttp for ScriptedHttp {
        fn get(&self, url: &str) -> HttpOutcome {
            lock(&self.scripts)
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
    fn throttling_aborts_without_retrying_or_sleeping() {
        let http = ScriptedHttp::new(URL, vec![
                HttpOutcome::RateLimited { cooldown_until_ms: 0 },
                HttpOutcome::Ok(json!({"data": 1})),
            ]);
        let sleeper = RecordingSleeper::new();
        assert_eq!(fetch_json(&http, &sleeper, URL), None);
        assert!(sleeper.recorded().is_empty());
    }

    #[test]
    fn a_short_recorded_cooldown_is_waited_out_and_the_request_retried() {
        let deadline = wfm_client::governor::unix_ms() + 150;
        let http = ScriptedHttp::new(
            URL,
            vec![
                HttpOutcome::RateLimited {
                    cooldown_until_ms: deadline,
                },
                HttpOutcome::Ok(json!({"data": 4})),
            ],
        );
        let sleeper = RecordingSleeper::new();
        assert_eq!(fetch_json(&http, &sleeper, URL), Some(json!(4)));
        let sleeps = sleeper.recorded();
        assert_eq!(sleeps.len(), 1, "the cooldown is waited out once: {sleeps:?}");
        assert!(
            sleeps[0] > Duration::ZERO && sleeps[0] <= Duration::from_millis(150),
            "the wait is the deadline's remainder: {sleeps:?}"
        );
    }

    #[test]
    fn throttle_patience_is_bounded_and_an_unknown_deadline_is_never_guessed() {
        let sweep = SweepMetrics::default();
        assert_eq!(admitted_cooldown_wait(&sweep, 0, 1_000), None);
        assert_eq!(admitted_cooldown_wait(&sweep, 1_000, 1_000), None);
        assert_eq!(
            sweep.cooldown_waits(),
            0,
            "a cooldown with nothing left to wait for is not patience"
        );
        assert_eq!(
            admitted_cooldown_wait(&sweep, 1_000 + MAX_COOLDOWN_WAIT_MS + 1, 1_000),
            None,
            "a long cooldown belongs to the next tick"
        );
        assert_eq!(
            admitted_cooldown_wait(&sweep, 1_000 + MAX_COOLDOWN_WAIT_MS, 1_000),
            Some(MAX_COOLDOWN_WAIT_MS)
        );
        let mut extra = 0;
        while admitted_cooldown_wait(&sweep, 1_000 + MAX_COOLDOWN_WAIT_MS, 1_000).is_some() {
            extra += 1;
        }
        assert!(extra < MAX_COOLDOWN_WAITS, "{extra} extra waits");
        assert!(
            sweep.cooldown_wait_ms() <= MAX_COOLDOWN_WAIT_TOTAL_MS,
            "the total cap binds too"
        );
    }

    #[test]
    fn workers_racing_for_the_last_of_the_patience_only_one_get_it() {
        // Repeated rounds: a check-then-reserve implementation often serialises
        // by luck, and one round would let it pass.
        for round in 0..8 {
            let sweep = SweepMetrics::default();
            for _ in 0..MAX_COOLDOWN_WAITS - 1 {
                assert!(sweep.admit_cooldown_wait(MAX_COOLDOWN_WAIT_MS));
            }
            let start = std::sync::Barrier::new(32);
            let mut admitted = 0;
            std::thread::scope(|scope| {
                let mut tasks = Vec::new();
                for _ in 0..32 {
                    tasks.push(scope.spawn(|| {
                        start.wait();
                        sweep.admit_cooldown_wait(MAX_COOLDOWN_WAIT_MS)
                    }));
                }
                for task in tasks {
                    admitted += usize::from(task.join().unwrap_or(false));
                }
            });
            assert_eq!(admitted, 1, "round {round}: one slot was left, and it is one slot");
            assert_eq!(sweep.cooldown_waits(), MAX_COOLDOWN_WAITS);
            assert!(sweep.cooldown_wait_ms() <= MAX_COOLDOWN_WAIT_TOTAL_MS);
        }
    }

    #[test]
    fn a_throttle_reaches_the_sweep_as_a_throttle_not_a_generic_failure() {
        use wfm_client::governor::AccessError;
        assert!(matches!(
            transport_error_outcome(&AccessError::Cooldown(7)),
            HttpOutcome::RateLimited {
                cooldown_until_ms: 7
            }
        ));
        assert!(matches!(
            transport_error_outcome(&AccessError::Paused("paused".into())),
            HttpOutcome::Access(_)
        ));
        assert!(matches!(
            transport_error_outcome(&AccessError::Transport),
            HttpOutcome::Transport(_)
        ));
    }

    #[test]
    fn retries_5xx_then_succeeds() {
        let http = ScriptedHttp::new(
            URL,
            vec![
                HttpOutcome::HttpError(503),
                HttpOutcome::Ok(json!({"data": 9})),
            ],
        );
        let sl = RecordingSleeper::new();
        assert_eq!(fetch_json_in(&SweepMetrics::default(), &http, &sl, URL), Some(json!(9)));
        assert_eq!(sl.recorded(), vec![Duration::from_secs(30)]);
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
        assert_eq!(fetch_json_in(&SweepMetrics::default(), &http, &sl, URL), None);
        // No sleep after the final attempt: 30 s, 90 s only.
        assert_eq!(
            sl.recorded(),
            vec![Duration::from_secs(30), Duration::from_secs(90)]
        );
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
        assert_eq!(fetch_json_in(&SweepMetrics::default(), &http, &sl, URL), Some(json!(7)));
        assert_eq!(sl.recorded(), vec![Duration::from_secs(30)]);
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
        // A single body serves every call - the always-200 base fixtures rely
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
        assert!(matches!(http.get(URL), HttpOutcome::RateLimited { .. }));
        assert!(matches!(http.get(URL), HttpOutcome::RateLimited { .. }));
        assert!(matches!(http.get(URL), HttpOutcome::Ok(_)));
        // Sticky last: a 4th call keeps returning the final element.
        assert!(matches!(http.get(URL), HttpOutcome::Ok(_)));
    }


    #[test]
    fn a_malformed_body_after_server_retries_does_not_restart_the_retry_budget() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            for status in [503, 503, 200] {
                let (mut socket, _) = listener.accept().unwrap();
                let mut input = [0; 4096];
                let _ = socket.read(&mut input).unwrap();
                write!(socket, "HTTP/1.1 {status} Test\r\nContent-Length: 1\r\nConnection: close\r\n\r\nx").unwrap();
            }
            listener
        });
        let http = LiveScrapeHttp { client: wfm_client::build_client(2).unwrap(), platform: "pc".into() };
        assert!(fetch_json_in(&SweepMetrics::default(), &http, &NoopSleeper, &url).is_none());
        // Counted at the server: the governor's request count is process-wide,
        // and other tests send through it concurrently.
        let listener = server.join().unwrap();
        listener.set_nonblocking(true).unwrap();
        assert!(listener.accept().is_err(), "a fourth request was sent");
    }

    /// Serve `script` in order on a loopback port: `Some(status)` answers with
    /// that status and a JSON body, `None` closes the connection unanswered.
    fn serve(script: Vec<Option<u16>>) -> (String, std::thread::JoinHandle<()>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            for step in script {
                let (mut socket, _) = listener.accept().unwrap();
                let mut input = [0; 4096];
                let _ = socket.read(&mut input).unwrap();
                if let Some(status) = step {
                    let body = r#"{"data":5}"#;
                    write!(socket, "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
                }
            }
        });
        (url, server)
    }

    // The production chain: the transport spends its own quick attempts, and
    // what it gives up on must reach the sweep as retryable rather than as a
    // reason to abort publication. Both failures below used to stop the sweep.
    #[test]
    fn a_server_error_outlasting_the_transport_is_retried_by_the_sweep() {
        let (url, server) = serve(vec![Some(503), Some(503), Some(503), Some(200)]);
        let http = LiveScrapeHttp { client: wfm_client::build_client(2).unwrap(), platform: "pc".into() };
        let sleeper = RecordingSleeper::new();
        let sweep = SweepMetrics::default();
        assert_eq!(fetch_json_in(&sweep, &http, &sleeper, &url), Some(json!(5)));
        server.join().unwrap();
        assert_eq!(sleeper.recorded(), vec![Duration::from_secs(30)]);
        assert_eq!(sweep.recovery_waits(), 1);
    }

    #[test]
    fn a_dropped_connection_outlasting_the_transport_is_retried_by_the_sweep() {
        let (url, server) = serve(vec![None, None, None, Some(200)]);
        let http = LiveScrapeHttp { client: wfm_client::build_client(2).unwrap(), platform: "pc".into() };
        let sleeper = RecordingSleeper::new();
        assert_eq!(fetch_json_in(&SweepMetrics::default(), &http, &sleeper, &url), Some(json!(5)));
        server.join().unwrap();
        assert_eq!(sleeper.recorded(), vec![Duration::from_secs(30)]);
    }

    #[test]
    fn a_client_error_is_not_retried() {
        let http = ScriptedHttp::new(URL, vec![HttpOutcome::HttpError(404), HttpOutcome::Ok(json!({"data": 1}))]);
        let sleeper = RecordingSleeper::new();
        assert_eq!(fetch_json_in(&SweepMetrics::default(), &http, &sleeper, URL), None);
        assert!(sleeper.recorded().is_empty());
    }

    /// An outage must still stop the sweep: once the patience is spent, a
    /// failing request gives up at once instead of waiting again.
    #[test]
    fn recovery_patience_is_per_sweep_and_bounded() {
        let sweep = SweepMetrics::default();
        let sleeper = RecordingSleeper::new();
        for _ in 0..10 {
            let http = ScriptedHttp::new(URL, (0..3).map(|_| HttpOutcome::Transport("down".into())).collect());
            assert_eq!(fetch_json_in(&sweep, &http, &sleeper, URL), None);
        }
        let waited: u64 = sleeper.recorded().iter().map(|d| d.as_millis() as u64).sum();
        assert!(sweep.recovery_waits() <= MAX_RECOVERY_WAITS);
        assert!(waited <= MAX_RECOVERY_WAIT_TOTAL_MS, "{waited} ms");
        assert_eq!(waited, sweep.recovery_wait_ms());
        let http = ScriptedHttp::new(URL, vec![HttpOutcome::Transport("down".into()), HttpOutcome::Ok(json!({"data": 1}))]);
        let before = sleeper.recorded().len();
        assert_eq!(fetch_json_in(&sweep, &http, &sleeper, URL), None);
        assert_eq!(sleeper.recorded().len(), before, "a spent sweep does not wait again");
    }

    #[test]
    fn workers_racing_for_the_last_recovery_wait_only_one_get_it() {
        let sweep = SweepMetrics::default();
        for _ in 0..MAX_RECOVERY_WAITS - 1 {
            assert!(sweep.admit_recovery_wait(1));
        }
        let start = std::sync::Barrier::new(16);
        let admitted: usize = std::thread::scope(|scope| {
            let tasks: Vec<_> = (0..16)
                .map(|_| scope.spawn(|| { start.wait(); sweep.admit_recovery_wait(1) }))
                .collect();
            tasks.into_iter().map(|t| usize::from(t.join().unwrap_or(false))).sum()
        });
        assert_eq!(admitted, 1);
    }
}
