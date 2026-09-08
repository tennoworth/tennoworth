use super::rivens::parse_weekly_rivens;
use super::*;
use crate::clock;
use crate::de::{DE_WEEKLY_RIVENS_URL, DE_WEEKLY_RIVEN_PLATFORMS};
use std::collections::HashMap;

fn fixture() -> FixtureHttp {
    let mut r = HashMap::new();
    r.insert(
            "https://api.warframe.market/v2/items".into(),
            serde_json::json!({"data": [
                {"slug": "primed_continuity", "i18n": {"en": {"name": "Primed Continuity"}}, "tags": ["mod"], "ducats": 0, "maxRank": 10, "subtypes": []},
                {"slug": "volt_prime_set", "i18n": {"en": {"name": "Volt Prime Set"}}, "tags": ["prime"], "ducats": null, "maxRank": null, "subtypes": []}
            ]}),
        );
    FixtureHttp { responses: r }
}

#[test]
fn fetch_catalog_returns_name_slug_map_and_meta() {
    let (catalog, meta) =
        fetch_catalog_wfm(&fixture(), "https://api.warframe.market/v2/items").unwrap();
    assert_eq!(
        catalog.get("primed continuity"),
        Some(&"primed_continuity".into())
    );
    assert_eq!(
        catalog.get("volt prime set"),
        Some(&"volt_prime_set".into())
    );
    assert_eq!(meta.get("primed_continuity").unwrap().tags, vec!["mod"]);
    assert_eq!(meta.get("primed_continuity").unwrap().ducats, Some(0));
}

#[test]
fn fetch_catalog_retries_3x_then_errors() {
    let empty = FixtureHttp {
        responses: HashMap::new(),
    };
    let err = fetch_catalog_wfm(&empty, "https://api.warframe.market/v2/items");
    assert!(err.is_err());
}

const BARO_URL: &str = "https://api.warframestat.us/pc/voidTrader/";

fn baro_http(body: serde_json::Value) -> FixtureHttp {
    let mut r = HashMap::new();
    r.insert(BARO_URL.into(), body);
    FixtureHttp { responses: r }
}

#[test]
fn baro_inventory_is_captured_while_he_is_present() {
    let got = fetch_baro(&baro_http(serde_json::json!({
        "activation": "2026-08-21T13:00:00.000Z",
        "expiry": "2026-08-23T13:00:00.000Z",
        "location": "Orcus Relay (Pluto)",
        "inventory": [
            {"item": "Primed Fury", "ducats": 350, "credits": 200000},
            {"item": "Prisma Grakata", "ducats": 500, "credits": 300000}
        ]
    })));
    let inv = got.get("inventory").unwrap().as_array().unwrap();
    assert_eq!(inv.len(), 2);
    assert_eq!(inv[0].get("item").unwrap(), "Primed Fury");
    assert_eq!(inv[0].get("ducats").unwrap(), 350);
    assert_eq!(inv[0].get("credits").unwrap(), 200000);
    // Tagged with the visit it belongs to.
    assert_eq!(
        got.get("inventory_for").unwrap(),
        "2026-08-21T13:00:00.000Z"
    );
}

#[test]
fn baro_inventory_is_absent_not_empty_between_visits() {
    // The live shape while he is away: schedule fields present, inventory [].
    let got = fetch_baro(&baro_http(serde_json::json!({
        "activation": "2026-08-21T13:00:00.000Z",
        "expiry": "2026-08-23T13:00:00.000Z",
        "location": "Orcus Relay (Pluto)",
        "inventory": []
    })));
    assert!(got.contains_key("activation"), "schedule still lands");
    assert!(
        !got.contains_key("inventory"),
        "absent, so carry-forward can fire"
    );
    assert!(!got.contains_key("inventory_for"));
}

#[test]
fn baro_entries_without_an_item_name_are_dropped() {
    let got = fetch_baro(&baro_http(serde_json::json!({
        "activation": "a", "expiry": "b", "location": "c",
        "inventory": [{"ducats": 350}, {"item": "Primed Fury"}]
    })));
    let inv = got.get("inventory").unwrap().as_array().unwrap();
    assert_eq!(inv.len(), 1);
    assert_eq!(inv[0].get("item").unwrap(), "Primed Fury");
    // Optional fields simply do not appear rather than defaulting to 0 -
    // a missing ducat cost is unknown, not free.
    assert!(inv[0].get("ducats").is_none());
}

