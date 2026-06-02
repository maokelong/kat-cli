use kat_rs_datasource::{DatasetInput, DatasourceQueryRequest, HtraceDatasource, TraceSource};
use std::path::PathBuf;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/traces")
        .join(name)
}

#[tokio::test]
async fn query_results_include_source_and_trace_provenance() {
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
        .unwrap();

    let result = datasource
        .query(
            &handle,
            DatasourceQueryRequest::new(
                "SELECT source_id, trace_id, COUNT(*) AS slices \
                 FROM sched_slice \
                 GROUP BY source_id, trace_id \
                 ORDER BY source_id",
            ),
        )
        .await
        .unwrap();

    assert_eq!(result.rows.len(), 2);
    assert_eq!(result.rows[0]["source_id"], "full");
    assert_eq!(result.rows[0]["trace_id"], "bytrace:b1aa5f38d23875c3");
    assert_eq!(result.rows[0]["slices"], 16);
    assert_eq!(result.rows[1]["source_id"], "thread");
    assert_eq!(result.rows[1]["trace_id"], "bytrace:2e82f386c0f9f639");
    assert_eq!(result.rows[1]["slices"], 15);
}
