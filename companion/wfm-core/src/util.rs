//! Small cross-cutting utilities: wall-clock stamps, random tokens, the
//! browser-UA HTTP client builders, and the companion's default config paths.

use anyhow::{Context, Result};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use rand::{rngs::OsRng, RngCore};
use reqwest::blocking::Client;
use std::path::PathBuf;
use std::time::Duration;

use crate::platform::{dirs_home, real_user_home};

pub fn chrono_now_iso() -> String {
    // We don't pull in chrono just for this; format manually from SystemTime.
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days_since_epoch = secs / 86400;
    let secs_in_day = secs % 86400;
    let h = secs_in_day / 3600;
    let m = (secs_in_day / 60) % 60;
    let s = secs_in_day % 60;
    let (y, mo, d) = civil_from_days(days_since_epoch as i64);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

// Howard Hinnant's algorithm — converts days-since-epoch to (year, month, day).
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// URL-safe, unpadded base64 of `bytes` random bytes. Used for the serve
/// session token and plan ids.
pub fn random_token(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    OsRng.fill_bytes(&mut buf);
    B64.encode(&buf)
        .replace('+', "-")
        .replace('/', "_")
        .trim_end_matches('=')
        .to_string()
}

/// A blocking reqwest client with the companion's browser UA and a caller-set
/// timeout. Network calls go through here so the BROWSER_UA + timeout policy
/// applies uniformly.
pub fn browser_client(timeout_secs: u64) -> Result<Client> {
    Client::builder()
        .user_agent(crate::BROWSER_UA)
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .context("building HTTP client")
}

/// The 30-second browser client the listing/order routes use.
pub fn wfm_client() -> Result<Client> {
    browser_client(30)
}

pub fn default_jwt_path() -> PathBuf {
    let home = real_user_home().unwrap_or_else(dirs_home);
    home.join(".config").join("wfminv").join("wfm-jwt.enc")
}

pub fn default_pending_path() -> PathBuf {
    let home = real_user_home().unwrap_or_else(dirs_home);
    home.join(".config").join("wfminv").join("pending_plan.json")
}
