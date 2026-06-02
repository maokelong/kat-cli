use kat_rs_datasource::{
    DatasetInput, DatasourceQueryRequest, HtraceDatasource, QueryStatus, TraceSource,
};
use std::path::PathBuf;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/traces")
        .join(name)
}

#[tokio::test]
async fn opens_inspects_and_queries_bytrace_fixture() {
    let datasource = HtraceDatasource::new();
    let input = DatasetInput {
        sources: vec![TraceSource {
            path: fixture_path("ut_bytrace_input_full.txt"),
            format_hint: Some("bytrace".to_string()),
            source_name: Some("bytrace_full".to_string()),
        }],
        cache_dir: None,
        required_tables: Vec::new(),
    };

    let handle = datasource.open_dataset(input).await.expect("open dataset");
    let inspection = datasource.inspect(&handle).await.expect("inspect dataset");

    assert_eq!(inspection.source_count, 1);
    assert!(inspection.tables["sched_slice"].available);
    assert_eq!(inspection.tables["sched_slice"].row_count, 16);

    let result = datasource
        .query(
            &handle,
            DatasourceQueryRequest::new("SELECT COUNT(*) AS slices FROM sched_slice"),
        )
        .await
        .expect("query dataset");

    assert_eq!(result.stats.rows_returned, 1);
    assert_eq!(result.rows[0]["slices"], serde_json::json!(16));
    assert_eq!(result.schema_version, "htrace.v1");
    assert_eq!(result.columns[0].name, "slices");
    assert!(!result.dataset_id.is_empty());
    assert!(result.metrics.elapsed_ms < 60_000);
    assert!(result.artifacts.is_empty());
    assert!(result.diagnostics.is_empty());
}

#[tokio::test]
async fn empty_query_returns_empty_result_status() {
    let datasource = HtraceDatasource::new();
    let input = DatasetInput {
        sources: vec![TraceSource {
            path: fixture_path("ut_bytrace_input_full.txt"),
            format_hint: None,
            source_name: None,
        }],
        cache_dir: None,
        required_tables: Vec::new(),
    };

    let handle = datasource.open_dataset(input).await.expect("open dataset");
    let result = datasource
        .query(
            &handle,
            DatasourceQueryRequest::new("SELECT * FROM sched_slice WHERE cpu = 9999"),
        )
        .await
        .expect("query dataset");

    assert_eq!(result.status, QueryStatus::EmptyResult);
    assert_eq!(result.stats.rows_returned, 0);
}

#[tokio::test]
async fn scheduler_only_required_tables_preserve_sched_slice_count() {
    let datasource = HtraceDatasource::new();
    let input = DatasetInput {
        sources: vec![TraceSource {
            path: fixture_path("ut_bytrace_input_full.txt"),
            format_hint: Some("bytrace".to_string()),
            source_name: None,
        }],
        cache_dir: None,
        required_tables: vec!["sched_slice".to_string()],
    };

    let handle = datasource.open_dataset(input).await.expect("open dataset");
    let inspection = datasource.inspect(&handle).await.expect("inspect dataset");
    assert_eq!(inspection.tables["sched_slice"].row_count, 16);
    assert_eq!(inspection.tables["raw_event"].row_count, 0);

    let result = datasource
        .query(
            &handle,
            DatasourceQueryRequest::new("SELECT COUNT(*) AS slices FROM sched_slice"),
        )
        .await
        .expect("query dataset");

    assert_eq!(result.stats.rows_returned, 1);
    assert_eq!(result.rows[0]["slices"], serde_json::json!(16));
    for phase in [
        "parser.file_read",
        "parser.bytrace.parse_lines",
        "parser.bytrace.finish_intervals",
        "parser.build_record_batches",
    ] {
        assert!(
            result.metrics.phase_elapsed_ms.contains_key(phase),
            "missing parser phase metric {phase}"
        );
    }
}
