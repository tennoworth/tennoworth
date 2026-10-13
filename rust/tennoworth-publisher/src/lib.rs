pub mod auth;
pub mod contracts;
pub mod ops;
pub mod public;
pub mod store;

use auth::Auth;
use axum::{
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use contracts::{Change, DashboardState, EventChange, PublicFeedCheck, RunJob, StatusState};
use ops::Ops;
use public::PublicFeed;
use std::sync::{Arc, Mutex};
use store::{Error, Store};

#[derive(Clone)]
pub struct App {
    pub store: Arc<Mutex<Store>>,
    pub auth: Arc<Auth>,
    pub origin: String,
    /// Box status and jobs; absent when the publisher runs without a box.
    pub ops: Option<Arc<Ops>>,
    /// Where visitors fetch the feed; absent when nothing serves it.
    pub public_feed: Option<Arc<PublicFeed>>,
    /// Held from a job's running and cooldown checks until its request is
    /// written and recorded, so two simultaneous clicks cannot both pass.
    admission: Arc<tokio::sync::Mutex<()>>,
}

impl App {
    pub fn new(store: Store, auth: Auth, origin: String) -> anyhow::Result<Self> {
        let url = url::Url::parse(&origin)?;
        if url.origin().ascii_serialization() != origin
            || url.query().is_some()
            || url.fragment().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            anyhow::bail!("origin must be an exact origin");
        }
        if matches!(auth, Auth::Access(_)) && (url.scheme() != "https" || url.port().is_some()) {
            anyhow::bail!("production origin must use HTTPS");
        }
        Ok(Self {
            store: Arc::new(Mutex::new(store)),
            auth: Arc::new(auth),
            origin,
            ops: None,
            public_feed: None,
            admission: Arc::default(),
        })
    }

    pub fn with_ops(mut self, ops: Ops) -> Self {
        self.ops = Some(Arc::new(ops));
        self
    }

    pub fn with_public_feed(mut self, feed: PublicFeed) -> Self {
        self.public_feed = Some(Arc::new(feed));
        self
    }

    pub async fn maintenance(&self) {
        let store = self.store.clone();
        let _ = tokio::task::spawn_blocking(move || {
            if let Ok(mut store) = store.lock() {
                let _ = store.retry_export();
            }
        })
        .await;
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let (status,message) = match self {
            Self::Conflict => (StatusCode::CONFLICT, "Another revision exists. Refresh before retrying; your edits remain in the form.".into()),
            Self::Invalid(message) => (StatusCode::BAD_REQUEST,message),
            Self::Unavailable => (StatusCode::SERVICE_UNAVAILABLE,"Publication storage is unavailable. Refresh to check whether the request was saved before retrying.".into()),
        };
        (status, Json(serde_json::json!({ "message":message }))).into_response()
    }
}

enum ApiError {
    Unauthorized,
    Forbidden,
    Storage(Error),
    Job(StatusCode, String),
}
impl From<Error> for ApiError {
    fn from(error: Error) -> Self {
        Self::Storage(error)
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "Sign in through the private dashboard to continue.",
            ),
            Self::Forbidden => (
                StatusCode::FORBIDDEN,
                "Publish from the configured dashboard origin.",
            ),
            Self::Storage(error) => return error.into_response(),
            Self::Job(status, message) => {
                return (status, Json(serde_json::json!({ "message": message }))).into_response()
            }
        };
        (status, Json(serde_json::json!({"message":message}))).into_response()
    }
}

async fn principal(app: &App, headers: &HeaderMap) -> Result<String, ApiError> {
    app.auth
        .principal(headers)
        .await
        .ok_or(ApiError::Unauthorized)
}

async fn read(
    State(app): State<App>,
    headers: HeaderMap,
) -> Result<Json<DashboardState>, ApiError> {
    let actor = principal(&app, &headers).await?;
    let snapshot = tokio::task::spawn_blocking(move || {
        app.store.lock().map_err(|_| Error::Unavailable)?.snapshot()
    })
    .await
    .map_err(|_| Error::Unavailable)??;
    Ok(Json(DashboardState {
        actor,
        snapshot,
        message_id: None,
    }))
}

