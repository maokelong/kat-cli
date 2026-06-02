use kat_rs_datasource::{
    run_golden_suite, DatasetInput, DatasourceService, HtraceDatasource, TraceSource,
};
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[tokio::test]
async fn bytrace_golden_suite_runs_all_sql_cases() {
    let service = DatasourceService::new(HtraceDatasource::new());
    let handle = service
        .open_dataset(DatasetInput {
            sources: vec![TraceSource {
                path: repo_root().join("tests/fixtures/traces/ut_bytrace_input_full.txt"),
                format_hint: None,
                source_name: None,
            }],
            cache_dir: None,
            required_tables: Vec::new(),
        })
        .await
        .unwrap();

    let report = run_golden_suite(
        &service,
        &handle,
        &repo_root().join("tests/golden/bytrace_full"),
    )
    .await
    .unwrap();

    assert_eq!(report.status, "ok");
    assert!(report
        .cases
        .iter()
        .any(|case| case.name == "sched_slice_count"));
    assert!(report.cases.iter().any(|case| case.name == "inspect"));
    assert!(report.cases.iter().any(|case| case.name == "missing_table"));
}
