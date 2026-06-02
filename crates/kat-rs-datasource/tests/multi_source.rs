use kat_rs_datasource::{DatasetInput, DatasourceQueryRequest, HtraceDatasource, TraceSource};
use std::path::PathBuf;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/traces")
        .join(name)
}

#[tokio::test]
async fn query_unions_sched_slice_across_sources() {
    let datasource = HtraceDatasource::new();
    let handle = datasource
        .open_dataset(DatasetInput {
            sources: vec![
                TraceSource {
                    path: fixture_path("ut_bytrace_input_full.txt"),
                    format_hint: None,
                    source_name: Some("full".to_string()),
                },
                TraceSource {
                    path: fixture_path("ut_bytrace_input_thread.txt"),
                    format_hint: None,
                    source_name: Some("thread".to_string()),
                },
            ],
            cache_dir: None,
            required_tables: Vec::new(),
        })
        .await
        .expect("open multi-source dataset");

    let result = datasource
        .query(
            &handle,
            DatasourceQueryRequest::new("SELECT COUNT(*) AS slices FROM sched_slice"),
        )
        .await
        .expect("query multi-source dataset");

    assert_eq!(result.rows[0]["slices"], serde_json::json!(31));
}