fn riven_http(weapons: &[(&str, &str, f64)]) -> FixtureHttp {
    let arr: Vec<serde_json::Value> = weapons
        .iter()
        .map(|(slug, name, d)| {
            serde_json::json!({
                "slug": slug, "disposition": d, "group": "primary", "rivenType": "rifle",
                "reqMasteryRank": 8, "gameRef": format!("/Lotus/Weapons/{slug}"),
                "i18n": {"en": {"name": name}}
            })
        })
        .collect();
    let mut r = HashMap::new();
    r.insert(
        WFM_RIVEN_WEAPONS_URL.into(),
        serde_json::json!({"apiVersion": "0.25.0", "data": arr}),
    );
    FixtureHttp { responses: r }
}

fn rivens_surface(
    weapons: &[(&str, f64)],
    changes: serde_json::Value,
) -> HashMap<String, serde_json::Value> {
    let mut w = serde_json::Map::new();
    for (slug, d) in weapons {
        w.insert(
            slug.to_string(),
            serde_json::json!({"name": slug, "disposition": d}),
        );
    }
    let mut m = HashMap::new();
    m.insert("weapons".into(), serde_json::Value::Object(w));
    m.insert("changes".into(), changes);
    m
}

#[test]
fn rivens_reduce_the_manifest_and_report_no_changes_without_a_prior() {
    let now = clock::parse_isoformat_utc("2026-08-16T12:00:00Z").unwrap();
    let got = fetch_rivens(&riven_http(&[
        ("kulstar", "Kulstar", 1.3),
        ("braton", "Braton", 1.15),
    ]));
    let w = got.get("weapons").unwrap().as_object().unwrap();
    assert_eq!(w.len(), 2);
    assert_eq!(w["kulstar"]["name"], "Kulstar");
    assert_eq!(w["kulstar"]["disposition"], 1.3);
    assert_eq!(w["kulstar"]["group"], "primary");
    assert_eq!(w["kulstar"]["riven_type"], "rifle");
    assert_eq!(w["kulstar"]["req_mr"], 8);
    // The fetch no longer computes the log at all - the caller does, after
    // applying DE's dispositions. See `riven_change_log`.
    assert!(!got.contains_key("changes"));
    let changes = riven_change_log(got["weapons"].as_object().unwrap(), None, now);
    assert!(changes.is_empty(), "no prior means no changes");
}

#[test]
fn rivens_diff_against_prior_and_carry_the_log_within_retention() {
    let now = clock::parse_isoformat_utc("2026-08-16T12:00:00Z").unwrap();
    let prior = rivens_surface(
        &[("kulstar", 1.3), ("braton", 1.10), ("lato", 1.4)],
        serde_json::json!([
            // recent: kept
            {"slug": "lato", "name": "Lato", "from": 1.35, "to": 1.4, "seen_at": "2026-07-01T00:00:00Z"},
            // older than 90 d: dropped
            {"slug": "burston", "name": "Burston", "from": 1.0, "to": 1.05, "seen_at": "2026-04-01T00:00:00Z"},
            // superseded by a change seen now
            {"slug": "braton", "name": "Braton", "from": 1.05, "to": 1.10, "seen_at": "2026-07-15T00:00:00Z"}
        ]),
    );
    let got = fetch_rivens(&riven_http(&[
        ("kulstar", "Kulstar", 1.3),
        ("braton", "Braton", 1.15),
        ("lato", "Lato", 1.4),
    ]));
    let ch = riven_change_log(got["weapons"].as_object().unwrap(), Some(&prior), now);
    let slugs: Vec<&str> = ch.iter().map(|c| c["slug"].as_str().unwrap()).collect();
    assert_eq!(
        slugs,
        vec!["braton", "lato"],
        "newest first; burston aged out; braton superseded"
    );
    assert_eq!(ch[0]["from"], 1.10);
    assert_eq!(ch[0]["to"], 1.15);
    assert_eq!(ch[0]["seen_at"], "2026-08-16T12:00:00Z");
}

