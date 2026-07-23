//! Pending-plan persistence — crash-recovery for a listing batch.
//!
//! Every plan is written to `~/.config/wfminv/pending_plan.json` before the
//! first POST, rewritten after each item, and deleted on clean completion. The
//! browser polls it on (re)connect and offers Resume / Discard. Writes are
//! atomic (tmp + rename) so a concurrent read never sees a torn file — same
//! convention as `os.replace` in wfm_demand.py — and the tmp file is created
//! at 0600 (it holds unsubmitted listing details).

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

#[derive(Serialize, Deserialize, Clone)]
pub struct PendingItem {
    pub slug: String,
    pub platinum: u32,
    pub quantity: u32,
    pub order_type: String,
    pub visible: bool,
    pub rank: Option<u32>,
    #[serde(default)]
    pub subtype: Option<String>,
    #[serde(default)]
    pub reference_low_sell: Option<u32>,
    /// "pending" | "ok" | "error"
    pub status: String,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub order_id: Option<String>,
    /// "created" | "updated" once terminal — how the ok state was reached.
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
    // Create the tmp file at 0o600 from the first syscall — pending plans
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

pub fn load_pending(path: &Path) -> Option<PendingPlan> {
    let data = fs::read(path).ok()?;
    serde_json::from_slice(&data).ok()
}

pub fn clear_pending(path: &Path) {
    let _ = fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::path::PathBuf;

    fn tmp_path(name: &str) -> PathBuf {
        let mut p = env::temp_dir();
        p.push(format!("wfmcore-pending-{}-{}.json", std::process::id(), name));
        p
    }

    fn sample_plan() -> PendingPlan {
        PendingPlan {
            plan_id: "abc12345".into(),
            started_at: "2026-05-27T15:30:00Z".into(),
            items: vec![
                PendingItem {
                    slug: "loki_prime_set".into(),
                    platinum: 120,
                    quantity: 1,
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
                    slug: "rhino_prime_set".into(),
                    platinum: 95,
                    quantity: 1,
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
    fn pending_plan_roundtrips_through_disk() {
        let path = tmp_path("roundtrip");
        let plan = sample_plan();
        write_pending_atomic(&path, &plan).unwrap();

        let loaded = load_pending(&path).expect("file readable");
        assert_eq!(loaded.plan_id, plan.plan_id);
        assert_eq!(loaded.items.len(), 2);
        assert_eq!(loaded.items[0].status, "ok");
        assert_eq!(loaded.items[1].status, "pending");

        clear_pending(&path);
        assert!(load_pending(&path).is_none());
    }

    #[test]
    fn load_pending_returns_none_when_missing() {
        let path = tmp_path("missing");
        let _ = std::fs::remove_file(&path);
        assert!(load_pending(&path).is_none());
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
        let loaded = load_pending(&path).expect("parses");
        assert_eq!(loaded.items.len(), 1);
        assert!(loaded.items[0].order_id.is_none());
        assert!(loaded.items[0].reference_low_sell.is_none());
        clear_pending(&path);
    }

    #[test]
    fn atomic_write_leaves_no_tmp_file() {
        let path = tmp_path("notmp");
        write_pending_atomic(&path, &sample_plan()).unwrap();
        let tmp = path.with_extension("json.tmp");
        assert!(!tmp.exists(), "tmp file should be renamed away");
        assert!(path.exists());
        clear_pending(&path);
    }
}
