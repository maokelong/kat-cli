use std::{collections::BTreeSet, fs, path::Path};

use arrow_array::{RecordBatch, StringArray, UInt64Array};
use arrow_json::writer::{JsonArray, WriterBuilder};
use datafusion::prelude::{ParquetReadOptions, SessionContext};
use kat_datasource::DatasetWriteTarget;
use prost::Message;
use serde_json::{Value, json};
use tempfile::tempdir;
use url::Url;

#[path = "native_hook_source_contract/fixture.rs"]
mod native_hook_fixture;
use native_hook_fixture::{batches, profiler_section};

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
async fn formal_import_atomically_publishes_all_four_native_hook_routes() {
    let root = tempdir().expect("temporary import directory is created");
    let source = root.path().join("four-native-hook-routes.htrace");
    let dataset = root.path().join("dataset");
    fs::write(&source, four_route_trace()).expect("typed OHOSPROF fixture is written");

    kat_datasource::import_hitrace(
        &source,
        DatasetWriteTarget::write_to_empty(&dataset),
        |_| Ok(()),
    )
    .expect("formal Hitrace import succeeds");

    let table_names = kat_datasource::inspect_dataset(&dataset)
        .expect("formal Dataset is inspectable")
        .tables()
        .iter()
        .map(|table| table.name().to_owned())
        .collect::<BTreeSet<_>>();
    for required in [
        "profiler_payload_occurrence",
        "batch_native_hook_data",
        "native_hook_config",
    ] {
        assert!(
            table_names.contains(required),
            "formal import must atomically publish {required:?}; actual tables: {table_names:?}"
        );
    }

    let occurrences = batches(&dataset.join("tables/profiler_payload_occurrence.parquet"));
    assert_eq!(
        strings(&occurrences, "envelope_name"),
        [
            "nativehook",
            "hookdaemon",
            "nativehook_config",
            "hookdaemon_config",
        ]
    );
    assert_eq!(u64s(&occurrences, "_kat_row_id"), [0, 1, 2, 3]);

    let data_roots = batches(&dataset.join("tables/batch_native_hook_data.parquet"));
    assert_eq!(u64s(&data_roots, "_kat_row_id"), [0, 1]);
    assert_eq!(u64s(&data_roots, "_kat_parent_row_id"), [0, 1]);

    let config_roots = batches(&dataset.join("tables/native_hook_config.parquet"));
    assert_eq!(u64s(&config_roots, "_kat_row_id"), [0, 1]);
    assert_eq!(u64s(&config_roots, "_kat_parent_row_id"), [2, 3]);

    let context = register_resolved_dataset(&dataset)
        .await
        .expect("four-route formal Dataset resolves in DataFusion");
    assert_eq!(
        query_json(
            &context,
            "select * from profiler_payload_occurrence order by _kat_row_id",
        )
        .await,
        json!([
            {
                "_kat_row_id": 0,
                "envelope_name": "nativehook",
                "status": 11,
                "clock_id": 0,
                "tv_sec": 111,
                "tv_nsec": 211,
                "version": "route-11",
                "sample_interval": 11,
            },
            {
                "_kat_row_id": 1,
                "envelope_name": "hookdaemon",
                "status": 12,
                "clock_id": 1,
                "tv_sec": 112,
                "tv_nsec": 212,
                "version": "route-12",
                "sample_interval": 12,
            },
            {
                "_kat_row_id": 2,
                "envelope_name": "nativehook_config",
                "status": 13,
                "clock_id": 4,
                "tv_sec": 113,
                "tv_nsec": 213,
                "version": "route-13",
                "sample_interval": 13,
            },
            {
                "_kat_row_id": 3,
                "envelope_name": "hookdaemon_config",
                "status": 14,
                "clock_id": 7,
                "tv_sec": 114,
                "tv_nsec": 214,
                "version": "route-14",
                "sample_interval": 14,
            },
        ])
    );
}

