use std::{fs, path::Path, sync::Arc};

use arrow_array::{Int32Array, RecordBatch};
use arrow_schema::{DataType, Field, Schema};
use kat_datasource::DatasetWriteTarget;
use prost::Message;
use tempfile::tempdir;

#[allow(dead_code)]
#[path = "native_hook_source_contract/fixture.rs"]
mod native_hook_fixture;
use native_hook_fixture::profiler_section;

#[allow(dead_code)]
#[path = "../src/dataset_writer.rs"]
mod dataset_writer;
#[path = "../src/table_name.rs"]
mod table_name;

pub(crate) use table_name::valid_table_name;

#[allow(dead_code)]
mod proto {
    pub mod kat {
        pub mod hitrace {
            include!(concat!(env!("OUT_DIR"), "/kat.hitrace.rs"));
        }

        pub mod native_hook {
            include!(concat!(env!("OUT_DIR"), "/kat.native_hook.rs"));
        }
    }
}

use proto::kat::{
    hitrace::ProfilerPluginData,
    native_hook::{BatchNativeHookData, NativeHookConfig, NativeHookData},
};

#[test]
fn malformed_bound_native_hook_payload_fails_before_overwrite_target_mutation() {
    assert_pre_begin_rejection_preserves_target(
        "malformed-bound-payload",
        vec![profiler_envelope("nativehook", vec![0x80], 7)],
        "failed to decode nativehook payload",
    );
}

#[test]
fn native_hook_clock_mismatch_fails_before_overwrite_target_mutation() {
    let config = NativeHookConfig {
        clock: "boot".to_owned(),
        ..Default::default()
    };
    let batch = BatchNativeHookData {
        // 只要存在 event，即使未选择 oneof 成员，也必须执行时钟准入。
        events: vec![NativeHookData::default()],
    };

    assert_pre_begin_rejection_preserves_target(
        "native-hook-clock-mismatch",
        vec![
            profiler_envelope("nativehook_config", config.encode_to_vec(), 7),
            profiler_envelope("nativehook", batch.encode_to_vec(), 1),
        ],
        "expects profiler envelope clock_id 7, but observed 1",
    );
}

#[test]
fn overwrite_validation_failure_invalidates_the_old_marker() {
    let root = tempdir().expect("temporary directory");
    let dataset = root.path().join("dataset");
    publish_resolvable_old_dataset(&dataset);
    assert!(
        kat_datasource::resolve_dataset(&dataset).is_ok(),
        "the overwrite target starts as a valid published Dataset"
    );
    let mut writer = dataset_writer::DatasetWriter::begin(
        dataset_writer::DatasetWriteTarget::permanently_replace_all_contents(&dataset),
    )
    .expect("authorized overwrite begins and invalidates the old marker");
    let mut table = writer
        .begin_table("facts", int_schema())
        .expect("table writer opens");
    table.write(&int_batch(42)).expect("valid batch is written");
    table.finish().expect("valid table closes");
    fs::write(
        dataset.join("tables/facts.parquet"),
        b"corrupted after close",
    )
    .expect("closed table can be corrupted before Dataset validation");

    writer
        .finish()
        .expect_err("Dataset validation rejects corrupted Parquet");

    assert_unpublished_dataset_is_rejected(&dataset);
}

#[test]
fn dataset_marker_failure_after_begin_never_publishes_a_valid_marker() {
    let root = tempdir().expect("temporary directory");
    let dataset = root.path().join("dataset");
    let writer = dataset_writer::DatasetWriter::begin(
        dataset_writer::DatasetWriteTarget::write_to_empty(&dataset),
    )
    .expect("Dataset writer begins");
    fs::create_dir(dataset.join(".kat-dataset"))
        .expect("a directory blocks marker file publication");

    writer
        .finish()
        .expect_err("marker publication rejects an existing directory");

    assert_unpublished_dataset_is_rejected(&dataset);
}

#[test]
fn staged_publication_keeps_old_dataset_until_tables_are_ready_then_removes_staging() {
    let root = tempdir().expect("temporary directory");
    let dataset = root.path().join("dataset");
    let old_table = publish_old_dataset_with_evidence(&dataset);

    let publication = dataset_writer::DatasetPublication::stage(
        dataset_writer::DatasetWriteTarget::permanently_replace_all_contents(&dataset),
    )
    .expect("staging starts without replacing the old Dataset");
    let staging_entries = fs::read_dir(&dataset)
        .expect("target remains readable")
        .map(|entry| entry.expect("target entry reads").file_name())
        .filter(|name| name.to_string_lossy().starts_with(".kat-staging-"))
        .collect::<Vec<_>>();
    assert_eq!(staging_entries.len(), 1, "one target-local staging exists");
    assert_old_dataset_unchanged(&dataset, &old_table);

    let tables = publication.table_factory();
    let mut table = tables
        .begin_table("facts", int_schema())
        .expect("staged table opens");
    table.write(&int_batch(42)).expect("staged batch writes");
    table.finish().expect("staged table closes");
    publication.publish().expect("staged Dataset publishes");

    assert!(kat_datasource::resolve_dataset(&dataset).is_ok());
    assert!(!dataset.join("tables/old_facts.parquet").exists());
    assert!(dataset.join("tables/facts.parquet").is_file());
    assert!(
        fs::read_dir(&dataset)
            .expect("published target reads")
            .all(|entry| !entry
                .expect("published target entry reads")
                .file_name()
                .to_string_lossy()
                .starts_with(".kat-staging-")),
        "successful publication removes its exact staging directory"
    );
}

