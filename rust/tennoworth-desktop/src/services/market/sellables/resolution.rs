use std::collections::HashMap;
use std::sync::OnceLock;

use market_domain::inventory::{resolve_path, CatalogItem, PathTables};

use super::{MarketData, BUNDLED_CATALOG};

/// The bundled catalog (DE path → name, category), parsed once: it is
/// compile-time data. A parse failure leaves it empty; `path_to_info` still
/// resolves the pre-baked parts.
fn bundled_catalog() -> &'static HashMap<String, CatalogItem> {
    static CATALOG: OnceLock<HashMap<String, CatalogItem>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str::<Vec<(String, CatalogItem)>>(BUNDLED_CATALOG)
            .unwrap_or_default()
            .into_iter()
            .collect()
    })
}

impl PathTables for MarketData {
    fn path_info(&self, path: &str) -> Option<(&str, &str, Option<&str>)> {
        self.path_to_info
            .get(path)
            .map(|info| (info.name.as_str(), info.slug.as_str(), None))
    }
    fn catalog_entry(&self, path: &str) -> Option<(&str, Option<&str>)> {
        bundled_catalog()
            .get(path)
            .map(|info| (info.name.as_str(), info.category.as_deref()))
    }
    fn slug_for(&self, lower_name: &str) -> Option<&str> {
        self.catalog.get(lower_name).map(String::as_str)
    }
}

impl MarketData {
    /// Resolve a DE item path to `(display name, WFM slug)` with the same rules
    /// as the webview's inventory table. Refined relics are left out: the tray,
    /// trade sessions and overlay count by slug alone, and a relic slug without
    /// its refinement would merge four different items.
    pub(super) fn resolve(&self, path: &str) -> Option<(String, String)> {
        resolve_path(path, self)
            .filter(|resolved| resolved.refinement.is_none())
            .map(|resolved| (resolved.name, resolved.slug))
    }
}
