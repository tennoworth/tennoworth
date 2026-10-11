use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use std::collections::BTreeSet;
use tennoworth_publisher::{
    auth::{Access, Auth},
    router,
    store::Store,
    App,
};
use tower::ServiceExt;

#[tokio::test]
async fn private_routes_refuse_missing_authentication() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(
        &dir.path().join("state.sqlite"),
        dir.path().join("announcements.json"),
        r#"{"version":1,"updated_at":"2026-01-01T00:00:00Z","messages":[]}"#,
    )
    .unwrap();
    let auth = Access::new(
        "https://test.cloudflareaccess.com".into(),
        "test-audience".into(),
        BTreeSet::from(["publisher@example.invalid".into()]),
    )
    .unwrap();
    let app = App::new(
        store,
        Auth::Access(auth),
        "https://admin.example.invalid".into(),
    )
    .unwrap();
    let response = router(app)
        .oneshot(
            Request::builder()
                .uri("/api/announcements")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert!(!String::from_utf8(
        to_bytes(response.into_body(), 65536)
            .await
            .unwrap()
            .to_vec()
    )
    .unwrap()
    .contains("draft"));
}