#[test]
fn riven_change_log_diffs_published_values_not_the_wfm_mirror() {
    // The bug: the log used to be computed inside the fetch, so it diffed
    // warframe.market's fresh mirror against the prior snapshot's STORED
    // values - which are DE's, because the override lands afterwards.
    //
    // Here WFM still reports the OLD 1.30 while DE has already moved
    // Kulstar to 1.15, and the prior snapshot recorded DE's old 1.30.
    // Diffing the mirror would log 1.30 → 1.30 (nothing) and then publish
    // 1.15 unannounced; diffing what we actually publish logs the real move.
    let now = clock::parse_isoformat_utc("2026-08-16T12:00:00Z").unwrap();
    let prior = rivens_surface(&[("kulstar", 1.30)], serde_json::json!([]));

    let mut published = serde_json::Map::new();
    published.insert(
        "kulstar".into(),
        serde_json::json!({"name": "Kulstar", "disposition": 1.15}),
    );
    let ch = riven_change_log(&published, Some(&prior), now);
    assert_eq!(ch.len(), 1, "the real DE move is logged");
    assert_eq!(ch[0]["from"], 1.30);
    assert_eq!(ch[0]["to"], 1.15);
}

#[test]
fn riven_change_log_reports_nothing_when_the_published_value_is_unchanged() {
    // The other half: on a carry cycle the published value equals the prior
    // one even though WFM's mirror disagrees. No change occurred, so none
    // may be logged - a phantom entry here would tell users a disposition
    // moved when it did not.
    let now = clock::parse_isoformat_utc("2026-08-16T12:00:00Z").unwrap();
    let prior = rivens_surface(&[("kulstar", 1.15)], serde_json::json!([]));
    let mut published = serde_json::Map::new();
    published.insert(
        "kulstar".into(),
        serde_json::json!({"name": "Kulstar", "disposition": 1.15}),
    );
    assert!(riven_change_log(&published, Some(&prior), now).is_empty());
}

#[test]
fn rivens_fetch_failure_is_an_empty_surface_for_reconcile_to_fall_back() {
    let got = fetch_rivens(&FixtureHttp {
        responses: HashMap::new(),
    });
    assert!(got.is_empty());
}

#[test]
fn rivens_carry_game_ref_and_the_attributes_manifest() {
    // The riven-weapons fixture does not serve /riven/attributes, so the
    // surface is built without it and the call still succeeds.
    let got = fetch_rivens(&riven_http(&[("kulstar", "Kulstar", 1.3)]));
    let w = got.get("weapons").unwrap().as_object().unwrap();
    assert_eq!(w["kulstar"]["game_ref"], "/Lotus/Weapons/kulstar");

    // Serve the attributes manifest and re-run: the surface gains it.
    let mut r = HashMap::new();
    r.insert(
        WFM_RIVEN_ATTRIBUTES_URL.into(),
        serde_json::json!({"data": [
            {"gameRef": "WeaponCritDamageMod", "slug": "critical_damage",
             "i18n": {"en": {"name": "Critical Damage"}}, "unit": "percent"},
            {"gameRef": "WeaponPunctureDepthMod", "slug": "punch_through",
             "i18n": {"en": {"name": "Punch Through"}}}
        ]}),
    );
    let mut weapons = serde_json::Map::new();
    weapons.insert(
        "kulstar".into(),
        serde_json::json!({"slug": "kulstar", "name": "Kulstar", "disposition": 1.3}),
    );
    r.insert(
        WFM_RIVEN_WEAPONS_URL.into(),
        serde_json::json!({"data": [
            {"slug": "kulstar", "disposition": 1.3, "gameRef": "/Lotus/x",
             "i18n": {"en": {"name": "Kulstar"}}}
        ]}),
    );
    let got2 = fetch_rivens(&FixtureHttp { responses: r });
    let attrs = got2.get("attributes").unwrap().as_array().unwrap();
    assert_eq!(attrs.len(), 2);
    assert_eq!(attrs[0]["game_ref"], "WeaponCritDamageMod");
    assert_eq!(attrs[0]["unit"], "percent");
    assert!(
        attrs[1].get("unit").is_none(),
        "non-percent stats carry no unit"
    );
}

