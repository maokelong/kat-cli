use kat_rs_datasource::{
    DatasetInput, DatasourceQueryRequest, DatasourceService, HtraceDatasource, TraceSource,
};
use std::path::PathBuf;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/traces")
        .join(name)
}

#[tokio::test]
async fn query_reports_stable_phase_metrics() {
    let service = DatasourceService::new(HtraceDatasource::new());
    let handle = service
        .open_dataset(DatasetInput {
            sources: vec![TraceSource {
                path: fixture_path("ut_bytrace_input_full.txt"),
                format_hint: None,
                source_name: None,
            }],
            cache_dir: None,
            required_tables: Vec::new(),
        })
        .await
        .unwrap();

    let result = service
        .query(
            &handle,
            DatasourceQueryRequest::new("SELECT COUNT(*) AS slices FROM sched_slice"),
        )
        .await
        .unwrap();

    assert!(result.metrics.elapsed_ms <= 60_000);
    assert!(result.metrics.phase_elapsed_ms.contains_key("open_dataset"));
    assert!(result.metrics.phase_elapsed_ms.contains_key("parse_source"));
    for phase in [
        "parser.file_read",
        "parser.unwrap",
        "parser.detect_format",
        "parser.dispatch",
        "parser.bytrace.parse_lines",
        "parser.bytrace.finish_intervals",
        "parser.build_record_batches",
    ] {
        assert!(
            result.metrics.phase_elapsed_ms.contains_key(phase),
            "missing parser phase metric {phase}"
        );
    }
    assert!(result
        .metrics
        .phase_elapsed_ms
        .contains_key("session_lookup"));
    assert!(result
        .metrics
        .phase_elapsed_ms
        .contains_key("session_build"));
    assert!(result
        .metrics
        .phase_elapsed_ms
        .contains_key("query_execute"));
    assert!(result
        .metrics
        .phase_elapsed_ms
        .contains_key("result_serialize"));
    assert_eq!(result.metrics.rows_returned, 1);
    assert!(result.metrics.bytes_inline > 0);
}