async fn change(
    State(app): State<App>,
    headers: HeaderMap,
    Json(change): Json<Change>,
) -> Result<Json<DashboardState>, ApiError> {
    let actor = principal(&app, &headers).await?;
    if headers.get("origin").and_then(|v| v.to_str().ok()) != Some(app.origin.as_str()) {
        return Err(ApiError::Forbidden);
    }
    let author = actor.clone();
    let (snapshot, message_id) = tokio::task::spawn_blocking(move || {
        let mut store = app.store.lock().map_err(|_| Error::Unavailable)?;
        let snapshot = store.apply(&change, &author, store::now())?;
        Ok::<_, Error>((
            snapshot,
            store.request_message(&change.request_id, &author)?,
        ))
    })
    .await
    .map_err(|_| Error::Unavailable)??;
    Ok(Json(DashboardState {
        actor,
        snapshot,
        message_id,
    }))
}

async fn change_event(
    State(app): State<App>,
    headers: HeaderMap,
    Json(change): Json<EventChange>,
) -> Result<Json<DashboardState>, ApiError> {
    let actor = principal(&app, &headers).await?;
    if headers.get("origin").and_then(|v| v.to_str().ok()) != Some(app.origin.as_str()) {
        return Err(ApiError::Forbidden);
    }
    let author = actor.clone();
    let (snapshot, message_id) = tokio::task::spawn_blocking(move || {
        let mut store = app.store.lock().map_err(|_| Error::Unavailable)?;
        let snapshot = store.apply_event(&change, &author, store::now())?;
        Ok::<_, Error>((
            snapshot,
            store.request_message(&change.request_id, &author)?,
        ))
    })
    .await
    .map_err(|_| Error::Unavailable)??;
    Ok(Json(DashboardState {
        actor,
        snapshot,
        message_id,
    }))
}

async fn public_feed(
    State(app): State<App>,
    headers: HeaderMap,
) -> Result<Json<PublicFeedCheck>, ApiError> {
    principal(&app, &headers).await?;
    let feed = app.public_feed.clone().ok_or_else(|| {
        ApiError::Job(
            StatusCode::SERVICE_UNAVAILABLE,
            "No public feed is configured for this publisher.".into(),
        )
    })?;
    let store = app.store.clone();
    let expected = tokio::task::spawn_blocking(move || {
        store
            .lock()
            .map_err(|_| Error::Unavailable)?
            .snapshot()
            .map(|snapshot| snapshot.feed.updated_at)
    })
    .await
    .map_err(|_| Error::Unavailable)??;
    Ok(Json(feed.check(&expected, store::now()).await))
}

fn ops(app: &App) -> Result<Arc<Ops>, ApiError> {
    app.ops.clone().ok_or_else(|| {
        ApiError::Job(
            StatusCode::SERVICE_UNAVAILABLE,
            "Box status is not configured for this publisher.".into(),
        )
    })
}

async fn latest_requests(app: &App) -> Result<Vec<(String, String, i64)>, ApiError> {
    let store = app.store.clone();
    Ok(tokio::task::spawn_blocking(move || {
        store
            .lock()
            .map_err(|_| Error::Unavailable)?
            .latest_job_requests()
    })
    .await
    .map_err(|_| Error::Unavailable)??)
}

async fn status(State(app): State<App>, headers: HeaderMap) -> Result<Json<StatusState>, ApiError> {
    let actor = principal(&app, &headers).await?;
    let ops = ops(&app)?;
    let requests = latest_requests(&app).await?;
    Ok(Json(StatusState {
        actor,
        status: ops.collect(store::now(), &requests).await,
    }))
}

