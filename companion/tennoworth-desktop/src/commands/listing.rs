//! WFM listing/order commands — the desktop mirror of serve's listing
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

use crate::wfm_session::{CmdError, WfmSession};

const PLAN_BUSY_MSG: &str = "A listing plan is already running — wait for it to finish.";

/// Execute a listing batch — the desktop POST /plan. Pacing, caps, pending-file
/// persistence, and per-item results all come from wfm-core's execute_plan.
#[tauri::command]
pub async fn submit_plan(
    session: State<'_, Arc<WfmSession>>,
    items: Vec<PlanItem>,
) -> Result<PlanResponse, CmdError> {
    let s = Arc::clone(&session);
    tauri::async_runtime::spawn_blocking(move || {
        let unlocked = s.require_unlocked()?;
        let _guard = s.begin_plan().ok_or_else(|| CmdError::of("busy", PLAN_BUSY_MSG))?;
        Ok(core_execute_plan(s.pending_path(), &unlocked, PlanRequest { items }))
    })
    .await
    .map_err(|e| CmdError::internal(format!("plan task failed to run: {e}")))?
}

/// The last interrupted plan, or null. No auth — mirrors serve's JWT-free
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
) -> Result<PlanResponse, CmdError> {
    let s = Arc::clone(&session);
    tauri::async_runtime::spawn_blocking(move || {
        // Pending-first ordering mirrors serve (its 404 outranks auth): with
        // nothing to resume the user must not be bounced into a login dialog.
        let mut pending = load_pending(s.pending_path())
            .ok_or_else(|| CmdError::of("no_pending", "No pending plan to resume."))?;
        let unlocked = s.require_unlocked()?;
        let _guard = s.begin_plan().ok_or_else(|| CmdError::of("busy", PLAN_BUSY_MSG))?;
        let response = run_pending(s.pending_path(), &unlocked, &mut pending);
        clear_pending(s.pending_path());
        Ok(response)
    })
    .await
    .map_err(|e| CmdError::internal(format!("resume task failed to run: {e}")))?
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
    // Same cap as the create path — mirrors serve's pre-auth 400 so an edit
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
