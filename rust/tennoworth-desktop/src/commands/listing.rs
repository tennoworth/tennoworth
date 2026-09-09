//! WFM listing/order commands - the desktop mirror of serve's listing
//! routes: same wfm-core services (`wfm_core::trading::listing` for single-order
//! CRUD, `wfm_core::trading::plan` for the bulk-plan executor), gated on
//! [`WfmSession`]'s unlock state instead of serve's lazy-JWT-unlock.
#![allow(
    clippy::unreachable,
    reason = "tauri::command injects unreachable code into async wrappers"
)]

use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

use wfm_core::trading::listing::{
    bulk_set_visibility, delete_order as core_delete_order, list_user_orders,
    update_order as core_update_order, PerOrderResult, UpdateRequest, VisibilityRequest,
    MAX_PLATINUM,
};
use wfm_core::trading::pending::{clear_pending, load_pending, PendingPlan};
use wfm_core::trading::plan::{
    execute_plan as core_execute_plan, run_pending, PlanItem, PlanRequest, PlanResponse,
    PlanValidationError,
};

use crate::persistence::{Db, ListingLogRow};
use crate::services::wfm_session::{CmdError, WfmSession};

const PLAN_BUSY_MSG: &str = "A listing plan is already running - wait for it to finish.";

fn validate_session_plan(app: &AppHandle, items: &[PlanItem]) -> Result<(), PlanValidationError> {
    validate_protected_plan(app, items)?;
    if items.iter().all(|i| i.session.is_none()) {
        return Ok(());
    }
    let context = items
        .first()
        .and_then(|i| i.session.as_ref())
        .ok_or("Mixed listing and Trade Session batch.")?;
    let db = app.state::<Db>();
    let allowance = db.session_allowance(
        context.snapshot_id,
        context.utc_day,
        crate::services::allowance::unix_now(),
    )?;
    let market = crate::services::sellables::MarketData::load(
        &app.state::<crate::services::market::MarketCache>(),
    );
    let quantities = market.session_quantities(&db)?;
    let session = app.state::<Arc<WfmSession>>();
    let unlocked = session
        .require_unlocked()
        .map_err(|_| "Unlock WFM before reviewing this batch.")?;
    validate_session_contents(
        items,
        context,
        allowance.remaining,
        &quantities,
        &unlocked.catalog,
        &market.session_recipes(),
    )
    .map_err(PlanValidationError::from)
}

fn validate_protected_plan(app: &AppHandle, items: &[PlanItem]) -> Result<(), PlanValidationError> {
    let db = app.state::<Db>();
    let protection = crate::services::protection::ProtectionPlan::load(&db)?;
    if !protection.active(&db)?
        && items
            .iter()
            .all(|item| item.session.is_none() && !item.slug.ends_with("_set"))
    {
        return Ok(());
    }
    let market = crate::services::sellables::MarketData::load(
        &app.state::<crate::services::market::MarketCache>(),
    );
    let session = app.state::<Arc<WfmSession>>();
    let unlocked = session
        .require_unlocked()
        .map_err(|_| "Unlock WFM before validating protected quantities.")?;
    // Invalid rows cannot reach a mutation. Let the executor report its
    // per-item errors without requiring a live order book for an empty run.
    if items.iter().all(|item| {
        item.platinum < wfm_core::trading::plan::MIN_PLATINUM
            || item.platinum > MAX_PLATINUM
            || item.quantity == 0
            || !unlocked.catalog.contains_key(&item.slug)
    }) {
        return Ok(());
    }
    let body = list_user_orders(&unlocked).map_err(PlanValidationError::Market)?;
    validate_protected_contents(&db, &market, &body, items).map_err(PlanValidationError::from)
}

