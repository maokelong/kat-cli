use kat_rs_datasource::{
    inspect_dataset_for_ui, DatasetInput, DatasourceService, HtraceDatasource, TraceSource,
};
use std::path::PathBuf;

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/traces/ut_bytrace_input_full.txt")
}

#[tokio::test]
async fn ui_inspection_contains_trace_table_and_columns() {
    let service = DatasourceService::new(HtraceDatasource::new());
    let handle = service
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

    let inspection = inspect_dataset_for_ui(&service, &handle).await.unwrap();

    assert_eq!(inspection.trace.trace_id, "bytrace:b1aa5f38d23875c3");
    assert_eq!(inspection.tables["sched_slice"].row_count, 16);
    assert!(inspection.tables["sched_slice"]
        .columns
        .iter()
        .any(|column| column.name == "cpu" && column.data_type == "UInt32"));
}
