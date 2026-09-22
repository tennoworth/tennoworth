//! Pending-plan persistence - crash-recovery for a listing batch.
//!
//! Every plan is written to `~/.config/wfminv/pending_plan.json` before the
//! first POST, rewritten after each item, and deleted on clean completion. The
//! browser polls it on (re)connect and offers Resume / Discard. Writes are
//! atomic (tmp + rename) so a concurrent read never sees a torn file - same
//! convention as `os.replace` in the retired Python pipeline - and the tmp
//! file is created at 0600 (it holds unsubmitted listing details).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

use crate::platform::{chown_to_real_user, restrict_dir_perms, write_restricted};

#[derive(Serialize, Deserialize, Clone)]
pub struct PendingPlan {
    pub plan_id: String,
    pub started_at: String,
    pub items: Vec<PendingItem>,
}

/// Journal status for an item whose send has not been resolved.
///
/// "Saved for explicit resume" - the request was never dispatched, or the
/// governor refused it before it went out. Safe to offer again.
pub const STATUS_PENDING: &str = "pending";

/// Journal status for an item whose send outcome is unknown.
///
/// The request may have reached the market and been applied. Deliberately NOT
/// `pending`: the resume loop re-offers everything it believes was never sent,
/// so recording an unknown outcome as `pending` would both misreport the journal
/// and retry a mutation that may already have landed.
pub const STATUS_UNCERTAIN: &str = "uncertain_mutation";

/// Journal status for an item the market accepted.
pub const STATUS_OK: &str = "ok";

/// Journal status for an item the market refused.
pub const STATUS_ERROR: &str = "error";

#[derive(Serialize, Deserialize, Clone)]
pub struct PendingItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inventory_snapshot_id: Option<i64>,
    pub slug: String,
    pub platinum: u32,
    pub quantity: u32,
    /// Absent in shipped pending files; retain the legacy inferred lot on resume.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_trade: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<crate::trading::plan::SessionConstraint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewed_order: Option<crate::trading::plan::ReviewedOrder>,
    pub order_type: String,
    pub visible: bool,
    pub rank: Option<u32>,
    #[serde(default)]
    pub subtype: Option<String>,
    #[serde(default)]
    pub reference_low_sell: Option<u32>,
    /// One of [`STATUS_PENDING`], [`STATUS_UNCERTAIN`], [`STATUS_OK`] or
    /// [`STATUS_ERROR`].
    pub status: String,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub order_id: Option<String>,
    /// "created" | "updated" once terminal - how the ok state was reached.
    /// Default None keeps pre-reconcile pending files loadable.
    #[serde(default)]
    pub action: Option<String>,
}

pub fn write_pending_atomic(path: &Path, plan: &PendingPlan) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
        restrict_dir_perms(parent);
        chown_to_real_user(parent);
    }
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec(plan).context("serializing pending plan")?;
    // Create the tmp file at 0o600 from the first syscall - pending plans
    // contain unsubmitted listing details, not OK to leak to other local
    // users even briefly.
    write_restricted(&tmp, &bytes)?;
    fs::rename(&tmp, path)
        .with_context(|| format!("renaming {} → {}", tmp.display(), path.display()))?;
    // chown the final path back to the real user so a sudo invocation of
    // `serve` doesn't leave a root-owned file in their config dir.
    chown_to_real_user(path);
    Ok(())
}

/// Why the pending-plan journal could not be read or removed. Both used to be
/// collapsed into "no journal", which is how an unreadable file came to look
/// exactly like an absent one.
#[derive(Debug)]
pub enum PendingStoreError {
    /// The file exists but could not be read or parsed. Its contents are the only
    /// record of what was already sent, so this is evidence to preserve, not an
    /// absence to paper over.
    Unreadable(String),
    /// The file exists but could not be removed.
    Undeletable(String),
}

impl std::fmt::Display for PendingStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreadable(detail) => write!(f, "the saved batch could not be read: {detail}"),
            Self::Undeletable(detail) => write!(f, "the saved batch could not be removed: {detail}"),
        }
    }
}

impl std::error::Error for PendingStoreError {}

