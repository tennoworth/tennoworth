pub mod auth;
pub mod contracts;
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
use contracts::{Change, DashboardState};
use std::sync::{Arc, Mutex};
use store::{Error, Store};

#[derive(Clone)]
pub struct App {
    pub store: Arc<Mutex<Store>>,
    pub auth: Arc<Auth>,
    pub origin: String,
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
        })
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
    Ok(Json(DashboardState { actor, snapshot }))
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
    let snapshot = tokio::task::spawn_blocking(move || {
        app.store
            .lock()
            .map_err(|_| Error::Unavailable)?
            .apply(&change, &author, store::now())
    })
    .await
    .map_err(|_| Error::Unavailable)??;
    Ok(Json(DashboardState { actor, snapshot }))
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
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
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
}
