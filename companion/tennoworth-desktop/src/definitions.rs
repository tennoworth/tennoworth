//! Remote scan definitions (Phase C7) — ETag-cached, fail-open.
//!
//! Our scanner pattern-searches for tokens rather than struct offsets, so
//! per-hotfix breakage is unlikely by design. This is the cheap insurance for
//! the day DE rotates a parameter name anyway: without it, a one-line pattern
//! change costs a tagged release, two platform builds, and every user updating
//! before anyone can scan again.
//!
//! Deliberately NOT sharing `market::refresh_with`. The two have different
//! contracts — the market refresh hands the SPA a body and a freshness
//! timestamp to compare against what it already rendered, whereas this one
//! either installs a pattern set or leaves the compiled-in defaults in place
//! and reports nothing to the UI. The genuinely shared pieces (`write_atomic`,
//! `TIMEOUT`) are imported rather than re-implemented.
//!
//! Fail-open at every step, which is the whole safety argument for letting a
//! remote file steer the scanner: offline, 404, garbage JSON, an unparseable
//! pattern, a pattern with the wrong capture arity — every one of them leaves
//! scanning exactly as it was compiled. The worst a bad push can do is nothing.

use std::path::{Path, PathBuf};

use wfm_core::scan::{install_patterns, patterns_from_definitions, ScanDefinitions};

use crate::market::{write_atomic, TIMEOUT};
use crate::overlay::install_reward_markers;

#[derive(serde::Deserialize)]
struct DesktopDefinitions {
    #[serde(flatten)]
    scan: ScanDefinitions,
    #[serde(default)]
    reward_log_markers: Vec<String>,
}

/// Same origin as the market snapshot — no new third-party egress, so
/// SECURITY.md's audited list is unchanged. Overridable for the probe/tests.
const DEFINITIONS_URL: &str = "https://tennoworth.app/definitions.json";
const CACHE_FILE: &str = "definitions.json";
const ETAG_FILE: &str = "definitions.etag";

fn definitions_url() -> String {
    std::env::var("TENNOWORTH_DEFINITIONS_URL").unwrap_or_else(|_| DEFINITIONS_URL.to_string())
}

fn cache_path(dir: &Path) -> PathBuf {
    dir.join(CACHE_FILE)
}

fn etag_path(dir: &Path) -> PathBuf {
    dir.join(ETAG_FILE)
}

/// What a refresh did, for logging and the probe. Not surfaced in the UI: a
/// definitions update is maintenance, not news.
#[derive(Debug, Default, PartialEq)]
pub struct DefinitionsOutcome {
    /// A pattern set was installed (from the network or the cache).
    pub installed: bool,
    /// The fetch delivered a new body (false on 304/offline/error).
    pub fetched: bool,
    /// Patterns the file supplied that were refused, with reasons.
    pub rejected: Vec<String>,
}

/// Install whatever definitions we can find, preferring a fresh fetch and
/// falling back to the last cached copy.
///
/// Called in the background at startup: a scan that happens before this lands
/// simply uses the compiled-in defaults, which is the correct behaviour and not
/// worth blocking app start for.
pub fn refresh_and_install(dir: &Path) -> DefinitionsOutcome {
    refresh_and_install_with(dir, &definitions_url())
}

fn refresh_and_install_with(dir: &Path, url: &str) -> DefinitionsOutcome {
    let body = match fetch(dir, url) {
        Some(fresh) => Some((fresh, true)),
        // 304/offline/error: the cache is the last known-good file and is
        // already validated-on-write, so re-installing it keeps a previously
        // shipped fix working offline.
        None => read_cache(dir).map(|c| (c, false)),
    };
    let Some((raw, fetched)) = body else {
        return DefinitionsOutcome::default();
    };

    let defs: DesktopDefinitions = match serde_json::from_str(&raw) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("tennoworth: definitions parse failed ({e}); keeping built-in patterns");
            return DefinitionsOutcome {
                installed: false,
                fetched,
                rejected: vec![],
            };
        }
    };

    let (patterns, rejections) = patterns_from_definitions(&defs.scan);
    let mut rejected: Vec<String> = rejections
        .iter()
        .map(|r| format!("{}: {}", r.field, r.reason))
        .collect();
    for line in &rejected {
        // A rejection means WE pushed something wrong. Log it loudly enough to
        // find, since the user-visible symptom would otherwise be "scan broke".
        eprintln!("tennoworth: definitions rejected {line}");
    }
    install_patterns(patterns);
    if let Err(reason) = install_reward_markers(&defs.reward_log_markers) {
        eprintln!("tennoworth: definitions rejected reward_log_markers: {reason}");
        rejected.push(format!("reward_log_markers: {reason}"));
    }
    DefinitionsOutcome {
        installed: true,
        fetched,
        rejected,
    }
}

fn read_cache(dir: &Path) -> Option<String> {
    std::fs::read_to_string(cache_path(dir))
        .ok()
        .filter(|s| !s.trim().is_empty())
}

