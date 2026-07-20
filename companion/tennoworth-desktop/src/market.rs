//! Desktop-only market snapshot cache + ETag-conditional refresh (Phase C4).
//!
//! The bundled `dist-desktop/market.json` is the floor — the asset protocol
//! serves it to the webview at `/market.json` (see the desktop spike, Q1), so it
//! is always available offline with zero work here. This module keeps that data
//! fresh: it conditionally GETs `https://tennoworth.app/market.json` with an
//! `If-None-Match` header and caches the body + ETag next to the SQLite DB in the
//! app-data dir. The webview never makes this third-party request — egress lives
//! in Rust, exactly the rule the loopback companion follows ("no third-party
//! fetches from the browser", prototype/CLAUDE.md).
//!
//! Freshness precedence the SPA relies on: cached file > bundled file. The cache
//! is written only from a validated 200, so once present it is the last
//! known-good copy from the live server. Every failure mode (offline, timeout,
//! non-200, truncated/garbage body, unwritable cache) is a no-op that keeps the
//! existing copy — a refresh must never leave the user with nothing, and must
//! never block app start, so callers run it in the background and swallow errors.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// The one remote origin the desktop app talks to. Overridable via
/// `TENNOWORTH_MARKET_URL` so the verification probe can point the fetch at a
/// local mock (200/304) or an unreachable host (offline) without a live server.
const MARKET_URL: &str = "https://tennoworth.app/market.json";
const CACHE_FILE: &str = "market.json";
const ETAG_FILE: &str = "market.etag";
/// Short — a slow refresh must never make the app feel stuck. The bundled/cached
/// copy is already on screen; this is a background top-up.
const TIMEOUT: Duration = Duration::from_secs(10);

fn market_url() -> String {
    std::env::var("TENNOWORTH_MARKET_URL").unwrap_or_else(|_| MARKET_URL.to_string())
}

/// The result the SPA acts on. `updated` is true only when a validated 200
/// delivered a new snapshot (the SPA parses `body` and swaps it in if its
/// `updated_at` is newer). On 304 / offline / error it is false and `body` is
/// absent — the SPA keeps what it already loaded. `updated_at` always reports the
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
}

impl MarketCache {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// The cached snapshot body, or None on a first run / unreadable / empty
    /// cache. No network — a fast local read the SPA does at boot to prefer the
    /// cache over the bundled floor.
    pub fn cached(&self) -> Option<String> {
        read_cache(&self.dir)
    }

    /// Cloneable owned dir, so the async command can hand it to `spawn_blocking`
    /// without borrowing the (non-'static) Tauri `State` guard across `.await`.
    pub fn dir(&self) -> PathBuf {
        self.dir.clone()
    }
}

fn cache_path(dir: &Path) -> PathBuf {
    dir.join(CACHE_FILE)
}
fn etag_path(dir: &Path) -> PathBuf {
    dir.join(ETAG_FILE)
}

fn read_cache(dir: &Path) -> Option<String> {
    std::fs::read_to_string(cache_path(dir))
        .ok()
        .filter(|s| !s.is_empty())
}

/// Pull the top-level `updated_at` string out of a snapshot body. Doubles as the
/// validity gate for a fetched 200: a truncated or non-JSON body (or one missing
/// `updated_at`) returns None and is refused, so a bad response can never clobber
/// a good cache.
fn parse_updated_at(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    v.get("updated_at")?.as_str().map(str::to_string)
}

/// Atomic write (tmp + rename), matching the repo-wide atomic-write rule — a
/// concurrent reader never sees a half-written cache file.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

/// "Nothing changed" outcome: report the cache's `updated_at` (parsed lazily) and
/// its ETag, no body. Covers 304, non-200, and every network/IO failure.
fn keep_cache(dir: &Path, etag: Option<String>) -> RefreshResult {
    let updated_at = read_cache(dir).as_deref().and_then(parse_updated_at);
    RefreshResult {
        updated: false,
        updated_at,
        etag,
        body: None,
    }
}

/// Blocking conditional refresh. Never panics, never returns Err — every failure
/// degrades to `keep_cache`. Runs inside `spawn_blocking` (reqwest::blocking must
/// not run on an async worker thread), the same pattern `scan_inventory` uses.
pub fn refresh(dir: &Path) -> RefreshResult {
    refresh_with(dir, &market_url())
}

