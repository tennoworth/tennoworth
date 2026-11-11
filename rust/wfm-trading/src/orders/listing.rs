//! warframe.market order service: list / bulk-visibility / update / delete
//! on a decrypted-JWT [`Unlocked`] credential bundle the adapter builds once
//! (see the auth module). See [`crate::orders::plan`] for bulk-order creation via
//! the crash-recoverable plan executor, and [`crate::market::catalog`] for the WFM
//! item catalog fetch both this module and the plan executor depend on.

use anyhow::{anyhow, bail, Context, Result};
use market_domain::limits::{MAX_PLATINUM, MIN_PLATINUM};
use market_domain::orders::OrderRow;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use wfm_client::governor::Kind;
use wfm_client::transport::GovernedRequest;

use crate::auth::fetch_wfm_me;
use crate::http::{browser_client, wfm_client};
use crate::market::catalog::{fetch_wfm_catalog, index_item_meta, ItemMeta, WfmCatalogItem};
use crate::orders::decode::{decode_orders, DecodedOrders};
use crate::orders::outcome::MutationOutcome;

/// Mutations are never replayed after an ambiguous transport or server failure.
pub(crate) fn send_mutation(
    builder: reqwest::blocking::RequestBuilder,
) -> Result<wfm_client::transport::GovernedResponse, wfm_client::governor::AccessError> {
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
/// this stays anyhow so wfm-trading owes nothing to either shell.
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
    let body = read_user_order_rows(unlocked, fresh)?;
    let decoded = decode_user_orders(unlocked, &body)?;
    Ok(orders_body(decoded, unlocked))
}

/// Fetch without whole-account reconciliation so auto-close can refuse individual rows.
pub fn list_user_order_rows(unlocked: &Unlocked) -> Result<serde_json::Value> {
    read_user_order_rows(unlocked, true)
}

fn read_user_order_rows(unlocked: &Unlocked, fresh: bool) -> Result<serde_json::Value> {
    let client = wfm_client()?;
    let url = format!(
        "https://api.warframe.market/v2/orders/user/{}",
        unlocked.username
    );
    let body = wfm_client::transport::read_json(
        wfm_client::wfm_authed_headers(client.get(&url), &unlocked.platform, &unlocked.jwt),
        Kind::Read,
        wfm_client::transport::ReadKey {
            url,
            platform: unlocked.platform.clone(),
            account: Some(unlocked.username.clone()),
        },
        std::time::Duration::from_secs(5),
        fresh,
    )?;
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

/// Longest id accepted. WFM ids are 24-character hex object ids; the bound
/// only has to keep an arbitrary string out of the request path.
const MAX_ORDER_ID_LEN: usize = 64;

/// An order id is interpolated into the request path, so a `/`, `..`, `?` or
/// `#` in it would address a different endpoint with the user's credentials.
pub fn validate_order_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > MAX_ORDER_ID_LEN
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        bail!("Invalid order id; refresh your orders and try again.");
    }
    Ok(())
}

fn order_url(id: &str) -> Result<String> {
    validate_order_id(id)?;
    Ok(format!("https://api.warframe.market/v2/order/{id}"))
}

/// Everything about an edit that can be refused without the live order. An
/// order's `platinum` is the price of one lot, so the cap here is the lot cap
/// and the floor is the single-unit floor; [`update_order`] adds the floor for
/// the order's actual lot once it has read it.
pub fn validate_update(id: &str, upd: &UpdateRequest) -> Result<()> {
    validate_order_id(id)?;
    if upd.platinum.is_none()
        && upd.quantity.is_none()
        && upd.visible.is_none()
        && upd.rank.is_none()
    {
        bail!("update body has no fields to patch");
    }
    if let Some(p) = upd.platinum {
        if p > MAX_PLATINUM {
            bail!("price {p}p > max {MAX_PLATINUM}p");
        }
        if p < MIN_PLATINUM {
            bail!("price {p}p < min {MIN_PLATINUM}p");
        }
    }
    if upd.quantity == Some(0) {
        bail!("quantity must be at least 1");
    }
    Ok(())
}

/// A lot of `per_trade` units must ask at least `MIN_PLATINUM` for each unit,
/// the same floor a reviewed batch applies before it multiplies by the lot.
pub fn validate_lot_price(platinum: u32, per_trade: Option<u64>) -> Result<()> {
    let lot = per_trade.unwrap_or(1).max(1);
    if u64::from(platinum) < u64::from(MIN_PLATINUM).saturating_mul(lot) {
        bail!("price {platinum}p for a lot of {lot} < min {MIN_PLATINUM}p each");
    }
    Ok(())
}

