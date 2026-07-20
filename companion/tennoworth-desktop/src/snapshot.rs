//! Turn a raw DE inventory payload (the exact bytes the scan / a dropped file
//! produces) into aggregated `SnapshotItem` rows for the history tables.
//!
//! This mirrors `flattenInventory()` in prototype/src/lib/inventory.ts: walk the
//! same tradeable categories, key by the DE item path, sum counts, and count
//! copies DE has flagged untradeable (XP > 0) as `leveled`. It deliberately does
//! NOT resolve the path to a WFM slug — resolution needs the wfstat catalog,
//! which is a client concern (prototype/src/lib/resolver.ts is its sole owner)
//! and out of wfm-core's scope. The stable DE path is stored as the slug; a
//! later join step can map it to a WFM slug when history is surfaced.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::db::SnapshotItem;

/// The categories flatten walks — kept 1:1 with `TRADEABLE_CATEGORIES` in
/// inventory.ts. Stack categories (MiscItems, Recipes, RawUpgrades) carry
/// `ItemCount` and no XP; instance categories (Suits, LongGuns, …) are one
/// entry per owned copy with its own XP.
const TRADEABLE_CATEGORIES: &[&str] = &[
    "MiscItems",
    "Recipes",
    "RawUpgrades",
    "Suits",
    "LongGuns",
    "Pistols",
    "Melee",
    "SpaceGuns",
    "SpaceMelee",
    "Sentinels",
    "SentinelWeapons",
];

/// Parse `inventory_json` and aggregate tradeable items by DE path. Result is
/// sorted by path (BTreeMap) for deterministic snapshots. A non-array or absent
/// category is skipped; entries without a path are skipped — matching the TS
/// walker's leniency.
pub fn extract_items(inventory_json: &[u8]) -> serde_json::Result<Vec<SnapshotItem>> {
    let root: Value = serde_json::from_slice(inventory_json)?;
    // path -> (count, leveled)
    let mut agg: BTreeMap<String, (i64, i64)> = BTreeMap::new();

    for cat in TRADEABLE_CATEGORIES {
        let Some(entries) = root.get(cat).and_then(Value::as_array) else {
            continue;
        };
        for e in entries {
            let path = e
                .get("ItemType")
                .and_then(Value::as_str)
                .or_else(|| e.get("Type").and_then(Value::as_str));
            let Some(path) = path else { continue };
            // ItemCount defaults to 1 (instance categories omit it).
            let count = e.get("ItemCount").and_then(Value::as_i64).unwrap_or(1);
            let xp = e.get("XP").and_then(Value::as_i64).unwrap_or(0);
            let slot = agg.entry(path.to_string()).or_insert((0, 0));
            slot.0 += count;
            // XP > 0 means DE flagged this copy untradeable; accumulate the same
            // way the SPA does (`rec.leveled += count`).
            if xp > 0 {
                slot.1 += count;
            }
        }
    }

    Ok(agg
        .into_iter()
        .map(|(slug, (count, leveled))| SnapshotItem {
            slug,
            count,
            leveled,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregates_counts_and_leveled_across_categories() {
        let json = br#"{
          "MiscItems": [
            {"ItemCount": 356872, "ItemType": "/Lotus/Types/Items/MiscItems/AlloyPlate"},
            {"ItemCount": 3, "ItemType": "/Lotus/Part"}
          ],
          "Suits": [
            {"ItemType": "/Lotus/Excalibur", "XP": 3903870},
            {"ItemType": "/Lotus/Excalibur", "XP": 0}
          ],
          "Consumables": [
            {"ItemCount": 99, "ItemType": "/Lotus/ShouldBeIgnored"}
          ]
        }"#;
        let items = extract_items(json).unwrap();
        // Sorted by path; Consumables is not a tradeable category → excluded.
        let by: std::collections::HashMap<_, _> =
            items.iter().map(|i| (i.slug.as_str(), (i.count, i.leveled))).collect();
        assert_eq!(by.len(), 3);
        assert_eq!(by["/Lotus/Types/Items/MiscItems/AlloyPlate"], (356872, 0));
        assert_eq!(by["/Lotus/Part"], (3, 0));
        // Two Excalibur instances aggregate to count 2; one has XP>0 → leveled 1.
        assert_eq!(by["/Lotus/Excalibur"], (2, 1));
    }

    #[test]
    fn missing_and_malformed_categories_are_skipped() {
        // No tradeable categories at all → empty (the "not an inventory" case).
        assert!(extract_items(br#"{"Foo": 1}"#).unwrap().is_empty());
        // A category that isn't an array is skipped rather than erroring.
        assert!(extract_items(br#"{"MiscItems": "nope"}"#).unwrap().is_empty());
        // Entry without a path is skipped.
        let items = extract_items(br#"{"MiscItems": [{"ItemCount": 5}]}"#).unwrap();
        assert!(items.is_empty());
    }

    #[test]
    fn invalid_json_is_an_error() {
        assert!(extract_items(b"not json").is_err());
    }
}
