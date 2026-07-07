use std::{
    collections::BTreeSet,
    env,
    fs::{self, File},
    io::Write,
    net::{IpAddr, Ipv4Addr},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::Duration,
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
async fn server_delete_returns_shutdown_state() {
    let state = kat_rs_daemon::AppState::new_for_tests();
    let shutdown = state.shutdown.clone();
    let notified = shutdown.notified();
    let app = kat_rs_daemon::router(state);

    let response = request_json(app, "DELETE", "/v1/server", None).await;

    assert_eq!(response.status, StatusCode::ACCEPTED, "{:?}", response.body);
    assert_eq!(
        response.body,
        json!({
            "data": {
                "state": "SHUTTING_DOWN"
            }
        })
    );
    tokio::time::timeout(Duration::from_millis(100), notified)
        .await
        .expect("shutdown is notified");
}

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
async fn openapi_endpoint_returns_current_api_paths() {
    let app = kat_rs_daemon::router(kat_rs_daemon::AppState::new_for_tests());

    let response = app
        .oneshot(
            Request::builder()
                .uri("/openapi.json")
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

    assert_eq!(value["openapi"], "3.1.0");
    assert_eq!(value["info"]["title"], "kat-rs local API");
    assert!(value["paths"]["/v1/health"]["get"].is_object());
    assert!(value["paths"]["/v1/datasets"]["get"].is_object());
    assert!(value["paths"]["/v1/datasets"]["post"].is_object());
    assert!(value["paths"]["/v1/datasets/{datasetName}"]["get"].is_object());
    assert!(value["paths"]["/v1/datasets/{datasetName}"]["delete"].is_object());
    assert!(value["paths"]["/v1/datasets/queries"]["post"].is_object());
    assert!(value["paths"]["/v1/datasources"]["get"].is_object());
    assert!(value["paths"]["/v1/datasources"]["post"].is_object());
    assert!(value["paths"]["/v1/datasources/{datasourceId}"]["get"].is_object());
    assert!(value["paths"]["/v1/datasources/{datasourceId}"]["delete"].is_object());
    assert!(value["paths"]["/v1/datasources/{datasourceId}/queries"]["post"].is_object());
    assert!(value["paths"]["/v1/runs"]["post"].is_object());
    assert!(value["paths"]["/v1/runs/{runId}"]["get"].is_object());
    assert!(value["paths"]["/v1/runs/{runId}/evidence"]["get"].is_object());
    assert!(value["paths"]["/v1/server"]["delete"].is_object());

    let schemas = &value["components"]["schemas"];
    for schema in [
        "CreateDatasetRequest",
        "CreateDatasourceRequest",
        "DatasetQueryMeta",
        "DatasetQueryRequest",
        "DatasetDto",
        "DatasetInspectResponse",
        "DatasetLocation",
        "DatasetResponse",
        "DatasetSourceInput",
        "DatasetTableDto",
        "DatasourceDto",
        "DatasourceQueryMeta",
        "EvidenceRecordDto",
        "EvidenceRefDto",
        "ErrorEnvelope",
        "PaginatedEnvelope_DatasetDto",
        "QueryRequest",
        "QueryResponse_DatasetQueryMeta",
        "QueryResponse_DatasourceQueryMeta",
        "CreateRunRequest",
        "RunDto",
        "RunEvidenceResponse",
        "RunOutputDto",
        "RunStatus",
    ] {
        assert!(schemas[schema].is_object(), "missing schema {schema}");
    }

    assert_eq!(
        value["paths"]["/v1/datasets"]["post"]["requestBody"]["content"]["application/json"]["schema"]
            ["$ref"],
        "#/components/schemas/CreateDatasetRequest"
    );
    assert_eq!(
        value["paths"]["/v1/datasets"]["get"]["responses"]["200"]["content"]["application/json"]["schema"]
            ["$ref"],
        "#/components/schemas/PaginatedEnvelope_DatasetDto"
    );
    assert_eq!(
        value["paths"]["/v1/datasets/{datasetName}"]["get"]["responses"]["200"]["content"]["application/json"]
            ["schema"]["$ref"],
        "#/components/schemas/DataEnvelope_DatasetInspectResponse"
    );
    assert_eq!(
        value["paths"]["/v1/datasets/queries"]["post"]["requestBody"]["content"]["application/json"]
            ["schema"]["$ref"],
        "#/components/schemas/DatasetQueryRequest"
    );
    assert_eq!(
        value["paths"]["/v1/datasources"]["post"]["requestBody"]["content"]["application/json"]["schema"]
            ["$ref"],
        "#/components/schemas/CreateDatasourceRequest"
    );
    assert_eq!(
        value["paths"]["/v1/datasources/{datasourceId}/queries"]["post"]["requestBody"]["content"]
            ["application/json"]["schema"]["$ref"],
        "#/components/schemas/QueryRequest"
    );
    assert_eq!(
        value["paths"]["/v1/datasources/{datasourceId}/queries"]["post"]["responses"]["200"]["content"]
            ["application/json"]["schema"]["$ref"],
        "#/components/schemas/QueryResponse_DatasourceQueryMeta"
    );
    assert_eq!(
        value["paths"]["/v1/datasources/{datasourceId}"]["get"]["responses"]["404"]["content"]["application/json"]
            ["schema"]["$ref"],
        "#/components/schemas/ErrorEnvelope"
    );
    assert_eq!(
        value["paths"]["/v1/runs"]["post"]["requestBody"]["content"]["application/json"]["schema"]
            ["$ref"],
        "#/components/schemas/CreateRunRequest"
    );
    assert_eq!(
        value["paths"]["/v1/runs/{runId}"]["get"]["responses"]["200"]["content"]["application/json"]
            ["schema"]["$ref"],
        "#/components/schemas/DataEnvelope_RunDto"
    );
    assert_eq!(
        value["paths"]["/v1/runs/{runId}/evidence"]["get"]["responses"]["200"]["content"]["application/json"]
            ["schema"]["$ref"],
        "#/components/schemas/DataEnvelope_RunEvidenceResponse"
    );
}

#[tokio::test]
async fn dataset_lifecycle_lists_inspects_and_deletes_dataset() {
    let fixture = LangfuseFixture::new();
    let datasets_dir = tempdir().expect("datasets tempdir is created");
    let datasets_root = datasets_dir.path().join("datasets");
    let dataset_name = "lifecycle-dataset";
    let dataset_path = datasets_root.join(dataset_name);
    let app = kat_rs_daemon::router(kat_rs_daemon::AppState::new_for_tests());

    let create = request_json(
        app.clone(),
        "POST",
        "/v1/datasets",
        Some(json!({
            "dataset": {
                "name": dataset_name,
                "directory": datasets_root.to_string_lossy(),
            },
            "input": {
                "source": "LANGFUSE_LEGACY",
                "observationsFile": fixture.observations_path(),
                "tracesFile": fixture.traces_path(),
            }
        })),
    )
    .await;
    assert_eq!(create.status, StatusCode::CREATED, "{:?}", create.body);

    let list = request_json(
        app.clone(),
        "GET",
        &format!(
            "/v1/datasets?directory={}&limit=100&offset=0",
            datasets_root.to_string_lossy()
        ),
        None,
    )
    .await;
    assert_eq!(list.status, StatusCode::OK, "{:?}", list.body);
    assert_eq!(list.body["pagination"]["limit"], 100);
    assert_eq!(list.body["pagination"]["offset"], 0);
    assert_eq!(list.body["pagination"]["totalItems"], 1);
    assert_eq!(list.body["data"][0]["name"], dataset_name);
    assert_eq!(
        list.body["data"][0]["directory"],
        datasets_root.to_string_lossy().as_ref()
    );
    assert_eq!(
        list.body["data"][0]["path"],
        dataset_path.to_string_lossy().as_ref()
    );

    let inspect = request_json(
        app.clone(),
        "GET",
        &format!(
            "/v1/datasets/{dataset_name}?directory={}",
            datasets_root.to_string_lossy()
        ),
        None,
    )
    .await;
    assert_eq!(inspect.status, StatusCode::OK, "{:?}", inspect.body);
    assert_eq!(inspect.body["data"]["dataset"]["name"], dataset_name);
    assert_eq!(
        inspect.body["data"]["dataset"]["directory"],
        datasets_root.to_string_lossy().as_ref()
    );
    assert_eq!(
        inspect.body["data"]["dataset"]["path"],
        dataset_path.to_string_lossy().as_ref()
    );
    assert_eq!(
        inspect.body["data"]["tables"],
        json!([
            {
                "kind": "source",
                "name": "langfuse_observations",
                "path": "tables/langfuse.langfuse_observations.parquet",
                "sizeBytes": inspect.body["data"]["tables"][0]["sizeBytes"].clone()
            },
            {
                "kind": "source",
                "name": "langfuse_traces",
                "path": "tables/langfuse.langfuse_traces.parquet",
                "sizeBytes": inspect.body["data"]["tables"][1]["sizeBytes"].clone()
            }
        ])
    );
    assert!(
        inspect.body["data"]["tables"][0]["sizeBytes"]
            .as_u64()
            .expect("table size is numeric")
            > 0
    );
    assert!(
        inspect.body["data"]["tables"][1]["sizeBytes"]
            .as_u64()
            .expect("table size is numeric")
            > 0
    );
    assert!(
        inspect.body["data"]["tables"][0]["rowCount"].is_null(),
        "{:?}",
        inspect.body
    );

    let delete = request_json(
        app.clone(),
        "DELETE",
        &format!(
            "/v1/datasets/{dataset_name}?directory={}",
            datasets_root.to_string_lossy()
        ),
        None,
    )
    .await;
    assert_eq!(delete.status, StatusCode::NO_CONTENT, "{:?}", delete.body);
    assert!(delete.body.is_null(), "{:?}", delete.body);
    assert!(
        !dataset_path.exists(),
        "delete removes the resolved dataset directory"
    );

    let missing = request_json(
        app.clone(),
        "GET",
        &format!(
            "/v1/datasets/{dataset_name}?directory={}",
            datasets_root.to_string_lossy()
        ),
        None,
    )
    .await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND, "{:?}", missing.body);
    assert_eq!(missing.body["error"]["code"], "DATASET_NOT_FOUND");

    let empty_list = request_json(
        app,
        "GET",
        &format!(
            "/v1/datasets?directory={}&limit=100&offset=0",
            datasets_root.to_string_lossy()
        ),
        None,
    )
    .await;
    assert_eq!(empty_list.status, StatusCode::OK, "{:?}", empty_list.body);
    assert_eq!(empty_list.body["pagination"]["totalItems"], 0);
    assert_eq!(empty_list.body["data"], json!([]));
}

#[tokio::test]
async fn dataset_lifecycle_rejects_relative_directory_with_validation_error() {
    let app = kat_rs_daemon::router(kat_rs_daemon::AppState::new_for_tests());

    for (method, uri) in [
        ("GET", "/v1/datasets?directory=relative/datasets"),
        ("GET", "/v1/datasets/my-dataset?directory=relative/datasets"),
        (
            "DELETE",
            "/v1/datasets/my-dataset?directory=relative/datasets",
        ),
    ] {
        let response = request_json(app.clone(), method, uri, None).await;
        assert_eq!(
            response.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{:?}",
            response.body
        );
        assert_eq!(response.body["error"]["code"], "VALIDATION_FAILED");
    }
}

#[tokio::test]
async fn dataset_create_returns_conflict_when_target_exists() {
    let fixture = LangfuseFixture::new();
    let datasets_dir = tempdir().expect("datasets tempdir is created");
    let datasets_root = datasets_dir.path().join("datasets");
    let dataset_name = "existing-dataset";

    fs::create_dir_all(datasets_root.join(dataset_name)).expect("existing dataset dir is created");

    let app = kat_rs_daemon::router(kat_rs_daemon::AppState::new_for_tests());
    let response = request_json(
        app,
        "POST",
        "/v1/datasets",
        Some(json!({
            "dataset": {
                "name": dataset_name,
                "directory": datasets_root.to_string_lossy(),
            },
            "input": {
                "source": "LANGFUSE_LEGACY",
                "observationsFile": fixture.observations_path(),
                "tracesFile": fixture.traces_path(),
            }
        })),
    )
    .await;

    assert_eq!(response.status, StatusCode::CONFLICT, "{:?}", response.body);
    assert_eq!(response.body["error"]["code"], "CONFLICT");
}

#[tokio::test]
async fn dataset_create_rejects_relative_directory_with_validation_error() {
    let fixture = LangfuseFixture::new();
    let app = kat_rs_daemon::router(kat_rs_daemon::AppState::new_for_tests());
    let response = request_json(
        app,
        "POST",
        "/v1/datasets",
        Some(json!({
            "dataset": {
                "name": "relative-dir",
                "directory": "relative/datasets",
            },
            "input": {
                "source": "LANGFUSE_LEGACY",
                "observationsFile": fixture.observations_path(),
                "tracesFile": fixture.traces_path(),
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
async fn dataset_create_rejects_missing_required_fields_with_bad_request_envelope() {
    let fixture = LangfuseFixture::new();
    let datasets_dir = tempdir().expect("datasets tempdir is created");
    let app = kat_rs_daemon::router(kat_rs_daemon::AppState::new_for_tests());

    for body in [
        json!({
            "dataset": {
                "directory": datasets_dir.path().to_string_lossy(),
            },
            "input": {
                "source": "LANGFUSE_LEGACY",
                "observationsFile": fixture.observations_path(),
                "tracesFile": fixture.traces_path(),
            }
        }),
        json!({
            "dataset": {
                "name": "missing-input",
                "directory": datasets_dir.path().to_string_lossy(),
            }
        }),
    ] {
        let response = request_json(app.clone(), "POST", "/v1/datasets", Some(body)).await;

        assert_bad_request_envelope(response);
    }
}

#[tokio::test]
async fn dataset_create_rejects_missing_source_file_with_validation_error() {
    let datasets_dir = tempdir().expect("datasets tempdir is created");
    let missing_file = datasets_dir.path().join("missing.htrace");
    let app = kat_rs_daemon::router(kat_rs_daemon::AppState::new_for_tests());
    let response = request_json(
        app,
        "POST",
        "/v1/datasets",
        Some(json!({
            "dataset": {
                "name": "missing-source",
                "directory": datasets_dir.path().to_string_lossy(),
            },
            "input": {
                "source": "HITRACE",
                "file": missing_file.to_string_lossy(),
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
async fn dataset_create_rejects_unknown_fields_with_bad_request_envelope() {
    let fixture = LangfuseFixture::new();
    let datasets_dir = tempdir().expect("datasets tempdir is created");
    let app = kat_rs_daemon::router(kat_rs_daemon::AppState::new_for_tests());
    let response = request_json(
        app,
        "POST",
        "/v1/datasets",
        Some(json!({
            "dataset": {
                "name": "with-extra-field",
                "directory": datasets_dir.path().to_string_lossy(),
                "path": datasets_dir.path().join("with-extra-field").to_string_lossy(),
            },
            "input": {
                "source": "LANGFUSE_LEGACY",
                "observationsFile": fixture.observations_path(),
                "tracesFile": fixture.traces_path(),
            }
        })),
    )
    .await;

    assert_bad_request_envelope(response);
}

#[tokio::test]
async fn dataset_create_materializes_langfuse_fixture_and_can_query_without_sources() {
    let fixture = LangfuseFixture::new();
    let datasets_dir = tempdir().expect("datasets tempdir is created");
    let datasets_root = datasets_dir.path().join("datasets");
    let dataset_name = "langfuse-fixture";
    let dataset_path = datasets_root.join(dataset_name);
    let app = kat_rs_daemon::router(kat_rs_daemon::AppState::new_for_tests());

    let create = request_json(
        app,
        "POST",
        "/v1/datasets",
        Some(json!({
            "dataset": {
                "name": dataset_name,
                "directory": datasets_root.to_string_lossy(),
            },
            "input": {
                "source": "LANGFUSE_LEGACY",
                "observationsFile": fixture.observations_path(),
                "tracesFile": fixture.traces_path(),
            }
        })),
    )
    .await;

    assert_eq!(create.status, StatusCode::CREATED, "{:?}", create.body);
    assert_eq!(create.body["data"]["dataset"]["name"], dataset_name);
    assert_eq!(
        create.body["data"]["dataset"]["directory"],
        datasets_root.to_string_lossy().as_ref()
    );
    assert_eq!(
        create.body["data"]["dataset"]["path"],
        dataset_path.to_string_lossy().as_ref()
    );
    assert!(
        dataset_path.join("catalog.json").exists(),
        "dataset catalog should exist"
    );

    fs::remove_file(&fixture.observations_path).expect("observations source is removed");
    fs::remove_file(&fixture.traces_path).expect("traces source is removed");

    let datasource = kat_rs_datasource::TraceDatasource::from_dataset(&dataset_path)
        .await
        .expect("dataset opens after source files are removed");
    let rows = datasource
        .query_json("select count(*) as trace_count from langfuse_traces")
        .await
        .expect("dataset query succeeds");

    assert_eq!(rows, json!([{ "trace_count": 1 }]));
}

#[tokio::test]
async fn dataset_create_materializes_sqlite_pack_demo_fixture() {
    let sqlite = SqliteFixture::new();
    let datasets_dir = tempdir().expect("datasets tempdir is created");
    let datasets_root = datasets_dir.path().join("datasets");
    let dataset_name = "sqlite-pack-demo";
    let app = kat_rs_daemon::router(kat_rs_daemon::AppState::new_for_tests());

    let create = request_json(
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
                "file": sqlite.path(),
            }
        })),
    )
    .await;

    assert_eq!(create.status, StatusCode::CREATED, "{:?}", create.body);

    let inspect = request_json(
        app.clone(),
        "GET",
        &format!(
            "/v1/datasets/{dataset_name}?directory={}",
            datasets_root.to_string_lossy()
        ),
        None,
    )
    .await;
    assert_eq!(inspect.status, StatusCode::OK, "{:?}", inspect.body);
    let names = inspect.body["data"]["tables"]
        .as_array()
        .expect("tables")
        .iter()
        .map(|table| table["name"].as_str().expect("name").to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        vec!["process", "thread", "callstack", "thread_state", "instant"]
    );

    let query = request_json(
        app,
        "POST",
        "/v1/datasets/queries",
        Some(json!({
            "dataset": {
                "name": dataset_name,
                "directory": datasets_root.to_string_lossy(),
            },
            "sql": "select count(*) as process_count from process where name = '.tencent.wechat'"
        })),
    )
    .await;
    assert_eq!(query.status, StatusCode::OK, "{:?}", query.body);
    assert_eq!(query.body["data"], json!([{ "process_count": 1 }]));
}

#[tokio::test]
async fn dataset_query_reads_materialized_dataset_without_sources() {
    let fixture = LangfuseFixture::new();
    let datasets_dir = tempdir().expect("datasets tempdir is created");
    let datasets_root = datasets_dir.path().join("datasets");
    let dataset_name = "queryable-dataset";
    let dataset_path = datasets_root.join(dataset_name);
    let app = kat_rs_daemon::router(kat_rs_daemon::AppState::new_for_tests());

    let create = request_json(
        app.clone(),
        "POST",
        "/v1/datasets",
        Some(json!({
            "dataset": {
                "name": dataset_name,
                "directory": datasets_root.to_string_lossy(),
            },
            "input": {
                "source": "LANGFUSE_LEGACY",
                "observationsFile": fixture.observations_path(),
                "tracesFile": fixture.traces_path(),
            }
        })),
    )
    .await;
    assert_eq!(create.status, StatusCode::CREATED, "{:?}", create.body);

    fs::remove_file(&fixture.observations_path).expect("observations source is removed");
    fs::remove_file(&fixture.traces_path).expect("traces source is removed");

    let query = request_json(
        app,
        "POST",
        "/v1/datasets/queries",
        Some(json!({
            "dataset": {
                "name": dataset_name,
                "directory": datasets_root.to_string_lossy(),
            },
            "sql": "select count(*) as trace_count from langfuse_traces"
        })),
    )
    .await;

    assert_eq!(query.status, StatusCode::OK, "{:?}", query.body);
    assert_eq!(query.body["meta"]["dataset"]["name"], dataset_name);
    assert_eq!(
        query.body["meta"]["dataset"]["directory"],
        datasets_root.to_string_lossy().as_ref()
    );
    assert_eq!(
        query.body["meta"]["dataset"]["path"],
        dataset_path.to_string_lossy().as_ref()
    );
    assert!(
        query.body["meta"]["elapsedMs"].is_number(),
        "{:?}",
        query.body
    );
    assert_eq!(query.body["rowCount"], 1);
    assert_eq!(query.body["data"], json!([{ "trace_count": 1 }]));
    assert!(query.body["data"]["rows"].is_null(), "{:?}", query.body);
}

#[tokio::test]
async fn dataset_query_returns_not_found_for_missing_dataset() {
    let datasets_dir = tempdir().expect("datasets tempdir is created");
    let app = kat_rs_daemon::router(kat_rs_daemon::AppState::new_for_tests());
    let response = request_json(
        app,
        "POST",
        "/v1/datasets/queries",
        Some(json!({
            "dataset": {
                "name": "missing-dataset",
                "directory": datasets_dir.path().to_string_lossy(),
            },
            "sql": "select 1"
        })),
    )
    .await;

    assert_eq!(
        response.status,
        StatusCode::NOT_FOUND,
        "{:?}",
        response.body
    );
    assert_eq!(response.body["error"]["code"], "DATASET_NOT_FOUND");
}

#[tokio::test]
async fn dataset_query_rejects_relative_directory_with_validation_error() {
    let app = kat_rs_daemon::router(kat_rs_daemon::AppState::new_for_tests());
    let response = request_json(
        app,
        "POST",
        "/v1/datasets/queries",
        Some(json!({
            "dataset": {
                "name": "relative-dir",
                "directory": "relative/datasets",
            },
            "sql": "select 1"
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
async fn query_endpoint_returns_meta_row_count_and_data() {
    let fixture = LangfuseFixture::new();
    let app = kat_rs_daemon::router(kat_rs_daemon::AppState::new_for_tests());

    let create = request_json(
        app.clone(),
        "POST",
        "/v1/datasources",
        Some(json!({
            "source": "LANGFUSE_LEGACY",
            "observationsFile": fixture.observations_path(),
            "tracesFile": fixture.traces_path(),
        })),
    )
    .await;
    assert_eq!(create.status, StatusCode::CREATED, "{:?}", create.body);
    let datasource_id = create.body["data"]["id"]
        .as_str()
        .expect("created datasource id");

    let query = request_json(
        app,
        "POST",
        &format!("/v1/datasources/{datasource_id}/queries"),
        Some(json!({
            "sql": "select count(*) as trace_count from langfuse_traces"
        })),
    )
    .await;

    assert_eq!(query.status, StatusCode::OK, "{:?}", query.body);
    assert_eq!(query.body["meta"]["datasourceId"], datasource_id);
    assert!(
        query.body["meta"]["elapsedMs"].is_number(),
        "{:?}",
        query.body
    );
    assert_eq!(query.body["rowCount"], 1);
    assert_eq!(query.body["data"], json!([{ "trace_count": 1 }]));
    assert!(query.body["data"]["rows"].is_null(), "{:?}", query.body);
}

#[tokio::test]
async fn query_endpoint_returns_query_failed_for_invalid_sql() {
    let fixture = LangfuseFixture::new();
    let app = kat_rs_daemon::router(kat_rs_daemon::AppState::new_for_tests());

    let create = request_json(
        app.clone(),
        "POST",
        "/v1/datasources",
        Some(json!({
            "source": "LANGFUSE_LEGACY",
            "observationsFile": fixture.observations_path(),
            "tracesFile": fixture.traces_path(),
        })),
    )
    .await;
    assert_eq!(create.status, StatusCode::CREATED, "{:?}", create.body);
    let datasource_id = create.body["data"]["id"]
        .as_str()
        .expect("created datasource id");

    let query = request_json(
        app,
        "POST",
        &format!("/v1/datasources/{datasource_id}/queries"),
        Some(json!({
            "sql": "select * from missing_table"
        })),
    )
    .await;

    assert_eq!(
        query.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{:?}",
        query.body
    );
    assert_eq!(query.body["error"]["code"], "QUERY_FAILED");
}

#[tokio::test]
async fn query_endpoint_returns_structured_error_for_missing_datasource() {
    let app = kat_rs_daemon::router(kat_rs_daemon::AppState::new_for_tests());

    let response = request_json(
        app,
        "POST",
        "/v1/datasources/ds_missing/queries",
        Some(json!({
            "sql": "select 1"
        })),
    )
    .await;

    assert_eq!(
        response.status,
        StatusCode::NOT_FOUND,
        "{:?}",
        response.body
    );
    assert_eq!(
        response.body,
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

#[tokio::test]
async fn run_endpoint_returns_not_found_for_missing_run() {
    let app = kat_rs_daemon::router(kat_rs_daemon::AppState::new_for_tests());

    let response = request_json(app, "GET", "/v1/runs/run_missing", None).await;

    assert_eq!(
        response.status,
        StatusCode::NOT_FOUND,
        "{:?}",
        response.body
    );
    assert_eq!(response.body["error"]["code"], "RUN_NOT_FOUND");
}

#[tokio::test]
async fn run_create_returns_not_found_for_missing_dataset() {
    let datasets_dir = tempdir().expect("datasets tempdir is created");
    let app = kat_rs_daemon::router(kat_rs_daemon::AppState::new_for_tests());

    let response = request_json(
        app,
        "POST",
        "/v1/runs",
        Some(json!({
            "dataset": {
                "name": "missing-dataset",
                "directory": datasets_dir.path().to_string_lossy(),
            },
            "packRef": "pack:test",
            "inputs": {}
        })),
    )
    .await;

    assert_eq!(
        response.status,
        StatusCode::NOT_FOUND,
        "{:?}",
        response.body
    );
    assert_eq!(response.body["error"]["code"], "DATASET_NOT_FOUND");
}

#[tokio::test]
async fn run_endpoint_executes_pack_until_query_outputs_exist() {
    let _guard = current_dir_lock().lock().expect("current dir lock");
    let _cwd = CurrentDirGuard::set(workspace_root());

    let pack_root = workspace_root().join("packs");
    if !pack_root.exists() {
        eprintln!("skipping run smoke test because packs directory is not present");
        return;
    }

    let sqlite = SqliteFixture::new();
    let datasets_dir = tempdir().expect("datasets tempdir is created");
    let datasets_root = datasets_dir.path().join("datasets");
    let dataset_name = "sqlite-run-smoke";
    let app = kat_rs_daemon::router(kat_rs_daemon::AppState::new_for_tests());

    let create = request_json(
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
                "file": sqlite.path(),
            }
        })),
    )
    .await;
    assert_eq!(create.status, StatusCode::CREATED, "{:?}", create.body);

    let run = request_json(
        app.clone(),
        "POST",
        "/v1/runs",
        Some(json!({
            "dataset": {
                "name": dataset_name,
                "directory": datasets_root.to_string_lossy(),
            },
            "packRef": "scheduling/app-launch-critical-path/critical-task-extraction",
            "inputs": {
                "process_name_pattern": "(^|\\.)tencent\\.wechat$|^com\\.tencent\\.wechat$",
                "start_marker_pattern": "HandleLaunchAbility.*com\\.tencent\\.wechat",
                "end_marker_pattern": "UIVsyncTask.*firstDrawFrame:1"
            }
        })),
    )
    .await;

    assert_eq!(run.status, StatusCode::CREATED, "{:?}", run.body);
    assert_eq!(run.body["data"]["status"], "SUCCEEDED");
    assert!(
        run.body["data"]["outputs"]["target_window"]["rowCount"]
            .as_u64()
            .unwrap()
            >= 1
    );
    let critical_tasks_row_count = run.body["data"]["outputs"]["critical_tasks"]["rowCount"]
        .as_i64()
        .unwrap();
    assert!(critical_tasks_row_count >= 0);
}

#[tokio::test]
async fn run_evidence_endpoint_returns_summary_records() {
    let _guard = current_dir_lock().lock().expect("current dir lock");
    let _cwd = CurrentDirGuard::set(workspace_root());

    let pack_root = workspace_root().join("packs");
    if !pack_root.exists() {
        eprintln!("skipping evidence test because packs directory is not present");
        return;
    }

    let sqlite = SqliteFixture::new();
    let datasets_dir = tempdir().expect("datasets tempdir is created");
    let datasets_root = datasets_dir.path().join("datasets");
    let dataset_name = "sqlite-run-evidence";
    let app = kat_rs_daemon::router(kat_rs_daemon::AppState::new_for_tests());

    let create = request_json(
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
                "file": sqlite.path(),
            }
        })),
    )
    .await;
    assert_eq!(create.status, StatusCode::CREATED, "{:?}", create.body);

    let run = request_json(
        app.clone(),
        "POST",
        "/v1/runs",
        Some(json!({
            "dataset": {
                "name": dataset_name,
                "directory": datasets_root.to_string_lossy(),
            },
            "packRef": "scheduling/app-launch-critical-path/critical-task-extraction",
            "inputs": {
                "process_name_pattern": "(^|\\.)tencent\\.wechat$|^com\\.tencent\\.wechat$",
                "start_marker_pattern": "HandleLaunchAbility.*com\\.tencent\\.wechat",
                "end_marker_pattern": "UIVsyncTask.*firstDrawFrame:1"
            }
        })),
    )
    .await;

    assert_eq!(run.status, StatusCode::CREATED, "{:?}", run.body);
    let run_id = run.body["data"]["runId"].as_str().expect("run id");
    assert_eq!(run.body["data"]["evidenceCount"], 2);
    assert_eq!(run.body["data"]["outputs"]["critical_task_evidence"]["kind"], "evidence");
    assert!(run.body["data"]["outputs"]["critical_task_evidence"]["rowCount"].is_null());

    let evidence = request_json(app, "GET", &format!("/v1/runs/{run_id}/evidence"), None).await;
    assert_eq!(evidence.status, StatusCode::OK, "{:?}", evidence.body);
    let records = evidence.body["data"]["evidence"]
        .as_array()
        .expect("evidence array");
    assert_eq!(records.len(), 2, "{:?}", evidence.body);

    let target_window = &records[0];
    assert_eq!(target_window["id"], "target_window_shape");
    assert_eq!(target_window["fact"], "target_window_shape");
    assert_eq!(
        target_window["producingStep"],
        "local.summaries.critical_task_evidence"
    );
    assert_eq!(target_window["metrics"]["window_count"], 1);
    assert_eq!(target_window["metrics"]["window_dur"], 300);
    let target_refs = target_window["refs"].as_array().expect("target refs");
    assert_eq!(target_refs.len(), 2);
    assert_eq!(target_refs[0]["table"], "target_thread");
    let target_thread_rows = target_refs[0]["rows"].as_array().expect("target thread rows");
    assert_eq!(target_thread_rows.len(), 1);
    assert_eq!(target_thread_rows[0]["process_row_id"], 1);
    assert_eq!(target_thread_rows[0]["ipid"], 89);
    assert_eq!(target_thread_rows[0]["pid"], 15040);
    assert_eq!(target_thread_rows[0]["process_name"], ".tencent.wechat");
    assert_eq!(target_thread_rows[0]["thread_row_id"], 1);
    assert_eq!(target_thread_rows[0]["itid"], 405);
    assert_eq!(target_thread_rows[0]["tid"], 15040);
    assert_eq!(target_thread_rows[0]["thread_name"], ".tencent.wechat");
    assert_eq!(target_thread_rows[0]["is_main_thread"], 1);
    assert_eq!(target_refs[1]["table"], "target_window");
    let target_window_rows = target_refs[1]["rows"].as_array().expect("target window rows");
    assert_eq!(target_window_rows.len(), 1);
    assert_eq!(target_window_rows[0]["itid"], 405);
    assert_eq!(target_window_rows[0]["tid"], 15040);
    assert_eq!(target_window_rows[0]["thread_name"], ".tencent.wechat");
    assert_eq!(target_window_rows[0]["window_start_callstack_id"], 1);
    assert_eq!(target_window_rows[0]["window_start_ts"], 1000);
    assert_eq!(target_window_rows[0]["window_start_dur"], 100);
    assert_eq!(
        target_window_rows[0]["window_start_marker_name"],
        "HandleLaunchAbility##com.tencent.wechat"
    );
    assert_eq!(target_window_rows[0]["window_end_callstack_id"], 2);
    assert_eq!(target_window_rows[0]["window_end_ts"], 1300);
    assert_eq!(target_window_rows[0]["window_end_dur"], 1);
    assert_eq!(
        target_window_rows[0]["window_end_marker_name"],
        "UIVsyncTask[firstDrawFrame:1]"
    );
    assert_eq!(target_window_rows[0]["window_dur"], 300);

    let critical = &records[1];
    assert_eq!(critical["id"], "critical_task_shape");
    assert_eq!(critical["fact"], "critical_task_shape");
    assert_eq!(
        critical["producingStep"],
        "local.summaries.critical_task_evidence"
    );
    assert_eq!(critical["metrics"]["path_edge_count"], 0);
    assert_eq!(critical["metrics"]["path_step_count"], 0);
    assert_eq!(critical["metrics"]["task_count"], 0);
    assert!(critical["metrics"]["total_ranked_duration_ns"].is_null());
    assert_eq!(critical["metrics"]["distinct_task_type_count"], 0);
    let critical_refs = critical["refs"].as_array().expect("critical refs");
    assert_eq!(critical_refs.len(), 2);
    assert_eq!(critical_refs[0]["table"], "path_steps");
    assert_eq!(
        critical_refs[0]["rows"].as_array().expect("path step rows").len(),
        0
    );
    assert_eq!(critical_refs[1]["table"], "critical_tasks");
    assert_eq!(
        critical_refs[1]
            ["rows"]
            .as_array()
            .expect("critical task rows")
            .len(),
        0
    );
}

#[tokio::test]
#[ignore = "requires local 60 MiB test/test.db fixture"]
async fn run_endpoint_succeeds_on_local_test_db_fixture() {
    let _guard = current_dir_lock().lock().expect("current dir lock");
    let _cwd = CurrentDirGuard::set(workspace_root());

    let workspace = workspace_root();
    let sqlite_path = workspace.join("test").join("test.db");
    assert!(
        sqlite_path.exists(),
        "local fixture is missing: {}",
        sqlite_path.display()
    );

    let datasets_dir = tempdir().expect("datasets tempdir is created");
    let datasets_root = datasets_dir.path().join("datasets");
    let dataset_name = "local-test-db";
    let app = kat_rs_daemon::router(kat_rs_daemon::AppState::new_for_tests());

    let create = request_json(
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
    assert_eq!(create.status, StatusCode::CREATED, "{:?}", create.body);

    let run = request_json(
        app.clone(),
        "POST",
        "/v1/runs",
        Some(json!({
            "dataset": {
                "name": dataset_name,
                "directory": datasets_root.to_string_lossy(),
            },
            "packRef": "scheduling/app-launch-critical-path/critical-task-extraction",
            "inputs": {
                "process_name_pattern": "(^|\\.)tencent\\.wechat$|^com\\.tencent\\.wechat$",
                "start_marker_pattern": "HandleLaunchAbility.*com\\.tencent\\.wechat",
                "end_marker_pattern": "UIVsyncTask.*firstDrawFrame:1"
            }
        })),
    )
    .await;

    assert_eq!(run.status, StatusCode::CREATED, "{:?}", run.body);
    assert_eq!(run.body["data"]["status"], "SUCCEEDED");
    assert_eq!(run.body["data"]["evidenceCount"], 2);
    assert_eq!(run.body["data"]["outputs"]["critical_task_evidence"]["kind"], "evidence");
    assert!(run.body["data"]["outputs"]["critical_task_evidence"]["rowCount"].is_null());

    let run_id = run.body["data"]["runId"].as_str().expect("run id");
    let evidence = request_json(app, "GET", &format!("/v1/runs/{run_id}/evidence"), None).await;
    assert_eq!(evidence.status, StatusCode::OK, "{:?}", evidence.body);
    let records = evidence.body["data"]["evidence"]
        .as_array()
        .expect("evidence array");
    assert_eq!(records.len(), 2, "{:?}", evidence.body);

    let critical = records
        .iter()
        .find(|record| record["id"] == "critical_task_shape")
        .expect("critical task evidence exists");
    assert!(critical["metrics"]["path_edge_count"].as_u64().unwrap() > 0);
    assert!(critical["metrics"]["path_step_count"].as_u64().unwrap() > 0);
    assert!(critical["metrics"]["task_count"].as_u64().unwrap() > 0);
    assert!(
        critical["metrics"]["total_ranked_duration_ns"]
            .as_i64()
            .unwrap()
            > 0
    );
    assert!(critical["metrics"]["distinct_task_type_count"].as_u64().unwrap() > 0);

    let critical_refs = critical["refs"].as_array().expect("critical refs");
    assert_eq!(critical_refs.len(), 2);
    let path_steps = critical_refs[0]["rows"].as_array().expect("path steps rows");
    assert!(!path_steps.is_empty(), "{:?}", evidence.body);
    assert!(path_steps.len() <= 12, "{:?}", evidence.body);
    assert_eq!(
        json_object_keys(&path_steps[0]),
        BTreeSet::from([
            "critical_cost_ns".to_string(),
            "edge_kind".to_string(),
            "iteration".to_string(),
            "purpose_code".to_string(),
            "purpose_hint".to_string(),
        ])
    );
    let step_iterations = path_steps
        .iter()
        .map(|row| row["iteration"].as_i64().expect("iteration"))
        .collect::<Vec<_>>();
    assert!(step_iterations.windows(2).all(|pair| pair[0] <= pair[1]));

    let critical_tasks = critical_refs[1]["rows"]
        .as_array()
        .expect("critical task rows");
    assert!(!critical_tasks.is_empty(), "{:?}", evidence.body);
    assert!(critical_tasks.len() <= 12, "{:?}", evidence.body);
    assert_eq!(
        json_object_keys(&critical_tasks[0]),
        BTreeSet::from([
            "duration_ns".to_string(),
            "label".to_string(),
            "rank".to_string(),
            "raw_refs".to_string(),
            "reason_code".to_string(),
            "task_type".to_string(),
            "thread_name".to_string(),
        ])
    );
    let ranks = critical_tasks
        .iter()
        .map(|row| row["rank"].as_i64().expect("rank"))
        .collect::<Vec<_>>();
    assert!(ranks.windows(2).all(|pair| pair[0] <= pair[1]));
}

#[test]
fn pack_loader_expands_current_critical_path_pack() {
    let pack_root = workspace_root().join("packs");
    if !pack_root.exists() {
        eprintln!("skipping pack loader test because packs directory is not present");
        return;
    }

    let snapshot = kat_rs_daemon::pack_runtime::load_snapshot(
        &pack_root,
        "scheduling/app-launch-critical-path/critical-task-extraction",
    )
    .expect("snapshot loads");

    assert_eq!(snapshot.entry.coord, "local.flows.critical-task-extraction");
    assert!(
        snapshot
            .resources
            .contains_key("common.query.process_by_name_regex")
    );
    assert!(
        snapshot
            .resources
            .contains_key("local.summaries.critical_task_evidence")
    );
    for resource in snapshot.resources.values() {
        assert!(resource.digest.starts_with("sha256:"), "{resource:?}");
        assert!(resource.path.ends_with(".yaml"), "{resource:?}");
    }
}

#[test]
fn pack_loader_rejects_recursive_pack_resource_reference() {
    let fixture = tempdir().expect("pack tempdir is created");
    let pack_root = fixture.path();
    let pack_ref = "cycle/demo";
    let pack_dir = pack_root.join("cycle");
    let local_flows_dir = pack_dir.join("local").join("flows");

    fs::create_dir_all(&local_flows_dir).expect("local flows dir is created");
    fs::write(
        pack_dir.join("demo.yaml"),
        r#"kind: flow
description: demo
inputs: {}
steps:
  - run: local.flows.a
"#,
    )
    .expect("entry flow is written");
    fs::write(
        local_flows_dir.join("a.yaml"),
        r#"kind: flow
description: a
inputs: {}
steps:
  - run: local.flows.b
"#,
    )
    .expect("flow a is written");
    fs::write(
        local_flows_dir.join("b.yaml"),
        r#"kind: flow
description: b
inputs: {}
steps:
  - run: local.flows.a
"#,
    )
    .expect("flow b is written");

    let error = kat_rs_daemon::pack_runtime::load_snapshot(pack_root, pack_ref)
        .expect_err("cycle should be rejected");

    assert!(
        error.message.contains("recursive pack resource reference"),
        "unexpected error: {:?}",
        error
    );
}

#[tokio::test]
async fn datasource_create_rejects_malformed_json_with_bad_request_envelope() {
    let app = kat_rs_daemon::router(kat_rs_daemon::AppState::new_for_tests());

    let response = request_raw_json(app, "POST", "/v1/datasources", r#"{"source":"HITRACE""#).await;

    assert_bad_request_envelope(response);
}

#[tokio::test]
async fn datasource_create_rejects_unknown_fields_with_bad_request_envelope() {
    let fixture = LangfuseFixture::new();
    let app = kat_rs_daemon::router(kat_rs_daemon::AppState::new_for_tests());

    for field in ["sizeBytes", "modifiedAt"] {
        let response = request_json(
            app.clone(),
            "POST",
            "/v1/datasources",
            Some(json!({
                "source": "LANGFUSE_LEGACY",
                "observationsFile": fixture.observations_path(),
                "tracesFile": fixture.traces_path(),
                field: 1,
            })),
        )
        .await;

        assert_bad_request_envelope(response);
    }
}

#[tokio::test]
async fn datasource_list_rejects_invalid_query_params_with_bad_request_envelope() {
    let app = kat_rs_daemon::router(kat_rs_daemon::AppState::new_for_tests());

    let response = request_json(app, "GET", "/v1/datasources?limit=abc", None).await;

    assert_bad_request_envelope(response);
}

#[tokio::test]
async fn query_endpoint_rejects_wrong_json_types_before_datasource_lookup() {
    let app = kat_rs_daemon::router(kat_rs_daemon::AppState::new_for_tests());

    let response = request_json(
        app,
        "POST",
        "/v1/datasources/ds_missing/queries",
        Some(json!({
            "sql": 1
        })),
    )
    .await;

    assert_bad_request_envelope(response);
}

#[tokio::test]
async fn serve_rejects_non_loopback_host_before_bind() {
    let result = tokio::time::timeout(
        Duration::from_millis(100),
        kat_rs_daemon::serve(kat_rs_daemon::DaemonConfig {
            host: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            port: 0,
        }),
    )
    .await
    .expect("serve rejects non-loopback host before serving");
    let error = result.expect_err("non-loopback host is rejected");

    assert!(
        format!("{error:#}").contains("loopback"),
        "unexpected error: {error:#}"
    );
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

async fn request_raw_json(
    app: axum::Router,
    method: &str,
    uri: &str,
    body: impl Into<Body>,
) -> JsonResponse {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(body.into())
        .expect("request builds");

    let response = app.oneshot(request).await.expect("response is returned");
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body bytes");
    let body = serde_json::from_slice(&body).expect("json body");

    JsonResponse { status, body }
}

fn assert_bad_request_envelope(response: JsonResponse) {
    assert_eq!(
        response.status,
        StatusCode::BAD_REQUEST,
        "{:?}",
        response.body
    );
    assert_eq!(response.body["error"]["code"], "BAD_REQUEST");
    assert!(
        response.body["error"]["message"].is_string(),
        "{:?}",
        response.body
    );
    assert!(response.body["error"]["details"].is_null());
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root canonicalizes")
}

fn current_dir_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct CurrentDirGuard {
    original: PathBuf,
}

impl CurrentDirGuard {
    fn set(path: PathBuf) -> Self {
        let original = env::current_dir().expect("original cwd");
        env::set_current_dir(path).expect("cwd changes");
        Self { original }
    }
}

impl Drop for CurrentDirGuard {
    fn drop(&mut self) {
        env::set_current_dir(&self.original).expect("cwd restores");
    }
}

fn json_object_keys(value: &serde_json::Value) -> BTreeSet<String> {
    value
        .as_object()
        .expect("json object")
        .keys()
        .cloned()
        .collect()
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

struct SqliteFixture {
    _dir: TempDir,
    path: PathBuf,
}

impl SqliteFixture {
    fn new() -> Self {
        let dir = tempdir().expect("sqlite tempdir is created");
        let path = dir.path().join("pack-demo.db");
        let connection = rusqlite::Connection::open(&path).expect("sqlite opens");
        connection
            .execute_batch(
                "create table process(id int, ipid int, pid int, name text, start_ts int);
                 create table thread(id int, itid int, tid int, name text, start_ts int, end_ts int, ipid int, is_main_thread int);
                 create table callstack(id int, ts int, dur int, callid int, cat text, name text, depth int, parent_id int);
                 create table thread_state(id int, ts int, dur int, cpu int, itid int, tid int, pid int, state text);
                 create table instant(ts int, name text, ref int, wakeup_from int, ref_type text, value real);
                 insert into process(id, ipid, pid, name, start_ts) values (1, 89, 15040, '.tencent.wechat', 0);
                 insert into thread(id, itid, tid, name, start_ts, end_ts, ipid, is_main_thread) values (1, 405, 15040, '.tencent.wechat', 0, 0, 89, 1);
                 insert into callstack(id, ts, dur, callid, cat, name, depth, parent_id) values (1, 1000, 100, 405, 'H', 'HandleLaunchAbility##com.tencent.wechat', 0, null);
                 insert into callstack(id, ts, dur, callid, cat, name, depth, parent_id) values (2, 1300, 1, 405, 'H', 'UIVsyncTask[firstDrawFrame:1]', 0, null);
                 insert into thread_state(id, ts, dur, cpu, itid, tid, pid, state) values (1, 1100, 100, 0, 405, 15040, 15040, 'Sleeping');
                 insert into instant(ts, name, ref, wakeup_from, ref_type, value) values (1150, 'sched_wakeup', 405, 405, 'itid', null);",
            )
            .expect("sqlite fixture is written");
        drop(connection);

        Self { _dir: dir, path }
    }

    fn path(&self) -> String {
        self.path.to_string_lossy().into_owned()
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
