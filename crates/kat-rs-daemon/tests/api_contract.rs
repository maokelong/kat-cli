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

#[tokio::test]
async fn unknown_datasource_returns_structured_error() {
    let app = kat_rs_daemon::router(kat_rs_daemon::AppState::new_for_tests());

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/datasources/ds_missing")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("response is returned");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body bytes");
    let value: serde_json::Value = serde_json::from_slice(&body).expect("json body");

    assert_eq!(
        value,
        json!({
            "error": {
                "code": "DATASOURCE_NOT_FOUND",
                "message": "datasource not found",
                "details": {
                    "datasourceId": "ds_missing"
                }
            }
        })
    );
}