/// Conditional GET. Returns a body only on a validated 200; every other
/// outcome is None and leaves the cache untouched.
fn fetch(dir: &Path, url: &str) -> Option<String> {
    let prior_etag = std::fs::read_to_string(etag_path(dir))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let client = reqwest::blocking::Client::builder()
        .user_agent(wfm_core::user_agent())
        .timeout(TIMEOUT)
        .build()
        .ok()?;

    let mut req = client.get(url);
    if let Some(tag) = &prior_etag {
        req = req.header(reqwest::header::IF_NONE_MATCH, tag);
    }
    let resp = match req.send() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("tennoworth: definitions request failed: {e}");
            return None;
        }
    };

    if resp.status() == reqwest::StatusCode::NOT_MODIFIED || !resp.status().is_success() {
        return None;
    }

    let new_etag = resp
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let body = resp.text().ok()?;

    // Validate before caching, so a truncated 200 cannot poison the cache for
    // every future offline start.
    if serde_json::from_str::<DesktopDefinitions>(&body).is_err() {
        eprintln!(
            "tennoworth: definitions body invalid (len {}); keeping cache",
            body.len()
        );
        return None;
    }

    let _ = write_atomic(&cache_path(dir), body.as_bytes());
    match &new_etag {
        Some(tag) => {
            let _ = write_atomic(&etag_path(dir), tag.as_bytes());
        }
        // No ETag means the next start does a full GET — correct, not an error.
        None => {
            let _ = std::fs::remove_file(etag_path(dir));
        }
    }
    Some(body)
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
            "tennoworth-defs-test-{}-{}",
            std::process::id(),
            nanos
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// Serves one response then exits, like market.rs's mock.
    fn serve_once(
        body: &'static str,
        status_line: &'static str,
        etag: Option<&'static str>,
    ) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/definitions.json", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                let mut buf = [0u8; 2048];
                let _ = sock.read(&mut buf);
                let etag_header = etag.map(|e| format!("ETag: {e}\r\n")).unwrap_or_default();
                let resp = format!(
                    "HTTP/1.1 {status_line}\r\nContent-Length: {}\r\n{etag_header}Connection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes());
            }
        });
        url
    }

    const GOOD: &str = r#"{"version":1,"cred_pattern":"acct=([0-9a-f]{24})&n=([0-9]{6,})"}"#;

    #[test]
    fn a_valid_file_installs_and_caches() {
        let dir = temp_dir();
        let url = serve_once(GOOD, "200 OK", Some("\"d1\""));
        let out = refresh_and_install_with(&dir, &url);
        assert!(out.installed && out.fetched, "{out:?}");
        assert!(out.rejected.is_empty(), "{out:?}");
        assert!(cache_path(&dir).exists(), "body cached for offline starts");
        assert_eq!(
            std::fs::read_to_string(etag_path(&dir)).unwrap(),
            "\"d1\"",
            "ETag persisted for the next conditional GET"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn offline_with_a_cache_still_installs_from_it() {
        // A shipped fix must keep working when the server is unreachable.
        let dir = temp_dir();
        std::fs::write(cache_path(&dir), GOOD).unwrap();
        // Port 1 on localhost: refused immediately, no network wait.
        let out = refresh_and_install_with(&dir, "http://127.0.0.1:1/definitions.json");
        assert!(out.installed, "cache is the fallback");
        assert!(!out.fetched);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn offline_with_no_cache_is_a_silent_noop() {
        let dir = temp_dir();
        let out = refresh_and_install_with(&dir, "http://127.0.0.1:1/definitions.json");
        assert_eq!(out, DefinitionsOutcome::default());
        assert!(!cache_path(&dir).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_garbage_200_does_not_poison_the_cache() {
        let dir = temp_dir();
        std::fs::write(cache_path(&dir), GOOD).unwrap();
        let url = serve_once("<html>502 from a proxy</html>", "200 OK", None);
        let out = refresh_and_install_with(&dir, &url);
        // Fell back to the good cache rather than caching the junk.
        assert!(out.installed && !out.fetched, "{out:?}");
        assert_eq!(std::fs::read_to_string(cache_path(&dir)).unwrap(), GOOD);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_bad_pattern_is_reported_and_scanning_survives() {
        let dir = temp_dir();
        let url = serve_once(r#"{"cred_pattern":"([unclosed"}"#, "200 OK", None);
        let out = refresh_and_install_with(&dir, &url);
        // Installed (with the default cred pattern), and the push is diagnosable.
        assert!(out.installed, "{out:?}");
        assert_eq!(out.rejected.len(), 1, "{out:?}");
        assert!(out.rejected[0].starts_with("cred_pattern:"), "{out:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_404_leaves_everything_alone() {
        let dir = temp_dir();
        let url = serve_once("not found", "404 Not Found", None);
        let out = refresh_and_install_with(&dir, &url);
        assert_eq!(out, DefinitionsOutcome::default());
        assert!(!cache_path(&dir).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
