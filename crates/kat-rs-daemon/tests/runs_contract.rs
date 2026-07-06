use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::json;
use tempfile::tempdir;
use tower::ServiceExt;

use kat_rs_daemon::error::ErrorCode;

#[tokio::test]
async fn run_endpoint_returns_validation_error_for_unknown_run() {
    let app = kat_rs_daemon::router(kat_rs_daemon::AppState::new_for_tests());

    let response = request_json(app, "GET", "/v1/runs/run_missing", None).await;

    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{:?}",
        response.body
    );
    assert_eq!(response.body["error"]["code"], "VALIDATION_FAILED");
}

#[tokio::test]
async fn run_endpoint_rejects_unknown_pack_for_existing_dataset() {
    let datasets_dir = tempdir().expect("datasets tempdir is created");
    let datasets_root = datasets_dir.path().join("datasets");
    let dataset_name = "existing-dataset";

    std::fs::create_dir_all(datasets_root.join(dataset_name)).expect("dataset dir is created");

    let app = kat_rs_daemon::router(kat_rs_daemon::AppState::new_for_tests());
    let response = request_json(
        app.clone(),
        "POST",
        "/v1/runs",
        Some(json!({
            "packRef": "packs/example.yaml",
            "dataset": {
                "name": dataset_name,
                "directory": datasets_root.to_string_lossy(),
            },
            "inputs": {
                "traceId": "trace-1"
            }
        })),
    )
    .await;

    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{:?}",
        response.body
    );
    assert_eq!(response.body["error"]["code"], "VALIDATION_FAILED");
}

#[tokio::test]
async fn runs_openharmony_critical_task_pack_on_materialized_sqlite_dataset() {
    let sqlite_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root is above crate manifest dir")
        .join("test/test.db");
    if !sqlite_path.exists() {
        eprintln!(
            "skip runs_openharmony_critical_task_pack_on_materialized_sqlite_dataset: missing {}",
            sqlite_path.display()
        );
        return;
    }

    let datasets_dir = tempdir().expect("datasets tempdir is created");
    let datasets_root = datasets_dir.path().join("datasets");
    let dataset_name = "openharmony-critical-task-test";
    let app = kat_rs_daemon::router(kat_rs_daemon::AppState::new_for_tests());

    let create_dataset = request_json(
        app.clone(),
        "POST",
        "/v1/datasets",
        Some(json!({
            "dataset": {
                "name": dataset_name,
                "directory": datasets_root.to_string_lossy(),
            },
            "input": {
                "source": "SQLITE",
                "file": sqlite_path.to_string_lossy(),
            }
        })),
    )
    .await;

    assert_eq!(
        create_dataset.status,
        StatusCode::CREATED,
        "{:?}",
        create_dataset.body
    );

    let dataset = create_dataset.body["data"]["dataset"].clone();
    let run = request_json(
        app.clone(),
        "POST",
        "/v1/runs",
        Some(json!({
            "packRef": "openharmony.critical_task_extraction",
            "dataset": {
                "name": dataset["name"],
                "directory": dataset["directory"],
            },
            "inputs": {
                "process_name_pattern": "(^|\\.)tencent\\.wechat$|^com\\.tencent\\.wechat$",
                "start_marker_pattern": "HandleLaunchAbility.*com\\.tencent\\.wechat",
                "end_marker_pattern": "UIVsyncTask.*firstDrawFrame\\s*[:=]\\s*1",
            }
        })),
    )
    .await;

    assert_eq!(run.status, StatusCode::OK, "{:?}", run.body);
    assert_eq!(run.body["data"]["status"], "COMPLETED");
    assert_eq!(
        run.body["data"]["packRef"],
        "openharmony.critical_task_extraction"
    );
    assert_eq!(run.body["data"]["evidenceCount"], 2);

    let run_id = run.body["data"]["runId"]
        .as_str()
        .expect("run id is returned");

    let evidence = request_json(
        app.clone(),
        "GET",
        &format!("/v1/runs/{run_id}/evidence"),
        None,
    )
    .await;

    assert_eq!(evidence.status, StatusCode::OK, "{:?}", evidence.body);
    let critical_task_shape = evidence.body["data"]["evidence"]
        .as_array()
        .expect("evidence is an array")
        .iter()
        .find(|item| item["id"] == "critical_task_shape")
        .expect("critical_task_shape evidence is returned");
    assert_eq!(critical_task_shape["metrics"]["path_edge_count"], json!(8));
    assert_eq!(critical_task_shape["metrics"]["path_step_count"], json!(8));
    assert_eq!(critical_task_shape["metrics"]["task_count"], json!(8));
    assert_eq!(
        critical_task_shape["metrics"]["total_ranked_duration_ns"],
        json!(3544401000_i64)
    );

    let brief = request_json(app, "GET", &format!("/v1/runs/{run_id}/brief"), None).await;
    assert_eq!(brief.status, StatusCode::OK, "{:?}", brief.body);
    let has_critical_tasks = brief.body["data"]["sections"]
        .as_array()
        .expect("brief sections are an array")
        .iter()
        .any(|section| section["id"] == "critical_tasks");
    assert!(has_critical_tasks, "{:?}", brief.body);
}

