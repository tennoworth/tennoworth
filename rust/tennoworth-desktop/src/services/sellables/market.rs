use std::collections::HashMap;

use crate::services::market::MarketCache;

use super::{MarketData, BUNDLED_MARKET};

impl MarketData {
    /// Load the freshest market we hold: the app-data cache (last known-good from
    /// tennoworth.app) when it is at least as fresh as the compile-time bundle,
    /// else the bundle.
    ///
    /// "Freshest", not "cached": the bundle ships with each release, so a user
    /// who upgraded while holding a cache written before a rollback has a cache
    /// that is older than the bundle they just installed. Preferring the cache
    /// unconditionally would pin every native consumer to the rolled-back
    /// snapshot even though a newer one is compiled in.
    pub fn load(cache: &MarketCache) -> MarketData {
        let cached = cache
            .cached()
            .and_then(|body| serde_json::from_str::<MarketData>(&body).ok());
        match cached {
            Some(mut m) if !Self::is_older_than_bundle(&m) => {
                m.build_usage_parent_index();
                m
            }
            _ => Self::bundled(),
        }
    }

    /// The slug a price report may name for a traded item, and whether the
    /// item has ranks or subtypes. None for names the catalog does not know,
    /// items without a market row, and rivens, whose price depends on stats
    /// the trade line does not carry.
    pub fn report_item(&self, name: &str) -> Option<(String, bool)> {
        // The snapshot does not carry WFM's max rank or subtypes, so these tags
        // stand in for them. Erring toward tiered only labels a price "rank
        // unknown"; erring the other way would mix ranks into one price.
        const TIERED: [&str; 5] = ["mod", "arcane_enhancement", "relic", "fish", "ayatan_sculpture"];
        const UNREPORTED: [&str; 2] = ["riven_mod", "veiled_riven"];
        let slug = self.catalog.get(&name.trim().to_lowercase())?;
        let tags = &self.items.get(slug)?.tags;
        if tags.iter().any(|t| UNREPORTED.contains(&t.as_str())) {
            return None;
        }
        Some((slug.clone(), tags.iter().any(|t| TIERED.contains(&t.as_str()))))
    }

    /// Whether `candidate` is strictly older than the compiled-in bundle.
    ///
    /// Unknown stamps answer false on both sides: a snapshot we cannot place in
    /// time is not evidence that the other one is newer, and treating it as older
    /// would silently swap the user back to the bundle for good.
    fn is_older_than_bundle(candidate: &MarketData) -> bool {
        let bundle = match serde_json::from_str::<MarketData>(BUNDLED_MARKET) {
            Ok(bundle) => bundle,
            Err(_) => return false,
        };
        let parse = |stamp: Option<&String>| {
            stamp.and_then(|stamp| chrono::DateTime::parse_from_rfc3339(stamp).ok())
        };
        match (
            parse(candidate.updated_at.as_ref()),
            parse(bundle.updated_at.as_ref()),
        ) {
            (Some(candidate), Some(bundle)) => candidate < bundle,
            _ => false,
        }
    }

    pub(super) fn bundled() -> MarketData {
        let mut market = serde_json::from_str(BUNDLED_MARKET).unwrap_or(MarketData {
            updated_at: None,
            items: HashMap::new(),
            catalog: HashMap::new(),
            path_to_info: HashMap::new(),
            usage: HashMap::new(),
            set_to_parts: HashMap::new(),
            relic_rewards: HashMap::new(),
            usage_parent_by_part: HashMap::new(),
        });
        market.build_usage_parent_index();
        market
    }

    pub(super) fn build_usage_parent_index(&mut self) {
        let mut index: HashMap<String, Option<String>> = HashMap::new();
        for (parent, set) in &self.set_to_parts {
            for part in &set.parts {
                if part.slug.is_empty() {
                    continue;
                }
                match index.get(&part.slug) {
                    None => {
                        index.insert(part.slug.clone(), Some(parent.clone()));
                    }
                    Some(Some(existing)) if existing != parent => {
                        index.insert(part.slug.clone(), None);
                    }
                    _ => {}
                }
            }
        }
        self.usage_parent_by_part = index;
    }

    pub(super) fn usage_share(&self, slug: &str) -> Option<f64> {
        self.usage_resolution(slug).map(|(_, share, _)| share)
    }

    pub(super) fn usage_resolution(&self, slug: &str) -> Option<(String, f64, bool)> {
        if let Some(direct) = self.usage.get(slug) {
            return valid_usage_share(direct).map(|share| (slug.to_string(), share, false));
        }
        let parent = self.usage_parent_by_part.get(slug)?.as_deref()?;
        let share = self.usage.get(parent).and_then(valid_usage_share)?;
        Some((parent.to_string(), share, true))
    }
}

fn valid_usage_share(value: &serde_json::Value) -> Option<f64> {
    market_domain::scoring::valid_usage(value).map(|usage| usage.share)
}
