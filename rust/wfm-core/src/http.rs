use anyhow::{Context, Result};
use reqwest::blocking::Client;
use std::time::Duration;

/// A blocking reqwest client with the companion's browser UA and a caller-set
/// timeout. Network calls go through here so the user_agent() + timeout policy
/// applies uniformly.
pub fn browser_client(timeout_secs: u64) -> Result<Client> {
    Client::builder()
        .retry(reqwest::retry::never())
        .redirect(wfm_client::redirect_policy())
        .user_agent(crate::user_agent())
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .context("building HTTP client")
}

/// The 30-second browser client the listing/order routes use.
pub fn wfm_client() -> Result<Client> {
    browser_client(30)
}
