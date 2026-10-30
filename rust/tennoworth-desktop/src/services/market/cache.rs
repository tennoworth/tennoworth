//! Desktop-only market snapshot cache + ETag-conditional refresh (Phase C4).
//!
//! The bundled `dist-desktop/market.json` is the floor - the asset protocol
//! serves it to the webview at `/market.json` (see the desktop spike, Q1), so it
//! is always available offline with zero work here. This module keeps that data
//! fresh: it conditionally GETs `https://tennoworth.app/market.json` with an
//! `If-None-Match` header and caches the body + ETag next to the SQLite DB in the
//! app-data dir. The webview never makes this third-party request - egress lives
//! in Rust: no third-party fetches from the webview.
//!
//! Freshness precedence the SPA relies on: cached file > bundled file. The cache
//! is written only from a validated 200, so once present it is the last
//! known-good copy from the live server. Every failure mode (offline, timeout,
//! non-200, truncated/garbage body, unwritable cache) is a no-op that keeps the
//! existing copy - a refresh must never leave the user with nothing, and must
//! never block app start, so callers run it in the background and swallow errors.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// The one remote origin the desktop app talks to. Overridable via
/// `TENNOWORTH_MARKET_URL` so the verification probe can point the fetch at a
/// local mock (200/304) or an unreachable host (offline) without a live server.
const MARKET_URL: &str = "https://tennoworth.app/market.json";
const HISTORY_URL: &str = "https://tennoworth.app/history.json";
/// Short - a slow refresh must never make the app feel stuck. The bundled/cached
/// copy is already on screen; this is a background top-up.
const TIMEOUT: Duration = Duration::from_secs(10);

/// One cached live-data artifact: where it comes from, what it's called on
/// disk, and which top-level field proves a 200 body is a whole, valid file.
#[derive(Debug, Clone, Copy)]
pub struct ArtifactSpec {
    pub cache_file: &'static str,
    pub etag_file: &'static str,
    /// Top-level string field a valid body MUST carry (`updated_at` for
    /// market.json, `generated_at` for history.json).
    pub stamp_key: &'static str,
    /// Env var that overrides the URL (probes point it at a local mock).
    pub url_env: &'static str,
    pub default_url: &'static str,
    /// The most body a refresh will read. The body comes from the network and
    /// decides what the app allocates, so a runaway or hostile response is
    /// refused rather than buffered whole. Set well above the published size.
    pub max_bytes: usize,
    pub validate: Option<fn(&str) -> Option<String>>,
}

impl ArtifactSpec {
    fn url(&self) -> String {
        std::env::var(self.url_env).unwrap_or_else(|_| self.default_url.to_string())
    }
}

/// The 2-hourly market snapshot - the app's price source.
pub const MARKET: ArtifactSpec = ArtifactSpec {
    cache_file: "market.json",
    etag_file: "market.etag",
    stamp_key: "updated_at",
    url_env: "TENNOWORTH_MARKET_URL",
    default_url: MARKET_URL,
    // ~3 MB published.
    max_bytes: 32 * 1024 * 1024,
    validate: Some(validated_market_stamp),
};

/// The year-long daily price history (relics.run-derived, built on the box -
/// see wfm-scrape/src/history.rs). ~1 MB gzipped; fetched on demand, not at
/// boot, and there is no bundled floor: absent = the 1y surfaces simply hide.
pub const HISTORY: ArtifactSpec = ArtifactSpec {
    cache_file: "history.json",
    etag_file: "history.etag",
    stamp_key: "generated_at",
    url_env: "TENNOWORTH_HISTORY_URL",
    default_url: HISTORY_URL,
    // ~10 MB published.
    max_bytes: 64 * 1024 * 1024,
    validate: None,
};

pub const ANNOUNCEMENTS: ArtifactSpec = ArtifactSpec {
    cache_file: "announcements.json",
    etag_file: "announcements.etag",
    stamp_key: "updated_at",
    url_env: "TENNOWORTH_ANNOUNCEMENTS_URL",
    default_url: "https://tennoworth.app/announcements.json",
    max_bytes: 65536,
    validate: Some(crate::services::announcements::validated_stamp),
};

/// The result the SPA acts on. `updated` is true only when a validated 200
/// delivered a new snapshot (the SPA parses `body` and swaps it in if its
/// `updated_at` is newer). On 304 / offline / error it is false and `body` is
/// absent - the SPA keeps what it already loaded. `updated_at` always reports the
/// freshest snapshot we now hold (fetched on 200, else the cache) so the SPA can
/// feed the staleness indicator even when nothing changed.
#[derive(serde::Serialize, Default, Debug, PartialEq)]
pub struct RefreshResult {
    pub updated: bool,
    pub updated_at: Option<String>,
    pub etag: Option<String>,
    pub body: Option<String>,
}

