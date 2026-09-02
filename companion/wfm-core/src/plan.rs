//! The bulk-listing plan executor: turns a browser-submitted [`PlanRequest`]
//! into WFM orders, reconciling against the user's existing orders (create
//! vs. update) and persisting progress after every item so a crash mid-batch
//! is resumable via [`crate::pending`].

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::thread;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::catalog::WfmCatalogItem;
use crate::listing::{
    list_user_orders, patch_one_order, send_with_retry, Unlocked, ORDER_RETRY_ATTEMPTS,
    MAX_PLATINUM, SERVE_RATE_LIMIT_MS,
};
use crate::pending::{clear_pending, write_pending_atomic, PendingItem, PendingPlan};
use crate::util::{chrono_now_iso, random_token};

/// Resets a plan-in-flight flag on scope exit - including early return and
/// panic - so a rejected or crashed request can't leave plan execution wedged.
/// Both adapters serialize plan runs behind an `AtomicBool` and had each
/// written this guard.
pub struct PlanGuard<'a>(&'a AtomicBool);

impl<'a> PlanGuard<'a> {
    /// Take the flag, or `None` if a plan is already running. The guard's
    /// existence IS the claim; dropping it releases.
    pub fn acquire(flag: &'a AtomicBool) -> Option<Self> {
        match flag.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => Some(PlanGuard(flag)),
            Err(_) => None,
        }
    }
}

impl Drop for PlanGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

const MAX_PLAN_ITEMS: usize = 50;
const MIN_PLATINUM: u32 = 5;
const SLUG_MISMATCH_GUARD_MULTIPLIER: u32 = 3;

// Maximum items per single in-game trade - six slots per side in Warframe's
// trade window. WFM rejects `perTrade` values above this with
// `app.field.tooBig` (verified on a real relic listing, May 2026).
const MAX_PER_TRADE: u32 = 6;

#[derive(Deserialize)]
pub struct PlanRequest {
    pub items: Vec<PlanItem>,
}

#[derive(Deserialize, Clone)]
pub struct PlanItem {
    /// warframe.market url_name.
    pub slug: String,
    /// Plat the user wants to list at.
    pub platinum: u32,
    /// How many copies.
    pub quantity: u32,
    /// "sell" or "buy".
    pub order_type: String,
    /// false = invisible until manually toggled.
    pub visible: bool,
    /// Optional rank (for mods / arcanes). When `None`, we use 0 if the
    /// catalog says the item supports ranks, and omit the field otherwise.
    pub rank: Option<u32>,
    /// Optional subtype (relic refinement, veiled-riven state). When
    /// `None`, we fall back to the catalog's first listed subtype (the
    /// lowest-value default - "intact" for relics, "unrevealed" for
    /// rivens). Omitting the field for items that require it returns 400.
    #[serde(default)]
    pub subtype: Option<String>,
    /// Reference low_sell from the market snapshot, used for slug-mismatch
    /// detection. Caller is expected to populate this from market.json.
    #[serde(default)]
    pub reference_low_sell: Option<u32>,
}

#[derive(Serialize)]
pub struct PlanResponse {
    pub plan_id: String,
    pub results: Vec<ItemResult>,
}

#[derive(Serialize)]
pub struct ItemResult {
    pub slug: String,
    pub status: String, // "ok" | "skipped" | "error"
    pub message: Option<String>,
    /// WFM order id when status = "ok".
    pub order_id: Option<String>,
    /// "created" (new order) | "updated" (reconciled onto an existing order).
    /// None on errors and on pre-reconcile pending files.
    pub action: Option<String>,
}

