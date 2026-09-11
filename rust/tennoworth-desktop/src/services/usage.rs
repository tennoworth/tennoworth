//! Consent and daily tokens stay in Rust; this client never shares WFM credentials.
#![allow(
    clippy::unreachable,
    reason = "tauri command wrappers expand to unreachable"
)]
use crate::persistence::Db;
use chrono::Utc;
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri::{Manager, State};
use tokio::sync::watch;

const CONSENT: &str = "usage.consent-v1";
const DAILY: &str = "usage.daily-v1";
const ENDPOINT: &str = "https://tennoworth.app/api/usage/check-in";

#[derive(Serialize)]
pub struct UsageStatus {
    pub enabled: bool,
    pub available: bool,
}
pub struct Usage {
    consent: watch::Sender<bool>,
    mutation: std::sync::Mutex<()>,
    available: bool,
}
#[derive(Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct DailyState {
    day: String,
    token: String,
    attempted: bool,
}

fn allowed() -> bool {
    !cfg!(any(debug_assertions, test))
        && option_env!("TENNOWORTH_OCR_TEST_BUILD") != Some("1")
        && std::env::var_os("TENNOWORTH_PROBE").is_none()
        && std::env::var_os("TENNOWORTH_PROBE_BOOT").is_none()
        && std::env::var_os("TENNOWORTH_OCR_BOOT_PROBE").is_none()
        && std::env::var("TENNOWORTH_DISABLE_USAGE").ok().as_deref() != Some("1")
}
fn opted_in(db: &Db) -> bool {
    db.get_setting(CONSENT).ok().flatten().as_deref() == Some("true")
}

#[tauri::command]
pub fn get_usage_preferences(usage: State<'_, Usage>) -> UsageStatus {
    UsageStatus {
        enabled: *usage.consent.borrow(),
        available: usage.available,
    }
}
#[tauri::command]
pub fn set_usage_preferences(
    db: State<'_, Db>,
    usage: State<'_, Usage>,
    enabled: bool,
) -> Result<UsageStatus, String> {
    update_preferences(&db, &usage, enabled)
}
fn update_preferences(db: &Db, usage: &Usage, enabled: bool) -> Result<UsageStatus, String> {
    let _mutation = usage
        .mutation
        .lock()
        .map_err(|_| "Usage preferences are unavailable.".to_string())?;
    if enabled && !usage.available {
        return Err("Usage sharing is disabled in this build or launch.".into());
    }
    // Revoke in memory even if persisting the withdrawal fails.
    if !enabled {
        usage.consent.send_replace(false);
    }
    db.set_setting(CONSENT, if enabled { "true" } else { "false" }).map_err(|_| "Could not save usage preference. Sharing is stopped for this session; retry before restarting.".to_string())?;
    usage.consent.send_replace(enabled);
    Ok(UsageStatus {
        enabled,
        available: usage.available,
    })
}

