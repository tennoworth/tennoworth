//! Contributor ids for price reports cost nothing, so one person can pose as
//! the five a published median needs (docs/price-reports.md). Until that is
//! solved the medians may be shown, never scored: no client code reads them.
//! A display-only consumer must be added to `DISPLAY_ONLY` deliberately.

#![cfg(test)]

use std::path::{Path, PathBuf};

const DISPLAY_ONLY: &[&str] = &[];

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if path.is_dir() {
            if !matches!(name.as_str(), "tests" | "node_modules" | "dev") {
                sources(&path, out);
            }
        } else if (name.ends_with(".rs") || name.ends_with(".ts") || name.ends_with(".svelte"))
            && !name.ends_with(".test.ts")
            && name != "tests.rs"
        {
            out.push(path);
        }
    }
}

#[test]
fn no_client_reads_the_published_report_prices() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    for root in [
        "rust/tennoworth-desktop/src",
        "rust/wfm-scrape/src",
        "rust/game-acquisition/src",
        "rust/wfm-trading/src",
        "rust/wfm-client/src",
        "frontend/src",
    ] {
        sources(&repo.join(root), &mut files);
    }
    assert!(files.len() > 100, "the scan found {} files", files.len());
    let readers: Vec<_> = files
        .iter()
        .filter(|path| {
            let text = std::fs::read_to_string(path).unwrap();
            text.contains("reports/prices")
                || (text.contains("/api/reports") && text.contains("/prices"))
        })
        .map(|path| {
            path.strip_prefix(&repo)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        })
        .filter(|path| !DISPLAY_ONLY.contains(&path.as_str()))
        .collect();
    assert!(
        readers.is_empty(),
        "report prices must not feed advice: {readers:?}"
    );
}
