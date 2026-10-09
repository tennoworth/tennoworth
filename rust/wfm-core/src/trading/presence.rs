//! The signed-in user's own warframe.market status - Online, Online in game or
//! Invisible - over the same socket as the order stream.
//!
//! WFM offers no HTTP endpoint for it: a client signs in on the socket and sends
//! `@wfm|cmd/status/set`. The server is the source of truth and answers a
//! sign-in with the current status, so a client reads that before deciding
//! anything. Verified live on 2026-10-09:
//!
//! * the desktop's webview JWT is accepted as the sign-in `token`;
//! * closing the socket does NOT clear a status: one set with a duration stays
//!   public until that duration ends, so a client that wants to stop showing as
//!   available must say so, and must keep its durations short when it cannot be
//!   sure it will get the chance;
//! * the website offers no activity, and the public profile did not show one we
//!   set, so this module never sends one.
//!
//! Message building and parsing are pure and tested against captured frames;
//! [`PresenceSocket`] is the thin blocking shell around them.

use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tungstenite::Message;

use super::ws::{open_socket, Socket};

/// The shortest and longest duration the server accepts, in seconds.
pub const MIN_DURATION_SECS: u32 = 60;
pub const MAX_DURATION_SECS: u32 = 21_600;

/// How long to wait for the sign-in reply and the status that follows it.
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(10);

/// One of the three statuses a user can choose on warframe.market.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Presence {
    Online,
    Ingame,
    Invisible,
}

impl Presence {
    pub fn wire(self) -> &'static str {
        match self {
            Presence::Online => "online",
            Presence::Ingame => "ingame",
            Presence::Invisible => "invisible",
        }
    }

    /// Public views report `invisible` as `offline`; for the user both mean the
    /// same choice.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "online" => Some(Presence::Online),
            "ingame" => Some(Presence::Ingame),
            "invisible" | "offline" => Some(Presence::Invisible),
            _ => None,
        }
    }
}

/// The committed status the server reports. Times stay as the server's
/// RFC 3339 strings; nothing here schedules from them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RichStatus {
    pub status: Presence,
    pub set_at: Option<String>,
    /// When a timed status ends; `None` for one without a duration.
    pub until: Option<String>,
    /// The stream revision, which only grows; it tells a newer report from an
    /// older one that arrives late.
    pub revision: Option<u64>,
}

/// A parsed frame from the signed-in socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresenceEvent {
    SignedIn { id: String },
    SignInRefused { id: String, reason: String },
    /// `status/set:ok`: the server committed the command `id`.
    StatusSet { id: String, status: RichStatus },
    /// `status/set:error`: the reason is the server's code, e.g. a field error.
    StatusRefused { id: String, reason: String },
    /// `event/status/set`: the committed status, after sign-in, after any of
    /// the account's clients changes it, and when a timed status expires.
    StatusChanged(RichStatus),
    /// `@wfm|protect/error`: the account may not use the route, e.g.
    /// `app.errors.userNotVerified`.
    Protected { id: Option<String>, reason: String },
    /// Anything else, including the periodic online count. The stream must
    /// survive WFM adding routes.
    Other,
}

pub fn sign_in_message(id: &str, token: &str) -> String {
    serde_json::json!({
        "route": "@wfm|cmd/auth/signIn",
        "id": id,
        "payload": { "token": token },
    })
    .to_string()
}

/// `duration` in seconds, clamped to what the server accepts; `None` sends
/// `null`, which removes any running duration.
pub fn set_status_message(id: &str, status: Presence, duration: Option<u32>) -> String {
    let duration = duration.map(|secs| secs.clamp(MIN_DURATION_SECS, MAX_DURATION_SECS));
    serde_json::json!({
        "route": "@wfm|cmd/status/set",
        "id": id,
        "payload": { "status": status.wire(), "duration": duration },
    })
    .to_string()
}

fn rich_status(payload: Option<&serde_json::Value>, meta: Option<&serde_json::Value>) -> Option<RichStatus> {
    let payload = payload?;
    let text = |key: &str| payload.get(key).and_then(|v| v.as_str()).map(String::from);
    Some(RichStatus {
        status: Presence::parse(payload.get("status")?.as_str()?)?,
        set_at: text("statusSetAt"),
        until: text("statusUntil"),
        revision: meta.and_then(|m| m.get("revision")).and_then(|r| r.as_u64()),
    })
}

/// A refusal's payload is a code string, or a map of field codes.
fn reason(payload: Option<&serde_json::Value>) -> String {
    match payload {
        Some(serde_json::Value::String(code)) => code.clone(),
        Some(serde_json::Value::Object(fields)) => fields
            .iter()
            .map(|(field, code)| format!("{field}: {}", code.as_str().unwrap_or("invalid")))
            .collect::<Vec<_>>()
            .join(", "),
        _ => "unknown".into(),
    }
}