fn weekly_http(body: &str) -> FixtureHttp {
    // The DE file is a JS literal; the raw-text fetch stands in for it as
    // a JSON string fixture (see Http::get_text's default impl).
    let mut r = HashMap::new();
    r.insert(
        DE_WEEKLY_RIVENS_URL.into(),
        serde_json::Value::String(body.into()),
    );
    FixtureHttp { responses: r }
}

fn weapons_by_name() -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert("acceltra".into(), "acceltra".into());
    m.insert("ack & brunt".into(), "ack_brunt".into());
    m.insert("ax-52".into(), "ax_52".into());
    m
}

#[test]
fn weekly_rivens_parses_the_js_object_literal() {
    let body = r#"[
            { itemType: 'Rifle Riven Mod', compatibility: null, rerolled: false,
              avg: 68.39, stddev: 250.66, min: 3, max: 2000, pop: 15, median: 10 },
            { itemType: 'Rifle Riven Mod', compatibility: 'AX-52', rerolled: true,
              avg: 204.69, stddev: 387.32, min: 2, max: 2069, pop: 6, median: 75 },
            { itemType: 'Rifle Riven Mod', compatibility: 'Acceltra', rerolled: false,
              avg: 41.75, stddev: 45.23, min: 5, max: 400, pop: 10, median: 35 }
        ]"#;
    let rows = parse_weekly_rivens(body).unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].compatibility, None);
    assert_eq!(rows[1].compatibility.as_deref(), Some("AX-52"));
    assert!(rows[1].rerolled);
    assert_eq!(rows[1].median, 75.0);
    assert_eq!(rows[2].pop, 10);
}

#[test]
fn weekly_rivens_rejects_garbage_loudly() {
    assert!(parse_weekly_rivens("not js at all").is_err());
    // Missing colon between key and value.
    assert!(parse_weekly_rivens("[{itemType 'x'}]").is_err());
    // Two values in an array without a separator.
    assert!(parse_weekly_rivens("[1 2]").is_err());
}

#[test]
fn riven_stats_reduce_by_weapon_and_reroll_state_with_drop_counting() {
    let body = r#"[
            { itemType: 'Rifle Riven Mod', compatibility: 'Acceltra', rerolled: false,
              avg: 41.75, stddev: 45.23, min: 5, max: 400, pop: 10, median: 35 },
            { itemType: 'Rifle Riven Mod', compatibility: 'Acceltra', rerolled: true,
              avg: 266.72, stddev: 580.64, min: 5, max: 4600, pop: 12, median: 100 },
            { itemType: 'Rifle Riven Mod', compatibility: 'AX-52', rerolled: false,
              avg: 87.2, stddev: 300.77, min: 5, max: 3000, pop: 10, median: 30 },
            { itemType: 'Rifle Riven Mod', compatibility: 'NotARealWeapon', rerolled: false,
              avg: 1.0, stddev: 0.5, min: 1, max: 2, pop: 1, median: 1 },
            { itemType: 'Melee Riven Mod', compatibility: null, rerolled: false,
              avg: 52.85, stddev: 274.96, min: 2, max: 2300, pop: 8, median: 7 }
        ]"#;
    let (stats, unmatched, _) = fetch_riven_stats(&weekly_http(body), &weapons_by_name());
    assert_eq!(unmatched, 1, "only the unknown weapon counts");
    let acceltra = stats.get("acceltra").unwrap();
    assert_eq!(acceltra["name"], "Acceltra");
    assert_eq!(acceltra["unrolled"]["median"], 35.0);
    assert_eq!(acceltra["rolled"]["median"], 100.0);
    assert_eq!(acceltra["rolled"]["pop"], 12);
    assert_eq!(acceltra["unrolled"]["avg"], 41.75);
    let ax = stats.get("ax_52").unwrap();
    assert!(ax.get("rolled").is_none(), "no rolled rows for AX-52");
    assert_eq!(ax["unrolled"]["min"], 5.0);
    assert!(!stats.contains_key("ack_brunt"));
}

