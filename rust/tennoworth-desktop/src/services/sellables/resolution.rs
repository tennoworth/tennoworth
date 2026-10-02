use std::collections::HashMap;
use std::sync::OnceLock;

use market_domain::inventory::{path_guess_candidates, slug_guess};
use serde::Deserialize;

use super::{MarketData, BUNDLED_CATALOG};

#[derive(Deserialize)]
struct SlimInfo {
    name: String,
}

/// Parse the bundled wfstat catalog once (DE path → display name). Static: the
/// catalog is compile-time data that never changes at runtime.
fn wfstat_catalog() -> &'static HashMap<String, SlimInfo> {
    static CATALOG: OnceLock<HashMap<String, SlimInfo>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        // Slim `[uniqueName, {name, category}]` pairs - the shape native
        // inventory normalization reads. A parse failure yields an empty map (path_to_info still works).
        let pairs: Vec<(String, SlimInfo)> =
            serde_json::from_str(BUNDLED_CATALOG).unwrap_or_default();
        pairs.into_iter().collect()
    })
}

impl MarketData {
    /// Resolve a DE item path to `(display name, WFM slug)`, or `None` when the
    /// path maps to nothing tradeable. Mirrors market_domain::inventory's
    /// non-relic paths.
    pub(super) fn resolve(&self, path: &str) -> Option<(String, String)> {
        // 1. Pre-baked direct hit (prime parts / warframes / recipes).
        if let Some(d) = self.path_to_info.get(path) {
            if !d.slug.is_empty() {
                return Some((d.name.clone(), d.slug.clone()));
            }
        }

        // 2. wfstat catalog: path → name, retrying with the Component/Blueprint
        //    suffix trimmed (the same fallback native normalization uses).
        let cat = wfstat_catalog();
        let mut info = cat.get(path);
        if info.is_none() {
            for suffix in ["Component", "Blueprint"] {
                if let Some(trimmed) = path.strip_suffix(suffix) {
                    if let Some(hit) = cat.get(trimmed) {
                        info = Some(hit);
                        break;
                    }
                }
            }
        }

        // 3. No catalog entry: guess a display name from the path basename and
        //    accept it ONLY on an exact market.catalog hit (strict, like the TS -
        //    a bad guess can never fabricate an item).
        let Some(info) = info else {
            let (name, slug) = path_guess_candidates(path)
                .into_iter()
                .find_map(|candidate| {
                    self.catalog
                        .get(&candidate.to_lowercase())
                        .map(|slug| (candidate, slug.clone()))
                })?;
            return Some((name, slug));
        };

        // 4. name → slug via the market catalog, else a slug guess (which may not
        //    exist in `items`; the caller drops it then).
        let slug = self
            .catalog
            .get(&info.name.to_lowercase())
            .cloned()
            .unwrap_or_else(|| slug_guess(&info.name));
        Some((info.name.clone(), slug))
    }
}
