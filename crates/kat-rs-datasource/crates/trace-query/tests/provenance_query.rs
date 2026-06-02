use std::path::PathBuf;
use trace_parser::parse_trace_file;
use trace_query::QueryRequest;
use trace_query::{ParsedTraceQuerySession, ParsedTraceSource};

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../tests/fixtures/traces")
        .join(name)
}

#[tokio::test]
async fn query_can_group_rows_by_source_id() {
    let full = parse_trace_file(&fixture_path("ut_bytrace_input_full.txt")).unwrap();
    let thread = parse_trace_file(&fixture_path("ut_bytrace_input_thread.txt")).unwrap();
    let session = ParsedTraceQuerySession::from_parsed_trace_sources(vec![
        ParsedTraceSource {
            dataset_id: "dataset:test".to_string(),
            source_id: "full".to_string(),
            trace_id: full.trace_id.clone(),
            parsed: full,
        },
        ParsedTraceSource {
            dataset_id: "dataset:test".to_string(),
            source_id: "thread".to_string(),
            trace_id: thread.trace_id.clone(),
            parsed: thread,
        },
    ])
    .unwrap();

    let result = session
        .query(QueryRequest {
            sql: "SELECT source_id, COUNT(*) AS slices FROM sched_slice GROUP BY source_id ORDER BY source_id".to_string(),
            max_inline_rows: 100,
        })
        .await
        .unwrap();

    assert_eq!(result.rows[0]["source_id"], "full");
    assert_eq!(result.rows[0]["slices"], 16);
    assert_eq!(result.rows[1]["source_id"], "thread");
    assert_eq!(result.rows[1]["slices"], 15);
}
