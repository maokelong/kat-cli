use std::{collections::BTreeSet, fs, path::Path};

use arrow_array::RecordBatch;
use arrow_json::writer::{JsonArray, WriterBuilder};
use datafusion::prelude::{ParquetReadOptions, SessionContext};
use kat_datasource::DatasetWriteTarget;
use prost::Message;
use serde_json::{Value, json};
use tempfile::tempdir;
use url::Url;

#[path = "native_hook_source_contract/fixture.rs"]
mod native_hook_fixture;
use native_hook_fixture::{
    full_native_hook_batches, full_native_hook_config, full_native_hook_table_names,
    profiler_section,
};

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
    native_hook::{BatchNativeHookData, NativeHookConfig},
};

#[tokio::test]
async fn formal_import_claims_all_routes_and_publishes_native_hook_source_tables() {
    let root = tempdir().expect("temporary import directory is created");
    let source = root.path().join("full-native-hook-topology.htrace");
    let dataset = root.path().join("dataset");
    let (first_batch, second_batch) = full_native_hook_batches();
    let config = full_native_hook_config("boot");
    fs::write(
        &source,
        profiler_section([
            profiler_envelope("nativehook", 21, 7, first_batch.encode_to_vec()),
            profiler_envelope("hookdaemon", 22, 7, second_batch.encode_to_vec()),
            profiler_envelope("nativehook_config", 23, 7, config.clone().encode_to_vec()),
            profiler_envelope("hookdaemon_config", 24, 7, config.encode_to_vec()),
            profiler_envelope("nativehook-preview", 25, 7, vec![0xff]),
            profiler_envelope("future-plugin", 26, 7, vec![0x80]),
        ]),
    )
    .expect("full typed OHOSPROF fixture is written");

    let imported = kat_datasource::import_hitrace(
        &source,
        DatasetWriteTarget::write_to_empty(&dataset),
        |_| Ok(()),
    )
    .expect("formal Hitrace import publishes Native Hook Source tables");
    assert_eq!(
        imported.unsupported_plugins(),
        ["future-plugin", "nativehook-preview"]
    );
    assert_eq!(dataset_table_names(&dataset), full_formal_table_names());

    let context = register_resolved_dataset(&dataset)
        .await
        .expect("formal Dataset resolves and registers in DataFusion");
    assert_eq!(
        query_json(
            &context,
            "select occurrence._kat_row_id as occurrence_id, occurrence.envelope_name, \
                    root._kat_row_id as root_id, root._kat_parent_row_id as root_parent \
             from profiler_payload_occurrence occurrence \
             join batch_native_hook_data root \
               on root._kat_parent_row_id = occurrence._kat_row_id \
             order by occurrence._kat_row_id",
        )
        .await,
        json!([
            {
                "occurrence_id": 0,
                "envelope_name": "nativehook",
                "root_id": 0,
                "root_parent": 0,
            },
            {
                "occurrence_id": 1,
                "envelope_name": "hookdaemon",
                "root_id": 1,
                "root_parent": 1,
            },
        ])
    );
    assert_eq!(
        query_json(
            &context,
            "select occurrence.envelope_name, config._kat_parent_row_id, config.clock \
             from profiler_payload_occurrence occurrence \
             join native_hook_config config \
               on config._kat_parent_row_id = occurrence._kat_row_id \
             order by occurrence._kat_row_id",
        )
        .await,
        json!([
            {
                "envelope_name": "nativehook_config",
                "_kat_parent_row_id": 2,
                "clock": "boot",
            },
            {
                "envelope_name": "hookdaemon_config",
                "_kat_parent_row_id": 3,
                "clock": "boot",
            },
        ])
    );
}

#[tokio::test]
async fn formal_import_keeps_occurrence_and_root_rows_for_empty_default_payloads() {
    let root = tempdir().expect("temporary import directory is created");
    let source = root.path().join("empty-native-hook-roots.htrace");
    let dataset = root.path().join("dataset");
    fs::write(
        &source,
        profiler_section([
            profiler_envelope(
                "nativehook",
                31,
                0,
                BatchNativeHookData::default().encode_to_vec(),
            ),
            profiler_envelope(
                "nativehook_config",
                32,
                0,
                NativeHookConfig::default().encode_to_vec(),
            ),
        ]),
    )
    .expect("empty/default OHOSPROF fixture is written");

    kat_datasource::import_hitrace(
        &source,
        DatasetWriteTarget::write_to_empty(&dataset),
        |_| Ok(()),
    )
    .expect("formal import preserves empty/default roots");

    let context = register_resolved_dataset(&dataset)
        .await
        .expect("formal Dataset resolves and registers in DataFusion");
    assert_eq!(
        query_json(
            &context,
            "select occurrence.envelope_name, data._kat_row_id as data_root, \
                    config._kat_row_id as config_root \
             from profiler_payload_occurrence occurrence \
             left join batch_native_hook_data data \
               on data._kat_parent_row_id = occurrence._kat_row_id \
             left join native_hook_config config \
               on config._kat_parent_row_id = occurrence._kat_row_id \
             order by occurrence._kat_row_id",
        )
        .await,
        json!([
            {"envelope_name": "nativehook", "data_root": 0, "config_root": null},
            {
                "envelope_name": "nativehook_config",
                "data_root": null,
                "config_root": 0,
            },
        ])
    );
}

fn dataset_table_names(dataset: &Path) -> BTreeSet<String> {
    kat_datasource::inspect_dataset(dataset)
        .expect("formal Dataset is inspectable")
        .tables()
        .iter()
        .map(|table| table.name().to_owned())
        .collect()
}

fn full_formal_table_names() -> BTreeSet<String> {
    let mut names = full_native_hook_table_names()
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    names.extend(["clock_domain".to_owned(), "clock_snapshot".to_owned()]);
    names
}

fn profiler_envelope(name: &str, status: u32, clock_id: i32, data: Vec<u8>) -> ProfilerPluginData {
    ProfilerPluginData {
        name: name.to_owned(),
        status,
        data,
        clock_id,
        tv_sec: 100 + u64::from(status),
        tv_nsec: 200 + u64::from(status),
        version: format!("route-{status}"),
        sample_interval: status,
    }
}

async fn register_resolved_dataset(dataset_path: &Path) -> anyhow::Result<SessionContext> {
    let resolved = kat_datasource::resolve_dataset(dataset_path)?;
    let context = SessionContext::new();
    for table in resolved.tables() {
        let url = Url::from_file_path(table.path()).map_err(|()| {
            anyhow::anyhow!(
                "formal table path cannot be converted to a file URL: {}",
                table.path().display()
            )
        })?;
        context
            .register_parquet(table.name(), url.as_str(), ParquetReadOptions::default())
            .await?;
    }
    Ok(context)
}

async fn query_json(context: &SessionContext, sql: &str) -> Value {
    let batches = context
        .sql(sql)
        .await
        .expect("formal Native Hook SQL plans")
        .collect()
        .await
        .expect("formal Native Hook SQL executes");
    record_batches_to_json(&batches)
}

fn record_batches_to_json(batches: &[RecordBatch]) -> Value {
    let batch_refs = batches.iter().collect::<Vec<_>>();
    let mut buffer = Vec::new();
    let mut writer = WriterBuilder::new()
        .with_explicit_nulls(true)
        .build::<_, JsonArray>(&mut buffer);
    writer
        .write_batches(&batch_refs)
        .expect("formal query batches encode as JSON");
    writer.finish().expect("formal JSON writer finishes");
    drop(writer);
    serde_json::from_slice(&buffer).expect("formal query JSON parses")
}
