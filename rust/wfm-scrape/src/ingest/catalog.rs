use super::Http;
use crate::render::CatalogItemMeta;
use std::collections::HashMap;

/// Fetch WFM catalog (`/v2/items`) - returns name→slug catalog AND
/// per-item metadata (tags, ducats, max_rank, subtypes).
///
/// The live transport owns retries so ingestion cannot multiply its budget.
pub type CatalogFetch = (HashMap<String, String>, HashMap<String, CatalogItemMeta>);

pub fn fetch_catalog_wfm(http: &dyn Http, url: &str) -> Result<CatalogFetch, String> {
    let body = http.get_json(url)?;
    let items = wfm_client::unwrap_envelope(&body);
    let arr = items
        .as_array()
        .ok_or_else(|| format!("{url}: not an array"))?;
    let mut catalog = HashMap::new();
    let mut meta = HashMap::new();
    for it in arr {
        let slug = it.get("slug").and_then(|s| s.as_str()).unwrap_or("");
        let nm = it
            .get("i18n")
            .and_then(|i| i.get("en"))
            .and_then(|n| n.get("name"))
            .and_then(|n| n.as_str())
            .unwrap_or("");
        if !slug.is_empty() && !nm.is_empty() {
            catalog.insert(nm.to_lowercase(), slug.to_string());
        }
        if !slug.is_empty() {
            let tags: Vec<String> = it
                .get("tags")
                .and_then(|t| t.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            meta.insert(
                slug.to_string(),
                CatalogItemMeta {
                    tags,
                    ducats: it.get("ducats").and_then(|d| d.as_i64()),
                    max_rank: it.get("maxRank").and_then(|r| r.as_i64()),
                    subtypes: it
                        .get("subtypes")
                        .and_then(|s| s.as_array())
                        .map(|a| {
                            a.iter()
                                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                .collect()
                        })
                        .unwrap_or_default(),
                },
            );
        }
    }
    Ok((catalog, meta))
}

/// Fetch warframestat parent endpoints → path_to_info + set_to_parts.
/// Returns `(path_to_info, set_to_parts, complete)` - `complete` is false
/// when any endpoint failed.
pub fn fetch_parent_data(
    http: &dyn Http,
    catalog: &HashMap<String, String>,
    wfstat_items: Option<&serde_json::Value>,
) -> (
    HashMap<String, serde_json::Value>,
    HashMap<String, serde_json::Value>,
    bool,
) {
    // NOTE: no /sentinels/ here. That endpoint started failing ~2026-07-31 and
    // now hard-404s with `Data key 'sentinels' not found`; /companions/ and
    // /pets/ do not exist either. The same parents are in the bulk /items/
    // payload under `category: "Sentinels"`, and that payload is already
    // fetched for the resolver catalog - so sentinels are read from it rather
    // than costing a second 44 MB download.
    let endpoints = [
        ("https://api.warframestat.us/warframes/", "Warframes"),
        ("https://api.warframestat.us/weapons/", "Weapons"),
    ];
    let mut path_to_info: HashMap<String, serde_json::Value> = HashMap::new();
    let mut set_to_parts: HashMap<String, serde_json::Value> = HashMap::new();
    let mut complete = true;

    for (url, fallback_cat) in &endpoints {
        let arr = match http.get_json(url) {
            Ok(body) => body,
            Err(e) => {
                eprintln!("  warning: could not fetch {url}: {e}");
                complete = false;
                continue;
            }
        };
        let items = match arr.as_array() {
            Some(a) => a,
            None => {
                eprintln!("  warning: {url} returned non-list (skipping)");
                complete = false;
                continue;
            }
        };
        for parent in items {
            absorb_parent(
                parent,
                fallback_cat,
                catalog,
                &mut path_to_info,
                &mut set_to_parts,
            );
        }
    }

    // Sentinels, from the bulk catalog. A missing payload marks the surface
    // incomplete for the same reason a failed endpoint does: reconcile must
    // merge over the prior snapshot rather than replace it, or every sentinel
    // prime silently disappears from set_to_parts.
    match wfstat_items.and_then(|v| v.as_array()) {
        Some(all) => {
            for parent in all
                .iter()
                .filter(|it| it.get("category").and_then(|c| c.as_str()) == Some("Sentinels"))
            {
                absorb_parent(
                    parent,
                    "Sentinels",
                    catalog,
                    &mut path_to_info,
                    &mut set_to_parts,
                );
            }
        }
        None => {
            eprintln!("  warning: no bulk item payload - sentinel parents unavailable this run");
            complete = false;
        }
    }

    (path_to_info, set_to_parts, complete)
}

/// Fold one warframestat parent (a Warframe/weapon/sentinel with `components`)
/// into the component-path map and the set→parts map.
///
/// Extracted so the sentinel source, which no longer comes from its own
/// endpoint, runs byte-identical logic to the two that still do.
fn absorb_parent(
    parent: &serde_json::Value,
    fallback_cat: &str,
    catalog: &HashMap<String, String>,
    path_to_info: &mut HashMap<String, serde_json::Value>,
    set_to_parts: &mut HashMap<String, serde_json::Value>,
) {
    let parent_name = parent.get("name").and_then(|n| n.as_str()).unwrap_or("");
    if !parent_name.contains("Prime") {
        return;
    }
    let parent_cat = parent
        .get("category")
        .and_then(|c| c.as_str())
        .unwrap_or(fallback_cat);
    let set_slug = catalog.get(&format!("{} set", parent_name.to_lowercase()));

    let mut this_set_parts: Vec<serde_json::Value> = Vec::new();
    for comp in parent
        .get("components")
        .and_then(|c| c.as_array())
        .unwrap_or(&vec![])
    {
        let un = comp
            .get("uniqueName")
            .and_then(|u| u.as_str())
            .unwrap_or("");
        let cn = comp.get("name").and_then(|n| n.as_str()).unwrap_or("");
        if un.is_empty() || cn.is_empty() {
            continue;
        }
        if un.starts_with("/Lotus/Types/Items/MiscItems/") {
            continue;
        }
        let full_name = format!("{parent_name} {cn}");
        let slug = catalog
            .get(&format!("{} blueprint", full_name.to_lowercase()))
            .or_else(|| catalog.get(&full_name.to_lowercase()))
            .or(set_slug)
            .cloned();
        let slug = match slug {
            Some(s) => s,
            None => continue,
        };
        let mut display_name = full_name.clone();
        if slug.ends_with("_set") && !full_name.ends_with("Set") {
            display_name = format!("{full_name} → set");
        } else if slug.ends_with("_blueprint") && !full_name.ends_with("Blueprint") {
            display_name = format!("{full_name} Blueprint");
        }
        let own_item = set_slug != Some(&slug);
        let info = serde_json::json!({"name": display_name, "slug": slug, "category": parent_cat});
        // The game stores an unbuilt prime part under a `...Blueprint` path while
        // warframestat lists the component as `...Component`; both name the same
        // WFM item. Register the sibling spelling too, or the form the inventory
        // actually carries resolves to nothing and the part silently vanishes
        // from the owned map - it cost one real inventory 590 ducats.
        if own_item {
            if let Some(stem) = un.strip_suffix("Component") {
                path_to_info
                    .entry(format!("{stem}Blueprint"))
                    .or_insert_with(|| info.clone());
            }
        }
        path_to_info.insert(un.to_string(), info);
        if own_item {
            let quantity = comp
                .get("itemCount")
                .and_then(|v| v.as_u64())
                .unwrap_or(1)
                .max(1);
            this_set_parts.push(serde_json::json!({
                "slug": slug,
                "component_name": cn,
                "quantity": quantity,
            }));
        }
    }
    if let (Some(ss), false) = (set_slug, this_set_parts.is_empty()) {
        set_to_parts.insert(
            ss.clone(),
            serde_json::json!({"name": parent_name, "parts": this_set_parts}),
        );
    }
}

pub const WFSTAT_ITEMS_URL: &str = "https://api.warframestat.us/items/";

/// Reduce the warframestat bulk item list to the resolver's slim
/// `[uniqueName, {name, category}]` pairs.
///
/// Shared by the live fetch and the fixture path. It was written out twice,
/// once each, and the filter and the shape have to agree exactly - the browser
/// resolver joins on these pairs, so a divergence surfaces as owned items that
/// silently fail to resolve.
pub fn slim_wfstat_items(
    arr: &serde_json::Value,
    url: &str,
) -> Result<Vec<serde_json::Value>, String> {
    let items = arr
        .as_array()
        .ok_or_else(|| format!("{url}: not an array"))?;
    Ok(items
        .iter()
        .filter(|it| it.get("uniqueName").is_some() && it.get("name").is_some())
        .map(|it| {
            serde_json::json!([it["uniqueName"], {"name": it["name"], "category": it.get("category")}])
        })
        .collect())
}

/// Fetch the warframestat bulk item catalog (resolver data).
///
/// Builds its own client rather than going through [`Http`], because English
/// must be forced per-call: the endpoint varies on `Accept-Language` and a
/// localized catalog silently breaks the name→WFM-slug join. The trait's
/// `get_json(url)` has nowhere to put a header, and pushing `Accept-Language`
/// onto the shared client would send it on every other endpoint too. Longer
/// timeout for the same reason: the body is multi-MB.
pub fn fetch_wfstat_slim() -> Result<Vec<serde_json::Value>, String> {
    slim_wfstat_items(&fetch_wfstat_raw()?, WFSTAT_ITEMS_URL)
}

/// The bulk warframestat item payload, unreduced.
///
/// Split out from `fetch_wfstat_slim` because this ~44 MB response now feeds
/// two consumers - the resolver catalog AND the sentinel parents, whose own
/// endpoint 404s - and downloading it twice would add minutes to a scrape that
/// already runs close to its systemd timeout.
pub fn fetch_wfstat_raw() -> Result<serde_json::Value, String> {
    let url = WFSTAT_ITEMS_URL;
    let resp = reqwest::blocking::Client::builder()
        .user_agent(wfm_client::user_agent(
            "wfm-scrape",
            env!("CARGO_PKG_VERSION"),
        ))
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| format!("build client: {e}"))?
        .get(url)
        .header("Accept-Language", "en")
        .send()
        .map_err(|e| format!("{url}: {e}"))?;
    let status = resp.status();
    let body = resp.text().map_err(|e| format!("{url}: read: {e}"))?;
    if !status.is_success() {
        return Err(format!("{url}: HTTP {status}"));
    }
    serde_json::from_str(&body).map_err(|e| format!("{url}: JSON: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SYSTEMS_COMPONENT: &str =
        "/Lotus/Types/Recipes/WarframeRecipes/CalibanPrimeSystemsComponent";
    const SYSTEMS_BLUEPRINT: &str =
        "/Lotus/Types/Recipes/WarframeRecipes/CalibanPrimeSystemsBlueprint";

    fn parent() -> serde_json::Value {
        json!({
            "name": "Caliban Prime",
            "category": "Warframes",
            "components": [
                { "uniqueName": SYSTEMS_COMPONENT, "name": "Systems Blueprint", "itemCount": 1 }
            ]
        })
    }

    fn catalogs(entries: &[(&str, &str)]) -> HashMap<String, String> {
        entries
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    fn absorb(catalog: &HashMap<String, String>) -> HashMap<String, serde_json::Value> {
        let mut path_to_info = HashMap::new();
        let mut set_to_parts = HashMap::new();
        absorb_parent(
            &parent(),
            "Warframes",
            catalog,
            &mut path_to_info,
            &mut set_to_parts,
        );
        path_to_info
    }

    /// The game stores an unbuilt prime part under a `...Blueprint` path while
    /// warframestat's component list carries `...Component`; both name the same
    /// WFM item. Baking only the Component spelling dropped nine parts from a
    /// real inventory and undercounted Baro's ducat total by 590.
    #[test]
    fn prime_part_component_also_registers_the_blueprint_path() {
        let catalog = catalogs(&[
            ("caliban prime set", "caliban_prime_set"),
            (
                "caliban prime systems blueprint",
                "caliban_prime_systems_blueprint",
            ),
        ]);
        let path_to_info = absorb(&catalog);

        let entry = path_to_info
            .get(SYSTEMS_BLUEPRINT)
            .expect("the blueprint path the game stores must be registered");
        assert_eq!(entry["slug"], "caliban_prime_systems_blueprint");
        assert_eq!(entry["name"], "Caliban Prime Systems Blueprint");
    }

    /// Coverage form of the rule: every Component path the bake emits needs its
    /// Blueprint sibling, whatever the part is called.
    #[test]
    fn every_component_path_gets_a_blueprint_sibling() {
        let catalog = catalogs(&[
            ("caliban prime set", "caliban_prime_set"),
            (
                "caliban prime systems blueprint",
                "caliban_prime_systems_blueprint",
            ),
            (
                "caliban prime chassis blueprint",
                "caliban_prime_chassis_blueprint",
            ),
            (
                "caliban prime neuroptics blueprint",
                "caliban_prime_neuroptics_blueprint",
            ),
        ]);
        let parent = json!({
            "name": "Caliban Prime",
            "category": "Warframes",
            "components": [
                {
                    "uniqueName": "/Lotus/Types/Recipes/WarframeRecipes/CalibanPrimeSystemsComponent",
                    "name": "Systems Blueprint",
                    "itemCount": 1
                },
                {
                    "uniqueName": "/Lotus/Types/Recipes/WarframeRecipes/CalibanPrimeChassisComponent",
                    "name": "Chassis Blueprint",
                    "itemCount": 1
                },
                {
                    "uniqueName": "/Lotus/Types/Recipes/WarframeRecipes/CalibanPrimeHelmetComponent",
                    "name": "Neuroptics Blueprint",
                    "itemCount": 1
                }
            ]
        });
        let mut path_to_info = HashMap::new();
        let mut set_to_parts = HashMap::new();
        absorb_parent(
            &parent,
            "Warframes",
            &catalog,
            &mut path_to_info,
            &mut set_to_parts,
        );

        let components: Vec<String> = path_to_info
            .keys()
            .filter(|key| key.ends_with("Component"))
            .cloned()
            .collect();
        assert_eq!(components.len(), 3, "expected three baked components");
        for component in &components {
            let stem = component.strip_suffix("Component").unwrap_or(component);
            assert!(
                path_to_info.contains_key(&format!("{stem}Blueprint")),
                "no Blueprint sibling for {component}"
            );
        }
    }

    /// A component that resolved only through its set must not hand that set
    /// slug to the blueprint path - a set is not the part the kiosk takes.
    #[test]
    fn component_resolved_only_to_its_set_registers_no_blueprint_path() {
        let catalog = catalogs(&[("caliban prime set", "caliban_prime_set")]);
        let path_to_info = absorb(&catalog);

        assert!(path_to_info.contains_key(SYSTEMS_COMPONENT));
        assert!(!path_to_info.contains_key(SYSTEMS_BLUEPRINT));
    }

    /// A blueprint path upstream already supplied wins; the sibling covers the
    /// spelling warframestat omits rather than overriding what it gave us.
    #[test]
    fn an_existing_blueprint_path_is_left_alone() {
        let catalog = catalogs(&[
            ("caliban prime set", "caliban_prime_set"),
            (
                "caliban prime systems blueprint",
                "caliban_prime_systems_blueprint",
            ),
        ]);
        let mut path_to_info = HashMap::new();
        path_to_info.insert(
            SYSTEMS_BLUEPRINT.to_string(),
            json!({"name": "Upstream Name", "slug": "upstream_slug", "category": "Upstream"}),
        );
        let mut set_to_parts = HashMap::new();
        absorb_parent(
            &parent(),
            "Warframes",
            &catalog,
            &mut path_to_info,
            &mut set_to_parts,
        );

        assert_eq!(path_to_info[SYSTEMS_BLUEPRINT]["slug"], "upstream_slug");
    }
}
