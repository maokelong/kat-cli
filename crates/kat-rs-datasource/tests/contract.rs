use kat_rs_datasource::{
    DatasetInput, DatasourceQueryRequest, QueryLimits, QueryOutputMode, QueryParam, TraceSource,
};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[test]
fn dataset_input_keeps_sources_and_cache_dir() {
    let input = DatasetInput {
        sources: vec![TraceSource {
            path: PathBuf::from("trace.txt"),
            format_hint: Some("bytrace".to_string()),
            source_name: Some("boot".to_string()),
        }],
        cache_dir: Some(PathBuf::from("target/cache")),
        required_tables: Vec::new(),
    };

    assert_eq!(input.sources.len(), 1);
    assert_eq!(input.sources[0].format_hint.as_deref(), Some("bytrace"));
    assert_eq!(
        input.cache_dir.as_deref(),
        Some(PathBuf::from("target/cache").as_path())
    );
}

#[test]
fn query_request_defaults_are_bounded() {
    let request = DatasourceQueryRequest::new("SELECT 1");

    assert_eq!(request.sql, "SELECT 1");
    assert_eq!(request.limits.timeout_ms, 30_000);
    assert_eq!(request.limits.max_rows_inline, 10_000);
    assert_eq!(request.limits.max_result_bytes_inline, 1_048_576);
}

#[test]
fn query_request_accepts_typed_params() {
    let mut params = BTreeMap::new();
    params.insert("pid".to_string(), QueryParam::I64(42));

    let request = DatasourceQueryRequest {
        sql: "SELECT * FROM thread WHERE tid = $pid".to_string(),
        params,
        limits: QueryLimits::default(),
        output: QueryOutputMode::InlineJson,
        required_tables: vec!["thread".to_string()],
        query_tag: Some("thread_by_pid".to_string()),
    };

    assert!(matches!(request.params["pid"], QueryParam::I64(42)));
    assert_eq!(request.required_tables, vec!["thread"]);
    assert_eq!(request.query_tag.as_deref(), Some("thread_by_pid"));
}
