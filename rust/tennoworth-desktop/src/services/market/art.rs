//! Item pictures for the lists, answered through the webview's `art` protocol
//! (`art://localhost/<slug>`; `http://art.localhost/<slug>` on Windows).
//!
//! The host downloads them from the Warframe wiki and re-serves them
//! (`tennoworth-pipeline art`); the app asks only tennoworth.app, never the
//! wiki, and keeps each picture on disk, so a list draws them offline after
//! its first view. The pictures are decorative: no signature, but only
//! `item-art/<name>` paths from the index are fetched and only real image bytes
//! are kept. Any failure is a missing picture, never an error in the list.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const BASE: &str = "https://tennoworth.app";
const INDEX_FILE: &str = "item-art.json";
const MAX_INDEX: usize = 1024 * 1024;
const MAX_IMAGE: usize = 256 * 1024;
/// The host rewrites the index once a day; asking more often finds nothing new.
const INDEX_FRESH: Duration = Duration::from_secs(24 * 3600);
/// After a failed index fetch, wait this long before asking again.
const INDEX_RETRY: Duration = Duration::from_secs(3600);

pub struct ArtStore {
    dir: PathBuf,
    base: String,
    /// slug → `item-art/<name>`, and when to ask the host again.
    index: Mutex<Option<(Instant, HashMap<String, String>)>>,
}

impl ArtStore {
    pub fn new(dir: PathBuf) -> Self {
        // Probes point this at a local mock, as TENNOWORTH_MARKET_URL does for the snapshot.
        let base = std::env::var("TENNOWORTH_ART_BASE").unwrap_or_else(|_| BASE.to_string());
        ArtStore {
            dir,
            base,
            index: Mutex::new(None),
        }
    }

    /// The picture for `slug` and its MIME type, or `None` when there is none.
    pub fn picture(&self, slug: &str) -> Option<(Vec<u8>, &'static str)> {
        if !valid_slug(slug) {
            return None;
        }
        let rel = self.lookup(slug)?;
        let name = rel.strip_prefix("item-art/")?;
        let path = self.dir.join(name);
        if let Ok(bytes) = std::fs::read(&path) {
            if let Some(mime) = image_type(&bytes) {
                return Some((bytes, mime));
            }
        }
        let bytes = self.fetch(&rel, MAX_IMAGE)?;
        let mime = image_type(&bytes)?;
        if std::fs::create_dir_all(&self.dir).is_ok() {
            let _ = super::cache::write_atomic(&path, &bytes);
        }
        Some((bytes, mime))
    }

    fn lookup(&self, slug: &str) -> Option<String> {
        let mut guard = self.index.lock().ok()?;
        let due = guard
            .as_ref()
            .is_none_or(|(next, _)| Instant::now() >= *next);
        if due {
            let fetched = self
                .fetch(INDEX_FILE, MAX_INDEX)
                .and_then(|body| parse_index(&body).map(|map| (body, map)));
            let next = match fetched {
                Some((body, map)) => {
                    if std::fs::create_dir_all(&self.dir).is_ok() {
                        let _ = super::cache::write_atomic(&self.dir.join(INDEX_FILE), &body);
                    }
                    Some((Instant::now() + INDEX_FRESH, map))
                }
                None => {
                    // Keep what we had; offline on first use, read the last copy on disk.
                    let held = guard.take().map(|(_, map)| map).or_else(|| {
                        std::fs::read(self.dir.join(INDEX_FILE))
                            .ok()
                            .and_then(|b| parse_index(&b))
                    });
                    Some((Instant::now() + INDEX_RETRY, held.unwrap_or_default()))
                }
            };
            *guard = next;
        }
        guard.as_ref()?.1.get(slug).cloned()
    }

    fn fetch(&self, rel: &str, max: usize) -> Option<Vec<u8>> {
        use std::io::Read;
        let client = super::cache::artifact_client()?;
        let resp = client.get(format!("{}/{rel}", self.base)).send().ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let mut raw = Vec::new();
        resp.take(max as u64 + 1).read_to_end(&mut raw).ok()?;
        (raw.len() <= max).then_some(raw)
    }

