//! wfm-core — the reusable core of the Warframe companion.
//!
//! Everything the companion does that is NOT terminal/HTTP-adapter glue lives
//! here: process detection + memory scan, DE inventory fetch, warframe.market
//! auth + encrypted-JWT storage, the listing/order service, pending-plan
//! persistence, and the DeepSeek assistant relay. The CLI (`wfm-fetch-inventory`)
//! is the first adapter over this crate; a Tauri desktop shell will be the
//! second.
//!
//! Design rule: **no interactive terminal I/O in this crate.** Where the CLI
//! reads a passphrase from a TTY, it does so itself and hands the plaintext to
//! `wfm-core` as a parameter. (A handful of best-effort, non-interactive
//! `eprintln!` diagnostics — pending-plan write warnings, a loose-key-perms
//! warning — are preserved verbatim from the pre-extraction binary.)

pub mod assistant;
pub mod auth;
pub mod catalog;
pub mod error;
pub mod inventory;
pub mod listing;
pub mod pending;
pub mod plan;
pub mod platform;
pub mod scan;
pub mod util;

// WFM is behind Cloudflare with bot protection. A non-browser UA gets a 1015
// rate-limit error or a JS challenge before our request ever reaches the API.
// This is the value proven against production traffic.
//
// Re-exported from wfm-client rather than declared again. The two copies were
// byte-identical and each carried a "bump both together" comment — an
// instruction a compiler cannot enforce and a reader can miss. Now there is
// one string. The Python scrapers still mirror it by hand; that pair is not
// something a Rust re-export can reach.
pub use wfm_client::BROWSER_UA;

/// This crate's version string. Trivial, side-effect-free entry point an
/// adapter (CLI or the Tauri desktop shell) can call to confirm it is linked
/// against a live `wfm-core`.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