#[tokio::test]
async fn formal_import_publishes_full_native_hook_topology_through_datafusion() {
    use native_hook_fixture::{
        full_native_hook_batches, full_native_hook_config, native_hook_frame,
    };

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
            "select sym_table, str_table \
                 from batch_native_hook_data_events_symbol_tab",
        )
        .await,
        json!([{"sym_table": "00ff80", "str_table": "fe007f"}])
    );
    assert_eq!(
        query_json(
            &context,
            "select frame_map.id, frame_map.frame, frame_map.pid \
                 from batch_native_hook_data_events_frame_map frame_map \
                 order by frame_map.id",
        )
        .await,
        json!([
            {"id": 101, "frame": null, "pid": 2000},
            {
                "id": 111,
                "frame": serde_json::to_value(native_hook_frame(50))
                    .expect("Frame serializes as fixture JSON"),
                "pid": 2100,
            },
        ])
    );
    assert_eq!(
        query_json(
            &context,
            "select parent.id, child._kat_repeated_index, child.value \
                 from batch_native_hook_data_events_stack_map parent \
                 join batch_native_hook_data_events_stack_map_ip child \
                   on child._kat_parent_row_id = parent._kat_row_id \
                 order by child._kat_repeated_index",
        )
        .await,
        json!([
            {"id": 121, "_kat_repeated_index": 0, "value": 0x2200},
            {"id": 121, "_kat_repeated_index": 1, "value": 0x2201},
            {"id": 121, "_kat_repeated_index": 2, "value": 0x2202},
        ])
    );
    assert_eq!(
        query_json(
            &context,
            "select origin_table, count(*) as symbol_count \
                 from protobuf_enum_symbol group by origin_table order by origin_table",
        )
        .await,
        json!([
            {
                "origin_table": "batch_native_hook_data_events_statistics_event",
                "symbol_count": 9,
            },
            {
                "origin_table": "batch_native_hook_data_events_trace_alloc_event",
                "symbol_count": 6,
            },
            {
                "origin_table": "batch_native_hook_data_events_trace_free_event",
                "symbol_count": 6,
            },
            {
                "origin_table": "profiler_payload_occurrence",
                "symbol_count": 12,
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

#[tokio::test]
async fn formal_import_preserves_parent_identity_across_the_default_spool_boundary() {
    let root = tempdir().expect("temporary import directory is created");
    let source = root.path().join("native-hook-spool-boundary.htrace");
    let dataset = root.path().join("dataset");
    let first_batch = tag_event_batch(0, 8_193);
    let second_batch = tag_event_batch(10_000, 2);
    let config = NativeHookConfig {
        clock: "mono".to_string(),
        ..Default::default()
    };
    fs::write(
        &source,
        profiler_section([
            profiler_envelope("nativehook", 31, 1, first_batch.encode_to_vec()),
            profiler_envelope("hookdaemon", 32, 1, second_batch.encode_to_vec()),
            profiler_envelope("nativehook_config", 33, 1, config.encode_to_vec()),
        ]),
    )
    .expect("large typed OHOSPROF fixture is written");

    kat_datasource::import_hitrace(
        &source,
        DatasetWriteTarget::write_to_empty(&dataset),
        |_| Ok(()),
    )
    .expect("formal import drains multiple Native Hook spool batches");
    let context = register_resolved_dataset(&dataset)
        .await
        .expect("large formal Dataset resolves in DataFusion");
    assert_eq!(
        query_json(
            &context,
            "with roots as ( \
               select _kat_row_id as root_id from batch_native_hook_data \
             ), events as ( \
               select _kat_row_id as event_id, _kat_parent_row_id as root_id, \
                      _kat_repeated_index \
               from batch_native_hook_data_events \
             ), tags as ( \
               select _kat_parent_row_id as event_id, addr, tag \
               from batch_native_hook_data_events_tag_event \
             ) \
             select roots.root_id, events.event_id, events._kat_repeated_index, \
                    tags.event_id as child_parent, tags.addr, tags.tag \
             from roots join events on events.root_id = roots.root_id \
             join tags on tags.event_id = events.event_id \
             where events.event_id in (0, 8191, 8192, 8193, 8194) \
             order by events.event_id",
        )
        .await,
        json!([
            {
                "root_id": 0,
                "event_id": 0,
                "_kat_repeated_index": 0,
                "child_parent": 0,
                "addr": 0,
                "tag": "tag-0",
            },
            {
                "root_id": 0,
                "event_id": 8191,
                "_kat_repeated_index": 8191,
                "child_parent": 8191,
                "addr": 8191,
                "tag": "tag-8191",
            },
            {
                "root_id": 0,
                "event_id": 8192,
                "_kat_repeated_index": 8192,
                "child_parent": 8192,
                "addr": 8192,
                "tag": "tag-8192",
            },
            {
                "root_id": 1,
                "event_id": 8193,
                "_kat_repeated_index": 0,
                "child_parent": 8193,
                "addr": 10_000,
                "tag": "tag-10000",
            },
            {
                "root_id": 1,
                "event_id": 8194,
                "_kat_repeated_index": 1,
                "child_parent": 8194,
                "addr": 10_001,
                "tag": "tag-10001",
            },
        ])
    );
    assert_eq!(
        query_json(
            &context,
            "select count(*) as event_count from batch_native_hook_data_events",
        )
        .await,
        json!([{"event_count": 8195}])
    );
}

#[test]
fn formal_import_enforces_native_hook_clock_admission_matrix() {
    for (clock, clock_id) in [("", 0), ("realtime", 0), ("mono", 1), ("mono_raw", 4)] {
        formal_clock_import(&[clock_id], &[clock], true).unwrap_or_else(|error| {
            panic!("late supported clock {clock:?}/{clock_id} must import: {error}")
        });
    }

    let missing = formal_clock_import(&[1], &[], true)
        .expect_err("eventful formal import requires a config clock");
    assert!(
        missing.contains("config") && missing.contains("clock"),
        "missing-clock error identifies the Native Hook contract: {missing}"
    );

    let unknown = formal_clock_import(&[1], &["unsupported-clock"], true)
        .expect_err("eventful formal import rejects an unknown config clock");
    assert!(
        unknown.contains("unsupported") && unknown.contains("clock"),
        "unknown-clock error identifies the Native Hook contract: {unknown}"
    );

    let conflict = formal_clock_import(&[1], &["mono", "boot"], true)
        .expect_err("eventful formal import rejects conflicting supported clocks");
    assert!(
        conflict.contains("conflicting") && conflict.contains("clock"),
        "clock-conflict error identifies the Native Hook contract: {conflict}"
    );

    formal_clock_import(&[99], &["unsupported-clock"], false)
        .expect("eventless data does not activate Native Hook clock admission");
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

fn tag_event_batch(start: u64, count: usize) -> BatchNativeHookData {
    use proto::kat::native_hook::{MemTagEvent, NativeHookData, native_hook_data::Event};

    BatchNativeHookData {
        events: (0..count)
            .map(|index| {
                let value = start + index as u64;
                NativeHookData {
                    tv_sec: value,
                    tv_nsec: value + 1,
                    event: Some(Event::TagEvent(MemTagEvent {
                        addr: value,
                        size: value + 2,
                        tag: format!("tag-{value}"),
                        pid: i32::try_from(value).expect("fixture pid fits i32"),
                    })),
                }
            })
            .collect(),
    }
}

fn formal_clock_import(
    event_clock_ids: &[i32],
    config_clocks: &[&str],
    has_event_element: bool,
) -> Result<(), String> {
    use proto::kat::native_hook::NativeHookData;

    let root = tempdir().expect("temporary clock import directory is created");
    let source = root.path().join("native-hook-clock.htrace");
    let dataset = root.path().join("dataset");
    let batch = BatchNativeHookData {
        events: has_event_element
            .then_some(NativeHookData {
                tv_sec: 7,
                tv_nsec: 8,
                event: None,
            })
            .into_iter()
            .collect(),
    };
    let mut messages = event_clock_ids
        .iter()
        .enumerate()
        .map(|(index, clock_id)| {
            profiler_envelope(
                if index % 2 == 0 {
                    "nativehook"
                } else {
                    "hookdaemon"
                },
                40 + index as u32,
                *clock_id,
                batch.encode_to_vec(),
            )
        })
        .collect::<Vec<_>>();
    messages.extend(config_clocks.iter().enumerate().map(|(index, clock)| {
        profiler_envelope(
            if index % 2 == 0 {
                "nativehook_config"
            } else {
                "hookdaemon_config"
            },
            50 + index as u32,
            0,
            NativeHookConfig {
                clock: (*clock).to_string(),
                ..Default::default()
            }
            .encode_to_vec(),
        )
    }));
    fs::write(&source, profiler_section(messages))
        .expect("typed clock OHOSPROF fixture is written");

    kat_datasource::import_hitrace(
        &source,
        DatasetWriteTarget::write_to_empty(&dataset),
        |_| Ok(()),
    )
    .map(|_| ())
    .map_err(|error| format!("{error:?}"))
}

fn four_route_trace() -> Vec<u8> {
    let empty_data = BatchNativeHookData::default().encode_to_vec();
    let default_config = NativeHookConfig::default().encode_to_vec();
    profiler_section([
        profiler_envelope("nativehook", 11, 0, empty_data.clone()),
        profiler_envelope("hookdaemon", 12, 1, empty_data),
        profiler_envelope("nativehook_config", 13, 4, default_config.clone()),
        profiler_envelope("hookdaemon_config", 14, 7, default_config),
    ])
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

fn strings(batches: &[arrow_array::RecordBatch], column: &str) -> Vec<String> {
    batches
        .iter()
        .flat_map(|batch| {
            let values = batch
                .column_by_name(column)
                .unwrap_or_else(|| panic!("fixture batch has no column {column:?}"))
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap_or_else(|| panic!("fixture column {column:?} is not Utf8"));
            (0..batch.num_rows()).map(|row| values.value(row).to_owned())
        })
        .collect()
}

fn u64s(batches: &[arrow_array::RecordBatch], column: &str) -> Vec<u64> {
    batches
        .iter()
        .flat_map(|batch| {
            let values = batch
                .column_by_name(column)
                .unwrap_or_else(|| panic!("fixture batch has no column {column:?}"))
                .as_any()
                .downcast_ref::<UInt64Array>()
                .unwrap_or_else(|| panic!("fixture column {column:?} is not UInt64"));
            (0..batch.num_rows()).map(|row| values.value(row))
        })
        .collect()
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
