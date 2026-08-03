//! Shared transport primitives for Warframe.market and warframestat API
//! calls. Browser UA, Cloudflare-appeasing headers, envelope unwrapping,
//! and retry backoff.
//!
//! Library crate — no binary. Shared by `wfm-core` (desktop app) and
//! `wfm-scrape` (pipeline).
//!
//! Scoping rule: share primitives only.
//! Do NOT build one abstraction covering both anonymous scraping and
//! authed order mutation; their auth/error semantics differ.

use std::time::Duration;

// The one definition. wfm-core re-exports this. Proven against WFM's live
// Cloudflare bot-protection via the production companion — a generic UA gets
// a 1015 or a JS challenge.
pub const BROWSER_UA: &str =
    "Mozilla/5.0 (X11; Linux x86_64; rv:130.0) Gecko/20100101 Firefox/130.0";

/// WFM requires these on every request — Cloudflare blocks without them.
pub const HEADER_CROSSPLAY: &str = "Crossplay";
pub const HEADER_PLATFORM: &str = "Platform";
pub const HEADER_LANGUAGE: &str = "Language";

/// Build a blocking reqwest client with the browser UA and a shared timeout.
pub fn build_client(timeout_secs: u64) -> Result<reqwest::blocking::Client, reqwest::Error> {
    reqwest::blocking::Client::builder()
        .user_agent(BROWSER_UA)
        .timeout(Duration::from_secs(timeout_secs))
        .build()
}

/// Add the three WFM-required headers to a request builder.
pub fn wfm_headers(
    builder: reqwest::blocking::RequestBuilder,
    platform: &str,
) -> reqwest::blocking::RequestBuilder {
    builder
        .header(HEADER_CROSSPLAY, "true")
        .header(HEADER_PLATFORM, platform)
        .header(HEADER_LANGUAGE, "en")
}

/// [`wfm_headers`] plus the JWT cookie + Origin/Referer an authed v2 call
/// needs. WFM's v2 endpoints rely on the cookie the website sets, not the
/// `Authorization` header — see `companion/CLAUDE.md`'s WFM API quirks.
pub fn wfm_authed_headers(
    builder: reqwest::blocking::RequestBuilder,
    platform: &str,
    jwt: &str,
) -> reqwest::blocking::RequestBuilder {
    wfm_headers(builder, platform)
        .header("Cookie", format!("JWT={jwt}"))
        .header("Origin", "https://warframe.market")
        .header("Referer", "https://warframe.market/")
}

/// WFM account platforms. `pc` covers Steam & Epic. Canonical list — wfm-core
/// re-exports this (see `wfm_core::auth::PLATFORMS`).
pub const PLATFORMS: [&str; 4] = ["pc", "ps4", "xbox", "switch"];

/// Reject a mistyped platform up front — an unknown value would otherwise be
/// baked into an encrypted JWT or a scrape run and silently target the wrong
/// (or a non-existent) WFM market.
pub fn validate_platform(platform: &str) -> Result<(), String> {
    if !PLATFORMS.contains(&platform) {
        return Err(format!(
            "Unknown platform '{}'. Use one of: {}. (pc covers Steam & Epic.)",
            platform,
            PLATFORMS.join(", ")
        ));
    }
    Ok(())
}

/// Unwrap WFM's variable envelope: `data` field, `payload` field, or bare body.
pub fn unwrap_envelope(body: &serde_json::Value) -> &serde_json::Value {
    if let Some(data) = body.get("data") {
        return data;
    }
    if let Some(payload) = body.get("payload") {
        return payload;
    }
    body
}

/// Backoff before retry attempt `attempt` (0-indexed): 2s, 4s, 6s, ...
/// Shared so every retry loop in the workspace uses the same curve instead
/// of each hand-rolling its own (wfm-core's order-mutation retries use this
/// too — see listing.rs's `send_with_retry`).
pub fn retry_backoff(attempt: u32) -> Duration {
    Duration::from_secs(2 * (attempt as u64 + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unwrap_envelope_prefers_data_over_payload() {
        let body = serde_json::json!({"data": [1, 2], "payload": [3, 4]});
        assert_eq!(unwrap_envelope(&body), &serde_json::json!([1, 2]));
    }

    #[test]
    fn unwrap_envelope_falls_back_to_payload() {
        let body = serde_json::json!({"payload": [3, 4]});
        assert_eq!(unwrap_envelope(&body), &serde_json::json!([3, 4]));
    }

    #[test]
    fn retry_backoff_increases_2s_4s_6s() {
        assert_eq!(retry_backoff(0), Duration::from_secs(2));
        assert_eq!(retry_backoff(1), Duration::from_secs(4));
        assert_eq!(retry_backoff(2), Duration::from_secs(6));
    }

    #[test]
    fn validate_platform_accepts_known_rejects_unknown() {
        assert!(validate_platform("pc").is_ok());
        assert!(validate_platform("switch").is_ok());
        assert!(validate_platform("PC").is_err());
        assert!(validate_platform("playstation").is_err());
    }

    #[test]
    fn unwrap_envelope_uses_bare_body() {
        let body = serde_json::json!([1, 2, 3]);
        assert_eq!(unwrap_envelope(&body), &serde_json::json!([1, 2, 3]));
    }
}
