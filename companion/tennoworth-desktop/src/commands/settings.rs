//! Key/value settings and per-item reserve-copy CRUD, plus snapshot history
//! listing — thin pass-throughs to [`crate::db::Db`].

use tauri::State;

use crate::db::{Db, ListingLogEntry, Reserve, SnapshotSummary};

#[tauri::command]
pub fn get_setting(db: State<'_, Db>, key: String) -> Result<Option<String>, String> {
    db.get_setting(&key).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_setting(db: State<'_, Db>, key: String, value: String) -> Result<(), String> {
    db.set_setting(&key, &value).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_reserves(db: State<'_, Db>) -> Result<Vec<Reserve>, String> {
    db.get_reserves().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_reserve(db: State<'_, Db>, slug: String, keep: i64) -> Result<(), String> {
    db.set_reserve(&slug, keep).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_reserve(db: State<'_, Db>, slug: String) -> Result<(), String> {
    db.delete_reserve(&slug).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_snapshots(db: State<'_, Db>, limit: i64) -> Result<Vec<SnapshotSummary>, String> {
    db.list_snapshots(limit).map_err(|e| e.to_string())
}

/// What we listed, when, at what price, and whether it worked — newest first.
/// Written by the listing commands; see `commands::listing::record_plan`.
#[tauri::command]
pub fn list_listing_log(db: State<'_, Db>, limit: i64) -> Result<Vec<ListingLogEntry>, String> {
    db.list_listing_log(limit).map_err(|e| e.to_string())
}
