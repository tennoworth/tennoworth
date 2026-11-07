use std::collections::{BTreeMap, HashMap};

use crate::persistence::Db;

use super::{MarketData, OverlayCatalogItem, OverlayMarketFacts};

/// What the reward screen shows for the Forma Blueprint, which warframe.market
/// does not list. Each word is the game's own (its exported item names):
/// "Forma" in Latin script and Chinese, Форма in Russian and Ukrainian, フォーマ
/// and 포르마. Matching takes whole words, so in a spaced script the word alone
/// finds the title in either order ("Forma (Schéma)", "Форма (Чертёж)").
/// Japanese and Chinese write no spaces between words, so they carry the whole
/// name, with the game's word for blueprint after the item as on every other
/// blueprint.
const FORMA_BLUEPRINT_NAMES: &[(&str, &str)] = &[
    ("de", "Forma"),
    ("es", "Forma"),
    ("fr", "Forma"),
    ("it", "Forma"),
    ("pl", "Forma"),
    ("pt", "Forma"),
    ("tr", "Forma"),
    ("ru", "Форма"),
    ("uk", "Форма"),
    ("ko", "포르마"),
    ("ja", "フォーマ設計図"),
    ("zh-hans", "Forma 蓝图"),
    ("zh-hant", "Forma 藍圖"),
];

impl MarketData {
    /// The names a reward screen can show. `language` is a WFM language code;
    /// its localized names are added beside the English ones, which stay as
    /// candidates because a localized client still shows some English text.
    pub fn overlay_catalog(&self, language: Option<&str>) -> Vec<OverlayCatalogItem> {
        let mut catalog = self.english_overlay_catalog();
        let Some(language) = language.filter(|language| *language != "en") else {
            return catalog;
        };
        let localized = self.reward_names.get(language);
        let mut seen: std::collections::BTreeSet<String> = catalog
            .iter()
            .map(|item| item.name.to_lowercase())
            .collect();
        for (slug, name) in localized.map(|entry| &entry.names).into_iter().flatten() {
            let name = name.trim();
            if !name.is_empty() && seen.insert(name.to_lowercase()) {
                catalog.push(OverlayCatalogItem {
                    name: name.to_string(),
                    slug: Some(slug.clone()),
                });
            }
        }
        if let Some((_, name)) = FORMA_BLUEPRINT_NAMES
            .iter()
            .find(|(code, _)| *code == language)
        {
            if seen.insert(name.to_lowercase()) {
                catalog.push(OverlayCatalogItem {
                    name: (*name).into(),
                    slug: None,
                });
            }
        }
        catalog
    }

    fn english_overlay_catalog(&self) -> Vec<OverlayCatalogItem> {
        let mut by_lower: BTreeMap<String, OverlayCatalogItem> = BTreeMap::new();
        for reward in self.relic_rewards.values().flatten() {
            let name = reward.reward_name.trim();
            if name.is_empty() {
                continue;
            }
            let slug = reward.reward_slug.trim();
            by_lower
                .entry(name.to_lowercase())
                .or_insert_with(|| OverlayCatalogItem {
                    name: name.to_string(),
                    slug: (!slug.is_empty()).then(|| slug.to_string()),
                });
        }
        by_lower
            .entry("forma blueprint".into())
            .or_insert_with(|| OverlayCatalogItem {
                name: "Forma Blueprint".into(),
                slug: None,
            });
        by_lower.into_values().collect()
    }

    pub fn overlay_market_facts(&self, slug: Option<&str>) -> OverlayMarketFacts {
        let Some(entry) = slug.and_then(|slug| self.items.get(slug)) else {
            return OverlayMarketFacts {
                cached_platinum: None,
                ducats: None,
            };
        };
        let cached_platinum = [entry.low5_avg, entry.low_sell, entry.avg]
            .into_iter()
            .find(|value| value.is_finite() && *value > 0.0)
            .map(|value| value.round().clamp(1.0, u32::MAX as f64) as u32);
        OverlayMarketFacts {
            cached_platinum,
            ducats: entry.ducats.and_then(|value| u32::try_from(value).ok()),
        }
    }