async fn run_job(
    State(app): State<App>,
    headers: HeaderMap,
    Json(request): Json<RunJob>,
) -> Result<Json<StatusState>, ApiError> {
    let actor = principal(&app, &headers).await?;
    if headers.get("origin").and_then(|v| v.to_str().ok()) != Some(app.origin.as_str()) {
        return Err(ApiError::Forbidden);
    }
    let ops = ops(&app)?;
    let job = ops::job(&request.job).ok_or_else(|| {
        ApiError::Job(
            StatusCode::BAD_REQUEST,
            "The dashboard does not offer that job.".into(),
        )
    })?;
    let admission = app.admission.lock().await;
    let now = store::now();
    let units = ops
        .units(&[job.unit], now)
        .await
        .map_err(|message| ApiError::Job(StatusCode::SERVICE_UNAVAILABLE, message))?;
    let unit = units.get(job.unit).ok_or_else(|| {
        ApiError::Job(
            StatusCode::SERVICE_UNAVAILABLE,
            "systemd did not report on this job.".into(),
        )
    })?;
    let mut requests = latest_requests(&app).await?;
    let cooldown_until = requests
        .iter()
        .find(|(id, _, _)| id == job.id)
        .map(|(_, _, at)| at + job.cooldown)
        .filter(|until| *until > now);
    if let Some(reason) = ops.blocked(job, unit, cooldown_until, now) {
        return Err(ApiError::Job(
            StatusCode::CONFLICT,
            format!("{} was not started. {reason}", job.label),
        ));
    }
    ops.trigger(job, now)
        .map_err(|message| ApiError::Job(StatusCode::SERVICE_UNAVAILABLE, message))?;
    // The request is already on its way to systemd. From here nothing may
    // report the job as not started: a failed audit row is logged, and the
    // status is built from what was read above plus this request, rather than
    // from a second read that could fail after dispatch.
    let store = app.store.clone();
    let (id, author) = (job.id, actor.clone());
    let recorded = tokio::task::spawn_blocking(move || {
        store
            .lock()
            .map_err(|_| Error::Unavailable)?
            .record_job_request(id, &author, now)
    })
    .await;
    if !matches!(recorded, Ok(Ok(()))) {
        eprintln!(
            "tennoworth-publisher: {} was started but its request was not recorded",
            job.id
        );
    }
    drop(admission);
    requests.retain(|(id, _, _)| id != job.id);
    requests.push((job.id.to_owned(), actor.clone(), now));
    Ok(Json(StatusState {
        actor,
        status: ops.collect(now, &requests).await,
    }))
}

async fn private_headers(request: axum::extract::Request, next: middleware::Next) -> Response {
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        "cache-control",
        axum::http::HeaderValue::from_static("no-store"),
    );
    response.headers_mut().insert(
        "x-content-type-options",
        axum::http::HeaderValue::from_static("nosniff"),
    );
    response
}

pub fn router(app: App) -> Router {
    Router::new()
        .route("/api/announcements", get(read))
        .route("/api/announcements/change", post(change))
        .route("/api/announcements/public", get(public_feed))
        .route("/api/announcements/events", post(change_event))
        .route("/api/status", get(status))
        .route("/api/jobs/run", post(run_job))
        .layer(DefaultBodyLimit::max(65536))
        .layer(middleware::from_fn(private_headers))
        .with_state(app)
}

pub async fn serve(app: App, port: u16) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
    let maintenance = app.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(15)).await;
            maintenance.maintenance().await;
        }
    });
    axum::serve(listener, router(app))
        .with_graceful_shutdown(shutdown())
        .await?;
    Ok(())
}

