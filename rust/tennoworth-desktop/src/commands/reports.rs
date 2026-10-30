//! Opening links from the desktop webview in the user's real browser.
//!
//! Opened through the native browser launcher rather than webview navigation,
//! which has no browser window to target in the desktop shell.
/// The parsed URL when it may open, serialized again so the launcher receives
/// exactly what was checked rather than the webview's original spelling.
fn allowed_external_url(raw: &str) -> Option<String> {
    let url = reqwest::Url::parse(raw).ok()?;
    if url.scheme() != "https" {
        return None;
    }
    let allowed = matches!(
        url.host_str(),
        Some("warframe.market" | "github.com" | "ko-fi.com")
    ) || crate::services::announcements::source_allowed(url.as_str());
    allowed.then(|| url.into())
}

/// Open links from the desktop webview in the user's real browser. Tauri does
/// not give `<a target="_blank">` a window by default, so leaving navigation
/// to the webview makes a normal item click silently do nothing.
#[tauri::command]
pub async fn open_external_url(app: tauri::AppHandle, url: String) -> Result<bool, String> {
    let Some(url) = allowed_external_url(&url) else {
        return Err("external URL is not on TennoWorth's allowlist".into());
    };
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
        ).is_some());
        assert!(allowed_external_url(
            "https://github.com/tennoworth/tennoworth"
        ).is_some());
        assert!(allowed_external_url("https://ko-fi.com/prowly").is_some());
        assert!(allowed_external_url("http://warframe.market/items/foo").is_none());
        assert!(allowed_external_url(
            "https://warframe.market.evil.example/items/foo"
        ).is_none());
        assert!(allowed_external_url("javascript:alert(1)").is_none());
    }

    #[test]
    fn the_launcher_receives_the_checked_spelling() {
        // The parser accepts spellings a launcher may read differently; only
        // its own serialization is handed on.
        assert_eq!(
            allowed_external_url(" https://warframe.market\\items/foo bar").as_deref(),
            Some("https://warframe.market/items/foo%20bar")
        );
    }

    #[test]
    fn every_valid_announcement_source_can_open_in_the_system_browser() {
        let cases: serde_json::Value = serde_json::from_str(include_str!("../../../../tests/fixtures/announcements/cases.json")).unwrap();
        for case in cases.as_array().unwrap() {
            if let Some(feed) = crate::services::announcements::parse(&case["feed"].to_string()) {
                for message in feed.messages { assert!(allowed_external_url(&message.source_url).is_some(), "{}", message.source_url); }
            }
        }
    }
}
