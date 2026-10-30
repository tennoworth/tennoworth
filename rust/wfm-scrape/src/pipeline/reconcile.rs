//! Reconcile - per-surface preserve / merge / stamp semantics.
//!
//! The per-surface rules are subtle and interleave with the stale-data
//! warning, lost-entry recovery count, and the wfstat-catalog file-level
//! preserve-on-empty rule - all of which live here as a single tested unit.
//!
//! RULES (contract, not opinion):
//! 1. Unavailable, unchanged, or invalid observations keep prior data and its
//!    data timestamp. These states are distinct in provenance even though the
//!    data transition is the same.
//! 2. A usable partial fetch with prior data merges fresh over prior (old
//!    entries the fresh fetch didn't cover are kept) and keeps the PRIOR
//!    stamp: only the fresh entries were re-observed, so the surface is as old
//!    as the entries it carried. A merge that carried nothing takes NOW. The
//!    retained entries are still-best-known, but nothing re-validated them, and
//!    stamping the surface NOW is what let a partition that stopped updating
//!    report itself as current - the case the age warning exists to reveal.
//!    Surfaces keyed by item (`reconcile_keyed`) refine this: each carried key
//!    keeps its own stamp, the surface is as old as its oldest carried key,
//!    and a key carried for `stale_days` or of unknown age is dropped.
//! 3. Authoritative empty is data. It clears a prior surface and stamps NOW;
//!    it must never fall into preserve-on-empty.
//! 4. Otherwise → return usable data, stamp NOW.
//! 5. wfstat-catalog.json is a file-level unit, not a surface: if the
//!    bulk /items/ fetch returns empty, the prior FILE is kept as-is. We
//!    represent that here as an `Option` - the caller reads the file,
//!    passes `Some(prior_content)`, and receives `None` to signal "write
//!    nothing" vs `Some(bytes)` to write.

use std::collections::{BTreeMap, HashMap};
use std::hash::Hash;

use chrono::{DateTime, Utc};

use crate::clock;

/// What the upstream observation actually established.
#[derive(Debug, Clone, PartialEq)]
pub enum Observation<T> {
    /// The request could not be made or did not receive a response.
    Unavailable,
    /// A content hash or cache validator proved the prior data is current.
    Unchanged,
    /// A response arrived but could not be trusted (malformed or all-invalid).
    Invalid,
    /// Valid data. `complete=false` means some independently fetched children
    /// failed and prior keys may be retained.
    Usable { data: T, complete: bool },
    /// A valid response explicitly asserted that the surface has no rows.
    AuthoritativeEmpty,
}

impl<T> Observation<T> {
    pub fn usable(data: T) -> Self {
        Self::Usable { data, complete: true }
    }

    pub fn partial(data: T) -> Self {
        Self::Usable { data, complete: false }
    }
}

/// Exact reason the published value looks the way it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Disposition {
    PublishedFresh,
    MergedPartial,
    ClearedAuthoritativeEmpty,
    PreservedUnavailable,
    PreservedUnchanged,
    PreservedInvalid,
    EmptyUnavailable,
    EmptyUnchanged,
    EmptyInvalid,
}

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
            "WARNING: {} has been stale for {} days - upstream looks permanently broken, investigate.",
            self.surface, self.days,
        )
    }
}

