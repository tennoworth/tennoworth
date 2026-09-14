//! Error types with a deliberate public shape (the rest of the crate leans on
//! `anyhow` for its fallible functions).

/// Why a memory scan / inventory fetch couldn't produce a result.
///
/// `Busy` is the single-flight guard rejecting a second concurrent scan; it is
/// transient (retry). `Failed` wraps the underlying scan or HTTP error.
pub enum ScanError {
    Busy,
    Failed(anyhow::Error),
}

impl ScanError {
    /// Render for the browser-facing `{"error": ...}` body.
    ///
    /// Everything that reaches the webview or the tray passes through here, so
    /// this is the last point at which session credentials can be removed. See
    /// [`redact_session_creds`] for why that is necessary even though the
    /// acquisition boundary already drops the request URL.
    pub fn into_message(self) -> String {
        match self {
            ScanError::Busy => {
                "a memory scan is already in progress; retry in a moment".to_string()
            }
            ScanError::Failed(e) => redact_session_creds(&format!("{e:#}")),
        }
    }
}

/// Written in place of a session credential.
const REDACTED: &str = "<redacted>";

/// Parameter names that carry a live session credential. Both are session
/// secrets for as long as the game is running - see "The app never prints
/// secrets" in `rust/AGENTS.md`.
const SECRET_NAMES: [&str; 2] = ["accountId", "nonce"];

/// A query-string value runs until one of these. Whatever ends it is kept, so
/// surrounding punctuation survives into the diagnostic.
const VALUE_END: [char; 8] = ['&', ' ', '\t', '\n', '\r', ')', '"', '\''];

/// Replace the values of session-credential parameters in `text`.
///
/// `reqwest` embeds the full request URL in the error it returns, and the
/// inventory URL carries `accountId` and `nonce` in its query string - so an
/// ordinary connection failure is enough to put live credentials into the tray
/// log and the webview. Dropping the URL at the acquisition boundary stops the
/// common path, but an upstream response that echoes the request (or any future
/// caller that formats a raw error) reintroduces them, so the boundary that
/// actually faces the user redacts as well.
///
/// Both shapes are covered because both occur: `nonce=123456` in a query string
/// and `"nonce":"123456"` in an echoed JSON body. Values run to the next
/// separator rather than to a fixed width, so a truncated or reordered value
/// still redacts completely. A bare mention of the name with no value - ordinary
/// prose like "no nonce found" - is left alone.
pub fn redact_session_creds(text: &str) -> String {
    let mut out = text.to_string();
    for name in SECRET_NAMES {
        out = redact_after(&out, &format!("{name}="), |c| VALUE_END.contains(&c));
        out = redact_after(&out, &format!("\"{name}\":\""), |c| c == '"');
    }
    out
}

/// Replace whatever follows each occurrence of `marker`, up to the first
/// character `ends` accepts, with the placeholder. The terminating character is
/// left in place.
fn redact_after(text: &str, marker: &str, ends: impl Fn(char) -> bool) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(pos) = rest.find(marker) {
        let value_at = pos + marker.len();
        let (head, tail) = rest.split_at(value_at);
        out.push_str(head);
        out.push_str(REDACTED);
        // `value_len` always lands on a char boundary: it is either a `find`
        // result or the full length, so `split_at` cannot panic here.
        let value_len = tail.find(&ends).unwrap_or(tail.len());
        rest = tail.split_at(value_len).1;
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;

    #[test]
    fn a_credential_query_string_is_redacted() {
        let text = "error sending request for url \
                    (https://api.warframe.com/api/inventory.php?accountId=0123456789abcdef01234567&nonce=918273645)";
        let out = redact_session_creds(text);
        assert!(!out.contains("0123456789abcdef01234567"), "{out}");
        assert!(!out.contains("918273645"), "{out}");
        // The endpoint survives, so the message is still diagnostic.
        assert!(out.contains("api/inventory.php"), "{out}");
        assert!(out.contains("accountId=<redacted>"), "{out}");
    }

    #[test]
    fn an_echoed_body_is_redacted_too() {
        let out = redact_session_creds(r#"{"accountId":"0123456789abcdef01234567","nonce":"918273645"}"#);
        assert!(!out.contains("0123456789abcdef01234567"), "{out}");
        assert!(!out.contains("918273645"), "{out}");
    }

    #[test]
    fn a_credential_at_the_end_of_the_text_is_redacted() {
        let out = redact_session_creds("nonce=918273645");
        assert_eq!(out, "nonce=<redacted>");
    }

    #[test]
    fn ordinary_text_is_left_alone() {
        let text = "Warframe doesn't appear to be running.\nStart the game, log past the title screen, then retry.";
        assert_eq!(redact_session_creds(text), text);
    }

    #[test]
    fn a_failed_scan_message_carries_no_credentials() {
        let err = anyhow!("boom").context("accountId=0123456789abcdef01234567 nonce=918273645");
        let msg = ScanError::Failed(err).into_message();
        assert!(!msg.contains("0123456789abcdef01234567"), "{msg}");
        assert!(!msg.contains("918273645"), "{msg}");
    }
}
