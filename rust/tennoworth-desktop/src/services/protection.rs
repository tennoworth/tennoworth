use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::persistence::Db;
use crate::services::sellables::MarketData;

const KEY: &str = "protected-selling-plan-v1";

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProtectionPlan {
    pub reserves: BTreeMap<String, u32>,
    pub goal: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct Allocation {
    pub owned: u32,
    pub protected: u32,
    /// Local protection only; never authorizes posting without current orders.
    pub estimated: Option<u32>,
    pub listed: Option<u32>,
    pub available: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GuidanceInventory {
    pub snapshot_id: Option<i64>,
    pub items: BTreeMap<String, GuidanceOwned>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GuidanceOwned {
    pub count: u32,
    pub leveled: u32,
}

#[derive(Serialize)]
pub struct ProtectionState {
    pub plan: ProtectionPlan,
    pub snapshot_id: Option<i64>,
    pub items: BTreeMap<String, Allocation>,
    pub issues: Vec<String>,
}

impl ProtectionPlan {
    pub fn active(&self, db: &Db) -> Result<bool, String> {
        Ok(self != &Self::default()
            || db
                .get_setting("reserve-copies")
                .map_err(|e| e.to_string())?
                .is_some_and(|value| value.parse::<u32>().map_or(true, |n| n > 0))
            || !db.get_reserves().map_err(|e| e.to_string())?.is_empty())
    }
    pub fn load(db: &Db) -> Result<Self, String> {
        match db.get_setting(KEY).map_err(|e| e.to_string())? {
            Some(raw) => serde_json::from_str(&raw).map_err(|_| {
                "Protected plan could not be read. Review protection before selling.".into()
            }),
            None => Ok(Self::default()),
        }
    }

    pub fn save(&self, db: &Db, market: &MarketData) -> Result<(), String> {
        if self.reserves.len() > 1000
            || self.reserves.iter().any(|(slug, count)| {
                !valid_slug(slug) || *count > 1_000_000 || !market.has_item(slug)
            })
        {
            return Err(
                "Choose valid market items and protection quantities from 0 to 1000000.".into(),
            );
        }
        self.requirements(market)?;
        let raw = serde_json::to_string(self).map_err(|e| e.to_string())?;
        db.set_setting(KEY, &raw).map_err(|e| e.to_string())
    }

    pub fn requirements(&self, market: &MarketData) -> Result<BTreeMap<String, u32>, String> {
        let mut requirements = self.reserves.clone();
        if let Some(goal) = &self.goal {
            for (slug, quantity) in market.set_recipe(goal)? {
                let count = requirements.entry(slug).or_default();
                *count = count
                    .checked_add(quantity)
                    .ok_or("Protected quantity exceeds supported limits.")?;
            }
        }
        Ok(requirements)
    }
}

fn valid_slug(slug: &str) -> bool {
    // Catalog-derived identifiers include punctuation and Unicode. They are map
    // keys; saved reserves additionally require market membership.
    !slug.is_empty() && slug.len() <= 512 && !slug.chars().any(char::is_control)
}

pub fn allocate(
    owned: u32,
    unavailable: u32,
    global_keep: u32,
    reserved: u32,
    listed: Option<u32>,
) -> Allocation {
    let protected = global_keep.max(unavailable.saturating_add(reserved));
    Allocation {
        owned,
        protected,
        estimated: Some(owned.saturating_sub(protected)),
        listed,
        available: listed.map(|listed| owned.saturating_sub(protected).saturating_sub(listed)),
    }
}

/// Expand existing set listings into the same component pool as part listings.
pub fn listed_components(
    body: &serde_json::Value,
    market: &MarketData,
) -> Result<BTreeMap<String, u32>, String> {
    let data = body.get("data").unwrap_or(body);
    let (rows, mixed) = match data.as_array() {
        Some(rows) => (rows, true),
        None => (
            data.get("sell")
                .and_then(|v| v.as_array())
                .ok_or("Current sell orders are unavailable.")?,
            false,
        ),
    };
    let mut listed = BTreeMap::<String, u32>::new();
    for row in rows {
        if mixed && row.get("type").and_then(|v| v.as_str()) == Some("buy") {
            continue;
        }
        if mixed && row.get("type").and_then(|v| v.as_str()) != Some("sell") {
            return Err("An order has an unknown side.".into());
        }
        let slug = row
            .get("item")
            .and_then(|i| i.get("slug"))
            .and_then(|v| v.as_str())
            .ok_or("An existing order has no resolved item identity.")?;
        let quantity = row
            .get("quantity")
            .and_then(|v| v.as_u64())
            .and_then(|n| u32::try_from(n).ok())
            .filter(|n| *n > 0)
            .ok_or("An existing order has an invalid quantity.")?;
        let parts = if slug.ends_with("_set") {
            market.set_recipe(slug)?
        } else {
            BTreeMap::from([(slug.to_owned(), 1)])
        };
        for (part, count) in parts {
            let quantity = quantity
                .checked_mul(count)
                .ok_or("Listed quantity exceeds supported limits.")?;
            let value = listed.entry(part).or_default();
            *value = value
                .checked_add(quantity)
                .ok_or("Listed quantity exceeds supported limits.")?;
        }
    }
    Ok(listed)
}

pub fn state(
    db: &Db,
    market: &MarketData,
    orders: Result<serde_json::Value, String>,
) -> Result<ProtectionState, String> {
    let plan = ProtectionPlan::load(db)?;
    let snapshot_id = db
        .list_snapshots(1)
        .map_err(|e| e.to_string())?
        .first()
        .map(|s| s.id);
    let mut issues = Vec::new();
    let required = match plan.requirements(market) {
        Ok(required) => Some(required),
        Err(error) => {
            issues.push(error);
            None
        }
    };
    let listed = match orders.and_then(|body| listed_components(&body, market)) {
        Ok(listed) => Some(listed),
        Err(error) => {
            issues.push(error);
            None
        }
    };
    if snapshot_id.is_none() {
        issues.push("Scan inventory before allocating copies.".into());
    }
    let global = db
        .get_setting("reserve-copies")
        .map_err(|e| e.to_string())?
        .map(|raw| {
            raw.parse::<u32>()
                .map_err(|_| "The keep-copy setting is invalid.".to_string())
        })
        .transpose()?
        .unwrap_or(0);
    let legacy: BTreeMap<_, _> = db
        .get_reserves()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|r| (r.slug, u32::try_from(r.keep).unwrap_or(u32::MAX)))
        .collect();
    let mut items = BTreeMap::new();
    for (slug, (owned, unavailable)) in market.owned_quantities(db)? {
        let required_count = required
            .as_ref()
            .and_then(|r| r.get(&slug))
            .copied()
            .unwrap_or(0);
        let mut row = allocate(
            owned,
            unavailable,
            global.max(legacy.get(&slug).copied().unwrap_or(0)),
            required_count,
            listed.as_ref().map(|l| l.get(&slug).copied().unwrap_or(0)),
        );
        if required.is_none() || snapshot_id.is_none() {
            row.available = None;
            row.estimated = None;
        }
        if row.protected.saturating_add(row.listed.unwrap_or(0)) > owned {
            issues.push(format!("{slug}: protected and listed copies exceed the current inventory; review existing orders."));
        }
        items.insert(slug, row);
    }
    if let Some(required) = required {
        for (slug, count) in required {
            if count > 0 && !items.contains_key(&slug) {
                issues.push(format!(
                    "{slug}: {count} protected copies are not present in the confirmed inventory."
                ));
                items.insert(
                    slug.clone(),
                    Allocation {
                        owned: 0,
                        protected: count,
                        estimated: snapshot_id.map(|_| 0),
                        listed: listed.as_ref().map(|l| l.get(&slug).copied().unwrap_or(0)),
                        available: snapshot_id.and(listed.as_ref().map(|_| 0)),
                    },
                );
            }
        }
    }
    Ok(ProtectionState {
        plan,
        snapshot_id,
        items,
        issues,
    })
}

/// Advisory input cannot create a trusted snapshot or authorize a mutation.
pub fn guidance_state(
    db: &Db,
    market: &MarketData,
    orders: Result<serde_json::Value, String>,
    inventory: GuidanceInventory,
) -> Result<ProtectionState, String> {
    if inventory.items.len() > 100_000 || inventory.items.iter().any(|(slug, row)| {
        !valid_slug(slug) || row.leveled > row.count
    }) {
        return Err("Inventory quantities are invalid. Scan again or restore a valid backup.".into());
    }
    let plan = ProtectionPlan::load(db)?;
    if validate_snapshot(db, inventory.snapshot_id).is_ok() {
        let native = state(db, market, orders)?;
        let native_owned = market.owned_quantities(db)?;
        let verified = inventory.snapshot_id == native.snapshot_id
            && validate_snapshot(db, inventory.snapshot_id).is_ok()
            && inventory.items.keys().filter(|slug| market.has_item(slug)).count() == native_owned.len()
            && inventory.items.iter().filter(|(slug, _)| market.has_item(slug))
                .all(|(slug, row)| native_owned.get(slug).is_some_and(|(count, _)| *count == row.count));
        if verified { return Ok(native); }
    }
    let mut issues = Vec::new();
    let requirements = match plan.requirements(market) {
        Ok(required) => Some(required),
        Err(error) => { issues.push(error); None }
    };
    let global = db.get_setting("reserve-copies").map_err(|e| e.to_string())?
        .map(|raw| raw.parse::<u32>().map_err(|_| "The keep-copy setting is invalid.".to_string()))
        .transpose()?.unwrap_or(0);
    let legacy: BTreeMap<_, _> = db.get_reserves().map_err(|e| e.to_string())?.into_iter()
        .map(|row| (row.slug, u32::try_from(row.keep).unwrap_or(u32::MAX))).collect();
    let items = inventory.items.into_iter().map(|(slug, owned)| {
        let mut row = allocate(owned.count, owned.leveled,
            global.max(legacy.get(&slug).copied().unwrap_or(0)),
            requirements.as_ref().and_then(|required| required.get(&slug)).copied().unwrap_or(0), None);
        if requirements.is_none() || !market.has_item(&slug) { row.estimated = None; }
        (slug, row)
    }).collect();
    Ok(ProtectionState { plan, snapshot_id: None, items, issues })
}

pub fn validate_snapshot(db: &Db, snapshot_id: Option<i64>) -> Result<(), String> {
    let latest = db.list_snapshots(1).map_err(|e| e.to_string())?.into_iter().next();
    if !latest.is_some_and(|snapshot| snapshot.source == "memory" && Some(snapshot.id) == snapshot_id) {
        return Err("Inventory changed or is not a verified game scan. Scan again and prepare a new batch.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn set_listings_consume_the_same_component_quantities_as_the_selector() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!("../../../../tests/fixtures/trade-session/sets.json")).unwrap();
        let parts: BTreeMap<String, u32> = serde_json::from_value(fixture["parts"].clone()).unwrap();
        let owned: BTreeMap<String, u32> = serde_json::from_value(fixture["owned"].clone()).unwrap();
        let market = serde_json::from_value(serde_json::json!({"items":{"barrel":{},"receiver":{},"blueprint":{}},
            "set_to_parts":{"example_set":{"parts":parts.iter().map(|(slug,count)| serde_json::json!({"slug":slug,"quantity":count})).collect::<Vec<_>>()}}
        })).unwrap();
        let listed = listed_components(&serde_json::json!({"data":{"sell":[{"item":{"slug":"example_set"},"quantity":fixture["expected_sets"]}]}}), &market).unwrap();
        let left: BTreeMap<String, u32> = owned.into_iter().map(|(slug,count)| { let remain = count - listed[&slug]; (slug,remain) }).collect();
        assert_eq!(serde_json::to_value(left).unwrap(), fixture["expected_left"]);
        assert_eq!(parts.values().sum::<u32>(), fixture["trade_slots"].as_u64().unwrap() as u32);
    }
    #[test]
    fn guidance_matches_preview_fixture_and_uses_the_displayed_inventory() {
        #[derive(Deserialize)]
        struct Case {
            name: String, native_count: u32, input_count: u32, leveled: u32,
            reserved: u32, keep: u32, goal: bool, request_id: Option<i64>, source: String,
            orders: bool, record: bool, expected_snapshot_id: Option<i64>, expected: Allocation,
        }
        let cases: Vec<Case> = serde_json::from_str(include_str!("../../../../tests/fixtures/protection/guidance.json")).unwrap();
        let market: MarketData = serde_json::from_value(serde_json::json!({
            "items": { "part": {} },
            "path_to_info": { "/Lotus/Part": { "name": "Part", "slug": "part" } },
            "set_to_parts": { "test_set": { "parts": [{ "slug": "part", "quantity": 2 }] } }
        })).unwrap();
        for case in cases {
            let db = Db::open(std::path::Path::new(":memory:")).unwrap();
            if case.record { db.insert_snapshot(&case.source, None, None, &[crate::persistence::SnapshotItem {
                slug: "/Lotus/Part".into(), count: i64::from(case.native_count), leveled: i64::from(case.leveled),
            }]).unwrap(); }
            db.set_setting("reserve-copies", &case.keep.to_string()).unwrap();
            ProtectionPlan { reserves: BTreeMap::from([("part".into(), case.reserved)]), goal: case.goal.then(|| "test_set".into()) }.save(&db, &market).unwrap();
            let input = GuidanceInventory { snapshot_id: case.request_id, items: BTreeMap::from([("part".into(), GuidanceOwned { count: case.input_count, leveled: case.leveled })]) };
            let orders = if case.orders { Ok(serde_json::json!({"data":{"sell":[]}})) } else { Err("Disconnected".into()) };
            let result = guidance_state(&db, &market, orders, input).unwrap();
            assert_eq!(result.snapshot_id, case.expected_snapshot_id, "{}", case.name);
            assert_eq!(result.items["part"], case.expected, "{}", case.name);
        }
    }

    #[test]
    fn catalog_identifiers_do_not_invalidate_other_inventory_quantities() {
        let db = Db::open(std::path::Path::new(":memory:")).unwrap();
        let slugs = ["part", "zid-an-osbok", "summoner’s_wrath", "la_cathédrale_scene", "nihils_oubliette_(key)"];
        let market: MarketData = serde_json::from_value(serde_json::json!({
            "items": slugs.iter().map(|slug| (*slug, serde_json::json!({}))).collect::<BTreeMap<_, _>>()
        })).unwrap();
        ProtectionPlan { reserves: slugs.iter().map(|slug| ((*slug).into(), 1)).collect(), goal: None }.save(&db, &market).unwrap();
        let input = GuidanceInventory { snapshot_id: None, items: slugs.iter().chain(["unknown/codex_item"].iter())
            .map(|slug| ((*slug).into(), GuidanceOwned { count: 4, leveled: 1 })).collect() };
        let result = guidance_state(&db, &market, Err("Disconnected".into()), input).unwrap();
        for slug in slugs {
            assert_eq!(result.items[slug].estimated, Some(2));
            assert_eq!(result.items[slug].available, None);
        }
        assert_eq!(result.items["unknown/codex_item"].estimated, None);
        assert!(ProtectionPlan { reserves: BTreeMap::from([("unknown/codex_item".into(), 1)]), goal: None }.save(&db, &market).is_err());
        let invalid = GuidanceInventory { snapshot_id: None, items: BTreeMap::from([("part".into(), GuidanceOwned { count: 1, leveled: 2 })]) };
        assert!(guidance_state(&db, &market, Err("Disconnected".into()), invalid).is_err());
    }

    #[test]
    fn non_market_inventory_does_not_break_verified_scan_identity() {
        let db = Db::open(std::path::Path::new(":memory:")).unwrap();
        let market: MarketData = serde_json::from_value(serde_json::json!({
            "items": { "part": {} }, "path_to_info": { "/Lotus/Part": { "name": "Part", "slug": "part" } }
        })).unwrap();
        let id = db.insert_snapshot("memory", None, None, &[crate::persistence::SnapshotItem {
            slug: "/Lotus/Part".into(), count: 4, leveled: 0,
        }]).unwrap();
        let mut input = GuidanceInventory { snapshot_id: Some(id), items: BTreeMap::from([
            ("part".into(), GuidanceOwned { count: 4, leveled: 0 }),
            ("non_market_item".into(), GuidanceOwned { count: 50, leveled: 0 })
        ]) };
        let result = guidance_state(&db, &market, Ok(serde_json::json!({"data":{"sell":[]}})), input.clone()).unwrap();
        assert_eq!(result.snapshot_id, Some(id));
        assert_eq!(result.items["part"].available, Some(4));
        input.items.get_mut("part").unwrap().count = 5;
        let changed = guidance_state(&db, &market, Ok(serde_json::json!({"data":{"sell":[]}})), input).unwrap();
        assert_eq!(changed.snapshot_id, None);
        assert_eq!(changed.items["part"].available, None);
    }

    #[test]
    fn listing_identity_requires_the_latest_memory_scan() {
        let db = Db::open(std::path::Path::new(":memory:")).unwrap();
        assert!(validate_snapshot(&db, None).is_err());
        let first = db.insert_snapshot("memory", None, None, &[]).unwrap();
        assert!(validate_snapshot(&db, Some(first)).is_ok());
        let second = db.insert_snapshot("memory", None, None, &[]).unwrap();
        assert!(validate_snapshot(&db, Some(first)).is_err());
        assert!(validate_snapshot(&db, Some(second)).is_ok());
        let imported = db.insert_snapshot("import", None, None, &[]).unwrap();
        assert!(validate_snapshot(&db, Some(imported)).is_err());
        assert!(validate_snapshot(&db, None).is_err());
    }

    #[test]
    fn allocation_matches_preview_fixture() {
        #[derive(Deserialize)]
        struct Case {
            owned: u32,
            unavailable: u32,
            global_keep: u32,
            reserved: u32,
            listed: Option<u32>,
            expected: Allocation,
        }
        let cases: Vec<Case> = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/protection/allocation.json"
        ))
        .unwrap();
        for row in cases {
            assert_eq!(
                allocate(
                    row.owned,
                    row.unavailable,
                    row.global_keep,
                    row.reserved,
                    row.listed
                ),
                row.expected
            );
        }
    }
    #[test]
    fn named_goals_reserve_unbuilt_copies_in_addition_to_untradeable_items() {
        assert_eq!(allocate(5, 0, 0, 2, Some(1)).available, Some(2));
        assert_eq!(allocate(5, 2, 1, 2, Some(1)).available, Some(0));
        assert_eq!(allocate(5, 1, 3, 0, Some(1)).available, Some(1));
        assert_eq!(allocate(1, 0, 0, 2, Some(0)).available, Some(0));
        assert_eq!(allocate(5, 0, 0, 2, None).available, None);
    }
    #[test]
    fn estimates_require_valid_local_protection_but_not_current_orders() {
        let db = Db::open(std::path::Path::new(":memory:")).unwrap();
        let market: MarketData = serde_json::from_value(serde_json::json!({
            "items": { "test_part": {} },
            "path_to_info": { "/Lotus/Test": { "name": "Test Part", "slug": "test_part" } }
        })).unwrap();
        let plan = ProtectionPlan { reserves: BTreeMap::from([("test_part".into(), 2)]), goal: None };
        plan.save(&db, &market).unwrap();
        let missing = state(&db, &market, Err("Disconnected".into())).unwrap();
        assert_eq!(missing.items["test_part"].estimated, None);
        db.insert_snapshot("memory", None, None, &[crate::persistence::SnapshotItem {
            slug: "/Lotus/Test".into(), count: 7, leveled: 2,
        }]).unwrap();
        db.set_setting("reserve-copies", "1").unwrap();
        let offline = state(&db, &market, Err("Disconnected".into())).unwrap();
        assert_eq!(offline.items["test_part"].estimated, Some(3));
        assert_eq!(offline.items["test_part"].available, None);
        assert_eq!(offline.items["test_part"].listed, None);
        db.set_setting(KEY, r#"{"reserves":{},"goal":"unknown_set"}"#).unwrap();
        let invalid = state(&db, &market, Ok(serde_json::json!({ "sell_orders": [] }))).unwrap();
        assert_eq!(invalid.items["test_part"].estimated, None);
        assert_eq!(invalid.items["test_part"].available, None);
        db.set_setting("reserve-copies", "bad").unwrap();
        assert!(state(&db, &market, Err("Disconnected".into())).is_err());
    }

    #[test]
    fn corrupt_protection_is_not_an_empty_plan() {
        let db = Db::open(std::path::Path::new(":memory:")).unwrap();
        assert_eq!(
            ProtectionPlan::load(&db).unwrap(),
            ProtectionPlan::default()
        );
        db.set_setting(KEY, "{bad").unwrap();
        assert!(ProtectionPlan::load(&db).is_err());
    }
}
