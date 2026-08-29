//! WFM listing/order commands - the desktop mirror of serve's listing
//! routes: same wfm-core services (`wfm_core::listing` for single-order
//! CRUD, `wfm_core::plan` for the bulk-plan executor), gated on
//! [`WfmSession`]'s unlock state instead of serve's lazy-JWT-unlock.

use std::sync::Arc;
use tauri::State;

use wfm_core::listing::{
    bulk_set_visibility, delete_order as core_delete_order, list_user_orders,
    update_order as core_update_order, PerOrderResult, UpdateRequest, VisibilityRequest,
    MAX_PLATINUM,
};
use wfm_core::pending::{clear_pending, load_pending, PendingPlan};
use wfm_core::plan::{
    execute_plan as core_execute_plan, run_pending, PlanItem, PlanRequest, PlanResponse,
};

use crate::db::{Db, ListingLogRow};
use crate::wfm_session::{CmdError, WfmSession};

const PLAN_BUSY_MSG: &str = "A listing plan is already running - wait for it to finish.";

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
/// single synthetic `<batch>` result that maps to no item, so there is nothing
/// item-shaped to record. Checking the length rather than the slug keeps that
/// safe if another early-exit is added later.
fn record_plan(db: &Db, response: &PlanResponse, price_qty: &[(i64, i64)]) {
    if response.results.is_empty() || response.results.len() != price_qty.len() {
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
    session: State<'_, Arc<WfmSession>>,
    db: State<'_, Db>,
    items: Vec<PlanItem>,
) -> Result<PlanResponse, CmdError> {
    // Captured before the call: execute_plan consumes `items` to seed the
    // pending file, and the price/qty the user actually asked for is not
    // recoverable from the response.
    let price_qty: Vec<(i64, i64)> = items
        .iter()
        .map(|i| (i.platinum as i64, i.quantity as i64))
        .collect();
    let s = Arc::clone(&session);
    let response = tauri::async_runtime::spawn_blocking(move || {
        let unlocked = s.require_unlocked()?;
        let _guard = s.begin_plan().ok_or_else(|| CmdError::of("busy", PLAN_BUSY_MSG))?;
        Ok::<_, CmdError>(core_execute_plan(s.pending_path(), &unlocked, PlanRequest { items }))
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
pub fn discard_pending_plan(session: State<'_, Arc<WfmSession>>) {
    clear_pending(session.pending_path());
}

/// Re-run the pending plan, skipping items already in a terminal state.
#[tauri::command]
pub async fn resume_pending_plan(
    session: State<'_, Arc<WfmSession>>,
    db: State<'_, Db>,
) -> Result<PlanResponse, CmdError> {
    let s = Arc::clone(&session);
    let (response, price_qty) = tauri::async_runtime::spawn_blocking(move || {
        // Pending-first ordering mirrors serve (its 404 outranks auth): with
        // nothing to resume the user must not be bounced into a login dialog.
        let mut pending = load_pending(s.pending_path())
            .ok_or_else(|| CmdError::of("no_pending", "No pending plan to resume."))?;
        let unlocked = s.require_unlocked()?;
        let _guard = s.begin_plan().ok_or_else(|| CmdError::of("busy", PLAN_BUSY_MSG))?;
        // EVERY item, not just the ones still 'pending'. A pending file only
        // survives when the original run never returned, and submit_plan logs
        // only after it returns - so nothing from this plan has been recorded
        // yet, including the items that succeeded before the interruption.
        // Skipping the already-terminal ones here would lose them for good.
        let price_qty: Vec<(i64, i64)> = pending
            .items
            .iter()
            .map(|i| (i.platinum as i64, i.quantity as i64))
            .collect();
        let response = run_pending(s.pending_path(), &unlocked, &mut pending);
        clear_pending(s.pending_path());
        Ok::<_, CmdError>((response, price_qty))
    })
    .await
    .map_err(|e| CmdError::internal(format!("resume task failed to run: {e}")))??;

    record_plan(&db, &response, &price_qty);
    Ok(response)
}

/// The user's current WFM listings, enriched with display names (GET /orders).
#[tauri::command]
pub async fn fetch_orders(session: State<'_, Arc<WfmSession>>) -> Result<serde_json::Value, CmdError> {
    let s = Arc::clone(&session);
    tauri::async_runtime::spawn_blocking(move || {
        let unlocked = s.require_unlocked()?;
        list_user_orders(&unlocked).map_err(CmdError::wfm)
    })
    .await
    .map_err(|e| CmdError::internal(format!("orders task failed to run: {e}")))?
}

/// PATCH one order: price / quantity / visible / rank.
#[tauri::command]
pub async fn update_order(
    session: State<'_, Arc<WfmSession>>,
    order_id: String,
    patch: UpdateRequest,
) -> Result<PerOrderResult, CmdError> {
    // Same cap as the create path - mirrors serve's pre-auth 400 so an edit
    // can't push a listing past what the WFM UI allows.
    if let Some(p) = patch.platinum {
        if p > MAX_PLATINUM {
            return Err(CmdError::wfm(format!("price {p}p > max {MAX_PLATINUM}p")));
        }
    }
    let s = Arc::clone(&session);
    tauri::async_runtime::spawn_blocking(move || {
        let unlocked = s.require_unlocked()?;
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
        Ok(bulk_set_visibility(&unlocked, &VisibilityRequest { order_ids, visible }))
    })
    .await
    .map_err(|e| CmdError::internal(format!("visibility task failed to run: {e}")))?
}