#[test]
fn riven_stats_fold_consoles_under_platforms_without_disturbing_pc() {
    let pc = r#"[
            { itemType: 'Rifle Riven Mod', compatibility: 'Acceltra', rerolled: false,
              avg: 41.75, stddev: 45.23, min: 5, max: 400, pop: 10, median: 35 }
        ]"#;
    let ps4 = r#"[
            { itemType: 'Rifle Riven Mod', compatibility: 'Acceltra', rerolled: false,
              avg: 88.0, stddev: 60.0, min: 10, max: 500, pop: 30, median: 75 },
            { itemType: 'Rifle Riven Mod', compatibility: 'Ack & Brunt', rerolled: false,
              avg: 9.0, stddev: 2.0, min: 7, max: 14, pop: 3, median: 9 }
        ]"#;
    let mut r = HashMap::new();
    r.insert(
        DE_WEEKLY_RIVENS_URL.into(),
        serde_json::Value::String(pc.into()),
    );
    r.insert(
        DE_WEEKLY_RIVEN_PLATFORMS[0].1.into(),
        serde_json::Value::String(ps4.into()),
    );
    let (stats, _, _) = fetch_riven_stats(&FixtureHttp { responses: r }, &weapons_by_name());

    let acceltra = stats.get("acceltra").unwrap();
    // PC keeps its place at the top level, untouched.
    assert_eq!(acceltra["unrolled"]["median"], 35.0);
    // The console rides along underneath - console riven prices diverge
    // sharply from PC's, which is the whole point of carrying them.
    assert_eq!(acceltra["platforms"]["ps4"]["unrolled"]["median"], 75.0);
    assert_eq!(acceltra["platforms"]["ps4"]["unrolled"]["pop"], 30);

    // A weapon only the console saw has no PC baseline to compare against,
    // so it contributes nothing rather than a lone console-only row.
    assert!(!stats.contains_key("ack_brunt"));
}

#[test]
fn riven_stats_survive_one_platform_failing() {
    // A Switch outage must not cost us PC, which is what we actually price
    // against.
    let pc = r#"[
            { itemType: 'Rifle Riven Mod', compatibility: 'Acceltra', rerolled: false,
              avg: 41.75, stddev: 45.23, min: 5, max: 400, pop: 10, median: 35 }
        ]"#;
    let (stats, _, children) = fetch_riven_stats(&weekly_http(pc), &weapons_by_name());
    assert_eq!(children["pc"], RivenChildOutcome::Usable);
    assert_eq!(children["swi"], RivenChildOutcome::Unavailable);
    let acceltra = stats.get("acceltra").unwrap();
    assert_eq!(acceltra["unrolled"]["median"], 35.0);
    assert!(acceltra.get("platforms").is_none());
}

#[test]
fn riven_stats_fetch_failure_is_empty_for_reconcile_to_fall_back() {
    let (stats, unmatched, children) = fetch_riven_stats(
        &FixtureHttp {
            responses: HashMap::new(),
        },
        &weapons_by_name(),
    );
    assert!(stats.is_empty());
    assert_eq!(unmatched, 0);
    assert_eq!(children["pc"], RivenChildOutcome::Unavailable);
}

