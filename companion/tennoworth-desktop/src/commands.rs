//! Tauri IPC command handlers, grouped by domain. Every command here is a
//! thin adapter over wfm-core or a local module (db/market/sellables) — see
//! each submodule's doc comment for its slice of the SPA's Transport
//! contract. `main.rs` only wires these into `generate_handler!`.

pub mod assistant;
pub mod inventory;
pub mod listing;
pub mod market;
pub mod settings;
