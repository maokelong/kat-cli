use kat_rs_datasource::{
    DatasetInput, DatasourceQueryRequest, HtraceDatasource, QueryStatus, TraceSource,
};
use std::path::PathBuf;

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/traces/ut_bytrace_input_full.txt")
}

async fn open_bytrace() -> (HtraceDatasource, kat_rs_datasource::DatasetHandle) {
    let datasource = HtraceDatasource::new();
    let handle = datasource
        .open_dataset(DatasetInput {
            sources: vec![TraceSource {
                path: fixture_path(),
                format_hint: None,
                source_name: None,
            }],
            cache_dir: None,
            required_tables: Vec::new(),
        })
        .await
        .unwrap();
    (datasource, handle)
}

#[tokio::test]
async fn query_returns_result_too_large_when_inline_bytes_limit_is_exceeded() {
    let (datasource, handle) = open_bytrace().await;
    let mut request = DatasourceQueryRequest::new("SELECT * FROM raw_event");
    request.limits.max_rows_inline = 10_000;
    request.limits.max_result_bytes_inline = 8;

    let result = datasource.query(&handle, request).await.unwrap();

    assert_eq!(result.status, QueryStatus::ResultTooLarge);
    assert!(result.rows.is_empty());
    assert!(result
        .diagnostics
        .iter()
        .any(|item| item.contains("max_result_bytes_inline")));
}