#[test]
fn failed_console_carries_only_onto_fresh_pc_weapons() {
    let prior = HashMap::from([
        (
            "kept".into(),
            serde_json::json!({"name":"Kept","unrolled":{"median":10},"platforms":{"swi":{"unrolled":{"median":20}}}}),
        ),
        (
            "gone".into(),
            serde_json::json!({"name":"Gone","platforms":{"swi":{"unrolled":{"median":30}}}}),
        ),
    ]);
    let mut fresh = HashMap::from([(
        "kept".into(),
        serde_json::json!({"name":"Kept","unrolled":{"median":11}}),
    )]);
    let failed = HashMap::from([
        ("pc".into(), RivenChildOutcome::Usable),
        ("swi".into(), RivenChildOutcome::Invalid),
    ]);
    carry_failed_riven_platforms(&mut fresh, Some(&prior), &failed);
    assert_eq!(fresh["kept"]["platforms"]["swi"]["unrolled"]["median"], 20);
    assert!(
        !fresh.contains_key("gone"),
        "console-only stale weapons stay gone"
    );

    let mut cleared = fresh.clone();
    cleared
        .get_mut("kept")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove("platforms");
    let cleared_outcomes = HashMap::from([
        ("pc".into(), RivenChildOutcome::Usable),
        ("swi".into(), RivenChildOutcome::AuthoritativeEmpty),
    ]);
    carry_failed_riven_platforms(&mut cleared, Some(&prior), &cleared_outcomes);
    assert!(
        cleared["kept"].get("platforms").is_none(),
        "valid empty Switch data clears prior"
    );
}

fn set_catalog() -> HashMap<String, String> {
    let mut m = HashMap::new();
    for n in [
        "revenant prime",
        "baruuk prime",
        "nezha prime",
        "octavia prime",
        "oberon prime",
        "gauss prime",
    ] {
        m.insert(format!("{n} set"), format!("{}_set", n.replace(' ', "_")));
    }
    m
}

#[test]
fn pack_names_resolve_to_frames() {
    assert_eq!(
        frames_in_pack_name("M P V Revenant Baruuk Prime Dual Pack"),
        vec!["Revenant Prime", "Baruuk Prime"]
    );
    assert_eq!(
        frames_in_pack_name("Nezha & Octavia Prime Dual Pack"),
        vec!["Nezha Prime", "Octavia Prime"]
    );
    assert_eq!(
        frames_in_pack_name("M P V Oberon Prime Single Pack"),
        vec!["Oberon Prime"]
    );
    assert!(frames_in_pack_name("Last Chance Item C").is_empty());
    assert!(frames_in_pack_name("").is_empty());
}

#[test]
fn resurgence_rotations_bound_each_window_by_the_previous_expiry() {
    let vt = serde_json::json!({
        "activation": "2026-08-06T18:00:00.000Z",
        "expiry": "2026-09-03T18:00:00.000Z",
        "initialStart": "2022-09-09T15:42:24.266Z",
        "inventory": [
            {"item": "M P V Revenant Prime Single Pack", "ducats": 6},
            {"item": "M P V Revenant Baruuk Prime Dual Pack", "ducats": 10}
        ],
        "schedule": [
            {"expiry": "2022-11-03T18:00:00.000Z", "item": "M P V Oberon Prime Single Pack"},
            {"expiry": "2022-12-01T19:00:00.000Z", "item": "Last Chance Item C"},
            {"expiry": "2023-01-05T19:00:00.000Z", "item": "Nezha & Octavia Prime Dual Pack"},
            {"expiry": "2023-02-02T19:00:00.000Z"}
        ]
    });
    let (rot, cur) = resurgence_rotations(&vt, &set_catalog());
    assert_eq!(
        rot.len(),
        2,
        "unresolvable packs are skipped but still advance the window"
    );
    assert_eq!(rot[0]["from"], "2022-09-09T15:42:24.266Z");
    assert_eq!(rot[0]["to"], "2022-11-03T18:00:00.000Z");
    assert_eq!(rot[0]["frames"], serde_json::json!(["oberon_prime_set"]));
    assert_eq!(
        rot[1]["from"], "2022-12-01T19:00:00.000Z",
        "the skipped rotation still bounds the next one"
    );
    assert_eq!(
        rot[1]["frames"],
        serde_json::json!(["nezha_prime_set", "octavia_prime_set"])
    );
    let cur = cur.unwrap();
    assert_eq!(cur["from"], "2026-08-06T18:00:00.000Z");
    assert_eq!(
        cur["frames"],
        serde_json::json!(["revenant_prime_set", "baruuk_prime_set"])
    );
}

