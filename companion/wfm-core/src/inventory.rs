//! DE inventory fetch: memory-scan the running game for the session creds, then
//! call `inventory.php` with them.

use anyhow::{anyhow, bail, Context, Result};
use reqwest::blocking::Client;
use std::sync::Mutex;
use std::time::Duration;

use crate::error::ScanError;
use crate::scan::{find_wf_pid, scan_session, SessionInfo};

const INVENTORY_URL: &str = "https://api.warframe.com/api/inventory.php";

/// Memory-scan the running game and fetch the raw inventory.json bytes.
/// Uses ONLY the in-memory session creds (accountId + nonce) — never the
/// encrypted JWT — so the inventory path needs no `login`. Silent (no prints):
/// callers add progress output as appropriate. Shared by `fetch` (writes a
/// file) and `serve`'s GET /inventory route (returns the bytes to the browser).
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

    let mut params: Vec<(&str, &str)> =
        vec![("accountId", &info.account_id), ("nonce", &info.nonce), ("ct", &ct)];
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
        .get(INVENTORY_URL)
        .query(&params)
        .send()
        .context("inventory request failed")?;
    let status = resp.status();
    let bytes = resp.bytes().context("reading inventory response")?;
    if !status.is_success() || bytes.len() < 1024 {
        let preview = String::from_utf8_lossy(&bytes[..bytes.len().min(400)]);
        bail!(
            "Inventory endpoint returned HTTP {status} ({} bytes).\nBody:\n{preview}\n\n\
             If the response was small or 4xx, DE may have rotated something.",
            bytes.len()
        );
    }
    Ok((bytes.to_vec(), info))
}

/// Serializes memory scans so two concurrent callers never run two scans at
/// once. The server is one-thread-per-request; without this, two `/inventory`
/// requests firing together would each walk the game's whole address space.
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
