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

use std::collections::HashMap;
use std::time::{Duration, Instant};

use tauri::webview::Cookie;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use wfm_core::trading::auth::jwt_is_signed_in;

use super::wfm_session::CmdError;

const LABEL: &str = "wfm-signin";
const SIGNIN_URL: &str = "https://warframe.market/auth/signin";
// The anonymous cookie is set for `.warframe.market`; ask both hosts so a
// host-only cookie from either is seen too.
const COOKIE_URLS: [&str; 2] = ["https://warframe.market/", "https://api.warframe.market/"];
const POLL: Duration = Duration::from_millis(750);
// A failed /v2/me check (network, throttling) is retried at this pace rather
// than every poll, so a flaky connection cannot spend the read budget.
const CHECK_RETRY: Duration = Duration::from_secs(5);
const RECHECK: Duration = Duration::from_secs(20);
const COOKIE_READ_LIMIT: Duration = Duration::from_secs(10);
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
    let cookie_urls = COOKIE_URLS
        .iter()
        .map(|u| u.parse::<tauri::Url>().map_err(CmdError::internal))
        .collect::<Result<Vec<_>, _>>()?;
    let started = Instant::now();
    // The page sets an anonymous JWT before sign-in. A value /v2/me rejected is
    // not asked about again until the page moves on (signing in navigates away
    // from the form) or RECHECK passes - WFM may upgrade the same session
    // server-side rather than issue a new cookie.
    let mut anonymous: HashMap<String, Instant> = HashMap::new();
    let mut last_page: Option<String> = None;
    let mut retry_at: Option<Instant> = None;
    let mut progress = Progress::default();
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
        let page = window.url().ok().map(|u| page_label(&u));
        if page != last_page {
            progress.note(&format!("page {}", page.as_deref().unwrap_or("unknown")));
            anonymous.clear();
            last_page = page;
        }
        let cookies = read_cookies(window, cookie_urls.clone()).await?;
        let values = jwt_cookies(&cookies);
        if values.is_empty() {
            progress.note("no WFM session cookie yet");
            continue;
        }
        if retry_at.is_some_and(|at| Instant::now() < at) {
            continue;
        }
        for jwt in values {
            if anonymous.get(&jwt).is_some_and(|at| at.elapsed() < RECHECK) {
                continue;
            }
            let (candidate, platform) = (jwt.clone(), platform.to_string());
            let checked = tauri::async_runtime::spawn_blocking(move || {
                jwt_is_signed_in(&candidate, &platform)
            })
            .await
            .map_err(|e| CmdError::internal(format!("sign-in check failed to run: {e}")))?;
            match checked {
                Ok(true) => {
                    progress.note("signed in");
                    return Ok(jwt);
                }
                Ok(false) => {
                    progress.note("session cookie is not signed in yet");
                    anonymous.insert(jwt, Instant::now());
                }
                Err(e) => {
                    // The error names the endpoint and status, never the cookie.
                    eprintln!("tennoworth: sign-in check failed: {e:#}");
                    retry_at = Some(Instant::now() + CHECK_RETRY);
                    break;
                }
            }
        }
    }
}

/// Read the window's cookies off the async worker, bounded: on Linux the read
/// spins the GTK loop until WebKit answers, and a read that never answers must
/// surface as an error rather than an endless "waiting for sign-in".
async fn read_cookies(
    window: tauri::WebviewWindow,
    urls: Vec<tauri::Url>,
) -> Result<Vec<Cookie<'static>>, CmdError> {
    let read = tauri::async_runtime::spawn_blocking(move || {
        let mut all = Vec::new();
        for url in urls {
            all.extend(window.cookies_for_url(url)?);
        }
        Ok::<_, tauri::Error>(all)
    });
    match tokio::time::timeout(COOKIE_READ_LIMIT, read).await {
        Ok(Ok(Ok(cookies))) => Ok(cookies),
        Ok(Ok(Err(e))) => Err(CmdError::internal(format!("reading the sign-in cookies: {e}"))),
        Ok(Err(e)) => Err(CmdError::internal(format!("cookie read failed to run: {e}"))),
        Err(_) => Err(CmdError::internal(
            "The sign-in window did not answer when asked for its cookies.",
        )),
    }
}

/// Host and path only: a query string could carry a token.
fn page_label(url: &tauri::Url) -> String {
    format!("{}{}", url.host_str().unwrap_or(""), url.path())
}

/// Logs each change of sign-in state once, so a stall is diagnosable from the
/// terminal. Never given a cookie value.
#[derive(Default)]
struct Progress(String);

impl Progress {
    fn note(&mut self, state: &str) {
        if self.0 != state {
            eprintln!("tennoworth: sign-in window: {state}");
            state.clone_into(&mut self.0);
        }
    }
}

/// Every distinct non-empty `JWT` value: a host-only cookie and the
/// `.warframe.market` one can coexist, and either may be the signed-in one.
fn jwt_cookies(cookies: &[Cookie<'static>]) -> Vec<String> {
    let mut values: Vec<String> = Vec::new();
    for c in cookies {
        if c.name() == "JWT" && !c.value().is_empty() && !values.iter().any(|v| v == c.value()) {
            values.push(c.value().to_string());
        }
    }
    values
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jwt_cookies_keeps_every_distinct_wfm_session_value() {
        let cookies = vec![
            Cookie::new("cf_clearance", "challenge-pass"),
            Cookie::new("JWT", "anonymous"),
            Cookie::new("JWT", "signed-in"),
            // The same cookie comes back once per host queried.
            Cookie::new("JWT", "anonymous"),
            Cookie::new("JWT", ""),
        ];
        assert_eq!(jwt_cookies(&cookies), ["anonymous", "signed-in"]);
        assert!(jwt_cookies(&[Cookie::new("cf_clearance", "x")]).is_empty());
    }

    #[test]
    fn page_label_drops_the_query_string() {
        let url: tauri::Url = "https://warframe.market/auth/signin?token=secret".parse().unwrap();
        assert_eq!(page_label(&url), "warframe.market/auth/signin");
    }
}
