//! Opening links from the desktop webview in the user's real browser.
//!
//! Opened through the native browser launcher rather than webview navigation,
//! which has no browser window to target in the desktop shell.
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
}
