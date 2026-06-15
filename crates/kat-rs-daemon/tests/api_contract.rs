use std::{
    fs::File,
    io::Write,
    path::{Path, PathBuf},
};

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use flate2::{Compression, write::GzEncoder};
use serde_json::json;
use tempfile::{TempDir, tempdir};
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
async fn langfuse_datasource_create_reuses_identity_and_can_be_deleted() {
    let fixture = LangfuseFixture::new();
    let app = kat_rs_daemon::router(kat_rs_daemon::AppState::new_for_tests());

    let create_body = json!({
        "source": "LANGFUSE_LEGACY",
        "observationsFile": fixture.observations_path(),
        "tracesFile": fixture.traces_path(),
    });

    let first = request_json(
        app.clone(),
        "POST",
        "/v1/datasources",
        Some(create_body.clone()),
    )
    .await;
    assert_eq!(first.status, StatusCode::CREATED, "{:?}", first.body);
    let first_id = first.body["data"]["id"]
        .as_str()
        .expect("created datasource id")
        .to_owned();

    let second = request_json(app.clone(), "POST", "/v1/datasources", Some(create_body)).await;
    assert_eq!(second.status, StatusCode::OK, "{:?}", second.body);
    assert_eq!(second.body["data"]["id"], first_id);

    let list = request_json(
        app.clone(),
        "GET",
        "/v1/datasources?limit=100&offset=0",
        None,
    )
    .await;
    assert_eq!(list.status, StatusCode::OK, "{:?}", list.body);
    assert_eq!(list.body["pagination"]["totalItems"], 1);

    let get = request_json(
        app.clone(),
        "GET",
        &format!("/v1/datasources/{first_id}"),
        None,
    )
    .await;
    assert_eq!(get.status, StatusCode::OK, "{:?}", get.body);
    assert_eq!(get.body["data"]["source"], "LANGFUSE_LEGACY");
    assert_eq!(get.body["data"]["inputs"][0]["role"], "OBSERVATIONS");
    assert!(
        get.body["data"]["inputs"][0]["sizeBytes"]
            .as_u64()
            .expect("input size is numeric")
            > 0
    );

    let delete = request_json(
        app.clone(),
        "DELETE",
        &format!("/v1/datasources/{first_id}"),
        None,
    )
    .await;
    assert_eq!(delete.status, StatusCode::NO_CONTENT, "{:?}", delete.body);

    let missing = request_json(app, "GET", &format!("/v1/datasources/{first_id}"), None).await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND, "{:?}", missing.body);
}

#[tokio::test]
async fn datasource_list_clamps_limit_in_pagination() {
    let app = kat_rs_daemon::router(kat_rs_daemon::AppState::new_for_tests());

    let zero = request_json(app.clone(), "GET", "/v1/datasources?limit=0&offset=0", None).await;
    assert_eq!(zero.status, StatusCode::OK, "{:?}", zero.body);
    assert_eq!(zero.body["pagination"]["limit"], 1);

    let huge = request_json(app, "GET", "/v1/datasources?limit=1000&offset=0", None).await;
    assert_eq!(huge.status, StatusCode::OK, "{:?}", huge.body);
    assert_eq!(huge.body["pagination"]["limit"], 500);
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

struct JsonResponse {
    status: StatusCode,
    body: serde_json::Value,
}

async fn request_json(
    app: axum::Router,
    method: &str,
    uri: &str,
    body: Option<serde_json::Value>,
) -> JsonResponse {
    let body = body
        .map(|body| Body::from(serde_json::to_vec(&body).expect("json body serializes")))
        .unwrap_or_else(Body::empty);
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(body)
        .expect("request builds");

    let response = app.oneshot(request).await.expect("response is returned");
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body bytes");
    let body = if body.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&body).expect("json body")
    };

    JsonResponse { status, body }
}

struct LangfuseFixture {
    _dir: TempDir,
    observations_path: PathBuf,
    traces_path: PathBuf,
}

impl LangfuseFixture {
    fn new() -> Self {
        let dir = tempdir().expect("tempdir is created");
        let observations_path = dir.path().join("observations.jsonl.gz");
        let traces_path = dir.path().join("traces.jsonl.gz");

        write_jsonl_gz(
            &observations_path,
            &[
                r#"{"id":"obs-1","trace_id":"trace-1","type":"GENERATION","input":"full prompt","output":"full completion"}"#,
            ],
        );
        write_jsonl_gz(
            &traces_path,
            &[r#"{"id":"trace-1","name":"chat request","user_id":"user-1"}"#],
        );

        Self {
            _dir: dir,
            observations_path,
            traces_path,
        }
    }

    fn observations_path(&self) -> String {
        self.observations_path.to_string_lossy().into_owned()
    }

    fn traces_path(&self) -> String {
        self.traces_path.to_string_lossy().into_owned()
    }
}

fn write_jsonl_gz(path: &Path, lines: &[&str]) {
    let file = File::create(path).expect("gzip fixture file is created");
    let mut encoder = GzEncoder::new(file, Compression::default());

    for line in lines {
        writeln!(encoder, "{line}").expect("jsonl line is written");
    }

    encoder.finish().expect("gzip stream is finished");
}
