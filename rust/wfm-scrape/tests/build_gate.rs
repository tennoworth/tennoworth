//! Build gate: the full-shape `market.json` the `build` stage renders from the
//! frozen build fixtures.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "integration-test helpers may panic to fail the test and are never shipped"
)]

mod common;
use common::*;

// ---- build gate: full-shape market.json from the convert fixtures ---------

#[test]
fn build_produces_full_shape_market_json() {
    let dir = stage_fixtures("build");
    let out = run(
        &[
            "build",
            "--fixtures-dir",
            dir.to_str().unwrap(),
            "--now",
            "2026-07-01T12:00:00Z",
        ],
        &dir,
    );
    assert!(
        out.status.success(),
        "build failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let market_path = dir.join("market.json");
    assert!(market_path.exists(), "market.json not written");
    let snap: serde_json::Value = serde_json::from_slice(&std::fs::read(&market_path).unwrap())
        .expect("market.json is valid JSON");

    // The site's prerendered pages refresh from the slim copy written beside it.
    let public: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("market-public.json")).expect("market-public.json written"))
            .expect("market-public.json is valid JSON");
    assert_eq!(public["updated_at"], snap["updated_at"]);
    let home: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("market-home.json")).expect("market-home.json written"))
            .expect("market-home.json is valid JSON");
    assert_eq!(home["items"], snap["items"]);
    assert!(home.get("path_to_info").is_none());
    assert_eq!(
        public["items"].as_object().unwrap().keys().collect::<Vec<_>>(),
        snap["items"].as_object().unwrap().keys().collect::<Vec<_>>()
    );

    let obj = snap.as_object().expect("market.json is an object");
    for key in [
        "updated_at",
        "platform",
        "item_count",
        "catalog_count",
        "source",
        "catalog",
        "items",
        "path_to_info",
        "set_to_parts",
        "relic_rewards",
        "vault_status",
        "baro",
        "surface_fetched_at",
    ] {
        assert!(obj.contains_key(key), "missing required key {key}");
    }

    let items = obj["items"].as_object().expect("items is an object");
    assert!(!items.is_empty(), "empty items");
    assert!(
        items.contains_key("primed_continuity"),
        "missing primed_continuity"
    );

    let priced = items
        .values()
        .filter(|it| {
            it["low_sell"].as_f64().unwrap_or(0.0) > 0.0
                || it["avg"].as_f64().unwrap_or(0.0) > 0.0
                || it["median_90d"].as_f64().unwrap_or(0.0) > 0.0
        })
        .count();
    assert!(
        priced as f64 / items.len() as f64 >= 0.9,
        "only {priced}/{} items priced",
        items.len()
    );

    let volumed = items
        .values()
        .filter(|it| it["vol"].as_f64().unwrap_or(0.0) > 0.0)
        .count();
    assert!(
        volumed as f64 / items.len() as f64 >= 0.9,
        "only {volumed}/{} items volumed",
        items.len()
    );

    assert!(
        obj["catalog"]
            .as_object()
            .map(|c| !c.is_empty())
            .unwrap_or(false),
        "empty catalog"
    );

    // The resolver catalog must also be written by `build`.
    let catalog_path = dir.join("wfstat-catalog.json");
    assert!(catalog_path.exists(), "wfstat-catalog.json not written");
    let catalog: serde_json::Value = serde_json::from_slice(&std::fs::read(&catalog_path).unwrap())
        .expect("wfstat-catalog.json is valid JSON");
    assert!(
        !catalog.as_array().unwrap_or(&vec![]).is_empty(),
        "empty wfstat-catalog"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

// ---- box layout: input and output outside any checkout ---------------------

/// The box runs `build` from its data directory with no checkout above it.
/// The flags alone must say where the CSV is read and where the lock and the
/// WFM state live; falling back to a checkout would write into it again.
#[test]
fn build_reads_and_locks_the_named_data_dir_outside_a_checkout() {
    let root = std::env::temp_dir().join(format!("wfm-scrape-dirs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (data, publish) = (root.join("scrape"), root.join("public"));
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&publish).unwrap();
    let out = run(
        &["build", "--data-dir", data.to_str().unwrap(), "--publish-dir", publish.to_str().unwrap()],
        &root,
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success());
    assert!(
        stderr.contains(&format!("{} not found", data.join("wfm_results.csv").display())),
        "{stderr}"
    );
    assert!(data.join(".wfm-output.lock").exists(), "the lock belongs in the data dir");
    assert!(!root.join(".wfm-output.lock").exists());
    std::fs::remove_dir_all(&root).unwrap();
}

