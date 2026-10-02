//! Snapshot construction coordinates independently observed upstream data.

mod catalog;
mod inputs;
mod paths;
mod publish;
mod rivens;
mod usage;
mod world;
pub use paths::find_root;

use std::collections::HashMap;
use std::path::Path;

use chrono::Utc;

use crate::clock;

/// Days after which a preserved-from-prior surface triggers a "this surface
/// is stale" warning.
const STALE_DAYS: i64 = 7;
use crate::csvin;
use crate::ingest;
use crate::reconcile::{reconcile, reconcile_keyed, KeyStamps, Observation};
use crate::render::{self, assemble_snapshot};
use crate::{de, de_extract};

pub fn build(fixtures_dir: Option<&Path>, now_arg: Option<&str>) -> Result<(), String> {
    let now = now_arg
        .map(|s| clock::parse_stamp(s).ok_or_else(|| format!("invalid --now stamp: {s}")))
        .unwrap_or_else(|| Ok(Utc::now()))?;

    let inputs::BuildInputs { http, csv_path, json_out, catalog_out, prior } = inputs::prepare(fixtures_dir)?;

    let prior_stamps: HashMap<String, String> = prior
        .get("surface_fetched_at")
        .and_then(|s| s.as_object())
        .map(|m| {
            m.iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                .collect()
        })
        .unwrap_or_default();
    let prior_key_stamps: HashMap<String, KeyStamps> = prior
        .get("surface_key_fetched_at")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    let catalog::CatalogSources {
        catalog, mut meta_by_slug, wfstat_raw, path_to_info,
        mut set_to_parts, parents_complete, catalog_fresh, path_to_info_complete,
    } = catalog::fetch(http.as_ref(), fixtures_dir, &catalog_out, &prior)?;

    // ---- Digital Extremes first-party ingest -------------------------------
    // Runs after path_to_info because every DE join resolves `/Lotus/...`
    // through it. Costs 490 bytes on a cycle where nothing changed: the export
    // index is content-hashed, so unchanged manifests are skipped entirely.
    let prior_de_hashes: std::collections::BTreeMap<String, String> = prior
        .get("de")
        .and_then(|d| d.get("hashes"))
        .and_then(|h| h.as_object())
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();

    eprintln!("Fetching DE Public Export index...");
    let de_snap = de::fetch_export(http.as_ref(), &prior_de_hashes);
    if de_snap.hashes.is_empty() {
        eprintln!("  unavailable - DE surfaces fall back to the prior snapshot");
    } else {
        // Fetched vs skipped, counted from what we actually hold - the old
        // line derived "skipped" from the changed count, which reported the
        // always-fetch manifests as skipped when they had just been pulled.
        let skipped = de::WANTED_MANIFESTS
            .iter()
            .filter(|n| de_snap.skipped(n))
            .count();
        eprintln!(
            "  {} manifests indexed · {} fetched ({}) · {} skipped as unchanged",
            de_snap.hashes.len(),
            de_snap.manifests.len(),
            if de_snap.changed.is_empty() {
                "unchanged but always-fetch".to_string()
            } else {
                de_snap.changed.join(", ")
            },
            skipped,
        );
    }

    eprintln!("Fetching DE worldState...");
    let de_world_observation = de::fetch_world_state(http.as_ref());
    let de_world = match &de_world_observation {
        Observation::Usable { data, .. } => Some(data),
        _ => None,
    };

    // Ducats, first-party. `primeSellingPrice` keys on the recipe (the
    // blueprint you trade), so it resolves through path_to_info the same way
    // relic rewards do. Only applied when the manifest actually came through
    // this cycle - a skipped manifest must not blank a good value.
    // ALWAYS_FETCH guarantees we ATTEMPT this manifest every cycle. It does not
    // guarantee we get it: the index can answer while the manifest itself 500s.
    // Ducats are an override on `meta_by_slug`, which is rebuilt from
    // warframe.market every cycle, so reconcile has nothing to carry and a
    // failed fetch would silently drop the whole catalogue back to WFM's
    // values. The failure path therefore re-applies the last known-good
    // override from the prior snapshot.
    let de_recipes = de_snap.manifests.get("ExportRecipes_en.json");

    // Prime sets warframestat has not published yet, from DE's recipes. Runs
    // before the DE ducat override below: the completeness check compares the
    // parts against the set total on warframe.market's side. Without recipes
    // this cycle the surface is partial, so reconcile keeps the prior entries.
    if let Some(recipes) = de_recipes {
        // With warframestat's parents only partly fetched, a set it normally
        // lists can be missing this cycle; the prior entry is carried by the
        // partial merge and must not be replaced by a derived breakdown.
        let mut known = set_to_parts.clone();
        if !parents_complete {
            if let Some(prior_sets) = prior.get("set_to_parts").and_then(|v| v.as_object()) {
                for (slug, parts) in prior_sets {
                    known.entry(slug.clone()).or_insert_with(|| parts.clone());
                }
            }
        }
        let derived = de_extract::sets_from_recipes(recipes, &path_to_info, &meta_by_slug, &known);
        eprintln!("  {} prime sets derived from DE recipes", derived.len());
        set_to_parts.extend(derived);
    }
    let set_to_parts_complete = parents_complete && catalog_fresh && de_recipes.is_some();

    // Slugs DE set, this cycle or carried from the last one. Written into the
    // snapshot as provenance so a later failure knows which values were ours.
    let mut de_ducats: std::collections::BTreeMap<String, i64> = std::collections::BTreeMap::new();

    // Built first so the same "did it actually produce anything" test that
    // governs relics governs this too - a manifest can parse and resolve
    // nothing, and treating arrival as success is precisely the mistake that
    // took four review rounds to stop making.
    let fresh_ducats: HashMap<String, i64> = de_recipes
        .map(|recipes| {
            let by_unique = de_extract::ducats_from_recipes(recipes);
            let alias = de_extract::recipe_alias(recipes);
            by_unique
                .iter()
                .filter_map(|(unique, ducats)| {
                    let info = de_extract::resolve_path(unique, &path_to_info, &alias)?;
                    let slug = info.get("slug").and_then(|s| s.as_str())?;
                    Some((slug.to_string(), *ducats))
                })
                .collect()
        })
        .unwrap_or_default();

    if fresh_ducats.is_empty() {
        // Carry ONLY the values DE previously set. Copying every prior ducat
        // would stamp stale numbers over fresh, legitimately-corrected WFM
        // ones - trading a known bug for a subtler one.
        let prior_de_ducats: std::collections::BTreeMap<String, i64> = prior
            .get("de")
            .and_then(|d| d.get("ducats"))
            .and_then(|m| m.as_object())
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| v.as_i64().map(|n| (k.clone(), n)))
                    .collect()
            })
            .unwrap_or_default();
        let mut carried = 0usize;
        for (slug, ducats) in &prior_de_ducats {
            if let Some(meta) = meta_by_slug.get_mut(slug) {
                meta.ducats = Some(*ducats);
                carried += 1;
            }
        }
        de_ducats = prior_de_ducats;
        eprintln!("  ducats: no DE values this cycle - carried {carried} from the prior snapshot");
    } else {
        let mut applied = 0usize;
        let mut disagreed = 0usize;
        for (slug, ducats) in &fresh_ducats {
            if let Some(meta) = meta_by_slug.get_mut(slug) {
                if meta.ducats.is_some_and(|d| d != *ducats && d != 0) {
                    disagreed += 1;
                }
                meta.ducats = Some(*ducats);
                de_ducats.insert(slug.clone(), *ducats);
                applied += 1;
            }
        }
        eprintln!("  ducats: {applied} slugs from DE ({disagreed} disagreed with WFM's value)");
    }

    // Build costs, keyed by the item produced. Only available when the recipes
    // manifest came through this cycle; reconcile preserves it otherwise.
    // An empty map here is correct on a skipped cycle: reconcile carries the
    // prior recipes forward, and there is no legacy source to be tempted by.
    let (recipes_surface, recipe_collisions) = de_recipes
        .map(|r| de_extract::recipes_from_export(r, &path_to_info))
        .unwrap_or_default();
    let recipes_observation = if !recipes_surface.is_empty() {
        Observation::usable(recipes_surface)
    } else {
        match de_snap.outcome("ExportRecipes_en.json") {
            de::ManifestOutcome::Unchanged => Observation::Unchanged,
            de::ManifestOutcome::Unavailable => Observation::Unavailable,
            de::ManifestOutcome::Invalid | de::ManifestOutcome::Usable => Observation::Invalid,
        }
    };
    if let Observation::Usable { data: recipes, .. } = &recipes_observation {
        eprintln!("  recipes: {} buildable items costed", recipes.len());
        if recipe_collisions > 0 {
            eprintln!(
                "  warning: {recipe_collisions} recipes collided on an already-taken slug \
                 (first kept) - path_to_info is not one-to-one"
            );
        }
    }

    let usage::UsageSources { usage_old, usage_history, usage_observation } = usage::fetch(http.as_ref(), &prior, &catalog);

    // Relic rewards. DE's table beats the drop-table scrape on two counts: all
    // four refinements instead of intact only, and correct rarity labels (the
    // old source calls 25.33% drops "Uncommon").
    //
    // The three cases below are NOT interchangeable, and conflating two of
    // them was a real bug: when the manifest is skipped as unchanged, falling
    // back to the legacy scrape overwrites a good four-refinement surface with
    // an intact-only one on every warm cycle. A skip must emit EMPTY so
    // reconcile carries the DE-derived surface forward; only an unreachable DE
    // justifies the fallback.
    //
    // The fallback fires ONLY when DE is unreachable outright. Any other
    // shortfall - a skipped manifest, or one that failed to fetch or parse -
    // emits EMPTY so reconcile carries the DE-derived surface forward.
    // Reaching for the legacy source in those cases produces a NON-EMPTY
    // intact-only table, and a non-empty surface is exactly what stops
    // reconcile preserving the good one.
    // Relic rewards, as ONE policy rather than a chain of special cases.
    //
    // Four rounds of review found four adjacent holes here, each because a
    // condition covered the state it was written for and not its neighbours.
    // The inputs are: did DE's manifests arrive, did they yield rows, and is
    // there a prior surface to fall back on. Those collapse to three outcomes,
    // in strict priority:
    //
    //   1. Fresh DE rows          → publish them.
    //   2. Otherwise, prior rows  → publish EMPTY, so reconcile carries them.
    //   3. Otherwise              → the legacy scrape; something beats nothing.
    //
    // The invariant that ties it together: **never publish an empty relic
    // surface while any source could produce one.** Empty is only ever a
    // deliberate instruction to reconcile, never an outcome.
    //
    // Note that "DE's manifests arrived" is not the same as "DE produced
    // rows": a manifest can parse cleanly and still resolve nothing usable,
    // which is the case that slipped through the previous fix.
    let prior_has_relics = prior
        .get("relic_rewards")
        .and_then(|r| r.as_object())
        .is_some_and(|r| !r.is_empty());

    let fresh_relics = match (
        de_snap.manifests.get("ExportRelicArcane_en.json"),
        de_recipes,
    ) {
        (Some(relics), Some(recipes)) => {
            eprintln!("Building relic tables from DE Public Export...");
            let build = de_extract::relic_rewards_from_de(relics, recipes, &path_to_info);
            eprintln!(
                "  {} relics · {} reward refs resolved · {} unresolved{}",
                build.rewards.len(),
                build.resolved,
                build.unresolved,
                if build.samples.is_empty() {
                    String::new()
                } else {
                    format!(" (e.g. {})", build.samples.join(", "))
                }
            );
            build.rewards
        }
        _ => HashMap::new(),
    };

    let (relic_observation, relic_source) = if !fresh_relics.is_empty() {
        (Observation::usable(fresh_relics), "de_public_export")
    } else if prior_has_relics {
        let (state, status) = match (
            de_snap.outcome("ExportRelicArcane_en.json"),
            &recipes_observation,
        ) {
            (de::ManifestOutcome::Invalid, _)
            | (_, Observation::Invalid | Observation::AuthoritativeEmpty) => {
                (Observation::Invalid, "invalid this cycle")
            }
            (de::ManifestOutcome::Unavailable, _) | (_, Observation::Unavailable) => {
                (Observation::Unavailable, "unavailable this cycle")
            }
            (
                de::ManifestOutcome::Unchanged,
                Observation::Usable { .. } | Observation::Unchanged,
            ) => (Observation::Unchanged, "hash-verified unchanged"),
            _ => (Observation::Invalid, "invalid this cycle"),
        };
        eprintln!("Relic tables {status} - carrying the prior DE surface");
        (state, "de_public_export")
    } else {
        // No DE rows and nothing to carry. The old drop-table scrape stays as
        // the fallback rather than being deleted: it is the only other source
        // of a relic table, and losing relics entirely would be a worse
        // regression than intact-only odds. It is off the happy path, so its
        // rarity mislabelling and intact-only coverage stop being what users
        // normally see - and it can only ever overwrite a DE surface when
        // there is no DE surface to protect.
        eprintln!("Fetching relic drop tables (fallback - no DE data available)...");
        let r = ingest::fetch_relic_rewards(http.as_ref(), &catalog);
        eprintln!("  {} relics with reward data (intact only)", r.len());
        if r.is_empty() {
            (Observation::Unavailable, "legacy_drop_table")
        } else {
            (Observation::usable(r), "legacy_drop_table")
        }
    };

    eprintln!("Fetching prime vault status...");
    let (vault_status, vault_complete) = ingest::fetch_vault_status(http.as_ref(), &catalog, now);
    {
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for v in vault_status.values() {
            *counts.entry(v.as_str()).or_default() += 1;
        }
        eprintln!("  {} slugs tagged · {:?}", vault_status.len(), counts);
    }

    eprintln!("Fetching Baro Ki'Teer schedule...");
    // worldState carries the manifest from ANNOUNCEMENT, so his stock is known
    // days before he lands - the old source returned an empty list between
    // visits and published no schedule at all.
    let de_alias = de_recipes.map(de_extract::recipe_alias).unwrap_or_default();
    let de_baro = de_world
        .as_ref()
        .map(|w| {
            de_extract::baro_from_world(w, &path_to_info, &de_alias, |ms| {
                clock::iso_z(clock::from_millis(ms))
            })
        })
        .filter(|b| !b.is_empty());
    let (baro, baro_source) = de_baro.map(|b| (b, "de_world_state")).unwrap_or_else(|| {
        eprintln!("  worldState had no trader - falling back to warframestat");
        (ingest::fetch_baro(http.as_ref()), "warframestat")
    });
    eprintln!(
        "  baro: {} · {} items",
        baro.get("location")
            .and_then(|l| l.as_str())
            .unwrap_or("unavailable"),
        baro.get("inventory")
            .and_then(|i| i.as_array())
            .map(|a| a.len())
            .unwrap_or(0)
    );

    crate::ingest::transport::ensure_wfm_complete()?;

    // ORDERING INVARIANT (see the market.json write below): wfstat-catalog.json
    // is written FIRST, market.json LAST. The two files are each individually
    // atomic (tmp+rename) but the PAIR is not - keep the catalog write ahead of
    // the snapshot write so a reader that catches the gap sees new-catalog +
    // old-market, never the reverse.
    // Reduced from the payload already in hand - no second request.
    let wfstat_slim = wfstat_raw
        .as_ref()
        .and_then(|v| {
            ingest::slim_wfstat_items(v, ingest::WFSTAT_ITEMS_URL)
                .map_err(|e| eprintln!("  warning: {e}"))
                .ok()
        })
        .unwrap_or_default();
    if wfstat_slim.is_empty() && catalog_out.exists() {
        eprintln!(
            "  fetch empty - keeping existing {}",
            catalog_out
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
        );
    } else if !wfstat_slim.is_empty() {
        let tmp = catalog_out.with_extension("json.tmp");
        let slim_json =
            serde_json::to_string(&wfstat_slim).map_err(|e| format!("serialize: {e}"))?;
        std::fs::create_dir_all(catalog_out.parent().unwrap_or(std::path::Path::new(".")))
            .map_err(|e| format!("mkdir: {e}"))?;
        std::fs::write(&tmp, &slim_json).map_err(|e| format!("write tmp: {e}"))?;
        std::fs::rename(&tmp, &catalog_out).map_err(|e| format!("rename: {e}"))?;
        eprintln!(
            "  {} entries → {}",
            wfstat_slim.len(),
            catalog_out
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
        );
    }

    let p2i_old: Option<HashMap<String, serde_json::Value>> = prior
        .get("path_to_info")
        .and_then(|s| serde_json::from_value(s.clone()).ok());
    let s2p_old: Option<HashMap<String, serde_json::Value>> = prior
        .get("set_to_parts")
        .and_then(|s| serde_json::from_value(s.clone()).ok());
    let rr_old: Option<HashMap<String, serde_json::Value>> = prior
        .get("relic_rewards")
        .and_then(|s| serde_json::from_value(s.clone()).ok());
    let vs_old: Option<HashMap<String, String>> = prior
        .get("vault_status")
        .and_then(|s| serde_json::from_value(s.clone()).ok());
    let baro_old: Option<HashMap<String, serde_json::Value>> = prior
        .get("baro")
        .and_then(|s| serde_json::from_value(s.clone()).ok());
    let rivens_old: Option<HashMap<String, serde_json::Value>> = prior
        .get("rivens")
        .and_then(|s| serde_json::from_value(s.clone()).ok());
    let calendar_old: Option<HashMap<String, serde_json::Value>> = prior
        .get("calendar")
        .and_then(|s| serde_json::from_value(s.clone()).ok());
    let riven_stats_old: Option<HashMap<String, serde_json::Value>> = prior
        .get("riven_stats")
        .and_then(|s| serde_json::from_value(s.clone()).ok());
    let recipes_old: Option<HashMap<String, serde_json::Value>> = prior
        .get("recipes")
        .and_then(|s| serde_json::from_value(s.clone()).ok());
    let prior_de = prior.get("de");
    let prior_child_stamps: std::collections::BTreeMap<String, String> = prior_de
        .and_then(|d| d.get("child_fetched_at"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    eprintln!("Fetching prime release/vault dates + Resurgence rotations...");
    let mut calendar = ingest::fetch_calendar(http.as_ref(), wfstat_raw.as_ref(), &catalog);
    let aligned = ingest::align_calendar_with_vault_status(&mut calendar, &vault_status, now);
    if aligned > 0 {
        eprintln!("  {aligned} primes marked vaulted from the fresher vault status");
    }
    eprintln!(
        "  {} primes dated · {} resurgence rotations",
        calendar
            .get("primes")
            .and_then(|p| p.as_object())
            .map(|p| p.len())
            .unwrap_or(0),
        calendar
            .get("resurgence")
            .and_then(|r| r.as_array())
            .map(|r| r.len())
            .unwrap_or(0)
    );

    let rivens::RivenSources { rivens, de_dispositions, disposition_state, riven_stats, riven_stats_children, pc_riven_stats_state } =
        rivens::fetch(http.as_ref(), &de_snap, &prior, &rivens_old, &riven_stats_old, now);

    let path_to_info_for_de = path_to_info.clone();
    let observation = |data: HashMap<String, serde_json::Value>, complete| {
        if data.is_empty() {
            Observation::Unavailable
        } else if complete {
            Observation::usable(data)
        } else {
            Observation::partial(data)
        }
    };
    let mut key_stamps: HashMap<String, KeyStamps> = HashMap::new();
    let (r_p2i, stamps) = reconcile_keyed(
        "path_to_info",
        observation(path_to_info, path_to_info_complete),
        p2i_old.as_ref(),
        prior_stamps.get("path_to_info").map(|s| s.as_str()),
        prior_key_stamps.get("path_to_info"),
        now,
        STALE_DAYS,
    );
    key_stamps.insert("path_to_info".into(), stamps);
    let (r_s2p, stamps) = reconcile_keyed(
        "set_to_parts",
        observation(set_to_parts, set_to_parts_complete),
        s2p_old.as_ref(),
        prior_stamps.get("set_to_parts").map(|s| s.as_str()),
        prior_key_stamps.get("set_to_parts"),
        now,
        STALE_DAYS,
    );
    key_stamps.insert("set_to_parts".into(), stamps);
    let r_rr = reconcile(
        "relic_rewards",
        relic_observation,
        rr_old.as_ref(),
        prior_stamps.get("relic_rewards").map(|s| s.as_str()),
        now,
        STALE_DAYS,
    );
    let vault_observation = if vault_status.is_empty() {
        Observation::Unavailable
    } else if vault_complete {
        Observation::usable(vault_status)
    } else {
        Observation::partial(vault_status)
    };
    let (r_vs, stamps) = reconcile_keyed(
        "vault_status",
        vault_observation,
        vs_old.as_ref(),
        prior_stamps.get("vault_status").map(|s| s.as_str()),
        prior_key_stamps.get("vault_status"),
        now,
        STALE_DAYS,
    );
    key_stamps.insert("vault_status".into(), stamps);
    // Before reconcile, not after: reconcile only falls back to the prior value
    // when the whole surface is empty, and Baro's never is (schedule fields keep
    // arriving between visits). His inventory is capturable only during the 48h
    // he is present, so it has to be carried across explicitly.
    let mut baro = baro;
    ingest::carry_baro_inventory(&mut baro, baro_old.as_ref());
    de_extract::refresh_baro_rows(&mut baro, &path_to_info_for_de, &de_alias);
    let baro_observation = if baro.is_empty() {
        Observation::Unavailable
    } else {
        Observation::usable(baro)
    };
    let r_baro = reconcile(
        "baro",
        baro_observation,
        baro_old.as_ref(),
        prior_stamps.get("baro").map(|s| s.as_str()),
        now,
        STALE_DAYS,
    );
    let r_rivens = reconcile(
        "rivens",
        observation(rivens, true),
        rivens_old.as_ref(),
        prior_stamps.get("rivens").map(|s| s.as_str()),
        now,
        STALE_DAYS,
    );
    let r_calendar = reconcile(
        "calendar",
        observation(calendar, true),
        calendar_old.as_ref(),
        prior_stamps.get("calendar").map(|s| s.as_str()),
        now,
        STALE_DAYS,
    );
    let riven_stats_observation = match pc_riven_stats_state {
        ingest::RivenChildOutcome::Unavailable => Observation::Unavailable,
        ingest::RivenChildOutcome::Invalid => Observation::Invalid,
        ingest::RivenChildOutcome::AuthoritativeEmpty => Observation::AuthoritativeEmpty,
        ingest::RivenChildOutcome::Usable => Observation::usable(riven_stats),
    };
    let r_riven_stats = reconcile(
        "riven_stats",
        riven_stats_observation,
        riven_stats_old.as_ref(),
        prior_stamps.get("riven_stats").map(|s| s.as_str()),
        now,
        STALE_DAYS,
    );
    let r_recipes = reconcile(
        "recipes",
        recipes_observation,
        recipes_old.as_ref(),
        prior_stamps.get("recipes").map(|s| s.as_str()),
        now,
        STALE_DAYS,
    );
    let r_usage = reconcile(
        "usage",
        usage_observation,
        usage_old.as_ref(),
        prior_stamps.get("usage").map(|s| s.as_str()),
        now,
        STALE_DAYS,
    );
    let world::WorldSurfaces { vault: r_world_vault, deals: r_world_deals, goals: r_world_goals, events: r_world_events } =
        world::reconcile_children(&de_world_observation, &path_to_info_for_de, &de_alias, &prior, &prior_child_stamps, now);

    for r in [&r_p2i, &r_s2p, &r_rr] {
        if let Some(w) = &r.stale_warning {
            eprintln!("{}", w.format());
        }
    }
    if let Some(w) = &r_vs.stale_warning {
        eprintln!("{}", w.format());
    }
    if let Some(w) = &r_baro.stale_warning {
        eprintln!("{}", w.format());
    }
    if let Some(w) = &r_rivens.stale_warning {
        eprintln!("{}", w.format());
    }
    if let Some(w) = &r_calendar.stale_warning {
        eprintln!("{}", w.format());
    }
    if let Some(w) = &r_riven_stats.stale_warning {
        eprintln!("{}", w.format());
    }
    if let Some(w) = &r_recipes.stale_warning {
        eprintln!("{}", w.format());
    }

    let mut surface_fetched_at: HashMap<String, String> = HashMap::new();
    surface_fetched_at.insert("path_to_info".into(), r_p2i.fetched_at.clone());
    surface_fetched_at.insert("set_to_parts".into(), r_s2p.fetched_at.clone());
    surface_fetched_at.insert("relic_rewards".into(), r_rr.fetched_at.clone());
    surface_fetched_at.insert("vault_status".into(), r_vs.fetched_at.clone());
    surface_fetched_at.insert("baro".into(), r_baro.fetched_at.clone());
    surface_fetched_at.insert("rivens".into(), r_rivens.fetched_at.clone());
    surface_fetched_at.insert("calendar".into(), r_calendar.fetched_at.clone());
    surface_fetched_at.insert("riven_stats".into(), r_riven_stats.fetched_at.clone());
    surface_fetched_at.insert("recipes".into(), r_recipes.fetched_at.clone());
    surface_fetched_at.insert("usage".into(), r_usage.fetched_at.clone());

    let mut surface_provenance = HashMap::new();
    macro_rules! provenance {
        ($name:literal, $result:expr) => {
            surface_provenance.insert(
                $name.to_string(),
                render::SurfaceProvenance {
                    disposition: $result.disposition,
                    attempted_at: $result.attempted_at.clone(),
                    data_fetched_at: $result.fetched_at.clone(),
                    source: None,
                },
            );
        };
    }
    provenance!("path_to_info", r_p2i);
    provenance!("set_to_parts", r_s2p);
    provenance!("relic_rewards", r_rr);
    provenance!("vault_status", r_vs);
    provenance!("baro", r_baro);
    provenance!("rivens", r_rivens);
    provenance!("calendar", r_calendar);
    provenance!("riven_stats", r_riven_stats);
    provenance!("recipes", r_recipes);
    provenance!("usage", r_usage);
    provenance!("world.vault_rotation", r_world_vault);
    provenance!("world.deals", r_world_deals);
    provenance!("world.goals", r_world_goals);
    provenance!("world.events", r_world_events);
    if let Some(p) = surface_provenance.get_mut("relic_rewards") {
        p.source = Some(relic_source.to_string());
    }
    if let Some(p) = surface_provenance.get_mut("baro") {
        p.source = Some(baro_source.to_string());
    }
    let prior_disposition_stamp = prior_child_stamps
        .get("de.dispositions")
        .cloned()
        .unwrap_or_else(|| clock::iso_z(now));
    let disposition_fetched_at =
        if disposition_state == crate::reconcile::Disposition::PublishedFresh {
            clock::iso_z(now)
        } else {
            prior_disposition_stamp
        };
    surface_provenance.insert(
        "de.dispositions".into(),
        render::SurfaceProvenance {
            disposition: disposition_state,
            attempted_at: clock::iso_z(now),
            data_fetched_at: disposition_fetched_at.clone(),
            source: Some("de_public_export".into()),
        },
    );

    eprintln!("Rendering {} CSV rows...", csv_path.display());
    let rows = csvin::read_csv_rows(&csv_path)?;
    let items = render::render_items(&rows, &meta_by_slug);

    // The DE provenance block. `hashes` is what makes the next cycle cheap -
    // it is compared against the fresh index so unchanged manifests are never
    // refetched. Carried forward verbatim when DE was unreachable, so an
    // outage does not force a full re-download on recovery.
    //
    // The worldState-derived rows are CARRIED on an outage rather than
    // emptied. This block is assigned after `assemble_snapshot`, so it never
    // passes through reconcile and nothing else would preserve it - a single
    // failed poll would silently drop an announced vault rotation, which is
    // exactly the event the feature exists to warn about. `world_ok: false`
    // tells the UI the rows are stale.
    let de_surface = render::DeSurface {
        hashes: if de_snap.hashes.is_empty() {
            prior_de_hashes.clone()
        } else {
            de_snap.hashes.clone()
        },
        changed: de_snap.changed.clone(),
        world_ok: matches!(de_world_observation, Observation::Usable { .. }),
        child_fetched_at: {
            let mut stamps = prior_child_stamps;
            stamps.insert(
                "world.vault_rotation".into(),
                r_world_vault.fetched_at.clone(),
            );
            stamps.insert("world.deals".into(), r_world_deals.fetched_at.clone());
            stamps.insert("world.goals".into(), r_world_goals.fetched_at.clone());
            stamps.insert("world.events".into(), r_world_events.fetched_at.clone());
            stamps.insert("de.dispositions".into(), disposition_fetched_at);
            for (child, outcome) in &riven_stats_children {
                if matches!(
                    outcome,
                    ingest::RivenChildOutcome::Usable
                        | ingest::RivenChildOutcome::AuthoritativeEmpty
                ) {
                    stamps.insert(format!("riven_stats.{child}"), clock::iso_z(now));
                }
            }
            stamps
        },
        vault_rotation: r_world_vault.data,
        deals: r_world_deals.data,
        ducats: de_ducats,
        dispositions: de_dispositions,
    };

    let mut snapshot = assemble_snapshot(
        now,
        catalog,
        items,
        r_p2i.data,
        r_s2p.data,
        r_rr.data,
        r_vs.data,
        r_baro.data,
        r_rivens.data,
        r_calendar.data,
        r_riven_stats.data,
        r_recipes.data,
        r_usage.data,
        usage_history,
        surface_fetched_at,
    );
    snapshot.de = Some(de_surface);
    snapshot.surface_provenance = surface_provenance;
    snapshot.surface_key_fetched_at = key_stamps;
    snapshot.event_rewards = render::EventRewardsSurface {
        goals: r_world_goals.data,
        events: r_world_events.data,
    };

    // market.json is written LAST - it's the generation anchor the browser app
    // joins everything through (items[slug], catalog, path_to_info, baro), while
    // wfstat-catalog.json (written above) is only a fallback resolver that
    // inventory normalization consults AFTER market.path_to_info and that the
    // webview caches in IndexedDB for 24h. A torn read of the non-atomic pair is then
    // always new-catalog + old-market (benign: a superset resolver over a
    // self-consistent older snapshot) rather than new-market + old-catalog
    // (which could leave fresh snapshot rows unresolvable until the catalog
    // lands).
    publish::snapshot(&snapshot, &json_out)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_stale_age_is_the_one_the_calendar_shows() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../../../tests/fixtures/stale-days.json")).unwrap();
        assert_eq!(fixture["stale_days"].as_i64(), Some(super::STALE_DAYS));
    }
}
