use std::{fs, path::Path};

use arrow_array::RecordBatch;
use arrow_json::writer::{JsonArray, WriterBuilder};
use datafusion::prelude::{ParquetReadOptions, SessionContext};
use kat_datasource::{DatasetWriteTarget, TraceDatasource};
use prost::Message;
use serde_json::{Value, json};
use tempfile::tempdir;
use url::Url;

#[allow(dead_code)]
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

use proto::kat::{
    hitrace::{ProfilerPluginData, profiler_plugin_data},
    native_hook::{
        AllocEvent, BatchNativeHookData, NativeHookConfig, NativeHookData, native_hook_data,
    },
};

#[tokio::test]
async fn formal_activation_publishes_all_four_native_hook_routes_and_isolates_unknown_content() {
    let root = tempdir().expect("tempdir");
    let source = root.path().join("mixed-activation.htrace");
    let dataset = root.path().join("dataset");
    fs::write(&source, mixed_trace()).expect("mixed OHOSPROF fixture is written");

    let mut observed = Vec::new();
    let imported = kat_datasource::import_hitrace(
        &source,
        DatasetWriteTarget::write_to_empty(&dataset),
        |content| {
            observed.push((
                content.kind().to_owned(),
                content.value().to_owned(),
                content.byte_offset(),
            ));
            Ok(())
        },
    )
    .expect("formal Hitrace import succeeds");

    assert_eq!(
        imported.unsupported_plugins(),
        ["future-plugin", "nativehook-preview"]
    );
    assert_eq!(
        observed
            .iter()
            .map(|(kind, value, _)| (kind.as_str(), value.as_str()))
            .collect::<Vec<_>>(),
        [
            ("plugin", "nativehook-preview"),
            ("plugin", "future-plugin"),
        ],
        "all four bound Native Hook routes must not be reported as unknown"
    );
    assert!(
        observed[0].2 < observed[1].2,
        "observer preserves source order"
    );

    let table_names = kat_datasource::inspect_dataset(&dataset)
        .expect("formal Dataset is inspectable")
        .tables()
        .iter()
        .map(|table| table.name().to_owned())
        .collect::<Vec<_>>();
    for required in [
        "clock_domain",
        "clock_snapshot",
        "profiler_payload_occurrence",
        "batch_native_hook_data",
        "batch_native_hook_data_events",
        "batch_native_hook_data_events_alloc_event",
        "native_hook_config",
    ] {
        assert!(
            table_names.iter().any(|name| name == required),
            "formal activation must publish {required:?}; actual tables: {table_names:?}"
        );
    }
    // 这里只锁定正式 Dataset 表面；旧 decoder 绕过由 claimant/registry 私有 spy 直接证明。
    for legacy_projection in [
        "native_hook_alloc",
        "native_hook_free",
        "profiler_plugin_data",
        "sched_switch",
    ] {
        assert!(
            !table_names.iter().any(|name| name == legacy_projection),
            "formal import must not publish legacy projection {legacy_projection:?}"
        );
    }

    let context = register_resolved_dataset(&dataset)
        .await
        .expect("formal Dataset resolves into DataFusion");
    assert_eq!(
        query_json(
            &context,
            "select envelope_name, status, clock_id \
             from profiler_payload_occurrence order by _kat_row_id",
        )
        .await,
        json!([
            {"envelope_name": "nativehook", "status": 1, "clock_id": 7},
            {"envelope_name": "nativehook_config", "status": 0, "clock_id": 7},
            {"envelope_name": "hookdaemon", "status": 1, "clock_id": 7},
            {"envelope_name": "hookdaemon_config", "status": 0, "clock_id": 7},
        ])
    );
    assert_eq!(
        query_json(
            &context,
            "select pid, process_name, clock from native_hook_config order by pid",
        )
        .await,
        json!([
            {"pid": 42, "process_name": "render", "clock": "boot"},
            {"pid": 84, "process_name": "compositor", "clock": "boot"},
        ])
    );
    assert_eq!(
        query_json(
            &context,
            "select pid, tid, addr, size \
             from batch_native_hook_data_events_alloc_event order by pid",
        )
        .await,
        json!([
            {"pid": 42, "tid": 43, "addr": 4096, "size": 64},
            {"pid": 84, "tid": 85, "addr": 8192, "size": 128},
        ])
    );
}

