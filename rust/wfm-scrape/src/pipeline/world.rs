use super::STALE_DAYS;
use crate::reconcile::{reconcile, Observation, Reconciled};
use crate::{clock, de, de_extract};
use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, HashMap};

pub(super) struct WorldSurfaces {
    pub(super) vault: Reconciled<Vec<serde_json::Value>>,
    pub(super) deals: Reconciled<Vec<serde_json::Value>>,
    pub(super) goals: Reconciled<BTreeMap<String, serde_json::Value>>,
    pub(super) events: Reconciled<BTreeMap<String, serde_json::Value>>,
}

pub(super) fn reconcile_children(
    de_world_observation: &Observation<serde_json::Value>,
    path_to_info_for_de: &HashMap<String, serde_json::Value>,
    de_alias: &HashMap<String, String>,
    prior: &serde_json::Value,
    prior_child_stamps: &BTreeMap<String, String>,
    now: DateTime<Utc>,
) -> WorldSurfaces {
    let de_world = match de_world_observation {
        Observation::Usable { data, .. } => Some(data),
        _ => None,
    };
    let prior_de = prior.get("de");
    let vault_rotation_old: Option<Vec<serde_json::Value>> = prior_de
        .and_then(|d| d.get("vault_rotation"))
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    let deals_old: Option<Vec<serde_json::Value>> = prior_de
        .and_then(|d| d.get("deals"))
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    let goals_old: Option<std::collections::BTreeMap<String, serde_json::Value>> = prior
        .get("event_rewards")
        .and_then(|v| v.get("goals"))
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    let events_old: Option<std::collections::BTreeMap<String, serde_json::Value>> = prior
        .get("event_rewards")
        .and_then(|v| v.get("events"))
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    let fresh_vault_rotation = de_world
        .as_ref()
        .map(|w| {
            de_extract::vault_rotation_from_world(w, |ms| clock::iso_z(clock::from_millis(ms)))
        })
        .unwrap_or_default();
    let fresh_deals = de_world
        .as_ref()
        .map(|w| {
            de_extract::deals_from_world(w, path_to_info_for_de, de_alias, |ms| {
                clock::iso_z(clock::from_millis(ms))
            })
        })
        .unwrap_or_default();
    let world_vault_observation = match de_world_observation {
        Observation::Unavailable => Observation::Unavailable,
        Observation::Invalid => Observation::Invalid,
        Observation::Usable { .. } => {
            de::world_array_observation(de_world, "PrimeVaultTraders", fresh_vault_rotation)
        }
        Observation::Unchanged | Observation::AuthoritativeEmpty => Observation::Invalid,
    };
    let world_deals_observation = match de_world_observation {
        Observation::Unavailable => Observation::Unavailable,
        Observation::Invalid => Observation::Invalid,
        Observation::Usable { .. } => {
            de::world_array_observation(de_world, "DailyDeals", fresh_deals)
        }
        Observation::Unchanged | Observation::AuthoritativeEmpty => Observation::Invalid,
    };
    let r_world_vault = reconcile(
        "world.vault_rotation",
        world_vault_observation,
        vault_rotation_old.as_ref(),
        prior_child_stamps
            .get("world.vault_rotation")
            .map(|s| s.as_str()),
        now,
        STALE_DAYS,
    );
    let r_world_deals = reconcile(
        "world.deals",
        world_deals_observation,
        deals_old.as_ref(),
        prior_child_stamps.get("world.deals").map(|s| s.as_str()),
        now,
        STALE_DAYS,
    );
    let goal_build = de_world
        .map(|world| {
            de_extract::event_rewards_from_world_child(
                world,
                "Goals",
                path_to_info_for_de,
                de_alias,
                |ms| clock::iso_z(clock::from_millis(ms)),
            )
        })
        .unwrap_or_default();
    let event_build = de_world
        .map(|world| {
            de_extract::event_rewards_from_world_child(
                world,
                "Events",
                path_to_info_for_de,
                de_alias,
                |ms| clock::iso_z(clock::from_millis(ms)),
            )
        })
        .unwrap_or_default();
    let child_observation = |key: &str, build: de_extract::EventRewardBuild| {
        match de_world_observation {
            Observation::Unavailable => Observation::Unavailable,
            Observation::Invalid => Observation::Invalid,
            Observation::Usable { data: world, .. } => {
                match world.get(key).and_then(|v| v.as_array()) {
                    None => Observation::Invalid,
                    Some(rows) if rows.is_empty() => Observation::AuthoritativeEmpty,
                    Some(_) if build.rows.is_empty() => Observation::Invalid,
                    // A child is the freshness unit. Stamping retained rows fresh
                    // after any malformed sibling would overstate what this poll
                    // established, so mixed payloads preserve the whole prior.
                    Some(_) if build.invalid_rows > 0 => Observation::Invalid,
                    Some(_) => Observation::usable(build.rows),
                }
            }
            Observation::Unchanged | Observation::AuthoritativeEmpty => Observation::Invalid,
        }
    };
    let r_world_goals = reconcile(
        "world.goals",
        child_observation("Goals", goal_build),
        goals_old.as_ref(),
        prior_child_stamps.get("world.goals").map(|s| s.as_str()),
        now,
        STALE_DAYS,
    );
    let r_world_events = reconcile(
        "world.events",
        child_observation("Events", event_build),
        events_old.as_ref(),
        prior_child_stamps.get("world.events").map(|s| s.as_str()),
        now,
        STALE_DAYS,
    );

    WorldSurfaces {
        vault: r_world_vault,
        deals: r_world_deals,
        goals: r_world_goals,
        events: r_world_events,
    }
}
