//! C5 auto-update: check the GitHub-releases `latest.json`, notify, install
//! only on explicit confirmation, apply on restart. NO silent updates — the
//! check is the only thing that runs unprompted (at launch), and it downloads
//! nothing but the manifest.
//!
//! Failure posture mirrors market.rs: `check` never panics and never returns
//! Err. Offline, DNS failure, HTTP error, a malformed/truncated manifest, an
//! a bad endpoint override all degrade to "no update available" (logged to
//! stderr) — an update check must never crash the app or block launch. An
//! install that cannot self-update is reported explicitly instead of being
//! presented as current. A bad *bundle signature* surfaces later, in `install_pending`:
//! the plugin verifies the minisign signature against the pubkey in
//! tauri.conf.json after download and refuses to install on mismatch, which
//! reaches the user as a plain error banner while the running app stays intact.

use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_updater::{Update, UpdaterExt};
use wfm_core::poison::guard;

/// Emitted to the webview whenever a launch, manual, or periodic check finds an
/// update. The SPA also reads `update_status` at mount, so a listener
/// registered after the launch emit still sees the result.
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
    pub support: UpdateSupport,
    pub current_version: String,
    pub version: Option<String>,
    pub notes: Option<String>,
}

#[derive(serde::Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UpdateSupport {
    Supported,
    AppimageRequired,
    DisabledTestBuild,
}

impl Default for UpdateStatus {
    fn default() -> Self {
        Self {
            checked: false,
            available: false,
            support: update_support(),
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
        guard(&self.last).clone()
    }

    /// Clone (not take): a failed install must stay retryable.
    fn pending(&self) -> Option<Update> {
        guard(&self.pending).clone()
    }

    fn store(&self, status: UpdateStatus, update: Option<Update>) {
        *guard(&self.last) = status;
        *guard(&self.pending) = update;
    }
}

/// Build the updater, honoring a `TENNOWORTH_UPDATE_URL` endpoint override so
/// the probe can exercise the offline / malformed-manifest paths against a
/// controlled endpoint (the market.rs `TENNOWORTH_MARKET_URL` pattern). None on
/// any builder failure — including a non-https override, which the plugin
/// rejects in release builds.
/// Tauri's Linux updater only supports the AppImage bundle, and that is
/// precisely why the AppImage is now the ONLY Linux channel we ship: it is the
/// one that can replace itself. In an AppImage run (the runtime sets
/// `APPIMAGE`) self-update works and latest.json carries a linux-x86_64 entry,
/// so the check is real. Any other Linux run — a `cargo run` dev build, an
/// extracted bundle, or a leftover install from the retired deb/rpm/AUR
/// packages — cannot self-update. The explicit support state lets Settings
/// explain that instead of claiming a skipped check found the current version.
///
/// (History: the first AppImage was withdrawn for an EGL abort on rolling
/// Mesa. Root cause found 2026-08-20: the bundle carried ubuntu-22.04's
/// libwayland-*, which host Mesa's Wayland-EGL platform rejects — fixed at
/// bundle time in release-desktop.yml by stripping those libs, verified on
/// Mesa 26.2. The WebKit stack itself was never the problem.)
///
/// The `TENNOWORTH_UPDATE_URL` override still forces a real check, so probe.rs
/// can exercise the full flow on the Linux CI runner.
fn classify_update_support(
    linux: bool,
    appimage: bool,
    endpoint_override: bool,
    test_build: bool,
) -> UpdateSupport {
    if test_build {
        UpdateSupport::DisabledTestBuild
    } else if linux && !appimage && !endpoint_override {
        UpdateSupport::AppimageRequired
    } else {
        UpdateSupport::Supported
    }
}

fn update_support() -> UpdateSupport {
    classify_update_support(
        cfg!(target_os = "linux"),
        std::env::var_os("APPIMAGE").is_some(),
        std::env::var_os("TENNOWORTH_UPDATE_URL").is_some(),
        option_env!("TENNOWORTH_OCR_TEST_BUILD") == Some("1"),
    )
}

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
    if status.support != UpdateSupport::Supported {
        match status.support {
            UpdateSupport::AppimageRequired => eprintln!(
                "tennoworth: update check skipped — this Linux install is not an AppImage"
            ),
            UpdateSupport::DisabledTestBuild => {
                eprintln!("tennoworth: update check skipped — disabled in OCR test builds")
            }
            UpdateSupport::Supported => unreachable!(),
        }
        state.store(status.clone(), None);
        return status;
    }
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

pub async fn check_and_emit(app: &AppHandle) -> UpdateStatus {
    let status = check(app).await;
    if status.available {
        if let Err(e) = app.emit(EVENT_UPDATE_AVAILABLE, &status) {
            eprintln!("tennoworth: update-available emit failed: {e}");
        }
    }
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

// ---- Tauri commands -----------------------------------------------------

/// Run an update check now and notify the SPA when one is available. Never
/// rejects — offline / malformed manifest / any updater failure reads as "no
/// update available" (see `check` above). Launch, manual, and periodic checks
/// share this routine.
#[tauri::command]
pub async fn check_update(app: AppHandle) -> UpdateStatus {
    check_and_emit(&app).await
}

/// The last check's outcome, no network. The SPA reads this at mount so an
/// `update-available` event emitted before its listener registered is never
/// lost (`checked: false` means the launch check hasn't completed yet).
#[tauri::command]
pub fn update_status(state: State<'_, UpdateState>) -> UpdateStatus {
    state.last()
}

/// C5: download + install the pending update — only ever invoked from the
/// SPA's explicit "Install update" confirmation. Errors (download failure, bad
/// signature) surface verbatim in the SPA's banner; the running app is
/// untouched and the update stays retryable.
#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    install_pending(&app).await
}

/// Relaunch to apply an installed update ("apply on restart" — the SPA's
/// "Restart now" button after a successful install).
#[tauri::command]
pub fn restart_app(app: AppHandle) {
    app.restart()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_support_distinguishes_appimage_windows_and_test_builds() {
        assert_eq!(
            classify_update_support(true, false, false, false),
            UpdateSupport::AppimageRequired
        );
        assert_eq!(
            classify_update_support(true, true, false, false),
            UpdateSupport::Supported
        );
        assert_eq!(
            classify_update_support(false, false, false, false),
            UpdateSupport::Supported
        );
        assert_eq!(
            classify_update_support(true, false, true, false),
            UpdateSupport::Supported
        );
        assert_eq!(
            classify_update_support(false, false, false, true),
            UpdateSupport::DisabledTestBuild
        );

        let expected: Vec<String> =
            serde_json::from_str(include_str!("../../../tests/fixtures/update-support.json"))
                .unwrap();
        let actual = [
            UpdateSupport::Supported,
            UpdateSupport::AppimageRequired,
            UpdateSupport::DisabledTestBuild,
        ]
        .into_iter()
        .map(|support| {
            serde_json::to_value(support)
                .unwrap()
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect::<Vec<_>>();
        assert_eq!(actual, expected);
    }
}
