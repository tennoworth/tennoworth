//! warframe.market order service: list / bulk-visibility / update / delete
//! on a decrypted-JWT [`Unlocked`] credential bundle the adapter builds once
//! (see the auth module). See [`crate::trading::plan`] for bulk-order creation via
//! the crash-recoverable plan executor, and [`crate::trading::catalog`] for the WFM
//! item catalog fetch both this module and the plan executor depend on.

use wfm_client::transport::GovernedRequest;
use wfm_client::governor::Kind;
use anyhow::{bail, Context, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::http::{browser_client, wfm_client};
use crate::trading::auth::fetch_wfm_me;
use crate::trading::catalog::{fetch_wfm_catalog, index_item_meta, ItemMeta, WfmCatalogItem};

// Matches WFM's own UI cap (3000) and the browser ListingReviewModal's
// MAX_PLATINUM. Previously 999, which silently blocked maxed-Arcane and
// Galvanized-mod listings that genuinely sell for 1500–2500p.
pub const MAX_PLATINUM: u32 = 3000;

/// Mutations are never replayed after an ambiguous transport or server failure.
pub(crate) fn send_mutation(builder: reqwest::blocking::RequestBuilder) -> Result<wfm_client::transport::GovernedResponse, wfm_client::governor::AccessError> {
    builder.send_governed(Kind::Mutation)
}

/// Everything a listing request needs, produced once on first use (decrypt +
/// catalog warm). The adapter builds this and passes it to every route.
pub struct Unlocked {
    pub jwt: String,
    pub username: String,
    /// The market the JWT authenticates against (pc / ps4 / xbox / switch).
    /// Carried with the credential so every listing call sends a Platform header
    /// consistent with the JWT, even when serve's startup snapshot said "pc"
    /// (no login on disk at startup, then a console login loaded late).
    pub platform: String,
    pub catalog: Arc<BTreeMap<String, WfmCatalogItem>>,
    /// itemId → {name, slug}. Injected into the /orders response so the UI
    /// doesn't show raw 24-char hex IDs and can price-check by slug.
    pub id_to_item: Arc<BTreeMap<String, ItemMeta>>,
}

/// Build the [`Unlocked`] bundle for an already-decrypted JWT: warm the WFM
/// catalog, derive the itemId→name map the /orders response needs, and confirm
/// the credential by resolving the username.
///
/// Both adapters need exactly this and had drifted into keeping their own copy
/// (serve's `build_unlocked` tail, desktop's `warm`) - the same four calls in
/// the same order, differing only in how they map the error. Callers map;
/// this stays anyhow so wfm-core owes nothing to either shell.
///
/// The only network in the unlock path. 60 s rather than the listing routes'
/// 30: the catalog is a multi-MB body and a user is watching a one-time warm,
/// not a per-request round-trip.
pub fn warm_unlocked(jwt: String, platform: String) -> Result<Unlocked> {
    let http = browser_client(60)?;
    let catalog = fetch_wfm_catalog(&http, &platform)?;
    let id_to_item = index_item_meta(&catalog);
    let username = fetch_wfm_me(&http, &jwt, &platform)?;
    Ok(Unlocked {
        jwt,
        username,
        platform,
        catalog: Arc::new(catalog),
        id_to_item: Arc::new(id_to_item),
    })
}

#[derive(Deserialize)]
pub struct VisibilityRequest {
    pub order_ids: Vec<String>,
    pub visible: bool,
}

#[derive(Deserialize)]
pub struct UpdateRequest {
    pub platinum: Option<u32>,
    pub quantity: Option<u32>,
    pub visible: Option<bool>,
    pub rank: Option<u32>,
}

#[derive(Serialize)]
pub struct PerOrderResult {
    pub order_id: String,
    pub status: String, // "ok" | "error"
    pub message: Option<String>,
}

pub fn list_user_orders(unlocked: &Unlocked) -> Result<serde_json::Value> {
    read_user_orders(unlocked, true)
}
pub fn cached_user_orders(unlocked: &Unlocked) -> Result<serde_json::Value> {
    read_user_orders(unlocked, false)
}
fn read_user_orders(unlocked: &Unlocked, fresh: bool) -> Result<serde_json::Value> {
    let client = wfm_client()?;
    let url = format!(
        "https://api.warframe.market/v2/orders/user/{}",
        unlocked.username
    );
    let mut body = wfm_client::transport::read_json(
        wfm_client::wfm_authed_headers(client.get(&url), &unlocked.platform, &unlocked.jwt), Kind::Read,
        wfm_client::transport::ReadKey { url, platform: unlocked.platform.clone(), account: Some(unlocked.username.clone()) },
        std::time::Duration::from_secs(5), fresh,
    )?;
    validate_orders_body(&body, unlocked)?;
    crate::trading::catalog::enrich_orders_with_names(&mut body, &unlocked.id_to_item);
    Ok(body)
}

pub fn bulk_set_visibility(unlocked: &Unlocked, req: &VisibilityRequest) -> Vec<PerOrderResult> {
    let client = match wfm_client() {
        Ok(c) => c,
        Err(e) => {
            return req
                .order_ids
                .iter()
                .map(|id| PerOrderResult {
                    order_id: id.clone(),
                    status: "error".into(),
                    message: Some(format!("client: {e}")),
                })
                .collect();
        }
    };
    let mut out = Vec::with_capacity(req.order_ids.len());
    for id in &req.order_ids {
        out.push(patch_one_order(
            &client,
            unlocked,
            id,
            &serde_json::json!({"visible": req.visible}),
        ));
    }
    out
}

pub fn update_order(unlocked: &Unlocked, id: &str, upd: &UpdateRequest) -> Result<PerOrderResult> {
    let client = wfm_client()?;
    let mut body = serde_json::Map::new();
    if let Some(v) = upd.platinum {
        body.insert("platinum".into(), serde_json::json!(v));
    }
    if let Some(v) = upd.quantity {
        body.insert("quantity".into(), serde_json::json!(v));
    }
    if let Some(v) = upd.visible {
        body.insert("visible".into(), serde_json::json!(v));
    }
    if let Some(v) = upd.rank {
        body.insert("rank".into(), serde_json::json!(v));
    }
    if body.is_empty() {
        bail!("update body has no fields to patch");
    }
    Ok(patch_one_order(
        &client,
        unlocked,
        id,
        &serde_json::Value::Object(body),
    ))
}

pub(crate) fn patch_one_order(
    client: &Client,
    unlocked: &Unlocked,
    id: &str,
    body: &serde_json::Value,
) -> PerOrderResult {
    let url = format!("https://api.warframe.market/v2/order/{id}");
    let resp = send_mutation(
        wfm_client::wfm_authed_headers(client.patch(&url), &unlocked.platform, &unlocked.jwt)
            .json(body),
    );
    match resp {
        Ok(r) => {
            let status = r.status();
            if status.is_success() {
                PerOrderResult {
                    order_id: id.into(),
                    status: "ok".into(),
                    message: None,
                }
            } else {
                let body: serde_json::Value = r.json().unwrap_or(serde_json::Value::Null);
                PerOrderResult {
                    order_id: id.into(),
                    status: "error".into(),
                    message: Some(format!(
                        "HTTP {status}: {}",
                        body.get("error")
                            .map(|v| v.to_string())
                            .unwrap_or_else(|| "(no message)".into())
                    )),
                }
            }
        }
        Err(e) => PerOrderResult {
            order_id: id.into(),
            status: if matches!(e, wfm_client::governor::AccessError::UncertainMutation) { "uncertain_mutation" } else { "pending" }.into(),
            message: Some(e.to_string()),
        },
    }
}

pub fn delete_order(unlocked: &Unlocked, id: &str) -> Result<()> {
    let client = wfm_client()?;
    let url = format!("https://api.warframe.market/v2/order/{id}");
    let resp = send_mutation(
        wfm_client::wfm_authed_headers(client.delete(&url), &unlocked.platform, &unlocked.jwt),
    )
    .context("DELETE request failed")?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().unwrap_or_default();
        let mut end = body.len().min(300);
        while !body.is_char_boundary(end) {
            end = end.saturating_sub(1);
        }
        bail!("WFM HTTP {status}: {}", body.get(..end).unwrap_or(""));
    }
    Ok(())
}

