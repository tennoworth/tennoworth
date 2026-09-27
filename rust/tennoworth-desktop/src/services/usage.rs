//! Consent and daily tokens stay in Rust; this client never shares WFM credentials.
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
    /// Set only once the service has accepted the day's check-in.
    attempted: bool,
    /// Attempts spent on this day. Absent in state written before this field
    /// existed, hence the default.
    #[serde(default)]
    attempts: u32,
}

/// How many times one day may be offered before the app stops asking. The loop
/// polls every 60 seconds, so this bounds a day of outage to ten requests per
/// install instead of 1440, while still covering the case that loses counts today:
/// the app started before the network was up.
const MAX_ATTEMPTS_PER_DAY: u32 = 10;

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
            attempts: 0,
        };
    }
    if state.attempted {
        return Ok(None);
    }
    if state.attempts >= MAX_ATTEMPTS_PER_DAY {
        return Ok(None);
    }
    // Counting here rather than marking the day done is the whole fix: the day is
    // only finished by `confirm`, so a send that never landed leaves it retryable.
    state.attempts += 1;
    db.set_setting(DAILY, &serde_json::to_string(&state).map_err(|_| ())?)
        .map_err(|_| ())?;
    Ok(Some(state))
}

/// Record that the service accepted this day's check-in. Scoped to the token the
/// caller actually sent, so a send that lands after midnight cannot close the day
/// that has since started.
fn confirm(db: &Db, day: &str, token: &str) {
    let Ok(Some(raw)) = db.get_setting(DAILY) else {
        return;
    };
    let Ok(mut state) = serde_json::from_str::<DailyState>(&raw) else {
        return;
    };
    if state.day != day || state.token != token {
        return;
    }
    state.attempted = true;
    if let Ok(json) = serde_json::to_string(&state) {
        let _ = db.set_setting(DAILY, &json);
    }
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
                    if report_if_enabled(&client, ENDPOINT, &daily.token, &mut receiver).await {
                        confirm(&app.state::<Db>(), &daily.day, &daily.token);
                    }
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
        // A retry carries the day's token: that identity is what lets the service
        // dedupe it, so offering the day again cannot inflate the count.
        let retry = prepare(&db, "2026-09-11").unwrap().unwrap();
        assert_eq!(first.token, retry.token);
        confirm(&db, "2026-09-11", &retry.token);
        assert!(prepare(&db, "2026-09-11").unwrap().is_none());
        let tomorrow = prepare(&db, "2026-09-12").unwrap().unwrap();
        assert_ne!(first.token, tomorrow.token);
        // A clock rollback must not reopen the day that was already confirmed.
        assert!(prepare(&db, "2026-09-11").unwrap().is_none());
        assert!(prepare(&db, "2026-09-12").unwrap().is_some());
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
        // The recorded attempt survived the reopen, so the day is still open with
        // the same token - until the service confirms it.
        assert_eq!(
            prepare(&db, "2026-09-11").unwrap().unwrap().token,
            first.token
        );
        confirm(&db, "2026-09-11", &first.token);
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

    /// An outage must not become a request every minute for a whole day.
    #[test]
    fn a_days_retries_are_bounded() {
        let db = Db::open_in_memory().unwrap();
        let mut offered = 0;
        while prepare(&db, "2026-09-11").unwrap().is_some() {
            offered += 1;
        }
        assert_eq!(offered, MAX_ATTEMPTS_PER_DAY);
        // The cap is per day: tomorrow starts over.
        assert!(prepare(&db, "2026-09-12").unwrap().is_some());
    }

    /// A confirmation belongs to the token it carries. One that lands after midnight
    /// must not close the day that has since begun.
    #[test]
    fn a_late_confirmation_does_not_close_the_new_day() {
        let db = Db::open_in_memory().unwrap();
        let yesterday = prepare(&db, "2026-09-11").unwrap().unwrap();
        let today = prepare(&db, "2026-09-12").unwrap().unwrap();
        confirm(&db, "2026-09-11", &yesterday.token);
        assert_ne!(today.token, yesterday.token);
        assert!(prepare(&db, "2026-09-12").unwrap().is_some());
    }

    #[test]
    fn a_confirmation_for_another_token_is_ignored() {
        let db = Db::open_in_memory().unwrap();
        let _ = prepare(&db, "2026-09-11").unwrap().unwrap();
        confirm(&db, "2026-09-11", "2026-09-11.deadbeef");
        assert!(
            prepare(&db, "2026-09-11").unwrap().is_some(),
            "a stale confirmation must leave the day open"
        );
    }

    /// The day used to be marked attempted before the request went out, so a single
    /// transient failure - offline at the one moment the app tried - lost the day
    /// permanently. An unconfirmed day has to stay offerable.
    #[test]
    fn an_unconfirmed_check_in_stays_retryable() {
        let db = Db::open_in_memory().unwrap();
        let first = prepare(&db, "2026-09-11").unwrap().unwrap();
        let retry = prepare(&db, "2026-09-11").unwrap();
        assert!(
            retry.is_some(),
            "a day whose check-in was never confirmed must stay retryable"
        );
        assert_eq!(
            first.token,
            retry.expect("retry").token,
            "the retry must carry the day's token, which is what lets the service dedupe it"
        );
    }
}