pub fn update_order(unlocked: &Unlocked, id: &str, upd: &UpdateRequest) -> Result<PerOrderResult> {
    validate_update(id, upd)?;
    if let Some(platinum) = upd.platinum {
        // Every mutation invalidates the read cache, so a cached lot cannot be
        // one this app has since changed.
        let rows = read_user_order_rows(unlocked, false)?;
        let per_trade = decode_user_orders(unlocked, &rows)?
            .orders
            .into_iter()
            .find_map(|row| match row {
                OrderRow::Supported(order) if order.id == id => Some(order.per_trade),
                _ => None,
            })
            .ok_or_else(|| {
                anyhow!("The order is not among your current orders; refresh and try again.")
            })?;
        validate_lot_price(platinum, per_trade)?;
    }
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
    let url = match order_url(id) {
        Ok(url) => url,
        Err(e) => {
            return PerOrderResult {
                order_id: id.into(),
                status: "error".into(),
                message: Some(e.to_string()),
            }
        }
    };
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
            status: MutationOutcome::from(&e).status_str().into(),
            message: Some(e.to_string()),
        },
    }
}

pub fn delete_order(unlocked: &Unlocked, id: &str) -> Result<()> {
    let url = order_url(id)?;
    let client = wfm_client()?;
    let resp = send_mutation(wfm_client::wfm_authed_headers(
        client.delete(&url),
        &unlocked.platform,
        &unlocked.jwt,
    ))
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

/// The catalogue's own rank and subtype rules, handed to the order decoder so
/// the two never state them separately. Rows carry an itemId, so the lookup
/// goes through the itemId→slug index the session already built.
struct CatalogConstraints<'a>(&'a Unlocked);

impl market_domain::orders::ItemConstraints for CatalogConstraints<'_> {
    fn accepts(&self, item_id: &str, rank: Option<u64>, subtype: Option<&str>) -> bool {
        let Some(item) = self
            .0
            .id_to_item
            .get(item_id)
            .and_then(|meta| self.0.catalog.get(&meta.slug))
        else {
            return true;
        };
        item.max_rank
            .is_none_or(|max| rank.is_some_and(|rank| rank <= u64::from(max)))
            && (item.subtypes.is_empty()
                || subtype.is_some_and(|subtype| item.subtypes.iter().any(|s| s == subtype)))
    }
}

/// Decode the account's orders against this session's catalogue. This is the
/// single reading of the endpoint: a caller that reconciles against orders uses
/// this rather than walking the response body itself.
pub fn decode_user_orders(unlocked: &Unlocked, body: &serde_json::Value) -> Result<DecodedOrders> {
    decode_orders(body, &CatalogConstraints(unlocked))
}

fn orders_body(decoded: DecodedOrders, unlocked: &Unlocked) -> serde_json::Value {
    let rows: Vec<_> = decoded
        .orders
        .into_iter()
        .filter_map(|row| {
            let market_domain::orders::OrderRow::Supported(order) = row else {
                return None;
            };
            let mut row = serde_json::json!({
                "id": order.id, "itemId": order.item_id, "type": order.side.as_str(),
                "platinum": order.platinum, "quantity": order.quantity,
                "rank": order.rank, "subtype": order.subtype, "perTrade": order.per_trade,
                "visible": order.visible,
            });
            crate::market::catalog::attach_item_meta(&mut row, &unlocked.id_to_item);
            Some(row)
        })
        .collect();
    serde_json::json!({"data": rows})
}

pub fn decode_user_order_rows(
    unlocked: &Unlocked,
    body: &serde_json::Value,
) -> Result<Vec<market_domain::orders::OrderRow>> {
    crate::orders::decode::decode_order_rows(body, &CatalogConstraints(unlocked))
}

#[cfg(test)]
fn validate_orders_body(body: &serde_json::Value, unlocked: &Unlocked) -> Result<()> {
    decode_user_orders(unlocked, body).map(|_| ())
}