fn validate_protected_contents(
    db: &Db,
    market: &crate::services::sellables::MarketData,
    body: &serde_json::Value,
    items: &[PlanItem],
) -> Result<(), String> {
    let data = body.get("data").unwrap_or(body);
    let rows = data
        .as_array()
        .or_else(|| data.get("sell").and_then(|v| v.as_array()))
        .ok_or("Current orders are unavailable.")?;
    if rows.iter().any(|row| {
        row.get("rank")
            .is_some_and(|v| !v.is_null() && v.as_u64().is_none())
    }) {
        return Err("An existing order has an unresolved rank identity.".into());
    }
    let sells: Vec<_> = items
        .iter()
        .filter(|item| item.order_type == "sell")
        .collect();
    if sells
        .iter()
        .any(|item| item.rank.unwrap_or(0) != 0 || item.subtype.is_some())
    {
        return Err("This protected plan supports confirmed unranked, unsubtyped items only. Review the item identity before listing.".into());
    }
    let mut projected: Vec<_> = rows
        .iter()
        .filter(|row| {
            if row.get("type").and_then(|v| v.as_str()) == Some("buy") {
                return false;
            }
            !sells.iter().any(|item| {
                row.get("item")
                    .and_then(|i| i.get("slug"))
                    .and_then(|v| v.as_str())
                    == Some(item.slug.as_str())
                    && row.get("rank").and_then(|v| v.as_u64()).unwrap_or(0) == 0
                    && row.get("subtype").is_none_or(|v| v.is_null())
            })
        })
        .cloned()
        .collect();
    for item in &sells {
        projected.push(
            serde_json::json!({"type":"sell", "item":{"slug":item.slug}, "quantity":item.quantity}),
        );
    }
    let projected_body = serde_json::json!({"data":{"sell":projected}});
    let state = crate::services::protection::state(db, market, Ok(projected_body))?;
    if state.snapshot_id.is_none() {
        return Err("Scan inventory before listing from a protected plan.".into());
    }
    for item in sells {
        let parts = if item.slug.ends_with("_set") {
            market.set_recipe(&item.slug)?
        } else {
            std::collections::BTreeMap::from([(item.slug.clone(), 1)])
        };
        for slug in parts.keys() {
            let row = state
                .items
                .get(slug)
                .ok_or_else(|| format!("{slug}: no confirmed inventory for this listing."))?;
            if row.available.is_none()
                || row.protected.saturating_add(row.listed.unwrap_or(u32::MAX)) > row.owned
            {
                return Err(format!("{slug}: the batch would consume protected or already allocated copies. Scan and review again."));
            }
        }
    }
    Ok(())
}

fn validate_session_contents(
    items: &[PlanItem],
    context: &wfm_core::trading::plan::SessionConstraint,
    remaining: Option<u32>,
    quantities: &std::collections::BTreeMap<String, u32>,
    catalog: &std::collections::BTreeMap<String, wfm_core::trading::catalog::WfmCatalogItem>,
    recipes: &std::collections::BTreeMap<String, std::collections::BTreeMap<String, u32>>,
) -> Result<(), String> {
    if context.budget == 0
        || context.budget as usize > wfm_core::trading::plan::MAX_PLAN_ITEMS
        || items.iter().any(|i| i.session.as_ref() != Some(context))
    {
        return Err("Invalid or inconsistent Trade Session budget.".into());
    }
    let mut trades = 0u32;
    let mut seen = std::collections::BTreeSet::new();
    for item in items {
        let cat = catalog
            .get(&item.slug)
            .filter(|c| c.session_supported)
            .ok_or_else(|| {
                format!(
                    "{} has an unsupported trade identity. Use the dedicated item flow instead.",
                    item.slug
                )
            })?;
        let lot = item
            .per_trade
            .ok_or("Trade Session requires explicit units per trade.")?;
        wfm_core::trading::plan::validate_lot(item.quantity, Some(lot), cat.bulk_tradable)?;
        if item.platinum.saturating_mul(lot) > MAX_PLATINUM {
            return Err(format!(
                "Lot total exceeds {MAX_PLATINUM}p; reduce price or units per trade."
            ));
        }
        if item.visible
            || item.order_type != "sell"
            || item.rank.unwrap_or(0) != 0
            || item.subtype.is_some()
            || (item.slug.ends_with("_set") && (!recipes.contains_key(&item.slug) || lot != 1))
            || item.reviewed_order.is_none()
            || !seen.insert(&item.slug)
        {
            return Err("Unsupported or ambiguous Trade Session item identity.".into());
        }
        if item.quantity > quantities.get(&item.slug).copied().unwrap_or(0) {
            return Err(format!(
                "{} no longer has the reviewed sellable quantity. Scan and review again.",
                item.slug
            ));
        }
        trades = trades
            .checked_add(item.quantity / lot)
            .ok_or("Trade estimate is too large.")?;
    }
    if trades > context.budget || trades > remaining.unwrap_or(0) {
        return Err(
            "The batch exceeds the current trade allowance or selected budget. Review it again."
                .into(),
        );
    }
    Ok(())
}

