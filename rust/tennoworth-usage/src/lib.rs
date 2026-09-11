use axum::{
    extract::{DefaultBodyLimit, State},
    http::{header, StatusCode},
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Days, Utc};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckIn {
    pub token: String,
}
#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Day {
    pub date: String,
    pub count: u64,
    pub complete: bool,
}
#[derive(Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Daily {
    pub updated_at: String,
    pub days: Vec<Day>,
}

pub struct Store {
    conn: Connection,
}
impl Store {
    pub fn existing(path: &Path) -> anyhow::Result<Self> {
        Self::new(Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE,
        )?)
    }
    pub fn open(path: &Path, now: DateTime<Utc>) -> anyhow::Result<Self> {
        let mut store = Self::new(Connection::open(path)?)?;
        store.tick(now, true)?;
        Ok(store)
    }
    fn new(conn: Connection) -> anyhow::Result<Self> {
        conn.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA secure_delete=ON;
            PRAGMA max_page_count=32768;
            CREATE TABLE IF NOT EXISTS seen (day TEXT NOT NULL, hash BLOB PRIMARY KEY);
            CREATE TABLE IF NOT EXISTS daily (day TEXT PRIMARY KEY, count INTEGER NOT NULL, complete INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS health (id INTEGER PRIMARY KEY CHECK(id=1), last_tick TEXT NOT NULL);")?;
        Ok(Self { conn })
    }
    pub fn tick(&mut self, now: DateTime<Utc>, restarted: bool) -> anyhow::Result<()> {
        let day = now.date_naive().to_string();
        let tx = self.conn.transaction()?;
        let last: Option<String> = tx
            .query_row("SELECT last_tick FROM health WHERE id=1", [], |r| r.get(0))
            .optional()?;
        let last = last
            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
            .map(|d| d.with_timezone(&Utc));
        // Do not erase newer deduplication state if the server clock moves back.
        if last.is_some_and(|last| now < last) {
            anyhow::bail!("clock moved backwards");
        }
        let gap = restarted
            || last.is_none_or(|last| now.signed_duration_since(last).num_seconds() > 120);
        let mut date = last.map_or(now.date_naive(), |d| d.date_naive());
        while date <= now.date_naive() {
            let key = date.to_string();
            tx.execute("INSERT OR IGNORE INTO daily VALUES (?1,0,?2)", (&key, !gap))?;
            if gap {
                tx.execute("UPDATE daily SET complete=0 WHERE day=?1", [&key])?;
            }
            let Some(next) = date.checked_add_days(Days::new(1)) else {
                break;
            };
            date = next;
        }
        tx.execute("DELETE FROM seen WHERE day <> ?1", [&day])?;
        tx.execute("INSERT INTO health VALUES (1,?1) ON CONFLICT(id) DO UPDATE SET last_tick=excluded.last_tick", [now.to_rfc3339()])?;
        tx.commit()?;
        Ok(())
    }
    pub fn accept(&mut self, token: &str, now: DateTime<Utc>) -> Result<(), StatusCode> {
        let Some((day, random)) = token.split_once('.') else {
            return Err(StatusCode::BAD_REQUEST);
        };
        if day != now.date_naive().to_string()
            || random.len() != 64
            || !random
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return Err(StatusCode::BAD_REQUEST);
        }
        self.tick(now, false)
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        let tx = self
            .conn
            .transaction()
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        let hash = Sha256::digest(token.as_bytes()).to_vec();
        let inserted = tx
            .execute("INSERT OR IGNORE INTO seen VALUES (?1,?2)", (day, hash))
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        if inserted != 0 {
            tx.execute("UPDATE daily SET count=count+1 WHERE day=?1", [day])
                .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        }
        tx.commit().map_err(|_| StatusCode::SERVICE_UNAVAILABLE)
    }
    pub fn daily(&self, now: DateTime<Utc>, all: bool) -> anyhow::Result<Daily> {
        let updated_at: String =
            self.conn
                .query_row("SELECT last_tick FROM health WHERE id=1", [], |r| r.get(0))?;
        let sql = if all {
            "SELECT day,count,complete FROM daily ORDER BY day"
        } else {
            "SELECT day,count,complete FROM (SELECT day,count,complete FROM daily WHERE day < ?1 ORDER BY day DESC LIMIT 90) ORDER BY day"
        };
        let mut stmt = self.conn.prepare(sql)?;
        let read = |r: &rusqlite::Row<'_>| {
            Ok(Day {
                date: r.get(0)?,
                count: r.get(1)?,
                complete: r.get(2)?,
            })
        };
        let days = if all {
            stmt.query_map([], read)?.collect::<Result<Vec<_>, _>>()?
        } else {
            stmt.query_map([now.date_naive().to_string()], read)?
                .collect::<Result<Vec<_>, _>>()?
        };
        Ok(Daily { updated_at, days })
    }
    pub fn restore(&mut self, data: Daily, now: DateTime<Utc>) -> anyhow::Result<()> {
        let exported_at = DateTime::parse_from_rfc3339(&data.updated_at)?.with_timezone(&Utc);
        anyhow::ensure!(exported_at <= now, "invalid export timestamp");
        let tx = self.conn.transaction()?;
        for day in data.days {
            let date = chrono::NaiveDate::parse_from_str(&day.date, "%Y-%m-%d")?;
            anyhow::ensure!(
                date <= now.date_naive() && day.count <= i64::MAX as u64,
                "invalid aggregate"
            );
            tx.execute("INSERT INTO daily VALUES (?1,?2,?3) ON CONFLICT(day) DO UPDATE SET count=MAX(count,excluded.count),complete=MIN(complete,excluded.complete)",
                (&day.date, day.count, day.complete && date < now.date_naive()))?;
        }
        // The export cannot certify coverage after it was captured, even on older days.
        let mut date = exported_at.date_naive();
        while date <= now.date_naive() {
            tx.execute(
                "INSERT INTO daily VALUES (?1,0,0) ON CONFLICT(day) DO UPDATE SET complete=0",
                [date.to_string()],
            )?;
            let Some(next) = date.checked_add_days(Days::new(1)) else {
                break;
            };
            date = next;
        }
        tx.commit()?;
        Ok(())
    }
}

