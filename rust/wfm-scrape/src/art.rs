//! Item pictures, downloaded from the Warframe wiki and served from our host.
//!
//! The wiki (wiki.warframe.com, run by Weird Gloop) hosts the art; the art is
//! Digital Extremes'. We fetch politely and re-serve rather than hotlink:
//! - only `/images/thumb/…` paths, which robots.txt allows (never `api.php` or
//!   `Special:` pages), at 64px - twice the 28px the lists draw;
//! - one request at a time, a pause between them, and a per-run cap, so the
//!   first fill spreads over several days;
//! - rechecks are conditional (`If-Modified-Since`; the wiki sends no ETag),
//!   so an unchanged file costs a 304 and no body;
//! - a missing file is remembered and retried after a month, not every run;
//! - a 429 or server error ends the run; the next one resumes.
//!
//! `build` saves the item → wiki-file sources from the warframestat bulk list
//! it already downloads (`item-art-sources.json` beside the CSV), so this step
//! never fetches that 44 MB list a second time.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const WIKI: &str = "https://wiki.warframe.com";
pub const THUMB_PX: u32 = 64;
pub const SOURCES_FILE: &str = "item-art-sources.json";
pub const STATE_FILE: &str = "item-art-state.json";
pub const MANIFEST_FILE: &str = "item-art.json";
pub const DIR: &str = "item-art";
pub const CREDIT: &str =
    "Item images: Warframe Wiki (wiki.warframe.com) / © Digital Extremes. Not affiliated with Digital Extremes.";
/// Largest body accepted as a 64px thumbnail; anything bigger is not one.
const MAX_BYTES: usize = 256 * 1024;
/// How long a replaced picture stays on disk for clients holding the old index.
const KEEP_REPLACED: Duration = Duration::from_secs(7 * 24 * 3600);

/// One warframestat item, reduced to what picks its picture.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Source {
    pub path: String,
    pub name: String,
    #[serde(default)]
    pub category: Option<String>,
    /// The wiki file name, decoded (`AbatingLinkMod.png`), when warframestat
    /// records one.
    #[serde(default)]
    pub wiki: Option<String>,
}