fn validate_orders_body(body: &serde_json::Value, unlocked: &Unlocked) -> Result<()> {
    let data = body.get("data").context("Orders response has no data; refusing to assume an empty account.")?;
    let valid_row = |row: &serde_json::Value, bucket: Option<&str>| {
        ["id", "itemId"].iter().all(|key| row.get(key).and_then(|v| v.as_str()).is_some_and(|s| !s.is_empty()))
            && matches!(row.get("type").and_then(|v| v.as_str()).or(bucket), Some("buy" | "sell"))
            && ["quantity", "platinum"].iter().all(|key| row.get(key).and_then(|v| v.as_u64()).is_some_and(|n| n > 0))
            && row.get("rank").is_none_or(|v| v.is_null() || v.as_u64().is_some())
            && row.get("subtype").is_none_or(|v| v.is_null() || v.as_str().is_some_and(|s| !s.is_empty()))
            && unlocked.catalog.values().find(|item| Some(item.item_id.as_str()) == row.get("itemId").and_then(|v| v.as_str())).is_none_or(|item| {
                item.max_rank.is_none_or(|max| row.get("rank").and_then(|v| v.as_u64()).is_some_and(|rank| rank <= u64::from(max)))
                    && (item.subtypes.is_empty() || row.get("subtype").and_then(|v| v.as_str()).is_some_and(|subtype| item.subtypes.iter().any(|s| s == subtype)))
            })
    };
    let valid = if let Some(rows) = data.as_array() {
        rows.iter().all(|row| valid_row(row, None))
    } else {
        ["sell", "buy"].iter().all(|bucket| data.get(bucket).and_then(|v| v.as_array()).is_some_and(|rows| rows.iter().all(|row| valid_row(row, Some(bucket)))))
    };
    if !valid { bail!("Orders response is incomplete; reconcile current orders before changing listings."); }
    Ok(())
}