#[test]
fn calendar_primes_come_from_the_wfstat_payload_and_need_a_wfm_set() {
    let raw = serde_json::json!([
        {"name": "Gauss Prime", "category": "Warframes", "releaseDate": "2024-01-17", "vaulted": true, "vaultDate": "2025-12-10", "estimatedVaultDate": "2025-12-10"},
        {"name": "Gauss Prime Helmet", "category": "Skins"},
        {"name": "Unknown Prime", "category": "Warframes", "releaseDate": "2020-01-01"},
        {"name": "Revenant Prime", "category": "Warframes", "releaseDate": "2022-08-30", "vaulted": false}
    ]);
    let http = FixtureHttp {
        responses: HashMap::new(),
    }; // vaultTrader absent → warning, no rotations
    let cal = fetch_calendar(&http, Some(&raw), &set_catalog());
    let primes = cal["primes"].as_object().unwrap();
    assert_eq!(primes.len(), 2);
    assert_eq!(primes["gauss_prime_set"]["vault_date"], "2025-12-10");
    assert_eq!(primes["gauss_prime_set"]["vaulted"], true);
    assert_eq!(primes["revenant_prime_set"]["vaulted"], false);
    assert!(primes["revenant_prime_set"].get("vault_date").is_none());
    assert!(!cal.contains_key("resurgence"));
}

#[test]
fn carry_forward_preserves_the_last_visits_stock() {
    let prior: HashMap<String, serde_json::Value> = serde_json::from_value(serde_json::json!({
        "activation": "2026-08-21T13:00:00.000Z",
        "expiry": "2026-08-23T13:00:00.000Z",
        "location": "Orcus Relay (Pluto)",
        "inventory": [{"item": "Primed Fury", "ducats": 350}],
        "inventory_for": "2026-08-21T13:00:00.000Z"
    }))
    .unwrap();
    // The next scrape, after he has left: new schedule, no stock.
    let mut fresh: HashMap<String, serde_json::Value> = serde_json::from_value(serde_json::json!({
        "activation": "2026-09-04T13:00:00.000Z",
        "expiry": "2026-09-06T13:00:00.000Z",
        "location": "Kronia Relay (Saturn)"
    }))
    .unwrap();

    carry_baro_inventory(&mut fresh, Some(&prior));

    // The NEW schedule wins; the OLD stock is kept and still labelled with
    // the visit it came from, so a consumer can see it is not current.
    assert_eq!(fresh.get("activation").unwrap(), "2026-09-04T13:00:00.000Z");
    assert_eq!(fresh.get("inventory").unwrap().as_array().unwrap().len(), 1);
    assert_eq!(
        fresh.get("inventory_for").unwrap(),
        "2026-08-21T13:00:00.000Z"
    );
}

#[test]
fn carry_forward_never_overwrites_a_live_capture() {
    let prior: HashMap<String, serde_json::Value> = serde_json::from_value(serde_json::json!({
        "inventory": [{"item": "Old Thing"}],
        "inventory_for": "2026-08-21T13:00:00.000Z"
    }))
    .unwrap();
    let mut fresh: HashMap<String, serde_json::Value> = serde_json::from_value(serde_json::json!({
        "activation": "2026-09-04T13:00:00.000Z",
        "inventory": [{"item": "New Thing"}],
        "inventory_for": "2026-09-04T13:00:00.000Z"
    }))
    .unwrap();

    carry_baro_inventory(&mut fresh, Some(&prior));

    assert_eq!(
        fresh.get("inventory").unwrap()[0].get("item").unwrap(),
        "New Thing"
    );
    assert_eq!(
        fresh.get("inventory_for").unwrap(),
        "2026-09-04T13:00:00.000Z"
    );
}