/// Reduce the warframestat bulk list to [`Source`] rows.
pub fn sources_from_wfstat(raw: &serde_json::Value) -> Vec<Source> {
    raw.as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|it| {
                    Some(Source {
                        path: it.get("uniqueName")?.as_str()?.to_string(),
                        name: it.get("name")?.as_str()?.to_string(),
                        category: it
                            .get("category")
                            .and_then(|c| c.as_str())
                            .map(str::to_string),
                        wiki: it
                            .get("wikiaThumbnail")
                            .and_then(|u| u.as_str())
                            .and_then(wiki_file),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The file name in a wiki image URL: `…/images/AbatingLinkMod.png?37034`,
/// `…/images/thumb/X.png/64px-X.png` and the old Fandom form
/// `…/images/a/ab/X.png/revision/latest` all give `X.png`.
pub fn wiki_file(url: &str) -> Option<String> {
    let path = url.split(['?', '#']).next()?;
    let after = path.split("/images/").nth(1)?;
    after
        .split('/')
        .find(|seg| {
            let lower = seg.to_ascii_lowercase();
            [".png", ".jpg", ".jpeg", ".webp", ".gif"]
                .iter()
                .any(|ext| lower.ends_with(ext))
        })
        .map(percent_decode)
        .filter(|f| !f.is_empty())
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        let hex = |j: usize| bytes.get(j).and_then(|c| (*c as char).to_digit(16));
        match (b, hex(i + 1), hex(i + 2)) {
            (b'%', Some(hi), Some(lo)) => {
                out.push((hi * 16 + lo) as u8);
                i += 3;
            }
            _ => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Categories whose wiki picture is the item name with spaces removed
/// (`Ash Prime` → `AshPrime.png`) when warframestat records none. A guess that
/// misses is a 404, remembered like any other missing file.
const NAMED_LIKE_ITEM: &[&str] = &[
    "Warframes",
    "Primary",
    "Secondary",
    "Melee",
    "Arch-Gun",
    "Arch-Melee",
    "Archwing",
    "Sentinels",
];
const RELIC_ERAS: &[&str] = &["lith", "meso", "neo", "axi", "requiem"];

fn named_file(name: &str) -> String {
    let compact: String = name.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    format!("{compact}.png")
}

/// Which wiki file pictures each tradeable slug in the snapshot.
///
/// In order: a file warframestat names for the item's game path, then for its
/// name; a set and its parts share the set's picture (the wiki only has
/// generic part icons); frames and weapons without a recorded file use their
/// name; a relic uses its era's intact icon. Only slugs in `items` appear.
pub fn resolve(market: &serde_json::Value, sources: &[Source]) -> BTreeMap<String, String> {
    let Some(items) = market.get("items").and_then(|i| i.as_object()) else {
        return BTreeMap::new();
    };
    let path_to_slug: HashMap<&str, &str> = market
        .get("path_to_info")
        .and_then(|p| p.as_object())
        .map(|p| {
            p.iter()
                .filter_map(|(path, info)| Some((path.as_str(), info.get("slug")?.as_str()?)))
                .collect()
        })
        .unwrap_or_default();
    let name_to_slug: HashMap<String, &str> = market
        .get("catalog")
        .and_then(|c| c.as_object())
        .map(|c| {
            c.iter()
                .filter_map(|(n, s)| Some((n.to_lowercase(), s.as_str()?)))
                .collect()
        })
        .unwrap_or_default();

    let file_of = |s: &Source| -> Option<String> {
        s.wiki.clone().or_else(|| {
            s.category
                .as_deref()
                .filter(|c| NAMED_LIKE_ITEM.contains(c))
                .map(|_| named_file(&s.name))
        })
    };
    let mut by_name: HashMap<String, String> = HashMap::new();
    for s in sources {
        if let Some(file) = file_of(s) {
            by_name.entry(s.name.to_lowercase()).or_insert(file);
        }
    }

    let mut out: BTreeMap<String, String> = BTreeMap::new();
    let mut put = |slug: &str, file: &str| {
        if items.contains_key(slug) {
            out.entry(slug.to_string())
                .or_insert_with(|| file.to_string());
        }
    };
    for s in sources {
        if let (Some(file), Some(slug)) = (&s.wiki, path_to_slug.get(s.path.as_str())) {
            put(slug, file);
        }
    }
    for s in sources {
        if let (Some(file), Some(slug)) = (&s.wiki, name_to_slug.get(&s.name.to_lowercase())) {
            put(slug, file);
        }
    }
    if let Some(sets) = market.get("set_to_parts").and_then(|s| s.as_object()) {
        for (set_slug, info) in sets {
            let Some(file) = info
                .get("name")
                .and_then(|n| n.as_str())
                .and_then(|n| by_name.get(&n.to_lowercase()))
            else {
                continue;
            };
            put(set_slug, file);
            for part in info
                .get("parts")
                .and_then(|p| p.as_array())
                .into_iter()
                .flatten()
            {
                if let Some(slug) = part.get("slug").and_then(|s| s.as_str()) {
                    put(slug, file);
                }
            }
        }
    }
    for s in sources {
        if let (Some(file), Some(slug)) = (file_of(s), name_to_slug.get(&s.name.to_lowercase())) {
            put(slug, &file);
        }
    }
    for slug in items.keys() {
        let Some(era) = slug
            .strip_suffix("_relic")
            .and_then(|r| r.split('_').next())
        else {
            continue;
        };
        if RELIC_ERAS.contains(&era) {
            let mut chars = era.chars();
            let cap: String = chars
                .next()
                .map(|c| c.to_ascii_uppercase())
                .into_iter()
                .chain(chars)
                .collect();
            put(slug, &format!("{cap}RelicIntact.png"));
        }
    }
    out
}

/// What we know about one wiki file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// Published path (`item-art/<hash>.png`); `None` while the wiki has no file.
    #[serde(default)]
    pub stored: Option<String>,
    #[serde(default)]
    pub last_modified: Option<String>,
    /// RFC 3339 time of the last request for this file.
    pub checked_at: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    pub last_run: Option<String>,
    #[serde(default)]
    pub files: BTreeMap<String, Entry>,
}

pub struct Options {
    /// Requests per run, at most.
    pub budget: usize,
    /// Pause between requests.
    pub pace: Duration,
    /// A stored file is rechecked once this old.
    pub recheck: chrono::Duration,
    /// A missing file is asked for again once this old.
    pub retry_missing: chrono::Duration,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            budget: 600,
            pace: Duration::from_secs(1),
            // Wiki pictures rarely change; a monthly look keeps steady load near
            // 60 conditional requests a day across ~1,700 files.
            recheck: chrono::Duration::days(30),
            retry_missing: chrono::Duration::days(30),
        }
    }
}

/// Which files to request this run: never-seen files first, then due
/// rechecks, then due retries of missing files, oldest first, up to `budget`.
pub fn plan(
    wanted: &BTreeMap<String, String>,
    state: &State,
    now: chrono::DateTime<chrono::Utc>,
    opts: &Options,
) -> Vec<String> {
    let mut files: Vec<&String> = wanted.values().collect();
    files.sort();
    files.dedup();
    let age = |e: &Entry| crate::clock::parse_stamp(&e.checked_at).map(|t| now - t);
    let mut fresh = Vec::new();
    let mut due: Vec<(chrono::Duration, u8, &String)> = Vec::new();
    for f in files {
        match state.files.get(f) {
            None => fresh.push(f.clone()),
            Some(e) => {
                let (limit, rank) = if e.stored.is_some() {
                    (opts.recheck, 0)
                } else {
                    (opts.retry_missing, 1)
                };
                match age(e) {
                    Some(a) if a < limit => {}
                    a => due.push((a.unwrap_or(chrono::Duration::MAX), rank, f)),
                }
            }
        }
    }
    due.sort_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)).then(a.2.cmp(b.2)));
    fresh
        .into_iter()
        .chain(due.into_iter().map(|(_, _, f)| f.clone()))
        .take(opts.budget)
        .collect()
}