pub fn execute_plan(pending_path: &std::path::Path, unlocked: &Unlocked, plan: PlanRequest) -> PlanResponse {
    let plan_id = random_token(8);

    if plan.items.is_empty() {
        return PlanResponse { plan_id, results: vec![] };
    }

    // --- enforced caps (defense in depth - the browser also validates) ---
    if plan.items.len() > MAX_PLAN_ITEMS {
        return PlanResponse {
            plan_id,
            results: vec![ItemResult {
                slug: "<batch>".into(),
                status: "error".into(),
                message: Some(format!(
                    "Batch has {} items; companion cap is {MAX_PLAN_ITEMS}.",
                    plan.items.len()
                )),
                order_id: None,
                action: None,
            }],
        };
    }

    // Seed the pending file before the first POST so a crash here is
    // recoverable - the browser polls /plan/pending on next connect.
    let mut pending = PendingPlan {
        plan_id: plan_id.clone(),
        started_at: chrono_now_iso(),
        items: plan.items.into_iter().map(|p| PendingItem {
            slug: p.slug,
            platinum: p.platinum,
            quantity: p.quantity,
            order_type: p.order_type,
            visible: p.visible,
            rank: p.rank,
            subtype: p.subtype,
            reference_low_sell: p.reference_low_sell,
            status: "pending".into(),
            message: None,
            order_id: None,
            action: None,
        }).collect(),
    };
    if let Err(e) = write_pending_atomic(pending_path, &pending) {
        eprintln!("warning: could not seed pending plan: {e:#}");
    }

    let response = run_pending(pending_path, unlocked, &mut pending);
    clear_pending(pending_path);
    response
}

// Drives a PendingPlan to completion, skipping items already in a terminal
// state (ok / error). Used both by the initial /plan POST and /plan/resume.
// Rewrites the on-disk pending file atomically after every item so a crash
// at any point leaves a consistent record.
pub fn run_pending(pending_path: &std::path::Path, unlocked: &Unlocked, pending: &mut PendingPlan) -> PlanResponse {
    let http = match Client::builder()
        .user_agent(crate::user_agent())
        .timeout(Duration::from_secs(30))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return PlanResponse {
                plan_id: pending.plan_id.clone(),
                results: vec![ItemResult {
                    slug: "<batch>".into(),
                    status: "error".into(),
                    message: Some(format!("HTTP client build failed: {e}")),
                    order_id: None,
                    action: None,
                }],
            };
        }
    };

    // Reconcile against the user's live orders: WFM 403s a duplicate
    // (same item/type/rank/subtype - "exceededOrderLimitSamePrice"), so those
    // become PATCHes of the existing order instead. Fetch once per run; on
    // failure fall back to create-only (the old behavior - a duplicate then
    // fails with WFM's own message, still rendered verbatim).
    let existing = if pending.items.iter().any(|i| i.status == "pending") {
        match list_user_orders(unlocked) {
            Ok(body) => index_existing_orders(&body),
            Err(e) => {
                eprintln!("warning: existing-order fetch failed; plan will create only: {e:#}");
                BTreeMap::new()
            }
        }
    } else {
        BTreeMap::new()
    };

    let mut last_call = std::time::Instant::now()
        .checked_sub(Duration::from_millis(SERVE_RATE_LIMIT_MS))
        .unwrap_or_else(std::time::Instant::now);
    for i in 0..pending.items.len() {
        let Some(item) = pending.items.get_mut(i) else {
            continue;
        };
        if item.status != "pending" {
            continue;
        }
        let since = last_call.elapsed();
        if since < Duration::from_millis(SERVE_RATE_LIMIT_MS) {
            thread::sleep(Duration::from_millis(SERVE_RATE_LIMIT_MS).saturating_sub(since));
        }
        last_call = std::time::Instant::now();

        let plan_item = PlanItem {
            slug: item.slug.clone(),
            platinum: item.platinum,
            quantity: item.quantity,
            order_type: item.order_type.clone(),
            visible: item.visible,
            rank: item.rank,
            subtype: item.subtype.clone(),
            reference_low_sell: item.reference_low_sell,
        };
        let result = execute_one(&http, unlocked, &plan_item, &existing);
        item.status = result.status.clone();
        item.message = result.message.clone();
        item.order_id = result.order_id.clone();
        item.action = result.action.clone();
        if let Err(e) = write_pending_atomic(pending_path, pending) {
            eprintln!("warning: could not persist pending update: {e:#}");
        }
    }

    PlanResponse {
        plan_id: pending.plan_id.clone(),
        results: pending.items.iter().map(|i| ItemResult {
            slug: i.slug.clone(),
            status: i.status.clone(),
            message: i.message.clone(),
            order_id: i.order_id.clone(),
            action: i.action.clone(),
        }).collect(),
    }
}

