//! WFM item catalog fetch (`GET /v2/items`) and order-response enrichment -
//! WFM's `/orders` endpoint returns only a raw `itemId`, so this injects the
//! display name the browser panels need, looked up against the catalog.

use wfm_client::transport::GovernedRequest;
use wfm_client::governor::Kind;
use anyhow::{bail, Context, Result};
use reqwest::blocking::Client;
use std::collections::BTreeMap;

pub struct WfmCatalogItem {
    pub item_id: String,
    /// WFM rejects perTrade for items without this capability.
    pub bulk_tradable: bool,
    /// Only identities the first Trade Session planner can reconstruct safely.
    pub session_supported: bool,
    /// Human-readable display name from /v2/items i18n.en.name. Used to
    /// enrich GET /orders so the panel doesn't render raw itemIds.
    pub display_name: String,
    /// Some items (mods, arcanes) accept a `rank` field on POST /v2/order
    /// and **require** that maxRank exists in the catalog. For items
    /// without `maxRank`, sending `rank` at all returns
    /// `app.field.notAllowed` - so we conditionally include the field.
    pub max_rank: Option<u32>,
    /// Items with multiple variants (relics: intact/exc/fla/rad;
    /// veiled rivens: unrevealed/revealed) require a `subtype` on POST
    /// /v2/order. Without it WFM returns `app.field.required`. We default
    /// to the first listed subtype (lowest-value: intact relic, unrevealed
    /// riven) - the user can pick a different one via the orders panel
    /// after listing succeeds.
    pub subtypes: Vec<String>,
}

pub fn fetch_wfm_catalog(
    client: &Client,
    platform: &str,
) -> Result<BTreeMap<String, WfmCatalogItem>> {
    // v1 retired; v2 returns a flat `data` array of {id, slug, ...}.
    // Order creation is v2 as well (POST /v2/order, see plan::build_order_body).
    let resp =
        wfm_client::wfm_headers(client.get("https://api.warframe.market/v2/items"), platform)
            .send_governed(Kind::Read)
            .context("fetching /v2/items")?;
    if !resp.status().is_success() {
        bail!("/v2/items returned HTTP {}", resp.status());
    }
    let body: serde_json::Value = resp.json().context("parsing /v2/items")?;
    parse_wfm_catalog(&body)
}

fn parse_wfm_catalog(body: &serde_json::Value) -> Result<BTreeMap<String, WfmCatalogItem>> {
    let items = body.get("data").and_then(|v| v.as_array()).ok_or_else(|| {
        anyhow::anyhow!("/v2/items response shape changed (no top-level data array)")
    })?;
    let mut out = BTreeMap::new();
    for it in items {
        let id = it.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let slug = it.get("slug").and_then(|v| v.as_str()).unwrap_or("");
        if !id.is_empty() && !slug.is_empty() {
            let display_name = it
                .pointer("/i18n/en/name")
                .and_then(|v| v.as_str())
                .unwrap_or(slug)
                .to_string();
            let max_rank = it.get("maxRank").and_then(|v| v.as_u64()).map(|n| n as u32);
            let subtypes: Vec<String> = it
                .get("subtypes")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let supported_tag = it
                .get("tags")
                .and_then(|v| v.as_array())
                .is_some_and(|tags| {
                    tags.iter().filter_map(|v| v.as_str()).any(|tag| {
                        matches!(
                            tag,
                            "mod"
                                | "arcane_enhancement"
                                | "component"
                                | "primary"
                                | "secondary"
                                | "melee"
                        )
                    })
                });
            let set_identity = slug.ends_with("_set") && max_rank.is_none()
                && it.get("setRoot").and_then(|v| v.as_bool()) == Some(true);
            let session_supported = (supported_tag || set_identity)
                && subtypes.is_empty()
                && it
                    .get("setRoot")
                    .is_none_or(|v| v.is_null() || v.as_bool() == Some(false) || set_identity)
                && it
                    .get("tradable")
                    .is_none_or(|v| v.is_null() || v.as_bool() == Some(true))
                && ["maxCharges", "maxAmberStars", "maxCyanStars"]
                    .iter()
                    .all(|key| it.get(key).is_none_or(|v| v.is_null()));
            out.insert(
                slug.to_string(),
                WfmCatalogItem {
                    item_id: id.to_string(),
                    bulk_tradable: it
                        .get("bulkTradable")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                    session_supported,
                    display_name,
                    max_rank,
                    subtypes,
                },
            );
        }
    }
    Ok(out)
}

/// Display name + slug for one WFM item id - what a bare `itemId` on a user
/// order needs to become something the UI can show AND price-check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemMeta {
    pub name: String,
    pub slug: String,
}