#[cfg(test)]
mod response_tests {
    use super::*;
    fn unlocked() -> Unlocked {
        Unlocked {
            jwt: String::new(),
            username: "fixture".into(),
            platform: "pc".into(),
            catalog: Arc::new(BTreeMap::new()),
            id_to_item: Arc::new(BTreeMap::new()),
        }
    }
    #[test]
    fn decoded_orders_resolve_metadata_by_catalogue_identity() {
        let mut session = unlocked();
        session.id_to_item = Arc::new(BTreeMap::from([(
            "item".into(),
            ItemMeta {
                name: "Part".into(),
                slug: "part".into(),
            },
        )]));
        let body = serde_json::json!({"data":{"sell":[{"id":"a","itemId":"item","platinum":10,"quantity":2}],"buy":[]}});
        let result = orders_body(decode_user_orders(&session, &body).unwrap(), &session);
        assert_eq!(
            result["data"][0]["item"],
            serde_json::json!({"name":"Part","slug":"part"})
        );
        assert_eq!(result["data"][0]["type"], "sell");
        assert_eq!(result["data"][0]["quantity"], 2);
    }

    #[test]
    fn malformed_orders_cannot_be_interpreted_as_an_empty_account() {
        for body in [
            serde_json::json!({}),
            serde_json::json!({"data": null}),
            serde_json::json!({"data": {"sell": []}}),
            serde_json::json!({"data": [{"id": "a"}]}),
        ] {
            assert!(validate_orders_body(&body, &unlocked()).is_err());
        }
        assert!(validate_orders_body(&serde_json::json!({"data": []}), &unlocked()).is_ok());
        assert!(validate_orders_body(
            &serde_json::json!({"data": {"sell": [], "buy": []}}),
            &unlocked()
        )
        .is_ok());
    }

    /// A row may take its side from its bucket or state it explicitly - but not
    /// contradict both. Consumers disagree about which one wins (plan indexing
    /// trusts the explicit side, My Orders overwrites it with the bucket), so
    /// accepting the contradiction makes one row a buy to one consumer and a sell
    /// to another. Refuse the body instead of picking a winner.
    #[test]
    fn a_row_whose_explicit_side_contradicts_its_bucket_is_rejected() {
        let contradicts = serde_json::json!({"data": {"sell": [
            {"id": "order", "itemId": "item", "type": "buy", "quantity": 1, "platinum": 10}
        ], "buy": []}});
        assert!(validate_orders_body(&contradicts, &unlocked()).is_err());

        // Agreeing, or stating nothing and taking the bucket's side, stay valid.
        let agrees = serde_json::json!({"data": {"sell": [
            {"id": "order", "itemId": "item", "type": "sell", "quantity": 1, "platinum": 10}
        ], "buy": []}});
        assert!(validate_orders_body(&agrees, &unlocked()).is_ok());
        let bucket_only = serde_json::json!({"data": {"sell": [
            {"id": "order", "itemId": "item", "quantity": 1, "platinum": 10}
        ], "buy": []}});
        assert!(validate_orders_body(&bucket_only, &unlocked()).is_ok());
    }
    #[test]
    fn missing_variant_metadata_cannot_hide_an_existing_order() {
        let mut session = unlocked();
        session.catalog = Arc::new(BTreeMap::from([(
            "fixture".into(),
            WfmCatalogItem {
                item_id: "item".into(),
                display_name: "Fixture".into(),
                bulk_tradable: false,
                session_supported: true,
                max_rank: Some(10),
                subtypes: vec!["revealed".into()],
            },
        )]));
        // The row an orders response carries names its item by itemId, so the
        // session's own itemId→slug index is what reaches the catalogue.
        session.id_to_item = Arc::new(BTreeMap::from([(
            "item".into(),
            ItemMeta {
                name: "Fixture".into(),
                slug: "fixture".into(),
            },
        )]));
        let mut body = serde_json::json!({"data": [{"id": "order", "itemId": "item", "type": "sell", "quantity": 1, "platinum": 10}]});
        assert!(validate_orders_body(&body, &session).is_err());
        body["data"][0]["rank"] = serde_json::json!(0);
        assert!(validate_orders_body(&body, &session).is_err());
        body["data"][0]["subtype"] = serde_json::json!("revealed");
        assert!(validate_orders_body(&body, &session).is_ok());
    }
}

#[cfg(test)]
mod edit_tests {
    use super::*;

    fn unlocked() -> Unlocked {
        Unlocked {
            jwt: String::new(),
            username: "fixture".into(),
            platform: "pc".into(),
            catalog: Arc::new(BTreeMap::new()),
            id_to_item: Arc::new(BTreeMap::new()),
        }
    }