#[test]
fn context_renderer_replaces_scalar_and_interval_slots() {
    let mut context = kat_rs_daemon::runs::context::ContextStore::new();
    context
        .publish_scalar("subject_thread_itid", json!(405), "test")
        .expect("scalar publishes");
    context
        .publish_interval("target_window", 245720189000, 246329390000, "test")
        .expect("interval publishes");

    let rendered = kat_rs_daemon::runs::render::render_template(
        "select {{ctx.subject_thread_itid}} as itid, {{ctx.target_window.start}} as start_ts, {{ctx.target_window.end}} as end_ts",
        &context,
    )
    .expect("template renders");

    assert_eq!(
        rendered,
        "select 405 as itid, 245720189000 as start_ts, 246329390000 as end_ts"
    );
}

#[test]
fn context_renderer_rejects_unknown_and_malformed_slots() {
    let mut context = kat_rs_daemon::runs::context::ContextStore::new();
    context
        .publish_interval("target_window", 245720189000, 246329390000, "test")
        .expect("interval publishes");

    let cases = [
        "select {{ctx.missing_slot}}",
        "select {{ctx.target_window}}",
        "select {{ ctx.target_window.start }}",
        "select {{ctx.target_window.starts}}",
        "select {{ctx.target_window.foo}}",
        "select {{ctx.target_window.start}",
    ];

    for template in cases {
        let error = kat_rs_daemon::runs::render::render_template(template, &context)
            .expect_err("template should be rejected");
        assert_eq!(
            error.code,
            ErrorCode::ValidationFailed,
            "template should fail validation: {template}"
        );
    }
}

#[test]
fn resource_root_loads_manifest_pack_and_entry_flow_from_fixture() {
    let temp = tempdir().expect("resource fixture tempdir is created");
    let resources_dir = temp.path().join("resources");
    let pack_dir = resources_dir.join("packs").join("example");
    std::fs::create_dir_all(&pack_dir).expect("fixture pack dir is created");
    std::fs::write(
        resources_dir.join("manifest.yaml"),
        r#"schema_version: 1
kind: kat.resources
packs:
  example:
    summary: Example test pack
    path: packs/example/pack.yaml
"#,
    )
    .expect("fixture manifest is written");
    std::fs::write(
        pack_dir.join("pack.yaml"),
        r#"pack:
  id: example
  title: Example
  domain: test
entry_flow: example_flow
"#,
    )
    .expect("fixture pack is written");
    std::fs::write(
        pack_dir.join("flow.yaml"),
        r#"id: example_flow
steps: []
"#,
    )
    .expect("fixture flow is written");

    let root = kat_rs_daemon::runs::resources::ResourceRoot::new(&resources_dir);

    let manifest = root.load_manifest().expect("manifest loads");
    let pack = root
        .load_pack(&manifest.value, "example")
        .expect("pack loads");
    let flow = root.load_entry_flow(&pack).expect("entry flow loads");

    assert!(manifest.digest.starts_with("sha256:"));
    assert!(pack.digest.starts_with("sha256:"));
    assert!(flow.digest.starts_with("sha256:"));
    assert_eq!(pack.value.pack.id, "example");
    assert_eq!(flow.value.id, "example_flow");
    assert_eq!(
        pack.path.file_name().and_then(|name| name.to_str()),
        Some("pack.yaml")
    );
    assert_eq!(
        flow.path.file_name().and_then(|name| name.to_str()),
        Some("flow.yaml")
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
