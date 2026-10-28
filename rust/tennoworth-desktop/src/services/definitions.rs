//! Remote scan definitions (Phase C7) - ETag-cached, fail-open.
//!
//! Our scanner pattern-searches for tokens rather than struct offsets, so
//! per-hotfix breakage is unlikely by design. This is the cheap insurance for
//! the day DE rotates a parameter name anyway: without it, a one-line pattern
//! change costs a tagged release, two platform builds, and every user updating
//! before anyone can scan again.
//!
//! Deliberately NOT sharing `market::refresh_with`. The two have different
//! contracts - the market refresh hands the SPA a body and a freshness
//! timestamp to compare against what it already rendered, whereas this one
//! either installs a pattern set or leaves the compiled-in defaults in place
//! and reports nothing to the UI. The genuinely shared pieces (`write_atomic`,
//! client, `read_bounded`) are imported rather than re-implemented.
//!
//! Fail-open at every step, which is the whole safety argument for letting a
//! remote file steer the scanner: offline, 404, garbage JSON, a missing or bad
//! signature and an unparseable pattern each leave scanning exactly as it was
//! compiled.
//!
//! The file is a Minisign envelope signed with the WFM policy key, because the
//! patterns it carries run over the game's memory to find the session
//! credentials: whoever can edit the site's copy should not be able to steer
//! that. Its payload names `purpose: "scan-definitions"`, so a signed policy can
//! never be replayed as definitions. A build without the key fetches nothing.
//!
//! The arity check is the one step that does NOT buy that on its own: it counts
//! capture groups, so a pattern whose required groups can be absent from a match
//! (a top-level alternation or an optional group) passes it. The match loop
//! handles that by reading the required groups as Options, so it can no longer
//! abort the app, which is what indexing an absent group used to do.
//!
//! Cost is a separate matter the arity check does not bound at all. Iterating
//! matches is `O(m * n^2)` in the worst case even without backtracking, and a
//! captured value is arbitrarily long, so a definition can spend unbounded CPU
//! and memory without ever matching a credential. Only a scan-side budget
//! contains that; "matches nothing" is not the worst a bad push can do.

use std::path::{Path, PathBuf};

use wfm_core::acquisition::scan::{install_patterns, patterns_from_definitions, ScanDefinitions};

use crate::services::market::{artifact_client, read_bounded, write_atomic};

const PURPOSE: &str = "scan-definitions";

#[derive(serde::Deserialize)]
struct DesktopDefinitions {
    purpose: String,
    #[serde(flatten)]
    scan: ScanDefinitions,
    #[serde(default)]
    reward_log_markers: Vec<String>,
}

/// Same origin as the market snapshot - no new third-party egress, so
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
    /// The reward-screen log markers the file supplied, not yet validated.
    /// Their owner is the overlay, which installs them and reports a refusal
    /// into `rejected`; definitions only carries them.
    pub reward_log_markers: Vec<String>,
}

/// Install whatever definitions we can find, preferring a fresh fetch and
/// falling back to the last cached copy.
///
/// Called in the background at startup: a scan that happens before this lands
/// simply uses the compiled-in defaults, which is the correct behaviour and not
/// worth blocking app start for.
pub fn refresh_and_install(dir: &Path) -> DefinitionsOutcome {
    let Some(key) = wfm_client::policy::PUBLIC_KEY else {
        return DefinitionsOutcome::default();
    };
    let keys = Keys {
        current: key,
        previous: wfm_client::policy::PREVIOUS_PUBLIC_KEYS,
    };
    refresh_and_install_with(dir, &definitions_url(), &keys)
}

/// The key a fetched file must verify with, and the retired keys a cached one
/// may still verify with - the same rotation rule as the WFM policy.
struct Keys<'a> {
    current: &'a str,
    previous: &'a [&'a str],
}

/// The verified definitions in an envelope, or None. Only a file already on
/// disk may lean on a retired key.
fn verified(raw: &str, keys: &Keys, cached: bool) -> Option<DesktopDefinitions> {
    let max = MAX_DEFINITION_BYTES as u64;
    let verify = |key: &str| wfm_client::policy::verify_envelope(raw.as_bytes(), key, max).ok();
    let (_, payload) = verify(keys.current).or_else(|| {
        cached
            .then(|| keys.previous.iter().find_map(|old| verify(old)))
            .flatten()
    })?;
    let defs: DesktopDefinitions = serde_json::from_slice(&payload).ok()?;
    (defs.purpose == PURPOSE).then_some(defs)
}