    #[cfg(test)]
    fn with_index(dir: PathBuf, index: HashMap<String, String>) -> Self {
        ArtStore {
            dir,
            base: "http://127.0.0.1:9".into(),
            index: Mutex::new(Some((Instant::now() + INDEX_FRESH, index))),
        }
    }
}

fn valid_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug.len() <= 160
        && slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// Only `item-art/<name>` with a plain file name; anything else is ignored.
fn valid_rel(rel: &str) -> bool {
    rel.strip_prefix("item-art/").is_some_and(|name| {
        !name.is_empty()
            && !name.starts_with('.')
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    })
}

fn parse_index(body: &[u8]) -> Option<HashMap<String, String>> {
    let doc: serde_json::Value = serde_json::from_slice(body).ok()?;
    let items = doc.get("items")?.as_object()?;
    Some(
        items
            .iter()
            .filter_map(|(slug, rel)| {
                let rel = rel.as_str()?;
                (valid_slug(slug) && valid_rel(rel)).then(|| (slug.clone(), rel.to_string()))
            })
            .collect(),
    )
}

fn image_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else if bytes.starts_with(b"GIF8") {
        Some("image/gif")
    } else {
        None
    }
}

/// The slug a protocol request asks for: the path without its leading slash.
pub fn slug_from_path(path: &str) -> &str {
    path.trim_start_matches('/')
}

/// Answer one `art` protocol request.
pub fn respond(store: Option<&ArtStore>, path: &str) -> tauri::http::Response<Vec<u8>> {
    let found = store.and_then(|s| s.picture(slug_from_path(path)));
    let built = match found {
        Some((bytes, mime)) => tauri::http::Response::builder()
            .status(200)
            .header(tauri::http::header::CONTENT_TYPE, mime)
            .header(tauri::http::header::CACHE_CONTROL, "max-age=86400")
            .body(bytes),
        None => tauri::http::Response::builder()
            .status(404)
            .body(Vec::new()),
    };
    built.unwrap_or_else(|_| tauri::http::Response::new(Vec::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nbody";

    fn temp_dir() -> PathBuf {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!("tw-art-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn the_index_keeps_only_plain_slugs_and_local_picture_paths() {
        let body = br#"{"items":{"abating_link":"item-art/0a1b.png","bad slug":"item-art/x.png",
            "escape":"item-art/../market.json","remote":"https://example.com/x.png","hidden":"item-art/.x"}}"#;
        let index = parse_index(body).unwrap();
        assert_eq!(index.len(), 1);
        assert_eq!(index["abating_link"], "item-art/0a1b.png");
        assert!(parse_index(b"[]").is_none());
        assert!(parse_index(b"not json").is_none());
    }

    #[test]
    fn a_cached_picture_is_served_without_the_network() {
        let dir = temp_dir();
        std::fs::write(dir.join("0a1b.png"), PNG).unwrap();
        let store = ArtStore::with_index(
            dir.clone(),
            [("abating_link".to_string(), "item-art/0a1b.png".to_string())].into(),
        );
        assert_eq!(
            store.picture("abating_link"),
            Some((PNG.to_vec(), "image/png"))
        );
        let resp = respond(Some(&store), "/abating_link");
        assert_eq!(resp.status(), 200);
        assert_eq!(resp.headers()["content-type"], "image/png");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unknown_or_malformed_requests_are_a_plain_404() {
        let dir = temp_dir();
        std::fs::write(dir.join("0a1b.png"), b"<html>not a picture").unwrap();
        let store = ArtStore::with_index(
            dir.clone(),
            [("broken".to_string(), "item-art/0a1b.png".to_string())].into(),
        );
        for path in [
            "/no_such_item",
            "/../market.json",
            "/Abating_Link",
            "/",
            "/broken",
        ] {
            assert_eq!(respond(Some(&store), path).status(), 404, "{path}");
        }
        assert_eq!(respond(None, "/abating_link").status(), 404);
        let _ = std::fs::remove_dir_all(dir);
    }
}