/// Parse one text frame. Never errors: an unknown or malformed frame is
/// [`PresenceEvent::Other`].
pub fn parse_presence_event(text: &str) -> PresenceEvent {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return PresenceEvent::Other;
    };
    let route = v.get("route").and_then(|r| r.as_str()).unwrap_or("");
    let id = v.get("id").and_then(|i| i.as_str()).map(String::from);
    let payload = v.get("payload");
    match (route, id) {
        ("@wfm|cmd/auth/signIn:ok", Some(id)) => PresenceEvent::SignedIn { id },
        ("@wfm|cmd/auth/signIn:error", Some(id)) => PresenceEvent::SignInRefused { id, reason: reason(payload) },
        ("@wfm|cmd/status/set:ok", Some(id)) => match rich_status(payload, v.get("meta")) {
            Some(status) => PresenceEvent::StatusSet { id, status },
            None => PresenceEvent::Other,
        },
        ("@wfm|cmd/status/set:error", Some(id)) => PresenceEvent::StatusRefused { id, reason: reason(payload) },
        ("@wfm|event/status/set", _) => match rich_status(payload, v.get("meta")) {
            Some(status) => PresenceEvent::StatusChanged(status),
            None => PresenceEvent::Other,
        },
        ("@wfm|protect/error", id) => PresenceEvent::Protected { id, reason: reason(payload) },
        _ => PresenceEvent::Other,
    }
}

/// Why signing in on the socket failed.
#[derive(Debug)]
pub enum SignInError {
    /// The server refused the token or the account (`reason` is its code).
    Refused(String),
    /// The governor or the network stopped it before the server answered.
    Unavailable(anyhow::Error),
}

impl std::fmt::Display for SignInError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SignInError::Refused(reason) => write!(f, "warframe.market refused the sign-in: {reason}"),
            SignInError::Unavailable(error) => write!(f, "{error:#}"),
        }
    }
}

/// A signed-in socket. Blocking; the caller owns it on one thread.
pub struct PresenceSocket {
    socket: Socket,
    next_id: u64,
}

impl PresenceSocket {
    /// Connect, sign in with `token`, and return the socket with the status the
    /// server reported after sign-in (`None` if it sent none in time).
    pub fn connect(token: &str, tick: Duration) -> Result<(Self, Option<RichStatus>), SignInError> {
        let socket = open_socket(tick).map_err(SignInError::Unavailable)?;
        let mut this = PresenceSocket { socket, next_id: 0 };
        let id = this.next_id();
        this.send_text(sign_in_message(&id, token)).map_err(SignInError::Unavailable)?;
        let deadline = std::time::Instant::now() + SIGN_IN_TIMEOUT;
        let mut signed_in = false;
        while std::time::Instant::now() < deadline {
            match this.poll().map_err(SignInError::Unavailable)? {
                Some(PresenceEvent::SignedIn { id: reply }) if reply == id => signed_in = true,
                Some(PresenceEvent::SignInRefused { id: reply, reason }) if reply == id => {
                    return Err(SignInError::Refused(reason));
                }
                Some(PresenceEvent::Protected { reason, .. }) => return Err(SignInError::Refused(reason)),
                Some(PresenceEvent::StatusChanged(status)) if signed_in => return Ok((this, Some(status))),
                _ => {}
            }
        }
        if signed_in {
            Ok((this, None))
        } else {
            Err(SignInError::Unavailable(anyhow::anyhow!("no sign-in reply within {}s", SIGN_IN_TIMEOUT.as_secs())))
        }
    }

    fn next_id(&mut self) -> String {
        self.next_id += 1;
        format!("tw-{}", self.next_id)
    }

    fn send_text(&mut self, text: String) -> Result<()> {
        self.socket.send(Message::Text(text)).context("ws send")
    }

    /// Send a status change; the outcome arrives from [`Self::poll`] as
    /// `StatusSet` or `StatusRefused` carrying the returned id.
    pub fn set_status(&mut self, status: Presence, duration: Option<u32>) -> Result<String> {
        wfm_client::governor::process().check(wfm_client::governor::Kind::WebSocket, &wfm_client::governor::context())?;
        let id = self.next_id();
        self.send_text(set_status_message(&id, status, duration))?;
        Ok(id)
    }

