//! The warframe.market sign-in window.
//!
//! Since 2026-09 every warframe.market page sits behind a Cloudflare bot
//! challenge that only a real browser passes, so the scripted CSRF + password
//! POST cannot sign in any more. Instead the user signs in on WFM's own page in
//! a webview window, and the app reads the site's `JWT` cookie from it. The WFM
//! password is typed into warframe.market and never reaches this process.
//!
//! The window has no IPC capability (capabilities/default.json names `main`
//! only), and it is incognito so the WFM session does not outlive it in the
//! webview profile - the encrypted envelope stays the only copy at rest.

use std::time::{Duration, Instant};

use tauri::webview::Cookie;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use wfm_core::trading::auth::jwt_is_signed_in;

use super::wfm_session::CmdError;

const LABEL: &str = "wfm-signin";
const SIGNIN_URL: &str = "https://warframe.market/auth/signin";
// The cookie is set for `.warframe.market`; ask for it as the API host sees it.
const COOKIE_URL: &str = "https://api.warframe.market/";
const POLL: Duration = Duration::from_millis(750);
// A failed /v2/me check (network, throttling) is retried at this pace rather
// than every poll, so a flaky connection cannot spend the read budget.
const CHECK_RETRY: Duration = Duration::from_secs(5);
const GIVE_UP: Duration = Duration::from_secs(15 * 60);

/// Open the sign-in window and resolve with the JWT once the user has signed
/// in. Closing the window, `cancel`, or the time limit end it with an error.
pub async fn capture_signed_in_jwt(app: &AppHandle, platform: &str) -> Result<String, CmdError> {
    if let Some(existing) = app.get_webview_window(LABEL) {
        let _ = existing.set_focus();
        return Err(CmdError::of(
            "busy",
            "The warframe.market sign-in window is already open.",
        ));
    }
    let url = SIGNIN_URL.parse().map_err(CmdError::internal)?;
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::External(url))
        .title("Sign in to warframe.market")
        .inner_size(520.0, 760.0)
        .incognito(true)
        .build()
        .map_err(|e| CmdError::internal(format!("opening the sign-in window: {e}")))?;
    let result = wait_for_sign_in(app, platform).await;
    // Already gone when the user closed it; nothing to report then.
    let _ = window.destroy();
    result
}

/// Close the sign-in window, which ends a pending `capture_signed_in_jwt`.
pub fn cancel(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.destroy();
    }
}

async fn wait_for_sign_in(app: &AppHandle, platform: &str) -> Result<String, CmdError> {
    let cookie_url: tauri::Url = COOKIE_URL.parse().map_err(CmdError::internal)?;
    let started = Instant::now();
    // The page sets an anonymous JWT before sign-in; remember the value /v2/me
    // rejected so it is checked once, not every poll.
    let mut anonymous: Option<String> = None;
    let mut retry_at: Option<Instant> = None;
    loop {
        tokio::time::sleep(POLL).await;
        let Some(window) = app.get_webview_window(LABEL) else {
            return Err(CmdError::of(
                "cancelled",
                "The sign-in window was closed before signing in.",
            ));
        };
        if started.elapsed() > GIVE_UP {
            return Err(CmdError::of(
                "cancelled",
                "Sign-in timed out. Log in again to retry.",
            ));
        }
        let cookies = window
            .cookies_for_url(cookie_url.clone())
            .map_err(|e| CmdError::internal(format!("reading the sign-in cookies: {e}")))?;
        let Some(jwt) = jwt_cookie(&cookies) else {
            continue;
        };
        if anonymous.as_deref() == Some(jwt.as_str())
            || retry_at.is_some_and(|at| Instant::now() < at)
        {
            continue;
        }
        let (candidate, platform) = (jwt.clone(), platform.to_string());
        let checked = tauri::async_runtime::spawn_blocking(move || {
            jwt_is_signed_in(&candidate, &platform)
        })
        .await
        .map_err(|e| CmdError::internal(format!("sign-in check failed to run: {e}")))?;
        match checked {
            Ok(true) => return Ok(jwt),
            Ok(false) => {
                anonymous = Some(jwt);
                retry_at = None;
            }
            Err(e) => {
                // The error names the endpoint and status, never the cookie.
                eprintln!("tennoworth: sign-in check failed: {e:#}");
                retry_at = Some(Instant::now() + CHECK_RETRY);
            }
        }
    }
}

fn jwt_cookie(cookies: &[Cookie<'static>]) -> Option<String> {
    cookies
        .iter()
        .find(|c| c.name() == "JWT" && !c.value().is_empty())
        .map(|c| c.value().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jwt_cookie_picks_the_wfm_session_cookie_only() {
        let cookies = vec![
            Cookie::new("cf_clearance", "challenge-pass"),
            Cookie::new("JWT", "header.payload.sig"),
        ];
        assert_eq!(jwt_cookie(&cookies).as_deref(), Some("header.payload.sig"));
        assert_eq!(jwt_cookie(&[Cookie::new("JWT", "")]), None);
        assert_eq!(jwt_cookie(&[Cookie::new("cf_clearance", "x")]), None);
    }
}