/// App-data-dir-backed market cache, held as Tauri managed state. Owns the two
/// files (`market.json`, `market.etag`) and the refresh routine.
pub struct MarketCache {
    dir: PathBuf,
    announcements: std::sync::Mutex<Option<String>>,
}

impl MarketCache {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir, announcements: std::sync::Mutex::new(None) }
    }

    /// The cached snapshot body, or None on a first run / unreadable / empty
    /// cache. No network - a fast local read the SPA does at boot to prefer the
    /// cache over the bundled floor.
    pub fn cached(&self) -> Option<String> {
        read_valid_cache(&self.dir, &MARKET)
    }

    /// The cached history body, if any. No network.
    pub fn cached_history(&self) -> Option<String> {
        read_cache(&self.dir, &HISTORY)
    }

    pub fn cached_announcements(&self) -> Option<String> {
        crate::poison::guard(&self.announcements).clone().or_else(|| read_cache(&self.dir, &ANNOUNCEMENTS))
    }

    pub fn adopt_announcements(&self, body: &str) {
        let Some(next) = crate::services::announcements::validated_stamp(body) else { return; };
        let mut current = crate::poison::guard(&self.announcements);
        let prior = current.as_deref().and_then(crate::services::announcements::validated_stamp);
        if prior.is_none_or(|prior| next >= prior) { *current = Some(body.to_owned()); }
    }

    /// Cloneable owned dir, so the async command can hand it to `spawn_blocking`
    /// without borrowing the (non-'static) Tauri `State` guard across `.await`.
    pub fn dir(&self) -> PathBuf {
        self.dir.clone()
    }
}

fn cache_path(dir: &Path, spec: &ArtifactSpec) -> PathBuf {
    dir.join(spec.cache_file)
}
fn etag_path(dir: &Path, spec: &ArtifactSpec) -> PathBuf {
    dir.join(spec.etag_file)
}

fn read_cache(dir: &Path, spec: &ArtifactSpec) -> Option<String> {
    std::fs::read_to_string(cache_path(dir, spec))
        .ok()
        .filter(|s| !s.is_empty())
}

/// The body's stamp when it passes the artifact's validity gate, else None.
fn valid_stamp(spec: &ArtifactSpec, body: &str) -> Option<String> {
    spec.validate.map_or_else(|| parse_stamp(body, spec.stamp_key), |validate| validate(body))
}

/// The cached body, only if it still passes the artifact's gate. A cache that
/// predates the gate, or that a fault left unusable, reads as absent: the
/// readers fall back to the bundle and the next refresh may replace it.
fn read_valid_cache(dir: &Path, spec: &ArtifactSpec) -> Option<String> {
    read_cache(dir, spec).filter(|body| valid_stamp(spec, body).is_some())
}

/// Pull the artifact's top-level stamp string (`updated_at` / `generated_at`)
/// out of a body. Doubles as the validity gate for a fetched 200: a truncated
/// or non-JSON body (or one missing the stamp) returns None and is refused, so
/// a bad response can never clobber a good cache.
fn parse_stamp(body: &str, key: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    v.get(key)?.as_str().map(str::to_string)
}

/// How far ahead of this clock a held snapshot's stamp may be and still count
/// as evidence for ordering. Enough for ordinary clock skew; a stamp further
/// out would otherwise win every comparison and block every later snapshot.
const MAX_FUTURE_SKEW_SECS: i64 = 60 * 60;

/// The runtime market contract, shared with the webview through
/// `tests/fixtures/market-contract/cases.json`: a JSON object with an RFC 3339
/// `updated_at` and a non-empty `items` object. Everything else stays
/// optional; nullable auxiliary maps are not a broken snapshot. The local clock
/// plays no part, because a slow clock must not reject real snapshots. Release
/// completeness is checked separately, when a snapshot is prepared.
fn validated_market_stamp(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    let root = value.as_object()?;
    let stamp = root.get("updated_at")?.as_str()?;
    stamp_instant(stamp)?;
    if root.get("items")?.as_object()?.is_empty() {
        return None;
    }
    Some(stamp.to_string())
}

