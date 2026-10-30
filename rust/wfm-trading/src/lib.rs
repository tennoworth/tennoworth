//! wfm-trading - everything the desktop app does with warframe.market.
//!
//! Sign-in and the encrypted JWT envelope (`auth`), the signed-in user's
//! orders and recoverable listing plans (`orders`), presence and the live
//! order stream (`presence`), and anonymous market reads (`market`). The
//! Tauri desktop shell drives this crate over IPC.
//!
//! Design rule: **no interactive terminal I/O in this crate.** Where the
//! desktop shell needs a passphrase, it collects it in the webview and hands
//! the plaintext over as a parameter. (A few best-effort, non-interactive
//! `eprintln!` diagnostics - pending-plan write warnings, a loose-key-perms
//! warning - remain.)

pub mod auth;
pub mod http;
pub mod identity;
pub mod market;
pub mod orders;
pub mod paths;
pub mod platform;
pub mod presence;
pub mod time;

// WFM is behind Cloudflare with bot protection. A non-browser UA gets a 1015
// rate-limit error or a JS challenge before our request ever reaches the API.
// This is the value proven against production traffic.
//
/// The `User-Agent` every WFM call from this crate sends - see
/// [`wfm_client::user_agent`] for the format WFM's rules require. The version
/// is the *binary's* (the desktop app's), not this library's: the app calls
/// [`set_app_identity`] once at startup; before that (tests, tools) the UA
/// carries this crate's own name and version.
pub fn user_agent() -> String {
    let (component, version) = APP_IDENTITY.get().cloned().unwrap_or_else(|| {
        (
            "wfm-trading".to_string(),
            env!("CARGO_PKG_VERSION").to_string(),
        )
    });
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
/// against a live `wfm-trading`.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
