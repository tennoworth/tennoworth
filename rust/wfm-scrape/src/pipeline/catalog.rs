use crate::ingest::{self, Http};
use crate::render::CatalogItemMeta;
use std::collections::HashMap;
use std::path::Path;

pub(super) struct CatalogSources {
    pub(super) catalog: HashMap<String, String>,
    pub(super) meta_by_slug: HashMap<String, CatalogItemMeta>,
    pub(super) wfstat_raw: Option<serde_json::Value>,
    pub(super) path_to_info: HashMap<String, serde_json::Value>,
    pub(super) set_to_parts: HashMap<String, serde_json::Value>,
    pub(super) parents_complete: bool,
    pub(super) catalog_fresh: bool,
    pub(super) path_to_info_complete: bool,
}

pub(super) fn fetch(
    http: &dyn Http,
    fixtures_dir: Option<&Path>,
    catalog_out: &Path,
    prior: &serde_json::Value,
) -> Result<CatalogSources, String> {
    eprintln!("Fetching warframe.market master catalog...");
    // `catalog_fresh` is false on the prior-snapshot fallback: that copy has
    // no gameRefs, so the resolver surfaces built from it are partial.
    let (catalog, meta_by_slug, catalog_fresh) =
        match ingest::fetch_catalog_wfm(http, "https://api.warframe.market/v2/items") {
            Ok((catalog, meta)) => (catalog, meta, true),
            Err(e) => {
                let Some(prior_catalog) = prior.get("catalog").and_then(|c| c.as_object()) else {
                    return Err(format!("{e} - and no prior snapshot to fall back on."));
                };
                eprintln!("  {e} - reusing the prior snapshot's catalog");
                let cat: HashMap<String, String> = prior_catalog
                    .iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                    .collect();
                let items_meta: HashMap<String, CatalogItemMeta> = prior
                    .get("items")
                    .and_then(|i| i.as_object())
                    .map(|items| {
                        items
                            .iter()
                            .map(|(slug, it)| {
                                (
                                    slug.clone(),
                                    CatalogItemMeta {
                                        tags: it
                                            .get("tags")
                                            .and_then(|t| t.as_array())
                                            .map(|a| {
                                                a.iter()
                                                    .filter_map(|v| {
                                                        v.as_str().map(|s| s.to_string())
                                                    })
                                                    .collect()
                                            })
                                            .unwrap_or_default(),
                                        ducats: it.get("ducats").and_then(|d| d.as_i64()),
                                        ..Default::default()
                                    },
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                (cat, items_meta, false)
            }
        };
    eprintln!("  {} items", catalog.len());

    // Fetched BEFORE the parent walk, and only once: sentinel parents now come
    // out of this payload (their own endpoint 404s since ~2026-07-31) and the
    // resolver catalog is reduced from the same copy further down. Two
    // consumers, one ~44 MB download - a second one would add minutes to a
    // scrape that already runs close to its systemd timeout.
    eprintln!("Fetching warframestat bulk item catalog...");
    let wfstat_raw: Option<serde_json::Value> = if fixtures_dir.is_none() {
        ingest::fetch_wfstat_raw()
            .map_err(|e| eprintln!("  warning: {e}"))
            .ok()
    } else {
        http.get_json(ingest::WFSTAT_ITEMS_URL)
            .map_err(|e| eprintln!("  warning: {e}"))
            .ok()
    };

    eprintln!("Fetching warframestat component path map + sets...");
    let (mut path_to_info, set_to_parts, parents_complete) =
        ingest::fetch_parent_data(http, &catalog, wfstat_raw.as_ref());
    eprintln!(
        "  {} component paths · {} prime sets",
        path_to_info.len(),
        set_to_parts.len()
    );
    // The catalogue the resolvers will read beside path_to_info: this cycle's,
    // or the preserved file when the bulk fetch failed.
    let wfstat_slim_for_paths: Vec<serde_json::Value> = match wfstat_raw.as_ref() {
        Some(raw) => ingest::slim_wfstat_items(raw, ingest::WFSTAT_ITEMS_URL).unwrap_or_default(),
        None => std::fs::read(catalog_out)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default(),
    };
    let game_ref_paths = ingest::add_game_ref_paths(
        &meta_by_slug,
        &mut path_to_info,
        &ingest::wfstat_categories(&wfstat_slim_for_paths),
    );
    eprintln!("  {game_ref_paths} paths from warframe.market gameRefs that warframestat lacks");
    let path_to_info_complete = parents_complete && catalog_fresh;

    Ok(CatalogSources {
        catalog,
        meta_by_slug,
        wfstat_raw,
        path_to_info,
        set_to_parts,
        parents_complete,
        catalog_fresh,
        path_to_info_complete,
    })
}