/// Whether a snapshot stamped `fetched` may replace one stamped `held`, at
/// `now`. Shared with the webview through
/// `tests/fixtures/market-contract/ordering.json`.
fn stamp_replaces(held: Option<&str>, fetched: &str, now: chrono::DateTime<chrono::Utc>) -> bool {
    let Some(held) = held.and_then(stamp_instant) else {
        return true;
    };
    if held.timestamp().saturating_sub(now.timestamp()) > MAX_FUTURE_SKEW_SECS {
        return true;
    }
    stamp_instant(fetched).is_none_or(|fetched| fetched >= held)
}

/// A stamp as an instant, for ordering two snapshots. A stamp we cannot read is
/// `None`, and every comparison below treats `None` as "no evidence about
/// ordering" rather than as an error.
fn stamp_instant(stamp: &str) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    chrono::DateTime::parse_from_rfc3339(stamp).ok()
}

/// Whether a fetched snapshot may replace the one on disk.
///
/// The webview refuses to swap in a snapshot older than the one it is showing.
/// Native consumers read this cache directly, so without the same rule here the
/// two surfaces disagree after an upstream rollback: the dashboard shows the
/// newer prices it already had while the tray, sellables, protection and
/// reminder freshness work from the older ones - and the next launch downgrades
/// the dashboard too because the cache was overwritten.
///
/// The rules mirror the webview's, including its handling of unusable stamps:
/// nothing cached (or a cached stamp we cannot read) accepts, and so does a
/// fetched stamp we cannot read, because an unreadable stamp must not be able to
/// freeze the cache permanently. A held stamp more than an hour ahead of this
/// clock is no evidence either: a 2099 stamp once blocked every refresh. Only a
/// readable, strictly older snapshot is refused - the same stamp is not a
/// regression.
fn replaces_cache(dir: &Path, spec: &ArtifactSpec, fetched: &str) -> bool {
    let held = read_cache(dir, spec).and_then(|body| valid_stamp(spec, &body));
    stamp_replaces(held.as_deref(), fetched, chrono::Utc::now())
}

/// Atomic write (tmp + rename), matching the repo-wide atomic-write rule - a
/// concurrent reader never sees a half-written cache file.
///
/// `pub(crate)` so the definitions cache reuses this one implementation rather
/// than growing a second tmp+rename.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

/// The one client for TennoWorth's own artifacts (market, history,
/// announcements, definitions). Built once: a blocking client owns a runtime
/// thread, which every refresh used to spawn and tear down again. Redirects are
/// refused because the site serves these files directly, and a followed
/// redirect could leave its origin or drop to plain HTTP.
pub(crate) fn artifact_client() -> Option<&'static reqwest::blocking::Client> {
    static CLIENT: std::sync::OnceLock<Option<reqwest::blocking::Client>> =
        std::sync::OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::blocking::Client::builder()
                .user_agent(wfm_trading::user_agent())
                .timeout(TIMEOUT)
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|e| eprintln!("tennoworth: artifact client build failed: {e}"))
                .ok()
        })
        .as_ref()
}

/// Read at most `max` bytes of UTF-8 body; anything longer is an error, not a
/// truncated body that could still happen to parse.
pub(crate) fn read_bounded(resp: reqwest::blocking::Response, max: usize) -> Result<String, String> {
    use std::io::Read;
    let mut raw = Vec::new();
    resp.take(max as u64 + 1)
        .read_to_end(&mut raw)
        .map_err(|e| e.to_string())?;
    if raw.len() > max {
        return Err(format!("body exceeds {max} bytes"));
    }
    String::from_utf8(raw).map_err(|e| e.to_string())
}

/// "Nothing changed" outcome: report the cache's `updated_at` (parsed lazily) and
/// its ETag, no body. Covers 304, non-200, and every network/IO failure.
fn keep_cache(dir: &Path, spec: &ArtifactSpec, etag: Option<String>) -> RefreshResult {
    let updated_at = read_cache(dir, spec).and_then(|body| valid_stamp(spec, &body));
    RefreshResult {
        updated: false,
        updated_at,
        etag,
        body: None,
    }
}

/// Blocking conditional refresh. Never panics, never returns Err - every failure
/// degrades to `keep_cache`. Runs inside `spawn_blocking` (reqwest::blocking must
/// not run on an async worker thread), the same pattern `scan_inventory` uses.
pub fn refresh(dir: &Path) -> RefreshResult {
    static REFRESH_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _lock = crate::poison::guard(&REFRESH_LOCK);
    refresh_artifact(dir, &MARKET, &MARKET.url())
}