#[test]
fn carry_forward_is_inert_without_usable_prior_data() {
    let mut fresh: HashMap<String, serde_json::Value> =
        serde_json::from_value(serde_json::json!({"activation": "x"})).unwrap();
    carry_baro_inventory(&mut fresh, None);
    assert!(!fresh.contains_key("inventory"));

    // A prior with a list but no `inventory_for` is pre-upgrade data; taking
    // it would leave stock that cannot be dated.
    let partial: HashMap<String, serde_json::Value> =
        serde_json::from_value(serde_json::json!({"inventory": [{"item": "x"}]})).unwrap();
    carry_baro_inventory(&mut fresh, Some(&partial));
    assert!(!fresh.contains_key("inventory"));

    // A totally failed fetch stays failed - carry-forward must not
    // resurrect a surface reconcile is about to treat as empty.
    let mut failed: HashMap<String, serde_json::Value> = HashMap::new();
    let full: HashMap<String, serde_json::Value> = serde_json::from_value(serde_json::json!({
        "inventory": [{"item": "x"}], "inventory_for": "t"
    }))
    .unwrap();
    carry_baro_inventory(&mut failed, Some(&full));
    assert!(failed.is_empty());
}
fn parent_http() -> FixtureHttp {
    // The two endpoints that still exist, both empty - isolates the
    // sentinel path.
    let mut r = HashMap::new();
    r.insert(
        "https://api.warframestat.us/warframes/".into(),
        serde_json::json!([]),
    );
    r.insert(
        "https://api.warframestat.us/weapons/".into(),
        serde_json::json!([]),
    );
    FixtureHttp { responses: r }
}

fn sentinel_catalog() -> HashMap<String, String> {
    let mut c = HashMap::new();
    c.insert("carrier prime set".into(), "carrier_prime_set".into());
    c.insert(
        "carrier prime cerebrum".into(),
        "carrier_prime_cerebrum".into(),
    );
    c
}

#[test]
fn sentinel_parents_come_from_the_bulk_item_payload() {
    // /sentinels/ 404s upstream since ~2026-07-31; these parents must still
    // resolve, out of the /items/ payload we already download.
    let items = serde_json::json!([
        {"name": "Excalibur", "category": "Warframes", "components": []},
        {"name": "Carrier Prime", "category": "Sentinels", "components": [
            {"uniqueName": "/Lotus/Types/Sentinels/CarrierPrime/Cerebrum", "name": "Cerebrum", "itemCount": 2}
        ]}
    ]);
    let (p2i, s2p, complete) = fetch_parent_data(&parent_http(), &sentinel_catalog(), Some(&items));

    assert!(
        complete,
        "both live endpoints answered and the payload was present"
    );
    let info = p2i
        .get("/Lotus/Types/Sentinels/CarrierPrime/Cerebrum")
        .expect("sentinel component path resolved");
    assert_eq!(info.get("slug").unwrap(), "carrier_prime_cerebrum");
    // Category comes from the item's own field, not the fallback.
    assert_eq!(info.get("category").unwrap(), "Sentinels");

    let set = s2p.get("carrier_prime_set").expect("sentinel set built");
    assert_eq!(set.get("name").unwrap(), "Carrier Prime");
    assert_eq!(set.get("parts").unwrap().as_array().unwrap().len(), 1);
    assert_eq!(set["parts"][0]["quantity"], 2);
}

#[test]
fn non_prime_sentinels_are_ignored_like_every_other_parent() {
    let items = serde_json::json!([
        {"name": "Carrier", "category": "Sentinels", "components": [
            {"uniqueName": "/Lotus/Types/Sentinels/Carrier/Cerebrum", "name": "Cerebrum"}
        ]}
    ]);
    let (p2i, s2p, _) = fetch_parent_data(&parent_http(), &sentinel_catalog(), Some(&items));
    assert!(p2i.is_empty() && s2p.is_empty());
}

#[test]
fn a_missing_item_payload_marks_the_surface_incomplete() {
    // `complete` is what makes reconcile MERGE over the prior snapshot
    // instead of replacing it. Getting this wrong would silently delete
    // every sentinel prime the moment the bulk fetch failed.
    let (_, _, complete) = fetch_parent_data(&parent_http(), &sentinel_catalog(), None);
    assert!(!complete);
}

#[test]
fn a_failed_live_endpoint_still_marks_the_surface_incomplete() {
    let empty = FixtureHttp {
        responses: HashMap::new(),
    };
    let (_, _, complete) =
        fetch_parent_data(&empty, &sentinel_catalog(), Some(&serde_json::json!([])));
    assert!(!complete);
}
