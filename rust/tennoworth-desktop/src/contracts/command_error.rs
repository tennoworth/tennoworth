/// Typed command error serialized to the webview as `{ code, message }`. The SPA
/// maps `code` to its own error classes:
///   - `needs_login`   - no login on this machine → open the login modal.
///   - `needs_unlock`  - login present, session locked → open the passphrase modal.
///   - `bad_passphrase`- wrong passphrase in the unlock/login modal.
///   - `no_pending` / `busy` - pending-plan resume edge cases.
///   - `wfm` / `internal` - everything else, message shown verbatim.
///
/// Never carries the JWT, the passphrase, or the WFM password.
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
            "Enter your passphrase to unlock warframe.market listing.",
        )
    }
    pub fn bad_passphrase() -> Self {
        Self::of(
            "bad_passphrase",
            "Wrong passphrase, or the login file was modified.",
        )
    }
    pub fn wfm(e: anyhow::Error) -> Self {
        if let Some(access) = e.downcast_ref::<wfm_client::governor::AccessError>() { return Self::of(access.code(), access.to_string()); }
        Self::of("wfm", e.to_string())
    }
    pub fn internal(e: impl std::fmt::Display) -> Self {
        Self::of("internal", e.to_string())
    }
}