/// Append a finished plan run to `listing_log`.
///
/// Best-effort by design: the orders already exist on WFM by the time this
/// runs, so a store write must never turn a successful listing into a reported
/// failure. Losing a local row is strictly less bad than lying about the trade.
///
/// `price_qty` is the plan's own items, positionally aligned with
/// `response.results` - `run_pending` emits exactly one result per item, in
/// order. When the lengths disagree the executor took an early-exit path
/// (empty batch, over the item cap, HTTP client build failure) and returned a
/// single synthetic `<batch>` result that maps to no item, including when the
/// original plan contained only one item. Neither case belongs in item history.
fn record_plan(db: &Db, response: &PlanResponse, price_qty: &[(i64, i64)]) {
    if response.results.is_empty()
        || response.results.len() != price_qty.len()
        || response.results.iter().any(|r| r.slug == "<batch>")
    {
        return;
    }
    let rows: Vec<ListingLogRow> = response
        .results
        .iter()
        .zip(price_qty)
        .map(|(r, &(price, qty))| ListingLogRow {
            slug: r.slug.clone(),
            price,
            qty,
            status: r.status.clone(),
            action: r.action.clone(),
            order_id: r.order_id.clone(),
            message: r.message.clone(),
        })
        .collect();
    if let Err(e) = db.insert_listing_log(&response.plan_id, &rows) {
        eprintln!("warning: could not record listing log: {e}");
    }
}

/// Execute a listing batch - the desktop POST /plan. Pacing, caps, pending-file
/// persistence, and per-item results all come from wfm-core's execute_plan.
#[tauri::command]
pub async fn submit_plan(
    app: AppHandle,
    session: State<'_, Arc<WfmSession>>,
    db: State<'_, Db>,
    items: Vec<PlanItem>,
    request_id: String,
) -> Result<PlanResponse, CmdError> {
    // Captured before the call: execute_plan consumes `items` to seed the
    // pending file, and the price/qty the user actually asked for is not
    // recoverable from the response.
    let price_qty: Vec<(i64, i64)> = items
        .iter()
        .map(|i| (i.platinum as i64, i.quantity as i64))
        .collect();
    let s = Arc::clone(&session);
    let reviewed = items.clone();
    let response = tauri::async_runtime::spawn_blocking(move || {
        let request = s.claim_plan_request(request_id)?;
        let _guard = s
            .begin_plan()
            .ok_or_else(|| CmdError::of("busy", PLAN_BUSY_MSG))?;
        let unlocked = s.require_unlocked()?;
        if load_pending(s.pending_path()).is_some_and(|plan| plan.items.iter().any(|item| item.status == "pending")) {
            return Err(CmdError::of("busy", "An unfinished batch is saved. Resume or discard it before sending another."));
        }
        Ok::<_, CmdError>(wfm_client::governor::with_context(request.context(), || core_execute_plan(
            s.pending_path(),
            &unlocked,
            PlanRequest { items },
            || validate_session_plan(&app, &reviewed),
        )))
    })
    .await
    .map_err(|e| CmdError::internal(format!("plan task failed to run: {e}")))??;

    record_plan(&db, &response, &price_qty);
    Ok(response)
}

