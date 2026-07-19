//! Reconcile — per-surface preserve / merge / stamp semantics.
//!
//! Mirror of the Python `reconcile()` closure in `csv_to_market_json.py`
//! (L514-539). The per-surface rules are subtle and interleave with the
//! stale-data warning, lost-entry recovery count, and the wfstat-catalog
//! file-level preserve-on-empty rule — all of which live here as a single
//! tested unit.
//!
//! RULES (contract, not opinion):
//! 1. Empty fresh + prior exists → keep prior data + prior stamp.
//! 2. Partial fetch (complete=false) + prior exists → merge fresh over
//!    prior (old entries the fresh fetch didn't cover are kept), stamp
//!    NOW. Whole-surface stamp on partial merge is INTENTIONAL — retained
//!    entries were just re-validated as still-best-known.
//! 3. Otherwise → return fresh, stamp NOW.
//! 4. wfstat-catalog.json is a file-level unit, not a surface: if the
//!    bulk /items/ fetch returns empty, the prior FILE is kept as-is. We
//!    represent that here as an `Option` — the caller reads the file,
//!    passes `Some(prior_content)`, and receives `None` to signal "write
//!    nothing" vs `Some(bytes)` to write.

use std::collections::HashMap;
use std::hash::Hash;

use chrono::{DateTime, Utc};

use crate::clock;

/// One stale-period warning generated when a kept surface exceeds the
/// threshold. Printed but non-fatal; the binary uses it to alert that the
/// upstream has been unreachable for too long.
#[derive(Debug, Clone, PartialEq)]
pub struct StaleWarning {
    pub surface: String,
    pub days: i64,
}

impl StaleWarning {
    pub fn format(&self) -> String {
        format!(
            "WARNING: {} has been stale for {} days — upstream looks permanently broken, investigate.",
            self.surface, self.days,
        )
    }
}

/// Result of reconciling one surface.
#[derive(Debug, Clone, PartialEq)]
pub struct Reconciled<T> {
    pub data: T,
    pub fetched_at: String,
    pub stale_warning: Option<StaleWarning>,
    pub recovered: usize,
}

/// Anything that can be empty-checked, merged, and length-counted.
pub trait Mergeable: Clone {
    fn is_empty(&self) -> bool;
    fn len(&self) -> usize;
    fn merge(old: &Self, fresh: &Self) -> Self;
}

impl<K: Clone + Eq + Hash, V: Clone> Mergeable for HashMap<K, V> {
    fn is_empty(&self) -> bool {
        self.is_empty()
    }

    fn len(&self) -> usize {
        self.len()
    }

    fn merge(old: &Self, fresh: &Self) -> Self {
        let mut merged = old.clone();
        for (k, v) in fresh.iter() {
            merged.insert(k.clone(), v.clone());
        }
        merged
    }
}

impl Mergeable for serde_json::Value {
    fn is_empty(&self) -> bool {
        match self {
            serde_json::Value::Null => true,
            serde_json::Value::Object(m) => m.is_empty(),
            serde_json::Value::Array(a) => a.is_empty(),
            _ => false,
        }
    }

    fn len(&self) -> usize {
        match self {
            serde_json::Value::Object(m) => m.len(),
            serde_json::Value::Array(a) => a.len(),
            _ => 0,
        }
    }

    fn merge(old: &Self, fresh: &Self) -> Self {
        match (old, fresh) {
            (serde_json::Value::Object(a), serde_json::Value::Object(b)) => {
                let mut merged = a.clone();
                for (k, v) in b {
                    merged.insert(k.clone(), v.clone());
                }
                serde_json::Value::Object(merged)
            }
            (_, fresh) => fresh.clone(),
        }
    }
}