fn refresh_and_install_with(dir: &Path, url: &str, keys: &Keys) -> DefinitionsOutcome {
    let fresh = fetch(dir, url, keys);
    let fetched = fresh.is_some();
    // 304/offline/error: the cache is the last known-good file and is
    // already validated-on-write, so re-installing it keeps a previously
    // shipped fix working offline. It is verified again on every read.
    let Some(defs) = fresh.or_else(|| {
        let cached = read_cache(dir)?;
        let defs = verified(&cached, keys, true);
        if defs.is_none() {
            eprintln!("tennoworth: cached definitions are not signed; dropping them");
            reset_cache(dir);
        }
        defs
    }) else {
        return DefinitionsOutcome::default();
    };

    let (patterns, rejections) = patterns_from_definitions(&defs.scan);
    let rejected: Vec<String> = rejections
        .iter()
        .map(|r| format!("{}: {}", r.field, r.reason))
        .collect();
    for line in &rejected {
        // A rejection means WE pushed something wrong. Log it loudly enough to
        // find, since the user-visible symptom would otherwise be "scan broke".
        eprintln!("tennoworth: definitions rejected {line}");
    }
    // A cached copy that no longer compiles cleanly is not a known-good fallback
    // any more: drop it so the next start falls back to the built-in patterns
    // instead of re-installing something unusable on every offline start.
    if !fetched && !rejections.is_empty() {
        eprintln!(
            "tennoworth: dropping cached definitions ({} unusable entry/entries)",
            rejections.len()
        );
        reset_cache(dir);
    }
    install_patterns(patterns);
    DefinitionsOutcome {
        installed: true,
        fetched,
        rejected,
        reward_log_markers: defs.reward_log_markers,
    }
}

fn read_cache(dir: &Path) -> Option<String> {
    std::fs::read_to_string(cache_path(dir))
        .ok()
        .filter(|s| !s.trim().is_empty())
}

/// Definitions are a few hundred bytes of patterns and reward markers. A much
/// larger body is corrupt or hostile, and reading it whole would let the
/// response decide how much memory the app allocates.
const MAX_DEFINITION_BYTES: usize = 64 * 1024;

/// Conditional GET. Returns a body only on a validated 200; every other
/// outcome is None and leaves the cache untouched.
fn fetch(dir: &Path, url: &str, keys: &Keys) -> Option<DesktopDefinitions> {
    let prior_etag = std::fs::read_to_string(etag_path(dir))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let client = artifact_client()?;

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

    // Bounded: this body decides how much memory the app allocates, and it
    // comes from the network.
    let body = match read_bounded(resp, MAX_DEFINITION_BYTES) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("tennoworth: definitions body refused ({e}); keeping cache");
            return None;
        }
    };

    // Compile before promoting: a body that only partly compiles still installs
    // for this run, because the fields that do compile are useful and a bad push
    // must not disable the others. It must not become the cached known-good
    // file, though - the cache is what every future offline start re-installs,
    // so it holds only a definition that compiled without a single rejection.
    let Some(defs) = verified(&body, keys, false) else {
        eprintln!(
            "tennoworth: definitions body is not a signed definitions file (len {}); keeping cache",
            body.len()
        );
        return None;
    };
    let (_, rejections) = patterns_from_definitions(&defs.scan);
    let usable = rejections.is_empty();
    if !usable {
        eprintln!(
            "tennoworth: definitions body has {} unusable entry/entries; \
             using it for this run only",
            rejections.len()
        );
    }

    if usable {
        let _ = write_atomic(&cache_path(dir), body.as_bytes());
        match &new_etag {
            Some(tag) => {
                let _ = write_atomic(&etag_path(dir), tag.as_bytes());
            }
            // No ETag means the next start does a full GET - correct, not an error.
            None => {
                let _ = std::fs::remove_file(etag_path(dir));
            }
        }
    }
    Some(defs)
}

