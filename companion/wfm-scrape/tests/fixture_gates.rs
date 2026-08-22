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

// ---- DE gate: Public Export + worldState surfaces --------------------------

/// The DE ingest is a *replacement* for two upstreams, so the gate asserts the
/// things that would silently regress: the four refinement variants collapsing
/// to one relic, rarity labels being DE's (the old source mislabels the common
/// tier), untradeable rewards staying unresolved rather than being forced to a
/// slug, and Baro's manifest arriving with prices.
#[test]
fn build_uses_de_export_and_world_state() {
    let dir = stage_fixtures("convert");
    let out = run(
        &["build", "--fixtures-dir", dir.to_str().unwrap(), "--now", "2026-07-01T12:00:00Z"],
        &dir,
    );
    assert!(out.status.success(), "build failed:\n{}", String::from_utf8_lossy(&out.stderr));
    let snap: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("market.json")).unwrap()).unwrap();

    let de = snap.get("de").expect("de surface written");
    assert_eq!(de["hashes"].as_object().unwrap().len(), 6, "all indexed manifests recorded");
    assert_eq!(de["world_ok"], true);
    assert_eq!(de["vault_rotation"].as_array().unwrap().len(), 1, "announced vault rotation");
    assert_eq!(de["deals"][0]["discount"], 40, "Darvo's daily deal");

    // One relic, not four: Bronze/Silver/Gold/Platinum are refinement variants
    // of the same relic and carry identical reward lists.
    let relics = snap["relic_rewards"].as_object().unwrap();
    assert_eq!(relics.len(), 1, "refinement variants must collapse");
    let rewards = relics["lith_v1_relic"].as_array().unwrap();
    assert_eq!(rewards.len(), 2, "Forma is untradeable and must stay unresolved");

    let chassis = rewards.iter().find(|r| r["reward_slug"] == "volt_prime_chassis_blueprint").unwrap();
    assert_eq!(chassis["rarity"], "Common", "DE's rarity, not the drop table's mislabel");
    assert_eq!(chassis["chance"], 25.33, "bare `chance` stays intact for existing consumers");
    assert_eq!(chassis["chances"]["radiant"], 16.67, "all four refinements present");
    assert_eq!(chassis["chances"]["intact"], 25.33);

    // Baro comes from worldState, so his stock is present with prices.
    let baro = &snap["baro"];
    assert_eq!(baro["location"], "Pluto Relay");
    assert_eq!(baro["character"], "Baro'Ki Teel");
    let inv = baro["inventory"].as_array().unwrap();
    assert_eq!(inv.len(), 2);
    assert_eq!(inv[0]["ducats"], 375);
    assert_eq!(inv[0]["credits"], 120000);
    assert_eq!(inv[0]["slug"], "volt_prime_chassis_blueprint");
    // A cosmetic has no market slug. It must stay readable and unpriced —
    // never given a slug it does not have, which would show a wrong price.
    assert!(inv[1].get("slug").is_none(), "cosmetics must not be assigned a slug");
    assert_eq!(inv[1]["item"], "Kiteer Sekhara");

    let _ = std::fs::remove_dir_all(&dir);
}