#[tokio::test]
async fn legacy_query_and_materialize_keep_native_hook_projection_values() {
    let root = tempdir().expect("tempdir");
    let source = root.path().join("legacy-native-hook.htrace");
    let dataset = root.path().join("legacy-dataset");
    fs::write(&source, mixed_trace()).expect("mixed OHOSPROF fixture is written");

    let direct = TraceDatasource::from_hitrace(&source).expect("legacy in-memory decode succeeds");
    kat_datasource::materialize_hitrace_dataset(&source, &dataset)
        .await
        .expect("legacy materialization succeeds");
    let materialized = TraceDatasource::from_dataset(&dataset)
        .await
        .expect("legacy materialized Dataset resolves");

    let config_sql = "select pid, process_name from native_hook_config order by pid";
    let expected_configs = json!([
        {"pid": 42, "process_name": "render"},
        {"pid": 84, "process_name": "compositor"},
    ]);
    assert_eq!(
        direct
            .query_json(config_sql)
            .await
            .expect("legacy in-memory configs are queryable"),
        expected_configs
    );
    assert_eq!(
        materialized
            .query_json(config_sql)
            .await
            .expect("legacy materialized configs are queryable"),
        expected_configs
    );

    let alloc_sql =
        "select tv_sec, tv_nsec, pid, tid, addr, size from native_hook_alloc order by pid";
    let expected_allocations = json!([
        {"tv_sec": 7, "tv_nsec": 8, "pid": 42, "tid": 43, "addr": 4096, "size": 64},
        {"tv_sec": 9, "tv_nsec": 10, "pid": 84, "tid": 85, "addr": 8192, "size": 128},
    ]);
    assert_eq!(
        direct
            .query_json(alloc_sql)
            .await
            .expect("legacy in-memory allocations are queryable"),
        expected_allocations
    );
    assert_eq!(
        materialized
            .query_json(alloc_sql)
            .await
            .expect("legacy materialized allocations are queryable"),
        expected_allocations
    );
}

fn mixed_trace() -> Vec<u8> {
    profiler_section([
        profiler_envelope("nativehook", native_hook_batch(42, 43, 7, 8, 0x1000, 64)),
        profiler_envelope("nativehook-preview", vec![0xff, 0x00]),
        profiler_envelope(
            "nativehook_config",
            native_hook_config(42, "render").encode_to_vec(),
        ),
        profiler_envelope("hookdaemon", native_hook_batch(84, 85, 9, 10, 0x2000, 128)),
        profiler_envelope(
            "hookdaemon_config",
            native_hook_config(84, "compositor").encode_to_vec(),
        ),
        profiler_envelope("future-plugin_config", vec![0x80]),
    ])
}

fn native_hook_config(pid: i32, process_name: &str) -> NativeHookConfig {
    NativeHookConfig {
        pid,
        process_name: process_name.to_owned(),
        clock: "boot".to_owned(),
        ..Default::default()
    }
}

fn native_hook_batch(
    pid: i32,
    tid: i32,
    tv_sec: u64,
    tv_nsec: u64,
    addr: u64,
    size: u64,
) -> Vec<u8> {
    BatchNativeHookData {
        events: vec![NativeHookData {
            tv_sec,
            tv_nsec,
            event: Some(native_hook_data::Event::AllocEvent(AllocEvent {
                pid,
                tid,
                addr,
                size,
                frame_info: Vec::new(),
                thread_name_id: 9,
                stack_id: 10,
            })),
        }],
    }
    .encode_to_vec()
}

fn profiler_envelope(name: &str, data: Vec<u8>) -> ProfilerPluginData {
    ProfilerPluginData {
        name: name.to_owned(),
        status: u32::from(!name.ends_with("_config")),
        data,
        clock_id: profiler_plugin_data::ClockId::ClockidBoottime as i32,
        tv_sec: 10,
        tv_nsec: 20,
        version: "1.0".to_owned(),
        sample_interval: 10,
    }
}

async fn register_resolved_dataset(dataset: &Path) -> anyhow::Result<SessionContext> {
    let resolved = kat_datasource::resolve_dataset(dataset)?;
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