/// The last interrupted plan, or null. No auth - mirrors serve's JWT-free
/// GET /plan/pending, so the SPA can poll it before any unlock.
#[tauri::command]
pub fn get_pending_plan(session: State<'_, Arc<WfmSession>>) -> Option<PendingPlan> {
    load_pending(session.pending_path())
}

#[tauri::command]
pub fn discard_pending_plan(session: State<'_, Arc<WfmSession>>) -> Result<(), CmdError> {
    let _guard = session.begin_plan().ok_or_else(|| CmdError::of("busy", PLAN_BUSY_MSG))?;
    clear_pending(session.pending_path());
    Ok(())
}
#[tauri::command]
pub fn cancel_plan(session: State<'_, Arc<WfmSession>>, request_id: String) -> Result<(), CmdError> {
    session.cancel_plan(&request_id)
}

/// Re-run the pending plan, skipping items already in a terminal state.
#[tauri::command]
pub async fn resume_pending_plan(
    app: AppHandle,
    session: State<'_, Arc<WfmSession>>,
    db: State<'_, Db>,
    request_id: String,
) -> Result<PlanResponse, CmdError> {
    let s = Arc::clone(&session);
    let (response, price_qty) = tauri::async_runtime::spawn_blocking(move || {
        // Pending-first ordering mirrors serve (its 404 outranks auth): with
        // nothing to resume the user must not be bounced into a login dialog.
        let mut pending = load_pending(s.pending_path())
            .ok_or_else(|| CmdError::of("no_pending", "No pending plan to resume."))?;
        let request = s.claim_plan_request(request_id)?;
        let _guard = s
            .begin_plan()
            .ok_or_else(|| CmdError::of("busy", PLAN_BUSY_MSG))?;
        let unlocked = s.require_unlocked()?;
        // Stable plan positions let history upsert prior successes during resume.
        let price_qty: Vec<(i64, i64)> = pending
            .items
            .iter()
            .map(|i| (i.platinum as i64, i.quantity as i64))
            .collect();
        let reviewed: Vec<PlanItem> = pending.items.iter().map(PlanItem::from).collect();
        let response = wfm_client::governor::with_context(request.context(), || run_pending(s.pending_path(), &unlocked, &mut pending, || {
            validate_session_plan(&app, &reviewed)
        }));
        if pending.items.iter().all(|i| i.status != "pending") {
            clear_pending(s.pending_path());
        }
        Ok::<_, CmdError>((response, price_qty))
    })
    .await
    .map_err(|e| CmdError::internal(format!("resume task failed to run: {e}")))??;

    record_plan(&db, &response, &price_qty);
    Ok(response)
}

/// The user's current WFM listings, enriched with display names (GET /orders).
#[tauri::command]
pub async fn fetch_orders(
    session: State<'_, Arc<WfmSession>>,
) -> Result<serde_json::Value, CmdError> {
    let s = Arc::clone(&session);
    tauri::async_runtime::spawn_blocking(move || {
        let unlocked = s.require_unlocked()?;
        wfm_core::trading::listing::cached_user_orders(&unlocked).map_err(CmdError::wfm)
    })
    .await
    .map_err(|e| CmdError::internal(format!("orders task failed to run: {e}")))?
}

