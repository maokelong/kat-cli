use std::{
    fs::{self, File},
    io::Write,
    net::{IpAddr, Ipv4Addr},
    path::{Path, PathBuf},
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
        "ErrorEnvelope",
        "PaginatedEnvelope_DatasetDto",
        "QueryRequest",
        "QueryResponse_DatasetQueryMeta",
        "QueryResponse_DatasourceQueryMeta",
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

    let datasource = kat_datasource::TraceDatasource::from_dataset(&dataset_path)
        .await
        .expect("dataset opens after source files are removed");
    let rows = datasource
        .query_json("select count(*) as trace_count from langfuse_traces")
        .await
        .expect("dataset query succeeds");

    assert_eq!(rows, json!([{ "trace_count": 1 }]));
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
