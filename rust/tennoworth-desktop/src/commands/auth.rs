//! Sign-in IPC and access-status events for the desktop webview.
#![allow(
    clippy::unreachable,
    reason = "tauri::command injects unreachable code into async wrappers"
)]

use std::sync::Arc;
use tauri::State;
use zeroize::Zeroizing;

use crate::contracts::command_error::CmdError;
use crate::services::wfm::{session::WfmSession, signin as wfm_signin};

#[derive(serde::Serialize)]
pub struct WfmAuthStatus {
    /// A sign-in file exists on disk (encrypted; says nothing about whether
    /// its saved key still opens it).
    logged_in: bool,
    /// This process holds the decrypted JWT in memory.
    unlocked: bool,
}

#[tauri::command]
pub fn wfm_auth_status(session: State<'_, Arc<WfmSession>>) -> WfmAuthStatus {
    let (logged_in, unlocked) = session.auth_status();
    WfmAuthStatus {
        logged_in,
        unlocked,
    }
}

/// Open the warframe.market sign-in window, wait for the user to sign in
/// there, then save the encrypted JWT and its key and unlock the session. The WFM password is typed into warframe.market's own page and
/// never reaches the app. Async by necessity: reading webview cookies from a
/// synchronous command deadlocks on Windows.
#[tauri::command]
pub async fn wfm_login(
    app: tauri::AppHandle,
    session: State<'_, Arc<WfmSession>>,
    platform: String,
) -> Result<(), CmdError> {
    WfmSession::validate_login(&platform)?;
    let generation = session.session_generation();
    let jwt = wfm_signin::capture_signed_in_jwt(&app, &platform).await?;
    let s = Arc::clone(&session);
    tauri::async_runtime::spawn_blocking(move || s.login(generation, jwt, &platform))
        .await
        .map_err(|e| CmdError::internal(format!("login task failed to run: {e}")))?
}

/// The fallback sign-in: a `JWT` cookie value the user copied from their own
/// browser after signing in on warframe.market. It is confirmed against
/// `/v2/me` before anything is written, so an anonymous or expired cookie is
/// refused instead of saved.
#[tauri::command]
pub async fn wfm_login_with_token(
    session: State<'_, Arc<WfmSession>>,
    token: String,
    platform: String,
) -> Result<(), CmdError> {
    let token = Zeroizing::new(token);
    WfmSession::validate_login(&platform)?;
    let jwt = wfm_signin::normalize_pasted_jwt(&token)?;
    let generation = session.session_generation();
    let s = Arc::clone(&session);
    tauri::async_runtime::spawn_blocking(move || {
        match wfm_trading::auth::jwt_is_signed_in(&jwt, &platform) {
            Ok(true) => {}
            Ok(false) => {
                return Err(CmdError::of(
                    "bad_token",
                    "warframe.market doesn't accept that token as signed in. Sign in on the site first, then copy the JWT cookie again.",
                ))
            }
            Err(e) => return Err(CmdError::wfm(e)),
        }
        s.login(generation, jwt, &platform)
    })
    .await
    .map_err(|e| CmdError::internal(format!("login task failed to run: {e}")))?
}

/// Close the sign-in window from the app's login dialog; the pending
/// `wfm_login` then fails with `cancelled`.
#[tauri::command]
pub fn wfm_login_cancel(app: tauri::AppHandle) {
    wfm_signin::cancel(&app);
}

/// Open the saved sign-in with the key this device kept and warm the WFM
/// catalog. Missing or unopenable file → `needs_login`; key store not
/// answering → `keyring_unavailable`; catalog/me failure → `wfm` (retryable).
/// Network on success - spawn_blocking.
#[tauri::command]
pub async fn unlock_jwt(session: State<'_, Arc<WfmSession>>) -> Result<(), CmdError> {
    let s = Arc::clone(&session);
    tauri::async_runtime::spawn_blocking(move || s.unlock())
        .await
        .map_err(|e| CmdError::internal(format!("unlock task failed to run: {e}")))?
}

/// Log out and remove both the live session and its encrypted on-disk login.
#[tauri::command]
pub fn wfm_logout(session: State<'_, Arc<WfmSession>>) -> Result<(), CmdError> {
    session.logout()
}

pub const WFM_ACCESS_EVENT: &str = "wfm-access-changed";
#[tauri::command]
pub fn wfm_access_status() -> wfm_client::governor::AccessStatus {
    wfm_client::governor::process().status()
}
pub fn publish_access_changes(app: tauri::AppHandle) {
    use tauri::Emitter;
    let _ = std::thread::Builder::new()
        .name("wfm-access-status".into())
        .spawn(move || {
            let governor = wfm_client::governor::process();
            let mut previous = governor.status();
            let _ = app.emit(WFM_ACCESS_EVENT, &previous);
            loop {
                governor.wait_for_status_change(&previous);
                // Coalesce counter bursts without waking at all while idle.
                std::thread::sleep(std::time::Duration::from_millis(100));
                let current = governor.status();
                if current != previous {
                    let _ = app.emit(WFM_ACCESS_EVENT, &current);
                    previous = current;
                }
            }
        });
}