/// Drop the cached definitions and its validator, so the next start falls back
/// to the compiled-in patterns. This is the recovery path for a cache that turns
/// out to be unusable.
fn reset_cache(dir: &Path) {
    let _ = std::fs::remove_file(cache_path(dir));
    let _ = std::fs::remove_file(etag_path(dir));
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

    fn fixture() -> &'static serde_json::Value {
        static FIXTURE: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
        FIXTURE.get_or_init(|| {
            serde_json::from_str(include_str!(
                "../../../../tests/fixtures/scan-definitions/signed.json"
            ))
            .unwrap()
        })
    }

    /// A fixture case serialized as the file the site serves.
    fn body(case: &str) -> &'static str {
        Box::leak(fixture()[case].to_string().into_boxed_str())
    }

    fn keys() -> Keys<'static> {
        static PREVIOUS: std::sync::OnceLock<[&'static str; 1]> = std::sync::OnceLock::new();
        Keys {
            current: fixture()["public_key"].as_str().unwrap(),
            previous: PREVIOUS.get_or_init(|| [fixture()["previous_public_key"].as_str().unwrap()]),
        }
    }

    fn run(dir: &Path, url: &str) -> DefinitionsOutcome {
        refresh_and_install_with(dir, url, &keys())
    }

    #[test]
    fn a_valid_file_installs_and_caches() {
        let dir = temp_dir();
        let url = serve_once(body("good"), "200 OK", Some("\"d1\""));
        let out = run(&dir, &url);
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
        std::fs::write(cache_path(&dir), body("good")).unwrap();
        // Port 1 on localhost: refused immediately, no network wait.
        let out = run(&dir, "http://127.0.0.1:1/definitions.json");
        assert!(out.installed, "cache is the fallback");
        assert!(!out.fetched);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn offline_with_no_cache_is_a_silent_noop() {
        let dir = temp_dir();
        let out = run(&dir, "http://127.0.0.1:1/definitions.json");
        assert_eq!(out, DefinitionsOutcome::default());
        assert!(!cache_path(&dir).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_garbage_200_does_not_poison_the_cache() {
        let dir = temp_dir();
        std::fs::write(cache_path(&dir), body("good")).unwrap();
        let url = serve_once("<html>502 from a proxy</html>", "200 OK", None);
        let out = run(&dir, &url);
        // Fell back to the good cache rather than caching the junk.
        assert!(out.installed && !out.fetched, "{out:?}");
        assert_eq!(std::fs::read_to_string(cache_path(&dir)).unwrap(), body("good"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_bad_pattern_is_reported_and_scanning_survives() {
        let dir = temp_dir();
        let url = serve_once(body("bad_pattern"), "200 OK", None);
        let out = run(&dir, &url);
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
        let out = run(&dir, &url);
        assert_eq!(out, DefinitionsOutcome::default());
        assert!(!cache_path(&dir).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_body_that_only_partly_compiles_is_not_promoted_to_the_cache() {
        let dir = temp_dir();
        // JSON-valid, so the shape check passes - but the cred pattern cannot
        // compile. Installing it for this run is correct (the other fields
        // still work); persisting it is not, because the cache is the
        // known-good fallback that every future offline start re-installs.
        let url = serve_once(body("bad_pattern"), "200 OK", None);
        let out = run(&dir, &url);
        assert!(out.installed, "this run still gets the usable fields: {out:?}");
        assert_eq!(out.rejected.len(), 1, "{out:?}");
        assert!(
            !cache_path(&dir).exists(),
            "a partly-unusable body must not become the cached known-good file"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_oversized_body_is_refused() {
        let dir = temp_dir();
        let huge = format!(
            r#"{{"version":1,"cred_pattern":"{}"}}"#,
            "a".repeat(128 * 1024)
        );
        let url = serve_once(Box::leak(huge.into_boxed_str()), "200 OK", None);
        let out = run(&dir, &url);
        assert!(!out.installed, "an oversized body must be refused: {out:?}");
        assert!(!out.fetched, "{out:?}");
        assert!(!cache_path(&dir).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_cached_definition_that_is_unusable_is_dropped() {
        // The recovery path: a cache that turns out to be unusable must not keep
        // re-installing itself on every offline start.
        let dir = temp_dir();
        std::fs::write(cache_path(&dir), body("bad_pattern")).unwrap();
        let out = run(&dir, "http://127.0.0.1:1/definitions.json");
        assert!(!out.rejected.is_empty(), "{out:?}");
        assert!(
            !cache_path(&dir).exists(),
            "an unusable cached definition must be dropped so the built-ins take over"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_a_signed_definitions_file_installs() {
        for case in ["unsigned", "wrong_purpose", "missing_purpose", "good_by_previous_key"] {
            let dir = temp_dir();
            let url = serve_once(body(case), "200 OK", None);
            let out = run(&dir, &url);
            assert_eq!(out, DefinitionsOutcome::default(), "{case}");
            assert!(!cache_path(&dir).exists(), "{case}");
            let _ = std::fs::remove_dir_all(&dir);
        }
        let mut tampered = fixture()["good"].clone();
        tampered["payload"] = fixture()["bad_pattern"]["payload"].clone();
        let dir = temp_dir();
        let url = serve_once(Box::leak(tampered.to_string().into_boxed_str()), "200 OK", None);
        assert_eq!(run(&dir, &url), DefinitionsOutcome::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_cache_signed_before_a_key_rotation_still_installs_but_an_unsigned_one_is_dropped() {
        let dir = temp_dir();
        std::fs::write(cache_path(&dir), body("good_by_previous_key")).unwrap();
        let out = run(&dir, "http://127.0.0.1:1/definitions.json");
        assert!(out.installed && !out.fetched, "{out:?}");
        std::fs::write(cache_path(&dir), body("unsigned")).unwrap();
        let out = run(&dir, "http://127.0.0.1:1/definitions.json");
        assert_eq!(out, DefinitionsOutcome::default());
        assert!(!cache_path(&dir).exists(), "an unsigned cache is not a fallback");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
