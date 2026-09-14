//! DE inventory fetch: memory-scan the running game for the session creds, then
//! call `inventory.php` with them.

use anyhow::{anyhow, bail, Context, Result};
use reqwest::blocking::Client;
use std::sync::Mutex;
use std::time::Duration;

use crate::acquisition::error::ScanError;
use crate::acquisition::scan::{find_wf_pid, scan_session, SessionInfo};

const INVENTORY_URL: &str = "https://api.warframe.com/api/inventory.php";

/// Memory-scan the running game and fetch the raw inventory.json bytes.
/// Uses ONLY the in-memory session creds (accountId + nonce) - never the
/// encrypted JWT - so the inventory path needs no login. Silent (no prints):
/// callers add progress output as appropriate. The desktop shell is the only
/// caller (scan_inventory over IPC).
pub fn fetch_inventory_bytes(
    pid: Option<u32>,
    platform_tag: Option<String>,
) -> Result<(Vec<u8>, SessionInfo)> {
    let pid = match pid {
        Some(p) => p,
        None => find_wf_pid().ok_or_else(|| {
            anyhow!(
                "Warframe doesn't appear to be running.\n\
                 Start the game, log past the title screen, then retry."
            )
        })?,
    };
    let info = scan_session(pid).context("memory scan failed")?;
    let ct = platform_tag.unwrap_or_else(|| info.ct.clone());
    let bytes = get_inventory(INVENTORY_URL, &info, &ct)?;
    Ok((bytes, info))
}

/// One inventory GET against `url`, carrying the session credentials.
///
/// Split out from [`fetch_inventory_bytes`] so the credential boundary is
/// reachable from a test: the public entry point cannot be exercised without a
/// running game, and this is the part that must never put the query string - and
/// with it `accountId` and `nonce` - into an error.
fn get_inventory(url: &str, info: &SessionInfo, ct: &str) -> Result<Vec<u8>> {
    let mut params: Vec<(&str, &str)> = vec![
        ("accountId", &info.account_id),
        ("nonce", &info.nonce),
        ("ct", ct),
    ];
    if let Some(b) = &info.build {
        params.push(("appVersion", b.as_str()));
    }
    let client = Client::builder()
        .user_agent(format!(
            "Warframe/{}",
            info.build.as_deref().unwrap_or("unknown")
        ))
        .timeout(Duration::from_secs(60))
        .build()
        .context("building HTTP client")?;
    let resp = client
        .get(url)
        .query(&params)
        .send()
        .map_err(without_url)
        .context("inventory request failed")?;
    let status = resp.status();
    let bytes = resp
        .bytes()
        .map_err(without_url)
        .context("reading inventory response")?;
    if !status.is_success() || bytes.len() < 1024 {
        bail!("{}", short_response_message(status, bytes.len()));
    }
    Ok(bytes.to_vec())
}

/// Drop the request URL from a `reqwest` error.
///
/// `reqwest` embeds the full URL in the error it returns, and this endpoint
/// carries the live `accountId` and `nonce` in its query string - so an
/// ordinary connection failure would otherwise put session credentials into the
/// tray log and the webview. Removing it here keeps them out of the error chain
/// entirely; [`crate::acquisition::error::redact_session_creds`] is the second
/// boundary for text that arrives some other way.
fn without_url(e: reqwest::Error) -> anyhow::Error {
    anyhow::Error::from(e.without_url())
}

/// Message for a response that is too small or not a success.
///
/// It carries no excerpt of the body on purpose: an upstream error page can
/// echo the request, credentials included.
fn short_response_message(status: reqwest::StatusCode, len: usize) -> String {
    format!(
        "Inventory endpoint returned HTTP {status} ({len} bytes).\n\n\
         If the response was small or 4xx, DE may have rotated something."
    )
}

/// Serializes memory scans so two concurrent callers never run two scans at
/// once. Without this, two concurrent `scan_inventory` invokes firing together
/// would each walk the game's whole address space.
/// The second caller gets `ScanError::Busy` (a transient, retryable state)
/// rather than a redundant parallel scan.
#[derive(Default)]
pub struct InventoryScanner {
    scan_lock: Mutex<()>,
}

impl InventoryScanner {
    pub fn new() -> Self {
        InventoryScanner {
            scan_lock: Mutex::new(()),
        }
    }

    /// Single-flight `fetch_inventory_bytes`. Holds the scan lock across the
    /// whole scan + HTTP fetch; a concurrent call returns `ScanError::Busy`.
    pub fn scan(
        &self,
        pid: Option<u32>,
        platform_tag: Option<String>,
    ) -> std::result::Result<(Vec<u8>, SessionInfo), ScanError> {
        // A scan thread that panicked would poison the lock; recover the guard
        // rather than wedge the route into permanent "busy".
        let _guard = match self.scan_lock.try_lock() {
            Ok(g) => g,
            Err(std::sync::TryLockError::WouldBlock) => return Err(ScanError::Busy),
            Err(std::sync::TryLockError::Poisoned(p)) => p.into_inner(),
        };
        fetch_inventory_bytes(pid, platform_tag).map_err(ScanError::Failed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SENTINEL_AID: &str = "0123456789abcdef01234567";
    const SENTINEL_NONCE: &str = "918273645";

    /// A real send failure carrying the same query parameters the inventory
    /// request puts on the wire. Port 1 on loopback refuses immediately, so
    /// this is deterministic and never reaches the network.
    fn send_failure_with_session_creds() -> reqwest::Error {
        let client = Client::builder().build().expect("build client");
        client
            .get("http://127.0.0.1:1/api/inventory.php")
            .query(&[("accountId", SENTINEL_AID), ("nonce", SENTINEL_NONCE)])
            .send()
            .expect_err("port 1 refuses the connection")
    }

    #[test]
    fn a_send_failure_never_exposes_the_session_credentials() {
        let err: anyhow::Error = send_failure_with_session_creds().into();
        let msg = ScanError::Failed(err.context("inventory request failed")).into_message();
        assert!(
            !msg.contains(SENTINEL_AID),
            "accountId leaked into the scan error: {msg}"
        );
        assert!(
            !msg.contains(SENTINEL_NONCE),
            "nonce leaked into the scan error: {msg}"
        );
    }

    fn session() -> SessionInfo {
        SessionInfo {
            account_id: SENTINEL_AID.to_string(),
            nonce: SENTINEL_NONCE.to_string(),
            build: Some("38.1.2".to_string()),
            ct: "STM".to_string(),
            cred_hits: 1,
            distinct_creds: 1,
        }
    }

    /// The funnel test above builds its own `reqwest` error, so it stays green
    /// even if the acquisition boundary stops dropping the URL. This one drives
    /// the boundary itself: port 1 refuses, and the credentials must not survive
    /// into the error the boundary returns.
    #[test]
    fn the_acquisition_boundary_drops_the_credential_query_string() {
        let err = get_inventory("http://127.0.0.1:1/api/inventory.php", &session(), "STM")
            .expect_err("port 1 refuses the connection");
        let msg = format!("{err:#}");
        assert!(
            !msg.contains(SENTINEL_AID),
            "accountId survived the acquisition boundary: {msg}"
        );
        assert!(
            !msg.contains(SENTINEL_NONCE),
            "nonce survived the acquisition boundary: {msg}"
        );
    }
}
