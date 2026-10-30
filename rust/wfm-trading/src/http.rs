use anyhow::{Context, Result};
use reqwest::blocking::Client;

/// Keep the desktop identity while sharing WFM transport policy.
pub fn browser_client(timeout_secs: u64) -> Result<Client> {
    wfm_client::build_client_with_user_agent(timeout_secs, crate::user_agent())
        .context("building HTTP client")
}

/// The 30-second browser client the listing/order routes use.
pub fn wfm_client() -> Result<Client> {
    browser_client(30)
}