// `perTrade` must EVENLY DIVIDE `quantity` on bulk-tradable items (relics
// and similar). Listing qty=27 with perTrade=6 returns
// `app.field.orders.perTradeMustDivideQuantity` because 27/6 is not an
// integer. We pick the largest divisor of `quantity` that fits under
// MAX_PER_TRADE. Examples:
//   qty=27 → 3   (divisors: 1, 3, 9, 27; only 3 fits ≤ 6)
//   qty=10 → 5   (1, 2, 5, 10; 5 is the largest ≤ 6)
//   qty=12 → 6   (1, 2, 3, 4, 6, 12; 6 fits exactly)
//   qty=7  → 1   (1, 7; only 1 fits)
//   qty=1  → 1
pub fn per_trade_for(quantity: u32) -> u32 {
    if quantity == 0 {
        return 1;
    }
    let start = quantity.min(MAX_PER_TRADE);
    for d in (1..=start).rev() {
        if quantity.is_multiple_of(d) {
            return d;
        }
    }
    1
}

/// One of the user's live WFM orders, as much as reconciliation needs.
pub struct ExistingOrder {
    pub id: String,
    pub platinum: u64,
    pub quantity: u64,
}

/// (itemId, order type, rank, subtype) - the identity WFM enforces uniqueness
/// on (a second order with the same key 403s with
/// `app.order.error.exceededOrderLimitSamePrice`).
pub type OrderKey = (String, String, Option<u64>, Option<String>);

fn index_one_order(
    out: &mut BTreeMap<OrderKey, ExistingOrder>,
    o: &serde_json::Value,
    bucket_type: Option<&str>,
) {
    let Some(id) = o.get("id").and_then(|v| v.as_str()) else { return };
    let Some(item_id) = o.get("itemId").and_then(|v| v.as_str()) else { return };
    let Some(ty) = o.get("type").and_then(|v| v.as_str()).or(bucket_type) else { return };
    out.insert(
        (
            item_id.to_string(),
            ty.to_string(),
            o.get("rank").and_then(|v| v.as_u64()),
            o.get("subtype").and_then(|v| v.as_str()).map(str::to_string),
        ),
        ExistingOrder {
            id: id.to_string(),
            platinum: o.get("platinum").and_then(|v| v.as_u64()).unwrap_or(0),
            quantity: o.get("quantity").and_then(|v| v.as_u64()).unwrap_or(0),
        },
    );
}

/// Index a /v2/orders/user/<username> response by OrderKey. Tolerates both
/// shapes WFM has shipped ({data:{sell,buy}} and flat {data:[...]}), same as
/// catalog::enrich_orders_with_names.
pub fn index_existing_orders(body: &serde_json::Value) -> BTreeMap<OrderKey, ExistingOrder> {
    let mut out = BTreeMap::new();
    let Some(data) = body.get("data") else { return out };
    if let Some(arr) = data.as_array() {
        for o in arr {
            index_one_order(&mut out, o, None);
        }
        return out;
    }
    for bucket in ["sell", "buy"] {
        if let Some(arr) = data.get(bucket).and_then(|v| v.as_array()) {
            for o in arr {
                index_one_order(&mut out, o, Some(bucket));
            }
        }
    }
    out
}

/// The OrderKey this plan item will occupy on WFM. MUST mirror
/// build_order_body's rank/subtype normalization - if the body would send
/// rank 0 by default, the key says Some(0), so it collides with exactly the
/// order WFM would reject as a duplicate.
pub fn plan_item_key(item: &PlanItem, cat: &WfmCatalogItem) -> OrderKey {
    let rank = cat.max_rank.map(|_| u64::from(item.rank.unwrap_or(0)));
    let subtype = if cat.subtypes.is_empty() {
        None
    } else {
        Some(
            item.subtype
                .clone()
                .filter(|s| cat.subtypes.contains(s))
                .unwrap_or_else(|| cat.subtypes.first().cloned().unwrap_or_default()),
        )
    };
    (cat.item_id.clone(), item.order_type.clone(), rank, subtype)
}

