//! warframe.market's live order stream — `wss://ws.warframe.market/socket`.
//!
//! WFM's API rules explicitly prefer the WebSocket to tight polling. The
//! public `newOrders` stream pushes every order anyone posts (~a few per
//! second); the price-watch checker uses it as the INSTANT path beside its
//! polite 10-minute poll: a new buy order over a watch's floor notifies in
//! seconds instead of minutes, at zero request cost.
//!
//! Parsing and matching are pure and tested against captured live frames;
//! [`run_new_orders_stream`] is the thin blocking shell (one connection
//! attempt per call — the caller owns reconnect policy).

use std::net::TcpStream;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use tungstenite::client::IntoClientRequest;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::Message;

pub const WS_URL: &str = "wss://ws.warframe.market/socket";

/// One order from the `newOrder` stream, reduced to what matching needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewOrder {
    pub id: String,
    /// "sell" | "buy".
    pub side: String,
    pub platinum: u32,
    pub quantity: u32,
    /// Absent on rankless items.
    pub rank: Option<u32>,
    pub subtype: Option<String>,
    /// WFM's 24-hex item id — the catalog maps it to a slug.
    pub item_id: String,
    pub user_name: Option<String>,
    pub user_status: Option<String>,
    pub visible: bool,
}

/// A parsed frame from the socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WsEvent {
    NewOrder(NewOrder),
    /// `cmd/subscribe/newOrders:ok` — the subscription is live.
    SubscribeOk,
    /// Periodic `reports/online` heartbeat; useful as a liveness signal.
    Online { connections: u64 },
    /// Anything else (unknown routes are data, not errors).
    Other,
}

/// Parse one text frame. Never errors: an unknown or malformed frame is
/// [`WsEvent::Other`] — the stream must survive WFM adding routes.
pub fn parse_ws_event(text: &str) -> WsEvent {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return WsEvent::Other;
    };
    let route = v.get("route").and_then(|r| r.as_str()).unwrap_or("");
    match route {
        "@wfm|event/subscriptions/newOrder" => {
            let Some(p) = v.get("payload") else { return WsEvent::Other };
            let s = |k: &str| p.get(k).and_then(|x| x.as_str()).map(String::from);
            let (Some(id), Some(side), Some(item_id)) = (s("id"), s("type"), s("itemId")) else {
                return WsEvent::Other;
            };
            let Some(platinum) = p.get("platinum").and_then(|x| x.as_u64()) else {
                return WsEvent::Other;
            };
            WsEvent::NewOrder(NewOrder {
                id,
                side,
                platinum: platinum.min(u32::MAX as u64) as u32,
                quantity: p.get("quantity").and_then(|x| x.as_u64()).unwrap_or(1) as u32,
                rank: p.get("rank").and_then(|x| x.as_u64()).map(|r| r as u32),
                subtype: s("subtype"),
                item_id,
                user_name: p.get("user").and_then(|u| u.get("ingameName")).and_then(|x| x.as_str()).map(String::from),
                user_status: p.get("user").and_then(|u| u.get("status")).and_then(|x| x.as_str()).map(String::from),
                visible: p.get("visible").and_then(|x| x.as_bool()).unwrap_or(true),
            })
        }
        "@wfm|cmd/subscribe/newOrders:ok" => WsEvent::SubscribeOk,
        "@wfm|event/reports/online" => WsEvent::Online {
            connections: v
                .get("payload")
                .and_then(|p| p.get("connections"))
                .and_then(|c| c.as_u64())
                .unwrap_or(0),
        },
        _ => WsEvent::Other,
    }
}

