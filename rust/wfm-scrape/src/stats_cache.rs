//! Statistics carried between sweeps, so a sweep refetches an item's
//! `/statistics` only when they are due, while its live book is read every
//! sweep.
//!
//! The policy is the one `replay` validated against the observation corpus
//! (2026-09-16 to 2026-10-13, 317 sweeps): refresh on first sight, on an
//! unusable cached entry, when the item reappears after an absence, when the
//! last book actually fetched was empty, and when the cached entry's age
//! reaches the interval. Everything else reuses the cache. Each trigger is
//! evidence a production run has before it fetches; a gate crossing that only
//! fresh statistics would reveal is not one.
//!
//! The file is written once, after a sweep completes, by atomic replacement.
//! A missing, unreadable or foreign file means an empty cache, which is a full
//! refresh: never a failed sweep.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::scrape::StatsStage;

/// Bumped whenever an entry's meaning changes; a reader discards other formats.
pub const FORMAT: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// `None` when the statistics payload was empty: an unusable entry that is
    /// refreshed on the next sweep.
    pub stage: Option<StatsStage>,
    /// UTC, `YYYY-MM-DDTHH:MM:SSZ`; an unreadable stamp counts as due.
    pub fetched_at: String,
    /// The last book this item's sweep fetched held no live orders, which
    /// contradicts statistics that made it eligible.
    pub empty_book: bool,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Cache {
    pub format: u32,
    /// Slugs the last completed sweep attempted, to recognise a reappearance.
    pub present: BTreeSet<String>,
    pub entries: BTreeMap<String, Entry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Bootstrap,
    UnusableCache,
    Reappeared,
    EmptyBook,
    Scheduled,
}

impl Reason {
    pub fn label(self) -> &'static str {
        match self {
            Reason::Bootstrap => "bootstrap",
            Reason::UnusableCache => "unusable_cache",
            Reason::Reappeared => "reappeared",
            Reason::EmptyBook => "empty_book",
            Reason::Scheduled => "scheduled",
        }
    }
}

impl Cache {
    pub fn empty() -> Self {
        Cache {
            format: FORMAT,
            ..Cache::default()
        }
    }

    /// Read the cache, or explain why it starts empty.
    pub fn load(path: &Path) -> (Self, Option<String>) {
        let raw = match std::fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return (
                    Cache::empty(),
                    Some("no statistics cache yet; refreshing every item".into()),
                )
            }
            Err(e) => {
                return (
                    Cache::empty(),
                    Some(format!(
                        "statistics cache unreadable ({e}); refreshing every item"
                    )),
                )
            }
        };
        match serde_json::from_str::<Cache>(&raw) {
            Ok(cache) if cache.format == FORMAT => (cache, None),
            Ok(cache) => (
                Cache::empty(),
                Some(format!(
                    "statistics cache format {} is not {FORMAT}; refreshing every item",
                    cache.format
                )),
            ),
            Err(e) => (
                Cache::empty(),
                Some(format!(
                    "statistics cache malformed ({e}); refreshing every item"
                )),
            ),
        }
    }

    /// Whether `slug` needs fresh statistics at `now`, and why. `None` reuses
    /// the cached entry.
    pub fn due(
        &self,
        slug: &str,
        now: DateTime<Utc>,
        interval: chrono::Duration,
    ) -> Option<Reason> {
        let Some(entry) = self.entries.get(slug) else {
            return Some(Reason::Bootstrap);
        };
        if entry.stage.is_none() {
            Some(Reason::UnusableCache)
        } else if !self.present.contains(slug) {
            Some(Reason::Reappeared)
        } else if entry.empty_book {
            Some(Reason::EmptyBook)
        } else if crate::clock::parse_stamp(&entry.fetched_at)
            .is_none_or(|fetched| now.signed_duration_since(fetched) >= interval)
        {
            Some(Reason::Scheduled)
        } else {
            None
        }
    }

    /// Replace the file atomically. The caller only saves after a completed
    /// sweep, so a failed one leaves the previous cache in place.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {parent:?}: {e}"))?;
        }
        let json =
            serde_json::to_string(self).map_err(|e| format!("serialize statistics cache: {e}"))?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, json).map_err(|e| format!("write {tmp:?}: {e}"))?;
        std::fs::rename(&tmp, path).map_err(|e| format!("rename {tmp:?} → {path:?}: {e}"))
    }
}

