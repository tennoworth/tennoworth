//! wfm-core — the reusable core of the Warframe companion.
//!
//! Everything the app does that is NOT shell/webview glue lives here:
//! process detection + memory scan, DE inventory fetch, warframe.market
//! auth + encrypted-JWT storage, the listing/order service, pending-plan
//! persistence, and the dormant DeepSeek assistant relay. The Tauri desktop
//! shell (`tennoworth-desktop`) drives this crate over IPC.
//!
//! Design rule: **no interactive terminal I/O in this crate.** Where the
//! desktop shell needs a passphrase, it collects it in the webview and hands
//! the plaintext to `wfm-core` as a parameter. (A handful of best-effort,
//! non-interactive `eprintln!` diagnostics — pending-plan write warnings, a
//! loose-key-perms warning — are preserved verbatim from the pre-extraction
//! binary.)

pub mod assistant;
pub mod auth;
pub mod catalog;
pub mod error;
pub mod inventory;
pub mod listing;
pub mod pending;
pub mod plan;
pub mod poison;
pub mod platform;
pub mod scan;
pub mod util;

// WFM is behind Cloudflare with bot protection. A non-browser UA gets a 1015
// rate-limit error or a JS challenge before our request ever reaches the API.
// This is the value proven against production traffic.
//
/// The `User-Agent` every WFM call from this crate sends — see
/// [`wfm_client::user_agent`] for the format WFM's rules require. The version
/// is the *binary's* (the desktop app's), not this library's: the app calls
/// [`set_app_identity`] once at startup; before that (tests, tools) the UA
/// carries wfm-core's own name and version.
pub fn user_agent() -> String {
    let (component, version) = APP_IDENTITY
        .get()
        .cloned()
        .unwrap_or_else(|| ("wfm-core".to_string(), env!("CARGO_PKG_VERSION").to_string()));
    wfm_client::user_agent(&component, &version)
}

static APP_IDENTITY: std::sync::OnceLock<(String, String)> = std::sync::OnceLock::new();

/// Name the binary embedding this crate, once, so [`user_agent`] reports its
/// version. A second call is ignored (first wins).
pub fn set_app_identity(component: &str, version: &str) {
    let _ = APP_IDENTITY.set((component.to_string(), version.to_string()));
}

/// This crate's version string. Trivial, side-effect-free entry point the
/// Tauri desktop shell can call to confirm it is linked
/// against a live `wfm-core`.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
