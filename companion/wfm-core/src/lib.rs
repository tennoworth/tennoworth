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

pub mod auth;
pub mod error;
pub mod inventory;
pub mod pending;
pub mod platform;
pub mod scan;
pub mod util;

// WFM is behind Cloudflare with bot protection. A non-browser UA gets a 1015
// rate-limit error or a JS challenge before our request ever reaches the API.
// Kept byte-identical to the pre-extraction companion UA (do not swap for
// wfm-client's — that crate carries a different Firefox version string).
pub const BROWSER_UA: &str =
    "Mozilla/5.0 (X11; Linux x86_64; rv:130.0) Gecko/20100101 Firefox/130.0";
