/// Typed command error serialized to the webview as `{ code, message }`. The SPA
/// maps `code` to its own error classes:
///   - `needs_login`   - no login on this machine → open the login modal.
///   - `needs_unlock`  - login present, session not open yet → call `unlock_jwt`.
///   - `keyring_unavailable` - the key store did not answer; the sign-in is kept
///     and a retry may open it.
///   - `no_pending` / `busy` - pending-plan resume edge cases.
///   - `pending_plan` - logout refused while a listing batch is still saved.
///   - `wfm` / `internal` - everything else, message shown verbatim.
///
/// Never carries the JWT, its key, or the WFM password.
#[derive(Debug, serde::Serialize, ts_rs::TS)]
pub struct CmdError {
    pub code: &'static str,
    pub message: String,
}

impl CmdError {
    pub fn of(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
    pub fn needs_login() -> Self {
        Self::of(
            "needs_login",
            "Log in to warframe.market to create or edit listings.",
        )
    }
    pub fn needs_unlock() -> Self {
        Self::of(
            "needs_unlock",
            "Opening your saved warframe.market sign-in.",
        )
    }
    /// The key store did not answer. The detail is the store's own error,
    /// which names a daemon or wallet, never the key.
    pub fn keyring_unavailable(detail: &str) -> Self {
        Self::of(
            "keyring_unavailable",
            format!("Your system keyring didn't answer ({detail})."),
        )
    }
    pub fn wfm(e: anyhow::Error) -> Self {
        if let Some(access) = e.downcast_ref::<wfm_client::governor::AccessError>() {
            return Self::of(access.code(), access.to_string());
        }
        Self::of("wfm", e.to_string())
    }
    pub fn internal(e: impl std::fmt::Display) -> Self {
        Self::of("internal", e.to_string())
    }
}