// Constructs the JSON body for `POST /v2/order`. Per-field rules captured
// from WFM 400 responses (May 2026):
//   - `itemId`, `type` (not `order_type`!), `platinum`, `quantity`,
//     `visible` are always required.
//   - `perTrade` is always required and capped at 6 (in-game trade
//     slots). Listings with quantity > 6 still work - buyers just split
//     across multiple trades. We default to min(quantity, 6).
//   - `rank` is required for items with `maxRank` in the catalog, and is
//     `app.field.notAllowed` for items without it. Default to 0 (unranked).
//   - `subtype` is required for items with `subtypes[]` in the catalog.
//     Default to the first listed subtype - that's the lowest-value
//     variant by WFM convention (intact relic, unrevealed riven) and
//     matches what the user almost always wants to dump first.
pub fn build_order_body(item: &PlanItem, cat: &WfmCatalogItem) -> serde_json::Value {
    let mut body = serde_json::Map::new();
    body.insert("itemId".into(), serde_json::json!(cat.item_id));
    body.insert("type".into(), serde_json::json!(item.order_type));
    body.insert("platinum".into(), serde_json::json!(item.platinum));
    body.insert("quantity".into(), serde_json::json!(item.quantity));
    body.insert("visible".into(), serde_json::json!(item.visible));
    body.insert("perTrade".into(), serde_json::json!(per_trade_for(item.quantity)));
    if cat.max_rank.is_some() {
        body.insert("rank".into(), serde_json::json!(item.rank.unwrap_or(0)));
    }
    if !cat.subtypes.is_empty() {
        let chosen = item
            .subtype
            .clone()
            .filter(|s| cat.subtypes.contains(s))
            .unwrap_or_else(|| cat.subtypes.first().cloned().unwrap_or_default());
        body.insert("subtype".into(), serde_json::json!(chosen));
    }
    serde_json::Value::Object(body)
}

