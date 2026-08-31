//! warframe.market order service: list / bulk-visibility / update / delete
//! on a decrypted-JWT [`Unlocked`] credential bundle the adapter builds once
//! (see the auth module). See [`crate::plan`] for bulk-order creation via
//! the crash-recoverable plan executor, and [`crate::catalog`] for the WFM
//! item catalog fetch both this module and the plan executor depend on.

use anyhow::{bail, Context, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::auth::fetch_wfm_me;
use crate::catalog::{fetch_wfm_catalog, index_item_meta, ItemMeta, WfmCatalogItem};
use crate::util::{browser_client, wfm_client};

/// Shared with [`crate::plan::run_pending`] - both pace their WFM calls to
/// the same 3 req/sec norm.
pub(crate) const SERVE_RATE_LIMIT_MS: u64 = 350;
// Matches WFM's own UI cap (3000) and the browser ListingReviewModal's
// MAX_PLATINUM. Previously 999, which silently blocked maxed-Arcane and
// Galvanized-mod listings that genuinely sell for 1500–2500p.
pub const MAX_PLATINUM: u32 = 3000;

/// Attempts for create/update/delete order calls. This is the path a user
/// watches in real time: these calls used to give up after one transport
/// error or 5xx, so a single dropped packet mid-batch surfaced as a permanent
/// failure on that item.
///
/// The catalog warm deliberately does NOT retry - `fetch_wfm_catalog` is
/// single-shot, and a failed warm is one visible error the user can act on
/// rather than a half-finished batch. This comment used to claim it "already
/// retries via wfm-client", which was the reason a dead `get_with_retry`
/// looked load-bearing for months.
pub(crate) const ORDER_RETRY_ATTEMPTS: u32 = 2;

/// Retry a request builder for transport errors, 5xx responses, and 429 -
/// never any other 4xx, which are semantic (a bad price, wrong subtype, a slug
/// that doesn't exist) and won't succeed on retry. Backoff shares
/// `wfm_client::retry_backoff` (2s/4s/6s) rather than a second hand-rolled
/// curve; a 429 that carries `Retry-After` (seconds) waits at least that long
/// instead, which is what WFM's rate-limit rules ask of clients. Requests
/// with a non-cloneable body (not the case for any call site here - all send
/// `.json(...)`) fall back to a single attempt.
#[allow(clippy::unreachable, reason = "the loop returns on its final attempt when max_attempts is at least one")]
pub(crate) fn send_with_retry(
    builder: reqwest::blocking::RequestBuilder,
    max_attempts: u32,
) -> reqwest::Result<reqwest::blocking::Response> {
    for attempt in 0..max_attempts {
        let this_send = match builder.try_clone() {
            Some(b) => b.send(),
            None => return builder.send(),
        };
        let (retryable, retry_after) = match &this_send {
            Ok(resp) => {
                let st = resp.status();
                let after = (st.as_u16() == 429)
                    .then(|| retry_after_secs(resp.headers()))
                    .flatten();
                (st.is_server_error() || st.as_u16() == 429, after)
            }
            Err(_) => (true, None),
        };
        if !retryable || attempt + 1 == max_attempts {
            return this_send;
        }
        let wait = wfm_client::retry_backoff(attempt);
        let wait = match retry_after {
            Some(ra) if ra > wait => ra,
            _ => wait,
        };
        std::thread::sleep(wait);
    }
    unreachable!("ORDER_RETRY_ATTEMPTS must be >= 1")
}

/// `Retry-After` as a delay, when it is the delta-seconds form (the only form
/// WFM/Cloudflare send). Absent, unparseable, or the HTTP-date form → `None`,
/// and the caller falls back to the standard backoff. Capped at 60 s so a
/// hostile or mistaken header cannot park the UI thread.
pub(crate) fn retry_after_secs(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    let secs: u64 = headers.get(reqwest::header::RETRY_AFTER)?.to_str().ok()?.trim().parse().ok()?;
    Some(Duration::from_secs(secs.min(60)))
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
    let client = wfm_client()?;
    let url = format!(
        "https://api.warframe.market/v2/orders/user/{}",
        unlocked.username
    );
    let resp = wfm_client::wfm_authed_headers(client.get(&url), &unlocked.platform, &unlocked.jwt)
        .send()
        .context("/v2/orders/user request failed")?;
    let status = resp.status();
    let mut body: serde_json::Value = resp.json().context("parsing orders response")?;
    if !status.is_success() {
        bail!("WFM HTTP {status}: {body}");
    }
    crate::catalog::enrich_orders_with_names(&mut body, &unlocked.id_to_item);
    Ok(body)
}

pub fn bulk_set_visibility(unlocked: &Unlocked, req: &VisibilityRequest) -> Vec<PerOrderResult> {
    let client = match wfm_client() {
        Ok(c) => c,
        Err(e) => {
            return req.order_ids.iter().map(|id| PerOrderResult {
                order_id: id.clone(),
                status: "error".into(),
                message: Some(format!("client: {e}")),
            }).collect();
        }
    };
    let mut out = Vec::with_capacity(req.order_ids.len());
    let mut last = std::time::Instant::now()
        .checked_sub(Duration::from_millis(SERVE_RATE_LIMIT_MS))
        .unwrap_or_else(std::time::Instant::now);
    for id in &req.order_ids {
        let elapsed = last.elapsed();
        if elapsed < Duration::from_millis(SERVE_RATE_LIMIT_MS) {
            thread::sleep(Duration::from_millis(SERVE_RATE_LIMIT_MS).saturating_sub(elapsed));
        }
        last = std::time::Instant::now();
        out.push(patch_one_order(&client, unlocked, id, &serde_json::json!({"visible": req.visible})));
    }
    out
}

pub fn update_order(unlocked: &Unlocked, id: &str, upd: &UpdateRequest) -> Result<PerOrderResult> {
    let client = wfm_client()?;
    let mut body = serde_json::Map::new();
    if let Some(v) = upd.platinum { body.insert("platinum".into(), serde_json::json!(v)); }
    if let Some(v) = upd.quantity { body.insert("quantity".into(), serde_json::json!(v)); }
    if let Some(v) = upd.visible  { body.insert("visible".into(),  serde_json::json!(v)); }
    if let Some(v) = upd.rank     { body.insert("rank".into(),     serde_json::json!(v)); }
    if body.is_empty() {
        bail!("update body has no fields to patch");
    }
    Ok(patch_one_order(&client, unlocked, id, &serde_json::Value::Object(body)))
}

pub(crate) fn patch_one_order(
    client: &Client,
    unlocked: &Unlocked,
    id: &str,
    body: &serde_json::Value,
) -> PerOrderResult {
    let url = format!("https://api.warframe.market/v2/order/{id}");
    let resp = send_with_retry(
        wfm_client::wfm_authed_headers(client.patch(&url), &unlocked.platform, &unlocked.jwt).json(body),
        ORDER_RETRY_ATTEMPTS,
    );
    match resp {
        Ok(r) => {
            let status = r.status();
            if status.is_success() {
                PerOrderResult { order_id: id.into(), status: "ok".into(), message: None }
            } else {
                let body: serde_json::Value = r.json().unwrap_or(serde_json::Value::Null);
                PerOrderResult {
                    order_id: id.into(),
                    status: "error".into(),
                    message: Some(format!("HTTP {status}: {}", body.get("error").map(|v| v.to_string()).unwrap_or_else(|| "(no message)".into()))),
                }
            }
        }
        Err(e) => PerOrderResult {
            order_id: id.into(),
            status: "error".into(),
            message: Some(format!("HTTP request failed: {e}")),
        },
    }
}

pub fn delete_order(unlocked: &Unlocked, id: &str) -> Result<()> {
    let client = wfm_client()?;
    let url = format!("https://api.warframe.market/v2/order/{id}");
    let resp = send_with_retry(
        wfm_client::wfm_authed_headers(client.delete(&url), &unlocked.platform, &unlocked.jwt),
        ORDER_RETRY_ATTEMPTS,
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

#[cfg(test)]
mod retry_after_tests {
    use super::retry_after_secs;
    use reqwest::header::{HeaderMap, HeaderValue, RETRY_AFTER};
    use std::time::Duration;

    fn hm(v: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(RETRY_AFTER, HeaderValue::from_str(v).unwrap());
        h
    }

    #[test]
    fn delta_seconds_is_honoured_and_capped() {
        assert_eq!(retry_after_secs(&hm("7")), Some(Duration::from_secs(7)));
        assert_eq!(retry_after_secs(&hm(" 3 ")), Some(Duration::from_secs(3)));
        assert_eq!(retry_after_secs(&hm("9999")), Some(Duration::from_secs(60)));
    }

    #[test]
    fn absent_or_http_date_falls_back_to_none() {
        assert_eq!(retry_after_secs(&HeaderMap::new()), None);
        assert_eq!(retry_after_secs(&hm("Wed, 21 Oct 2026 07:28:00 GMT")), None);
        assert_eq!(retry_after_secs(&hm("-1")), None);
    }
}
