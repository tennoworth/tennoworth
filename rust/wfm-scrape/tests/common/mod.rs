//! Helpers shared by the pipeline's fixture gates.
//!
//! There is no second implementation to diff against, so the gates assert the
//! binary's behaviour on the frozen fixtures, end to end: shell the
//! freshly-built binary (`env!("CARGO_BIN_EXE_wfm-scrape")` - Cargo rebuilds it
//! before integration tests run, so a stale artifact cannot green them), run it
//! against a copy of the committed fixtures, and check the output.
#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "each gate binary uses a subset of these test-only helpers"
)]

pub use std::path::{Path, PathBuf};
pub use std::process::Command;
pub const BIN: &str = env!("CARGO_BIN_EXE_wfm-scrape");
/// Root of the cargo workspace (rust/).
pub const MANIFEST_DIR: &str = env!("CARGO_MANIFEST_DIR");

pub fn repo_root() -> PathBuf {
    Path::new(MANIFEST_DIR)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

pub fn fixtures_dir(name: &str) -> PathBuf {
    repo_root().join("tests").join("fixtures").join(name)
}

/// Copy a fixtures subdir into a unique temp dir; returns the temp dir.
pub fn stage_fixtures(name: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let src = fixtures_dir(name);
    let dst = std::env::temp_dir().join(format!(
        "wfm-scrape-gate-{}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst),
        name.replace('/', "-")
    ));
    let _ = std::fs::remove_dir_all(&dst);
    copy_dir(&src, &dst).expect("copy fixture tree");
    dst
}

pub fn copy_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

pub fn run(args: &[&str], cwd: &Path) -> std::process::Output {
    Command::new(BIN)
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run wfm-scrape")
}