#[test]
fn dropping_unpublished_candidate_removes_only_its_staging_directory() {
    let root = tempdir().expect("temporary directory");
    let dataset = root.path().join("dataset");
    let publication = dataset_writer::DatasetPublication::stage(
        dataset_writer::DatasetWriteTarget::write_to_empty(&dataset),
    )
    .expect("staging starts");
    assert_eq!(
        fs::read_dir(&dataset).expect("target reads").count(),
        1,
        "target contains only this publication's staging directory"
    );

    drop(publication);

    assert!(
        dataset.is_dir(),
        "Drop does not remove the canonical target"
    );
    assert_eq!(
        fs::read_dir(&dataset).expect("target still reads").count(),
        0,
        "Drop removes exactly its staging directory"
    );
}

fn assert_pre_begin_rejection_preserves_target(
    fixture_name: &str,
    envelopes: Vec<ProfilerPluginData>,
    expected_error: &str,
) {
    let root = tempdir().expect("temporary directory");
    let source = root.path().join(format!("{fixture_name}.htrace"));
    let dataset = root.path().join("dataset");
    fs::write(&source, profiler_section(envelopes)).expect("Hitrace fixture is written");
    let old_table = publish_old_dataset_with_evidence(&dataset);

    let error = kat_datasource::import_hitrace(
        &source,
        DatasetWriteTarget::permanently_replace_all_contents(&dataset),
        |_| Ok(()),
    )
    .expect_err("invalid bound Native Hook content rejects formal import");
    let message = format!("{error:?}");
    assert!(
        message.contains(expected_error),
        "expected {expected_error:?} in error chain, got {message}"
    );

    assert_old_dataset_unchanged(&dataset, &old_table);
}

fn publish_old_dataset_with_evidence(dataset: &Path) -> Vec<u8> {
    publish_resolvable_old_dataset(dataset);
    fs::write(dataset.join("sentinel.bin"), [0, 1, 0xff, 2]).expect("binary sentinel is written");
    fs::read(dataset.join("tables/old_facts.parquet")).expect("old Dataset table is readable")
}

fn assert_old_dataset_unchanged(dataset: &Path, old_table: &[u8]) {
    assert!(kat_datasource::resolve_dataset(dataset).is_ok());
    assert_eq!(
        fs::read(dataset.join("sentinel.bin")).expect("binary sentinel remains readable"),
        [0, 1, 0xff, 2]
    );
    assert_eq!(
        fs::read(dataset.join("tables/old_facts.parquet"))
            .expect("old Dataset table remains readable"),
        old_table
    );
}

fn assert_unpublished_dataset_is_rejected(dataset: &Path) {
    assert!(
        !dataset.join(".kat-dataset").is_file(),
        "a post-begin failure must not leave a valid Dataset marker"
    );
    assert!(
        kat_datasource::resolve_dataset(dataset).is_err(),
        "the formal resolver must reject every post-begin partial Dataset"
    );
}

fn int_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![Field::new(
        "value",
        DataType::Int32,
        false,
    )]))
}

fn int_batch(value: i32) -> RecordBatch {
    RecordBatch::try_new(int_schema(), vec![Arc::new(Int32Array::from(vec![value]))])
        .expect("integer batch is valid")
}

fn publish_resolvable_old_dataset(dataset: &Path) {
    let mut writer = dataset_writer::DatasetWriter::begin(
        dataset_writer::DatasetWriteTarget::write_to_empty(dataset),
    )
    .expect("old Dataset writer begins");
    let mut table = writer
        .begin_table("old_facts", int_schema())
        .expect("old table writer opens");
    table.write(&int_batch(7)).expect("old batch is written");
    table.finish().expect("old table closes");
    writer.finish().expect("old Dataset marker is published");
}

fn profiler_envelope(name: &str, data: Vec<u8>, clock_id: i32) -> ProfilerPluginData {
    ProfilerPluginData {
        name: name.to_owned(),
        status: u32::from(!name.ends_with("_config")),
        data,
        clock_id,
        tv_sec: 10,
        tv_nsec: 20,
        version: "1.0".to_owned(),
        sample_interval: 10,
    }
}