#[cfg(test)]
mod response_tests {
    use super::*;
    fn unlocked() -> Unlocked {
        Unlocked { jwt: String::new(), username: "fixture".into(), platform: "pc".into(), catalog: Arc::new(BTreeMap::new()), id_to_item: Arc::new(BTreeMap::new()) }
    }
    #[test]
    fn malformed_orders_cannot_be_interpreted_as_an_empty_account() {
        for body in [serde_json::json!({}), serde_json::json!({"data": null}), serde_json::json!({"data": {"sell": []}}), serde_json::json!({"data": [{"id": "a"}]})] {
            assert!(validate_orders_body(&body, &unlocked()).is_err());
        }
        assert!(validate_orders_body(&serde_json::json!({"data": []}), &unlocked()).is_ok());
        assert!(validate_orders_body(&serde_json::json!({"data": {"sell": [], "buy": []}}), &unlocked()).is_ok());
    }
    #[test]
    fn missing_variant_metadata_cannot_hide_an_existing_order() {
        let mut session = unlocked();
        session.catalog = Arc::new(BTreeMap::from([("fixture".into(), WfmCatalogItem { item_id: "item".into(), display_name: "Fixture".into(), bulk_tradable: false, session_supported: true, max_rank: Some(10), subtypes: vec!["revealed".into()] })]));
        let mut body = serde_json::json!({"data": [{"id": "order", "itemId": "item", "type": "sell", "quantity": 1, "platinum": 10}]});
        assert!(validate_orders_body(&body, &session).is_err());
        body["data"][0]["rank"] = serde_json::json!(0);
        assert!(validate_orders_body(&body, &session).is_err());
        body["data"][0]["subtype"] = serde_json::json!("revealed");
        assert!(validate_orders_body(&body, &session).is_ok());
    }
}