fn execute_one(
    http: &Client,
    unlocked: &Unlocked,
    item: &PlanItem,
    existing: &BTreeMap<OrderKey, ExistingOrder>,
) -> ItemResult {
    let mk_err = |msg: String| ItemResult {
        slug: item.slug.clone(),
        status: "error".into(),
        message: Some(msg),
        order_id: None,
        action: None,
    };

    // --- safety caps ---
    if item.platinum < MIN_PLATINUM {
        return mk_err(format!("price {}p < min {MIN_PLATINUM}p", item.platinum));
    }
    if item.platinum > MAX_PLATINUM {
        return mk_err(format!("price {}p > max {MAX_PLATINUM}p", item.platinum));
    }
    if let Some(low) = item.reference_low_sell {
        if low > 0 && low > item.platinum * SLUG_MISMATCH_GUARD_MULTIPLIER {
            return mk_err(format!(
                "ref low_sell {low}p is more than {SLUG_MISMATCH_GUARD_MULTIPLIER}× our {}p; \
                 likely a slug mismatch - refusing",
                item.platinum
            ));
        }
    }
    if !matches!(item.order_type.as_str(), "sell" | "buy") {
        return mk_err(format!("order_type {:?} not in (sell, buy)", item.order_type));
    }
    if item.quantity == 0 {
        return mk_err("quantity must be > 0".into());
    }

    // --- resolve slug → item_id ---
    let cat = match unlocked.catalog.get(&item.slug) {
        Some(c) => c,
        None => return mk_err(format!("slug {:?} not in WFM catalog", item.slug)),
    };

    // An order with this exact identity already exists → PATCH it. The plan's
    // quantities come from the current inventory scan (they already count the
    // listed copies), so overwrite price + quantity - never sum. Visibility is
    // left alone: the existing order keeps whatever the user chose on WFM.
    if let Some(prior) = existing.get(&plan_item_key(item, cat)) {
        let patch = serde_json::json!({ "platinum": item.platinum, "quantity": item.quantity });
        let r = patch_one_order(http, unlocked, &prior.id, &patch);
        return if r.status == "ok" {
            ItemResult {
                slug: item.slug.clone(),
                status: "ok".into(),
                message: Some(format!(
                    "updated existing order (was {}p × {})",
                    prior.platinum, prior.quantity
                )),
                order_id: Some(prior.id.clone()),
                action: Some("updated".into()),
            }
        } else {
            mk_err(format!(
                "updating existing order: {}",
                r.message.unwrap_or_else(|| "(no message)".into())
            ))
        };
    }

    let body = build_order_body(item, cat);

    // Order-creation endpoint (verified via the WFM frontend's actual
    // network call, May 2026): POST /v2/order. Singular. /v2/me/orders
    // returns 404 for POST - that path is for GET-list semantics, not
    // create. v2 endpoints rely on the JWT cookie that the website sets
    // (not the Authorization header). We send both so either auth path
    // works - the WFM server picks whichever it understands for v1 vs v2.
    // Header set captured from the live frontend's preflight:
    //   access-control-request-headers: content-type, crossplay, language, platform
    // It uses pure cookie auth - no Authorization header. We mirror that.
    let resp = send_with_retry(
        wfm_client::wfm_authed_headers(
            http.post("https://api.warframe.market/v2/order"),
            &unlocked.platform,
            &unlocked.jwt,
        )
        .json(&body),
        ORDER_RETRY_ATTEMPTS,
    );
    let resp = match resp {
        Ok(r) => r,
        Err(e) => return mk_err(format!("HTTP request failed: {e}")),
    };
    let status = resp.status();
    let resp_body: serde_json::Value = resp.json().unwrap_or(serde_json::Value::Null);
    if !status.is_success() {
        // v2 puts errors under `.error` (object or array of strings); v1 used
        // a top-level `.error` string. Render whatever we can find verbatim
        // so the user sees the real validation message.
        let msg = resp_body
            .get("error")
            .map(|v| v.to_string())
            .unwrap_or_else(|| "(no error message)".to_string());
        return mk_err(format!("WFM HTTP {status}: {msg}"));
    }
    // v2 returns the created order under .data; v1 used .payload.order. Try both.
    let order_id = resp_body
        .pointer("/data/id")
        .or_else(|| resp_body.pointer("/payload/order/id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    ItemResult {
        slug: item.slug.clone(),
        status: "ok".into(),
        message: None,
        order_id,
        action: Some("created".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_existing_orders_reads_both_response_shapes() {
        // Bucketed shape: type comes from the bucket name.
        let bucketed = serde_json::json!({
            "data": {
                "sell": [
                    {"id": "o1", "itemId": "item-a", "platinum": 20, "quantity": 3, "rank": 0},
                ],
                "buy": [
                    {"id": "o2", "itemId": "item-a", "platinum": 5, "quantity": 1, "rank": 0},
                ]
            }
        });
        let idx = index_existing_orders(&bucketed);
        let sell = idx.get(&("item-a".into(), "sell".into(), Some(0), None)).unwrap();
        assert_eq!((sell.id.as_str(), sell.platinum, sell.quantity), ("o1", 20, 3));
        assert!(idx.contains_key(&("item-a".into(), "buy".into(), Some(0), None)));

        // Flat shape: type is a field on the order.
        let flat = serde_json::json!({
            "data": [
                {"id": "o3", "itemId": "item-b", "type": "sell", "platinum": 9, "quantity": 2,
                 "subtype": "radiant"},
            ]
        });
        let idx = index_existing_orders(&flat);
        assert!(idx.contains_key(&("item-b".into(), "sell".into(), None, Some("radiant".into()))));
    }

    #[test]
    fn plan_item_key_mirrors_build_order_body_defaults() {
        // Ranked item, no explicit rank: the body sends rank 0, so the key
        // must say Some(0) - that's the order WFM would call a duplicate.
        let ranked = WfmCatalogItem {
            item_id: "item-r".into(),
            display_name: "Some Arcane".into(),
            max_rank: Some(5),
            subtypes: vec![],
        };
        let item = PlanItem {
            slug: "some_arcane".into(),
            platinum: 20,
            quantity: 1,
            order_type: "sell".into(),
            visible: false,
            rank: None,
            subtype: None,
            reference_low_sell: None,
        };
        let key = plan_item_key(&item, &ranked);
        let body = build_order_body(&item, &ranked);
        assert_eq!(key.2, body.get("rank").and_then(|v| v.as_u64()));
        assert_eq!(key.3, None);

        // Subtyped item, bogus requested subtype: both fall back to the
        // catalog's first entry.
        let relic = WfmCatalogItem {
            item_id: "item-s".into(),
            display_name: "Axi A1 Relic".into(),
            max_rank: None,
            subtypes: vec!["intact".into(), "radiant".into()],
        };
        let item = PlanItem { subtype: Some("nonsense".into()), ..item };
        let key = plan_item_key(&item, &relic);
        let body = build_order_body(&item, &relic);
        assert_eq!(key.2, None);
        assert_eq!(
            key.3.as_deref(),
            body.get("subtype").and_then(|v| v.as_str())
        );
        assert_eq!(key.3.as_deref(), Some("intact"));
    }

    fn cat(name: &str, max_rank: Option<u32>, subtypes: &[&str]) -> WfmCatalogItem {
        WfmCatalogItem {
            item_id: format!("id-{name}"),
            display_name: name.into(),
            max_rank,
            subtypes: subtypes.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn plan_item(slug: &str, rank: Option<u32>, subtype: Option<&str>) -> PlanItem {
        PlanItem {
            slug: slug.into(),
            platinum: 12,
            quantity: 3,
            order_type: "sell".into(),
            visible: false,
            rank,
            subtype: subtype.map(|s| s.into()),
            reference_low_sell: None,
        }
    }

    #[test]
    fn order_body_for_relic_includes_subtype_omits_rank() {
        // Reproducer for the May 2026 400: {"rank":"app.field.notAllowed",
        // "subtype":"app.field.required","perTrade":"app.field.required"}.
        //
        // The four refinements below (here and at the other `cat(...,
        // &["intact", ...])` call sites in this module) are test data only -
        // production reads subtypes[] from WFM's live catalog. They mirror
        // prototype/src/lib/resolver.ts's REFINEMENTS set by hand; keep both
        // in sync if WFM ever adds a refinement tier.
        let cat = cat("neo_b2_relic", None, &["intact", "exceptional", "flawless", "radiant"]);
        let item = plan_item("neo_b2_relic", None, None);
        let body = build_order_body(&item, &cat);
        assert_eq!(body["itemId"], "id-neo_b2_relic");
        assert_eq!(body["type"], "sell");
        assert_eq!(body["platinum"], 12);
        assert_eq!(body["quantity"], 3);
        assert_eq!(body["visible"], false);
        assert_eq!(body["perTrade"], 3);
        assert_eq!(body["subtype"], "intact");          // default to first
        assert!(body.get("rank").is_none(), "rank must be absent for non-rankable items");
    }

    #[test]
    fn order_body_for_mod_includes_rank_omits_subtype() {
        let cat = cat("creeping_bullseye", Some(5), &[]);
        let item = plan_item("creeping_bullseye", None, None);
        let body = build_order_body(&item, &cat);
        assert_eq!(body["rank"], 0); // default for unmaxed
        assert!(body.get("subtype").is_none());
    }

    #[test]
    fn order_body_respects_explicit_rank_for_mods() {
        let cat = cat("creeping_bullseye", Some(5), &[]);
        let item = plan_item("creeping_bullseye", Some(5), None);
        let body = build_order_body(&item, &cat);
        assert_eq!(body["rank"], 5);
    }

    #[test]
    fn order_body_uses_user_subtype_when_valid() {
        let cat = cat("neo_b2_relic", None, &["intact", "exceptional", "flawless", "radiant"]);
        let item = plan_item("neo_b2_relic", None, Some("radiant"));
        let body = build_order_body(&item, &cat);
        assert_eq!(body["subtype"], "radiant");
    }

    #[test]
    fn order_body_falls_back_to_first_when_user_subtype_invalid() {
        // Don't silently send a bogus subtype WFM will reject.
        let cat = cat("neo_b2_relic", None, &["intact", "exceptional", "flawless", "radiant"]);
        let item = plan_item("neo_b2_relic", None, Some("super-radiant"));
        let body = build_order_body(&item, &cat);
        assert_eq!(body["subtype"], "intact");
    }

    #[test]
    fn per_trade_picks_largest_divisor_under_cap() {
        // Reproducer for `app.field.orders.perTradeMustDivideQuantity` -
        // WFM rejects when perTrade does not evenly divide quantity.
        assert_eq!(per_trade_for(27), 3);  // {1,3,9,27} ∩ ≤6 → 3
        assert_eq!(per_trade_for(10), 5);  // {1,2,5,10} ∩ ≤6 → 5
        assert_eq!(per_trade_for(12), 6);  // {1,2,3,4,6,12} ∩ ≤6 → 6
        assert_eq!(per_trade_for(6),  6);  // exact fit
        assert_eq!(per_trade_for(7),  1);  // prime > 6 → only 1 divides
        assert_eq!(per_trade_for(11), 1);  // prime > 6 → 1
        assert_eq!(per_trade_for(1),  1);
        assert_eq!(per_trade_for(0),  1);  // defensive
    }

    #[test]
    fn order_body_per_trade_divides_quantity_for_27_relic_stack() {
        // Reproducer for the May 2026 400 on a 27-relic stack:
        // {"inputs":{"perTrade":"app.field.orders.perTradeMustDivideQuantity"}}.
        // perTrade must EVENLY DIVIDE quantity. Largest divisor of 27 ≤ 6 is 3.
        let cat = cat("neo_b2_relic", None, &["intact", "exceptional", "flawless", "radiant"]);
        let mut item = plan_item("neo_b2_relic", None, None);
        item.quantity = 27;
        let body = build_order_body(&item, &cat);
        assert_eq!(body["quantity"], 27);
        assert_eq!(body["perTrade"], 3);
        // Sanity: 27 must divide perfectly.
        assert_eq!(body["quantity"].as_u64().unwrap() % body["perTrade"].as_u64().unwrap(), 0);
    }

    #[test]
    fn order_body_per_trade_uses_quantity_when_quantity_under_cap() {
        let cat = cat("ash_prime_set", None, &[]);
        let mut item = plan_item("ash_prime_set", None, None);
        item.quantity = 3;
        let body = build_order_body(&item, &cat);
        assert_eq!(body["perTrade"], 3);
    }

    // Parity gate: prototype/src/lib/limits.ts mirrors these three so the UI can
    // reject an out-of-range value before the round-trip. This crate is the
    // source of truth; drift surfaces as the UI accepting a batch the companion
    // then rejects. Both sides read tests/fixtures/limits.json. Kept in plan.rs
    // because MAX_PLAN_ITEMS and MIN_PLATINUM are private here - a test is not
    // a reason to widen their visibility.
    #[test]
    fn listing_limits_match_the_shared_fixture() {
        #[derive(serde::Deserialize)]
        struct Fixture {
            max_plan_items: usize,
            min_platinum: u32,
            max_platinum: u32,
        }
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/limits.json"
        );
        let raw = std::fs::read_to_string(path).expect("read the shared limits fixture");
        let fx: Fixture = serde_json::from_str(&raw).expect("parse the limits fixture");
        assert_eq!(MAX_PLAN_ITEMS, fx.max_plan_items);
        assert_eq!(MIN_PLATINUM, fx.min_platinum);
        assert_eq!(MAX_PLATINUM, fx.max_platinum);
    }
}
