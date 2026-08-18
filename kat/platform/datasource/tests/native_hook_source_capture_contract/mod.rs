use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::Context;
use arrow_array::RecordBatch;
use arrow_json::writer::{JsonArray, WriterBuilder};
use datafusion::prelude::{ParquetReadOptions, SessionContext};
use prost::Message;
use serde_json::{Value, json};
use tempfile::tempdir;
use url::Url;

use crate::{
    dataset_writer, formats, generated_native_hook_source_emitter, proto, protobuf_source,
};
use proto::kat::hitrace::profiler_plugin_data::ClockId;

static BATCH_TYPED_DECODES: AtomicUsize = AtomicUsize::new(0);
static CONFIG_TYPED_DECODES: AtomicUsize = AtomicUsize::new(0);

fn counting_decode_batch(
    envelope: &formats::hitrace::profiler::PluginEnvelope<'_>,
) -> anyhow::Result<proto::BatchNativeHookData> {
    BATCH_TYPED_DECODES.fetch_add(1, Ordering::Relaxed);
    formats::hitrace::profiler::decode_payload(envelope)
}

fn counting_decode_config(
    envelope: &formats::hitrace::profiler::PluginEnvelope<'_>,
) -> anyhow::Result<proto::NativeHookConfig> {
    CONFIG_TYPED_DECODES.fetch_add(1, Ordering::Relaxed);
    formats::hitrace::profiler::decode_payload(envelope)
}

#[test]
fn generated_native_hook_source_contract_is_available() {
    use generated_native_hook_source_emitter::{
        append_batch_native_hook_data_root, append_native_hook_config_root,
        profiler_clock_id_symbols, protobuf_source_specs,
    };

    let (relations, enum_origins) = protobuf_source_specs();
    assert_eq!(relations.len(), native_hook_relation_names().len());
    assert_eq!(enum_origins.len(), 3);
    let (clock_enum_fqn, clock_symbols) = profiler_clock_id_symbols();
    assert_eq!(clock_enum_fqn, "kat.hitrace.ProfilerPluginData.ClockId");
    assert_eq!(clock_symbols.len(), 12);

    let _append_data: fn(
        &mut protobuf_source::SourceTableCapture,
        u64,
        &proto::BatchNativeHookData,
    ) -> anyhow::Result<()> = append_batch_native_hook_data_root;
    let _append_config: fn(
        &mut protobuf_source::SourceTableCapture,
        u64,
        &proto::NativeHookConfig,
    ) -> anyhow::Result<()> = append_native_hook_config_root;
}

#[test]
fn profiler_capture_invokes_each_bound_root_typed_decoder_once() {
    BATCH_TYPED_DECODES.store(0, Ordering::Relaxed);
    CONFIG_TYPED_DECODES.store(0, Ordering::Relaxed);
    let mut capture = protobuf_source::native_hook::NativeHookSourceCapture::with_decoders(
        protobuf_source::SpoolOptions::new(2),
        counting_decode_batch,
        counting_decode_config,
    )
    .expect("capture accepts narrow typed decoder spies");
    for route in ["nativehook", "hookdaemon"] {
        let data = profiler_message(route, proto::BatchNativeHookData::default().encode_to_vec());
        assert!(
            capture
                .try_claim(
                    &formats::hitrace::profiler::PluginEnvelope::from_profiler_plugin_data(
                        &data, 1_024,
                    ),
                )
                .expect("data payload is claimed")
        );
    }
    assert_eq!(BATCH_TYPED_DECODES.load(Ordering::Relaxed), 2);
    assert_eq!(CONFIG_TYPED_DECODES.load(Ordering::Relaxed), 0);

    for route in ["nativehook_config", "hookdaemon_config"] {
        let config = profiler_message(route, proto::NativeHookConfig::default().encode_to_vec());
        assert!(
            capture
                .try_claim(
                    &formats::hitrace::profiler::PluginEnvelope::from_profiler_plugin_data(
                        &config, 2_048,
                    ),
                )
                .expect("config payload is claimed")
        );
    }
    assert_eq!(BATCH_TYPED_DECODES.load(Ordering::Relaxed), 2);
    assert_eq!(CONFIG_TYPED_DECODES.load(Ordering::Relaxed), 2);

    capture
        .finish()
        .expect("spy-decoded default roots pass preflight");
}

#[tokio::test]
async fn profiler_capture_claims_only_four_native_hook_routes_and_publishes_empty_roots() {
    use formats::hitrace::profiler::{PluginEnvelope, for_each_profiler_envelope_frame};
    use protobuf_source::SpoolOptions;
    use protobuf_source::native_hook::NativeHookSourceCapture;

    let empty_data = proto::BatchNativeHookData::default().encode_to_vec();
    let default_config = proto::NativeHookConfig::default().encode_to_vec();
    let frames = profiler_frames([
        profiler_message_with_provenance(
            "nativehook",
            EnvelopeProvenance {
                status: 11,
                clock_id: ClockId::ClockidRealtime as i32,
                tv_sec: 101,
                tv_nsec: 201,
                version: "data-a",
                sample_interval: 1,
            },
            empty_data.clone(),
        ),
        profiler_message_with_provenance(
            "hookdaemon",
            EnvelopeProvenance {
                status: 12,
                clock_id: ClockId::ClockidMonotonic as i32,
                tv_sec: 102,
                tv_nsec: 202,
                version: "data-b",
                sample_interval: 2,
            },
            empty_data,
        ),
        profiler_message_with_provenance(
            "nativehook_config",
            EnvelopeProvenance {
                status: 13,
                clock_id: ClockId::ClockidMonotonicRaw as i32,
                tv_sec: 103,
                tv_nsec: 203,
                version: "config-a",
                sample_interval: 3,
            },
            default_config.clone(),
        ),
        profiler_message_with_provenance(
            "hookdaemon_config",
            EnvelopeProvenance {
                status: 14,
                clock_id: ClockId::ClockidBoottime as i32,
                tv_sec: 104,
                tv_nsec: 204,
                version: "config-b",
                sample_interval: 4,
            },
            default_config,
        ),
        profiler_message("ftrace-plugin", vec![0x80]),
        profiler_message("ftrace-plugin_config", vec![0x80]),
        profiler_message("nativehookx", vec![0xff]),
        profiler_message("hookdaemonx", vec![0xff]),
        profiler_message("nativehook_config_extra", vec![0xff]),
        profiler_message("hookdaemon_config_config", vec![0xff]),
    ]);
    let mut capture = NativeHookSourceCapture::new(SpoolOptions::new(2))
        .expect("dormant Native Hook capture is valid");
    let mut claims = Vec::new();
    for_each_profiler_envelope_frame(&frames, |message, frame_offset| {
        let envelope = PluginEnvelope::from_profiler_plugin_data(&message, 1_024 + frame_offset);
        claims.push((message.name.clone(), capture.try_claim(&envelope)?));
        Ok(())
    })
    .expect("typed profiler frames decode and visit");
    assert_eq!(
        claims,
        [
            ("nativehook".to_string(), true),
            ("hookdaemon".to_string(), true),
            ("nativehook_config".to_string(), true),
            ("hookdaemon_config".to_string(), true),
            ("ftrace-plugin".to_string(), false),
            ("ftrace-plugin_config".to_string(), false),
            ("nativehookx".to_string(), false),
            ("hookdaemonx".to_string(), false),
            ("nativehook_config_extra".to_string(), false),
            ("hookdaemon_config_config".to_string(), false),
        ]
    );

    let directory = tempdir().expect("temporary Dataset directory is created");
    let dataset_path = directory.path().join("dataset");
    publish_prepared(
        capture
            .finish()
            .expect("empty data roots do not require clock admission"),
        &dataset_path,
    );
    let resolved = crate::resolve_dataset(&dataset_path)
        .expect("formal Dataset resolver accepts the dormant capture");
    assert_eq!(
        resolved
            .tables()
            .iter()
            .map(|table| table.name())
            .collect::<std::collections::BTreeSet<_>>(),
        [
            "batch_native_hook_data",
            "native_hook_config",
            "profiler_payload_occurrence",
            "protobuf_enum_symbol",
        ]
        .into_iter()
        .collect()
    );

    let context = register_resolved_dataset(&dataset_path)
        .await
        .expect("dormant capture tables register in DataFusion");
    let occurrence = context
        .table("profiler_payload_occurrence")
        .await
        .expect("occurrence table is registered");
    assert_eq!(
        occurrence
            .schema()
            .fields()
            .iter()
            .map(|field| field.name().as_str())
            .collect::<Vec<_>>(),
        [
            "_kat_row_id",
            "envelope_name",
            "status",
            "clock_id",
            "tv_sec",
            "tv_nsec",
            "version",
            "sample_interval",
        ],
        "transport data bytes must not be duplicated in occurrence provenance"
    );
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
                "clock_id": ClockId::ClockidRealtime as i32,
                "tv_sec": 101,
                "tv_nsec": 201,
                "version": "data-a",
                "sample_interval": 1,
            },
            {
                "_kat_row_id": 1,
                "envelope_name": "hookdaemon",
                "status": 12,
                "clock_id": ClockId::ClockidMonotonic as i32,
                "tv_sec": 102,
                "tv_nsec": 202,
                "version": "data-b",
                "sample_interval": 2,
            },
            {
                "_kat_row_id": 2,
                "envelope_name": "nativehook_config",
                "status": 13,
                "clock_id": ClockId::ClockidMonotonicRaw as i32,
                "tv_sec": 103,
                "tv_nsec": 203,
                "version": "config-a",
                "sample_interval": 3,
            },
            {
                "_kat_row_id": 3,
                "envelope_name": "hookdaemon_config",
                "status": 14,
                "clock_id": ClockId::ClockidBoottime as i32,
                "tv_sec": 104,
                "tv_nsec": 204,
                "version": "config-b",
                "sample_interval": 4,
            },
        ])
    );
    assert_eq!(
        query_json(
            &context,
            "select occurrence.envelope_name, root._kat_parent_row_id \
             from profiler_payload_occurrence occurrence \
             join batch_native_hook_data root \
               on root._kat_parent_row_id = occurrence._kat_row_id \
             order by occurrence._kat_row_id",
        )
        .await,
        json!([
            { "envelope_name": "nativehook", "_kat_parent_row_id": 0 },
            { "envelope_name": "hookdaemon", "_kat_parent_row_id": 1 },
        ])
    );
    assert_eq!(
        query_json(
            &context,
            "select occurrence.envelope_name, root._kat_parent_row_id \
             from profiler_payload_occurrence occurrence \
             join native_hook_config root \
               on root._kat_parent_row_id = occurrence._kat_row_id \
             order by occurrence._kat_row_id",
        )
        .await,
        json!([
            { "envelope_name": "nativehook_config", "_kat_parent_row_id": 2 },
            { "envelope_name": "hookdaemon_config", "_kat_parent_row_id": 3 },
        ])
    );
    assert_eq!(
        query_json(
            &context,
            "select origin_table, origin_field_path, enum_type_name, \
             enum_number, enum_symbol from protobuf_enum_symbol \
             order by enum_number",
        )
        .await,
        json!([
            {
                "origin_table": "profiler_payload_occurrence",
                "origin_field_path": "clock_id",
                "enum_type_name": "kat.hitrace.ProfilerPluginData.ClockId",
                "enum_number": ClockId::ClockidRealtime as i32,
                "enum_symbol": "CLOCKID_REALTIME",
            },
            {
                "origin_table": "profiler_payload_occurrence",
                "origin_field_path": "clock_id",
                "enum_type_name": "kat.hitrace.ProfilerPluginData.ClockId",
                "enum_number": ClockId::ClockidMonotonic as i32,
                "enum_symbol": "CLOCKID_MONOTONIC",
            },
            {
                "origin_table": "profiler_payload_occurrence",
                "origin_field_path": "clock_id",
                "enum_type_name": "kat.hitrace.ProfilerPluginData.ClockId",
                "enum_number": ClockId::ClockidProcessCputimeId as i32,
                "enum_symbol": "CLOCKID_PROCESS_CPUTIME_ID",
            },
            {
                "origin_table": "profiler_payload_occurrence",
                "origin_field_path": "clock_id",
                "enum_type_name": "kat.hitrace.ProfilerPluginData.ClockId",
                "enum_number": ClockId::ClockidThreadCputimeId as i32,
                "enum_symbol": "CLOCKID_THREAD_CPUTIME_ID",
            },
            {
                "origin_table": "profiler_payload_occurrence",
                "origin_field_path": "clock_id",
                "enum_type_name": "kat.hitrace.ProfilerPluginData.ClockId",
                "enum_number": ClockId::ClockidMonotonicRaw as i32,
                "enum_symbol": "CLOCKID_MONOTONIC_RAW",
            },
            {
                "origin_table": "profiler_payload_occurrence",
                "origin_field_path": "clock_id",
                "enum_type_name": "kat.hitrace.ProfilerPluginData.ClockId",
                "enum_number": ClockId::ClockidRealtimeCoarse as i32,
                "enum_symbol": "CLOCKID_REALTIME_COARSE",
            },
            {
                "origin_table": "profiler_payload_occurrence",
                "origin_field_path": "clock_id",
                "enum_type_name": "kat.hitrace.ProfilerPluginData.ClockId",
                "enum_number": ClockId::ClockidMonotonicCoarse as i32,
                "enum_symbol": "CLOCKID_MONOTONIC_COARSE",
            },
            {
                "origin_table": "profiler_payload_occurrence",
                "origin_field_path": "clock_id",
                "enum_type_name": "kat.hitrace.ProfilerPluginData.ClockId",
                "enum_number": ClockId::ClockidBoottime as i32,
                "enum_symbol": "CLOCKID_BOOTTIME",
            },
            {
                "origin_table": "profiler_payload_occurrence",
                "origin_field_path": "clock_id",
                "enum_type_name": "kat.hitrace.ProfilerPluginData.ClockId",
                "enum_number": ClockId::ClockidRealtimeAlarm as i32,
                "enum_symbol": "CLOCKID_REALTIME_ALARM",
            },
            {
                "origin_table": "profiler_payload_occurrence",
                "origin_field_path": "clock_id",
                "enum_type_name": "kat.hitrace.ProfilerPluginData.ClockId",
                "enum_number": ClockId::ClockidBoottimeAlarm as i32,
                "enum_symbol": "CLOCKID_BOOTTIME_ALARM",
            },
            {
                "origin_table": "profiler_payload_occurrence",
                "origin_field_path": "clock_id",
                "enum_type_name": "kat.hitrace.ProfilerPluginData.ClockId",
                "enum_number": ClockId::ClockidSgiCycle as i32,
                "enum_symbol": "CLOCKID_SGI_CYCLE",
            },
            {
                "origin_table": "profiler_payload_occurrence",
                "origin_field_path": "clock_id",
                "enum_type_name": "kat.hitrace.ProfilerPluginData.ClockId",
                "enum_number": ClockId::ClockidTai as i32,
                "enum_symbol": "CLOCKID_TAI",
            },
        ])
    );
}