/// Same routine for history.json (see [`HISTORY`]), under its own lock: two
/// overlapping refreshes share `history.tmp`, and one renaming it while the
/// other is still writing would cache a truncated file.
pub fn refresh_history(dir: &Path) -> RefreshResult {
    static REFRESH_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _lock = crate::poison::guard(&REFRESH_LOCK);
    refresh_artifact(dir, &HISTORY, &HISTORY.url())
}

pub fn refresh_announcements(dir: &Path) -> RefreshResult {
    static REFRESH_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _lock = crate::poison::guard(&REFRESH_LOCK);
    refresh_artifact(dir, &ANNOUNCEMENTS, &ANNOUNCEMENTS.url())
}

fn refresh_artifact(dir: &Path, spec: &ArtifactSpec, url: &str) -> RefreshResult {
    let prior_etag = std::fs::read_to_string(etag_path(dir, spec))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let Some(client) = artifact_client() else {
        return keep_cache(dir, spec, prior_etag);
    };

    let mut req = client.get(url);
    if spec.cache_file == ANNOUNCEMENTS.cache_file {
        req = req.header(reqwest::header::CACHE_CONTROL, "no-cache");
    }
    if let Some(tag) = &prior_etag {
        req = req.header(reqwest::header::IF_NONE_MATCH, tag);
    }
    let resp = match req.send() {
        Ok(r) => r,
        Err(e) => {
            // Offline / DNS / timeout / connection refused - the common case, not
            // an error the user should ever see. Log and keep the existing copy.
            eprintln!(
                "tennoworth: {} refresh request failed: {e}",
                spec.cache_file
            );
            return keep_cache(dir, spec, prior_etag);
        }
    };

    let status = resp.status();
    if status == reqwest::StatusCode::NOT_MODIFIED {
        // Cheap path: the server confirms our cache is current. Keep the prior
        // ETag (a 304 need not echo it) and change nothing on disk.
        let etag = resp
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
            .or(prior_etag);
        return keep_cache(dir, spec, etag);
    }
    if !status.is_success() {
        eprintln!("tennoworth: {} refresh got HTTP {status}", spec.cache_file);
        return keep_cache(dir, spec, prior_etag);
    }

    let new_etag = resp
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let body = match read_bounded(resp, spec.max_bytes) {
        Ok(b) => b,
        Err(e) => {
            eprintln!(
                "tennoworth: {} refresh read body failed: {e}",
                spec.cache_file
            );
            return keep_cache(dir, spec, prior_etag);
        }
    };

    // Validate before caching: a truncated or non-JSON 200 must not overwrite a
    // good cache. parse_stamp is the gate AND gives us the timestamp.
    let updated_at = match valid_stamp(spec, &body) {
        Some(u) => u,
        None => {
            eprintln!(
                "tennoworth: {} refresh body invalid (len {}); keeping cache",
                spec.cache_file,
                body.len()
            );
            return keep_cache(dir, spec, prior_etag);
        }
    };

    if !replaces_cache(dir, spec, &updated_at) {
        // A 200 carrying an older snapshot than we hold. The ETag path does not
        // cover this: the conditional request only asks whether the copy we have
        // is still the one the server would send, and after a rollback it is the
        // body that changed, so the answer is a full 200 with the older snapshot
        // in it.
        eprintln!(
            "tennoworth: {} refresh is older than the cache ({}); keeping cache",
            spec.cache_file, updated_at
        );
        return keep_cache(dir, spec, prior_etag);
    }

    if let Err(e) = write_atomic(&cache_path(dir, spec), body.as_bytes()) {
        // Couldn't persist, but the fetch succeeded - hand the SPA the fresh body
        // for THIS session anyway (next launch just re-fetches without the ETag).
        eprintln!("tennoworth: {} cache write failed: {e}", spec.cache_file);
        return RefreshResult {
            updated: true,
            updated_at: Some(updated_at),
            etag: new_etag,
            body: Some(body),
        };
    }
    // Persist the ETag for next launch's conditional request. Best-effort; a
    // missing/removed ETag file just means a full (non-conditional) GET later.
    match &new_etag {
        Some(tag) => {
            let _ = write_atomic(&etag_path(dir, spec), tag.as_bytes());
        }
        None => {
            let _ = std::fs::remove_file(etag_path(dir, spec));
        }
    }

    RefreshResult {
        updated: true,
        updated_at: Some(updated_at),
        etag: new_etag,
        body: Some(body),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn temp_dir() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let p = std::env::temp_dir().join(format!(
            "tennoworth-market-test-{}-{}",
            std::process::id(),
            nanos
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    const ETAG: &str = "\"test-etag-1\"";
    const BODY: &str = r#"{"updated_at":"2026-07-20T10:00:00Z","platform":"pc","items":{"a":{}}}"#;

    #[test]
    fn announcements_refuse_invalid_siblings_and_keep_the_last_valid_cache() {
        let dir = temp_dir();
        let body = include_str!("../../../../../frontend/public/announcements.json");
        let (url, handle) = spawn_mock_serving(1, body);
        assert!(refresh_artifact(&dir, &ANNOUNCEMENTS, &url).updated);
        handle.join().unwrap();
        // The mock reuses one ETag, so force a body response to exercise validation.
        std::fs::remove_file(etag_path(&dir, &ANNOUNCEMENTS)).unwrap();
        let invalid = r#"{"version":1,"updated_at":"2026-10-14T00:00:00Z","messages":[{"id":"broken"}]}"#;
        let (url, handle) = spawn_mock_serving(1, invalid);
        assert!(!refresh_artifact(&dir, &ANNOUNCEMENTS, &url).updated);
        handle.join().unwrap();
        assert!(!refresh_artifact(&dir, &ANNOUNCEMENTS, "http://127.0.0.1:1/feed").updated);
        assert_eq!(read_cache(&dir, &ANNOUNCEMENTS).unwrap(), body);
        let empty = r#"{"version":1,"updated_at":"2026-10-14T00:00:00Z","messages":[]}"#;
        let (url, handle) = spawn_mock_serving(1, empty);
        assert!(refresh_artifact(&dir, &ANNOUNCEMENTS, &url).updated);
        handle.join().unwrap();
        assert!(crate::services::announcements::parse(&read_cache(&dir, &ANNOUNCEMENTS).unwrap()).unwrap().messages.is_empty());
        let limits: serde_json::Value = serde_json::from_str(include_str!("../../../../../tests/fixtures/announcements/limits.json")).unwrap();
        assert_eq!(ANNOUNCEMENTS.max_bytes as u64, limits["max_bytes"].as_u64().unwrap());
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Minimal HTTP/1.1 mock: serves 200 + ETag on a plain request, 304 when the
    /// request carries a matching `If-None-Match`. Records, per request, whether
    /// `If-None-Match` was seen so the caller can assert the conditional path.
    fn spawn_mock(requests: usize) -> (String, std::thread::JoinHandle<Vec<bool>>) {
        spawn_mock_serving(requests, BODY)
    }

    /// The same mock, serving `body` - so a test can drive what a rollback
    /// actually delivers: a well-formed 200 carrying an older snapshot.
    fn spawn_mock_serving(
        requests: usize,
        body: &'static str,
    ) -> (String, std::thread::JoinHandle<Vec<bool>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let url = format!("http://{addr}/market.json");
        let handle = std::thread::spawn(move || {
            let mut seen_inm = Vec::new();
            for _ in 0..requests {
                let (mut stream, _) = match listener.accept() {
                    Ok(s) => s,
                    Err(_) => break,
                };
                let mut buf = Vec::new();
                let mut tmp = [0u8; 1024];
                loop {
                    match stream.read(&mut tmp) {
                        Ok(0) => break,
                        Ok(n) => {
                            buf.extend_from_slice(&tmp[..n]);
                            if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                let req = String::from_utf8_lossy(&buf);
                let inm = req.to_ascii_lowercase().contains("if-none-match:");
                let etag_matches = req.contains(ETAG);
                seen_inm.push(inm);
                let resp = if inm && etag_matches {
                    format!(
                        "HTTP/1.1 304 Not Modified\r\nETag: {ETAG}\r\nConnection: close\r\n\r\n"
                    )
                } else {
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nETag: {ETAG}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    )
                };
                let _ = stream.write_all(resp.as_bytes());
                let _ = stream.flush();
            }
            seen_inm
        });
        (url, handle)
    }

    #[test]
    fn history_spec_validates_on_generated_at_and_caches_beside_market() {
        let dir = temp_dir();
        assert!(read_cache(&dir, &HISTORY).is_none());
        write_atomic(
            &cache_path(&dir, &HISTORY),
            br#"{"generated_at":"2026-08-17T06:00:00Z","items":{"a":{}}}"#,
        )
        .unwrap();
        assert_eq!(
            read_cache(&dir, &MARKET),
            None,
            "history and market caches are separate files"
        );
        let kept = keep_cache(&dir, &HISTORY, None);
        assert_eq!(kept.updated_at.as_deref(), Some("2026-08-17T06:00:00Z"));
        assert_eq!(
            parse_stamp(r#"{"updated_at":"x"}"#, HISTORY.stamp_key),
            None,
            "market's stamp is not history's"
        );
    }

    #[test]
    fn parse_updated_at_extracts_the_field_and_rejects_garbage() {
        assert_eq!(
            parse_stamp(BODY, "updated_at").as_deref(),
            Some("2026-07-20T10:00:00Z")
        );
        assert_eq!(parse_stamp("{}", "updated_at"), None); // valid JSON, no field
        assert_eq!(parse_stamp("not json at all", "updated_at"), None);
        assert_eq!(parse_stamp(r#"{"updated_at":"trunc"#, "updated_at"), None); // truncated
        assert_eq!(parse_stamp(r#"{"updated_at":123}"#, "updated_at"), None); // wrong type
    }

    #[test]
    fn write_atomic_replaces_content_leaving_no_tmp() {
        let dir = temp_dir();
        let p = dir.join("x.json");
        write_atomic(&p, b"first").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "first");
        write_atomic(&p, b"second").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "second");
        assert!(!p.with_extension("tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_body_over_the_ceiling_is_refused_and_the_cache_kept() {
        let dir = temp_dir();
        let cached = r#"{"updated_at":"2026-07-01T00:00:00Z","items":{"a":{}}}"#;
        write_atomic(&cache_path(&dir, &MARKET), cached.as_bytes()).unwrap();
        let (url, handle) = spawn_mock(2);
        let tight = ArtifactSpec { max_bytes: BODY.len() - 1, ..MARKET };

        let refused = refresh_artifact(&dir, &tight, &url);
        assert!(!refused.updated, "an oversized body must not be accepted");
        assert_eq!(read_cache(&dir, &MARKET).as_deref(), Some(cached));

        // Positive control: exactly at the ceiling is still read whole.
        let exact = ArtifactSpec { max_bytes: BODY.len(), ..MARKET };
        assert!(refresh_artifact(&dir, &exact, &url).updated);
        handle.join().unwrap();
    }

    #[test]
    fn first_run_200_caches_body_and_etag_then_second_run_304_keeps_it() {
        let dir = temp_dir();
        let (url, handle) = spawn_mock(2);

        // Launch 1: no cache, no prior ETag → unconditional GET → 200.
        let r1 = refresh_artifact(&dir, &MARKET, &url);
        assert!(r1.updated, "first fetch should report updated");
        assert_eq!(r1.updated_at.as_deref(), Some("2026-07-20T10:00:00Z"));
        assert_eq!(r1.etag.as_deref(), Some(ETAG));
        assert_eq!(r1.body.as_deref(), Some(BODY));
        // Cache + ETag persisted.
        assert_eq!(read_cache(&dir, &MARKET).as_deref(), Some(BODY));
        assert_eq!(
            std::fs::read_to_string(etag_path(&dir, &MARKET))
                .unwrap()
                .trim(),
            ETAG
        );

        // Launch 2: prior ETag on disk → conditional GET → 304 → keep cache.
        let r2 = refresh_artifact(&dir, &MARKET, &url);
        assert!(!r2.updated, "304 must not report updated");
        assert_eq!(r2.body, None, "304 sends no body - SPA keeps what it has");
        assert_eq!(
            r2.updated_at.as_deref(),
            Some("2026-07-20T10:00:00Z"),
            "updated_at still reported from cache on 304"
        );

        let seen = handle.join().unwrap();
        assert_eq!(
            seen,
            vec![false, true],
            "req1 unconditional, req2 If-None-Match"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_redirect_is_not_followed_and_the_cache_is_kept() {
        // The redirect target serves a newer, valid snapshot; following it would
        // replace the cache, so an unchanged cache proves the redirect stopped.
        const NEWER: &str =
            r#"{"updated_at":"2026-07-21T10:00:00Z","platform":"pc","items":{"a":{}}}"#;
        let (target, _target_thread) = spawn_mock_serving(1, NEWER);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/market.json", listener.local_addr().unwrap());
        let location = target.clone();
        let redirector = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut tmp = [0u8; 1024];
            let _ = stream.read(&mut tmp);
            let resp = format!(
                "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            let _ = stream.write_all(resp.as_bytes());
        });
        let dir = temp_dir();
        write_atomic(&cache_path(&dir, &MARKET), BODY.as_bytes()).unwrap();
        let r = refresh_artifact(&dir, &MARKET, &url);
        redirector.join().unwrap();
        assert!(!r.updated);
        assert_eq!(read_cache(&dir, &MARKET).as_deref(), Some(BODY));
        // Release the unused target so its thread can finish.
        let host = reqwest::Url::parse(&target).unwrap();
        let _ = std::net::TcpStream::connect((host.host_str().unwrap(), host.port().unwrap()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn offline_with_a_cache_keeps_the_cache_and_reports_its_timestamp() {
        let dir = temp_dir();
        write_atomic(&cache_path(&dir, &MARKET), BODY.as_bytes()).unwrap();
        write_atomic(&etag_path(&dir, &MARKET), ETAG.as_bytes()).unwrap();
        // Port 1 refuses instantly - a deterministic "offline".
        let r = refresh_artifact(&dir, &MARKET, "http://127.0.0.1:1/market.json");
        assert!(!r.updated);
        assert_eq!(r.body, None);
        assert_eq!(r.updated_at.as_deref(), Some("2026-07-20T10:00:00Z"));
        // Cache untouched.
        assert_eq!(read_cache(&dir, &MARKET).as_deref(), Some(BODY));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn offline_with_no_cache_is_an_empty_noop() {
        let dir = temp_dir();
        let r = refresh_artifact(&dir, &MARKET, "http://127.0.0.1:1/market.json");
        assert_eq!(r, RefreshResult::default());
        assert!(read_cache(&dir, &MARKET).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cached_reads_back_a_written_snapshot_and_none_when_absent_or_empty() {
        let dir = temp_dir();
        let cache = MarketCache::new(dir.clone());
        assert_eq!(cache.cached(), None);
        write_atomic(&cache_path(&dir, &MARKET), b"").unwrap();
        assert_eq!(cache.cached(), None, "empty cache file reads as absent");
        write_atomic(&cache_path(&dir, &MARKET), BODY.as_bytes()).unwrap();
        assert_eq!(cache.cached().as_deref(), Some(BODY));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---- snapshot regression -------------------------------------------------
    //
    // The webview refuses to swap in a snapshot older than the one it already
    // shows. Native consumers read the cache directly, so the same rule has to
    // hold at the cache boundary or the two surfaces disagree about prices.

    #[test]
    fn an_older_snapshot_is_refused_and_leaves_the_cache_alone() {
        let dir = temp_dir();
        let newer = r#"{"updated_at":"2026-07-20T10:00:00Z","platform":"pc","items":{"a":{}}}"#;
        let older = r#"{"updated_at":"2026-07-19T10:00:00Z","platform":"pc","items":{"a":{}}}"#;
        write_atomic(&cache_path(&dir, &MARKET), newer.as_bytes()).unwrap();
        write_atomic(&etag_path(&dir, &MARKET), b"\"stale\"").unwrap();

        let (url, handle) = spawn_mock_serving(1, older);
        let r = refresh_artifact(&dir, &MARKET, &url);

        assert!(!r.updated, "a rollback must not report a new snapshot");
        assert_eq!(r.body, None, "a rollback must not hand the SPA older prices");
        assert_eq!(
            read_cache(&dir, &MARKET).as_deref(),
            Some(newer),
            "the newer cache must survive the rollback"
        );
        assert_eq!(
            r.updated_at.as_deref(),
            Some("2026-07-20T10:00:00Z"),
            "the reported freshness is the snapshot actually held"
        );
        let _ = handle.join();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_newer_snapshot_still_replaces_the_cache() {
        let dir = temp_dir();
        let older = r#"{"updated_at":"2026-07-19T10:00:00Z","platform":"pc","items":{"a":{}}}"#;
        let newer = r#"{"updated_at":"2026-07-20T10:00:00Z","platform":"pc","items":{"a":{}}}"#;
        write_atomic(&cache_path(&dir, &MARKET), older.as_bytes()).unwrap();

        let (url, handle) = spawn_mock_serving(1, newer);
        let r = refresh_artifact(&dir, &MARKET, &url);

        assert!(r.updated);
        assert_eq!(r.updated_at.as_deref(), Some("2026-07-20T10:00:00Z"));
        assert_eq!(read_cache(&dir, &MARKET).as_deref(), Some(newer));
        let _ = handle.join();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An unparseable stored stamp cannot be compared, so it must not be allowed
    /// to veto every future refresh - the app would be stuck on that snapshot
    /// forever. The webview treats an unusable current stamp as "swap".
    #[test]
    fn an_unusable_cached_stamp_does_not_block_a_refresh() {
        let dir = temp_dir();
        write_atomic(
            &cache_path(&dir, &MARKET),
            br#"{"updated_at":"not a date","platform":"pc","items":{"a":{}}}"#,
        )
        .unwrap();

        let (url, handle) = spawn_mock(1);
        let r = refresh_artifact(&dir, &MARKET, &url);

        assert!(r.updated, "a stamp we cannot read must not freeze the cache");
        assert_eq!(read_cache(&dir, &MARKET).as_deref(), Some(BODY));
        let _ = handle.join();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The same stamp is not a rollback. The server re-serving what we already
    /// hold is a no-op only because the ETag path usually catches it, and a
    /// strictly-newer rule must not turn that into a permanent refusal.
    #[test]
    fn an_identical_stamp_is_accepted() {
        let dir = temp_dir();
        write_atomic(&cache_path(&dir, &MARKET), BODY.as_bytes()).unwrap();

        let (url, handle) = spawn_mock_serving(1, BODY);
        let r = refresh_artifact(&dir, &MARKET, &url);

        assert!(r.updated, "the same snapshot is not a regression");
        assert_eq!(read_cache(&dir, &MARKET).as_deref(), Some(BODY));
        let _ = handle.join();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn market_contract_matches_the_shared_cases() {
        let cases: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../../tests/fixtures/market-contract/cases.json"
        ))
        .unwrap();
        for case in cases.as_array().unwrap() {
            let body = serde_json::to_string(&case["body"]).unwrap();
            assert_eq!(
                validated_market_stamp(&body).is_some(),
                case["valid"].as_bool().unwrap(),
                "{}",
                case["name"]
            );
        }
    }

    #[test]
    fn snapshot_ordering_matches_the_shared_cases() {
        let cases: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../../tests/fixtures/market-contract/ordering.json"
        ))
        .unwrap();
        for case in cases.as_array().unwrap() {
            let now = chrono::DateTime::parse_from_rfc3339(case["now"].as_str().unwrap())
                .unwrap()
                .with_timezone(&chrono::Utc);
            assert_eq!(
                stamp_replaces(case["current"].as_str(), case["next"].as_str().unwrap(), now),
                case["replaces"].as_bool().unwrap(),
                "{}",
                case["name"]
            );
        }
    }

    #[test]
    fn the_bundled_snapshot_satisfies_the_runtime_contract() {
        assert!(validated_market_stamp(crate::services::market::sellables::BUNDLED_MARKET).is_some());
    }

    /// A cache stamped in the future used to win every ordering check, so no
    /// later snapshot could replace it and every native reader stayed on it.
    #[test]
    fn a_future_stamped_cache_does_not_block_a_valid_refresh() {
        let dir = temp_dir();
        let cache = MarketCache::new(dir.clone());
        let future = r#"{"updated_at":"2099-01-01T00:00:00Z","platform":"pc","items":{"a":{}}}"#;
        write_atomic(&cache_path(&dir, &MARKET), future.as_bytes()).unwrap();

        let (url, handle) = spawn_mock(1);
        let r = refresh_artifact(&dir, &MARKET, &url);

        assert!(r.updated, "a valid snapshot must replace the future-stamped cache");
        assert_eq!(cache.cached().as_deref(), Some(BODY));
        let _ = handle.join();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_cache_that_breaks_the_contract_reads_as_absent() {
        let dir = temp_dir();
        let cache = MarketCache::new(dir.clone());
        write_atomic(
            &cache_path(&dir, &MARKET),
            br#"{"updated_at":"2026-07-20T10:00:00Z","platform":"pc","items":{}}"#,
        )
        .unwrap();
        assert_eq!(cache.cached(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_parseable_but_unusable_200_keeps_the_cache() {
        let dir = temp_dir();
        write_atomic(&cache_path(&dir, &MARKET), BODY.as_bytes()).unwrap();

        let (url, handle) =
            spawn_mock_serving(1, r#"{"updated_at":"2026-07-21T10:00:00Z","items":{}}"#);
        let r = refresh_artifact(&dir, &MARKET, &url);

        assert!(!r.updated, "a market with no items must not replace prices");
        assert_eq!(r.body, None);
        assert_eq!(read_cache(&dir, &MARKET).as_deref(), Some(BODY));
        let _ = handle.join();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