/// Result of reconciling one surface.
#[derive(Debug, Clone, PartialEq)]
pub struct Reconciled<T> {
    pub data: T,
    pub fetched_at: String,
    /// Time this upstream was observed, even when the data timestamp remains
    /// old. Keeping this separate prevents a successful hash check from making
    /// carried child data look freshly fetched.
    pub attempted_at: String,
    pub disposition: Disposition,
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

impl<K: Clone + Ord, V: Clone> Mergeable for std::collections::BTreeMap<K, V> {
    fn is_empty(&self) -> bool { self.is_empty() }
    fn len(&self) -> usize { self.len() }
    fn merge(old: &Self, fresh: &Self) -> Self {
        let mut merged = old.clone();
        for (key, value) in fresh { merged.insert(key.clone(), value.clone()); }
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

impl<T: Clone> Mergeable for Vec<T> {
    fn is_empty(&self) -> bool { self.is_empty() }
    fn len(&self) -> usize { self.len() }
    fn merge(_old: &Self, fresh: &Self) -> Self { fresh.clone() }
}

/// Reconcile a single surface, returning the outcome including the
/// assigned `fetched_at` stamp and any stale warning.
///
/// `name` - surface key in `surface_fetched_at` ("path_to_info", etc.).
/// `observation` - the classified upstream result. Empty is meaningful only
///   when explicitly classified as [`Observation::AuthoritativeEmpty`].
/// `prior` - data from the prior snapshot (may be `None` if no prior).
/// `prior_stamp` - the stamp from the prior snapshot's `surface_fetched_at`.
/// `now` - the injected clock (same one flowing through render).
/// `stale_days` - threshold for warnings about preserved data that could not
///   be verified against the current upstream.
pub fn reconcile<T: Mergeable + Default>(
    name: &str,
    observation: Observation<T>,
    prior: Option<&T>,
    prior_stamp: Option<&str>,
    now: DateTime<Utc>,
    stale_days: i64,
) -> Reconciled<T> {
    let attempted_at = clock::iso_z(now);
    let preserve = |disposition: Disposition| {
        if let Some(old) = prior {
            let kept_since = prior_stamp.unwrap_or("");
            let stamp = kept_since.to_string();
            // A matching content hash proves the retained payload is still
            // current. Only a failed or invalid observation makes its age an
            // operational warning.
            let stale_warning = if disposition == Disposition::PreservedUnchanged {
                None
            } else {
                clock::parse_stamp(kept_since).and_then(|kept_dt| {
                    let age = now.signed_duration_since(kept_dt);
                    if age.num_days() >= stale_days {
                        Some(StaleWarning {
                            surface: name.to_string(),
                            days: age.num_days(),
                        })
                    } else {
                        None
                    }
                })
            };
            Reconciled {
                data: old.clone(),
                fetched_at: stamp,
                attempted_at: attempted_at.clone(),
                disposition,
                stale_warning,
                recovered: 0,
            }
        } else {
            let empty_disposition = match disposition {
                Disposition::PreservedUnavailable => Disposition::EmptyUnavailable,
                Disposition::PreservedUnchanged => Disposition::EmptyUnchanged,
                Disposition::PreservedInvalid => Disposition::EmptyInvalid,
                other => other,
            };
            Reconciled {
                data: T::default(),
                fetched_at: attempted_at.clone(),
                attempted_at: attempted_at.clone(),
                disposition: empty_disposition,
                stale_warning: None,
                recovered: 0,
            }
        }
    };

    match observation {
        Observation::Unavailable => preserve(Disposition::PreservedUnavailable),
        Observation::Unchanged => preserve(Disposition::PreservedUnchanged),
        Observation::Invalid => preserve(Disposition::PreservedInvalid),
        Observation::AuthoritativeEmpty => Reconciled {
            data: T::default(),
            fetched_at: attempted_at.clone(),
            attempted_at,
            disposition: Disposition::ClearedAuthoritativeEmpty,
            stale_warning: None,
            recovered: 0,
        },
        Observation::Usable { data: fresh, complete: false } if fresh.is_empty() => {
            preserve(Disposition::PreservedInvalid)
        }
        Observation::Usable { data: fresh, complete: true } if fresh.is_empty() => {
            preserve(Disposition::PreservedInvalid)
        }
        Observation::Usable { data: fresh, complete: false } => {
          if let Some(old) = prior {
            let merged = T::merge(old, &fresh);
            let recovered = merged.len().saturating_sub(fresh.len());
            Reconciled {
                data: merged,
                // Only `fresh`'s keys were re-observed, so a surface holding
                // carried rows is exactly as old as the oldest of them. Stamping
                // it NOW is what let a partition that stopped updating report
                // itself as current, which is the case the age warning exists to
                // reveal. A merge that carried nothing takes the current time.
                fetched_at: if recovered > 0 {
                    prior_stamp.unwrap_or("").to_string()
                } else {
                    clock::iso_z(now)
                },
                attempted_at,
                disposition: Disposition::MergedPartial,
                stale_warning: None,
                recovered,
            }
          } else {
            Reconciled {
                data: fresh,
                fetched_at: clock::iso_z(now),
                attempted_at,
                disposition: Disposition::MergedPartial,
                stale_warning: None,
                recovered: 0,
            }
          }
        }
        Observation::Usable { data, complete: true } => Reconciled {
            fetched_at: clock::iso_z(now),
            attempted_at,
            disposition: Disposition::PublishedFresh,
            stale_warning: None,
            recovered: 0,
            data,
        },
    }
}

/// When each key of a partially fetchable surface was last observed: every key
/// at `fresh_at`, except the ones a partial merge carried, which keep their own.
///
/// One surface stamp cannot describe rows from different runs. Pinned to the
/// prior stamp whenever anything was carried, it reported a surface as old as
/// its oldest run for as long as some endpoint kept failing, even when every
/// row had been refreshed within the last few runs.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct KeyStamps {
    pub fresh_at: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub carried: BTreeMap<String, String>,
}

impl KeyStamps {
    fn of(&self, key: &str) -> &str {
        self.carried.get(key).map_or(self.fresh_at.as_str(), String::as_str)
    }
}

/// [`reconcile`] for a surface keyed by item, tracking each key's evidence age.
///
/// Everything but a partial merge over prior data behaves exactly as
/// `reconcile`. A partial merge keeps a carried key only while its own stamp is
/// younger than `stale_days`: a key upstream no longer returns would otherwise
/// be carried, and age the surface, for as long as some fetch keeps failing. A
/// key whose age cannot be established is not kept either, since nothing
/// bounds it. Dropping keys raises the stale warning.
pub fn reconcile_keyed<V: Clone>(
    name: &str,
    observation: Observation<HashMap<String, V>>,
    prior: Option<&HashMap<String, V>>,
    prior_stamp: Option<&str>,
    prior_keys: Option<&KeyStamps>,
    now: DateTime<Utc>,
    stale_days: i64,
) -> (Reconciled<HashMap<String, V>>, KeyStamps) {
    let (mut data, prior_map) = match (observation, prior) {
        (Observation::Usable { data, complete: false }, Some(prior)) if !data.is_empty() => {
            (data, prior)
        }
        (observation, prior) => {
            let r = reconcile(name, observation, prior, prior_stamp, now, stale_days);
            let stamps = match (r.disposition, prior_keys) {
                (
                    Disposition::PreservedUnavailable
                    | Disposition::PreservedUnchanged
                    | Disposition::PreservedInvalid,
                    Some(prior_keys),
                ) => prior_keys.clone(),
                _ => KeyStamps {
                    fresh_at: r.fetched_at.clone(),
                    carried: BTreeMap::new(),
                },
            };
            return (r, stamps);
        }
    };

    // A snapshot written before per-key stamps observed every key at once.
    let legacy = KeyStamps {
        fresh_at: prior_stamp.unwrap_or("").to_string(),
        carried: BTreeMap::new(),
    };
    let stamps = prior_keys.unwrap_or(&legacy);
    let mut carried = BTreeMap::new();
    let mut oldest = now;
    let mut dropped_days: Option<i64> = None;
    for (key, value) in prior_map {
        if data.contains_key(key) {
            continue;
        }
        let stamp = stamps.of(key);
        match clock::parse_stamp(stamp) {
            Some(at) if now.signed_duration_since(at).num_days() < stale_days => {
                oldest = oldest.min(at);
                data.insert(key.clone(), value.clone());
                carried.insert(key.clone(), stamp.to_string());
            }
            at => {
                let days = at.map_or(stale_days, |at| now.signed_duration_since(at).num_days());
                dropped_days = Some(dropped_days.map_or(days, |d| d.max(days)));
            }
        }
    }
    let observed = clock::iso_z(now);
    let reconciled = Reconciled {
        data,
        fetched_at: clock::iso_z(oldest),
        attempted_at: observed.clone(),
        disposition: Disposition::MergedPartial,
        stale_warning: dropped_days.map(|days| StaleWarning {
            surface: name.to_string(),
            days,
        }),
        recovered: carried.len(),
    };
    (
        reconciled,
        KeyStamps {
            fresh_at: observed,
            carried,
        },
    )
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

    fn reconcile<T: Mergeable + Default>(
        name: &str,
        fresh: T,
        prior: Option<&T>,
        prior_stamp: Option<&str>,
        now: DateTime<Utc>,
        complete: bool,
        stale_days: i64,
    ) -> Reconciled<T> {
        let observation = if fresh.is_empty() {
            Observation::Unavailable
        } else if complete {
            Observation::usable(fresh)
        } else {
            Observation::partial(fresh)
        };
        super::reconcile(name, observation, prior, prior_stamp, now, stale_days)
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
    fn empty_fresh_no_prior_stamp_keeps_unknown_age() {
        let prior = hm(&[("a", 1)]);
        let fresh: HashMap<String, i32> = HashMap::new();
        let now = utc(2026, 7, 1, 0, 0, 0);

        let r = reconcile("test", fresh, Some(&prior), None, now, true, 7);
        assert_eq!(r.fetched_at, "");
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
    fn partial_fetch_merges_fresh_over_prior_keeping_the_prior_stamp() {
        let prior = hm(&[("a", 1), ("b", 2), ("c", 3)]);
        let fresh = hm(&[("a", 10), ("d", 40)]);
        let now = utc(2026, 7, 1, 0, 0, 0);
        let prior_stamp = "2026-06-20T00:00:00Z";

        let r = reconcile("test", fresh, Some(&prior), Some(prior_stamp), now, false, 7);

        assert_eq!(r.data, hm(&[("a", 10), ("b", 2), ("c", 3), ("d", 40)]));
        assert_eq!(
            r.fetched_at, prior_stamp,
            "two rows were carried, so the surface is still that old"
        );
        assert_eq!(r.attempted_at, clock::iso_z(now), "the attempt is recorded");
        assert_eq!(r.recovered, 2); // b + c were kept
        assert!(r.stale_warning.is_none());
    }

    /// The failure the stale-data warning exists to reveal is a surface that
    /// quietly stops updating. A partition whose parent keeps partially
    /// succeeding reaches the merge above on every run, and the entries that
    /// partition owns are carried from the prior map - never re-fetched. If the
    /// merge stamps NOW, a surface whose real evidence is four weeks old reports
    /// itself as fetched this minute, and the consumer's age warning - which
    /// exists to reveal exactly this - stays suppressed forever. The carried
    /// entries are the prior map's values, taken unchanged, which is the whole
    /// difference from the `preserved_unchanged` case the consumer exempts.
    #[test]
    fn a_partial_merge_keeps_the_evidence_age_of_the_rows_it_carried() {
        let prior = hm(&[("live", 1), ("carried", 2)]);
        let fresh = hm(&[("live", 10)]);
        let first_seen = "2026-07-01T00:00:00Z";

        let later = reconcile(
            "set_to_parts",
            fresh,
            Some(&prior),
            Some(first_seen),
            utc(2026, 7, 29, 0, 0, 0),
            false,
            7,
        );

        // The carried value is byte-for-byte the prior one - never re-fetched.
        assert_eq!(later.data.get("carried"), prior.get("carried"));
        assert_eq!(later.recovered, 1);
        // So the surface's evidence age is still the day that row arrived, not
        // the day it was last copied forward.
        assert_eq!(later.fetched_at, first_seen);
        // The attempt itself is still recorded separately.
        assert_eq!(later.attempted_at, clock::iso_z(utc(2026, 7, 29, 0, 0, 0)));
        assert_eq!(later.disposition, Disposition::MergedPartial);
    }

    /// The counterpart: when the partial fetch covered everything the prior
    /// snapshot held, nothing was carried and every row in the result is freshly
    /// observed, so it takes the current stamp and must not inherit the old one.
    #[test]
    fn a_partial_merge_that_carried_nothing_is_stamped_now() {
        let prior = hm(&[("live", 1), ("also", 2)]);
        let fresh = hm(&[("live", 10), ("also", 20)]);

        let r = reconcile(
            "set_to_parts",
            fresh,
            Some(&prior),
            Some("2026-07-01T00:00:00Z"),
            utc(2026, 7, 29, 0, 0, 0),
            false,
            7,
        );

        assert_eq!(r.recovered, 0, "every prior row was re-fetched");
        assert_eq!(r.fetched_at, clock::iso_z(utc(2026, 7, 29, 0, 0, 0)));
    }

    /// A legacy snapshot can hold rows without a source stamp. Their age is
    /// unknown even after a later attempt partially refreshes the surface.
    #[test]
    fn a_partial_merge_without_a_prior_stamp_keeps_unknown_age() {
        let prior = hm(&[("carried", 2)]);
        let fresh = hm(&[("live", 10)]);

        let r = reconcile(
            "set_to_parts",
            fresh,
            Some(&prior),
            None,
            utc(2026, 7, 29, 0, 0, 0),
            false,
            7,
        );

        assert_eq!(r.recovered, 1);
        assert_eq!(r.fetched_at, "");
    }

    // ---- reconcile_keyed: per-key evidence age -----------------------------

    fn keyed(
        observation: Observation<HashMap<String, i32>>,
        prior: Option<&HashMap<String, i32>>,
        prior_stamp: Option<&str>,
        prior_keys: Option<&KeyStamps>,
        now: DateTime<Utc>,
    ) -> (Reconciled<HashMap<String, i32>>, KeyStamps) {
        reconcile_keyed("set_to_parts", observation, prior, prior_stamp, prior_keys, now, 7)
    }

    /// Different endpoints failing on successive runs is the ordinary case. Every
    /// row was fetched within the last run or two, so the surface must not
    /// report the age of the first failure for as long as failures continue.
    #[test]
    fn rolling_partial_fetches_age_the_surface_by_its_oldest_carried_key() {
        let t0 = utc(2026, 7, 1, 0, 0, 0);
        let t1 = utc(2026, 7, 1, 2, 0, 0);
        let t2 = utc(2026, 7, 1, 4, 0, 0);

        let (first, stamps) = keyed(Observation::usable(hm(&[("a", 1), ("b", 2)])), None, None, None, t0);
        assert_eq!(first.disposition, Disposition::PublishedFresh);

        let (second, stamps) = keyed(
            Observation::partial(hm(&[("a", 10)])),
            Some(&first.data),
            Some(&first.fetched_at),
            Some(&stamps),
            t1,
        );
        assert_eq!(second.data, hm(&[("a", 10), ("b", 2)]));
        assert_eq!(second.fetched_at, clock::iso_z(t0), "b was last fetched at t0");

        let (third, stamps) = keyed(
            Observation::partial(hm(&[("b", 20)])),
            Some(&second.data),
            Some(&second.fetched_at),
            Some(&stamps),
            t2,
        );
        assert_eq!(third.data, hm(&[("a", 10), ("b", 20)]));
        assert_eq!(
            third.fetched_at,
            clock::iso_z(t1),
            "a was fetched at t1; a surface stamp would still say t0"
        );
        assert_eq!(third.recovered, 1);
        assert_eq!(
            stamps,
            KeyStamps {
                fresh_at: clock::iso_z(t2),
                carried: [("a".to_string(), clock::iso_z(t1))].into(),
            }
        );
    }

    /// A key upstream stopped returning is carried only for the stale window;
    /// after that it is dropped, the operator is warned, and the surface stops
    /// aging on its account.
    #[test]
    fn a_key_carried_past_the_stale_window_is_dropped_with_a_warning() {
        let prior = hm(&[("live", 1), ("gone", 2)]);
        let prior_keys = KeyStamps {
            fresh_at: "2026-07-20T00:00:00Z".into(),
            carried: [("gone".to_string(), "2026-07-01T00:00:00Z".to_string())].into(),
        };
        let now = utc(2026, 7, 21, 0, 0, 0);

        let (r, stamps) = keyed(
            Observation::partial(hm(&[("live", 10)])),
            Some(&prior),
            Some("2026-07-01T00:00:00Z"),
            Some(&prior_keys),
            now,
        );

        assert_eq!(r.data, hm(&[("live", 10)]));
        assert_eq!(r.fetched_at, clock::iso_z(now));
        assert_eq!(r.recovered, 0);
        assert_eq!(r.disposition, Disposition::MergedPartial);
        assert_eq!(
            r.stale_warning,
            Some(StaleWarning { surface: "set_to_parts".into(), days: 20 })
        );
        assert!(stamps.carried.is_empty());
    }

    /// Inside the window the carried key stays, with its own age.
    #[test]
    fn a_key_inside_the_stale_window_is_kept_at_its_own_age() {
        let prior = hm(&[("live", 1), ("carried", 2)]);
        let prior_keys = KeyStamps {
            fresh_at: "2026-07-18T00:00:00Z".into(),
            carried: BTreeMap::new(),
        };
        let now = utc(2026, 7, 21, 0, 0, 0);

        let (r, _) = keyed(
            Observation::partial(hm(&[("live", 10)])),
            Some(&prior),
            Some("2026-07-18T00:00:00Z"),
            Some(&prior_keys),
            now,
        );

        assert_eq!(r.data.get("carried"), Some(&2));
        assert_eq!(r.fetched_at, "2026-07-18T00:00:00Z");
        assert!(r.stale_warning.is_none());
    }

    /// A snapshot from before per-key stamps observed every key at its surface
    /// stamp. Without one, a carried key's age is unknown and nothing bounds it.
    #[test]
    fn a_legacy_prior_uses_its_surface_stamp_and_drops_keys_of_unknown_age() {
        let prior = hm(&[("live", 1), ("carried", 2)]);
        let now = utc(2026, 7, 21, 0, 0, 0);
        let partial = || Observation::partial(hm(&[("live", 10)]));

        let (dated, _) = keyed(partial(), Some(&prior), Some("2026-07-19T00:00:00Z"), None, now);
        assert_eq!(dated.data.get("carried"), Some(&2));
        assert_eq!(dated.fetched_at, "2026-07-19T00:00:00Z");

        let (undated, _) = keyed(partial(), Some(&prior), None, None, now);
        assert_eq!(undated.data, hm(&[("live", 10)]));
        assert_eq!(undated.fetched_at, clock::iso_z(now));
        assert!(undated.stale_warning.is_some());
    }

    /// A failed read keeps the prior rows, so it keeps their stamps too.
    #[test]
    fn a_preserved_surface_keeps_its_key_stamps() {
        let prior = hm(&[("a", 1)]);
        let prior_keys = KeyStamps {
            fresh_at: "2026-07-18T00:00:00Z".into(),
            carried: [("a".to_string(), "2026-07-17T00:00:00Z".to_string())].into(),
        };

        let (r, stamps) = keyed(
            Observation::Unavailable,
            Some(&prior),
            Some("2026-07-17T00:00:00Z"),
            Some(&prior_keys),
            utc(2026, 7, 21, 0, 0, 0),
        );

        assert_eq!(r.disposition, Disposition::PreservedUnavailable);
        assert_eq!(stamps, prior_keys);
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

    #[test]
    fn authoritative_empty_clears_prior_and_stamps_now() {
        let prior = hm(&[("old", 1)]);
        let now = utc(2026, 7, 1, 0, 0, 0);
        let r = super::reconcile(
            "events",
            Observation::<HashMap<String, i32>>::AuthoritativeEmpty,
            Some(&prior),
            Some("2026-06-01T00:00:00Z"),
            now,
            7,
        );
        assert!(r.data.is_empty());
        assert_eq!(r.fetched_at, "2026-07-01T00:00:00Z");
        assert_eq!(r.disposition, Disposition::ClearedAuthoritativeEmpty);
    }

    #[test]
    fn preserve_states_have_distinct_provenance_and_warning_semantics() {
        let prior = hm(&[("old", 1)]);
        let now = utc(2026, 7, 1, 0, 0, 0);
        let cases = [
            (Observation::Unavailable, Disposition::PreservedUnavailable, true),
            (Observation::Unchanged, Disposition::PreservedUnchanged, false),
            (Observation::Invalid, Disposition::PreservedInvalid, true),
        ];
        for (observation, expected, warns) in cases {
            let r = super::reconcile(
                "surface", observation, Some(&prior), Some("2026-06-01T00:00:00Z"), now, 7,
            );
            assert_eq!(r.data, prior);
            assert_eq!(r.fetched_at, "2026-06-01T00:00:00Z");
            assert_eq!(r.attempted_at, "2026-07-01T00:00:00Z");
            assert_eq!(r.disposition, expected);
            assert_eq!(r.stale_warning.is_some(), warns);
        }
    }

    #[test]
    fn usable_empty_is_invalid_and_cannot_clear_prior() {
        let prior = hm(&[("old", 1)]);
        let now = utc(2026, 7, 1, 0, 0, 0);
        for observation in [Observation::usable(HashMap::new()), Observation::partial(HashMap::new())] {
            let r = super::reconcile("surface", observation, Some(&prior), None, now, 7);
            assert_eq!(r.data, prior);
            assert_eq!(r.disposition, Disposition::PreservedInvalid);
        }
    }
}