    pub fn overlay_owned(&self, db: &Db) -> Option<HashMap<String, u32>> {
        if db.list_snapshots(1).ok()?.is_empty() {
            return None;
        }
        let mut owned: HashMap<String, u32> = HashMap::new();
        for item in db.latest_snapshot_items().ok()? {
            let Some((_, slug)) = self.resolve(&item.slug) else {
                continue;
            };
            let count = u32::try_from(item.count.max(0)).unwrap_or(u32::MAX);
            owned
                .entry(slug)
                .and_modify(|total| *total = total.saturating_add(count))
                .or_insert(count);
        }
        Some(owned)
    }
}

#[cfg(test)]
mod localized_catalog_tests {
    use super::*;

    fn market(reward_names: serde_json::Value) -> MarketData {
        serde_json::from_value(serde_json::json!({
            "items": {},
            "relic_rewards": {"lith_v1_relic": [
                {"reward_slug": "volt_prime_neuroptics_blueprint", "reward_name": "Volt Prime Neuroptics Blueprint"}
            ]},
            "reward_names": reward_names
        }))
        .unwrap()
    }

    fn names(catalog: &[OverlayCatalogItem]) -> Vec<(&str, Option<&str>)> {
        catalog
            .iter()
            .map(|item| (item.name.as_str(), item.slug.as_deref()))
            .collect()
    }

    #[test]
    fn a_localized_catalog_adds_the_language_beside_english() {
        let market = market(serde_json::json!({
            "fr": {"fetched_at": "2026-11-07T00:00:00Z",
                   "names": {"volt_prime_neuroptics_blueprint": "Volt Prime - Neuroptiques (Schéma)"}},
            "ru": {"fetched_at": "2026-11-07T00:00:00Z",
                   "names": {"volt_prime_neuroptics_blueprint": "Вольт Прайм: Нейрооптика (Чертёж)"}}
        }));
        let english = market.overlay_catalog(Some("en"));
        assert_eq!(english, market.overlay_catalog(None));
        assert_eq!(
            names(&english),
            vec![
                ("Forma Blueprint", None),
                (
                    "Volt Prime Neuroptics Blueprint",
                    Some("volt_prime_neuroptics_blueprint")
                ),
            ]
        );
        let french = market.overlay_catalog(Some("fr"));
        assert_eq!(&french[..2], &english[..]);
        assert_eq!(
            names(&french[2..]),
            vec![
                (
                    "Volt Prime - Neuroptiques (Schéma)",
                    Some("volt_prime_neuroptics_blueprint")
                ),
                ("Forma", None),
            ]
        );
        // Each script gets the Forma Blueprint as its game writes it.
        let russian = market.overlay_catalog(Some("ru"));
        assert_eq!(russian.len(), 4);
        assert_eq!(names(&russian[3..]), vec![("Форма", None)]);
        // A language the snapshot has no names for reads English, plus the
        // Forma Blueprint, which never comes from the snapshot.
        let japanese = market.overlay_catalog(Some("ja"));
        assert_eq!(&japanese[..2], &english[..]);
        assert_eq!(names(&japanese[2..]), vec![("フォーマ設計図", None)]);
        // Thai has no names, so it reads English only.
        assert_eq!(market.overlay_catalog(Some("th")), english);
    }

    #[test]
    fn missing_or_null_localized_names_are_an_empty_surface() {
        assert_eq!(
            market(serde_json::Value::Null)
                .overlay_catalog(Some("fr"))
                .len(),
            3
        );
        let market: MarketData = serde_json::from_value(serde_json::json!({"items": {}})).unwrap();
        assert_eq!(market.overlay_catalog(Some("fr")).len(), 2);
    }
}