/// Parse an interval such as `6h`, `90m` or `1d`.
pub fn parse_interval(raw: &str) -> Result<chrono::Duration, String> {
    let (number, unit) = raw.split_at(raw.find(|c: char| !c.is_ascii_digit()).unwrap_or(raw.len()));
    let n: i64 = number
        .parse()
        .map_err(|_| format!("invalid --stats-interval: {raw}"))?;
    let duration = match unit {
        "m" => chrono::Duration::minutes(n),
        "h" => chrono::Duration::hours(n),
        "d" => chrono::Duration::days(n),
        _ => {
            return Err(format!(
                "invalid --stats-interval unit in {raw}; use m, h or d"
            ))
        }
    };
    if n <= 0 {
        return Err(format!("--stats-interval must be positive: {raw}"));
    }
    Ok(duration)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stage() -> StatsStage {
        StatsStage {
            pick: None,
            volume_48h: 8.0,
            median_now: 26.0,
            median_90d: 25.0,
            medians_7d: vec![25.0, 26.0],
            donch_top_90d: 30.0,
            donch_bot_90d: 20.0,
            avg_price_48h: 26.0,
        }
    }
    fn at(hour: i64) -> DateTime<Utc> {
        DateTime::UNIX_EPOCH + chrono::Duration::hours(hour)
    }

    #[test]
    fn each_trigger_matches_the_replayed_policy() {
        let six = chrono::Duration::hours(6);
        let mut cache = Cache::empty();
        assert_eq!(cache.due("a", at(0), six), Some(Reason::Bootstrap));
        cache.present.insert("a".into());
        cache.entries.insert(
            "a".into(),
            Entry {
                stage: Some(stage()),
                fetched_at: crate::clock::iso_z(at(0)),
                empty_book: false,
            },
        );
        assert_eq!(cache.due("a", at(2), six), None);
        assert_eq!(cache.due("a", at(4), six), None);
        assert_eq!(cache.due("a", at(6), six), Some(Reason::Scheduled));
        cache.entries.get_mut("a").unwrap().empty_book = true;
        assert_eq!(cache.due("a", at(2), six), Some(Reason::EmptyBook));
        cache.entries.get_mut("a").unwrap().empty_book = false;
        cache.present.clear();
        assert_eq!(cache.due("a", at(2), six), Some(Reason::Reappeared));
        cache.present.insert("a".into());
        cache.entries.get_mut("a").unwrap().fetched_at = "garbled".into();
        assert_eq!(cache.due("a", at(2), six), Some(Reason::Scheduled));
        cache.entries.get_mut("a").unwrap().stage = None;
        assert_eq!(cache.due("a", at(2), six), Some(Reason::UnusableCache));
    }

    #[test]
    fn a_missing_or_foreign_file_starts_empty_and_a_saved_one_round_trips() {
        let dir = std::env::temp_dir().join(format!("wfm-stats-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("state/stats-cache.json");
        let (cache, note) = Cache::load(&path);
        assert_eq!(cache, Cache::empty());
        assert!(note.unwrap().contains("no statistics cache"));

        let mut saved = Cache::empty();
        saved.present.insert("a".into());
        saved.entries.insert(
            "a".into(),
            Entry {
                stage: Some(stage()),
                fetched_at: crate::clock::iso_z(at(3)),
                empty_book: true,
            },
        );
        saved.save(&path).unwrap();
        assert_eq!(Cache::load(&path), (saved, None));

        std::fs::write(&path, "{not json").unwrap();
        assert!(Cache::load(&path).1.unwrap().contains("malformed"));
        std::fs::write(&path, r#"{"format":99,"present":[],"entries":{}}"#).unwrap();
        assert_eq!(Cache::load(&path).0, Cache::empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn intervals_parse_in_minutes_hours_and_days() {
        assert_eq!(parse_interval("6h").unwrap(), chrono::Duration::hours(6));
        assert_eq!(
            parse_interval("90m").unwrap(),
            chrono::Duration::minutes(90)
        );
        assert_eq!(parse_interval("1d").unwrap(), chrono::Duration::days(1));
        for bad in ["", "h", "6", "6x", "0h", "-1h"] {
            assert!(parse_interval(bad).is_err(), "{bad}");
        }
    }
}