/// PATCH one order: price / quantity / visible / rank.
#[tauri::command]
pub async fn update_order(
    app: AppHandle,
    session: State<'_, Arc<WfmSession>>,
    order_id: String,
    patch: UpdateRequest,
) -> Result<PerOrderResult, CmdError> {
    // Same cap as the create path - mirrors serve's pre-auth 400 so an edit
    // can't push a listing past what the WFM UI allows.
    if let Some(p) = patch.platinum {
        if p > MAX_PLATINUM {
            return Err(CmdError::of("wfm", format!("price {p}p > max {MAX_PLATINUM}p")));
        }
    }
    let s = Arc::clone(&session);
    tauri::async_runtime::spawn_blocking(move || {
        let unlocked = s.require_unlocked()?;
        let _guard = s.begin_plan().ok_or_else(|| CmdError::of("busy", PLAN_BUSY_MSG))?;
        let protection = crate::services::protection::ProtectionPlan::load(&app.state::<Db>()).map_err(CmdError::internal)?;
        if protection.active(&app.state::<Db>()).map_err(CmdError::internal)? && (patch.quantity.is_some() || patch.rank.is_some()) {
            let market = crate::services::sellables::MarketData::load(&app.state::<crate::services::market::MarketCache>());
            let body = list_user_orders(&unlocked).map_err(CmdError::wfm)?;
            let data = body.get("data").unwrap_or(&body);
            let rows = data.as_array().or_else(|| data.get("sell").and_then(|v| v.as_array())).ok_or_else(|| CmdError::internal("Current orders unavailable."))?;
            let row = rows.iter().find(|row| row.get("id").and_then(|v| v.as_str()) == Some(order_id.as_str())).ok_or_else(|| CmdError::internal("The order changed; refresh orders."))?;
            if row.get("type").and_then(|v| v.as_str()) != Some("buy") {
                let item: PlanItem = serde_json::from_value(serde_json::json!({
                    "slug":row.get("item").and_then(|i| i.get("slug")),
                    "quantity":patch.quantity.map(serde_json::Value::from).or_else(|| row.get("quantity").cloned()),
                    "rank":patch.rank.map(serde_json::Value::from).or_else(|| row.get("rank").cloned()),
                    "subtype":row.get("subtype"), "platinum":patch.platinum.unwrap_or(5), "visible":false, "order_type":"sell"
                })).map_err(|e| CmdError::internal(e.to_string()))?;
                validate_protected_contents(&app.state::<Db>(), &market, &body, &[item]).map_err(CmdError::internal)?;
            }
        }
        core_update_order(&unlocked, &order_id, &patch).map_err(CmdError::wfm)
    })
    .await
    .map_err(|e| CmdError::internal(format!("order update task failed to run: {e}")))?
}

#[tauri::command]
pub async fn delete_order(
    session: State<'_, Arc<WfmSession>>,
    order_id: String,
) -> Result<(), CmdError> {
    let s = Arc::clone(&session);
    tauri::async_runtime::spawn_blocking(move || {
        let unlocked = s.require_unlocked()?;
        core_delete_order(&unlocked, &order_id).map_err(CmdError::wfm)
    })
    .await
    .map_err(|e| CmdError::internal(format!("order delete task failed to run: {e}")))?
}