    fn patch(platinum: Option<u32>, quantity: Option<u32>) -> UpdateRequest {
        UpdateRequest {
            platinum,
            quantity,
            visible: None,
            rank: None,
        }
    }

    fn refusal(result: Result<PerOrderResult>) -> String {
        match result {
            Ok(_) => panic!("the edit should have been refused"),
            Err(error) => error.to_string(),
        }
    }

    const PATH_SHAPED: [&str; 7] = [
        "",
        "../../v2/me",
        "abc/def",
        "abc?visible=true",
        "abc#fragment",
        "abc%2F..",
        "abc def",
    ];

    #[test]
    fn order_ids_are_limited_to_the_id_alphabet() {
        assert!(validate_order_id("5f1e8c2b9d3a4e7f6c0b1a2d").is_ok());
        assert!(validate_order_id("order-id_1").is_ok());
        assert!(validate_order_id(&"a".repeat(64)).is_ok());
        assert!(validate_order_id(&"a".repeat(65)).is_err());
        assert!(validate_order_id("ordér").is_err());
        for id in PATH_SHAPED {
            assert!(validate_order_id(id).is_err(), "{id:?} must be refused");
        }
    }

    #[test]
    fn a_path_shaped_id_is_refused_before_any_request() {
        let session = unlocked();
        let error = delete_order(&session, "../../v2/me").unwrap_err();
        assert!(error.to_string().contains("Invalid order id"));
        let error = refusal(update_order(&session, "abc/def", &patch(None, Some(1))));
        assert!(error.contains("Invalid order id"));
    }

    #[test]
    fn bulk_visibility_reports_a_bad_id_per_order() {
        let results = bulk_set_visibility(
            &unlocked(),
            &VisibilityRequest {
                order_ids: vec!["../../v2/me".into(), "abc?x".into()],
                visible: true,
            },
        );
        assert_eq!(results.len(), 2);
        for (result, id) in results.iter().zip(["../../v2/me", "abc?x"]) {
            assert_eq!(result.order_id, id);
            assert_eq!(result.status, "error");
            assert!(result
                .message
                .as_deref()
                .is_some_and(|m| m.contains("Invalid order id")));
        }
    }

    #[test]
    fn edits_enforce_the_listing_caps() {
        let id = "5f1e8c2b9d3a4e7f6c0b1a2d";
        assert!(validate_update(id, &patch(Some(MIN_PLATINUM), None)).is_ok());
        assert!(validate_update(id, &patch(Some(MAX_PLATINUM), None)).is_ok());
        assert!(validate_update(id, &patch(None, Some(1))).is_ok());
        let low = validate_update(id, &patch(Some(MIN_PLATINUM - 1), None)).unwrap_err();
        assert!(low.to_string().contains("< min"));
        let high = validate_update(id, &patch(Some(MAX_PLATINUM + 1), None)).unwrap_err();
        assert!(high.to_string().contains("> max"));
        let empty = validate_update(id, &patch(Some(0), None)).unwrap_err();
        assert!(empty.to_string().contains("< min"));
        let zero = validate_update(id, &patch(None, Some(0))).unwrap_err();
        assert!(zero.to_string().contains("quantity"));
        assert!(validate_update(id, &patch(None, None)).is_err());
    }

    #[test]
    fn caps_are_checked_before_any_request() {
        let session = unlocked();
        let id = "5f1e8c2b9d3a4e7f6c0b1a2d";
        let error = refusal(update_order(&session, id, &patch(None, Some(0))));
        assert!(error.contains("quantity"));
        let error = refusal(update_order(
            &session,
            id,
            &patch(Some(MAX_PLATINUM + 1), None),
        ));
        assert!(error.contains("> max"));
    }

    #[test]
    fn a_lot_price_must_cover_the_minimum_for_each_unit() {
        assert!(validate_lot_price(MIN_PLATINUM, None).is_ok());
        assert!(validate_lot_price(MIN_PLATINUM, Some(1)).is_ok());
        assert!(validate_lot_price(MIN_PLATINUM * 3, Some(3)).is_ok());
        assert!(validate_lot_price(MIN_PLATINUM * 3 - 1, Some(3)).is_err());
        assert!(validate_lot_price(MIN_PLATINUM * 6 - 1, Some(6)).is_err());
    }
}
