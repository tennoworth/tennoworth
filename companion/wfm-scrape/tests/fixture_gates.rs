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
    assert_eq!(de["dispositions"]["volt_prime"], 1.15, "only DE-matched dispositions are provenance");
    assert_eq!(de["child_fetched_at"]["world_state"], "2026-07-01T12:00:00Z");
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
        second_err.contains("4 skipped as unchanged") && second_err.contains("2 fetched"),
        "a warm run skips the four carryable manifests and still pulls the two \
         whose data cannot be carried:\n{second_err}"
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

    // Dispositions and ducats cannot be carried — both are overrides on
    // surfaces that are refetched every cycle — so their manifests must be
    // pulled every time.
    assert!(
        second_err.contains("dispositions:"),
        "ExportWeapons must be refetched even when unchanged:\n{second_err}"
    );
    assert!(
        second_err.contains("ducats:"),
        "ExportRecipes must be refetched even when unchanged:\n{second_err}"
    );

    // The assertion the first warm-cycle test was missing. The fixture makes
    // the two sources disagree on purpose: warframe.market says a Volt Prime
    // Chassis Blueprint is 45 ducats, DE's recipe says 65. If the warm cycle
    // ever reverts to 45, the DE override was silently lost.
    assert_eq!(
        snap["items"]["volt_prime_chassis_blueprint"]["ducats"], 65,
        "a warm cycle must keep DE's ducat value, not revert to WFM's"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// A failed ExportRecipes fetch must not undo either DE override.
///
/// `ALWAYS_FETCH` guarantees the manifest is ATTEMPTED, not that it arrives —
/// the index can answer while the manifest itself 500s. In that state the
/// ducat override has no fresh source and the relic table cannot be rebuilt,
/// and the naive handling of both is a silent downgrade: ducats revert to
/// warframe.market's values, and the legacy relic scrape returns a NON-EMPTY
/// intact-only table, which is precisely what stops reconcile from preserving
/// the good one.
#[test]
fn a_failed_recipes_manifest_preserves_both_de_overrides() {
    let dir = stage_fixtures("convert");
    let args = ["build", "--fixtures-dir", dir.to_str().unwrap(), "--now", "2026-07-01T12:00:00Z"];

    // A good cycle first, so there is a prior snapshot holding DE's values.
    assert!(run(&args, &dir).status.success());
    std::fs::copy(dir.join("market.json"), dir.join("prior-market.json")).unwrap();

    let good: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("market.json")).unwrap()).unwrap();
    assert_eq!(good["items"]["volt_prime_chassis_blueprint"]["ducats"], 65);

    // Now make ONLY the recipes manifest unavailable, leaving the index intact.
    let resp_path = dir.join("fixture_responses.json");
    let mut responses: serde_json::Map<String, serde_json::Value> =
        serde_json::from_slice(&std::fs::read(&resp_path).unwrap()).unwrap();
    let recipes_key = responses
        .keys()
        .find(|k| k.contains("ExportRecipes_en.json"))
        .cloned()
        .expect("the fixture serves a recipes manifest");
    responses.remove(&recipes_key);
    std::fs::write(&resp_path, serde_json::to_vec(&responses).unwrap()).unwrap();

    let out = run(&args, &dir);
    assert!(out.status.success(), "a failed manifest must not fail the build");
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(
        stderr.contains("ducats: no DE values this cycle"),
        "the failure should be reported, not swallowed:\n{stderr}"
    );

    let snap: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("market.json")).unwrap()).unwrap();

    // WFM says 45 for this item; DE said 65. A failed fetch must keep 65.
    assert_eq!(
        snap["items"]["volt_prime_chassis_blueprint"]["ducats"], 65,
        "a failed recipes fetch must not revert ducats to WFM's value"
    );

    // And the relic table must still be DE's four-refinement one, not the
    // legacy intact-only scrape.
    let rewards = snap["relic_rewards"]["lith_v1_relic"].as_array().unwrap();
    let chassis = rewards
        .iter()
        .find(|r| r["reward_slug"] == "volt_prime_chassis_blueprint")
        .expect("the DE-derived reward survived");
    assert_eq!(
        chassis["chances"]["radiant"], 16.67,
        "a failed recipes fetch must not downgrade relics to the legacy source"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// A recipes failure on a COLD build must still publish a relic table.
///
/// The carry-on-failure rule has a hole at the start of time: reconcile can
/// only preserve something that already exists, so emitting empty with no
/// prior snapshot behind it publishes no relic data at all — strictly worse
/// than the legacy intact-only table the fallback was keeping in reserve. The
/// other failure test always seeds a good prior, so it cannot see this.
#[test]
fn a_failed_recipes_manifest_on_a_cold_build_falls_back_rather_than_publishing_nothing() {
    let dir = stage_fixtures("convert");

    // No prior-market.json at all, and the recipes manifest unavailable.
    let _ = std::fs::remove_file(dir.join("prior-market.json"));
    let resp_path = dir.join("fixture_responses.json");
    let mut responses: serde_json::Map<String, serde_json::Value> =
        serde_json::from_slice(&std::fs::read(&resp_path).unwrap()).unwrap();
    let recipes_key = responses
        .keys()
        .find(|k| k.contains("ExportRecipes_en.json"))
        .cloned()
        .expect("the fixture serves a recipes manifest");
    responses.remove(&recipes_key);
    std::fs::write(&resp_path, serde_json::to_vec(&responses).unwrap()).unwrap();

    let out = run(
        &["build", "--fixtures-dir", dir.to_str().unwrap(), "--now", "2026-07-01T12:00:00Z"],
        &dir,
    );
    assert!(out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(
        stderr.contains("fallback — no DE data available"),
        "a cold build with a failed manifest must reach for the fallback:\n{stderr}"
    );

    let snap: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("market.json")).unwrap()).unwrap();
    assert!(
        !snap["relic_rewards"].as_object().unwrap().is_empty(),
        "some relic data beats none when there is nothing to carry"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// The ducat carry must touch only the values DE set.
///
/// Copying every prior ducat would stamp a stale number over a fresh,
/// legitimately-corrected warframe.market one — a subtler bug than the
/// revert it was written to prevent.
#[test]
fn the_ducat_carry_leaves_wfm_sourced_values_alone() {
    let dir = stage_fixtures("convert");
    let args = ["build", "--fixtures-dir", dir.to_str().unwrap(), "--now", "2026-07-01T12:00:00Z"];

    assert!(run(&args, &dir).status.success());
    let good: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("market.json")).unwrap()).unwrap();
    // DE set this one; provenance records it.
    assert_eq!(good["de"]["ducats"]["volt_prime_chassis_blueprint"], 65);
    // This one DE never touched — no recipe produces it — so it must be absent
    // from provenance even though it carries a WFM ducat value.
    assert!(
        good["de"]["ducats"].get("primed_continuity").is_none(),
        "provenance must list only what DE set"
    );

    // Seed the prior, then move WFM's value for the untouched item and make
    // the recipes manifest fail.
    std::fs::write(
        dir.join("prior-market.json"),
        serde_json::to_vec(&good).unwrap(),
    )
    .unwrap();

    let resp_path = dir.join("fixture_responses.json");
    let mut responses: serde_json::Map<String, serde_json::Value> =
        serde_json::from_slice(&std::fs::read(&resp_path).unwrap()).unwrap();
    let recipes_key = responses.keys().find(|k| k.contains("ExportRecipes_en.json")).cloned().unwrap();
    responses.remove(&recipes_key);
    let items = responses
        .get_mut("https://api.warframe.market/v2/items")
        .and_then(|v| v.get_mut("data"))
        .and_then(|v| v.as_array_mut())
        .unwrap();
    for it in items.iter_mut() {
        if it["slug"] == "primed_continuity" {
            it["ducats"] = serde_json::json!(7);
        }
    }
    std::fs::write(&resp_path, serde_json::to_vec(&responses).unwrap()).unwrap();

    assert!(run(&args, &dir).status.success());
    let snap: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("market.json")).unwrap()).unwrap();

    assert_eq!(
        snap["items"]["volt_prime_chassis_blueprint"]["ducats"], 65,
        "DE's value is carried through the failure"
    );
    assert_eq!(
        snap["items"]["primed_continuity"]["ducats"], 7,
        "a fresh WFM correction must survive the carry, not be overwritten by the prior"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Manifests that parse but yield nothing must not publish an empty table.
///
/// The case the previous three fixes all missed: DE's manifests can arrive
/// intact and still resolve no usable rows — every reward path pointing at
/// something the catalogue does not carry. "Did the manifest arrive" is not
/// "did it produce rows", and treating them as the same published an empty
/// relic surface on a cold build while the legacy table sat unused.
#[test]
fn de_manifests_that_yield_no_rows_fall_back_rather_than_publishing_empty() {
    let dir = stage_fixtures("convert");
    let _ = std::fs::remove_file(dir.join("prior-market.json"));

    // A well-formed relic manifest whose every reward points at an item the
    // catalogue has never heard of: it parses, and it resolves to nothing.
    let resp_path = dir.join("fixture_responses.json");
    let mut responses: serde_json::Map<String, serde_json::Value> =
        serde_json::from_slice(&std::fs::read(&resp_path).unwrap()).unwrap();
    let relic_key = responses
        .keys()
        .find(|k| k.contains("ExportRelicArcane_en.json"))
        .cloned()
        .expect("the fixture serves a relic manifest");
    responses.insert(
        relic_key,
        serde_json::json!({"ExportRelicArcane": [{
            "name": "Lith V1 Relic",
            "uniqueName": "/Lotus/Types/Game/Projections/T1VoidProjectionGhostA",
            "relicRewards": [
                {"rewardName": "/Lotus/Types/Recipes/NoSuchThingAtAll",
                 "rarity": "COMMON", "tier": 0, "itemCount": 1}
            ]
        }]}),
    );
    std::fs::write(&resp_path, serde_json::to_vec(&responses).unwrap()).unwrap();

    let out = run(
        &["build", "--fixtures-dir", dir.to_str().unwrap(), "--now", "2026-07-01T12:00:00Z"],
        &dir,
    );
    assert!(out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(
        stderr.contains("fallback — no DE data available"),
        "a manifest that resolves nothing must reach the fallback:\n{stderr}"
    );

    let snap: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("market.json")).unwrap()).unwrap();
    assert!(
        !snap["relic_rewards"].as_object().unwrap().is_empty(),
        "never publish an empty relic surface while any source could produce one"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// The same case, but WITH a prior surface: carry it, do not downgrade.
#[test]
fn de_manifests_that_yield_no_rows_carry_a_prior_surface_rather_than_downgrading() {
    let dir = stage_fixtures("convert");
    let args = ["build", "--fixtures-dir", dir.to_str().unwrap(), "--now", "2026-07-01T12:00:00Z"];

    assert!(run(&args, &dir).status.success());
    std::fs::copy(dir.join("market.json"), dir.join("prior-market.json")).unwrap();

    let resp_path = dir.join("fixture_responses.json");
    let mut responses: serde_json::Map<String, serde_json::Value> =
        serde_json::from_slice(&std::fs::read(&resp_path).unwrap()).unwrap();
    let relic_key = responses.keys().find(|k| k.contains("ExportRelicArcane_en.json")).cloned().unwrap();
    responses.insert(
        relic_key,
        serde_json::json!({"ExportRelicArcane": [{
            "name": "Lith V1 Relic",
            "uniqueName": "/Lotus/Types/Game/Projections/T1VoidProjectionGhostB",
            "relicRewards": [
                {"rewardName": "/Lotus/Types/Recipes/NoSuchThingAtAll",
                 "rarity": "COMMON", "tier": 0, "itemCount": 1}
            ]
        }]}),
    );
    std::fs::write(&resp_path, serde_json::to_vec(&responses).unwrap()).unwrap();

    assert!(run(&args, &dir).status.success());
    let snap: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("market.json")).unwrap()).unwrap();

    let rewards = snap["relic_rewards"]["lith_v1_relic"].as_array().unwrap();
    let chassis = rewards
        .iter()
        .find(|r| r["reward_slug"] == "volt_prime_chassis_blueprint")
        .expect("the prior DE surface was carried");
    assert_eq!(
        chassis["chances"]["radiant"], 16.67,
        "a manifest yielding nothing must carry the prior surface, not downgrade to legacy"
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

/// Every DE-derived surface must survive a manifest that parses but is empty.
///
/// This is the rule the previous five rounds kept rediscovering one surface at
/// a time: **"the manifest arrived" is not "the manifest produced anything"**,
/// and each override — ducats, dispositions, relics — has to carry its prior
/// value when the fresh build comes back empty. Asserting all three together
/// is the point; testing them one at a time is how the hole kept moving.
#[test]
fn every_de_surface_carries_when_a_manifest_parses_to_nothing() {
    let dir = stage_fixtures("convert");
    let args = ["build", "--fixtures-dir", dir.to_str().unwrap(), "--now", "2026-07-01T12:00:00Z"];

    assert!(run(&args, &dir).status.success());
    std::fs::copy(dir.join("market.json"), dir.join("prior-market.json")).unwrap();
    let good: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("market.json")).unwrap()).unwrap();
    assert_eq!(good["items"]["volt_prime_chassis_blueprint"]["ducats"], 65);

    // Well-formed, correctly-keyed, and empty.
    let resp_path = dir.join("fixture_responses.json");
    let mut responses: serde_json::Map<String, serde_json::Value> =
        serde_json::from_slice(&std::fs::read(&resp_path).unwrap()).unwrap();
    for (basename, key) in [
        ("ExportRecipes_en.json", "ExportRecipes"),
        ("ExportWeapons_en.json", "ExportWeapons"),
        ("ExportRelicArcane_en.json", "ExportRelicArcane"),
    ] {
        let url = responses.keys().find(|k| k.contains(basename)).cloned().unwrap();
        responses.insert(url, serde_json::json!({ key: [] }));
    }
    std::fs::write(&resp_path, serde_json::to_vec(&responses).unwrap()).unwrap();

    let out = run(&args, &dir);
    assert!(out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    for expected in [
        "ducats: no DE values this cycle",
        "dispositions: none from DE this cycle",
        "Relic tables not rebuilt this cycle",
    ] {
        assert!(stderr.contains(expected), "missing `{expected}` in:\n{stderr}");
    }

    let snap: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("market.json")).unwrap()).unwrap();

    assert_eq!(
        snap["items"]["volt_prime_chassis_blueprint"]["ducats"], 65,
        "ducats must survive an empty recipes manifest"
    );
    assert_eq!(
        snap["de"]["ducats"]["volt_prime_chassis_blueprint"], 65,
        "and so must their provenance, or the NEXT cycle has nothing to carry"
    );
    assert_eq!(
        snap["de"]["dispositions"]["volt_prime"], 1.15,
        "disposition carry must use exact DE provenance, not every prior WFM row"
    );
    assert_eq!(snap["rivens"]["weapons"]["volt_prime"]["disposition"], 1.15);
    let rewards = snap["relic_rewards"]["lith_v1_relic"].as_array().unwrap();
    assert!(
        rewards.iter().any(|r| r["chances"]["radiant"] == 16.67),
        "relics must survive an empty relic manifest"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// The disposition change log must diff PUBLISHED values, not WFM's mirror.
///
/// The fixture makes the two sources disagree the way they do in production:
/// warframe.market still reports Volt Prime at 1.30 while DE's ExportWeapons
/// has moved it to 1.15. The snapshot publishes DE's value, so:
///
/// - a cycle whose published value matches the prior one logs NOTHING, even
///   though the mirror disagrees with both;
/// - computing the log before the override instead would diff 1.30 against the
///   stored 1.15 every single cycle and announce a disposition change that
///   never happened.
#[test]
fn the_disposition_log_reports_no_change_when_only_the_mirror_disagrees() {
    let dir = stage_fixtures("convert");
    let args = ["build", "--fixtures-dir", dir.to_str().unwrap(), "--now", "2026-07-01T12:00:00Z"];

    assert!(run(&args, &dir).status.success());
    let first: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("market.json")).unwrap()).unwrap();
    assert_eq!(
        first["rivens"]["weapons"]["volt_prime"]["disposition"], 1.15,
        "DE's value is what gets published, not WFM's 1.30"
    );

    // Feed that snapshot back in and run again. Nothing moved, so nothing may
    // be logged — the mirror still says 1.30 and must not provoke an entry.
    std::fs::copy(dir.join("market.json"), dir.join("prior-market.json")).unwrap();
    assert!(run(&args, &dir).status.success());
    let second: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("market.json")).unwrap()).unwrap();

    assert_eq!(second["rivens"]["weapons"]["volt_prime"]["disposition"], 1.15);
    let changes = second["rivens"]["changes"].as_array().cloned().unwrap_or_default();
    assert!(
        !changes.iter().any(|c| c["slug"] == "volt_prime"),
        "a lagging mirror must not manufacture a disposition change: {changes:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
