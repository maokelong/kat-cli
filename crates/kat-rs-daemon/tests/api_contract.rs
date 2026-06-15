use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::json;
use tower::ServiceExt;

#[tokio::test]
async fn health_endpoint_returns_ok() {
    let app = kat_rs_daemon::router(kat_rs_daemon::AppState::new_for_tests());

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/health")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("response is returned");

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body bytes");
    let value: serde_json::Value = serde_json::from_slice(&body).expect("json body");

    assert_eq!(
        value,
        json!({
            "data": {
                "status": "ok"
            }
        })
    );
}
