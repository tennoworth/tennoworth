//! `stamp` gate: deploy/bin/host-check.sh reads the published snapshot's age
//! from this subcommand's output and exit status alone.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "integration-test helpers may panic to fail the test and are never shipped"
)]

mod common;
use common::*;

#[test]
fn stamp_prints_unix_seconds_or_fails() {
    let dir = std::env::temp_dir().join(format!("wfm-scrape-stamp-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let snapshot = dir.join("market.json");

    std::fs::write(
        &snapshot,
        r#"{"updated_at":"2026-10-13T10:00:00Z","items":{"primed_flow":{"avg":10}}}"#,
    )
    .unwrap();
    let out = run(&["stamp", snapshot.to_str().unwrap()], &dir);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1791885600\n");

    std::fs::write(&snapshot, r#"{"updated_at":"yesterday","items":{"a":{}}}"#).unwrap();
    let out = run(&["stamp", snapshot.to_str().unwrap()], &dir);
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());

    std::fs::remove_file(&snapshot).unwrap();
    assert!(!run(&["stamp", snapshot.to_str().unwrap()], &dir)
        .status
        .success());
    assert!(!run(&["stamp"], &dir).status.success());

    std::fs::remove_dir_all(&dir).unwrap();
}
