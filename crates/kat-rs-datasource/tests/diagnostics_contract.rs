use kat_rs_datasource::{DatasetInput, DatasourceQueryRequest, HtraceDatasource, TraceSource};
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[tokio::test]
async fn malformed_bytrace_input_is_queryable_without_panic() {
    let datasource = HtraceDatasource::new();
    let handle = datasource
        .open_dataset(DatasetInput {
            sources: vec![TraceSource {
                path: repo_root().join("tests/malformed/malformed_bytrace.txt"),
                format_hint: Some("bytrace".to_string()),
                source_name: Some("malformed".to_string()),
            }],
            cache_dir: None,
            required_tables: Vec::new(),
        })
        .await
        .unwrap();

    let result = datasource
        .query(
            &handle,
            DatasourceQueryRequest::new(
                "SELECT event_name, COUNT(*) AS rows \
                 FROM raw_event \
                 GROUP BY event_name \
                 ORDER BY event_name",
            ),
        )
        .await
        .unwrap();

    assert_eq!(result.rows[0]["event_name"], "malformed_bytrace_line");
    assert_eq!(result.rows[0]["rows"], 1);
}