/// Does this streamed order satisfy a watch on `(rank, subtype)` at
/// `threshold`? Mirrors the poll path's semantics exactly:
///   - side "buy": someone BIDS at or above the threshold,
///   - side "sell": someone ASKS at or below the threshold,
///   - tier equality with the same defaults the poll uses (absent rank = 0,
///     absent subtype = "").
///
/// Hidden orders never match — no buyer can act on them either.
pub fn order_matches_watch(
    o: &NewOrder,
    watch_side: &str,
    threshold: i64,
    watch_rank: Option<u32>,
    watch_subtype: Option<&str>,
) -> bool {
    if !o.visible || o.side != watch_side {
        return false;
    }
    if o.rank.unwrap_or(0) != watch_rank.unwrap_or(0) {
        return false;
    }
    if o.subtype.as_deref().unwrap_or("") != watch_subtype.unwrap_or("") {
        return false;
    }
    match watch_side {
        "buy" => o.platinum as i64 >= threshold,
        "sell" => o.platinum as i64 <= threshold,
        _ => false,
    }
}

/// How long a silent socket is tolerated before the connection is declared
/// dead. WFM's `reports/online` heartbeat arrives about every 30 s, so two
/// missed heartbeats plus slack means real trouble, not a quiet minute.
pub const SILENCE_TIMEOUT: Duration = Duration::from_secs(180);

/// Read-poll granularity — how often the stop check runs on a quiet socket.
const READ_TICK: Duration = Duration::from_secs(5);

/// Connect, subscribe to `newOrders` for `platform` (crossplay on — matching
/// filters afterwards), and deliver every parsed [`NewOrder`] to `on_order`
/// until the socket dies or `stop()` returns true. Returns Ok(()) on a
/// requested stop, Err on any connection failure — the caller decides
/// backoff and retry. Blocking; run it on a dedicated thread.
pub fn run_new_orders_stream(
    platform: &str,
    on_order: &mut dyn FnMut(NewOrder),
    stop: &dyn Fn() -> bool,
) -> Result<()> {
    let mut req = WS_URL.into_client_request().context("ws request")?;
    req.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        "wfm".parse().expect("static header value"),
    );
    req.headers_mut().insert(
        "User-Agent",
        wfm_client::user_agent("wfm-core-ws", env!("CARGO_PKG_VERSION"))
            .parse()
            .context("ua header")?,
    );
    let (mut socket, _resp) = tungstenite::connect(req).context("ws connect")?;

    // Read with a short timeout so the stop flag is honoured on a quiet
    // socket; the TLS wrapper exposes the raw TcpStream for that.
    match socket.get_ref() {
        MaybeTlsStream::Plain(s) => set_read_timeout(s)?,
        MaybeTlsStream::Rustls(s) => set_read_timeout(s.get_ref())?,
        _ => {}
    }

    let sub = serde_json::json!({
        "route": "@wfm|cmd/subscribe/newOrders",
        "payload": { "platform": platform, "crossplay": true }
    });
    socket
        .send(Message::Text(sub.to_string()))
        .context("ws subscribe")?;

    let mut last_frame = std::time::Instant::now();
    loop {
        if stop() {
            let _ = socket.close(None);
            return Ok(());
        }
        if last_frame.elapsed() > SILENCE_TIMEOUT {
            bail!("stream silent for {}s", SILENCE_TIMEOUT.as_secs());
        }
        let msg = match socket.read() {
            Ok(m) => m,
            Err(tungstenite::Error::Io(e))
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                continue; // quiet tick — loop to re-check stop/silence
            }
            Err(e) => return Err(e).context("ws read"),
        };
        last_frame = std::time::Instant::now();
        match msg {
            Message::Text(t) => {
                if let WsEvent::NewOrder(o) = parse_ws_event(&t) {
                    on_order(o);
                }
            }
            Message::Ping(p) => {
                let _ = socket.send(Message::Pong(p));
            }
            Message::Close(_) => bail!("server closed the stream"),
            _ => {}
        }
    }
}