    /// One read. `Ok(None)` is a quiet tick or a control frame; `Err` means the
    /// socket is gone and the caller reconnects.
    pub fn poll(&mut self) -> Result<Option<PresenceEvent>> {
        match self.socket.read() {
            Ok(Message::Text(text)) => Ok(Some(parse_presence_event(&text))),
            Ok(Message::Ping(payload)) => {
                let _ = self.socket.send(Message::Pong(payload));
                Ok(None)
            }
            Ok(Message::Close(_)) => anyhow::bail!("server closed the socket"),
            Ok(_) => Ok(None),
            Err(tungstenite::Error::Io(e))
                if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                Ok(None)
            }
            Err(e) => Err(e).context("ws read"),
        }
    }

    pub fn close(mut self) {
        let _ = self.socket.close(None);
        let _ = self.socket.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured live 2026-10-09 (presence probe against the real socket; the
    // user id is replaced).
    const SIGN_IN_OK: &str = r#"{"route":"@wfm|cmd/auth/signIn:ok","id":"a1"}"#;
    const STATUS_AFTER_SIGN_IN: &str = r#"{"route":"@wfm|event/status/set","payload":{"status":"invisible","statusSetAt":"2026-10-09T10:24:52Z"},"meta":{"stream":"status:user","revision":1}}"#;
    const SET_OK: &str = r#"{"route":"@wfm|cmd/status/set:ok","payload":{"status":"ingame","statusUntil":"2026-10-09T10:29:52Z","statusSetAt":"2026-10-09T10:24:52Z","activity":{"type":"ON_MISSION","details":"Yuvan Peak (Earth)","startedAt":"2026-10-09T10:24:47Z"}},"id":"s1","meta":{"stream":"status:user","revision":2}}"#;
    const SET_INVISIBLE_OK: &str = r#"{"route":"@wfm|cmd/status/set:ok","payload":{"status":"invisible","statusSetAt":"2026-10-09T10:26:26Z"},"id":"r2","meta":{"stream":"status:user","revision":3}}"#;

    #[test]
    fn parses_the_captured_sign_in_and_status_frames() {
        assert_eq!(parse_presence_event(SIGN_IN_OK), PresenceEvent::SignedIn { id: "a1".into() });
        assert_eq!(
            parse_presence_event(STATUS_AFTER_SIGN_IN),
            PresenceEvent::StatusChanged(RichStatus {
                status: Presence::Invisible,
                set_at: Some("2026-10-09T10:24:52Z".into()),
                until: None,
                revision: Some(1),
            })
        );
        assert_eq!(
            parse_presence_event(SET_OK),
            PresenceEvent::StatusSet {
                id: "s1".into(),
                status: RichStatus {
                    status: Presence::Ingame,
                    set_at: Some("2026-10-09T10:24:52Z".into()),
                    until: Some("2026-10-09T10:29:52Z".into()),
                    revision: Some(2),
                },
            }
        );
        let PresenceEvent::StatusSet { status, .. } = parse_presence_event(SET_INVISIBLE_OK) else {
            panic!("expected StatusSet")
        };
        assert_eq!((status.status, status.until, status.revision), (Presence::Invisible, None, Some(3)));
    }

    #[test]
    fn refusals_keep_the_server_code() {
        assert_eq!(
            parse_presence_event(r#"{"route":"@wfm|cmd/auth/signIn:error","payload":"app.errors.unauthorized","id":"a1"}"#),
            PresenceEvent::SignInRefused { id: "a1".into(), reason: "app.errors.unauthorized".into() }
        );
        assert_eq!(
            parse_presence_event(r#"{"route":"@wfm|cmd/status/set:error","payload":{"duration":"app.field.tooSmall"},"id":"s2"}"#),
            PresenceEvent::StatusRefused { id: "s2".into(), reason: "duration: app.field.tooSmall".into() }
        );
        assert_eq!(
            parse_presence_event(r#"{"route":"@wfm|protect/error","payload":"app.errors.userNotVerified","id":"s3"}"#),
            PresenceEvent::Protected { id: Some("s3".into()), reason: "app.errors.userNotVerified".into() }
        );
    }

    #[test]
    fn unknown_and_malformed_frames_are_other() {
        for frame in [
            "not json",
            r#"{"route":"@wfm|event/reports/online","payload":{"connections":1,"authorizedUsers":1}}"#,
            r#"{"route":"@wfm|event/status/set","payload":{"status":"busy"}}"#,
            r#"{"route":"@wfm|event/status/set"}"#,
            r#"{"route":"@wfm|cmd/status/set:ok","payload":{"status":"ingame"}}"#,
        ] {
            assert_eq!(parse_presence_event(frame), PresenceEvent::Other, "{frame}");
        }
    }

    #[test]
    fn public_offline_reads_as_the_invisible_choice() {
        assert_eq!(Presence::parse("offline"), Some(Presence::Invisible));
        assert_eq!(Presence::parse("Online"), None);
    }

    #[test]
    fn status_messages_send_no_activity_and_clamp_the_duration() {
        let sent: serde_json::Value = serde_json::from_str(&set_status_message("s1", Presence::Ingame, Some(5))).unwrap();
        assert_eq!(sent["route"], "@wfm|cmd/status/set");
        assert_eq!(sent["payload"]["status"], "ingame");
        assert_eq!(sent["payload"]["duration"], MIN_DURATION_SECS);
        assert!(sent["payload"].get("activity").is_none());
        let long: serde_json::Value = serde_json::from_str(&set_status_message("s2", Presence::Online, Some(99_999))).unwrap();
        assert_eq!(long["payload"]["duration"], MAX_DURATION_SECS);
        let open: serde_json::Value = serde_json::from_str(&set_status_message("s3", Presence::Invisible, None)).unwrap();
        assert!(open["payload"]["duration"].is_null());
    }

    #[test]
    fn sign_in_carries_the_token_under_the_documented_route() {
        let sent: serde_json::Value = serde_json::from_str(&sign_in_message("a1", "token")).unwrap();
        assert_eq!(sent["route"], "@wfm|cmd/auth/signIn");
        assert_eq!(sent["id"], "a1");
        assert_eq!(sent["payload"]["token"], "token");
    }
}