struct RealNativeHookWireCensus {
    batch_roots: i64,
    config_roots: i64,
    events: i64,
    alloc_events: i64,
    free_events: i64,
    mmap_events: i64,
    munmap_events: i64,
    tag_events: i64,
    file_paths: i64,
    thread_names: i64,
    maps_info: i64,
    symbol_tabs: i64,
    stack_maps: i64,
    stack_ips: i64,
    none_events: i64,
    first_parent_row_id: u64,
    first_parent_event: Value,
    first_stack_row_id: u64,
    first_stack_ips: Value,
    config_values: Value,
    variant_samples: Vec<(&'static str, &'static str, Value)>,
}

fn census_real_native_hook_wire(
    path: &std::path::Path,
) -> anyhow::Result<RealNativeHookWireCensus> {
    use formats::hitrace::{
        file::{HIPROFILER_PROTOBUF_BIN, read_profiler_section},
        profiler::for_each_profiler_envelope_frame,
    };
    use proto::kat::native_hook::native_hook_data::Event;

    let bytes = std::fs::read(path)?;
    let mut batch_roots = 0_i64;
    let mut config_roots = 0_i64;
    let mut events = 0_i64;
    let mut alloc_events = 0_i64;
    let mut free_events = 0_i64;
    let mut mmap_events = 0_i64;
    let mut munmap_events = 0_i64;
    let mut tag_events = 0_i64;
    let mut file_paths = 0_i64;
    let mut thread_names = 0_i64;
    let mut maps_info = 0_i64;
    let mut symbol_tabs = 0_i64;
    let mut stack_maps = 0_i64;
    let mut stack_ips = 0_i64;
    let mut none_events = 0_i64;
    let mut first_parent_row_id = 0_u64;
    let mut first_parent_event = Value::Null;
    let mut first_stack_row_id = 0_u64;
    let mut first_stack_ips = Value::Null;
    let mut config_values = Vec::new();
    let mut alloc_sample = None;
    let mut free_sample = None;
    let mut mmap_sample = None;
    let mut munmap_sample = None;
    let mut tag_sample = None;
    let mut file_path_sample = None;
    let mut thread_name_sample = None;
    let mut maps_info_sample = None;
    let mut symbol_tab_sample = None;
    let mut stack_sample = None;
    let mut offset = 0;
    while offset < bytes.len() {
        let section = read_profiler_section(&bytes, offset)?;
        if section.header.data_type == HIPROFILER_PROTOBUF_BIN {
            for_each_profiler_envelope_frame(section.body(&bytes), |message, _| {
                match message.name.as_str() {
                    "nativehook" | "hookdaemon" => {
                        let batch = proto::BatchNativeHookData::decode(message.data.as_slice())?;
                        let root_row_id = u64::try_from(batch_roots)?;
                        batch_roots += 1;
                        for (repeated_index, event) in batch.events.into_iter().enumerate() {
                            let event_row_id = u64::try_from(events)?;
                            events += 1;
                            match event.event {
                                Some(Event::AllocEvent(value)) => {
                                    alloc_events += 1;
                                    if alloc_sample.is_none() {
                                        first_parent_row_id = event_row_id;
                                        first_parent_event = json!({
                                            "_kat_parent_row_id": root_row_id,
                                            "_kat_repeated_index": repeated_index,
                                            "tv_sec": event.tv_sec,
                                            "tv_nsec": event.tv_nsec,
                                        });
                                    }
                                    alloc_sample.get_or_insert_with(|| {
                                        json!({
                                            "_kat_parent_row_id": event_row_id,
                                            "pid": value.pid,
                                            "tid": value.tid,
                                            "addr": value.addr,
                                            "size": value.size,
                                            "thread_name_id": value.thread_name_id,
                                            "stack_id": value.stack_id,
                                        })
                                    });
                                }
                                Some(Event::FreeEvent(value)) => {
                                    free_events += 1;
                                    free_sample.get_or_insert_with(|| {
                                        json!({
                                            "_kat_parent_row_id": event_row_id,
                                            "pid": value.pid,
                                            "tid": value.tid,
                                            "addr": value.addr,
                                            "thread_name_id": value.thread_name_id,
                                            "stack_id": value.stack_id,
                                        })
                                    });
                                }
                                Some(Event::MmapEvent(value)) => {
                                    mmap_events += 1;
                                    mmap_sample.get_or_insert_with(|| {
                                        json!({
                                            "_kat_parent_row_id": event_row_id,
                                            "pid": value.pid,
                                            "tid": value.tid,
                                            "addr": value.addr,
                                            "type": value.r#type,
                                            "size": value.size,
                                            "thread_name_id": value.thread_name_id,
                                            "stack_id": value.stack_id,
                                        })
                                    });
                                }
                                Some(Event::MunmapEvent(value)) => {
                                    munmap_events += 1;
                                    munmap_sample.get_or_insert_with(|| {
                                        json!({
                                            "_kat_parent_row_id": event_row_id,
                                            "pid": value.pid,
                                            "tid": value.tid,
                                            "addr": value.addr,
                                            "size": value.size,
                                            "thread_name_id": value.thread_name_id,
                                            "stack_id": value.stack_id,
                                        })
                                    });
                                }
                                Some(Event::TagEvent(value)) => {
                                    tag_events += 1;
                                    tag_sample.get_or_insert_with(|| {
                                        json!({
                                            "_kat_parent_row_id": event_row_id,
                                            "addr": value.addr,
                                            "size": value.size,
                                            "tag": value.tag,
                                            "pid": value.pid,
                                        })
                                    });
                                }
                                Some(Event::FilePath(value)) => {
                                    file_paths += 1;
                                    file_path_sample.get_or_insert_with(|| {
                                        json!({
                                            "_kat_parent_row_id": event_row_id,
                                            "id": value.id,
                                            "name": value.name,
                                            "pid": value.pid,
                                        })
                                    });
                                }
                                Some(Event::ThreadNameMap(value)) => {
                                    thread_names += 1;
                                    thread_name_sample.get_or_insert_with(|| {
                                        json!({
                                            "_kat_parent_row_id": event_row_id,
                                            "id": value.id,
                                            "name": value.name,
                                            "pid": value.pid,
                                        })
                                    });
                                }
                                Some(Event::MapsInfo(value)) => {
                                    maps_info += 1;
                                    maps_info_sample.get_or_insert_with(|| {
                                        json!({
                                            "_kat_parent_row_id": event_row_id,
                                            "pid": value.pid,
                                            "start": value.start,
                                            "end": value.end,
                                            "offset": value.offset,
                                            "file_path_id": value.file_path_id,
                                        })
                                    });
                                }
                                Some(Event::SymbolTab(value)) => {
                                    symbol_tabs += 1;
                                    symbol_tab_sample.get_or_insert_with(|| json!({
                                        "_kat_parent_row_id": event_row_id,
                                        "file_path_id": value.file_path_id,
                                        "text_exec_vaddr": value.text_exec_vaddr,
                                        "text_exec_vaddr_file_offset": value.text_exec_vaddr_file_offset,
                                        "sym_entry_size": value.sym_entry_size,
                                        "sym_table": hex_bytes(&value.sym_table),
                                        "str_table": hex_bytes(&value.str_table),
                                        "pid": value.pid,
                                    }));
                                }
                                Some(Event::StackMap(value)) => {
                                    let stack_row_id = u64::try_from(stack_maps)?;
                                    stack_maps += 1;
                                    stack_ips += i64::try_from(value.ip.len())?;
                                    stack_sample.get_or_insert_with(|| {
                                        json!({
                                            "_kat_parent_row_id": event_row_id,
                                            "id": value.id,
                                            "pid": value.pid,
                                        })
                                    });
                                    if first_stack_ips.is_null() && !value.ip.is_empty() {
                                        first_stack_row_id = stack_row_id;
                                        first_stack_ips = Value::Array(
                                            value
                                                .ip
                                                .iter()
                                                .enumerate()
                                                .map(|(index, value)| {
                                                    json!({
                                                        "_kat_parent_row_id": stack_row_id,
                                                        "_kat_repeated_index": index,
                                                        "value": value,
                                                    })
                                                })
                                                .collect(),
                                        );
                                    }
                                }
                                None => none_events += 1,
                                Some(_) => {}
                            }
                        }
                    }
                    "nativehook_config" | "hookdaemon_config" => {
                        let config = proto::NativeHookConfig::decode(message.data.as_slice())?;
                        config_roots += 1;
                        config_values.push(json!({
                            "pid": config.pid,
                            "clock": config.clock,
                            "sample_interval": config.sample_interval,
                        }));
                    }
                    _ => {}
                }
                Ok(())
            })?;
        }
        offset = section.end;
    }
    anyhow::ensure!(
        !first_stack_ips.is_null(),
        "real Native Hook has no stack IPs"
    );
    let variant_samples = vec![
        (
            "batch_native_hook_data_events_alloc_event",
            "_kat_parent_row_id, pid, tid, addr, size, thread_name_id, stack_id",
            alloc_sample.context("real Native Hook has no alloc_event")?,
        ),
        (
            "batch_native_hook_data_events_free_event",
            "_kat_parent_row_id, pid, tid, addr, thread_name_id, stack_id",
            free_sample.context("real Native Hook has no free_event")?,
        ),
        (
            "batch_native_hook_data_events_mmap_event",
            "_kat_parent_row_id, pid, tid, addr, type, size, thread_name_id, stack_id",
            mmap_sample.context("real Native Hook has no mmap_event")?,
        ),
        (
            "batch_native_hook_data_events_munmap_event",
            "_kat_parent_row_id, pid, tid, addr, size, thread_name_id, stack_id",
            munmap_sample.context("real Native Hook has no munmap_event")?,
        ),
        (
            "batch_native_hook_data_events_stack_map",
            "_kat_parent_row_id, id, pid",
            stack_sample.context("real Native Hook has no stack_map")?,
        ),
        (
            "batch_native_hook_data_events_tag_event",
            "_kat_parent_row_id, addr, size, tag, pid",
            tag_sample.context("real Native Hook has no tag_event")?,
        ),
        (
            "batch_native_hook_data_events_file_path",
            "_kat_parent_row_id, id, name, pid",
            file_path_sample.context("real Native Hook has no file_path")?,
        ),
        (
            "batch_native_hook_data_events_thread_name_map",
            "_kat_parent_row_id, id, name, pid",
            thread_name_sample.context("real Native Hook has no thread_name_map")?,
        ),
        (
            "batch_native_hook_data_events_maps_info",
            "_kat_parent_row_id, pid, \"start\", \"end\", \"offset\", file_path_id",
            maps_info_sample.context("real Native Hook has no maps_info")?,
        ),
        (
            "batch_native_hook_data_events_symbol_tab",
            "_kat_parent_row_id, file_path_id, text_exec_vaddr, text_exec_vaddr_file_offset, \
             sym_entry_size, sym_table, str_table, pid",
            symbol_tab_sample.context("real Native Hook has no symbol_tab")?,
        ),
    ];
    Ok(RealNativeHookWireCensus {
        batch_roots,
        config_roots,
        events,
        alloc_events,
        free_events,
        mmap_events,
        munmap_events,
        tag_events,
        file_paths,
        thread_names,
        maps_info,
        symbol_tabs,
        stack_maps,
        stack_ips,
        none_events,
        first_parent_row_id,
        first_parent_event,
        first_stack_row_id,
        first_stack_ips,
        config_values: Value::Array(config_values),
        variant_samples,
    })
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[tokio::test]
#[ignore = "requires KAT_REAL_NATIVE_HOOK_SOURCE to name the reviewed real capture"]
async fn formal_import_real_native_hook_keeps_existing_root_counts() -> anyhow::Result<()> {
    let path = std::env::var_os("KAT_REAL_NATIVE_HOOK_SOURCE")
        .ok_or_else(|| anyhow::anyhow!("KAT_REAL_NATIVE_HOOK_SOURCE is required"))?;
    let wire = census_real_native_hook_wire(std::path::Path::new(&path))?;
    let directory = tempdir()?;
    let dataset_path = directory.path().join("dataset");
    crate::import_hitrace(
        &path,
        crate::DatasetWriteTarget::write_to_empty(&dataset_path),
        |_| Ok(()),
    )?;
    assert_eq!(crate::resolve_dataset(&dataset_path)?.tables().len(), 18);
    let context = register_resolved_dataset(&dataset_path).await?;

    for (table, expected) in [
        (
            "profiler_payload_occurrence",
            wire.batch_roots + wire.config_roots,
        ),
        ("batch_native_hook_data", wire.batch_roots),
        ("native_hook_config", wire.config_roots),
        ("batch_native_hook_data_events", wire.events),
        (
            "batch_native_hook_data_events_alloc_event",
            wire.alloc_events,
        ),
        ("batch_native_hook_data_events_free_event", wire.free_events),
        ("batch_native_hook_data_events_mmap_event", wire.mmap_events),
        (
            "batch_native_hook_data_events_munmap_event",
            wire.munmap_events,
        ),
        ("batch_native_hook_data_events_tag_event", wire.tag_events),
        ("batch_native_hook_data_events_file_path", wire.file_paths),
        (
            "batch_native_hook_data_events_thread_name_map",
            wire.thread_names,
        ),
        ("batch_native_hook_data_events_maps_info", wire.maps_info),
        ("batch_native_hook_data_events_symbol_tab", wire.symbol_tabs),
        ("batch_native_hook_data_events_stack_map", wire.stack_maps),
        ("batch_native_hook_data_events_stack_map_ip", wire.stack_ips),
    ] {
        assert_eq!(
            query_json(&context, &format!("select count(*) as rows from {table}")).await,
            json!([{"rows": expected}]),
            "unexpected real Native Hook row count for {table}"
        );
    }

    assert_eq!(wire.batch_roots, 6_613);
    assert_eq!(wire.config_roots, 1);
    assert_eq!(wire.events, 243_791);
    assert_eq!(wire.alloc_events, 114_976);
    assert_eq!(wire.free_events, 117_976);
    assert_eq!(wire.mmap_events, 64);
    assert_eq!(wire.munmap_events, 65);
    assert_eq!(wire.stack_maps, 8_408);
    assert_eq!(wire.stack_ips, 282_982);
    assert_eq!(
        wire.alloc_events
            + wire.free_events
            + wire.mmap_events
            + wire.munmap_events
            + wire.tag_events
            + wire.file_paths
            + wire.thread_names
            + wire.maps_info
            + wire.symbol_tabs
            + wire.stack_maps
            + wire.none_events,
        wire.events,
        "real Native Hook census must classify every active oneof or explicit absence"
    );

    for (table, columns, expected) in wire.variant_samples {
        assert_eq!(
            query_json(
                &context,
                &format!("select {columns} from {table} order by _kat_parent_row_id limit 1"),
            )
            .await,
            json!([expected]),
            "real Native Hook oneof values drifted in {table}"
        );
    }
    assert_eq!(
        query_json(
            &context,
            &format!(
                "select _kat_parent_row_id, _kat_repeated_index, tv_sec, tv_nsec \
                 from batch_native_hook_data_events where _kat_row_id = {}",
                wire.first_parent_row_id
            ),
        )
        .await,
        json!([wire.first_parent_event])
    );
    assert_eq!(
        query_json(
            &context,
            &format!(
                "select _kat_parent_row_id, _kat_repeated_index, value \
                 from batch_native_hook_data_events_stack_map_ip \
                 where _kat_parent_row_id = {} order by _kat_repeated_index",
                wire.first_stack_row_id
            ),
        )
        .await,
        wire.first_stack_ips
    );
    assert_eq!(
        query_json(
            &context,
            "select pid, clock, sample_interval from native_hook_config order by _kat_row_id",
        )
        .await,
        wire.config_values
    );

    Ok(())
}

#[test]
fn route_match_uses_raw_envelope_name_and_kind_not_derived_plugin_name() {
    use formats::hitrace::profiler::{PluginEnvelope, PluginEnvelopeKind};
    use protobuf_source::SpoolOptions;
    use protobuf_source::native_hook::NativeHookSourceCapture;

    let data_payload = proto::BatchNativeHookData::default().encode_to_vec();
    let config_payload = proto::NativeHookConfig::default().encode_to_vec();
    for (envelope_name, kind, payload) in [
        (
            "nativehook",
            PluginEnvelopeKind::Data,
            data_payload.as_slice(),
        ),
        (
            "hookdaemon",
            PluginEnvelopeKind::Data,
            data_payload.as_slice(),
        ),
        (
            "nativehook_config",
            PluginEnvelopeKind::Config,
            config_payload.as_slice(),
        ),
        (
            "hookdaemon_config",
            PluginEnvelopeKind::Config,
            config_payload.as_slice(),
        ),
    ] {
        let envelope = PluginEnvelope {
            plugin_name: "legacy-derived-wrong",
            envelope_name,
            kind,
            payload,
            status: 0,
            clock_id: ClockId::ClockidRealtime as i32,
            tv_sec: 0,
            tv_nsec: 0,
            version: "",
            sample_interval: 0,
            section_start: 1_024,
        };
        let mut capture = NativeHookSourceCapture::new(SpoolOptions::new(2))
            .expect("profiler source capture is valid");
        assert!(
            capture
                .try_claim(&envelope)
                .expect("raw exact route decodes"),
            "raw route {envelope_name:?}/{kind:?} must not depend on derived plugin_name"
        );
        capture.finish().expect("empty/default route preflights");
    }

    for (envelope_name, wrong_kind) in [
        ("nativehook", PluginEnvelopeKind::Config),
        ("hookdaemon", PluginEnvelopeKind::Config),
        ("nativehook_config", PluginEnvelopeKind::Data),
        ("hookdaemon_config", PluginEnvelopeKind::Data),
        ("ftrace-plugin", PluginEnvelopeKind::Data),
        ("ftrace-plugin_config", PluginEnvelopeKind::Config),
    ] {
        let envelope = PluginEnvelope {
            plugin_name: "nativehook",
            envelope_name,
            kind: wrong_kind,
            payload: &[0xff],
            status: 0,
            clock_id: ClockId::ClockidRealtime as i32,
            tv_sec: 0,
            tv_nsec: 0,
            version: "",
            sample_interval: 0,
            section_start: 2_048,
        };
        let mut capture = NativeHookSourceCapture::new(SpoolOptions::new(2))
            .expect("profiler source capture is valid");
        assert!(
            !capture
                .try_claim(&envelope)
                .expect("wrong-kind raw route is not decoded"),
            "raw name {envelope_name:?} with {wrong_kind:?} must not claim"
        );
        capture
            .finish()
            .expect("unclaimed route leaves capture healthy");
    }
}

#[test]
fn malformed_unbound_payload_is_ignored_but_bound_failure_is_terminal() {
    use formats::hitrace::profiler::PluginEnvelope;
    use protobuf_source::SpoolOptions;
    use protobuf_source::native_hook::NativeHookSourceCapture;

    let unbound = profiler_message("nativehook-near", vec![0xff]);
    let mut healthy = NativeHookSourceCapture::new(SpoolOptions::new(2))
        .expect("profiler source capture is valid");
    assert!(
        !healthy
            .try_claim(&PluginEnvelope::from_profiler_plugin_data(&unbound, 1_024))
            .expect("an unbound payload is not decoded")
    );
    let valid = profiler_message(
        "nativehook",
        proto::BatchNativeHookData::default().encode_to_vec(),
    );
    assert!(
        healthy
            .try_claim(&PluginEnvelope::from_profiler_plugin_data(&valid, 2_048))
            .expect("unbound malformed input does not poison later claims")
    );
    healthy
        .finish()
        .expect("healthy empty-root capture passes preflight");

    let malformed_bound = profiler_message("nativehook", vec![0xff]);
    let mut poisoned = NativeHookSourceCapture::new(SpoolOptions::new(2))
        .expect("profiler source capture is valid");
    let first_error = poisoned
        .try_claim(&PluginEnvelope::from_profiler_plugin_data(
            &malformed_bound,
            3_072,
        ))
        .expect_err("a bound malformed payload fails instead of becoming unsupported");
    assert!(
        first_error.to_string().contains("decode"),
        "bound failure identifies typed decode: {first_error:#}"
    );
    assert!(
        poisoned
            .try_claim(&PluginEnvelope::from_profiler_plugin_data(&unbound, 4_096))
            .is_err(),
        "a poisoned capture rejects even later unbound input"
    );
    assert!(
        poisoned.finish().is_err(),
        "a bound failure remains terminal through finish"
    );
}

#[test]
fn nonempty_batch_requires_config_even_when_event_and_envelope_clock_are_present() {
    use formats::hitrace::profiler::PluginEnvelope;
    use protobuf_source::SpoolOptions;
    use protobuf_source::native_hook::NativeHookSourceCapture;

    let batch = proto::BatchNativeHookData {
        events: vec![proto::kat::native_hook::NativeHookData {
            tv_sec: 7,
            tv_nsec: 8,
            event: None,
        }],
    };
    let message = profiler_message_with_provenance(
        "nativehook",
        EnvelopeProvenance {
            clock_id: ClockId::ClockidBoottime as i32,
            ..Default::default()
        },
        batch.encode_to_vec(),
    );
    let mut capture = NativeHookSourceCapture::new(SpoolOptions::new(2))
        .expect("profiler source capture is valid");
    assert!(
        capture
            .try_claim(&PluginEnvelope::from_profiler_plugin_data(&message, 1_024))
            .expect("bound None-event batch decodes")
    );
    let error = match capture.finish() {
        Ok(_) => panic!("an event element requires config clock admission"),
        Err(error) => error,
    };
    assert!(
        error.to_string().contains("config") && error.to_string().contains("clock"),
        "envelope clock must not become a missing-config fallback: {error:#}"
    );
}

#[test]
fn clock_admission_accepts_late_mono_config_and_rejects_unknown_clock() {
    use formats::hitrace::profiler::{PluginEnvelope, for_each_profiler_envelope_frame};
    use protobuf_source::SpoolOptions;
    use protobuf_source::native_hook::NativeHookSourceCapture;

    for (clock, expected_clock_id, should_succeed) in [
        ("mono", ClockId::ClockidMonotonic as i32, true),
        ("unsupported-clock", ClockId::ClockidMonotonic as i32, false),
    ] {
        let batch = proto::BatchNativeHookData {
            events: vec![proto::kat::native_hook::NativeHookData {
                tv_sec: 7,
                tv_nsec: 8,
                event: None,
            }],
        };
        let config = proto::NativeHookConfig {
            clock: clock.to_string(),
            ..Default::default()
        };
        let frames = profiler_frames([
            profiler_message_with_provenance(
                "nativehook",
                EnvelopeProvenance {
                    clock_id: expected_clock_id,
                    ..Default::default()
                },
                batch.encode_to_vec(),
            ),
            profiler_message("nativehook_config", config.encode_to_vec()),
        ]);
        let mut capture = NativeHookSourceCapture::new(SpoolOptions::new(2))
            .expect("profiler source capture is valid");
        for_each_profiler_envelope_frame(&frames, |message, frame_offset| {
            let envelope =
                PluginEnvelope::from_profiler_plugin_data(&message, 1_024 + frame_offset);
            assert!(capture.try_claim(&envelope)?, "fixture route must claim");
            Ok(())
        })
        .expect("late-config frames decode and claim");

        match (should_succeed, capture.finish()) {
            (true, Ok(_)) => {}
            (true, Err(error)) => panic!("supported late clock must pass: {error:#}"),
            (false, Ok(_)) => panic!("unknown config clock must fail admission"),
            (false, Err(error)) => assert!(
                error.to_string().contains("clock"),
                "unknown-clock error must identify the clock contract: {error:#}"
            ),
        }
    }
}

#[test]
fn clock_admission_rejects_event_envelope_mismatch() {
    let error = match finish_clock_fixture(
        &[
            ClockId::ClockidMonotonic as i32,
            ClockId::ClockidMonotonicRaw as i32,
        ],
        &["mono"],
    ) {
        Ok(_) => panic!("one mismatched event envelope clock must fail admission"),
        Err(error) => error,
    };
    assert!(
        error.to_string().contains("clock"),
        "mismatch error must identify the clock contract: {error:#}"
    );
}

#[test]
fn clock_admission_rejects_conflicting_supported_configs() {
    let error = match finish_clock_fixture(&[ClockId::ClockidMonotonic as i32], &["mono", "boot"]) {
        Ok(_) => panic!("conflicting supported config clocks must fail admission"),
        Err(error) => error,
    };
    assert!(
        error.to_string().contains("clock") && error.to_string().contains("conflict"),
        "conflict error must identify both the clock and conflict: {error:#}"
    );
}

#[test]
fn clock_admission_supports_all_values_equivalence_and_eventless_gating() {
    for (clock, clock_id) in [
        ("", ClockId::ClockidRealtime as i32),
        ("realtime", ClockId::ClockidRealtime as i32),
        ("mono", ClockId::ClockidMonotonic as i32),
        ("mono_raw", ClockId::ClockidMonotonicRaw as i32),
        ("boot", ClockId::ClockidBoottime as i32),
    ] {
        finish_clock_fixture(&[clock_id], &[clock]).unwrap_or_else(|error| {
            panic!("supported eventful clock {clock:?}/{clock_id} must pass: {error:#}")
        });
    }
    finish_clock_fixture(&[ClockId::ClockidRealtime as i32], &["", "realtime"])
        .expect("empty and realtime are equivalent duplicate configs");
    finish_clock_fixture(&[ClockId::ClockidMonotonic as i32], &["mono", "mono"])
        .expect("an identical supported clock can be repeated");

    finish_empty_clock_fixture(&[99], &["unsupported-clock", "mono", "boot"])
        .expect("empty data does not activate Native Hook clock admission");
    finish_empty_clock_fixture(&[], &["unsupported-clock", "mono", "boot"])
        .expect("config-only capture does not activate Native Hook clock admission");
}

#[tokio::test]
async fn full_ohosprof_topology_publishes_only_the_25_data_and_3_config_relations_with_rows() {
    use protobuf_source::SpoolOptions;
    use protobuf_source::native_hook::NativeHookSourceCapture;

    let (first_batch, second_batch) = full_native_hook_batches();
    let config = full_native_hook_config("boot");
    let trace_file = [
        profiler_section([profiler_message_with_provenance(
            "nativehook",
            EnvelopeProvenance {
                status: 21,
                clock_id: ClockId::ClockidBoottime as i32,
                tv_sec: 501,
                tv_nsec: 601,
                version: "topology-a",
                sample_interval: 10,
            },
            first_batch.encode_to_vec(),
        )]),
        profiler_section([profiler_message_with_provenance(
            "hookdaemon",
            EnvelopeProvenance {
                status: 22,
                clock_id: ClockId::ClockidBoottime as i32,
                tv_sec: 502,
                tv_nsec: 602,
                version: "topology-b",
                sample_interval: 20,
            },
            second_batch.encode_to_vec(),
        )]),
        profiler_section([profiler_message_with_provenance(
            "hookdaemon_config",
            EnvelopeProvenance {
                status: 23,
                clock_id: ClockId::ClockidBoottime as i32,
                tv_sec: 503,
                tv_nsec: 603,
                version: "topology-config",
                sample_interval: 30,
            },
            config.encode_to_vec(),
        )]),
    ]
    .concat();

    let mut capture = NativeHookSourceCapture::new(SpoolOptions::with_limits(1, 128))
        .expect("profiler source capture is valid");
    assert_eq!(
        claim_profiler_file(&mut capture, &trace_file).expect("OHOSPROF sections decode and claim"),
        3
    );
    let prepared = capture
        .finish()
        .expect("boot config admits both eventful data envelopes");
    assert_eq!(
        prepared.preflighted_row_group_count("batch_native_hook_data"),
        Some(2),
        "tiny row bound must flush each data parent independently"
    );
    assert_eq!(
        prepared.preflighted_row_group_count("batch_native_hook_data_events"),
        Some(17),
        "tiny row bound must flush events while preserving all parents and indexes"
    );
    assert_eq!(
        prepared.preflighted_row_group_count("profiler_payload_occurrence"),
        Some(3),
        "envelope provenance must cross the same bounded-spool path"
    );

    let directory = tempdir().expect("temporary Dataset directory is created");
    let dataset_path = directory.path().join("dataset");
    publish_prepared(prepared, &dataset_path);
    let resolved = crate::resolve_dataset(&dataset_path)
        .expect("formal Dataset resolver accepts full Native Hook topology");
    let actual_tables = resolved
        .tables()
        .iter()
        .map(|table| table.name())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(actual_tables, full_native_hook_table_names());
    for legacy_name in [
        "clock_domain",
        "clock_value",
        "native_hook_alloc",
        "native_hook_free",
    ] {
        assert!(
            !actual_tables.contains(legacy_name),
            "legacy projection {legacy_name:?} must not be published"
        );
    }
    assert_native_hook_physical_schemas(&dataset_path);

    let context = register_resolved_dataset(&dataset_path)
        .await
        .expect("full Native Hook tables register in DataFusion");
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
            "select _kat_parent_row_id, _kat_repeated_index, tv_sec, tv_nsec \
             from batch_native_hook_data_events \
             order by _kat_row_id",
        )
        .await,
        expected_native_hook_event_identity()
    );
    assert_eq!(
        query_json(
            &context,
            "with variants as ( \
               select _kat_parent_row_id as event_id, 'alloc_event' as kind \
                 from batch_native_hook_data_events_alloc_event union all \
               select _kat_parent_row_id, 'free_event' \
                 from batch_native_hook_data_events_free_event union all \
               select _kat_parent_row_id, 'mmap_event' \
                 from batch_native_hook_data_events_mmap_event union all \
               select _kat_parent_row_id, 'munmap_event' \
                 from batch_native_hook_data_events_munmap_event union all \
               select _kat_parent_row_id, 'tag_event' \
                 from batch_native_hook_data_events_tag_event union all \
               select _kat_parent_row_id, 'file_path' \
                 from batch_native_hook_data_events_file_path union all \
               select _kat_parent_row_id, 'symbol_name' \
                 from batch_native_hook_data_events_symbol_name union all \
               select _kat_parent_row_id, 'thread_name_map' \
                 from batch_native_hook_data_events_thread_name_map union all \
               select _kat_parent_row_id, 'maps_info' \
                 from batch_native_hook_data_events_maps_info union all \
               select _kat_parent_row_id, 'symbol_tab' \
                 from batch_native_hook_data_events_symbol_tab union all \
               select _kat_parent_row_id, 'frame_map' \
                 from batch_native_hook_data_events_frame_map union all \
               select _kat_parent_row_id, 'stack_map' \
                 from batch_native_hook_data_events_stack_map union all \
               select _kat_parent_row_id, 'statistics_event' \
                 from batch_native_hook_data_events_statistics_event union all \
               select _kat_parent_row_id, 'trace_alloc_event' \
                 from batch_native_hook_data_events_trace_alloc_event union all \
               select _kat_parent_row_id, 'trace_free_event' \
                 from batch_native_hook_data_events_trace_free_event \
             ) \
             select event._kat_row_id as event_id, event._kat_parent_row_id as root_id, \
                    event._kat_repeated_index, variants.kind \
             from batch_native_hook_data_events event \
             left join variants on variants.event_id = event._kat_row_id \
             order by event._kat_row_id",
        )
        .await,
        expected_native_hook_membership()
    );
    for (table, expected) in native_hook_variant_payloads() {
        assert_eq!(
            query_json(&context, &format!("select * from {table}")).await,
            expected,
            "variant payload drifted in {table:?}"
        );
    }
    assert_eq!(
        query_json(
            &context,
            "with frames as ( \
               select 'alloc' as kind, parent._kat_parent_row_id as event_id, \
                      child.* from batch_native_hook_data_events_alloc_event parent \
                 join batch_native_hook_data_events_alloc_event_frame_info child \
                   on child._kat_parent_row_id = parent._kat_row_id union all \
               select 'free', parent._kat_parent_row_id, child.* \
                 from batch_native_hook_data_events_free_event parent \
                 join batch_native_hook_data_events_free_event_frame_info child \
                   on child._kat_parent_row_id = parent._kat_row_id union all \
               select 'mmap', parent._kat_parent_row_id, child.* \
                 from batch_native_hook_data_events_mmap_event parent \
                 join batch_native_hook_data_events_mmap_event_frame_info child \
                   on child._kat_parent_row_id = parent._kat_row_id union all \
               select 'munmap', parent._kat_parent_row_id, child.* \
                 from batch_native_hook_data_events_munmap_event parent \
                 join batch_native_hook_data_events_munmap_event_frame_info child \
                   on child._kat_parent_row_id = parent._kat_row_id union all \
               select 'trace_alloc', parent._kat_parent_row_id, child.* \
                 from batch_native_hook_data_events_trace_alloc_event parent \
                 join batch_native_hook_data_events_trace_alloc_event_frame_info child \
                   on child._kat_parent_row_id = parent._kat_row_id union all \
               select 'trace_free', parent._kat_parent_row_id, child.* \
                 from batch_native_hook_data_events_trace_free_event parent \
                 join batch_native_hook_data_events_trace_free_event_frame_info child \
                   on child._kat_parent_row_id = parent._kat_row_id \
             ) \
             select * from frames order by kind, _kat_repeated_index",
        )
        .await,
        expected_native_hook_frames()
    );
    assert_eq!(
        query_json(
            &context,
            "select parent._kat_parent_row_id as event_id, child._kat_parent_row_id as stack_id, \
                    child._kat_repeated_index, child.value \
             from batch_native_hook_data_events_stack_map parent \
             join batch_native_hook_data_events_stack_map_frame_map_id child \
               on child._kat_parent_row_id = parent._kat_row_id \
             order by child._kat_repeated_index",
        )
        .await,
        json!([
            { "event_id": 12, "stack_id": 0, "_kat_repeated_index": 0, "value": 501 },
            { "event_id": 12, "stack_id": 0, "_kat_repeated_index": 1, "value": 502 },
        ])
    );
    assert_eq!(
        query_json(
            &context,
            "select parent._kat_parent_row_id as event_id, child._kat_parent_row_id as stack_id, \
                    child._kat_repeated_index, child.value \
             from batch_native_hook_data_events_stack_map parent \
             join batch_native_hook_data_events_stack_map_ip child \
               on child._kat_parent_row_id = parent._kat_row_id \
             order by child._kat_repeated_index",
        )
        .await,
        json!([
            { "event_id": 12, "stack_id": 0, "_kat_repeated_index": 0, "value": 0x2200 },
            { "event_id": 12, "stack_id": 0, "_kat_repeated_index": 1, "value": 0x2201 },
            { "event_id": 12, "stack_id": 0, "_kat_repeated_index": 2, "value": 0x2202 },
        ])
    );
    assert_eq!(
        query_json(
            &context,
            "select event._kat_row_id as event_id, frame_map._kat_parent_row_id as event_parent, \
                    frame_map.id, frame_map.frame, frame_map.pid \
             from batch_native_hook_data_events event \
             join batch_native_hook_data_events_frame_map frame_map \
               on frame_map._kat_parent_row_id = event._kat_row_id \
             order by event._kat_row_id",
        )
        .await,
        json!([
            {
                "event_id": 10,
                "event_parent": 10,
                "id": 101,
                "frame": null,
                "pid": 2000,
            },
            {
                "event_id": 11,
                "event_parent": 11,
                "id": 111,
                "frame": native_hook_frame_json(50),
                "pid": 2100,
            },
        ])
    );
    assert_eq!(
        query_json(
            &context,
            "select _kat_parent_row_id, sym_table, str_table \
             from batch_native_hook_data_events_symbol_tab",
        )
        .await,
        json!([{
            "_kat_parent_row_id": 9,
            "sym_table": "00ff80",
            "str_table": "fe007f",
        }])
    );
    assert_eq!(
        query_json(&context, "select * from native_hook_config").await,
        expected_native_hook_config_root(&config)
    );
    assert_eq!(
        query_json(
            &context,
            "select _kat_parent_row_id, _kat_repeated_index, value \
             from native_hook_config_expand_pids order by _kat_repeated_index",
        )
        .await,
        json!([
            { "_kat_parent_row_id": 0, "_kat_repeated_index": 0, "value": 4242 },
            { "_kat_parent_row_id": 0, "_kat_repeated_index": 1, "value": 4343 },
        ])
    );
    assert_eq!(
        query_json(
            &context,
            "select _kat_parent_row_id, _kat_repeated_index, value \
             from native_hook_config_restrace_tag order by _kat_repeated_index",
        )
        .await,
        json!([
            { "_kat_parent_row_id": 0, "_kat_repeated_index": 0, "value": "tag-a" },
            { "_kat_parent_row_id": 0, "_kat_repeated_index": 1, "value": "tag-b" },
        ])
    );
    assert_eq!(
        query_json(
            &context,
            "select origin_table, origin_field_path, enum_type_name, count(*) as symbol_count \
             from protobuf_enum_symbol \
             group by origin_table, origin_field_path, enum_type_name \
             order by origin_table",
        )
        .await,
        json!([
            {
                "origin_table": "batch_native_hook_data_events_statistics_event",
                "origin_field_path": "type",
                "enum_type_name": "kat.native_hook.RecordStatisticsEvent.MemoryType",
                "symbol_count": 9,
            },
            {
                "origin_table": "batch_native_hook_data_events_trace_alloc_event",
                "origin_field_path": "trace_type",
                "enum_type_name": "kat.native_hook.TraceType",
                "symbol_count": 6,
            },
            {
                "origin_table": "batch_native_hook_data_events_trace_free_event",
                "origin_field_path": "trace_type",
                "enum_type_name": "kat.native_hook.TraceType",
                "symbol_count": 6,
            },
            {
                "origin_table": "profiler_payload_occurrence",
                "origin_field_path": "clock_id",
                "enum_type_name": "kat.hitrace.ProfilerPluginData.ClockId",
                "symbol_count": 12,
            },
        ])
    );
    assert_eq!(
        query_json(
            &context,
            "with values as ( \
               select 'statistics' as kind, \
                      'batch_native_hook_data_events_statistics_event' as origin_table, \
                      'type' as origin_field_path, type as enum_number \
                 from batch_native_hook_data_events_statistics_event union all \
               select 'trace_alloc', \
                      'batch_native_hook_data_events_trace_alloc_event', \
                      'trace_type', trace_type \
                 from batch_native_hook_data_events_trace_alloc_event union all \
               select 'trace_free', \
                      'batch_native_hook_data_events_trace_free_event', \
                      'trace_type', trace_type \
                 from batch_native_hook_data_events_trace_free_event \
             ) \
             select values.kind, values.enum_number, definition.enum_symbol \
             from values left join protobuf_enum_symbol definition \
               on definition.origin_table = values.origin_table \
              and definition.origin_field_path = values.origin_field_path \
              and definition.enum_number = values.enum_number \
             order by values.kind",
        )
        .await,
        json!([
            { "kind": "statistics", "enum_number": 6, "enum_symbol": "GPU_VK" },
            { "kind": "trace_alloc", "enum_number": 5, "enum_symbol": "OTHER" },
            { "kind": "trace_free", "enum_number": 99, "enum_symbol": null },
        ])
    );
}