/// `itemId → {name, slug}` over a loaded catalog.
pub fn index_item_meta(catalog: &BTreeMap<String, WfmCatalogItem>) -> BTreeMap<String, ItemMeta> {
    catalog
        .iter()
        .map(|(slug, c)| {
            (
                c.item_id.clone(),
                ItemMeta {
                    name: c.display_name.clone(),
                    slug: slug.clone(),
                },
            )
        })
        .collect()
}

pub(crate) fn attach_item_meta(order: &mut serde_json::Value, id_to_item: &BTreeMap<String, ItemMeta>) {
    let id = order
        .get("itemId")
        .and_then(|v| v.as_str())
        .or_else(|| order.get("item_id").and_then(|v| v.as_str()))
        .map(|s| s.to_string());
    let Some(id) = id else { return };
    let Some(meta) = id_to_item.get(&id) else {
        return;
    };
    let Some(obj) = order.as_object_mut() else {
        return;
    };
    // Don't clobber if WFM has started including item metadata on its own -
    // fill only the keys that are missing.
    let item = obj.entry("item").or_insert_with(|| serde_json::json!({}));
    if let Some(item_obj) = item.as_object_mut() {
        item_obj
            .entry("name")
            .or_insert_with(|| serde_json::json!(meta.name));
        item_obj
            .entry("slug")
            .or_insert_with(|| serde_json::json!(meta.slug));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_support_excludes_unreconstructable_item_variants() {
        let base = serde_json::json!({"id":"item","slug":"item","tags":["mod"],"maxRank":5});
        let catalog = parse_wfm_catalog(&serde_json::json!({"data":[base.clone()]})).unwrap();
        assert!(catalog["item"].session_supported);
        for (key, value) in [
            ("subtypes", serde_json::json!(["intact"])),
            ("setRoot", serde_json::json!(true)),
            ("maxCharges", serde_json::json!(3)),
            ("maxAmberStars", serde_json::json!(4)),
            ("maxCyanStars", serde_json::json!(4)),
            ("tradable", serde_json::json!(false)),
            ("tags", serde_json::json!(["unknown-class"])),
            ("setRoot", serde_json::json!("false")),
            ("tradable", serde_json::json!("true")),
        ] {
            let mut row = base.clone();
            row[key] = value;
            let catalog = parse_wfm_catalog(&serde_json::json!({"data":[row]})).unwrap();
            assert!(!catalog["item"].session_supported, "{key}");
        }
    }

    #[test]
    fn complete_set_identity_requires_an_explicit_tradable_rankless_root() {
        let base = serde_json::json!({"id":"set", "slug":"example_set", "setRoot":true, "tradable":true});
        let parse = |row| parse_wfm_catalog(&serde_json::json!({"data":[row]})).unwrap();
        assert!(parse(base.clone())["example_set"].session_supported);
        for (key, value) in [("tradable", serde_json::json!(false)), ("maxRank", serde_json::json!(5)), ("setRoot", serde_json::json!("true"))] {
            let mut row = base.clone(); row[key] = value;
            assert!(!parse(row)["example_set"].session_supported);
        }
    }

    #[test]
    fn bulk_trading_requires_explicit_catalog_capability() {
        let body = serde_json::json!({ "data": [
            { "id": "a", "slug": "bulk", "bulkTradable": true },
            { "id": "b", "slug": "single", "bulkTradable": false },
            { "id": "c", "slug": "unknown" },
            { "id": "d", "slug": "malformed", "bulkTradable": "true" }
        ] });
        let catalog = parse_wfm_catalog(&body).unwrap();
        assert!(catalog["bulk"].bulk_tradable);
        for slug in ["single", "unknown", "malformed"] {
            assert!(!catalog[slug].bulk_tradable, "{slug}");
        }
    }

    #[test]
    fn index_item_meta_keys_by_item_id_and_carries_the_slug() {
        let mut cat = BTreeMap::new();
        cat.insert(
            "loki_prime_set".to_string(),
            WfmCatalogItem {
                item_id: "aaaaaaaaaaaaaaaaaaaaaaaa".into(),
                bulk_tradable: false,
                session_supported: false,
                display_name: "Loki Prime Set".into(),
                max_rank: None,
                subtypes: vec![],
            },
        );
        let idx = index_item_meta(&cat);
        assert_eq!(
            idx["aaaaaaaaaaaaaaaaaaaaaaaa"],
            ItemMeta {
                name: "Loki Prime Set".into(),
                slug: "loki_prime_set".into()
            }
        );
    }
}