pub enum Fetched {
    Image {
        bytes: Vec<u8>,
        last_modified: Option<String>,
    },
    NotModified,
    Missing,
}

/// One wiki thumbnail request; `Err` means stop the run (rate limit, outage).
pub trait Fetch {
    fn thumb(&self, file: &str, if_modified_since: Option<&str>) -> Result<Fetched, String>;
}

pub fn thumb_url(file: &str) -> Result<String, String> {
    let mut url = reqwest::Url::parse(WIKI).map_err(|e| e.to_string())?;
    let px = format!("{THUMB_PX}px-{file}");
    url.path_segments_mut()
        .map_err(|()| "wiki URL cannot take a path".to_string())?
        .extend(["images", "thumb", file, px.as_str()]);
    Ok(url.into())
}

pub struct LiveFetch {
    client: reqwest::blocking::Client,
}

impl LiveFetch {
    pub fn new() -> Result<Self, String> {
        let client = reqwest::blocking::Client::builder()
            .user_agent(wfm_client::user_agent(
                "wfm-scrape",
                env!("CARGO_PKG_VERSION"),
            ))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| format!("build client: {e}"))?;
        Ok(LiveFetch { client })
    }
}

impl Fetch for LiveFetch {
    fn thumb(&self, file: &str, if_modified_since: Option<&str>) -> Result<Fetched, String> {
        let url = thumb_url(file)?;
        let mut req = self.client.get(&url);
        if let Some(lm) = if_modified_since {
            req = req.header(reqwest::header::IF_MODIFIED_SINCE, lm);
        }
        let resp = req.send().map_err(|e| format!("{url}: {e}"))?;
        let status = resp.status();
        if status == reqwest::StatusCode::NOT_MODIFIED {
            return Ok(Fetched::NotModified);
        }
        // 404 is a missing file; MediaWiki answers 400/500 when a source image
        // is smaller than the thumbnail asked for - also nothing to store.
        if matches!(status.as_u16(), 400 | 404 | 410) {
            return Ok(Fetched::Missing);
        }
        if !status.is_success() {
            return Err(format!("{url}: HTTP {status}"));
        }
        let last_modified = resp
            .headers()
            .get(reqwest::header::LAST_MODIFIED)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let bytes = resp
            .bytes()
            .map_err(|e| format!("{url}: read: {e}"))?
            .to_vec();
        Ok(Fetched::Image {
            bytes,
            last_modified,
        })
    }
}

