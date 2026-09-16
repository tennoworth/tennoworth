//! Fixture gates for `wfm-scrape replay`.
//!
//! The replay reads the committed observation logs under
//! `tests/fixtures/replay/logs` and must reproduce the committed
//! `tests/fixtures/replay/report.json` exactly, so a change to the report shape
//! or to the simulation fails here instead of silently redefining the evidence.
//! Two runs over the same logs must also be byte-identical.
//!
//! `env!("CARGO_BIN_EXE_wfm-scrape")` is rebuilt by cargo before these run, so a
//! stale binary cannot green them.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "integration-test helpers may panic to fail the test and are never shipped"
)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_wfm-scrape");
const MANIFEST_DIR: &str = env!("CARGO_MANIFEST_DIR");

fn repo_root() -> PathBuf {
    Path::new(MANIFEST_DIR)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn fixtures_dir() -> PathBuf {
    repo_root().join("tests").join("fixtures").join("replay")
}

fn temp_dir(name: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "wfm-replay-gate-{}-{}-{}",
        std::process::id(),
        name,
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Run replay over the fixture logs; returns the report text and stdout.
fn run_replay(out: &Path) -> (String, String) {
    let output = Command::new(BIN)
        .arg("replay")
        .arg("--observations")
        .arg(fixtures_dir().join("logs"))
        .arg("--schedule")
        .arg("1h,2h,4h")
        .arg("--out")
        .arg(out)
        .output()
        .expect("run wfm-scrape replay");
    assert!(
        output.status.success(),
        "replay failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    (
        std::fs::read_to_string(out).expect("report written"),
        String::from_utf8_lossy(&output.stdout).to_string(),
    )
}

#[test]
fn replay_simulates_the_schedule_and_writes_a_report() {
    let dir = temp_dir("basic");
    let (text, stdout) = run_replay(&dir.join("report.json"));
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();

    assert_eq!(value["format"], 1);
    assert_eq!(value["logs_read"], 3);
    assert!(stdout.contains("replay:"), "stdout summary missing: {stdout}");

    let intervals = value["intervals"].as_array().unwrap();
    assert_eq!(intervals.len(), 3);
    // 1h and 2h are at or below the 2h sweep cadence, so they are the identity.
    assert_eq!(intervals[0]["identity"], true);
    assert_eq!(intervals[1]["identity"], true);
    assert_eq!(intervals[2]["identity"], false);
    assert!(intervals[2]["calls"]["saving_requests"].as_i64().unwrap() > 0);
    assert_eq!(intervals[2]["calls"]["fresh_total_requests"], 34);
    assert_eq!(intervals[2]["false_exclusions"]["count"], 1);
    // Unknown is counted, never folded into a zero.
    assert!(intervals[2]["unknown_books"]["unknown"].as_u64().unwrap() > 0);
    assert!(intervals[2]["unknown_books"]["coverage"].as_f64().unwrap() < 1.0);
    assert!(intervals[2]["unknown_books"]["unknown_subtype_change"].as_u64().unwrap() > 0);
    // A stale-but-known candidate carries a real price error in the fixture.
    assert!(intervals[2]["price_error"]["p95_absolute"].as_f64().unwrap() > 0.0);
    assert!(!intervals[2]["strata"]["book_depth"].as_array().unwrap().is_empty());
    assert!(intervals[2]["diagnostics"]["oracle_extra_refreshes"].as_u64().unwrap() > 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_report_matches_the_golden_fixture() {
    let dir = temp_dir("golden");
    let (text, _) = run_replay(&dir.join("report.json"));
    let produced: serde_json::Value = serde_json::from_str(&text).unwrap();
    let golden_text = std::fs::read_to_string(fixtures_dir().join("report.json"))
        .expect("the golden report fixture exists");
    let golden: serde_json::Value = serde_json::from_str(&golden_text).unwrap();
    assert_eq!(
        produced, golden,
        "replay report drifted from tests/fixtures/replay/report.json"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn two_runs_over_the_same_logs_are_byte_identical() {
    let dir = temp_dir("determinism");
    let (first, _) = run_replay(&dir.join("a.json"));
    let (second, _) = run_replay(&dir.join("b.json"));
    assert_eq!(first, second, "the report is not deterministic");
    let _ = std::fs::remove_dir_all(&dir);
}
