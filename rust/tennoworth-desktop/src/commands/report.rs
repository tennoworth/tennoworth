//! The "scan broke" report flow (Phase C7's second half).
//!
//! Scan reports have no collection backend; nothing leaves
//! the machine unless the user clicks. The app opens a GitHub issue with the
//! boring parts already filled in (app version, OS, the error text the scan
//! actually produced), because the report that never gets filed is the one that
//! needs three facts the reporter has to go dig up.
//!
//! Opened through the native browser launcher rather than webview navigation,
//! which has no browser window to target in the desktop shell.
#![allow(
    clippy::unreachable,
    reason = "tauri::command injects unreachable code into async wrappers"
)]

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

/// The endpoint a scan talks to, named from a local constant. The report says
/// which endpoint failed without ever reading it back out of the error text.
const INVENTORY_ENDPOINT: &str = "api.warframe.com/api/inventory.php";

/// Build the prefilled issue URL.
///
/// Pure and separately tested: the encoding and the field layout are where this
/// can actually be wrong, whereas "does the browser open" is the plugin's job.
///
/// The body is an **allowlist** of locally-derived fields - a category, the
/// endpoint constant, an HTTP status code when the text names one, the app
/// version and the OS. The error text is classified, never embedded.
///
/// The reason is that this is the boundary that faces the network: the URL is
/// opened in a browser the moment the user clicks, which transmits it to GitHub
/// before anything is submitted, and a submitted issue is public. Redacting
/// known credential names (see `wfm_core::acquisition::error`) closes only the
/// shapes we thought of; a name we did not anticipate - or any other sensitive
/// detail a scan error happens to carry - would ride along in free-form text.
/// Classifying removes that class entirely. The raw text stays available in the
/// app's own error banner, where the user can read it and paste it deliberately.
///
/// There is no truncation any more, because nothing input-derived is included:
/// the body length is bounded by the constants above.
pub fn issue_url(app_version: &str, os: &str, error: Option<&str>) -> String {
    let category = error_category(error);
    let status_line = match error.and_then(http_status) {
        Some(code) => format!("- HTTP status: {code}\n"),
        None => String::new(),
    };

    let body = format!(
        "## What happened\n\n\
         The inventory scan failed.\n\n\
         ## What the app reported\n\n\
         - Category: {category}\n\
         - Endpoint: {INVENTORY_ENDPOINT}\n\
         {status_line}\n\
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

/// Reduce a scan error to one fixed category.
///
/// Every input maps to one of these, so a report can never carry text. The
/// categories are chosen to separate the failures a maintainer acts on
/// differently: a blocked network path, a rotated endpoint contract, a game that
/// was not running, and a scan that simply found nothing.
pub fn error_category(error: Option<&str>) -> &'static str {
    let Some(text) = error else {
        return "no_error_text";
    };
    let t = text.to_ascii_lowercase();
    if t.contains("doesn't appear to be running") {
        return "game_not_running";
    }
    if t.contains("no accountid/nonce pair found") {
        return "credentials_not_found";
    }
    if t.contains("already running") || t.contains("already in progress") {
        return "scan_busy";
    }
    if http_status(text).is_some() {
        return "endpoint_rejected";
    }
    if t.contains("timed out") || t.contains("timeout") {
        return "timeout";
    }
    if t.contains("connect") || t.contains("network") || t.contains("offline") {
        return "connection_error";
    }
    "unclassified_error"
}

/// The HTTP status named in the error text, if it names one.
///
/// Only the numeric code is taken - never the reason phrase, which is upstream
/// input.
pub fn http_status(error: &str) -> Option<u16> {
    let at = error.find("HTTP ")?;
    let rest = error.get(at + 5..)?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

/// The report URL, and whether we managed to open it.
///
/// A failed open is NOT an error: the URL is still perfectly filable by hand,
/// and returning Err would leave the SPA holding a message instead of the link.
/// Returning both lets the UI fall back to "copy this" on a box with no
/// registered browser.
#[derive(serde::Serialize, ts_rs::TS)]
pub struct ScanReport {
    pub url: String,
    pub opened: bool,
}

/// Open the prefilled report in the user's browser.
#[tauri::command]
pub async fn report_scan_issue(app: tauri::AppHandle, error: Option<String>) -> ScanReport {
    let url = issue_url(
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        error.as_deref(),
    );
    let opened = match crate::services::browser::open(app, url.clone()).await {
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
pub async fn open_external_url(app: tauri::AppHandle, url: String) -> Result<bool, String> {
    if !allowed_external_url(&url) {
        return Err("external URL is not on TennoWorth's allowlist".into());
    }
    match crate::services::browser::open(app, url).await {
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
    fn the_report_carries_a_category_not_the_error_text() {
        let u = issue_url("0.3.6", "linux", Some("No accountId/nonce pair found"));
        assert!(u.contains(&encode("credentials_not_found")), "{u}");
        assert!(
            !u.contains(&encode("pair found")),
            "the error text must not travel: {u}"
        );
        assert!(u.contains(&encode("0.3.6")));
        assert!(u.contains("linux"));
    }

    #[test]
    fn spaces_encode_as_pct20_not_plus() {
        // GitHub renders the body literally; `+` would show as a plus sign.
        let u = issue_url("0.3.6", "linux", None);
        assert!(u.contains("Scan%20broke%20on%20linux"), "{u}");
        assert!(!u.contains('+'), "{u}");
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
        assert!(u.starts_with(ISSUE_BASE), "{u}");
        assert!(u.contains(&encode("no_error_text")), "{u}");
    }

    #[test]
    fn an_enormous_error_cannot_bloat_the_url() {
        // Nothing input-derived is embedded, so the length is bounded by the
        // constants above - no truncation step, and so no half-truncated value
        // left behind by one either.
        let long = "x".repeat(5000);
        let u = issue_url("0.3.6", "linux", Some(&long));
        assert!(u.len() < 1500, "url was {} bytes", u.len());
    }

    #[test]
    fn a_multibyte_error_is_classified_without_panicking() {
        let u = issue_url("0.3.6", "linux", Some(&"日".repeat(5000)));
        assert!(u.starts_with(ISSUE_BASE), "{u}");
        assert!(!u.contains('日'), "{u}");
    }

    #[test]
    fn an_endpoint_error_reports_the_status_code_but_not_the_reason() {
        let u = issue_url(
            "0.3.6",
            "linux",
            Some("Inventory endpoint returned HTTP 403 Forbidden (12 bytes)."),
        );
        assert!(u.contains(&encode("endpoint_rejected")), "{u}");
        assert!(u.contains(&encode("HTTP status: 403")), "{u}");
        // The reason phrase is upstream text and must not travel.
        assert!(!u.contains(&encode("Forbidden")), "{u}");
    }

    #[test]
    fn session_credentials_never_reach_the_report_url() {
        // The report is prefilled from whatever the scan reported and is opened
        // in a browser immediately, so it is a disclosure path independent of
        // the acquisition boundary: an error string arriving here unredacted
        // must still be scrubbed before it becomes a URL.
        let leaky = "inventory request failed: error sending request for url \
                     (https://api.warframe.com/api/inventory.php\
                     ?accountId=0123456789abcdef01234567&nonce=918273645)";
        let u = issue_url("0.3.6", "linux", Some(leaky));
        assert!(!u.contains("0123456789abcdef01234567"), "{u}");
        assert!(!u.contains("918273645"), "{u}");
    }

    #[test]
    fn no_free_form_error_text_reaches_the_report_url() {
        // Redacting known credential names only closes the shapes we thought of.
        // The report is the one boundary that faces the network, so it carries
        // an allowlist of locally-derived fields instead of the error text: an
        // unseen credential name then has nothing to ride along in.
        let leaky = "inventory request failed: error sending request for url \
                     (https://api.warframe.com/api/inventory.php\
                     ?accountId=0123456789abcdef01234567&nonce=918273645): \
                     client error (Connect): tcp connect error: Connection refused";
        let u = issue_url("0.3.6", "linux", Some(leaky));
        assert!(!u.contains("0123456789abcdef01234567"), "{u}");
        assert!(!u.contains("918273645"), "{u}");
        for fragment in [
            "client error",
            "tcp connect",
            "Connection refused",
            "sending request",
            "accountId",
        ] {
            assert!(
                !u.contains(&encode(fragment)),
                "free-form fragment {fragment:?} reached the report URL: {u}"
            );
        }
        // Still filable, and still says what class of failure it was.
        assert!(u.starts_with(ISSUE_BASE), "{u}");
        assert!(u.contains(&encode("connection_error")), "{u}");
    }
}