fn set_read_timeout(s: &TcpStream) -> Result<()> {
    s.set_read_timeout(Some(READ_TICK)).context("read timeout")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured live 2026-08-20 (ws-probe against the real socket).
    const LIVE_ORDER: &str = r#"{"route":"@wfm|event/subscriptions/newOrder","payload":{"id":"6a865c6690780835bc5875e2","type":"buy","platinum":4,"quantity":1,"perTrade":1,"rank":0,"visible":true,"createdAt":"2026-08-20T01:46:14Z","updatedAt":"2026-08-20T01:46:14Z","itemId":"64c2ab1c66456704fef15835","user":{"id":"5d682f31a7a621026e8bd272","ingameName":"TechnoRaptor","slug":"technoraptor","reputation":79,"platform":"pc","crossplay":true,"locale":"en","status":"offline","activity":null,"lastSeen":"2026-08-20T01:31:56Z"}}}"#;

    #[test]
    fn parses_a_live_new_order_frame() {
        let WsEvent::NewOrder(o) = parse_ws_event(LIVE_ORDER) else {
            panic!("expected NewOrder");
        };
        assert_eq!(o.side, "buy");
        assert_eq!(o.platinum, 4);
        assert_eq!(o.rank, Some(0));
        assert_eq!(o.item_id, "64c2ab1c66456704fef15835");
        assert_eq!(o.user_name.as_deref(), Some("TechnoRaptor"));
        assert_eq!(o.user_status.as_deref(), Some("offline"));
        assert!(o.visible);
    }

    #[test]
    fn parses_control_frames_and_shrugs_at_the_unknown() {
        assert_eq!(parse_ws_event(r#"{"route":"@wfm|cmd/subscribe/newOrders:ok"}"#), WsEvent::SubscribeOk);
        assert_eq!(
            parse_ws_event(r#"{"route":"@wfm|event/reports/online","payload":{"connections":36972,"authorizedUsers":20628}}"#),
            WsEvent::Online { connections: 36972 }
        );
        assert_eq!(parse_ws_event(r#"{"route":"@wfm|event/reports/newRoute","payload":{}}"#), WsEvent::Other);
        assert_eq!(parse_ws_event("not json"), WsEvent::Other);
        // A newOrder frame missing required fields is Other, not a panic.
        assert_eq!(parse_ws_event(r#"{"route":"@wfm|event/subscriptions/newOrder","payload":{"type":"buy"}}"#), WsEvent::Other);
    }

    fn order(side: &str, plat: u32, rank: Option<u32>, subtype: Option<&str>) -> NewOrder {
        NewOrder {
            id: "x".into(), side: side.into(), platinum: plat, quantity: 1,
            rank, subtype: subtype.map(String::from), item_id: "i".into(),
            user_name: None, user_status: None, visible: true,
        }
    }

    #[test]
    fn buy_watch_fires_at_or_above_threshold_sell_at_or_below() {
        assert!(order_matches_watch(&order("buy", 50, Some(0), None), "buy", 50, None, None));
        assert!(order_matches_watch(&order("buy", 51, Some(0), None), "buy", 50, None, None));
        assert!(!order_matches_watch(&order("buy", 49, Some(0), None), "buy", 50, None, None));
        assert!(order_matches_watch(&order("sell", 30, None, None), "sell", 30, Some(0), None));
        assert!(!order_matches_watch(&order("sell", 31, None, None), "sell", 30, None, None));
        // wrong side never matches
        assert!(!order_matches_watch(&order("sell", 1, None, None), "buy", 1, None, None));
    }

    #[test]
    fn tier_must_match_with_poll_path_defaults() {
        // absent rank on both sides = rank 0 = equal
        assert!(order_matches_watch(&order("buy", 99, None, None), "buy", 1, Some(0), None));
        assert!(!order_matches_watch(&order("buy", 99, Some(8), None), "buy", 1, Some(0), None));
        assert!(order_matches_watch(&order("buy", 99, None, Some("intact")), "buy", 1, None, Some("intact")));
        assert!(!order_matches_watch(&order("buy", 99, None, Some("radiant")), "buy", 1, None, Some("intact")));
    }

    #[test]
    fn hidden_orders_never_match() {
        let mut o = order("buy", 99, None, None);
        o.visible = false;
        assert!(!order_matches_watch(&o, "buy", 1, None, None));
    }
}