fn prepare(db: &Db, day: &str) -> Result<Option<DailyState>, ()> {
    let raw = db.get_setting(DAILY).map_err(|_| ())?;
    let mut state: DailyState = match raw {
        Some(raw) => serde_json::from_str(&raw).map_err(|_| ())?,
        None => DailyState::default(),
    };
    if state.day.as_str() > day {
        return Ok(None);
    }
    if state.day != day {
        let mut bytes = [0u8; 32];
        OsRng.try_fill_bytes(&mut bytes).map_err(|_| ())?;
        let random: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        state = DailyState {
            day: day.into(),
            token: format!("{day}.{random}"),
            attempted: false,
        };
    }
    if state.attempted {
        return Ok(None);
    }
    state.attempted = true;
    db.set_setting(DAILY, &serde_json::to_string(&state).map_err(|_| ())?)
        .map_err(|_| ())?;
    Ok(Some(state))
}
fn client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .retry(reqwest::retry::never())
        .redirect(reqwest::redirect::Policy::none())
        .cookie_store(false)
        .build()
}
async fn report(client: &reqwest::Client, endpoint: &str, token: &str) -> bool {
    client
        .post(endpoint)
        .json(&serde_json::json!({"token":token}))
        .send()
        .await
        .is_ok_and(|response| response.status() == reqwest::StatusCode::NO_CONTENT)
}
async fn report_if_enabled(
    client: &reqwest::Client,
    endpoint: &str,
    token: &str,
    consent: &mut watch::Receiver<bool>,
) -> bool {
    if !*consent.borrow_and_update() {
        return false;
    }
    tokio::select! {
        biased;
        _ = consent.changed() => false,
        sent = report(client, endpoint, token) => sent,
    }
}
pub fn start(app: tauri::AppHandle) {
    let available = allowed();
    let enabled = available && opted_in(&app.state::<Db>());
    let (consent, mut receiver) = watch::channel(enabled);
    app.manage(Usage {
        consent,
        available,
        mutation: std::sync::Mutex::new(()),
    });
    if !available {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let Ok(client) = client() else { return };
        loop {
            if *receiver.borrow_and_update() && opted_in(&app.state::<Db>()) {
                let now = Utc::now();
                let prepared = prepare(&app.state::<Db>(), &now.date_naive().to_string());
                if let Ok(Some(daily)) = prepared {
                    let _ = report_if_enabled(&client, ENDPOINT, &daily.token, &mut receiver).await;
                }
            }
            tokio::select! {
                _ = receiver.changed() => {},
                _ = tokio::time::sleep(Duration::from_secs(60)) => {},
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn no_connection_without_consent_and_minimal_payload_after_opt_in() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!(
            "http://{}/api/usage/check-in",
            listener.local_addr().unwrap()
        );
        let (sender, mut consent) = watch::channel(false);
        let http = client().unwrap();
        assert!(!report_if_enabled(&http, &endpoint, "test-token", &mut consent).await);
        listener.set_nonblocking(true).unwrap();
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        listener.set_nonblocking(false).unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut data = Vec::new();
            loop {
                let mut byte = [0];
                socket.read_exact(&mut byte).unwrap();
                data.extend(byte);
                if data.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            let headers = String::from_utf8(data).unwrap().to_lowercase();
            let length: usize = headers
                .lines()
                .find_map(|line| line.strip_prefix("content-length: "))
                .unwrap()
                .parse()
                .unwrap();
            let mut body = vec![0; length];
            socket.read_exact(&mut body).unwrap();
            socket
                .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
                .unwrap();
            (headers, body)
        });
        sender.send_replace(true);
        assert!(report_if_enabled(&http, &endpoint, "test-token", &mut consent).await);
        let (headers, body) = server.join().unwrap();
        for field in [
            "cookie:",
            "authorization:",
            "user-agent:",
            "platform:",
            "language:",
            "referer:",
        ] {
            assert!(!headers.contains(field));
        }
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
            serde_json::json!({"token":"test-token"})
        );
    }
    #[tokio::test]
    async fn withdrawal_cancels_an_inflight_request() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/", listener.local_addr().unwrap());
        let (sender, mut consent) = watch::channel(true);
        let server = tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.unwrap();
            sender.send_replace(false);
            tokio::time::sleep(Duration::from_secs(1)).await;
        });
        assert!(!tokio::time::timeout(
            Duration::from_millis(500),
            report_if_enabled(&client().unwrap(), &endpoint, "test-token", &mut consent)
        )
        .await
        .unwrap());
        server.abort();
    }
    #[test]
    fn consent_is_explicit_and_tests_are_excluded() {
        let db = Db::open_in_memory().unwrap();
        assert!(!opted_in(&db));
        assert!(!allowed());
        for value in ["false", "1", "TRUE", "null", "{broken"] {
            db.set_setting(CONSENT, value).unwrap();
            assert!(!opted_in(&db));
        }
        db.set_setting(CONSENT, "true").unwrap();
        assert!(opted_in(&db));
    }
    #[test]
    fn retries_restarts_rollover_and_clock_rollback() {
        let db = Db::open_in_memory().unwrap();
        let first = prepare(&db, "2026-09-11").unwrap().unwrap();
        assert!(prepare(&db, "2026-09-11").unwrap().is_none());
        assert!(prepare(&db, "2026-09-11").unwrap().is_none());
        assert!(prepare(&db, "2026-09-11").unwrap().is_none());
        let tomorrow = prepare(&db, "2026-09-12").unwrap().unwrap();
        assert_ne!(first.token, tomorrow.token);
        assert!(prepare(&db, "2026-09-11").unwrap().is_none());
        assert!(prepare(&db, "2026-09-12").unwrap().is_none());
    }
    #[test]
    fn persistence_failures_stop_withdrawal_and_prevent_unsaved_attempts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.db");
        let db = Db::open(&path).unwrap();
        let (consent, _) = watch::channel(false);
        let usage = Usage {
            consent,
            available: true,
            mutation: std::sync::Mutex::new(()),
        };
        update_preferences(&db, &usage, true).unwrap();
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TRIGGER fail_setting BEFORE INSERT ON setting BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
        assert!(update_preferences(&db, &usage, false).is_err());
        assert!(!*usage.consent.borrow());
        assert!(prepare(&db, "2026-09-11").is_err());
        assert!(db.get_setting(DAILY).unwrap().is_none());
        conn.execute_batch("DROP TRIGGER fail_setting;").unwrap();
        update_preferences(&db, &usage, false).unwrap();
        drop(db);
        assert!(!opted_in(&Db::open(&path).unwrap()));
    }
    #[test]
    fn daily_attempt_survives_a_real_database_reopen_and_consent_toggles() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.db");
        let db = Db::open(&path).unwrap();
        let first = prepare(&db, "2026-09-11").unwrap().unwrap();
        drop(db);
        let db = Db::open(&path).unwrap();
        db.set_setting(CONSENT, "false").unwrap();
        db.set_setting(CONSENT, "true").unwrap();
        assert!(prepare(&db, "2026-09-11").unwrap().is_none());
        assert_ne!(
            first.token,
            prepare(&db, "2026-09-12").unwrap().unwrap().token
        );
    }
    #[test]
    fn corrupt_daily_state_fails_closed() {
        let db = Db::open_in_memory().unwrap();
        db.set_setting(DAILY, "broken").unwrap();
        assert!(prepare(&db, "2026-09-11").is_err());
    }
}
