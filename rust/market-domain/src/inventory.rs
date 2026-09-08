use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use ts_rs::TS;

pub const TRADEABLE_CATEGORIES: &[&str] = &[
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

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct CatalogItem {
    pub name: String,
    pub category: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct InventoryPathInfo {
    pub name: String,
    pub slug: String,
    pub category: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
pub struct InventoryMarket {
    #[serde(default)]
    pub catalog: BTreeMap<String, String>,
    #[serde(default)]
    pub path_to_info: BTreeMap<String, InventoryPathInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct InventoryRequest {
    #[ts(type = "unknown")]
    pub inventory: Value,
    pub catalog: Vec<(String, CatalogItem)>,
    pub market: InventoryMarket,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
pub struct OwnedItem {
    pub count: f64,
    pub name: String,
    #[serde(rename = "type")]
    pub item_type: String,
    pub slug: String,
    pub subtype: Option<String>,
    pub kept_lvl: Option<f64>,
    pub leveled: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
pub struct NormalizedInventory {
    pub owned: Vec<(String, OwnedItem)>,
    #[ts(type = "Record<string, number>")]
    pub unresolved: BTreeMap<String, u32>,
    pub flat_count: u32,
}

pub struct FlatInventoryEntry {
    pub category: String,
    pub path: String,
    pub count: f64,
    pub xp: f64,
}

pub fn flatten_inventory(root: &Value) -> Result<Vec<FlatInventoryEntry>, String> {
    if !root.is_object() {
        return Err("Inventory must be an object".into());
    }
    let mut out = Vec::new();
    for category in TRADEABLE_CATEGORIES {
        let Some(entries) = root.get(category).and_then(Value::as_array) else {
            continue;
        };
        for entry in entries {
            let path_value = entry
                .get("ItemType")
                .filter(|v| !v.is_null())
                .or_else(|| entry.get("Type"));
            let Some(path) = path_value.and_then(Value::as_str).filter(|p| !p.is_empty()) else {
                continue;
            };
            let count = match entry.get("ItemCount").filter(|v| !v.is_null()) {
                None => 1.0,
                Some(v) => v
                    .as_f64()
                    .filter(|n| {
                        n.is_finite()
                            && *n >= 0.0
                            && n.fract() == 0.0
                            && *n <= 9_007_199_254_740_991.0
                    })
                    .ok_or_else(|| {
                        "Inventory count must be a nonnegative safe integer".to_string()
                    })?,
            };
            let xp = entry.get("XP").and_then(Value::as_f64).unwrap_or(0.0);
            out.push(FlatInventoryEntry {
                category: category.to_string(),
                path: path.into(),
                count,
                xp,
            });
        }
    }
    Ok(out)
}

fn kept_levels(root: &Value) -> BTreeMap<String, f64> {
    let mut out = BTreeMap::<String, f64>::new();
    for entry in root
        .get("Upgrades")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(path) = entry
            .get("ItemType")
            .and_then(Value::as_str)
            .filter(|p| !p.is_empty())
        else {
            continue;
        };
        let level = entry
            .get("UpgradeFingerprint")
            .and_then(Value::as_str)
            .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
            .and_then(|v| v.get("lvl").and_then(Value::as_f64))
            .unwrap_or(0.0);
        if level > out.get(path).copied().unwrap_or(-1.0) {
            out.insert(path.into(), level);
        }
    }
    out
}

fn resolve(
    path: &str,
    catalog: &BTreeMap<&str, &CatalogItem>,
    market: &InventoryMarket,
) -> Option<(String, String, Option<String>, Option<String>)> {
    if let Some(info) = market.path_to_info.get(path) {
        return Some((
            info.name.clone(),
            info.slug.clone(),
            info.category.clone(),
            None,
        ));
    }
    let info = catalog.get(path).copied().or_else(|| {
        ["Component", "Blueprint"].iter().find_map(|suffix| {
            path.strip_suffix(suffix)
                .and_then(|p| catalog.get(p).copied())
        })
    });
    let Some(info) = info else {
        let name = path_name_guess(path)?;
        let slug = market
            .catalog
            .get(&name.to_lowercase())
            .filter(|s| !s.is_empty())?;
        return Some((name, slug.clone(), None, None));
    };
    if let Some((base, refinement)) = info.name.rsplit_once(' ') {
        if ["Intact", "Exceptional", "Flawless", "Radiant"].contains(&refinement) {
            if let Some(slug) = market
                .catalog
                .get(&format!("{} relic", base.to_lowercase()))
                .filter(|s| !s.is_empty())
            {
                return Some((
                    format!("{base} Relic ({refinement})"),
                    slug.clone(),
                    Some("Relics".into()),
                    Some(refinement.to_lowercase()),
                ));
            }
        }
    }
    let slug = market
        .catalog
        .get(&info.name.to_lowercase())
        .cloned()
        .unwrap_or_else(|| slug_guess(&info.name));
    Some((info.name.clone(), slug, info.category.clone(), None))
}

pub fn normalize_inventory(request: InventoryRequest) -> Result<NormalizedInventory, String> {
    let flat = flatten_inventory(&request.inventory)?;
    let kept = kept_levels(&request.inventory);
    let catalog = request
        .catalog
        .iter()
        .map(|(path, info)| (path.as_str(), info))
        .collect();
    let mut output = NormalizedInventory {
        owned: Vec::new(),
        unresolved: BTreeMap::new(),
        flat_count: u32::try_from(flat.len()).map_err(|_| "Inventory has too many rows")?,
    };
    let mut positions = BTreeMap::new();
    for item in flat {
        let resolved = resolve(&item.path, &catalog, &request.market)
            .filter(|(_, slug, _, _)| !slug.is_empty());
        let Some((name, slug, category, subtype)) = resolved else {
            *output.unresolved.entry(item.category).or_default() += 1;
            continue;
        };
        let key = format!("{}|{}", slug, subtype.as_deref().unwrap_or(""));
        let index = *positions.entry(key.clone()).or_insert_with(|| {
            let index = output.owned.len();
            output.owned.push((
                key,
                OwnedItem {
                    count: 0.0,
                    name,
                    slug,
                    item_type: category.filter(|c| !c.is_empty()).unwrap_or(item.category),
                    subtype,
                    kept_lvl: None,
                    leveled: 0.0,
                },
            ));
            index
        });
        let Some((_, row)) = output.owned.get_mut(index) else {
            return Err("Inventory aggregation failed".into());
        };
        row.count += item.count;
        if row.count > 9_007_199_254_740_991.0 {
            return Err("Inventory total exceeds safe integer range".into());
        }
        if item.xp > 0.0 {
            row.leveled += item.count;
        }
        if let Some(level) = kept.get(&item.path) {
            if row.kept_lvl.is_none_or(|previous| *level > previous) {
                row.kept_lvl = Some(*level);
            }
        }
    }
    Ok(output)
}

pub fn path_name_guess(path: &str) -> Option<String> {
    let mut base = path.rsplit('/').next().unwrap_or("");
    for suffix in ["Blueprint", "Component"] {
        if let Some(trimmed) = base.strip_suffix(suffix) {
            base = trimmed;
        }
    }
    if base.is_empty() {
        return None;
    }
    Some(decamel(base))
}

/// Insert a space between a lowercase/digit and an uppercase letter, matching
/// resolver.ts's `/([a-z0-9])([A-Z])/g` → `$1 $2`.
fn decamel(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len() + 4);
    for (i, &c) in chars.iter().enumerate() {
        if i > 0 && c.is_ascii_uppercase() {
            let prev = chars.get(i.saturating_sub(1)).copied().unwrap_or_default();
            if prev.is_ascii_lowercase() || prev.is_ascii_digit() {
                out.push(' ');
            }
        }
        out.push(c);
    }
    out
}

/// `slugGuess` from resolver.ts: strip non-alphanumerics (keep spaces), trim,
/// lowercase, collapse whitespace to underscores.
pub fn slug_guess(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == ' ' {
                c
            } else {
                ' '
            }
        })
        .collect::<String>();
    // Collapse runs of whitespace to single underscores; trims ends implicitly
    // (split_whitespace drops leading/trailing/empty tokens).
    cleaned
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("_")
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Deserialize)]
    struct Case {
        name: String,
        request: InventoryRequest,
        expected: NormalizedInventory,
    }
    #[derive(Deserialize)]
    struct Fixture {
        cases: Vec<Case>,
    }
    #[test]
    fn invalid_counts_do_not_become_sellable_copies() {
        for count in [
            serde_json::json!(-1),
            serde_json::json!(1.5),
            serde_json::json!("3"),
            serde_json::json!(9_007_199_254_740_992_u64),
        ] {
            assert!(flatten_inventory(
                &serde_json::json!({"MiscItems":[{"ItemType":"/Lotus/Part","ItemCount":count}]})
            )
            .is_err());
        }
        assert!(flatten_inventory(&Value::Null).is_err());
    }

    #[test]
    fn matches_shared_frontend_fixture() {
        let fixture: Fixture = serde_json::from_str(include_str!(
            "../../../tests/fixtures/inventory-normalization/cases.json"
        ))
        .unwrap();
        for case in fixture.cases {
            assert_eq!(
                normalize_inventory(case.request).unwrap(),
                case.expected,
                "{}",
                case.name
            );
        }
    }
}