fn full_native_hook_batches() -> (proto::BatchNativeHookData, proto::BatchNativeHookData) {
    use proto::kat::native_hook::{
        AllocEvent, FilePathMap, FrameMap, FreeEvent, MapsInfo, MemTagEvent, MmapEvent,
        MunmapEvent, NativeHookData, RecordStatisticsEvent, StackMap, SymbolMap, SymbolTable,
        ThreadNameMap, TraceAllocEvent, TraceFreeEvent, TraceType, native_hook_data::Event,
        record_statistics_event::MemoryType,
    };

    let event = |index: u64, event| NativeHookData {
        tv_sec: 100 + index,
        tv_nsec: 200 + index,
        event,
    };
    let first = proto::BatchNativeHookData {
        events: vec![
            event(
                0,
                Some(Event::AllocEvent(AllocEvent {
                    pid: 1000,
                    tid: 1001,
                    addr: 0x1000,
                    size: 64,
                    frame_info: vec![native_hook_frame(10), native_hook_frame(11)],
                    thread_name_id: 12,
                    stack_id: 13,
                })),
            ),
            event(
                1,
                Some(Event::FreeEvent(FreeEvent {
                    pid: 1100,
                    tid: 1101,
                    addr: 0x1100,
                    frame_info: vec![native_hook_frame(20), native_hook_frame(21)],
                    thread_name_id: 22,
                    stack_id: 23,
                })),
            ),
            event(
                2,
                Some(Event::MmapEvent(MmapEvent {
                    pid: 1200,
                    tid: 1201,
                    addr: 0x1200,
                    r#type: "file-backed".to_string(),
                    size: 4096,
                    frame_info: vec![native_hook_frame(30), native_hook_frame(31)],
                    thread_name_id: 32,
                    stack_id: 33,
                })),
            ),
            event(
                3,
                Some(Event::MunmapEvent(MunmapEvent {
                    pid: 1300,
                    tid: 1301,
                    addr: 0x1300,
                    size: 8192,
                    frame_info: vec![native_hook_frame(40), native_hook_frame(41)],
                    thread_name_id: 42,
                    stack_id: 43,
                })),
            ),
            event(
                4,
                Some(Event::TagEvent(MemTagEvent {
                    addr: 0x1400,
                    size: 128,
                    tag: "graphics".to_string(),
                    pid: 1400,
                })),
            ),
            event(
                5,
                Some(Event::FilePath(FilePathMap {
                    id: 51,
                    name: "/system/lib64/libfixture.so".to_string(),
                    pid: 1500,
                })),
            ),
            event(
                6,
                Some(Event::SymbolName(SymbolMap {
                    id: 61,
                    name: "fixture_symbol".to_string(),
                    pid: 1600,
                })),
            ),
            event(
                7,
                Some(Event::ThreadNameMap(ThreadNameMap {
                    id: 71,
                    name: "fixture-thread".to_string(),
                    pid: 1700,
                })),
            ),
            event(
                8,
                Some(Event::MapsInfo(MapsInfo {
                    pid: 1800,
                    start: 0x1800,
                    end: 0x18ff,
                    offset: 24,
                    file_path_id: 81,
                })),
            ),
        ],
    };
    let second = proto::BatchNativeHookData {
        events: vec![
            event(
                9,
                Some(Event::SymbolTab(SymbolTable {
                    file_path_id: 91,
                    text_exec_vaddr: 0x1900,
                    text_exec_vaddr_file_offset: 32,
                    sym_entry_size: 24,
                    sym_table: vec![0x00, 0xff, 0x80],
                    str_table: vec![0xfe, 0x00, 0x7f],
                    pid: 1900,
                })),
            ),
            event(
                10,
                Some(Event::FrameMap(FrameMap {
                    id: 101,
                    frame: None,
                    pid: 2000,
                })),
            ),
            event(
                11,
                Some(Event::FrameMap(FrameMap {
                    id: 111,
                    frame: Some(native_hook_frame(50)),
                    pid: 2100,
                })),
            ),
            event(
                12,
                Some(Event::StackMap(StackMap {
                    id: 121,
                    frame_map_id: vec![501, 502],
                    ip: vec![0x2200, 0x2201, 0x2202],
                    pid: 2200,
                })),
            ),
            event(
                13,
                Some(Event::StatisticsEvent(RecordStatisticsEvent {
                    pid: 2300,
                    callstack_id: 131,
                    r#type: MemoryType::GpuVk as i32,
                    apply_count: 5,
                    release_count: 3,
                    apply_size: 500,
                    release_size: 300,
                    tag_name: "stats".to_string(),
                })),
            ),
            event(
                14,
                Some(Event::TraceAllocEvent(TraceAllocEvent {
                    pid: 2400,
                    tid: 2401,
                    addr: 0x2400,
                    trace_type: TraceType::Other as i32,
                    tag_name: "trace-alloc".to_string(),
                    size: 1024,
                    frame_info: vec![native_hook_frame(60), native_hook_frame(61)],
                    thread_name_id: 142,
                    stack_id: 143,
                })),
            ),
            event(
                15,
                Some(Event::TraceFreeEvent(TraceFreeEvent {
                    pid: 2500,
                    tid: 2501,
                    addr: 0x2500,
                    trace_type: 99,
                    tag_name: "trace-free".to_string(),
                    frame_info: vec![native_hook_frame(70), native_hook_frame(71)],
                    thread_name_id: 152,
                    stack_id: 153,
                })),
            ),
            event(16, None),
        ],
    };
    (first, second)
}

