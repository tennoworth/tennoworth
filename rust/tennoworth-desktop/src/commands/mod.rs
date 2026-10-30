//! Tauri IPC command handlers, grouped by domain. Every command here is a
//! thin adapter over the native crates or a local module (db/market/sellables) - see
//! each submodule's doc comment for its slice of the SPA's Transport
//! contract. `main.rs` only wires these into `generate_handler!`.

pub mod auth;
pub mod auto_scan;
pub mod inventory;
pub mod listing;
pub mod market;
pub mod presence;
pub mod reports;
pub mod settings;
pub mod trades;
pub mod watches;

#[cfg(test)]
mod bindings;

pub mod domain;
