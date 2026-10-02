use crate::ingest::{self, Http, RivenChildOutcome};
use crate::reconcile::Disposition;
use crate::{clock, de, de_extract};
use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, HashMap};

pub(super) struct RivenSources {
    pub(super) rivens: HashMap<String, serde_json::Value>,
    pub(super) de_dispositions: BTreeMap<String, f64>,
    pub(super) disposition_state: Disposition,
    pub(super) riven_stats: HashMap<String, serde_json::Value>,
    pub(super) riven_stats_children: HashMap<String, RivenChildOutcome>,
    pub(super) pc_riven_stats_state: RivenChildOutcome,
}

pub(super) fn fetch(
    http: &dyn Http,
    de_snap: &de::DeSnapshot,
    prior: &serde_json::Value,
    rivens_old: &Option<HashMap<String, serde_json::Value>>,
    riven_stats_old: &Option<HashMap<String, serde_json::Value>>,
    now: DateTime<Utc>,
) -> RivenSources {
    eprintln!("Fetching riven dispositions...");
    let mut rivens = ingest::fetch_rivens(http);
    // DE is the authority on dispositions; warframe.market mirrors them and
    // lags. Overriding here (rather than replacing the fetch) keeps WFM's
    // group/riven_type/req_mr metadata, which DE does not publish, and lets
    // the existing 90-day change log diff against the authoritative value.
    // Applied only when the weapons manifest actually came through this cycle.
    // Same rule again: a weapons manifest that parses but yields no
    // dispositions must not silently leave every value at warframe.market's
    // lagging mirror. When there is nothing fresh, re-apply the prior
    // snapshot's - which were DE's - so the override survives.
    let de_dispos = de_snap
        .manifests
        .get("ExportWeapons_en.json")
        .map(de_extract::dispositions_from_weapons)
        .unwrap_or_default();
    let prior_de_dispositions: std::collections::BTreeMap<String, f64> = prior
        .get("de")
        .and_then(|d| d.get("dispositions"))
        .and_then(|m| m.as_object())
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| v.as_f64().map(|n| (k.clone(), n)))
                .collect()
        })
        .unwrap_or_default();
    let mut joined_dispositions = std::collections::BTreeMap::new();
    if let Some(map) = rivens.get("weapons").and_then(|w| w.as_object()) {
        for (slug, row) in map {
            let Some(name) = row
                .get("name")
                .and_then(|n| n.as_str())
                .map(|n| n.to_lowercase())
            else {
                continue;
            };
            if let Some(value) = de_dispos.get(&name) {
                joined_dispositions.insert(slug.clone(), *value);
            }
        }
    }
    let prior_coverage_ok = prior_de_dispositions.is_empty()
        || joined_dispositions.len() * 100 >= prior_de_dispositions.len() * 80;
    let joined_usable = !de_dispos.is_empty()
        && !joined_dispositions.is_empty()
        && joined_dispositions.len() * 100 >= de_dispos.len() * 80
        && prior_coverage_ok;
    let disposition_state = match de_snap.outcome("ExportWeapons_en.json") {
        de::ManifestOutcome::Usable if joined_usable => {
            crate::reconcile::Disposition::PublishedFresh
        }
        de::ManifestOutcome::Usable | de::ManifestOutcome::Invalid => {
            crate::reconcile::Disposition::PreservedInvalid
        }
        de::ManifestOutcome::Unchanged => crate::reconcile::Disposition::PreservedUnchanged,
        de::ManifestOutcome::Unavailable => crate::reconcile::Disposition::PreservedUnavailable,
    };
    if !joined_usable && !de_dispos.is_empty() {
        eprintln!("  warning: {}/{} DE dispositions joined (prior exact set {}) - preserving prior exact provenance", joined_dispositions.len(), de_dispos.len(), prior_de_dispositions.len());
    }
    let mut de_dispositions = if joined_usable {
        joined_dispositions
    } else {
        std::collections::BTreeMap::new()
    };
    if !joined_usable {
        let mut carried = 0usize;
        if let Some(map) = rivens.get_mut("weapons").and_then(|w| w.as_object_mut()) {
            for (slug, prior_dispo) in &prior_de_dispositions {
                if let Some(row) = map.get_mut(slug) {
                    if let Some(obj) = row.as_object_mut() {
                        obj.insert("disposition".into(), serde_json::json!(*prior_dispo));
                        de_dispositions.insert(slug.clone(), *prior_dispo);
                        carried += 1;
                    }
                }
            }
        }
        eprintln!(
            "  dispositions: none from DE this cycle - carried {carried} from the prior snapshot"
        );
    } else {
        let mut moved = 0usize;
        if let Some(map) = rivens.get_mut("weapons").and_then(|w| w.as_object_mut()) {
            for (slug, de_dispo) in &de_dispositions {
                let Some(row) = map.get_mut(slug) else {
                    continue;
                };
                let was = row.get("disposition").and_then(|d| d.as_f64());
                if was != Some(*de_dispo) {
                    moved += 1;
                }
                if let Some(obj) = row.as_object_mut() {
                    obj.insert("disposition".into(), serde_json::json!(de_dispo));
                }
            }
        }
        eprintln!(
            "  dispositions: {} matched to DE ({moved} differed from WFM's mirror)",
            de_dispositions.len()
        );
    }
    // The change log LAST, diffing what we are about to publish against what we
    // published before. Computing it inside the fetch made it diff WFM's
    // mirror against DE's stored values, which logged a phantom change every
    // time the mirror lagged and missed every real one, because the override
    // lands after the fetch.
    if let Some(weapons) = rivens.get("weapons").and_then(|w| w.as_object()).cloned() {
        let changes = ingest::riven_change_log(&weapons, rivens_old.as_ref(), now);
        if !changes.is_empty() {
            rivens.insert("changes".into(), serde_json::Value::Array(changes));
        }
    }
    let rivens = rivens;
    if let Some(ch) = rivens.get("changes").and_then(|c| c.as_array()) {
        let today = ch
            .iter()
            .filter(|c| {
                c.get("seen_at").and_then(|s| s.as_str()) == Some(clock::iso_z(now).as_str())
            })
            .count();
        eprintln!(
            "  {} weapons · {} changes in log ({} new this run)",
            rivens
                .get("weapons")
                .and_then(|w| w.as_object())
                .map(|w| w.len())
                .unwrap_or(0),
            ch.len(),
            today
        );
    }

    eprintln!("Fetching DE weekly riven stats...");
    // DE names the weapons by display name; the riven-weapons manifest already
    // fetched above maps those to slugs - no second request.
    let weapons_by_name: HashMap<String, String> = rivens
        .get("weapons")
        .and_then(|w| w.as_object())
        .map(|w| {
            w.iter()
                .filter_map(|(slug, row)| {
                    row.get("name")
                        .and_then(|n| n.as_str())
                        .map(|n| (n.to_lowercase(), slug.clone()))
                })
                .collect()
        })
        .unwrap_or_default();
    let (mut riven_stats, unmatched_stats, riven_stats_children) =
        ingest::fetch_riven_stats(http, &weapons_by_name);
    let pc_riven_stats_state = riven_stats_children
        .get("pc")
        .copied()
        .unwrap_or(ingest::RivenChildOutcome::Unavailable);
    if pc_riven_stats_state == ingest::RivenChildOutcome::Usable {
        ingest::carry_failed_riven_platforms(
            &mut riven_stats,
            riven_stats_old.as_ref(),
            &riven_stats_children,
        );
    }
    eprintln!(
        "  {} weapons · {unmatched_stats} DE rows without a WFM slug",
        riven_stats.len()
    );

    RivenSources {
        rivens,
        de_dispositions,
        disposition_state,
        riven_stats,
        riven_stats_children,
        pc_riven_stats_state,
    }
}