fn native_hook_frame(seed: u64) -> proto::kat::native_hook::Frame {
    proto::kat::native_hook::Frame {
        ip: 10_000 + seed,
        sp: 20_000 + seed,
        symbol_name: format!("symbol-{seed}"),
        file_path: format!("/fixture/{seed}.so"),
        offset: 30_000 + seed,
        symbol_offset: 40_000 + seed,
        symbol_name_id: 50_000 + seed as u32,
        file_path_id: 60_000 + seed as u32,
    }
}

fn full_native_hook_config(clock: &str) -> proto::NativeHookConfig {
    proto::NativeHookConfig {
        pid: 4242,
        save_file: true,
        file_name: "native-hook.htrace".to_string(),
        filter_size: 16,
        smb_pages: 32,
        max_stack_depth: 64,
        process_name: "fixture-process".to_string(),
        malloc_disable: true,
        mmap_disable: true,
        free_stack_report: true,
        munmap_stack_report: true,
        malloc_free_matching_interval: 101,
        malloc_free_matching_cnt: 102,
        string_compressed: true,
        fp_unwind: true,
        blocked: true,
        record_accurately: true,
        startup_mode: true,
        memtrace_enable: true,
        offline_symbolization: true,
        callframe_compress: true,
        statistics_interval: 103,
        clock: clock.to_string(),
        sample_interval: 104,
        response_library_mode: true,
        expand_pids: vec![4242, 4343],
        js_stack_report: 105,
        max_js_stack_depth: 106,
        filter_napi_name: "napi_fixture".to_string(),
        dump_nmd: true,
        target_so_name: "libfixture.so".to_string(),
        restrace_tag: vec!["tag-a".to_string(), "tag-b".to_string()],
    }
}

