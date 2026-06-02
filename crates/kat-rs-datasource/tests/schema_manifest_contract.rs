use kat_rs_datasource::{
    load_schema_manifest, DatasetInput, HtraceDatasource, TableAvailability, TraceSource,
};
use std::path::PathBuf;

#[test]
fn schema_manifest_matches_runtime_sched_slice_schema() {
    let manifest = load_schema_manifest().unwrap();
    let sched_slice = manifest.table("sched_slice").unwrap();

    assert_eq!(sched_slice.columns[0].name, "cpu");
    assert_eq!(sched_slice.columns[0].data_type, "UInt32");
    assert!(!sched_slice.columns[0].nullable);
}

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/traces/ut_bytrace_input_full.txt")
}

#[tokio::test]
async fn datasource_capability_reports_manifest_columns() {
    let datasource = HtraceDatasource::new();
    let handle = datasource
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

    let inspection = datasource.inspect(&handle).await.unwrap();
    let sched_slice = &inspection.tables["sched_slice"];

    assert_eq!(sched_slice.availability, TableAvailability::Available);
    assert!(sched_slice
        .columns
        .iter()
        .any(|column| column.name == "cpu" && column.data_type == "UInt32"));
}