// systemd stops the service with SIGTERM; Ctrl+C covers an interactive run.
async fn shutdown() {
    #[cfg(unix)]
    if let Ok(mut terminate) =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
    {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
        return;
    }
    let _ = tokio::signal::ctrl_c().await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{to_bytes, Body},
        http::Request,
    };
    use tower::ServiceExt;

    #[tokio::test]
    async fn same_origin_json_writes_are_bounded_and_errors_keep_private_headers() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(
            &dir.path().join("state.sqlite"),
            dir.path().join("announcements.json"),
            r#"{"version":1,"updated_at":"2026-01-01T00:00:00Z","messages":[]}"#,
        )
        .unwrap();
        let app = App::new(store, Auth::Local, "http://127.0.0.1:5194".into()).unwrap();
        let body = serde_json::json!({"request_id":"cross-origin-request-12345","expected_revision":0,"operation":"save","id":null,"content":{"title":"Test","body":"Useful next step","source_url":"https://forums.warframe.com/","starts_at":"2026-10-12T00:00:00Z","ends_at":"2026-10-15T15:00:00Z"}}).to_string();
        for origin in [None, Some("https://attacker.invalid")] {
            let mut request = Request::builder()
                .uri("/api/announcements/change")
                .method("POST")
                .header("content-type", "application/json");
            if let Some(origin) = origin {
                request = request.header("origin", origin);
            }
            let response = router(app.clone())
                .oneshot(request.body(Body::from(body.clone())).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
            assert_eq!(response.headers()["cache-control"], "no-store");
        }
        let response = router(app.clone())
            .oneshot(
                Request::builder()
                    .uri("/api/announcements/change")
                    .method("POST")
                    .header("content-type", "application/json")
                    .header("origin", "http://127.0.0.1:5194")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let result: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
        assert_eq!(result["snapshot"]["revision"], 1);
        for (body, status) in [
            ("{".to_string(), StatusCode::BAD_REQUEST),
            ("x".repeat(65537), StatusCode::PAYLOAD_TOO_LARGE),
        ] {
            let response = router(app.clone())
                .oneshot(
                    Request::builder()
                        .uri("/api/announcements/change")
                        .method("POST")
                        .header("content-type", "application/json")
                        .header("origin", "http://127.0.0.1:5194")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), status);
            assert_eq!(response.headers()["cache-control"], "no-store");
        }
        assert_eq!(app.store.lock().unwrap().snapshot().unwrap().revision, 1);
    }

    fn job_request(job: &str, origin: Option<&str>) -> Request<Body> {
        let mut request = Request::builder()
            .uri("/api/jobs/run")
            .method("POST")
            .header("content-type", "application/json");
        if let Some(origin) = origin {
            request = request.header("origin", origin);
        }
        request
            .body(Body::from(serde_json::json!({ "job": job }).to_string()))
            .unwrap()
    }

    async fn message(response: Response) -> String {
        let body: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 1 << 20).await.unwrap())
                .unwrap();
        body["message"].as_str().unwrap_or_default().to_owned()
    }

    #[tokio::test]
    async fn jobs_start_only_from_the_dashboard_once_per_run_and_are_audited() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(
            &dir.path().join("state.sqlite"),
            dir.path().join("announcements.json"),
            r#"{"version":1,"updated_at":"2026-01-01T00:00:00Z","messages":[]}"#,
        )
        .unwrap();
        let box_root = dir.path().join("box");
        std::fs::create_dir_all(box_root.join("jobs/backup")).unwrap();
        let app = App::new(store, Auth::Local, "http://127.0.0.1:5194".into())
            .unwrap()
            .with_ops(Ops::simulated(&box_root));
        let request_file = box_root.join("jobs/backup/request");
        let origin = Some("http://127.0.0.1:5194");

        for bad in [None, Some("https://attacker.invalid")] {
            let response = router(app.clone())
                .oneshot(job_request("backup", bad))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
        }
        let response = router(app.clone())
            .oneshot(job_request("../../etc", origin))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        // Offered by the publisher but not installed on the box: refused, nothing created.
        let response = router(app.clone())
            .oneshot(job_request("scrape", origin))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert!(message(response).await.contains("Not installed"));
        assert!(!box_root.join("jobs/scrape").exists());
        assert!(!request_file.exists());

        let response = router(app.clone())
            .oneshot(job_request("backup", origin))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 1 << 20).await.unwrap())
                .unwrap();
        let backup = body["status"]["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|j| j["id"] == "backup")
            .unwrap()
            .clone();
        assert_eq!(
            backup["last_request"]["actor"],
            "local-preview@example.invalid"
        );
        assert!(request_file.exists());

        // A second click while it runs is refused, and so is one inside the pacing window after it.
        let response = router(app.clone())
            .oneshot(job_request("backup", origin))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert!(message(response).await.contains("Running now"));
        let finished = std::time::SystemTime::now() - std::time::Duration::from_secs(30);
        std::fs::File::options()
            .write(true)
            .open(&request_file)
            .unwrap()
            .set_modified(finished)
            .unwrap();
        let response = router(app.clone())
            .oneshot(job_request("backup", origin))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert!(message(response).await.contains("Requested recently"));
        assert_eq!(
            app.store
                .lock()
                .unwrap()
                .latest_job_requests()
                .unwrap()
                .len(),
            1
        );
    }

    fn job_app(dir: &std::path::Path) -> App {
        let store = Store::open(
            &dir.join("state.sqlite"),
            dir.join("announcements.json"),
            r#"{"version":1,"updated_at":"2026-01-01T00:00:00Z","messages":[]}"#,
        )
        .unwrap();
        let box_root = dir.join("box");
        std::fs::create_dir_all(box_root.join("jobs/backup")).unwrap();
        App::new(store, Auth::Local, "http://127.0.0.1:5194".into())
            .unwrap()
            .with_ops(Ops::simulated(&box_root))
    }

    /// Two clicks that arrive together, from two tabs or a double submit, used to
    /// pass the running and cooldown checks before either had written anything.
    #[tokio::test]
    async fn simultaneous_requests_start_a_job_once() {
        let dir = tempfile::tempdir().unwrap();
        let app = job_app(dir.path());
        let origin = Some("http://127.0.0.1:5194");
        let (a, b) = tokio::join!(
            router(app.clone()).oneshot(job_request("backup", origin)),
            router(app.clone()).oneshot(job_request("backup", origin)),
        );
        let mut statuses = [a.unwrap().status(), b.unwrap().status()];
        statuses.sort();
        assert_eq!(statuses, [StatusCode::OK, StatusCode::CONFLICT]);
    }

    /// Once the request file is written the job is on its way. Nothing that
    /// fails afterwards may report it as not started, which invites a second click.
    #[tokio::test]
    async fn a_dispatched_job_is_reported_started_even_if_its_audit_row_fails() {
        let dir = tempfile::tempdir().unwrap();
        let app = job_app(dir.path());
        rusqlite::Connection::open(dir.path().join("state.sqlite"))
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER refuse_job_requests BEFORE INSERT ON job_requests
                 BEGIN SELECT RAISE(ABORT, 'audit unavailable'); END;",
            )
            .unwrap();
        let response = router(app.clone())
            .oneshot(job_request("backup", Some("http://127.0.0.1:5194")))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(dir.path().join("box/jobs/backup/request").exists());
        let body: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 1 << 20).await.unwrap())
                .unwrap();
        let backup = body["status"]["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|j| j["id"] == "backup")
            .unwrap()
            .clone();
        assert_eq!(
            backup["last_request"]["actor"],
            "local-preview@example.invalid"
        );
    }

    #[tokio::test]
    async fn public_feed_check_compares_the_served_feed_with_the_latest_publication() {
        let dir = tempfile::tempdir().unwrap();
        let export = dir.path().join("announcements.json");
        let open = || {
            Store::open(
                &dir.path().join("state.sqlite"),
                export.clone(),
                r#"{"version":1,"updated_at":"2026-01-01T00:00:00Z","messages":[]}"#,
            )
            .unwrap()
        };
        let check = |app: App| async move {
            let response = router(app)
                .oneshot(
                    Request::builder()
                        .uri("/api/announcements/public")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            let status = response.status();
            let body: serde_json::Value =
                serde_json::from_slice(&to_bytes(response.into_body(), 1 << 20).await.unwrap())
                    .unwrap();
            (status, body)
        };
        let unconfigured = App::new(open(), Auth::Local, "http://127.0.0.1:5194".into()).unwrap();
        assert_eq!(check(unconfigured).await.0, StatusCode::SERVICE_UNAVAILABLE);

        let app = App::new(open(), Auth::Local, "http://127.0.0.1:5194".into())
            .unwrap()
            .with_public_feed(PublicFeed::File(export.clone()));
        let (status, body) = check(app.clone()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["state"], "live");
        // The site still serves an older copy, as it would until caches expire.
        std::fs::write(
            &export,
            r#"{"version":1,"updated_at":"2025-12-31T00:00:00Z","messages":[]}"#,
        )
        .unwrap();
        let (_, body) = check(app).await;
        assert_eq!(body["state"], "behind");
        assert_eq!(body["served_updated_at"], "2025-12-31T00:00:00Z");
        assert_eq!(body["expected_updated_at"], "2026-01-01T00:00:00Z");
    }

    #[tokio::test]
    async fn status_reports_missing_sources_instead_of_failing() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(
            &dir.path().join("state.sqlite"),
            dir.path().join("announcements.json"),
            r#"{"version":1,"updated_at":"2026-01-01T00:00:00Z","messages":[]}"#,
        )
        .unwrap();
        let app = App::new(store, Auth::Local, "http://127.0.0.1:5194".into())
            .unwrap()
            .with_ops(Ops::simulated(&dir.path().join("empty-box")));
        let response = router(app)
            .oneshot(
                Request::builder()
                    .uri("/api/status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["cache-control"], "no-store");
        let body: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 1 << 20).await.unwrap())
                .unwrap();
        assert!(body["status"]["readiness"].is_null());
        assert!(body["status"]["problems"][0]
            .as_str()
            .unwrap()
            .contains("readiness"));
        assert!(body["status"]["backup"]["error"]
            .as_str()
            .unwrap()
            .contains("never run"));
        assert!(body["status"]["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|j| j["blocked"].as_str().unwrap().contains("Not installed")));
    }
}