fn expected_native_hook_event_identity() -> Value {
    let mut rows = Vec::new();
    for index in 0_u64..17 {
        let (parent, repeated_index) = if index < 9 {
            (0, index)
        } else {
            (1, index - 9)
        };
        rows.push(json!({
            "_kat_parent_row_id": parent,
            "_kat_repeated_index": repeated_index,
            "tv_sec": 100 + index,
            "tv_nsec": 200 + index,
        }));
    }
    Value::Array(rows)
}

fn expected_native_hook_membership() -> Value {
    let kinds = [
        Some("alloc_event"),
        Some("free_event"),
        Some("mmap_event"),
        Some("munmap_event"),
        Some("tag_event"),
        Some("file_path"),
        Some("symbol_name"),
        Some("thread_name_map"),
        Some("maps_info"),
        Some("symbol_tab"),
        Some("frame_map"),
        Some("frame_map"),
        Some("stack_map"),
        Some("statistics_event"),
        Some("trace_alloc_event"),
        Some("trace_free_event"),
        None,
    ];
    Value::Array(
        kinds
            .into_iter()
            .enumerate()
            .map(|(event_id, kind)| {
                let (root_id, repeated_index) = if event_id < 9 {
                    (0, event_id)
                } else {
                    (1, event_id - 9)
                };
                json!({
                    "event_id": event_id,
                    "root_id": root_id,
                    "_kat_repeated_index": repeated_index,
                    "kind": kind,
                })
            })
            .collect(),
    )
}