/// What the journal says about starting a new batch. `Unreadable` is deliberately
/// not `Absent`: submitting would replace the only record of what was sent.
#[derive(Debug)]
pub enum JournalState {
    /// Nothing saved.
    Absent,
    /// Saved, and nothing is left to send.
    Finished,
    /// Saved, with at least one item still pending or of unknown outcome.
    Unfinished,
    /// Saved but unreadable.
    Unreadable(String),
}

/// Read the journal's state for a submit/recovery decision. The only case that
/// permits replacing the file is a journal that is absent or fully finished.
pub fn journal_state(path: &Path) -> JournalState {
    match load_pending(path) {
        Ok(None) => JournalState::Absent,
        Ok(Some(plan)) => {
            if plan.items.iter().any(|item| item.status == "pending") {
                JournalState::Unfinished
            } else {
                JournalState::Finished
            }
        }
        Err(error) => JournalState::Unreadable(error.to_string()),
    }
}

pub fn load_pending(path: &Path) -> Result<Option<PendingPlan>, PendingStoreError> {
    match fs::read(path) {
        Ok(data) => serde_json::from_slice(&data).map(Some).map_err(|e| {
            PendingStoreError::Unreadable(format!("{} is not a readable batch: {e}", path.display()))
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(PendingStoreError::Unreadable(format!(
            "{}: {error}",
            path.display()
        ))),
    }
}

/// Remove the journal. An already-absent file is success - only a file that is
/// there and could not be removed is an error.
pub fn clear_pending(path: &Path) -> Result<(), PendingStoreError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(PendingStoreError::Undeletable(format!(
            "{}: {error}",
            path.display()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::path::PathBuf;

    fn tmp_path(name: &str) -> PathBuf {
        let mut p = env::temp_dir();
        p.push(format!(
            "wfmcore-pending-{}-{}.json",
            std::process::id(),
            name
        ));
        p
    }

    fn sample_plan() -> PendingPlan {
        PendingPlan {
            plan_id: "abc12345".into(),
            started_at: "2026-05-27T15:30:00Z".into(),
            items: vec![
                PendingItem {
                    inventory_snapshot_id: Some(7),
                    slug: "loki_prime_set".into(),
                    platinum: 120,
                    quantity: 1,
                    per_trade: Some(1),
                    session: None,
                    reviewed_order: None,
                    order_type: "sell".into(),
                    visible: false,
                    rank: None,
                    subtype: None,
                    reference_low_sell: Some(110),
                    status: "ok".into(),
                    message: None,
                    order_id: Some("order-1".into()),
                    action: Some("created".into()),
                },
                PendingItem {
                    inventory_snapshot_id: Some(7),
                    slug: "rhino_prime_set".into(),
                    platinum: 95,
                    quantity: 1,
                    per_trade: None,
                    session: None,
                    reviewed_order: None,
                    order_type: "sell".into(),
                    visible: false,
                    rank: None,
                    subtype: None,
                    reference_low_sell: Some(90),
                    status: "pending".into(),
                    message: None,
                    order_id: None,
                    action: None,
                },
            ],
        }
    }

    #[test]
    fn reviewed_inventory_identity_survives_pending_recovery() {
        let plan = sample_plan();
        let bytes = serde_json::to_vec(&plan).unwrap();
        let restored: PendingPlan = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(crate::trading::plan::PlanItem::from(&restored.items[0]).inventory_snapshot_id, Some(7));
    }

    #[test]
    fn pending_plan_roundtrips_through_disk() {
        let path = tmp_path("roundtrip");
        let plan = sample_plan();
        write_pending_atomic(&path, &plan).unwrap();

        let loaded = load_pending(&path)
            .expect("file readable")
            .expect("present");
        assert_eq!(loaded.plan_id, plan.plan_id);
        assert_eq!(loaded.items.len(), 2);
        assert_eq!(loaded.items[0].status, "ok");
        assert_eq!(loaded.items[1].status, "pending");
        assert_eq!(loaded.items[0].per_trade, Some(1));
        assert_eq!(loaded.items[1].per_trade, None);

        let _ = clear_pending(&path);
        assert!(load_pending(&path).expect("readable").is_none());
    }

    #[test]
    fn pending_bundle_survives_disk_recovery_without_reinference() {
        let path = tmp_path("bundle");
        let mut plan = sample_plan();
        plan.items[1].quantity = 12;
        plan.items[1].per_trade = Some(3);
        plan.items[1].session = Some(crate::trading::plan::SessionConstraint {
            snapshot_id: 12,
            utc_day: 20_000,
            budget: 8,
        });
        plan.items[1].reviewed_order = Some(crate::trading::plan::ReviewedOrder::New);
        write_pending_atomic(&path, &plan).unwrap();
        let loaded = load_pending(&path).expect("readable").expect("present");
        assert_eq!(loaded.items[1].quantity, 12);
        assert_eq!(loaded.items[1].per_trade, Some(3));
        let resumed = crate::trading::plan::PlanItem::from(&loaded.items[1]);
        assert_eq!(resumed.per_trade, Some(3));
        assert_eq!(resumed.session, plan.items[1].session);
        assert_eq!(resumed.reviewed_order, plan.items[1].reviewed_order);
        let _ = clear_pending(&path);
    }

    #[test]
    fn load_pending_returns_none_when_missing() {
        let path = tmp_path("missing");
        let _ = std::fs::remove_file(&path);
        assert!(load_pending(&path).expect("readable").is_none());
    }

    #[test]
    fn load_pending_tolerates_missing_optional_fields() {
        // older file written before rank/reference_low_sell were optional, or
        // a hand-edit. Deserialization must still succeed.
        let path = tmp_path("partial");
        let raw = r#"{
            "plan_id":"x","started_at":"t","items":[
              {"slug":"a","platinum":5,"quantity":1,"order_type":"sell","visible":false,"rank":null,"status":"pending"}
            ]}"#;
        std::fs::write(&path, raw).unwrap();
        let loaded = load_pending(&path).expect("readable").expect("parses");
        assert_eq!(loaded.items.len(), 1);
        assert!(loaded.items[0].order_id.is_none());
        assert!(loaded.items[0].reference_low_sell.is_none());
        assert!(loaded.items[0].per_trade.is_none());
        let _ = clear_pending(&path);
    }

    #[test]
    fn atomic_write_leaves_no_tmp_file() {
        let path = tmp_path("notmp");
        write_pending_atomic(&path, &sample_plan()).unwrap();
        let tmp = path.with_extension("json.tmp");
        assert!(!tmp.exists(), "tmp file should be renamed away");
        assert!(path.exists());
        let _ = clear_pending(&path);
    }

    /// The only states that permit replacing the journal are "nothing saved" and
    /// "nothing left to send". A file that is there but unreadable is neither:
    /// starting a batch would overwrite the sole record of what was already sent.
    #[test]
    fn journal_state_distinguishes_a_corrupt_journal_from_an_absent_one() {
        let path = tmp_path("journal-corrupt");
        let _ = std::fs::remove_file(&path);
        assert!(
            matches!(journal_state(&path), JournalState::Absent),
            "no file at all is absent"
        );

        std::fs::write(&path, b"{ this is not a batch").unwrap();
        assert!(
            matches!(journal_state(&path), JournalState::Unreadable(_)),
            "a journal that exists but cannot be read is not an absent one"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn journal_state_reports_unfinished_and_finished() {
        let path = tmp_path("journal-states");
        write_pending_atomic(&path, &sample_plan()).unwrap();
        assert!(
            matches!(journal_state(&path), JournalState::Unfinished),
            "a plan with a pending item blocks a new batch"
        );

        let mut finished = sample_plan();
        for item in &mut finished.items {
            item.status = "ok".into();
        }
        write_pending_atomic(&path, &finished).unwrap();
        assert!(
            matches!(journal_state(&path), JournalState::Finished),
            "a fully sent plan does not block a new batch"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// Discarding reports success only when the file is really gone. An absent file
    /// is success too - clearing twice is not an error.
    #[test]
    fn clear_pending_reports_a_file_it_could_not_remove() {
        let path = tmp_path("clear-fails");
        let _ = std::fs::remove_file(&path);
        std::fs::create_dir_all(&path).unwrap();
        assert!(
            clear_pending(&path).is_err(),
            "a path that is there and was not removed is not a successful discard"
        );
        std::fs::remove_dir_all(&path).unwrap();
        assert!(clear_pending(&path).is_ok(), "already absent is success");
    }
}
