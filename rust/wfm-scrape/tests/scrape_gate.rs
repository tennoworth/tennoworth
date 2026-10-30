//! Scrape gate: the row set, limits, sweep metrics and failure behaviour of
//! `scrape` on the frozen scrape fixtures.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "integration-test helpers may panic to fail the test and are never shipped"
)]

mod common;
use common::*;

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
    assert!(
        out.status.success(),
        "scrape failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let csv_path = dir.join("wfm_results.csv");
    std::fs::read_to_string(&csv_path).expect("wfm_results.csv written")
}

/// A scrape that keeps nothing must fail and leave the previous generation's
/// CSV alone. `run-scrape.sh` counts whatever CSV is on disk and rebuilds from
/// it, so a success exit that never replaced the file republishes stale rows
/// under a fresh timestamp.
#[test]
fn zero_result_scrape_fails_and_leaves_the_previous_snapshot() {
    let dir = stage_fixtures("scrape");
    // One catalog item whose 48h volume is zero: the loop runs to completion
    // and keeps nothing - the quiet path that used to exit 0. The shared
    // fixture cannot exercise this, because its deliberate retry/abort cases
    // fail the run for unrelated reasons first.
    let responses = serde_json::json!({
        "https://api.warframe.market/v2/items": {"data": [
            {"slug": "zero_volume_item", "i18n": {"en": {"name": "Zero Volume Item"}}, "tags": ["mod"]}
        ]},
        "https://api.warframe.market/v1/items/zero_volume_item/statistics": {"payload": {"statistics_closed": {
            "48hours": [{"median": 10, "volume": 0}],
            "90days": [{"median": 10, "volume": 0}]
        }}}
    });
    std::fs::write(
        dir.join("fixture_responses.json"),
        serde_json::to_vec(&responses).unwrap(),
    )
    .unwrap();

    let csv_path = dir.join("wfm_results.csv");
    let previous = "url_name,median_90d\nsentinel,1\n";
    std::fs::write(&csv_path, previous).unwrap();

    let out = run(
        &[
            "scrape",
            "--fixtures-dir",
            dir.to_str().unwrap(),
            "--filter",
            "",
            "--exclude",
            "",
            "--min-volume",
            "1",
        ],
        &dir,
    );

    assert!(
        !out.status.success(),
        "a zero-result scrape must not report success:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(&csv_path).unwrap(),
        previous,
        "the previous generation's CSV must be left untouched"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

fn metric(line: &str, key: &str) -> u64 {
    line.split(key)
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| panic!("{key} not a number in: {line}"))
}

/// Read one class's counter out of a bracketed block (`failed[orders=1 ...]`),
/// so an assertion does not depend on the order of the classes.
fn class_metric(line: &str, block: &str, class: &str) -> u64 {
    let inner = line
        .split(&format!("{block}["))
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .unwrap_or_else(|| panic!("no {block}[...] block in: {line}"));
    metric(&format!("{inner} "), &format!("{class}="))
}

/// A sweep must report what it spent even when it fails: the counters are the
/// only evidence of our request footprint, and a failed sweep is exactly the
/// run that gets repeated.
#[test]
fn failed_sweep_still_reports_request_metrics() {
    let dir = stage_fixtures("scrape");
    let out = run(
        &[
            "scrape",
            "--fixtures-dir",
            dir.to_str().unwrap(),
            "--filter",
            "",
            "--exclude",
            "",
            "--min-volume",
            "1",
        ],
        &dir,
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success(),
        "the shared fixture aborts by design:\n{stderr}"
    );
    let line = stderr
        .lines()
        .find(|line| line.contains("sweep metrics:"))
        .unwrap_or_else(|| panic!("a failed sweep must still report metrics:\n{stderr}"));

    assert!(
        line.contains("attempts[catalog=1"),
        "the catalog fetch must be counted: {line}"
    );
    assert!(
        line.contains("statistics=") && line.contains("orders="),
        "every WFM class must be counted: {line}"
    );
    assert!(
        metric(line, "decoded_bytes=") > 0,
        "fixture bodies are counted: {line}"
    );
    assert!(
        class_metric(line, "failed", "orders") == 1,
        "the aborting request must be counted as failed: {line}"
    );
    assert_eq!(
        metric(line, "cooldown_waits="),
        0,
        "a throttle with no recorded deadline must not spend patience: {line}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Attempts, retries and bytes must be counted per class. A 5xx is retried at
/// this layer (a 429 is not - the transport already spent its retry budget and
/// the scrape stops), so a scripted 503/503/200 pins both numbers.
#[test]
fn sweep_metrics_count_attempts_retries_and_bytes() {
    let dir = stage_fixtures("scrape");
    let responses = serde_json::json!({
        "https://api.warframe.market/v2/items": {"data": [
            {"slug": "retry_item", "i18n": {"en": {"name": "Retry Item"}}, "tags": ["mod"]}
        ]},
        "https://api.warframe.market/v1/items/retry_item/statistics": [
            {"status": 503},
            {"status": 503},
            {"payload": {"statistics_closed": {
                "48hours": [{"median": 10, "volume": 5}],
                "90days": [{"median": 10, "volume": 5}]
            }}}
        ],
        "https://api.warframe.market/v2/orders/item/retry_item": {"data": [
            {"type": "sell", "platinum": 12, "visible": true, "user": {"status": "ingame"}}
        ]}
    });
    std::fs::write(
        dir.join("fixture_responses.json"),
        serde_json::to_vec(&responses).unwrap(),
    )
    .unwrap();

    let out = run(
        &[
            "scrape",
            "--fixtures-dir",
            dir.to_str().unwrap(),
            "--filter",
            "",
            "--exclude",
            "",
            "--min-volume",
            "1",
        ],
        &dir,
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "scrape failed:\n{stderr}");
    let line = stderr
        .lines()
        .find(|line| line.contains("sweep metrics:"))
        .unwrap_or_else(|| panic!("a successful sweep must report metrics:\n{stderr}"));

    assert!(
        line.contains("attempts[catalog=1 statistics=3 orders=1"),
        "one catalog call, three statistics attempts, one orders call: {line}"
    );
    assert!(
        line.contains("ok[catalog=1 statistics=1 orders=1"),
        "each class succeeded once: {line}"
    );
    assert_eq!(metric(line, "retries="), 2, "two 503s are retried: {line}");
    assert!(
        metric(line, "decoded_bytes=") > 0,
        "the payload we parsed is counted: {line}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

fn row_urls(csv: &str) -> Vec<String> {
    let mut rows = Vec::new();
    let mut lines = csv.lines();
    let header = lines.next().expect("header row");
    let idx = header
        .split(',')
        .position(|c| c == "url_name")
        .expect("url_name column");
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
    let responses_path = dir.join("fixture_responses.json");
    let mut responses: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&responses_path).unwrap()).unwrap();
    for outcome in responses["https://api.warframe.market/v2/orders/item/retry_recover"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .take(2)
    {
        outcome["status"] = serde_json::json!(503);
    }
    responses["https://api.warframe.market/v1/items/stats_exhaust/statistics"] =
        serde_json::json!({"payload": {"statistics_closed": {"48hours": [], "90days": []}}});
    std::fs::write(responses_path, serde_json::to_vec(&responses).unwrap()).unwrap();
    let csv = scrape_csv(&dir, &[]);
    let urls: std::collections::BTreeSet<String> = row_urls(&csv).into_iter().collect();

    for slug in [
        "volt_prime_barrel",
        "goopolla",
        "primed_continuity",
        "lith_v1_relic",
        "axi_a1_relic",
        "nova_prime_blueprint",
        "ivara_prime_set",
        "retry_recover",
        "missing_ninetydays",
    ] {
        assert!(
            urls.contains(slug),
            "expected {slug} in scrape output, got {urls:?}"
        );
    }
    assert!(
        !urls.contains("stats_exhaust"),
        "stats_exhaust must be skipped"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn scrape_limit_truncates_after_filter() {
    // --limit truncates AFTER filter/exclude: the first-N in catalog order
    // survive. The output CSV is re-sorted by score, so compare as a set -
    // the parity gate this replaces asserted `set(py_rows) == first_four`.
    let dir = stage_fixtures("scrape");
    let csv = scrape_csv(&dir, &["--limit", "4"]);
    let urls: std::collections::BTreeSet<String> = row_urls(&csv).into_iter().collect();
    let first_four: std::collections::BTreeSet<String> = [
        "volt_prime_barrel",
        "goopolla",
        "primed_continuity",
        "lith_v1_relic",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    assert_eq!(
        urls.len(),
        4,
        "expected exactly 4 rows with --limit 4, got {urls:?}"
    );
    assert_eq!(urls, first_four, "limit truncation set drifted");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn throttled_scrape_preserves_the_published_csv() {
    let dir = stage_fixtures("scrape");
    let csv = dir.join("wfm_results.csv");
    std::fs::write(&csv, "previous snapshot").unwrap();
    let out = run(
        &[
            "scrape",
            "--fixtures-dir",
            dir.to_str().unwrap(),
            "--filter",
            "",
            "--exclude",
            "",
            "--min-volume",
            "1",
        ],
        &dir,
    );
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("publication aborted"));
    assert_eq!(std::fs::read_to_string(csv).unwrap(), "previous snapshot");
    std::fs::remove_dir_all(dir).unwrap();
}