/// Reconcile a single surface, returning the outcome including the
/// assigned `fetched_at` stamp and any stale warning.
///
/// `name` — surface key in `surface_fetched_at` ("path_to_info", etc.).
/// `fresh` — just-fetched data (may be empty on fetch failure).
/// `prior` — data from the prior snapshot (may be `None` if no prior).
/// `prior_stamp` — the stamp from the prior snapshot's `surface_fetched_at`.
/// `now` — the injected clock (same one flowing through render).
/// `complete` — whether ALL upstream endpoints succeeded (true) or any
///   failed (false). Drives the partial-merge path.
/// `stale_days` — threshold for stale warnings (Python uses 7).
pub fn reconcile<T: Mergeable>(
    name: &str,
    fresh: T,
    prior: Option<&T>,
    prior_stamp: Option<&str>,
    now: DateTime<Utc>,
    complete: bool,
    stale_days: i64,
) -> Reconciled<T> {
    if fresh.is_empty() {
        if let Some(old) = prior {
            let kept_since = prior_stamp.unwrap_or("");
            let stamp = if kept_since.is_empty() { clock::iso_z(now) } else { kept_since.to_string() };
            let stale_warning = clock::parse_stamp(kept_since).and_then(|kept_dt| {
                let age = now.signed_duration_since(kept_dt);
                if age.num_days() >= stale_days {
                    Some(StaleWarning {
                        surface: name.to_string(),
                        days: age.num_days(),
                    })
                } else {
                    None
                }
            });
            return Reconciled {
                data: old.clone(),
                fetched_at: stamp,
                stale_warning,
                recovered: 0,
            };
        }
    }

    if !complete {
        if let Some(old) = prior {
            let merged = T::merge(old, &fresh);
            let recovered = merged.len().saturating_sub(fresh.len());
            return Reconciled {
                data: merged,
                fetched_at: clock::iso_z(now),
                stale_warning: None,
                recovered,
            };
        }
    }

    Reconciled {
        fetched_at: clock::iso_z(now),
        stale_warning: None,
        recovered: 0,
        data: fresh,
    }
}

