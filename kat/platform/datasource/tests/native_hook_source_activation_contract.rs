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
use native_hook_fixture::profiler_section;

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

use proto::kat::hitrace::ProfilerPluginData;

#[tokio::test]
async fn formal_import_publishes_full_native_hook_topology_through_datafusion() {
    use native_hook_fixture::{full_native_hook_batches, full_native_hook_config};

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
            profiler_envelope("hookdaemon_config", 23, 7, config.encode_to_vec()),
        ]),
    )
    .expect("full typed OHOSPROF fixture is written");

    let imported = kat_datasource::import_hitrace(
        &source,
        DatasetWriteTarget::write_to_empty(&dataset),
        |_| Ok(()),
    )
    .expect("formal Hitrace import publishes full Native Hook topology");
    assert!(imported.unsupported_plugins().is_empty());
    assert_eq!(
        kat_datasource::inspect_dataset(&dataset)
            .expect("full formal Dataset is inspectable")
            .tables()
            .iter()
            .map(|table| table.name().to_owned())
            .collect::<BTreeSet<_>>(),
        full_formal_table_names()
    );

    let context = register_resolved_dataset(&dataset)
        .await
        .expect("formal Dataset resolves and registers in DataFusion");
    assert_eq!(
        query_json(
            &context,
            "with roots as ( \
               select occurrence.envelope_name, root._kat_row_id as root_id \
               from profiler_payload_occurrence occurrence \
               join batch_native_hook_data root \
                 on root._kat_parent_row_id = occurrence._kat_row_id \
             ), events as ( \
               select _kat_row_id as event_id, _kat_parent_row_id as root_id, \
                      _kat_repeated_index \
               from batch_native_hook_data_events \
             ), variants as ( \
               select _kat_row_id as variant_id, _kat_parent_row_id as event_id, pid \
               from batch_native_hook_data_events_trace_free_event \
             ), frames as ( \
               select _kat_parent_row_id as variant_id, \
                      _kat_repeated_index as frame_index, ip, symbol_name \
               from batch_native_hook_data_events_trace_free_event_frame_info \
             ) \
             select roots.envelope_name, roots.root_id, events._kat_repeated_index, \
                    variants.pid, frames.frame_index, frames.ip, frames.symbol_name \
             from roots \
             join events on events.root_id = roots.root_id \
             join variants on variants.event_id = events.event_id \
             join frames on frames.variant_id = variants.variant_id \
             order by frames.frame_index",
        )
        .await,
        json!([
            {
                "envelope_name": "hookdaemon",
                "root_id": 1,
                "_kat_repeated_index": 6,
                "pid": 2500,
                "frame_index": 0,
                "ip": 10_070,
                "symbol_name": "symbol-70",
            },
            {
                "envelope_name": "hookdaemon",
                "root_id": 1,
                "_kat_repeated_index": 6,
                "pid": 2500,
                "frame_index": 1,
                "ip": 10_071,
                "symbol_name": "symbol-71",
            },
        ])
    );
    assert_eq!(
        query_json(
            &context,
            "select event.trace_type as enum_number, definition.enum_symbol \
                 from batch_native_hook_data_events_trace_free_event event \
                 left join protobuf_enum_symbol definition \
                   on definition.origin_table = \
                        'batch_native_hook_data_events_trace_free_event' \
                  and definition.origin_field_path = 'trace_type' \
                  and definition.enum_number = event.trace_type",
        )
        .await,
        json!([{"enum_number": 99, "enum_symbol": null}])
    );
    assert_eq!(
        query_json(
            &context,
            "select envelope_name, clock_id from profiler_payload_occurrence \
                 order by _kat_row_id",
        )
        .await,
        json!([
            {"envelope_name": "nativehook", "clock_id": 7},
            {"envelope_name": "hookdaemon", "clock_id": 7},
            {"envelope_name": "hookdaemon_config", "clock_id": 7},
        ])
    );
}

fn full_formal_table_names() -> BTreeSet<String> {
    [
        "batch_native_hook_data",
        "batch_native_hook_data_events",
        "batch_native_hook_data_events_alloc_event",
        "batch_native_hook_data_events_alloc_event_frame_info",
        "batch_native_hook_data_events_free_event",
        "batch_native_hook_data_events_free_event_frame_info",
        "batch_native_hook_data_events_mmap_event",
        "batch_native_hook_data_events_mmap_event_frame_info",
        "batch_native_hook_data_events_munmap_event",
        "batch_native_hook_data_events_munmap_event_frame_info",
        "batch_native_hook_data_events_tag_event",
        "batch_native_hook_data_events_file_path",
        "batch_native_hook_data_events_symbol_name",
        "batch_native_hook_data_events_thread_name_map",
        "batch_native_hook_data_events_maps_info",
        "batch_native_hook_data_events_symbol_tab",
        "batch_native_hook_data_events_frame_map",
        "batch_native_hook_data_events_stack_map",
        "batch_native_hook_data_events_stack_map_frame_map_id",
        "batch_native_hook_data_events_stack_map_ip",
        "batch_native_hook_data_events_statistics_event",
        "batch_native_hook_data_events_trace_alloc_event",
        "batch_native_hook_data_events_trace_alloc_event_frame_info",
        "batch_native_hook_data_events_trace_free_event",
        "batch_native_hook_data_events_trace_free_event_frame_info",
        "native_hook_config",
        "native_hook_config_expand_pids",
        "native_hook_config_restrace_tag",
        "profiler_payload_occurrence",
        "protobuf_enum_symbol",
        "clock_domain",
        "clock_snapshot",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
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
