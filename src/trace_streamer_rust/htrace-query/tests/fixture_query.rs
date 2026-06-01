use htrace_core::QueryRequest;
use htrace_parser_harmony::parse_trace_file;
use htrace_query::query_parsed_trace;
use serde_json::json;
use std::path::PathBuf;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/resource")
        .join(name)
}

#[tokio::test]
async fn query_bytrace_fixture_from_test_resource() {
    let trace = fixture_path("ut_bytrace_input_full.txt");
    assert!(
        trace.exists(),
        "missing repository fixture {}",
        trace.display()
    );

    let parsed = parse_trace_file(&trace).expect("parse bytrace fixture");
    let result = query_parsed_trace(
        &parsed,
        QueryRequest {
            sql: "SELECT COUNT(*) AS slices FROM sched_slice".to_string(),
            max_inline_rows: 10,
        },
    )
    .await
    .expect("query bytrace fixture");

    assert_eq!(result.status, "ok");
    assert_eq!(result.stats.rows_returned, 1);
    assert_eq!(result.rows[0]["slices"], json!(16));
}
