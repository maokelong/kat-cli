use serde_json::json;
use trace_model::{ParsedTrace, SchedSliceRow, TraceTableBuilder};
use trace_query::query_parsed_trace;
use trace_query::QueryRequest;

#[tokio::test]
async fn query_sched_slice_count() {
    let mut builder = TraceTableBuilder::default();
    builder.push_sched_slice(SchedSliceRow {
        cpu: 0,
        utid: 1,
        ts: 100,
        dur: Some(50),
        priority: Some(120),
        end_state: Some("sleeping".to_string()),
    });

    let tables = builder
        .finish(
            "test".to_string(),
            Some(100),
            Some(150),
            "boottime".to_string(),
        )
        .unwrap();
    let parsed = ParsedTrace {
        trace_id: "test".to_string(),
        start_ts: Some(100),
        end_ts: Some(150),
        clock_domain: "boottime".to_string(),
        tables,
    };

    let result = query_parsed_trace(
        &parsed,
        QueryRequest {
            sql: "SELECT cpu, COUNT(*) AS slices FROM sched_slice GROUP BY cpu".to_string(),
            max_inline_rows: 10,
        },
    )
    .await
    .unwrap();

    assert_eq!(result.status, "ok");
    assert_eq!(result.stats.rows_returned, 1);
    assert_eq!(result.rows[0]["cpu"], json!(0));
    assert_eq!(result.rows[0]["slices"], json!(1));
}
