//! Sign-in IPC and access-status events for the desktop webview.
#![allow(
    clippy::unreachable,
    reason = "tauri::command injects unreachable code into async wrappers"
)]

use std::sync::Arc;
use tauri::State;
use zeroize::Zeroizing;

use crate::command_error::CmdError;
use crate::services::{wfm_session::WfmSession, wfm_signin};

#[derive(serde::Serialize)]
pub struct WfmAuthStatus {
    /// A login envelope exists on disk (encrypted; says nothing about the
    /// passphrase being known).
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
/// there, then persist the encrypted JWT (unchanged envelope format) and unlock
/// the session. The WFM password is typed into warframe.market's own page and
/// never reaches the app. Async by necessity: reading webview cookies from a
/// synchronous command deadlocks on Windows.
#[tauri::command]
pub async fn wfm_login(
    app: tauri::AppHandle,
    session: State<'_, Arc<WfmSession>>,
    passphrase: String,
    platform: String,
    remember: bool,
) -> Result<(), CmdError> {
    // Zeroizing scrubs OUR copy of the passphrase when this ends - best-effort
    // (the IPC deserializer made its own transient copies).
    let passphrase = Zeroizing::new(passphrase);
    WfmSession::validate_login(&passphrase, &platform)?;
    let generation = session.session_generation();
    let jwt = wfm_signin::capture_signed_in_jwt(&app, &platform).await?;
    let s = Arc::clone(&session);
    tauri::async_runtime::spawn_blocking(move || {
        s.login(generation, jwt, &passphrase, &platform, remember)
    })
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
    passphrase: String,
    platform: String,
    remember: bool,
) -> Result<(), CmdError> {
    let token = Zeroizing::new(token);
    let passphrase = Zeroizing::new(passphrase);
    WfmSession::validate_login(&passphrase, &platform)?;
    let jwt = wfm_signin::normalize_pasted_jwt(&token)?;
    let generation = session.session_generation();
    let s = Arc::clone(&session);
    tauri::async_runtime::spawn_blocking(move || {
        match wfm_core::trading::auth::jwt_is_signed_in(&jwt, &platform) {
            Ok(true) => {}
            Ok(false) => {
                return Err(CmdError::of(
                    "bad_token",
                    "warframe.market doesn't accept that token as signed in. Sign in on the site first, then copy the JWT cookie again.",
                ))
            }
            Err(e) => return Err(CmdError::wfm(e)),
        }
        s.login(generation, jwt, &passphrase, &platform, remember)
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

/// Decrypt the stored JWT with the passphrase from the SPA's unlock dialog and
/// warm the WFM catalog. Missing file → `needs_login`; wrong passphrase →
/// `bad_passphrase`; catalog/me failure → `wfm` (transient, retryable).
#[tauri::command]
pub async fn unlock_jwt(
    session: State<'_, Arc<WfmSession>>,
    passphrase: String,
    remember: bool,
) -> Result<(), CmdError> {
    let s = Arc::clone(&session);
    tauri::async_runtime::spawn_blocking(move || {
        let passphrase = Zeroizing::new(passphrase);
        s.unlock(&passphrase, remember)
    })
    .await
    .map_err(|e| CmdError::internal(format!("unlock task failed to run: {e}")))?
}

/// Try the OS-keyring "remember on this device" key before the SPA raises the
/// passphrase modal. Infallible by contract: any miss (no entry, no keyring
/// daemon, stale key, network warm failure) returns false and the modal opens
/// exactly as before. Network on success (catalog warm) - spawn_blocking.
#[tauri::command]
pub async fn try_silent_unlock(session: State<'_, Arc<WfmSession>>) -> Result<bool, CmdError> {
    let s = Arc::clone(&session);
    tauri::async_runtime::spawn_blocking(move || s.try_silent_unlock())
        .await
        .map_err(|e| CmdError::internal(format!("silent-unlock task failed to run: {e}")))
}

/// Whether "remember on this device" can work here: a keyring service has to
/// answer. Asked when a sign-in or unlock dialog opens. Blocking DBus I/O, so
/// spawn_blocking.
#[tauri::command]
pub async fn wfm_remember_available() -> bool {
    tauri::async_runtime::spawn_blocking(crate::persistence::keyring_store::available)
        .await
        .unwrap_or(false)
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
    let _ = std::thread::Builder::new().name("wfm-access-status".into()).spawn(move || {
        let mut previous = None;
        loop {
            let current = wfm_access_status();
            if previous.as_ref() != Some(&current) {
                let _ = app.emit(WFM_ACCESS_EVENT, &current);
                previous = Some(current);
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    });
}
