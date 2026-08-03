//! Fixture regression gates for the `wfm-scrape` binary.
//!
//! These replace the retired Python parity tests (test_scrape_parity.py /
//! test_convert_parity.py). There is no second implementation to diff against
//! anymore — the Rust pipeline is the only one — so the gates assert the
//! binary's behaviour on the SAME frozen fixtures, end to end: shell the
//! freshly-built binary (`env!("CARGO_BIN_EXE_wfm-scrape")` — cargo guarantees
//! it matches this source), run against a copy of the committed fixtures, and
//! check the output shape.
//!
//! Cargo rebuilds the binary before integration tests run, so a stale
//! artifact cannot silently green these — the same guarantee conftest.py used
//! to provide by building in the fixture.

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_wfm-scrape");
/// Root of the cargo workspace (companion/).
const MANIFEST_DIR: &str = env!("CARGO_MANIFEST_DIR");

fn repo_root() -> PathBuf {
    Path::new(MANIFEST_DIR).parent().unwrap().parent().unwrap().to_path_buf()
}

fn fixtures_dir(name: &str) -> PathBuf {
    repo_root().join("tests").join("fixtures").join(name)
}

/// Copy a fixtures subdir into a unique temp dir; returns the temp dir.
fn stage_fixtures(name: &str) -> PathBuf {
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

fn copy_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
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

fn run(args: &[&str], cwd: &Path) -> std::process::Output {
    Command::new(BIN)
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run wfm-scrape")
}

// ---- build gate: full-shape market.json from the convert fixtures ---------

#[test]
fn build_produces_full_shape_market_json() {
    let dir = stage_fixtures("convert");
    let out = run(
        &["build", "--fixtures-dir", dir.to_str().unwrap(), "--now", "2026-07-01T12:00:00Z"],
        &dir,
    );
    assert!(out.status.success(), "build failed:\n{}", String::from_utf8_lossy(&out.stderr));

    let market_path = dir.join("market.json");
    assert!(market_path.exists(), "market.json not written");
    let snap: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&market_path).unwrap()).expect("market.json is valid JSON");

    let obj = snap.as_object().expect("market.json is an object");
    for key in [
        "updated_at", "platform", "item_count", "catalog_count", "source", "catalog",
        "items", "path_to_info", "set_to_parts", "relic_rewards", "vault_status",
        "baro", "surface_fetched_at",
    ] {
        assert!(obj.contains_key(key), "missing required key {key}");
    }

    let items = obj["items"].as_object().expect("items is an object");
    assert!(!items.is_empty(), "empty items");
    assert!(items.contains_key("primed_continuity"), "missing primed_continuity");

    let priced = items.values().filter(|it| {
        it["low_sell"].as_f64().unwrap_or(0.0) > 0.0
            || it["avg"].as_f64().unwrap_or(0.0) > 0.0
            || it["median_90d"].as_f64().unwrap_or(0.0) > 0.0
    }).count();
    assert!(priced as f64 / items.len() as f64 >= 0.9, "only {priced}/{} items priced", items.len());

    let volumed = items.values().filter(|it| it["vol"].as_f64().unwrap_or(0.0) > 0.0).count();
    assert!(volumed as f64 / items.len() as f64 >= 0.9, "only {volumed}/{} items volumed", items.len());

    assert!(obj["catalog"].as_object().map(|c| !c.is_empty()).unwrap_or(false), "empty catalog");

    // The resolver catalog must also be written by `build`.
    let catalog_path = dir.join("wfstat-catalog.json");
    assert!(catalog_path.exists(), "wfstat-catalog.json not written");
    let catalog: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&catalog_path).unwrap()).expect("wfstat-catalog.json is valid JSON");
    assert!(!catalog.as_array().unwrap_or(&vec![]).is_empty(), "empty wfstat-catalog");

    let _ = std::fs::remove_dir_all(&dir);
}

// ---- scrape gate: row set + limit from the scrape fixtures -----------------

fn scrape_csv(dir: &Path, extra: &[&str]) -> String {
    let mut args = vec![
        "scrape",
        "--fixtures-dir",
        dir.to_str().unwrap(),
        "--filter",
        "",
        "--exclude",
        "",
        "--min-volume",
        "1",
    ];
    args.extend_from_slice(extra);
    let out = run(&args, dir);
    assert!(out.status.success(), "scrape failed:\n{}", String::from_utf8_lossy(&out.stderr));
    let csv_path = dir.join("wfm_results.csv");
    std::fs::read_to_string(&csv_path).expect("wfm_results.csv written")
}

fn row_urls(csv: &str) -> Vec<String> {
    let mut rows = Vec::new();
    let mut lines = csv.lines();
    let header = lines.next().expect("header row");
    let idx = header.split(',').position(|c| c == "url_name").expect("url_name column");
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        rows.push(line.split(',').nth(idx).unwrap_or("").to_string());
    }
    rows
}

#[test]
fn scrape_keeps_expected_fixture_rows() {
    let dir = stage_fixtures("scrape");
    let csv = scrape_csv(&dir, &[]);
    let urls: std::collections::BTreeSet<String> = row_urls(&csv).into_iter().collect();

    // Same survivors the Python gate asserted: retry_recover survives its
    // orders 429->429->200; missing_ninetydays survives off its 48h window;
    // stats_exhaust is dropped (stats 429 exhausts retries).
    for slug in [
        "volt_prime_barrel", "goopolla", "primed_continuity", "lith_v1_relic",
        "axi_a1_relic", "nova_prime_blueprint", "ivara_prime_set",
        "retry_recover", "missing_ninetydays",
    ] {
        assert!(urls.contains(slug), "expected {slug} in scrape output, got {urls:?}");
    }
    assert!(!urls.contains("stats_exhaust"), "stats_exhaust must be skipped");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn scrape_limit_truncates_after_filter() {
    // --limit truncates AFTER filter/exclude: the first-N in catalog order
    // survive. The output CSV is re-sorted by score, so compare as a set —
    // the parity gate this replaces asserted `set(py_rows) == first_four`.
    let dir = stage_fixtures("scrape");
    let csv = scrape_csv(&dir, &["--limit", "4"]);
    let urls: std::collections::BTreeSet<String> = row_urls(&csv).into_iter().collect();
    let first_four: std::collections::BTreeSet<String> = [
        "volt_prime_barrel", "goopolla", "primed_continuity", "lith_v1_relic",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    assert_eq!(urls.len(), 4, "expected exactly 4 rows with --limit 4, got {urls:?}");
    assert_eq!(urls, first_four, "limit truncation set drifted");

    let _ = std::fs::remove_dir_all(&dir);
}
