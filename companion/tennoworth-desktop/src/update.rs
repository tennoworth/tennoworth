//! C5 auto-update: check the GitHub-releases `latest.json`, notify, install
//! only on explicit confirmation, apply on restart. NO silent updates — the
//! check is the only thing that runs unprompted (at launch), and it downloads
//! nothing but the manifest.
//!
//! Failure posture mirrors market.rs: `check` never panics and never returns
//! Err. Offline, DNS failure, HTTP error, a malformed/truncated manifest, an
//! unsupported platform, or a bad endpoint override all degrade to "no update
//! available" (logged to stderr) — an update check must never crash the app or
//! block launch. A bad *bundle signature* surfaces later, in `install_pending`:
//! the plugin verifies the minisign signature against the pubkey in
//! tauri.conf.json after download and refuses to install on mismatch, which
//! reaches the user as a plain error banner while the running app stays intact.

use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

/// Emitted to the webview when the launch check finds an update. The SPA also
/// reads `update_status` at mount, so a listener registered after the emit
/// still sees the result — the event is the push path, the command the pull.
pub const EVENT_UPDATE_AVAILABLE: &str = "update-available";

/// Manifest fetch cap — a hung check must never hold a pending `check_update`
/// invoke (or the launch task) open indefinitely.
const CHECK_TIMEOUT: Duration = Duration::from_secs(15);

/// What the SPA sees. `checked` distinguishes "no update" from "no check has
/// completed yet" (the launch task may still be in flight when the SPA boots).
#[derive(serde::Serialize, Clone, Debug)]
pub struct UpdateStatus {
    pub checked: bool,
    pub available: bool,
    pub current_version: String,
    pub version: Option<String>,
    pub notes: Option<String>,
}

impl Default for UpdateStatus {
    fn default() -> Self {
        Self {
            checked: false,
            available: false,
            current_version: env!("CARGO_PKG_VERSION").to_string(),
            version: None,
            notes: None,
        }
    }
}

/// Managed state: the last check's outcome (pull surface for the SPA + probe
/// evidence) and the checked `Update` handle `install_pending` consumes — so
/// installing never re-downloads the manifest, and there is no window for a
/// different release to appear between "user saw vX" and "user installed".
#[derive(Default)]
pub struct UpdateState {
    last: Mutex<UpdateStatus>,
    pending: Mutex<Option<Update>>,
}

impl UpdateState {
    pub fn last(&self) -> UpdateStatus {
        self.last.lock().unwrap().clone()
    }

    /// Clone (not take): a failed install must stay retryable.
    fn pending(&self) -> Option<Update> {
        self.pending.lock().unwrap().clone()
    }

    fn store(&self, status: UpdateStatus, update: Option<Update>) {
        *self.last.lock().unwrap() = status;
        *self.pending.lock().unwrap() = update;
    }
}

/// Build the updater, honoring a `TENNOWORTH_UPDATE_URL` endpoint override so
/// the probe can exercise the offline / malformed-manifest paths against a
/// controlled endpoint (the market.rs `TENNOWORTH_MARKET_URL` pattern). None on
/// any builder failure — including a non-https override, which the plugin
/// rejects in release builds.
fn build_updater(app: &AppHandle) -> Option<tauri_plugin_updater::Updater> {
    let mut builder = app.updater_builder().timeout(CHECK_TIMEOUT);
    if let Ok(raw) = std::env::var("TENNOWORTH_UPDATE_URL") {
        let url = match raw.parse() {
            Ok(u) => u,
            Err(e) => {
                eprintln!("tennoworth: TENNOWORTH_UPDATE_URL invalid ({raw}): {e}");
                return None;
            }
        };
        builder = match builder.endpoints(vec![url]) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("tennoworth: update endpoint override rejected: {e}");
                return None;
            }
        };
    }
    match builder.build() {
        Ok(u) => Some(u),
        Err(e) => {
            eprintln!("tennoworth: updater unavailable: {e}");
            None
        }
    }
}

/// One update check. Always completes with a status (stored for `update_status`
/// and returned); `available: true` also parks the `Update` handle for
/// `install_pending`. Every failure is logged and reported as no-update.
pub async fn check(app: &AppHandle) -> UpdateStatus {
    let mut status = UpdateStatus {
        checked: true,
        ..Default::default()
    };
    let state = app.state::<UpdateState>();
    let Some(updater) = build_updater(app) else {
        state.store(status.clone(), None);
        return status;
    };
    let update = match updater.check().await {
        Ok(found) => found,
        Err(e) => {
            eprintln!("tennoworth: update check failed (treating as no update): {e}");
            None
        }
    };
    if let Some(u) = &update {
        status.available = true;
        status.version = Some(u.version.clone());
        status.notes = u.body.clone();
    }
    state.store(status.clone(), update);
    status
}

/// Download + install the update the last check found. Explicit-confirmation
/// only — nothing calls this but the SPA's "Install update" button. Unlike
/// `check`, failures here ARE surfaced (the user asked for this action): a
/// download error or a signature mismatch becomes the banner text, the running
/// app is untouched, and the pending update stays retryable. On success nothing
/// restarts by itself — the new version applies when the user restarts (the
/// SPA offers `restart_app`; the Windows installer restarts as part of its
/// passive install flow).
pub async fn install_pending(app: &AppHandle) -> Result<(), String> {
    let update = app
        .state::<UpdateState>()
        .pending()
        .ok_or("No update is pending — check for updates first.")?;
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| format!("Update could not be installed: {e}"))
}
