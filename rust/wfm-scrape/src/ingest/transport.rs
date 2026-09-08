use std::collections::HashMap;

/// Narrow GET interface so every fetch stage is testable offline.
pub trait Http {
    fn get_json(&self, url: &str) -> Result<serde_json::Value, String>;
    /// Raw-text GET for non-JSON endpoints (DE's weekly riven stats file is a
    /// JS object literal). The default impl serves a JSON *string* fixture -
    /// how tests stand in for a raw body without a second trait method.
    fn get_text(&self, url: &str) -> Result<String, String> {
        self.get_json(url).and_then(|v| {
            v.as_str()
                .map(String::from)
                .ok_or_else(|| format!("{url}: expected a string fixture"))
        })
    }
    /// Raw-bytes GET for binary endpoints - DE's export index is an LZMA-alone
    /// stream, which neither of the above can carry. The default impl reads a
    /// `"base64:..."` string fixture, so a fixture can hold a real compressed
    /// body rather than a pretend one; a plain string fixture is passed through
    /// as UTF-8 bytes for the cases where that is enough.
    fn get_bytes(&self, url: &str) -> Result<Vec<u8>, String> {
        let text = self.get_text(url)?;
        match text.strip_prefix("base64:") {
            Some(b64) => {
                use base64::Engine as _;
                base64::engine::general_purpose::STANDARD
                    .decode(b64.trim())
                    .map_err(|e| format!("{url}: fixture base64 decode: {e}"))
            }
            None => Ok(text.into_bytes()),
        }
    }
}

/// Live implementation using `wfm_client`.
pub struct LiveHttp {
    pub client: reqwest::blocking::Client,
}

impl Http for LiveHttp {
    fn get_json(&self, url: &str) -> Result<serde_json::Value, String> {
        let resp = self
            .client
            .get(url)
            .send()
            .map_err(|e| format!("{url}: {e}"))?;
        let status = resp.status();
        let body = resp.text().map_err(|e| format!("{url}: read body: {e}"))?;
        if !status.is_success() {
            return Err(format!("{url}: HTTP {status}: {body}"));
        }
        serde_json::from_str(&body).map_err(|e| format!("{url}: JSON parse: {e}"))
    }

    fn get_text(&self, url: &str) -> Result<String, String> {
        let resp = self
            .client
            .get(url)
            .send()
            .map_err(|e| format!("{url}: {e}"))?;
        let status = resp.status();
        let body = resp.text().map_err(|e| format!("{url}: read body: {e}"))?;
        if !status.is_success() {
            return Err(format!("{url}: HTTP {status}: {body}"));
        }
        Ok(body)
    }

    /// Bytes, not text. The default impl round-trips through `String`, which
    /// would corrupt an LZMA stream - so the live path must not inherit it.
    fn get_bytes(&self, url: &str) -> Result<Vec<u8>, String> {
        let resp = self
            .client
            .get(url)
            .send()
            .map_err(|e| format!("{url}: {e}"))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(format!("{url}: HTTP {status}"));
        }
        resp.bytes()
            .map(|b| b.to_vec())
            .map_err(|e| format!("{url}: read body: {e}"))
    }
}

/// Fixture implementation of [`Http`] - serves pre-recorded responses from
/// a map. Missing keys are treated as an error, so the fixture can simulate
/// per-endpoint outages.
pub struct FixtureHttp {
    pub responses: HashMap<String, serde_json::Value>,
}

impl Http for FixtureHttp {
    fn get_json(&self, url: &str) -> Result<serde_json::Value, String> {
        self.responses
            .get(url)
            .cloned()
            .ok_or_else(|| format!("{url}: not in fixture set"))
    }

    /// A raw-body fixture may be written either way: a JSON string (for
    /// genuinely non-JSON bodies like DE's weekly riven JS literal) or a plain
    /// JSON object (for DE's export manifests, which ARE JSON and would be
    /// unreadable in the fixture file if escaped into a string).
    fn get_text(&self, url: &str) -> Result<String, String> {
        let v = self.get_json(url)?;
        Ok(match v.as_str() {
            Some(s) => s.to_string(),
            None => v.to_string(),
        })
    }
}
