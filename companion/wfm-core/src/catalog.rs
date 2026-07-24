//! WFM item catalog fetch (`GET /v2/items`) and order-response enrichment —
//! WFM's `/orders` endpoint returns only a raw `itemId`, so this injects the
//! display name the browser panels need, looked up against the catalog.

use anyhow::{bail, Context, Result};
use reqwest::blocking::Client;
use std::collections::BTreeMap;

pub struct WfmCatalogItem {
    pub item_id: String,
    /// Human-readable display name from /v2/items i18n.en.name. Used to
    /// enrich GET /orders so the panel doesn't render raw itemIds.
    pub display_name: String,
    /// Some items (mods, arcanes) accept a `rank` field on POST /v2/order
    /// and **require** that maxRank exists in the catalog. For items
    /// without `maxRank`, sending `rank` at all returns
    /// `app.field.notAllowed` — so we conditionally include the field.
    pub max_rank: Option<u32>,
    /// Items with multiple variants (relics: intact/exc/fla/rad;
    /// veiled rivens: unrevealed/revealed) require a `subtype` on POST
    /// /v2/order. Without it WFM returns `app.field.required`. We default
    /// to the first listed subtype (lowest-value: intact relic, unrevealed
    /// riven) — the user can pick a different one via the orders panel
    /// after listing succeeds.
    pub subtypes: Vec<String>,
}

pub fn fetch_wfm_catalog(client: &Client, platform: &str) -> Result<BTreeMap<String, WfmCatalogItem>> {
    // v1 retired; v2 returns a flat `data` array of {id, slug, ...}.
    // Order creation is v2 as well (POST /v2/order, see plan::build_order_body).
    let resp = wfm_client::wfm_headers(client.get("https://api.warframe.market/v2/items"), platform)
        .send()
        .context("fetching /v2/items")?;
    if !resp.status().is_success() {
        bail!("/v2/items returned HTTP {}", resp.status());
    }
    let body: serde_json::Value = resp.json().context("parsing /v2/items")?;
    let items = body
        .get("data")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow::anyhow!("/v2/items response shape changed (no top-level data array)"))?;
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
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            out.insert(slug.to_string(), WfmCatalogItem {
                item_id: id.to_string(),
                display_name,
                max_rank,
                subtypes,
            });
        }
    }
    Ok(out)
}

// WFM /v2/orders/user/<username> returns orders that carry `itemId` but no
// display name. The MyOrdersPanel falls all the way through to the raw id
// without this. We mutate the response in place to attach
// `item: { name, slug }` per order, looked up against the catalog we already
// loaded at startup. Tolerates both shapes WFM has shipped:
//   { data: { sell: [...], buy: [...] } }   ← current v2
//   { data: [...] }                          ← flat list, occasional v1-ish
pub fn enrich_orders_with_names(body: &mut serde_json::Value, id_to_name: &BTreeMap<String, String>) {
    let Some(data) = body.get_mut("data") else { return };
    if let Some(arr) = data.as_array_mut() {
        for o in arr {
            attach_item_name(o, id_to_name);
        }
        return;
    }
    for bucket in ["sell", "buy"] {
        if let Some(arr) = data.get_mut(bucket).and_then(|v| v.as_array_mut()) {
            for o in arr {
                attach_item_name(o, id_to_name);
            }
        }
    }
}

fn attach_item_name(order: &mut serde_json::Value, id_to_name: &BTreeMap<String, String>) {
    let id = order
        .get("itemId")
        .and_then(|v| v.as_str())
        .or_else(|| order.get("item_id").and_then(|v| v.as_str()))
        .map(|s| s.to_string());
    let Some(id) = id else { return };
    let Some(name) = id_to_name.get(&id) else { return };
    if let Some(obj) = order.as_object_mut() {
        // Don't clobber if WFM has started including item metadata on its own.
        if !obj.contains_key("item") {
            obj.insert("item".into(), serde_json::json!({ "name": name }));
        } else if let Some(item_obj) = obj.get_mut("item").and_then(|v| v.as_object_mut()) {
            if !item_obj.contains_key("name") {
                item_obj.insert("name".into(), serde_json::json!(name));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_id_map() -> BTreeMap<String, String> {
        let mut m = BTreeMap::new();
        m.insert("54aae292e7798909064f1575".into(), "Secura Dual Cestra".into());
        m.insert("aaaaaaaaaaaaaaaaaaaaaaaa".into(), "Loki Prime Set".into());
        m
    }

    #[test]
    fn enrich_orders_handles_split_sell_buy_shape() {
        let mut body = serde_json::json!({
            "data": {
                "sell": [
                    {"id": "o1", "itemId": "aaaaaaaaaaaaaaaaaaaaaaaa", "platinum": 120},
                ],
                "buy": [
                    {"id": "o2", "itemId": "54aae292e7798909064f1575", "platinum": 5},
                ]
            }
        });
        enrich_orders_with_names(&mut body, &sample_id_map());
        assert_eq!(body["data"]["sell"][0]["item"]["name"], "Loki Prime Set");
        assert_eq!(body["data"]["buy"][0]["item"]["name"], "Secura Dual Cestra");
    }

    #[test]
    fn enrich_orders_handles_flat_array_shape() {
        let mut body = serde_json::json!({
            "data": [
                {"id": "o1", "itemId": "aaaaaaaaaaaaaaaaaaaaaaaa", "platinum": 120},
            ]
        });
        enrich_orders_with_names(&mut body, &sample_id_map());
        assert_eq!(body["data"][0]["item"]["name"], "Loki Prime Set");
    }

    #[test]
    fn enrich_orders_leaves_unknown_ids_alone() {
        let mut body = serde_json::json!({
            "data": { "sell": [{ "id": "o1", "itemId": "deadbeef", "platinum": 9 }] }
        });
        enrich_orders_with_names(&mut body, &sample_id_map());
        // No `item` key injected because the id wasn't in the catalog.
        assert!(body["data"]["sell"][0].get("item").is_none());
    }

    #[test]
    fn enrich_orders_preserves_existing_item_metadata() {
        // If WFM ever starts returning `item` itself, don't clobber.
        let mut body = serde_json::json!({
            "data": { "sell": [{
                "id": "o1",
                "itemId": "aaaaaaaaaaaaaaaaaaaaaaaa",
                "item": { "name": "Custom Name", "icon": "x.png" },
            }]}
        });
        enrich_orders_with_names(&mut body, &sample_id_map());
        assert_eq!(body["data"]["sell"][0]["item"]["name"], "Custom Name");
        assert_eq!(body["data"]["sell"][0]["item"]["icon"], "x.png");
    }
}
