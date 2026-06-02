use kat_rs_datasource::{DatasetInput, DatasourceQueryRequest, HtraceDatasource, TraceSource};
use std::path::{Path, PathBuf};

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/traces/ut_bytrace_input_full.txt")
}

fn input_with_cache(cache_dir: &Path) -> DatasetInput {
    DatasetInput {
        sources: vec![TraceSource {
            path: fixture_path(),
            format_hint: None,
            source_name: None,
        }],
        cache_dir: Some(cache_dir.to_path_buf()),
        required_tables: Vec::new(),
    }
}

#[tokio::test]
async fn same_input_uses_stable_dataset_cache_key_and_reports_metadata_cache_hit() {
    let cache_dir = tempfile::tempdir().unwrap();
    let datasource = HtraceDatasource::new();

    let first = datasource
        .open_dataset(input_with_cache(cache_dir.path()))
        .await
        .unwrap();
    let second = datasource
        .open_dataset(input_with_cache(cache_dir.path()))
        .await
        .unwrap();

    assert_eq!(first.dataset_id, second.dataset_id);
    assert!(cache_dir.path().join("datasets").exists());

    let result = datasource
        .query(
            &second,
            DatasourceQueryRequest::new("SELECT COUNT(*) AS slices FROM sched_slice"),
        )
        .await
        .unwrap();

    assert!(result.metrics.cache_hit);
}