fn expected_native_hook_frames() -> Value {
    let mut rows = Vec::new();
    for (kind, event_id, seeds) in [
        ("alloc", 0, [10, 11]),
        ("free", 1, [20, 21]),
        ("mmap", 2, [30, 31]),
        ("munmap", 3, [40, 41]),
        ("trace_alloc", 14, [60, 61]),
        ("trace_free", 15, [70, 71]),
    ] {
        for (repeated_index, seed) in seeds.into_iter().enumerate() {
            let Value::Object(mut row) = native_hook_frame_json(seed) else {
                unreachable!("Frame serializes as a JSON object")
            };
            row.insert("kind".to_string(), json!(kind));
            row.insert("event_id".to_string(), json!(event_id));
            row.insert("_kat_parent_row_id".to_string(), json!(0));
            row.insert("_kat_repeated_index".to_string(), json!(repeated_index));
            rows.push(Value::Object(row));
        }
    }
    Value::Array(rows)
}

fn native_hook_variant_payloads() -> Vec<(&'static str, Value)> {
    vec![
        (
            "batch_native_hook_data_events_alloc_event",
            json!([{
                "_kat_row_id": 0,
                "_kat_parent_row_id": 0,
                "pid": 1000,
                "tid": 1001,
                "addr": 0x1000,
                "size": 64,
                "thread_name_id": 12,
                "stack_id": 13,
            }]),
        ),
        (
            "batch_native_hook_data_events_free_event",
            json!([{
                "_kat_row_id": 0,
                "_kat_parent_row_id": 1,
                "pid": 1100,
                "tid": 1101,
                "addr": 0x1100,
                "thread_name_id": 22,
                "stack_id": 23,
            }]),
        ),
        (
            "batch_native_hook_data_events_mmap_event",
            json!([{
                "_kat_row_id": 0,
                "_kat_parent_row_id": 2,
                "pid": 1200,
                "tid": 1201,
                "addr": 0x1200,
                "type": "file-backed",
                "size": 4096,
                "thread_name_id": 32,
                "stack_id": 33,
            }]),
        ),
        (
            "batch_native_hook_data_events_munmap_event",
            json!([{
                "_kat_row_id": 0,
                "_kat_parent_row_id": 3,
                "pid": 1300,
                "tid": 1301,
                "addr": 0x1300,
                "size": 8192,
                "thread_name_id": 42,
                "stack_id": 43,
            }]),
        ),
        (
            "batch_native_hook_data_events_tag_event",
            json!([{
                "_kat_parent_row_id": 4,
                "addr": 0x1400,
                "size": 128,
                "tag": "graphics",
                "pid": 1400,
            }]),
        ),
        (
            "batch_native_hook_data_events_file_path",
            json!([{
                "_kat_parent_row_id": 5,
                "id": 51,
                "name": "/system/lib64/libfixture.so",
                "pid": 1500,
            }]),
        ),
        (
            "batch_native_hook_data_events_symbol_name",
            json!([{
                "_kat_parent_row_id": 6,
                "id": 61,
                "name": "fixture_symbol",
                "pid": 1600,
            }]),
        ),
        (
            "batch_native_hook_data_events_thread_name_map",
            json!([{
                "_kat_parent_row_id": 7,
                "id": 71,
                "name": "fixture-thread",
                "pid": 1700,
            }]),
        ),
        (
            "batch_native_hook_data_events_maps_info",
            json!([{
                "_kat_parent_row_id": 8,
                "pid": 1800,
                "start": 0x1800,
                "end": 0x18ff,
                "offset": 24,
                "file_path_id": 81,
            }]),
        ),
        (
            "batch_native_hook_data_events_symbol_tab",
            json!([{
                "_kat_parent_row_id": 9,
                "file_path_id": 91,
                "text_exec_vaddr": 0x1900,
                "text_exec_vaddr_file_offset": 32,
                "sym_entry_size": 24,
                "sym_table": "00ff80",
                "str_table": "fe007f",
                "pid": 1900,
            }]),
        ),
        (
            "batch_native_hook_data_events_frame_map",
            json!([
                {
                    "_kat_parent_row_id": 10,
                    "id": 101,
                    "frame": null,
                    "pid": 2000,
                },
                {
                    "_kat_parent_row_id": 11,
                    "id": 111,
                    "frame": native_hook_frame_json(50),
                    "pid": 2100,
                },
            ]),
        ),
        (
            "batch_native_hook_data_events_stack_map",
            json!([{
                "_kat_row_id": 0,
                "_kat_parent_row_id": 12,
                "id": 121,
                "pid": 2200,
            }]),
        ),
        (
            "batch_native_hook_data_events_statistics_event",
            json!([{
                "_kat_parent_row_id": 13,
                "pid": 2300,
                "callstack_id": 131,
                "type": 6,
                "apply_count": 5,
                "release_count": 3,
                "apply_size": 500,
                "release_size": 300,
                "tag_name": "stats",
            }]),
        ),
        (
            "batch_native_hook_data_events_trace_alloc_event",
            json!([{
                "_kat_row_id": 0,
                "_kat_parent_row_id": 14,
                "pid": 2400,
                "tid": 2401,
                "addr": 0x2400,
                "trace_type": 5,
                "tag_name": "trace-alloc",
                "size": 1024,
                "thread_name_id": 142,
                "stack_id": 143,
            }]),
        ),
        (
            "batch_native_hook_data_events_trace_free_event",
            json!([{
                "_kat_row_id": 0,
                "_kat_parent_row_id": 15,
                "pid": 2500,
                "tid": 2501,
                "addr": 0x2500,
                "trace_type": 99,
                "tag_name": "trace-free",
                "thread_name_id": 152,
                "stack_id": 153,
            }]),
        ),
    ]
}