struct Limit {
    started: Instant,
    requests: u32,
}
pub struct Collector {
    store: Mutex<Store>,
    limit: Mutex<Limit>,
    slots: tokio::sync::Semaphore,
}
impl Collector {
    pub fn new(store: Store) -> Arc<Self> {
        Arc::new(Self {
            store: Mutex::new(store),
            slots: tokio::sync::Semaphore::new(32),
            limit: Mutex::new(Limit {
                started: Instant::now(),
                requests: 0,
            }),
        })
    }
    pub fn tick(&self) -> anyhow::Result<()> {
        self.store
            .lock()
            .map_err(|_| anyhow::anyhow!("store unavailable"))?
            .tick(Utc::now(), false)
    }
}
pub fn router(state: Arc<Collector>) -> Router {
    Router::new()
        .route("/api/usage/check-in", post(check_in))
        .route("/api/usage/daily", get(daily))
        .route("/health", get(health))
        .layer(DefaultBodyLimit::max(256))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            bound_request,
        ))
        .with_state(state)
}
async fn check_in(
    State(state): State<Arc<Collector>>,
    payload: Result<Json<CheckIn>, axum::extract::rejection::JsonRejection>,
) -> StatusCode {
    let Ok(Json(payload)) = payload else {
        return StatusCode::BAD_REQUEST;
    };
    let Ok(mut store) = state.store.try_lock() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    store
        .accept(&payload.token, Utc::now())
        .map_or_else(|s| s, |_| StatusCode::NO_CONTENT)
}
async fn daily(
    State(state): State<Arc<Collector>>,
) -> Result<([(header::HeaderName, &'static str); 1], Json<Daily>), StatusCode> {
    let store = state
        .store
        .try_lock()
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let data = store
        .daily(Utc::now(), false)
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    Ok((
        [(header::CACHE_CONTROL, "public, max-age=3600")],
        Json(data),
    ))
}
async fn bound_request(
    State(state): State<Arc<Collector>>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    {
        let Ok(mut limit) = state.limit.try_lock() else {
            return StatusCode::TOO_MANY_REQUESTS.into_response();
        };
        if limit.started.elapsed() >= Duration::from_secs(1) {
            limit.started = Instant::now();
            limit.requests = 0;
        }
        if limit.requests >= 20 {
            return StatusCode::TOO_MANY_REQUESTS.into_response();
        }
        limit.requests += 1;
    }
    let Ok(_slot) = state.slots.try_acquire() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    tokio::time::timeout(Duration::from_secs(5), next.run(request))
        .await
        .unwrap_or_else(|_| StatusCode::REQUEST_TIMEOUT.into_response())
}
async fn health(State(state): State<Arc<Collector>>) -> StatusCode {
    let Ok(store) = state.store.try_lock() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    let Ok(data) = store.daily(Utc::now(), false) else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    let Ok(time) = DateTime::parse_from_rfc3339(&data.updated_at) else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    let age = Utc::now().signed_duration_since(time).num_seconds();
    if (0..=120).contains(&age) {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn now() -> DateTime<Utc> {
        "2026-09-11T12:00:00Z".parse().unwrap()
    }
    fn store() -> Store {
        let mut s = Store::new(Connection::open_in_memory().unwrap()).unwrap();
        s.tick(now(), true).unwrap();
        s
    }
    fn token() -> String {
        format!("2026-09-11.{}", "a".repeat(64))
    }
    #[test]
    fn duplicates_concurrency_and_transaction_rollback() {
        let store = Arc::new(Mutex::new(store()));
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let store = store.clone();
                std::thread::spawn(move || store.lock().unwrap().accept(&token(), now()).unwrap())
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        let mut s = store.lock().unwrap();
        assert_eq!(s.daily(now(), true).unwrap().days[0].count, 1);
        s.conn.execute_batch("CREATE TRIGGER fail_count BEFORE UPDATE OF count ON daily BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
        let second = format!("2026-09-11.{}", "b".repeat(64));
        assert!(s.accept(&second, now()).is_err());
        assert_eq!(
            s.conn
                .query_row("SELECT COUNT(*) FROM seen", [], |r| r.get::<_, u64>(0))
                .unwrap(),
            1
        );
        s.conn.execute_batch("DROP TRIGGER fail_count;").unwrap();
        s.accept(&second, now()).unwrap();
        assert_eq!(s.daily(now(), true).unwrap().days[0].count, 2);
    }
    #[test]
    fn rollover_erases_tokens_and_publishes_only_completed_days() {
        let mut s = store();
        s.accept(&token(), now()).unwrap();
        assert!(s.daily(now(), false).unwrap().days.is_empty());
        let tomorrow = now() + chrono::Duration::days(1);
        s.tick(tomorrow, false).unwrap();
        assert_eq!(
            s.conn
                .query_row("SELECT COUNT(*) FROM seen", [], |r| r.get::<_, u64>(0))
                .unwrap(),
            0
        );
        let daily = s.daily(tomorrow, false).unwrap();
        assert_eq!(daily.days[0].count, 1);
        assert!(!daily.days[0].complete);
        assert!(s.accept(&token(), tomorrow).is_err());
        assert!(s.tick(now(), false).is_err());
        let output = serde_json::to_string(&daily).unwrap();
        assert!(!output.contains("token"));
        assert!(!output.contains("hash"));
    }
    #[test]
    fn zero_requires_coverage_and_restore_marks_current_day_incomplete() {
        let mut s = store();
        let mut time = now();
        for _ in 0..2881 {
            time += chrono::Duration::seconds(30);
            s.tick(time, false).unwrap();
        }
        assert!(s
            .daily(time, true)
            .unwrap()
            .days
            .iter()
            .any(|d| d.date == "2026-09-12" && d.count == 0 && d.complete));
        s.restore(
            Daily {
                updated_at: time.to_rfc3339(),
                days: vec![Day {
                    date: time.date_naive().to_string(),
                    count: 10,
                    complete: true,
                }],
            },
            time,
        )
        .unwrap();
        assert!(!s.daily(time, true).unwrap().days.last().unwrap().complete);
    }
    #[test]
    fn restoring_an_old_export_marks_the_missing_days_incomplete() {
        let mut original = store();
        original.accept(&token(), now()).unwrap();
        let exported = original.daily(now(), true).unwrap();
        let recovered_at = now() + chrono::Duration::days(3);
        let mut recovered = Store::new(Connection::open_in_memory().unwrap()).unwrap();
        recovered.tick(recovered_at, true).unwrap();
        recovered.restore(exported, recovered_at).unwrap();
        let days = recovered.daily(recovered_at, true).unwrap().days;
        assert_eq!(days.len(), 4);
        assert_eq!(days[0].count, 1);
        assert!(days.iter().all(|day| !day.complete));
    }
    #[test]
    fn restarting_preserves_same_day_deduplication_and_purges_old_hashes() {
        let mut s = store();
        s.accept(&token(), now()).unwrap();
        s.tick(now(), true).unwrap();
        s.accept(&token(), now()).unwrap();
        assert_eq!(s.daily(now(), true).unwrap().days[0].count, 1);
        s.tick(now() + chrono::Duration::days(1), true).unwrap();
        assert_eq!(
            s.conn
                .query_row("SELECT COUNT(*) FROM seen", [], |r| r.get::<_, u64>(0))
                .unwrap(),
            0
        );
    }
    #[test]
    fn aggregate_contract_matches_shared_fixture() {
        let fixture: Daily =
            serde_json::from_str(include_str!("../../../tests/fixtures/usage/daily.json")).unwrap();
        assert_eq!(fixture.days.len(), 3);
        assert_eq!(fixture.days[1].count, 0);
        assert!(!fixture.days[2].complete);
    }
    #[tokio::test]
    async fn http_contract_rejects_extra_data_and_deduplicates() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let state = Collector::new(Store::new(Connection::open_in_memory().unwrap()).unwrap());
        state.tick().unwrap();
        let task = tokio::spawn(axum::serve(listener, router(state)).into_future());
        let client = reqwest::Client::new();
        let token = format!("{}.{}", Utc::now().date_naive(), "c".repeat(64));
        for _ in 0..2 {
            assert_eq!(
                client
                    .post(format!("{url}/api/usage/check-in"))
                    .json(&serde_json::json!({"token":token}))
                    .send()
                    .await
                    .unwrap()
                    .status(),
                204
            );
        }
        for payload in [
            serde_json::json!({"token":token,"account":"private"}),
            serde_json::json!({"token":"bad"}),
            serde_json::json!({"token":"x".repeat(1024)}),
        ] {
            assert!(client
                .post(format!("{url}/api/usage/check-in"))
                .json(&payload)
                .send()
                .await
                .unwrap()
                .status()
                .is_client_error());
        }
        let daily = client
            .get(format!("{url}/api/usage/daily"))
            .send()
            .await
            .unwrap();
        assert_eq!(
            daily.headers().get("cache-control").unwrap(),
            "public, max-age=3600"
        );
        assert!(daily.json::<Daily>().await.unwrap().days.is_empty());
        task.abort();
    }
    use std::future::IntoFuture;
}
