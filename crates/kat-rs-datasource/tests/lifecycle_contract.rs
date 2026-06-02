use kat_rs_datasource::{DatasetInput, DatasourceQueryRequest, HtraceDatasource, TraceSource};
use std::path::PathBuf;

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/traces/ut_bytrace_input_full.txt")
}

fn bytrace_input() -> DatasetInput {
    DatasetInput {
        sources: vec![TraceSource {
            path: fixture_path(),
            format_hint: None,
            source_name: Some("full".to_string()),
        }],
        cache_dir: None,
        required_tables: Vec::new(),
    }
}

#[tokio::test]
async fn datasource_lists_and_closes_open_datasets() {
    let datasource = HtraceDatasource::new();
    let handle = datasource.open_dataset(bytrace_input()).await.unwrap();

    let open = datasource.list_datasets().await.unwrap();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].dataset_id, handle.dataset_id);
    assert_eq!(open[0].source_count, 1);
    assert_eq!(open[0].source_ids, vec!["full"]);

    datasource.close_dataset(&handle).await.unwrap();

    let open = datasource.list_datasets().await.unwrap();
    assert!(open.is_empty());
    let err = datasource
        .query(
            &handle,
            DatasourceQueryRequest::new("SELECT COUNT(*) FROM sched_slice"),
        )
        .await
        .unwrap_err();
    assert!(err.to_string().contains("unknown dataset handle"));
}