fn native_hook_frame_json(seed: u64) -> Value {
    serde_json::to_value(native_hook_frame(seed)).expect("Frame serializes to fixture JSON")
}

fn expected_native_hook_config_root(config: &proto::NativeHookConfig) -> Value {
    let Value::Object(mut row) =
        serde_json::to_value(config).expect("NativeHookConfig serializes to fixture JSON")
    else {
        unreachable!("NativeHookConfig serializes as a JSON object")
    };
    row.remove("expand_pids");
    row.remove("restrace_tag");
    row.insert("_kat_row_id".to_string(), json!(0));
    row.insert("_kat_parent_row_id".to_string(), json!(2));
    Value::Array(vec![Value::Object(row)])
}

fn assert_native_hook_physical_schemas(dataset_path: &std::path::Path) {
    use arrow_schema::DataType;

    assert_flat_schema(
        parquet_arrow_schema(dataset_path, "profiler_payload_occurrence").as_ref(),
        &[
            ("_kat_row_id", DataType::UInt64, false),
            ("envelope_name", DataType::Utf8, false),
            ("status", DataType::UInt32, false),
            ("clock_id", DataType::Int32, false),
            ("tv_sec", DataType::UInt64, false),
            ("tv_nsec", DataType::UInt64, false),
            ("version", DataType::Utf8, false),
            ("sample_interval", DataType::UInt32, false),
        ],
    );
    assert_flat_schema(
        parquet_arrow_schema(dataset_path, "protobuf_enum_symbol").as_ref(),
        &[
            ("origin_table", DataType::Utf8, false),
            ("origin_field_path", DataType::Utf8, false),
            ("enum_type_name", DataType::Utf8, false),
            ("enum_number", DataType::Int32, false),
            ("enum_symbol", DataType::Utf8, false),
        ],
    );

    assert_schema_prefix(
        dataset_path,
        "batch_native_hook_data",
        &[
            ("_kat_row_id", DataType::UInt64),
            ("_kat_parent_row_id", DataType::UInt64),
        ],
    );
    assert_schema_prefix(
        dataset_path,
        "batch_native_hook_data_events",
        &[
            ("_kat_row_id", DataType::UInt64),
            ("_kat_parent_row_id", DataType::UInt64),
            ("_kat_repeated_index", DataType::UInt64),
        ],
    );
    assert_schema_prefix(
        dataset_path,
        "native_hook_config",
        &[
            ("_kat_row_id", DataType::UInt64),
            ("_kat_parent_row_id", DataType::UInt64),
        ],
    );
    assert_eq!(
        parquet_arrow_schema(dataset_path, "native_hook_config")
            .fields()
            .len(),
        32,
        "config root is two relation keys plus all 30 scalar fields"
    );

    for table in [
        "batch_native_hook_data_events_alloc_event",
        "batch_native_hook_data_events_free_event",
        "batch_native_hook_data_events_mmap_event",
        "batch_native_hook_data_events_munmap_event",
        "batch_native_hook_data_events_stack_map",
        "batch_native_hook_data_events_trace_alloc_event",
        "batch_native_hook_data_events_trace_free_event",
    ] {
        assert_schema_prefix(
            dataset_path,
            table,
            &[
                ("_kat_row_id", DataType::UInt64),
                ("_kat_parent_row_id", DataType::UInt64),
            ],
        );
    }
    for table in [
        "batch_native_hook_data_events_tag_event",
        "batch_native_hook_data_events_file_path",
        "batch_native_hook_data_events_symbol_name",
        "batch_native_hook_data_events_thread_name_map",
        "batch_native_hook_data_events_maps_info",
        "batch_native_hook_data_events_symbol_tab",
        "batch_native_hook_data_events_frame_map",
        "batch_native_hook_data_events_statistics_event",
    ] {
        let schema = parquet_arrow_schema(dataset_path, table);
        assert_eq!(schema.field(0).name(), "_kat_parent_row_id");
        assert_eq!(schema.field(0).data_type(), &DataType::UInt64);
        assert!(
            schema.field_with_name("_kat_row_id").is_err(),
            "leaf variant {table:?} must not publish a row ID"
        );
    }
    for table in [
        "batch_native_hook_data_events_alloc_event_frame_info",
        "batch_native_hook_data_events_free_event_frame_info",
        "batch_native_hook_data_events_mmap_event_frame_info",
        "batch_native_hook_data_events_munmap_event_frame_info",
        "batch_native_hook_data_events_stack_map_frame_map_id",
        "batch_native_hook_data_events_stack_map_ip",
        "batch_native_hook_data_events_trace_alloc_event_frame_info",
        "batch_native_hook_data_events_trace_free_event_frame_info",
        "native_hook_config_expand_pids",
        "native_hook_config_restrace_tag",
    ] {
        assert_schema_prefix(
            dataset_path,
            table,
            &[
                ("_kat_parent_row_id", DataType::UInt64),
                ("_kat_repeated_index", DataType::UInt64),
            ],
        );
    }

    let symbol_table =
        parquet_arrow_schema(dataset_path, "batch_native_hook_data_events_symbol_tab");
    assert_eq!(
        symbol_table
            .field_with_name("sym_table")
            .unwrap()
            .data_type(),
        &DataType::Binary
    );
    assert_eq!(
        symbol_table
            .field_with_name("str_table")
            .unwrap()
            .data_type(),
        &DataType::Binary
    );

    let frame_map = parquet_arrow_schema(dataset_path, "batch_native_hook_data_events_frame_map");
    let frame = frame_map
        .field_with_name("frame")
        .expect("FrameMap.frame exists");
    assert!(frame.is_nullable(), "FrameMap.frame presence is nullable");
    let DataType::Struct(frame_fields) = frame.data_type() else {
        panic!(
            "FrameMap.frame must be a Struct, got {:?}",
            frame.data_type()
        )
    };
    assert_eq!(frame_fields.len(), 8);
    assert!(
        frame_fields.iter().all(|field| field.is_nullable()),
        "every nested Frame field inherits the optional ancestor presence"
    );
    assert!(
        frame_map
            .fields()
            .iter()
            .filter(|field| field.name() != "frame")
            .all(|field| !field.is_nullable()),
        "FrameMap relation key, id, and pid remain non-null"
    );

    for table in native_hook_relation_names() {
        if table == "batch_native_hook_data_events_frame_map" {
            continue;
        }
        let schema = parquet_arrow_schema(dataset_path, table);
        assert!(
            schema.fields().iter().all(|field| !field.is_nullable()),
            "all physical columns in {table:?} must be non-null"
        );
    }
}