/// wfstat-catalog.json file-level preserve-on-empty rule.
///
/// The bulk /items/ fetch produces a slim catalog that is NOT representable
/// as a `market.json` surface — it can't be reconstructed from the prior
/// snapshot. On an empty fetch, the prior FILE is retained as-is. This
/// function expresses that: `None` means "write nothing" (keep the file
/// on disk), `Some(bytes)` means "write these bytes atomically".
pub fn preserve_catalog_file(
    fresh_bytes: Vec<u8>,
    prior_exists: bool,
) -> Option<Vec<u8>> {
    if fresh_bytes.is_empty() && prior_exists {
        None
    } else if fresh_bytes.is_empty() {
        None
    } else {
        Some(fresh_bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, s).unwrap()
    }

    fn hm<V: Clone>(pairs: &[(&str, V)]) -> HashMap<String, V> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
    }

    // ---- Rule 1: empty fresh + prior exists → keep prior ----------------

    #[test]
    fn empty_fresh_keeps_prior_and_stamp() {
        let prior = hm(&[("a", 1), ("b", 2)]);
        let fresh: HashMap<String, i32> = HashMap::new();
        let now = utc(2026, 7, 1, 12, 0, 0);
        let prior_stamp = "2026-06-01T00:00:00Z";

        let r = reconcile("test_surface", fresh, Some(&prior), Some(prior_stamp), now, true, 7);

        assert_eq!(r.data, prior);
        assert_eq!(r.fetched_at, "2026-06-01T00:00:00Z");
        assert!(r.stale_warning.is_some()); // 30 days > 7
        assert_eq!(r.stale_warning.unwrap().days, 30);
        assert_eq!(r.recovered, 0);
    }

    #[test]
    fn empty_fresh_no_stale_warning_when_recent() {
        let prior = hm(&[("a", 1)]);
        let fresh: HashMap<String, i32> = HashMap::new();
        let now = utc(2026, 6, 5, 0, 0, 0);
        let prior_stamp = "2026-06-01T00:00:00Z";

        let r = reconcile("test", fresh, Some(&prior), Some(prior_stamp), now, true, 7);
        assert!(r.stale_warning.is_none());
    }

    #[test]
    fn empty_fresh_no_prior_stamp_uses_now_and_no_warning() {
        let prior = hm(&[("a", 1)]);
        let fresh: HashMap<String, i32> = HashMap::new();
        let now = utc(2026, 7, 1, 0, 0, 0);

        let r = reconcile("test", fresh, Some(&prior), None, now, true, 7);
        assert_eq!(r.fetched_at, clock::iso_z(now));
        assert!(r.stale_warning.is_none());
    }

    #[test]
    fn empty_fresh_no_prior_returns_fresh_empty() {
        let fresh: HashMap<String, i32> = HashMap::new();
        let now = utc(2026, 7, 1, 0, 0, 0);

        let r = reconcile::<HashMap<String, i32>>("test", fresh, None, None, now, true, 7);
        assert!(r.data.is_empty());
        assert_eq!(r.fetched_at, clock::iso_z(now));
    }

    // ---- Rule 2: partial fetch merges fresh over prior ------------------

    #[test]
    fn partial_fetch_merges_fresh_over_prior_and_stamps_now() {
        let prior = hm(&[("a", 1), ("b", 2), ("c", 3)]);
        let fresh = hm(&[("a", 10), ("d", 40)]);
        let now = utc(2026, 7, 1, 0, 0, 0);

        let r = reconcile("test", fresh, Some(&prior), None, now, false, 7);

        assert_eq!(r.data, hm(&[("a", 10), ("b", 2), ("c", 3), ("d", 40)]));
        assert_eq!(r.fetched_at, clock::iso_z(now));
        assert_eq!(r.recovered, 2); // b + c were kept
        assert!(r.stale_warning.is_none());
    }

    #[test]
    fn partial_fetch_no_prior_returns_fresh_unchanged() {
        let fresh = hm(&[("a", 1)]);
        let now = utc(2026, 7, 1, 0, 0, 0);

        let r = reconcile("test", fresh.clone(), None, None, now, false, 7);
        assert_eq!(r.data, fresh);
        assert_eq!(r.fetched_at, clock::iso_z(now));
        assert_eq!(r.recovered, 0);
    }

    // ---- Rule 3: normal path --------------------------------------------

    #[test]
    fn complete_fresh_returns_as_is_with_now_stamp() {
        let fresh = hm(&[("x", 5)]);
        let prior = hm(&[("x", 3), ("y", 7)]);
        let now = utc(2026, 7, 1, 0, 0, 0);

        let r = reconcile("test", fresh.clone(), Some(&prior), None, now, true, 7);
        assert_eq!(r.data, fresh);
        assert_eq!(r.fetched_at, clock::iso_z(now));
        assert_eq!(r.recovered, 0);
    }

    // ---- serde_json::Value support (baro) -------------------------------

    #[test]
    fn baro_like_empty_fresh_keeps_prior() {
        let prior: serde_json::Value = serde_json::json!({"activation": "2026-07-01T00:00:00Z"});
        let fresh = serde_json::Value::Object(serde_json::Map::new());
        let now = utc(2026, 7, 1, 0, 0, 0);

        let r = reconcile("baro", fresh, Some(&prior), Some("2026-06-30T00:00:00Z"), now, true, 7);
        assert_eq!(r.data, prior);
    }

    #[test]
    fn baro_like_fresh_replaces_prior_when_complete() {
        let prior: serde_json::Value = serde_json::json!({"activation": "old"});
        let fresh = serde_json::json!({"activation": "new", "expiry": "later"});
        let now = utc(2026, 7, 1, 0, 0, 0);

        let r = reconcile("baro", fresh.clone(), Some(&prior), None, now, true, 7);
        assert_eq!(r.data, fresh);
    }

    // ---- wfstat-catalog file-level preserve-on-empty --------------------

    #[test]
    fn catalog_preserve_keeps_file_when_fetch_empty_and_prior_exists() {
        assert_eq!(preserve_catalog_file(vec![], true), None);
    }

    #[test]
    fn catalog_drops_when_fetch_empty_and_no_prior() {
        assert_eq!(preserve_catalog_file(vec![], false), None);
    }

    #[test]
    fn catalog_writes_when_fresh_arrives() {
        assert_eq!(preserve_catalog_file(vec![1, 2, 3], true), Some(vec![1, 2, 3]));
        assert_eq!(preserve_catalog_file(vec![1, 2, 3], false), Some(vec![1, 2, 3]));
    }
}
