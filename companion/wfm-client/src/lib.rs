//! Shared transport primitives for Warframe.market and warframestat API
//! calls. Browser UA, Cloudflare-appeasing headers, envelope unwrapping,
//! retry, and a 3 req/s rate limiter.
//!
//! Library crate — no binary. Shared by `wfm-scrape` (pipeline) and,
//! later, potentially by the serve subcommand.
//!
//! Scoping rule: share primitives only.
//! Do NOT build one abstraction covering both anonymous scraping and
//! authed order mutation; their auth/error semantics differ.

use std::time::{Duration, Instant};

pub const BROWSER_UA: &str =
    "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0";

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
) -> reqwest::blocking::RequestBuilder {
    builder
        .header(HEADER_CROSSPLAY, "true")
        .header(HEADER_PLATFORM, "pc")
        .header(HEADER_LANGUAGE, "en")
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

/// GET a URL with retry, returning the parsed JSON body.
///
/// Retries with increasing backoff (2s, 4s, 6s), same as the Python
/// converter's `fetch_catalog`. The rate limiter is NOT applied here —
/// callers batch their own rate windows.
pub fn get_with_retry(
    client: &reqwest::blocking::Client,
    url: &str,
    max_attempts: u32,
) -> Result<serde_json::Value, String> {
    let mut last_err = String::new();
    for attempt in 0..max_attempts {
        let result = client.get(url).send();
        match result {
            Ok(resp) => {
                let status = resp.status();
                let body = resp
                    .text()
                    .map_err(|e| format!("{url}: read body: {e}"))?;
                if !status.is_success() {
                    last_err = format!("{url}: HTTP {status}: {body}");
                    eprintln!("  warning: WFM request attempt {}/{} failed: {}", attempt + 1, max_attempts, last_err);
                    std::thread::sleep(Duration::from_secs(2 * (attempt as u64 + 1)));
                    continue;
                }
                let v: serde_json::Value = serde_json::from_str(&body)
                    .map_err(|e| format!("{url}: JSON parse: {e}"))?;
                return Ok(v);
            }
            Err(e) => {
                last_err = format!("{url}: {e}");
                eprintln!("  warning: WFM request attempt {}/{} failed: {}", attempt + 1, max_attempts, last_err);
                if attempt + 1 < max_attempts {
                    std::thread::sleep(Duration::from_secs(2 * (attempt as u64 + 1)));
                }
            }
        }
    }
    Err(last_err)
}

/// A simple blocking rate limiter targeting 3 requests per second.
///
/// Call [`RateLimiter::wait()`] before each request. Returns immediately
/// when the current window has remaining capacity; sleeps otherwise.
pub struct RateLimiter {
    max_per_second: u32,
    count: u32,
    window_start: Instant,
}

impl RateLimiter {
    pub fn new(max_per_second: u32) -> Self {
        RateLimiter {
            max_per_second,
            count: 0,
            window_start: Instant::now(),
        }
    }

    pub fn wait(&mut self) {
        let elapsed = self.window_start.elapsed();
        if elapsed >= Duration::from_secs(1) {
            self.count = 0;
            self.window_start = Instant::now();
        }
        if self.count >= self.max_per_second {
            let remaining = Duration::from_secs(1).saturating_sub(elapsed);
            if remaining > Duration::ZERO {
                std::thread::sleep(remaining);
            }
            self.count = 0;
            self.window_start = Instant::now();
        }
        self.count += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limiter_allows_burst_then_slows() {
        let mut rl = RateLimiter::new(3);
        let start = Instant::now();
        for _ in 0..6 {
            rl.wait();
        }
        // 6 requests at 3/sec: first 3 instant, next 3 after 1 second
        let elapsed = start.elapsed();
        assert!(elapsed >= Duration::from_millis(900));
        assert!(elapsed < Duration::from_millis(1500));
    }

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
    fn unwrap_envelope_uses_bare_body() {
        let body = serde_json::json!([1, 2, 3]);
        assert_eq!(unwrap_envelope(&body), &serde_json::json!([1, 2, 3]));
    }
}