fn assert_schema_prefix(
    dataset_path: &std::path::Path,
    table: &str,
    expected: &[(&str, arrow_schema::DataType)],
) {
    let schema = parquet_arrow_schema(dataset_path, table);
    for (actual, (expected_name, expected_type)) in schema.fields().iter().zip(expected) {
        assert_eq!(
            actual.name(),
            expected_name,
            "unexpected key order in {table:?}"
        );
        assert_eq!(
            actual.data_type(),
            expected_type,
            "unexpected key type in {table:?}"
        );
        assert!(
            !actual.is_nullable(),
            "relation key in {table:?} must be non-null"
        );
    }
}

fn assert_flat_schema(
    schema: &arrow_schema::Schema,
    expected: &[(&str, arrow_schema::DataType, bool)],
) {
    assert_eq!(schema.fields().len(), expected.len());
    for (actual, (expected_name, expected_type, expected_nullable)) in
        schema.fields().iter().zip(expected)
    {
        assert_eq!(actual.name(), expected_name);
        assert_eq!(actual.data_type(), expected_type);
        assert_eq!(actual.is_nullable(), *expected_nullable);
    }
}

fn parquet_arrow_schema(
    dataset_path: &std::path::Path,
    table_name: &str,
) -> arrow_schema::SchemaRef {
    use parquet::arrow::arrow_reader::{ArrowReaderMetadata, ArrowReaderOptions};

    let resolved = crate::resolve_dataset(dataset_path)
        .expect("published Native Hook Dataset resolves for schema inspection");
    let table = resolved
        .tables()
        .iter()
        .find(|table| table.name() == table_name)
        .unwrap_or_else(|| panic!("published Native Hook Dataset has no table {table_name:?}"));
    let file = std::fs::File::open(table.path())
        .unwrap_or_else(|error| panic!("Native Hook table {table_name:?} opens: {error}"));
    ArrowReaderMetadata::load(&file, ArrowReaderOptions::new())
        .expect("Native Hook Parquet metadata loads")
        .schema()
        .clone()
}

fn full_native_hook_table_names() -> std::collections::BTreeSet<&'static str> {
    let mut names = native_hook_relation_names();
    names.extend(["profiler_payload_occurrence", "protobuf_enum_symbol"]);
    names
}

fn native_hook_relation_names() -> std::collections::BTreeSet<&'static str> {
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
    ]
    .into_iter()
    .collect()
}

fn finish_clock_fixture(
    event_clock_ids: &[i32],
    config_clocks: &[&str],
) -> anyhow::Result<protobuf_source::PreparedSourceTables> {
    finish_clock_fixture_with_events(event_clock_ids, config_clocks, true)
}

fn finish_empty_clock_fixture(
    event_clock_ids: &[i32],
    config_clocks: &[&str],
) -> anyhow::Result<protobuf_source::PreparedSourceTables> {
    finish_clock_fixture_with_events(event_clock_ids, config_clocks, false)
}

fn finish_clock_fixture_with_events(
    event_clock_ids: &[i32],
    config_clocks: &[&str],
    has_event_element: bool,
) -> anyhow::Result<protobuf_source::PreparedSourceTables> {
    use formats::hitrace::profiler::{PluginEnvelope, for_each_profiler_envelope_frame};
    use protobuf_source::SpoolOptions;
    use protobuf_source::native_hook::NativeHookSourceCapture;

    let batch = proto::BatchNativeHookData {
        events: has_event_element
            .then_some(proto::kat::native_hook::NativeHookData {
                tv_sec: 7,
                tv_nsec: 8,
                event: None,
            })
            .into_iter()
            .collect(),
    };
    let mut messages = event_clock_ids
        .iter()
        .map(|clock_id| {
            profiler_message_with_provenance(
                "nativehook",
                EnvelopeProvenance {
                    clock_id: *clock_id,
                    ..Default::default()
                },
                batch.encode_to_vec(),
            )
        })
        .collect::<Vec<_>>();
    messages.extend(config_clocks.iter().map(|clock| {
        profiler_message(
            "nativehook_config",
            proto::NativeHookConfig {
                clock: (*clock).to_string(),
                ..Default::default()
            }
            .encode_to_vec(),
        )
    }));
    let frames = profiler_frames(messages);
    let mut capture = NativeHookSourceCapture::new(SpoolOptions::new(2))?;
    for_each_profiler_envelope_frame(&frames, |message, frame_offset| {
        let envelope = PluginEnvelope::from_profiler_plugin_data(&message, 1_024 + frame_offset);
        anyhow::ensure!(
            capture.try_claim(&envelope)?,
            "clock fixture route was not claimed"
        );
        Ok(())
    })?;
    capture.finish()
}

fn profiler_message(name: &str, data: Vec<u8>) -> proto::ProfilerPluginData {
    profiler_message_with_provenance(name, EnvelopeProvenance::default(), data)
}

#[derive(Clone, Copy, Default)]
struct EnvelopeProvenance<'a> {
    status: u32,
    clock_id: i32,
    tv_sec: u64,
    tv_nsec: u64,
    version: &'a str,
    sample_interval: u32,
}

fn profiler_message_with_provenance(
    name: &str,
    provenance: EnvelopeProvenance<'_>,
    data: Vec<u8>,
) -> proto::ProfilerPluginData {
    proto::ProfilerPluginData {
        name: name.to_string(),
        status: provenance.status,
        data,
        clock_id: provenance.clock_id,
        tv_sec: provenance.tv_sec,
        tv_nsec: provenance.tv_nsec,
        version: provenance.version.to_string(),
        sample_interval: provenance.sample_interval,
    }
}

fn profiler_frames(messages: impl IntoIterator<Item = proto::ProfilerPluginData>) -> Vec<u8> {
    let mut bytes = Vec::new();
    for message in messages {
        let frame = message.encode_to_vec();
        bytes.extend_from_slice(&(frame.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&frame);
    }
    bytes
}

fn profiler_section(messages: impl IntoIterator<Item = proto::ProfilerPluginData>) -> Vec<u8> {
    use formats::hitrace::file::{HIPROFILER_PROTOBUF_BIN, PROFILER_HEADER_SIZE};

    const PROFILER_HEADER_MAGIC: u64 = 0x464F_5250_534F_484F;
    let body = profiler_frames(messages);
    let section_len = PROFILER_HEADER_SIZE + body.len();
    let mut section = vec![0; PROFILER_HEADER_SIZE];
    section[0..8].copy_from_slice(&PROFILER_HEADER_MAGIC.to_le_bytes());
    section[8..16].copy_from_slice(&(section_len as u64).to_le_bytes());
    section[56..60].copy_from_slice(&HIPROFILER_PROTOBUF_BIN.to_le_bytes());
    section.extend_from_slice(&body);
    section
}

fn claim_profiler_file(
    capture: &mut protobuf_source::native_hook::NativeHookSourceCapture,
    bytes: &[u8],
) -> anyhow::Result<usize> {
    use formats::hitrace::{
        file::{HIPROFILER_PROTOBUF_BIN, PROFILER_HEADER_SIZE, read_profiler_section},
        profiler::{PluginEnvelope, for_each_profiler_envelope_frame},
    };

    let mut offset = 0;
    let mut claimed = 0;
    while offset < bytes.len() {
        let section = read_profiler_section(bytes, offset)?;
        anyhow::ensure!(
            section.header.data_type == HIPROFILER_PROTOBUF_BIN,
            "fixture profiler section must carry protobuf frames"
        );
        for_each_profiler_envelope_frame(section.body(bytes), |message, frame_offset| {
            let envelope = PluginEnvelope::from_profiler_plugin_data(
                &message,
                section.start + PROFILER_HEADER_SIZE + frame_offset,
            );
            anyhow::ensure!(
                capture.try_claim(&envelope)?,
                "fixture route was not claimed"
            );
            claimed += 1;
            Ok(())
        })?;
        offset = section.end;
    }
    Ok(claimed)
}

fn publish_prepared(
    prepared: protobuf_source::PreparedSourceTables,
    dataset_path: &std::path::Path,
) {
    use dataset_writer::{DatasetWriteTarget, DatasetWriter};

    let mut writer = DatasetWriter::begin(DatasetWriteTarget::write_to_empty(dataset_path))
        .expect("Dataset writer begins after profiler capture preflight");
    prepared
        .write_into(&mut writer)
        .expect("prepared profiler tables write to Dataset");
    writer.finish().expect("Dataset publishes");
}

async fn register_resolved_dataset(
    dataset_path: &std::path::Path,
) -> anyhow::Result<SessionContext> {
    let resolved = crate::resolve_dataset(dataset_path)?;
    let context = SessionContext::new();
    for table in resolved.tables() {
        let url = Url::from_file_path(table.path()).map_err(|()| {
            anyhow::anyhow!(
                "fixture table path cannot be converted to a file URL: {}",
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
        .expect("fixture SQL plans")
        .collect()
        .await
        .expect("fixture SQL executes");
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
        .expect("fixture query batches encode as JSON");
    writer.finish().expect("fixture JSON writer finishes");
    drop(writer);
    serde_json::from_slice(&buffer).expect("fixture query JSON parses")
}
