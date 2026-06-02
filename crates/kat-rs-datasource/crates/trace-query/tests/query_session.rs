use std::path::PathBuf;
use trace_parser::parse_trace_file;
use trace_query::ParsedTraceQuerySession;
use trace_query::QueryRequest;

#[tokio::test]
async fn query_session_runs_multiple_queries_without_reregistering_tables() {
    let parsed = parse_trace_file(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../tests/fixtures/traces/ut_bytrace_input_full.txt"),
    )
    .unwrap();
    let session = ParsedTraceQuerySession::from_parsed_traces(vec![parsed]).unwrap();

    let first = session
        .query(QueryRequest {
            sql: "SELECT COUNT(*) AS slices FROM sched_slice".to_string(),
            max_inline_rows: 100,
        })
        .await
        .unwrap();
    let second = session
        .query(QueryRequest {
            sql: "SELECT COUNT(*) AS threads FROM thread".to_string(),
            max_inline_rows: 100,
        })
        .await
        .unwrap();

    assert_eq!(first.rows[0]["slices"], 16);
    assert_eq!(second.rows[0]["threads"], 7);
}
