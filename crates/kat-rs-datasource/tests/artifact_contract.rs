use kat_rs_datasource::{
    DatasetInput, DatasourceQueryRequest, HtraceDatasource, QueryOutputMode, QueryStatus,
    TraceSource,
};
use std::path::{Path, PathBuf};

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/traces/ut_bytrace_input_full.txt")
}

#[tokio::test]
async fn query_can_write_large_result_as_jsonl_artifact() {
    let cache_dir = tempfile::tempdir().unwrap();
    let datasource = HtraceDatasource::new();
    let handle = datasource
        .open_dataset(DatasetInput {
            sources: vec![TraceSource {
                path: fixture_path(),
                format_hint: None,
                source_name: None,
            }],
            cache_dir: Some(cache_dir.path().to_path_buf()),
            required_tables: Vec::new(),
        })
        .await
        .unwrap();

    let mut request = DatasourceQueryRequest::new("SELECT * FROM raw_event");
    request.output = QueryOutputMode::Artifact;
    request.limits.max_rows_inline = 0;
    request.query_tag = Some("raw_event".to_string());

    let result = datasource.query(&handle, request).await.unwrap();

    assert_eq!(result.status, QueryStatus::Ok);
    assert!(result.rows.is_empty());
    assert_eq!(result.artifacts.len(), 1);
    assert_eq!(result.artifacts[0].format, "jsonl");
    assert_eq!(result.artifacts[0].row_count, result.stats.rows_returned);
    assert!(Path::new(&result.artifacts[0].path).exists());
    assert!(cache_dir.path().join("artifacts").exists());
}
