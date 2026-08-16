//! Price-watch commands: CRUD + an on-demand pass.

use tauri::{AppHandle, State};

use crate::db::{Db, NewWatch, Watch};
use crate::watch::{run_pass, WatchOutcome, MAX_WATCHES};
use crate::wfm_session::CmdError;

#[tauri::command]
pub fn list_watches(db: State<'_, Db>) -> Result<Vec<Watch>, CmdError> {
    db.list_watches().map_err(|e| CmdError::internal(format!("list watches: {e}")))
}

#[tauri::command]
pub fn add_watch(db: State<'_, Db>, watch: NewWatch) -> Result<Vec<Watch>, CmdError> {
    if watch.side != "sell" && watch.side != "buy" {
        return Err(CmdError::of("bad_request", "side must be 'sell' or 'buy'"));
    }
    if watch.threshold < 1 {
        return Err(CmdError::of("bad_request", "threshold must be at least 1p"));
    }
    if watch.slug.trim().is_empty() || watch.name.trim().is_empty() {
        return Err(CmdError::of("bad_request", "slug and name are required"));
    }
    let existing = db.list_watches().map_err(|e| CmdError::internal(format!("list watches: {e}")))?;
    if existing.len() >= MAX_WATCHES {
        return Err(CmdError::of("too_many", format!("at most {MAX_WATCHES} watches — delete one first")));
    }
    db.add_watch(&watch, None).map_err(|e| CmdError::internal(format!("add watch: {e}")))?;
    db.list_watches().map_err(|e| CmdError::internal(format!("list watches: {e}")))
}

#[tauri::command]
pub fn delete_watch(db: State<'_, Db>, id: i64) -> Result<Vec<Watch>, CmdError> {
    db.delete_watch(id).map_err(|e| CmdError::internal(format!("delete watch: {e}")))?;
    db.list_watches().map_err(|e| CmdError::internal(format!("list watches: {e}")))
}

/// Run one pass right now (the same one the background loop runs) and return
/// every outcome, fired or not — the UI shows "12p as of just now" per row.
#[tauri::command]
pub async fn check_watches_now(app: AppHandle) -> Result<Vec<WatchOutcome>, CmdError> {
    tauri::async_runtime::spawn_blocking(move || run_pass(&app))
        .await
        .map_err(|e| CmdError::internal(format!("watch pass failed to run: {e}")))
}