fn refresh_with(dir: &Path, url: &str) -> RefreshResult {
    let prior_etag = std::fs::read_to_string(etag_path(dir))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let client = match reqwest::blocking::Client::builder()
        .user_agent(concat!("tennoworth-desktop/", env!("CARGO_PKG_VERSION")))
        .timeout(TIMEOUT)
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("tennoworth: market refresh client build failed: {e}");
            return keep_cache(dir, prior_etag);
        }
    };

    let mut req = client.get(url);
    if let Some(tag) = &prior_etag {
        req = req.header(reqwest::header::IF_NONE_MATCH, tag);
    }
    let resp = match req.send() {
        Ok(r) => r,
        Err(e) => {
            // Offline / DNS / timeout / connection refused — the common case, not
            // an error the user should ever see. Log and keep the existing copy.
            eprintln!("tennoworth: market refresh request failed: {e}");
            return keep_cache(dir, prior_etag);
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
        return keep_cache(dir, etag);
    }
    if !status.is_success() {
        eprintln!("tennoworth: market refresh got HTTP {status}");
        return keep_cache(dir, prior_etag);
    }

    let new_etag = resp
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let body = match resp.text() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("tennoworth: market refresh read body failed: {e}");
            return keep_cache(dir, prior_etag);
        }
    };

    // Validate before caching: a truncated or non-JSON 200 must not overwrite a
    // good cache. parse_updated_at is the gate AND gives us the timestamp.
    let updated_at = match parse_updated_at(&body) {
        Some(u) => u,
        None => {
            eprintln!(
                "tennoworth: market refresh body invalid (len {}); keeping cache",
                body.len()
            );
            return keep_cache(dir, prior_etag);
        }
    };

    if let Err(e) = write_atomic(&cache_path(dir), body.as_bytes()) {
        // Couldn't persist, but the fetch succeeded — hand the SPA the fresh body
        // for THIS session anyway (next launch just re-fetches without the ETag).
        eprintln!("tennoworth: market cache write failed: {e}");
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
            let _ = write_atomic(&etag_path(dir), tag.as_bytes());
        }
        None => {
            let _ = std::fs::remove_file(etag_path(dir));
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
    const BODY: &str = r#"{"updated_at":"2026-07-20T10:00:00Z","platform":"pc","items":{}}"#;

    /// Minimal HTTP/1.1 mock: serves 200 + ETag on a plain request, 304 when the
    /// request carries a matching `If-None-Match`. Records, per request, whether
    /// `If-None-Match` was seen so the caller can assert the conditional path.
    fn spawn_mock(requests: usize) -> (String, std::thread::JoinHandle<Vec<bool>>) {
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
                    format!("HTTP/1.1 304 Not Modified\r\nETag: {ETAG}\r\nConnection: close\r\n\r\n")
                } else {
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nETag: {ETAG}\r\nConnection: close\r\n\r\n{}",
                        BODY.len(),
                        BODY
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
    fn parse_updated_at_extracts_the_field_and_rejects_garbage() {
        assert_eq!(
            parse_updated_at(BODY).as_deref(),
            Some("2026-07-20T10:00:00Z")
        );
        assert_eq!(parse_updated_at("{}"), None); // valid JSON, no field
        assert_eq!(parse_updated_at("not json at all"), None);
        assert_eq!(parse_updated_at(r#"{"updated_at":"trunc"#), None); // truncated
        assert_eq!(parse_updated_at(r#"{"updated_at":123}"#), None); // wrong type
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
    fn first_run_200_caches_body_and_etag_then_second_run_304_keeps_it() {
        let dir = temp_dir();
        let (url, handle) = spawn_mock(2);

        // Launch 1: no cache, no prior ETag → unconditional GET → 200.
        let r1 = refresh_with(&dir, &url);
        assert!(r1.updated, "first fetch should report updated");
        assert_eq!(r1.updated_at.as_deref(), Some("2026-07-20T10:00:00Z"));
        assert_eq!(r1.etag.as_deref(), Some(ETAG));
        assert_eq!(r1.body.as_deref(), Some(BODY));
        // Cache + ETag persisted.
        assert_eq!(read_cache(&dir).as_deref(), Some(BODY));
        assert_eq!(
            std::fs::read_to_string(etag_path(&dir)).unwrap().trim(),
            ETAG
        );

        // Launch 2: prior ETag on disk → conditional GET → 304 → keep cache.
        let r2 = refresh_with(&dir, &url);
        assert!(!r2.updated, "304 must not report updated");
        assert_eq!(r2.body, None, "304 sends no body — SPA keeps what it has");
        assert_eq!(
            r2.updated_at.as_deref(),
            Some("2026-07-20T10:00:00Z"),
            "updated_at still reported from cache on 304"
        );

        let seen = handle.join().unwrap();
        assert_eq!(seen, vec![false, true], "req1 unconditional, req2 If-None-Match");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn offline_with_a_cache_keeps_the_cache_and_reports_its_timestamp() {
        let dir = temp_dir();
        write_atomic(&cache_path(&dir), BODY.as_bytes()).unwrap();
        write_atomic(&etag_path(&dir), ETAG.as_bytes()).unwrap();
        // Port 1 refuses instantly — a deterministic "offline".
        let r = refresh_with(&dir, "http://127.0.0.1:1/market.json");
        assert!(!r.updated);
        assert_eq!(r.body, None);
        assert_eq!(r.updated_at.as_deref(), Some("2026-07-20T10:00:00Z"));
        // Cache untouched.
        assert_eq!(read_cache(&dir).as_deref(), Some(BODY));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn offline_with_no_cache_is_an_empty_noop() {
        let dir = temp_dir();
        let r = refresh_with(&dir, "http://127.0.0.1:1/market.json");
        assert_eq!(r, RefreshResult::default());
        assert!(read_cache(&dir).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cached_reads_back_a_written_snapshot_and_none_when_absent_or_empty() {
        let dir = temp_dir();
        let cache = MarketCache::new(dir.clone());
        assert_eq!(cache.cached(), None);
        write_atomic(&cache_path(&dir), b"").unwrap();
        assert_eq!(cache.cached(), None, "empty cache file reads as absent");
        write_atomic(&cache_path(&dir), BODY.as_bytes()).unwrap();
        assert_eq!(cache.cached().as_deref(), Some(BODY));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