/// The content hash is the whole economy of this integration: a cycle where
/// nothing changed must fetch the 490-byte index and nothing else.
#[test]
fn build_skips_manifests_whose_hash_has_not_moved() {
    let dir = stage_fixtures("convert");
    let args = ["build", "--fixtures-dir", dir.to_str().unwrap(), "--now", "2026-07-01T12:00:00Z"];

    let first = run(&args, &dir);
    assert!(first.status.success());
    let first_err = String::from_utf8_lossy(&first.stderr).to_string();
    assert!(first_err.contains("6 fetched"), "cold run pulls everything:\n{first_err}");

    // Feed the snapshot we just wrote back in as the prior one.
    std::fs::copy(dir.join("market.json"), dir.join("prior-market.json")).unwrap();
    let second = run(&args, &dir);
    assert!(second.status.success());
    let second_err = String::from_utf8_lossy(&second.stderr).to_string();
    assert!(
        second_err.contains("5 skipped as unchanged") && second_err.contains("1 fetched"),
        "a warm run skips the five carryable manifests and still pulls ExportWeapons:\n{second_err}"
    );

    // And the surfaces those manifests feed must survive the skip rather than
    // blanking — that is what reconcile is for.
    let snap: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("market.json")).unwrap()).unwrap();
    assert!(
        !snap["relic_rewards"].as_object().unwrap().is_empty(),
        "a skipped manifest must not blank the surface it feeds"
    );

    // The bug this test missed the first time: non-empty is not enough. A warm
    // cycle used to fall back to the legacy intact-only scrape, quietly
    // replacing four-refinement DE data with a worse surface on EVERY run
    // after the first. The refinement ladder is the proof it is still DE's.
    let rewards = snap["relic_rewards"]["lith_v1_relic"].as_array().unwrap();
    let chassis = rewards
        .iter()
        .find(|r| r["reward_slug"] == "volt_prime_chassis_blueprint")
        .expect("the DE-derived reward survived the skip");
    assert_eq!(
        chassis["chances"]["radiant"], 16.67,
        "a skipped manifest must not downgrade the surface to the legacy source"
    );
    assert_eq!(chassis["rarity"], "Common", "DE's rarity, not the drop table's mislabel");

    // Dispositions cannot be carried — they are an override on a surface that
    // is refetched every cycle — so their manifest must be fetched every time.
    assert!(
        second_err.contains("dispositions:"),
        "ExportWeapons must be refetched even when unchanged:\n{second_err}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// A manifest we failed to read must NOT have its hash recorded.
///
/// Recording it would tell the next cycle we already hold that manifest, and
/// the failure would never be retried — a transient 500 would freeze a surface
/// until DE happened to change the file.
#[test]
fn a_failed_manifest_does_not_record_its_hash() {
    use wfm_scrape::de;
    use wfm_scrape::fetch::{FixtureHttp, Http};

    // The index names ExportRecipes, but no fixture answers for its URL.
    let index_text = "ExportRecipes_en.json!00_abc\n";
    let compressed = {
        // Round-trip through the same decoder the pipeline uses, so this test
        // cannot pass against a bad stream.
        let raw = std::process::Command::new("python3")
            .args([
                "-c",
                "import lzma,base64,sys; sys.stdout.write(base64.b64encode(lzma.compress(b'ExportRecipes_en.json!00_abc\\n', format=lzma.FORMAT_ALONE)).decode())",
            ])
            .output()
            .expect("python3 for the fixture");
        String::from_utf8(raw.stdout).unwrap()
    };
    assert!(de::decode_lzma_alone(
        &{
            use base64::Engine as _;
            base64::engine::general_purpose::STANDARD.decode(&compressed).unwrap()
        }
    )
    .unwrap()
    .contains("ExportRecipes"));

    let mut responses = std::collections::HashMap::new();
    responses.insert(
        de::DE_INDEX_URL.to_string(),
        serde_json::Value::String(format!("base64:{compressed}")),
    );
    let http = FixtureHttp { responses };
    let _ = index_text;

    let snap = de::fetch_export(&http, &std::collections::BTreeMap::new());
    assert!(snap.index_ok, "the index itself parsed");
    assert!(
        !snap.hashes.contains_key("ExportRecipes_en.json"),
        "a manifest we could not read must stay un-held so the next cycle retries"
    );
    assert!(!snap.skipped("ExportRecipes_en.json"), "a failure is not a skip");
    let _ = http.get_text(de::DE_INDEX_URL);
}

/// Live check against DE, opt-in.
///
/// The fixture gates prove our parsing; this proves the *contract* — that the
/// index is still LZMA-alone at that URL, that the hash is still part of the
/// manifest path, and that worldState still answers where it moved to. Five
/// of the endpoints the community wiki documents are already dead, so this is
/// the test that tells us when ours joins them.
///
/// Ignored by default: it hits the network and must never gate CI or a
/// release. Run it deliberately:
///   cargo test --package wfm-scrape -- --ignored de_endpoints_are_still_alive
#[test]
#[ignore = "hits Digital Extremes' live endpoints"]
fn de_endpoints_are_still_alive() {
    use wfm_scrape::de;
    use wfm_scrape::fetch::{Http, LiveHttp};

    let client = reqwest::blocking::Client::builder()
        .user_agent(wfm_client::user_agent("wfm-scrape", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("build client");
    let http = LiveHttp { client };

    let raw = http.get_bytes(de::DE_INDEX_URL).expect("index fetch");
    let text = de::decode_lzma_alone(&raw).expect("index is still LZMA-alone");
    let index = de::parse_index(&text);
    assert!(index.len() >= 10, "index shrank unexpectedly: {} entries", index.len());

    for name in de::WANTED_MANIFESTS {
        assert!(index.contains_key(*name), "{name} vanished from DE's index");
    }

    // One manifest, hash and all — the bare filename 404s, so this also proves
    // the hashed path still works.
    let entry = &index["ExportWeapons_en.json"];
    let body = http.get_text(&entry.url()).expect("manifest fetch");
    let doc = de::parse_manifest(&body).expect("manifest parse");
    let rows = de::manifest_rows_for(&doc, "ExportWeapons_en.json");
    // Guards the two-array trap: ExportWeapons_en.json also carries
    // ExportRailjackWeapons (~143 rows), which sorts first.
    assert!(rows.len() > 500, "only {} weapons — wrong array?", rows.len());
    assert!(
        rows.iter().any(|r| r.get("omegaAttenuation").and_then(|v| v.as_f64()).is_some_and(|d| d > 0.0)),
        "no dispositions in ExportWeapons — the field was renamed"
    );

    let world = de::fetch_world_state(&http).expect("worldState fetch");
    assert!(world.get("VoidTraders").is_some(), "worldState lost VoidTraders");
}
