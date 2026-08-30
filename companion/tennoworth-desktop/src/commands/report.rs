//! The "scan broke" report flow (Phase C7's second half).
//!
//! No telemetry backend, by design - nothing is collected, and nothing leaves
//! the machine unless the user clicks. The app opens a GitHub issue with the
//! boring parts already filled in (app version, OS, the error text the scan
//! actually produced), because the report that never gets filed is the one that
//! needs three facts the reporter has to go dig up.
//!
//! Opened through tauri-plugin-opener rather than an `<a target="_blank">`:
//! this app registers no window-open handler and its capability grants no
//! opener ACL by default, so such a link silently does nothing. A report button
//! that quietly no-ops is worse than no button.

use tauri_plugin_opener::OpenerExt;

const ISSUE_BASE: &str = "https://github.com/tennoworth/tennoworth/issues/new";

/// Percent-encode for a query-string VALUE.
///
/// Hand-rolled because the app has no url crate and this is the only caller.
/// Unreserved set per RFC 3986 plus the usual `-._~`; everything else, space
/// included, goes to %XX. Space must NOT become `+` here - GitHub renders the
/// body literally and a `+` would show up as a plus sign in the issue text.
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(*b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Build the prefilled issue URL.
///
/// Pure and separately tested: the encoding and the field layout are where this
/// can actually be wrong, whereas "does the browser open" is the plugin's job.
///
/// `error` is whatever the scan reported. It is truncated because a scan error
/// can carry a long tail, and browsers/servers cap URL length - a report that
/// silently fails to open because the URL was too long is the failure mode this
/// truncation exists to prevent.
pub fn issue_url(app_version: &str, os: &str, error: Option<&str>) -> String {
    const MAX_ERROR: usize = 600;
    let mut err = error.unwrap_or("(no error text captured)").trim().to_string();
    if err.chars().count() > MAX_ERROR {
        err = err.chars().take(MAX_ERROR).collect::<String>() + "… (truncated)";
    }

    let body = format!(
        "## What happened\n\n\
         The inventory scan failed.\n\n\
         ## What the app reported\n\n\
         ```\n{err}\n```\n\n\
         ## Environment\n\n\
         - App version: {app_version}\n\
         - OS: {os}\n\n\
         ## Anything else\n\n\
         <!-- Were you at the login screen? Just updated the game? Running through Proton? -->\n"
    );

    format!(
        "{ISSUE_BASE}?title={}&labels={}&body={}",
        encode(&format!("Scan broke on {os} ({app_version})")),
        encode("scan-broke"),
        encode(&body)
    )
}

/// The report URL, and whether we managed to open it.
///
/// A failed open is NOT an error: the URL is still perfectly filable by hand,
/// and returning Err would leave the SPA holding a message instead of the link.
/// Returning both lets the UI fall back to "copy this" on a box with no
/// registered browser.
#[derive(serde::Serialize)]
pub struct ScanReport {
    pub url: String,
    pub opened: bool,
}

/// Open the prefilled report in the user's browser.
#[tauri::command]
pub fn report_scan_issue(app: tauri::AppHandle, error: Option<String>) -> ScanReport {
    let url = issue_url(
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        error.as_deref(),
    );
    let opened = match app.opener().open_url(url.clone(), None::<&str>) {
        Ok(()) => true,
        Err(e) => {
            eprintln!("tennoworth: could not open the report URL: {e}");
            false
        }
    };
    ScanReport { url, opened }
}

fn allowed_external_url(raw: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(raw) else {
        return false;
    };
    if url.scheme() != "https" {
        return false;
    }
    matches!(
        url.host_str(),
        Some("warframe.market" | "github.com" | "ko-fi.com")
    )
}

/// Open links from the desktop webview in the user's real browser. Tauri does
/// not give `<a target="_blank">` a window by default, so leaving navigation
/// to the webview makes a normal item click silently do nothing.
#[tauri::command]
pub fn open_external_url(app: tauri::AppHandle, url: String) -> Result<bool, String> {
    if !allowed_external_url(&url) {
        return Err("external URL is not on TennoWorth's allowlist".into());
    }
    match app.opener().open_url(url, None::<&str>) {
        Ok(()) => Ok(true),
        Err(e) => {
            eprintln!("tennoworth: could not open external URL: {e}");
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_is_the_issue_endpoint_with_all_three_fields() {
        let u = issue_url("0.3.6", "linux", Some("No accountId/nonce pair found"));
        assert!(u.starts_with(ISSUE_BASE), "{u}");
        assert!(u.contains("?title="));
        assert!(u.contains("&labels=scan-broke"));
        assert!(u.contains("&body="));
    }

    #[test]
    fn the_error_text_survives_into_the_body() {
        let u = issue_url("0.3.6", "linux", Some("No accountId/nonce pair found"));
        assert!(u.contains(&encode("No accountId/nonce pair found")), "{u}");
        assert!(u.contains(&encode("0.3.6")));
        assert!(u.contains("linux"));
    }

    #[test]
    fn spaces_encode_as_pct20_not_plus() {
        // GitHub renders the body literally; `+` would show as a plus sign.
        let u = issue_url("0.3.6", "linux", Some("a b"));
        assert!(u.contains("a%20b"), "{u}");
        assert!(!u.contains("a+b"), "{u}");
    }

    #[test]
    fn external_links_are_https_and_host_allowlisted() {
        assert!(allowed_external_url(
            "https://warframe.market/items/kogake_prime_set"
        ));
        assert!(allowed_external_url(
            "https://github.com/tennoworth/tennoworth"
        ));
        assert!(allowed_external_url("https://ko-fi.com/prowly"));
        assert!(!allowed_external_url("http://warframe.market/items/foo"));
        assert!(!allowed_external_url(
            "https://warframe.market.evil.example/items/foo"
        ));
        assert!(!allowed_external_url("javascript:alert(1)"));
    }

    #[test]
    fn characters_that_would_break_the_query_string_are_escaped() {
        let u = issue_url("0.3.6", "windows", Some("path=C:\\x&y#z?q"));
        // The only structural separators are the ones we wrote ourselves.
        assert_eq!(u.matches('?').count(), 1, "{u}");
        assert_eq!(u.matches('#').count(), 0, "{u}");
        assert_eq!(u.matches('&').count(), 2, "{u}");
    }

    #[test]
    fn a_missing_error_still_produces_a_filable_report() {
        let u = issue_url("0.3.6", "linux", None);
        assert!(u.contains(&encode("(no error text captured)")), "{u}");
    }

    #[test]
    fn a_huge_error_is_truncated_so_the_url_stays_openable() {
        let long = "x".repeat(5000);
        let u = issue_url("0.3.6", "linux", Some(&long));
        assert!(u.contains(&encode("… (truncated)")), "truncation is visible");
        // Comfortably under the ~8k that browsers and GitHub start refusing.
        assert!(u.len() < 4000, "url was {} bytes", u.len());
    }

    #[test]
    fn multibyte_errors_truncate_without_splitting_a_character() {
        // Slicing by bytes here would panic on a char boundary.
        let long = "日".repeat(5000);
        let u = issue_url("0.3.6", "linux", Some(&long));
        assert!(u.contains(&encode("… (truncated)")));
    }
}
