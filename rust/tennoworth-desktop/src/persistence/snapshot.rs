//! Turn a raw DE inventory payload (the exact bytes the scan / a dropped file
//! produces) into aggregated `SnapshotItem` rows for the history tables.
//!
//! The portable inventory walker supplies the same path/count/XP facts used by
//! the sell table. History keeps stable DE paths so later market revisions can
//! resolve them without rewriting past snapshots.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::persistence::SnapshotItem;

/// Aggregate the validated inventory facts by DE path for deterministic history.
pub fn extract_items(inventory_json: &[u8]) -> serde_json::Result<Vec<SnapshotItem>> {
    let root: Value = serde_json::from_slice(inventory_json)?;
    // path -> (count, leveled)
    let mut agg: BTreeMap<String, (i64, i64)> = BTreeMap::new();

    let rows = market_domain::inventory::flatten_inventory(&root)
        .map_err(<serde_json::Error as serde::de::Error>::custom)?;
    for row in rows {
        let slot = agg.entry(row.path).or_insert((0, 0));
        let count = row.count as i64;
        slot.0 = slot.0.saturating_add(count);
        if row.xp > 0.0 { slot.1 = slot.1.saturating_add(count); }
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
        let by: std::collections::HashMap<_, _> = items
            .iter()
            .map(|i| (i.slug.as_str(), (i.count, i.leveled)))
            .collect();
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
        assert!(extract_items(br#"{"MiscItems": "nope"}"#)
            .unwrap()
            .is_empty());
        // Entry without a path is skipped.
        let items = extract_items(br#"{"MiscItems": [{"ItemCount": 5}]}"#).unwrap();
        assert!(items.is_empty());
    }

    #[test]
    fn invalid_json_is_an_error() {
        assert!(extract_items(b"not json").is_err());
    }

    // Parity gate: frontend/src/lib/inventory.ts walks the same DE categories
    // to build the sell table. Both sides read
    // tests/fixtures/tradeable-categories.json so a category added on one side
    // and forgotten on the other fails CI instead of silently under-counting.
    #[test]
    fn tradeable_categories_matches_the_shared_fixture() {
        #[derive(serde::Deserialize)]
        struct Fixture {
            categories: Vec<String>,
        }
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/tradeable-categories.json"
        );
        let raw = std::fs::read_to_string(path).expect("read the shared category fixture");
        let fx: Fixture = serde_json::from_str(&raw).expect("parse the category fixture");

        let mut got: Vec<&str> = market_domain::inventory::TRADEABLE_CATEGORIES.to_vec();
        got.sort_unstable();
        let mut want: Vec<&str> = fx.categories.iter().map(String::as_str).collect();
        want.sort_unstable();
        assert_eq!(got, want);
    }
}
