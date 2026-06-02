use kat_rs_datasource::{
    DatasetInput, DatasourceQueryRequest, DatasourceService, HtraceDatasource, TraceSource,
};
use std::path::PathBuf;

fn bytrace_input() -> DatasetInput {
    DatasetInput {
        sources: vec![TraceSource {
            path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/traces/ut_bytrace_input_full.txt"),
            format_hint: None,
            source_name: None,
        }],
        cache_dir: None,
        required_tables: Vec::new(),
    }
}

#[tokio::test]
async fn service_opens_inspects_and_queries_through_port() {
    let service = DatasourceService::new(HtraceDatasource::new());
    let handle = service.open_dataset(bytrace_input()).await.unwrap();

    let inspection = service.inspect(&handle).await.unwrap();
    assert_eq!(inspection.source_count, 1);
    assert_eq!(inspection.tables.get("sched_slice").unwrap().row_count, 16);

    let result = service
        .query(
            &handle,
            DatasourceQueryRequest::new("SELECT COUNT(*) AS slices FROM sched_slice"),
        )
        .await
        .unwrap();
    assert_eq!(result.rows[0]["slices"], 16);
}