/// The picture's format from its first bytes; anything else is refused.
fn image_ext(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some("jpg")
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some("webp")
    } else if bytes.starts_with(b"GIF8") {
        Some("gif")
    } else {
        None
    }
}

/// FNV-1a: a stable content name, so a changed picture gets a new URL and the
/// old one can be cached forever. Not a security hash; nothing trusts it.
fn content_name(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("{}: no file name", path.display()))?;
    let tmp = path.with_file_name(format!(".{name}.tmp"));
    std::fs::write(&tmp, bytes).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("rename {}: {e}", path.display()))
}

#[derive(Debug, Default, PartialEq)]
pub struct Summary {
    pub requested: usize,
    pub stored: usize,
    pub unchanged: usize,
    pub missing: usize,
    pub stopped: Option<String>,
    pub published: usize,
}

/// One pass: request what [`plan`] picks, store new pictures under
/// `publish/item-art/`, update `state`, and write `publish/item-art.json`.
pub fn run(
    wanted: &BTreeMap<String, String>,
    state: &mut State,
    fetch: &dyn Fetch,
    publish: &Path,
    now: chrono::DateTime<chrono::Utc>,
    opts: &Options,
    sleep: &dyn Fn(Duration),
) -> Result<Summary, String> {
    let dir = publish.join(DIR);
    std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let stamp = crate::clock::iso_z(now);
    let mut summary = Summary::default();
    for (i, file) in plan(wanted, state, now, opts).into_iter().enumerate() {
        if i > 0 {
            sleep(opts.pace);
        }
        let prior = state.files.get(&file).cloned().unwrap_or_default();
        let conditional = prior.stored.as_ref().and(prior.last_modified.as_deref());
        summary.requested += 1;
        let outcome = match fetch.thumb(&file, conditional) {
            Ok(o) => o,
            Err(e) => {
                summary.stopped = Some(e);
                break;
            }
        };
        let entry = match outcome {
            Fetched::NotModified => {
                summary.unchanged += 1;
                Entry {
                    checked_at: stamp.clone(),
                    ..prior
                }
            }
            Fetched::Missing => {
                summary.missing += 1;
                Entry {
                    stored: None,
                    last_modified: None,
                    checked_at: stamp.clone(),
                }
            }
            Fetched::Image {
                bytes,
                last_modified,
            } => {
                let Some(ext) = image_ext(&bytes).filter(|_| bytes.len() <= MAX_BYTES) else {
                    eprintln!("  item-art: {file}: not a thumbnail-sized image; skipped");
                    summary.missing += 1;
                    state.files.insert(
                        file,
                        Entry {
                            stored: None,
                            last_modified: None,
                            checked_at: stamp.clone(),
                        },
                    );
                    continue;
                };
                let rel = format!("{DIR}/{}.{ext}", content_name(&bytes));
                let path = publish.join(&rel);
                if !path.exists() {
                    write_atomic(&path, &bytes)?;
                }
                summary.stored += 1;
                Entry {
                    stored: Some(rel),
                    last_modified,
                    checked_at: stamp.clone(),
                }
            }
        };
        state.files.insert(file, entry);
    }
    state.last_run = Some(stamp.clone());

    let items: BTreeMap<&String, &String> = wanted
        .iter()
        .filter_map(|(slug, file)| Some((slug, state.files.get(file)?.stored.as_ref()?)))
        .collect();
    summary.published = items.len();
    let manifest = serde_json::json!({
        "updated_at": stamp,
        "credit": CREDIT,
        "items": items,
    });
    let body = serde_json::to_vec(&manifest).map_err(|e| e.to_string())?;
    write_atomic(&publish.join(MANIFEST_FILE), &body)?;
    remove_replaced(&dir, state);
    Ok(summary)
}