/// Bulk-toggle listing visibility (POST /orders/visibility). Per-order results;
/// pacing lives in wfm-core's bulk_set_visibility.
#[tauri::command]
pub async fn bulk_visibility(
    session: State<'_, Arc<WfmSession>>,
    order_ids: Vec<String>,
    visible: bool,
) -> Result<Vec<PerOrderResult>, CmdError> {
    let s = Arc::clone(&session);
    tauri::async_runtime::spawn_blocking(move || {
        let unlocked = s.require_unlocked()?;
        Ok(bulk_set_visibility(
            &unlocked,
            &VisibilityRequest { order_ids, visible },
        ))
    })
    .await
    .map_err(|e| CmdError::internal(format!("visibility task failed to run: {e}")))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use wfm_core::trading::catalog::WfmCatalogItem;
    use wfm_core::trading::plan::SessionConstraint;

    #[test]
    fn protection_checks_projected_orders_and_recipe_multiplicity() {
        let db = Db::open(std::path::Path::new(":memory:")).unwrap();
        let market = serde_json::from_value(serde_json::json!({
            "items":{"barrel":{"vol":10,"low_sell":10},"test_set":{"vol":10,"low_sell":50}},
            "path_to_info":{"/Lotus/Barrel":{"name":"Barrel","slug":"barrel"}},
            "set_to_parts":{"test_set":{"parts":[{"slug":"barrel","quantity":2}]}}
        }))
        .unwrap();
        db.insert_snapshot(
            "memory",
            None,
            None,
            &[crate::persistence::SnapshotItem {
                slug: "/Lotus/Barrel".into(),
                count: 5,
                leveled: 0,
            }],
        )
        .unwrap();
        let plan = crate::services::protection::ProtectionPlan {
            reserves: BTreeMap::new(),
            goal: Some("test_set".into()),
        };
        plan.save(&db, &market).unwrap();
        let orders = serde_json::json!({"data":{"sell":[{"id":"set-order","item":{"slug":"test_set"},"quantity":1}]}});
        let item: PlanItem = serde_json::from_value(serde_json::json!({"slug":"barrel","quantity":1,"platinum":10,"order_type":"sell","visible":false})).unwrap();
        assert!(
            validate_protected_contents(&db, &market, &orders, std::slice::from_ref(&item)).is_ok()
        );
        let mut too_many = item.clone();
        too_many.quantity = 2;
        assert!(validate_protected_contents(&db, &market, &orders, &[too_many]).is_err());
        let same_order = serde_json::json!({"data":{"sell":[{"id":"part-order","item":{"slug":"barrel"},"quantity":3}]}});
        assert!(
            validate_protected_contents(&db, &market, &same_order, std::slice::from_ref(&item))
                .is_ok(),
            "replacement quantities do not add to the old quantity"
        );
        db.insert_snapshot(
            "memory",
            None,
            None,
            &[crate::persistence::SnapshotItem {
                slug: "/Lotus/Barrel".into(),
                count: 2,
                leveled: 0,
            }],
        )
        .unwrap();
        assert!(
            validate_protected_contents(&db, &market, &same_order, &[item]).is_err(),
            "a later scan cannot leak protected copies"
        );
    }

    #[test]
    fn session_validation_rejects_unsafe_quantities_budgets_and_identities() {
        let context = SessionConstraint {
            snapshot_id: 1,
            utc_day: 20_000,
            budget: 8,
        };
        let item: PlanItem = serde_json::from_value(serde_json::json!({
            "slug":"arcane", "platinum":20, "quantity":12, "per_trade":3,
            "order_type":"sell", "visible":false, "session":context, "reviewed_order":{"state":"new"}
        })).unwrap();
        let mut catalog = BTreeMap::from([(
            "arcane".into(),
            WfmCatalogItem {
                item_id: "item".into(),
                display_name: "Arcane".into(),
                bulk_tradable: true,
                session_supported: true,
                max_rank: Some(5),
                subtypes: vec![],
            },
        )]);
        let mut quantities = BTreeMap::from([("arcane".into(), 12)]);
        let check = |items: &[PlanItem],
                     remaining,
                     qty: &BTreeMap<String, u32>,
                     cat: &BTreeMap<String, WfmCatalogItem>| {
            validate_session_contents(items, &context, remaining, qty, cat, &BTreeMap::new())
        };
        assert!(check(std::slice::from_ref(&item), Some(8), &quantities, &catalog).is_ok());
        for remaining in [None, Some(0), Some(3)] {
            assert!(check(
                std::slice::from_ref(&item),
                remaining,
                &quantities,
                &catalog
            )
            .is_err());
        }
        quantities.insert("arcane".into(), 11);
        assert!(check(std::slice::from_ref(&item), Some(8), &quantities, &catalog).is_err());
        quantities.insert("arcane".into(), 12);
        let mut changed = item.clone();
        changed.rank = Some(5);
        assert!(check(&[changed], Some(8), &quantities, &catalog).is_err());
        let mut changed = item.clone();
        changed.quantity = 11;
        assert!(check(&[changed], Some(8), &quantities, &catalog).is_err());
        let mut changed = item.clone();
        changed.reviewed_order = None;
        assert!(check(&[changed], Some(8), &quantities, &catalog).is_err());
        assert!(check(
            &[item.clone(), item.clone()],
            Some(8),
            &quantities,
            &catalog
        )
        .is_err());
        catalog.get_mut("arcane").unwrap().bulk_tradable = false;
        assert!(check(std::slice::from_ref(&item), Some(8), &quantities, &catalog).is_err());
        catalog.get_mut("arcane").unwrap().session_supported = false;
        assert!(check(&[item], Some(8), &quantities, &catalog).is_err());
    }
}
