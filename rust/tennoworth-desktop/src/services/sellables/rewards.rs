use std::collections::{BTreeMap, HashMap};

use crate::persistence::Db;

use super::{MarketData, OverlayCatalogItem, OverlayMarketFacts};

impl MarketData {
    pub fn overlay_catalog(&self) -> Vec<OverlayCatalogItem> {
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