/// Delete pictures no file entry points at any more, once they are a week old.
fn remove_replaced(dir: &Path, state: &State) {
    let live: std::collections::HashSet<PathBuf> = state
        .files
        .values()
        .filter_map(|e| e.stored.as_ref())
        .filter_map(|rel| Path::new(rel).file_name().map(|n| dir.join(n)))
        .collect();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > KEEP_REPLACED);
        if old && !live.contains(&path) {
            let _ = std::fs::remove_file(&path);
        }
    }
}

pub fn write_sources(path: &Path, raw: &serde_json::Value) -> Result<usize, String> {
    let sources = sources_from_wfstat(raw);
    if sources.is_empty() {
        return Err("warframestat list had no items; keeping the previous sources".into());
    }
    let body = serde_json::to_vec(&sources).map_err(|e| e.to_string())?;
    write_atomic(path, &body)?;
    Ok(sources.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::RefCell;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nrest-of-a-picture";

    /// A fresh folder under the system temp dir, removed on drop.
    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let p = std::env::temp_dir().join(format!("item-art-test-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            TempDir(p)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn src(path: &str, name: &str, category: &str, wiki: Option<&str>) -> Source {
        Source {
            path: path.into(),
            name: name.into(),
            category: Some(category.into()),
            wiki: wiki.map(Into::into),
        }
    }

    fn market() -> serde_json::Value {
        json!({
            "items": {
                "abating_link": {}, "arcane_energize": {}, "ash_prime_set": {}, "ash_prime_chassis": {},
                "soma_prime": {}, "axi_a1_relic": {}, "requiem_i_relic": {}, "unpictured": {}
            },
            "catalog": { "arcane energize": "arcane_energize", "soma prime": "soma_prime", "ash prime chassis": "ash_prime_chassis" },
            "path_to_info": { "/Lotus/Mods/AbatingLink": { "slug": "abating_link" } },
            "set_to_parts": { "ash_prime_set": { "name": "Ash Prime", "parts": [{ "slug": "ash_prime_chassis" }] } }
        })
    }

    fn sources() -> Vec<Source> {
        vec![
            src(
                "/Lotus/Mods/AbatingLink",
                "Abating Link",
                "Mods",
                Some("AbatingLinkMod.png"),
            ),
            src(
                "/Lotus/Arcane/Energize",
                "Arcane Energize",
                "Arcanes",
                Some("ArcaneEnergize.png"),
            ),
            src("/Lotus/Frames/AshPrime", "Ash Prime", "Warframes", None),
            src("/Lotus/Weapons/SomaPrime", "Soma Prime", "Primary", None),
            src(
                "/Lotus/Parts/AshPrimeChassis",
                "Ash Prime Chassis",
                "Misc",
                None,
            ),
            src("/Lotus/Untraded", "Not On Market", "Mods", Some("Nope.png")),
        ]
    }

    #[test]
    fn wiki_file_reads_every_url_form() {
        assert_eq!(
            wiki_file("https://wiki.warframe.com/images/AbatingLinkMod.png?37034").as_deref(),
            Some("AbatingLinkMod.png")
        );
        assert_eq!(
            wiki_file("https://wiki.warframe.com/images/thumb/X.png/64px-X.png").as_deref(),
            Some("X.png")
        );
        assert_eq!(wiki_file("https://static.wikia.nocookie.net/warframe/images/a/ab/Rhino%27s_Roar.png/revision/latest").as_deref(), Some("Rhino's_Roar.png"));
        assert_eq!(wiki_file("https://wiki.warframe.com/w/Abating_Link"), None);
        assert_eq!(wiki_file(""), None);
    }

    #[test]
    fn resolve_pictures_items_from_path_name_set_and_era() {
        let got = resolve(&market(), &sources());
        let want: BTreeMap<String, String> = [
            ("abating_link", "AbatingLinkMod.png"),
            ("arcane_energize", "ArcaneEnergize.png"),
            ("ash_prime_set", "AshPrime.png"),
            ("ash_prime_chassis", "AshPrime.png"),
            ("soma_prime", "SomaPrime.png"),
            ("axi_a1_relic", "AxiRelicIntact.png"),
            ("requiem_i_relic", "RequiemRelicIntact.png"),
        ]
        .into_iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
        assert_eq!(got, want);
    }

    #[test]
    fn resolve_without_items_is_empty() {
        assert!(resolve(&json!({}), &sources()).is_empty());
    }

    fn at(s: &str) -> chrono::DateTime<chrono::Utc> {
        crate::clock::parse_stamp(s).unwrap()
    }

    fn wanted(files: &[&str]) -> BTreeMap<String, String> {
        files
            .iter()
            .map(|f| (f.to_lowercase(), f.to_string()))
            .collect()
    }

    #[test]
    fn a_stored_picture_is_rechecked_monthly_not_sooner() {
        let now = at("2026-11-10T00:00:00Z");
        let stored = |when: &str| Entry {
            stored: Some("item-art/x.png".into()),
            last_modified: None,
            checked_at: when.into(),
        };
        let mut state = State::default();
        state
            .files
            .insert("TenDays.png".into(), stored("2026-10-31T00:00:00Z"));
        state
            .files
            .insert("ThreeWeeks.png".into(), stored("2026-10-20T00:00:00Z"));
        state
            .files
            .insert("FiveWeeks.png".into(), stored("2026-10-06T00:00:00Z"));
        let w = wanted(&["TenDays.png", "ThreeWeeks.png", "FiveWeeks.png"]);
        assert_eq!(
            plan(&w, &state, now, &Options::default()),
            ["FiveWeeks.png"]
        );
    }

    #[test]
    fn plan_takes_new_files_first_then_due_rechecks_then_due_misses_within_budget() {
        let now = at("2026-11-10T00:00:00Z");
        let mut state = State::default();
        let e = |stored: bool, when: &str| Entry {
            stored: stored.then(|| "item-art/x.png".to_string()),
            last_modified: None,
            checked_at: when.into(),
        };
        state
            .files
            .insert("Fresh.png".into(), e(true, "2026-11-08T00:00:00Z"));
        state
            .files
            .insert("Old.png".into(), e(true, "2026-10-01T00:00:00Z"));
        state
            .files
            .insert("Older.png".into(), e(true, "2026-09-01T00:00:00Z"));
        state
            .files
            .insert("GoneRecent.png".into(), e(false, "2026-11-01T00:00:00Z"));
        state
            .files
            .insert("GoneLong.png".into(), e(false, "2026-09-01T00:00:00Z"));
        let w = wanted(&[
            "New.png",
            "Fresh.png",
            "Old.png",
            "Older.png",
            "GoneRecent.png",
            "GoneLong.png",
        ]);
        let opts = Options {
            budget: 10,
            ..Options::default()
        };
        assert_eq!(
            plan(&w, &state, now, &opts),
            ["New.png", "Older.png", "Old.png", "GoneLong.png"]
        );
        let opts = Options {
            budget: 2,
            ..Options::default()
        };
        assert_eq!(plan(&w, &state, now, &opts), ["New.png", "Older.png"]);
    }

    struct Fake {
        answers: HashMap<String, Result<Fetched, String>>,
        asked: RefCell<Vec<(String, Option<String>)>>,
    }

    impl Fetch for Fake {
        fn thumb(&self, file: &str, ims: Option<&str>) -> Result<Fetched, String> {
            self.asked
                .borrow_mut()
                .push((file.to_string(), ims.map(str::to_string)));
            match self.answers.get(file) {
                Some(Ok(Fetched::Image {
                    bytes,
                    last_modified,
                })) => Ok(Fetched::Image {
                    bytes: bytes.clone(),
                    last_modified: last_modified.clone(),
                }),
                Some(Ok(Fetched::NotModified)) => Ok(Fetched::NotModified),
                Some(Ok(Fetched::Missing)) | None => Ok(Fetched::Missing),
                Some(Err(e)) => Err(e.clone()),
            }
        }
    }

    fn image(lm: &str) -> Result<Fetched, String> {
        Ok(Fetched::Image {
            bytes: PNG.to_vec(),
            last_modified: Some(lm.into()),
        })
    }

    #[test]
    fn run_stores_pictures_records_misses_and_publishes_the_index() {
        let dir = TempDir::new();
        let fake = Fake {
            answers: [
                ("A.png".to_string(), image("Tue, 03 Nov 2026 16:00:00 GMT")),
                ("B.png".to_string(), Ok(Fetched::Missing)),
                (
                    "C.png".to_string(),
                    Ok(Fetched::Image {
                        bytes: b"<html>".to_vec(),
                        last_modified: None,
                    }),
                ),
            ]
            .into_iter()
            .collect(),
            asked: RefCell::new(vec![]),
        };
        let mut state = State::default();
        let pauses = RefCell::new(0);
        let w = wanted(&["A.png", "B.png", "C.png"]);
        let s = run(
            &w,
            &mut state,
            &fake,
            dir.path(),
            at("2026-11-03T17:00:00Z"),
            &Options::default(),
            &|_| *pauses.borrow_mut() += 1,
        )
        .unwrap();
        assert_eq!(
            (s.requested, s.stored, s.missing, s.published),
            (3, 1, 2, 1)
        );
        assert_eq!(
            *pauses.borrow(),
            2,
            "a pause between requests, none before the first"
        );
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.path().join(MANIFEST_FILE)).unwrap())
                .unwrap();
        let rel = manifest["items"]["a.png"].as_str().unwrap();
        assert_eq!(std::fs::read(dir.path().join(rel)).unwrap(), PNG);
        assert_eq!(manifest["credit"], CREDIT);
        assert!(
            manifest["items"].get("b.png").is_none() && manifest["items"].get("c.png").is_none()
        );
        assert_eq!(state.files["B.png"].stored, None);
    }

    #[test]
    fn a_recheck_is_conditional_and_an_unchanged_file_keeps_its_picture() {
        let dir = TempDir::new();
        let mut state = State::default();
        state.files.insert(
            "A.png".into(),
            Entry {
                stored: Some("item-art/abc.png".into()),
                last_modified: Some("LM".into()),
                checked_at: "2026-10-01T00:00:00Z".into(),
            },
        );
        let fake = Fake {
            answers: [("A.png".to_string(), Ok(Fetched::NotModified))]
                .into_iter()
                .collect(),
            asked: RefCell::new(vec![]),
        };
        let s = run(
            &wanted(&["A.png"]),
            &mut state,
            &fake,
            dir.path(),
            at("2026-11-03T00:00:00Z"),
            &Options::default(),
            &|_| {},
        )
        .unwrap();
        assert_eq!(
            fake.asked.borrow().as_slice(),
            [("A.png".to_string(), Some("LM".to_string()))]
        );
        assert_eq!((s.unchanged, s.published), (1, 1));
        assert_eq!(
            state.files["A.png"].stored.as_deref(),
            Some("item-art/abc.png")
        );
        assert_eq!(state.files["A.png"].checked_at, "2026-11-03T00:00:00Z");
    }

    #[test]
    fn a_rate_limit_ends_the_run_and_keeps_what_was_done() {
        let dir = TempDir::new();
        let fake = Fake {
            answers: [
                ("A.png".to_string(), image("LM")),
                ("B.png".to_string(), Err("HTTP 429".into())),
            ]
            .into_iter()
            .collect(),
            asked: RefCell::new(vec![]),
        };
        let mut state = State::default();
        let s = run(
            &wanted(&["A.png", "B.png", "C.png"]),
            &mut state,
            &fake,
            dir.path(),
            at("2026-11-03T00:00:00Z"),
            &Options::default(),
            &|_| {},
        )
        .unwrap();
        assert_eq!(s.stopped.as_deref(), Some("HTTP 429"));
        assert_eq!(
            fake.asked.borrow().len(),
            2,
            "nothing is requested after the refusal"
        );
        assert!(state.files.contains_key("A.png") && !state.files.contains_key("B.png"));
        assert_eq!(s.published, 1);
    }

    #[test]
    fn thumb_url_asks_for_the_small_thumbnail_and_encodes_the_name() {
        assert_eq!(
            thumb_url("AxiRelicIntact.png").unwrap(),
            "https://wiki.warframe.com/images/thumb/AxiRelicIntact.png/64px-AxiRelicIntact.png"
        );
        assert_eq!(
            thumb_url("Rhino's Roar.png").unwrap(),
            "https://wiki.warframe.com/images/thumb/Rhino's%20Roar.png/64px-Rhino's%20Roar.png"
        );
    }
}
