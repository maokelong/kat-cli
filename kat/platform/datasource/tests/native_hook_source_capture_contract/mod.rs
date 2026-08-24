use arrow_array::RecordBatch;
use arrow_json::writer::{JsonArray, WriterBuilder};
use datafusion::prelude::{ParquetReadOptions, SessionContext};
use prost::Message;
use serde_json::{Value, json};
use tempfile::tempdir;
use url::Url;

use crate::{
    dataset_writer, formats, proto,
    protobuf_source::{self, native_hook as native_hook_source},
};
use proto::kat::hitrace::profiler_plugin_data::ClockId;

#[allow(dead_code)]
#[path = "../native_hook_source_contract/fixture.rs"]
mod native_hook_fixture;
use native_hook_fixture::{
    full_native_hook_batches, full_native_hook_config, full_native_hook_table_names,
    native_hook_frame, native_hook_relation_names, profiler_section,
};

mod real_sample;

struct NativeHookCaptureFixture {
    _directory: tempfile::TempDir,
    dataset_path: std::path::PathBuf,
    publication: dataset_writer::DatasetPublication,
    capture: native_hook_source::NativeHookSourceCapture,
}

impl NativeHookCaptureFixture {
    fn new(options: protobuf_source::BufferOptions) -> anyhow::Result<Self> {
        use dataset_writer::{DatasetPublication, DatasetWriteTarget};

        let directory = tempdir()?;
        let dataset_path = directory.path().join("dataset");
        let publication =
            DatasetPublication::stage(DatasetWriteTarget::write_to_empty(&dataset_path))?;
        let capture =
            native_hook_source::NativeHookSourceCapture::new(options, publication.table_factory())?;
        Ok(Self {
            _directory: directory,
            dataset_path,
            publication,
            capture,
        })
    }

    fn finish(self) -> anyhow::Result<()> {
        self.capture.finish()
    }

    fn publish(self) -> anyhow::Result<PublishedDataset> {
        self.capture.finish()?;
        self.publication.publish()?;
        Ok(PublishedDataset {
            _directory: self._directory,
            path: self.dataset_path,
        })
    }
}

impl std::ops::Deref for NativeHookCaptureFixture {
    type Target = native_hook_source::NativeHookSourceCapture;

    fn deref(&self) -> &Self::Target {
        &self.capture
    }
}

impl std::ops::DerefMut for NativeHookCaptureFixture {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.capture
    }
}

struct PublishedDataset {
    _directory: tempfile::TempDir,
    path: std::path::PathBuf,
}

impl PublishedDataset {
    fn path(&self) -> &std::path::Path {
        &self.path
    }
}

#[tokio::test]
async fn staged_capture_claims_only_exact_routes_and_publishes_empty_roots() {
    use formats::hitrace::profiler::{PluginEnvelope, for_each_profiler_envelope_frame};
    use protobuf_source::BufferOptions;

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
        profiler_message("nativehookx", vec![0xff]),
        profiler_message("hookdaemonx", vec![0xff]),
        profiler_message("nativehook_config_extra", vec![0xff]),
        profiler_message("hookdaemon_config_config", vec![0xff]),
    ]);
    let mut capture = NativeHookCaptureFixture::new(BufferOptions::new(2))
        .expect("staged Native Hook capture is valid");
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
            ("nativehookx".to_string(), false),
            ("hookdaemonx".to_string(), false),
            ("nativehook_config_extra".to_string(), false),
            ("hookdaemon_config_config".to_string(), false),
        ]
    );

    let dataset = capture
        .publish()
        .expect("empty data roots publish without clock admission");
    let dataset_path = dataset.path();
    let resolved = crate::resolve_dataset(dataset_path)
        .expect("formal Dataset resolver accepts the staged capture");
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

    let context = register_resolved_dataset(dataset_path)
        .await
        .expect("staged capture tables register in DataFusion");
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
            "select 'data' as kind, occurrence.envelope_name, root._kat_parent_row_id \
             from batch_native_hook_data root \
             join profiler_payload_occurrence occurrence \
               on root._kat_parent_row_id = occurrence._kat_row_id \
             union all \
             select 'config', occurrence.envelope_name, root._kat_parent_row_id \
             from native_hook_config root \
             join profiler_payload_occurrence occurrence \
               on root._kat_parent_row_id = occurrence._kat_row_id \
             order by _kat_parent_row_id",
        )
        .await,
        json!([
            { "kind": "data", "envelope_name": "nativehook", "_kat_parent_row_id": 0 },
            { "kind": "data", "envelope_name": "hookdaemon", "_kat_parent_row_id": 1 },
            { "kind": "config", "envelope_name": "nativehook_config", "_kat_parent_row_id": 2 },
            { "kind": "config", "envelope_name": "hookdaemon_config", "_kat_parent_row_id": 3 },
        ])
    );
    assert_eq!(
        query_json(
            &context,
            "select origin_table, origin_field_path, enum_type_name, count(*) as symbol_count \
             from protobuf_enum_symbol \
             group by origin_table, origin_field_path, enum_type_name",
        )
        .await,
        json!([{
            "origin_table": "profiler_payload_occurrence",
            "origin_field_path": "clock_id",
            "enum_type_name": "kat.hitrace.ProfilerPluginData.ClockId",
            "symbol_count": 12,
        }])
    );
    assert_eq!(
        query_json(
            &context,
            &format!(
                "select enum_number, enum_symbol from protobuf_enum_symbol \
                 where enum_number in ({}, {}) order by enum_number",
                ClockId::ClockidRealtime as i32,
                ClockId::ClockidBoottime as i32,
            ),
        )
        .await,
        json!([
            {
                "enum_number": ClockId::ClockidRealtime as i32,
                "enum_symbol": "CLOCKID_REALTIME",
            },
            {
                "enum_number": ClockId::ClockidBoottime as i32,
                "enum_symbol": "CLOCKID_BOOTTIME",
            },
        ])
    );
}

#[test]
fn route_match_uses_raw_envelope_name_and_kind_not_derived_plugin_name() {
    use formats::hitrace::profiler::{PluginEnvelope, PluginEnvelopeKind};
    use protobuf_source::BufferOptions;

    let config_payload = proto::NativeHookConfig::default().encode_to_vec();
    let envelope = PluginEnvelope {
        plugin_name: "legacy-derived-wrong",
        envelope_name: "nativehook_config",
        kind: PluginEnvelopeKind::Config,
        payload: &config_payload,
        status: 0,
        clock_id: ClockId::ClockidRealtime as i32,
        tv_sec: 0,
        tv_nsec: 0,
        version: "",
        sample_interval: 0,
        section_start: 1_024,
    };
    let mut capture = NativeHookCaptureFixture::new(BufferOptions::new(2))
        .expect("staged Native Hook capture is valid");
    assert!(
        capture
            .try_claim(&envelope)
            .expect("raw exact route decodes"),
        "route matching must not depend on the derived plugin_name"
    );
    capture.finish().expect("empty/default route closes");

    for (envelope_name, wrong_kind) in [
        ("nativehook", PluginEnvelopeKind::Config),
        ("hookdaemon", PluginEnvelopeKind::Config),
        ("nativehook_config", PluginEnvelopeKind::Data),
        ("hookdaemon_config", PluginEnvelopeKind::Data),
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
        let mut capture = NativeHookCaptureFixture::new(BufferOptions::new(2))
            .expect("staged Native Hook capture is valid");
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
    use protobuf_source::BufferOptions;

    let unbound = profiler_message("nativehook-near", vec![0xff]);
    let mut healthy = NativeHookCaptureFixture::new(BufferOptions::new(2))
        .expect("staged Native Hook capture is valid");
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
    healthy.finish().expect("healthy empty-root capture closes");

    let malformed_bound = profiler_message("nativehook", vec![0xff]);
    let mut poisoned = NativeHookCaptureFixture::new(BufferOptions::new(2))
        .expect("staged Native Hook capture is valid");
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
    use protobuf_source::BufferOptions;

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
    let mut capture = NativeHookCaptureFixture::new(BufferOptions::new(2))
        .expect("staged Native Hook capture is valid");
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
    finish_clock_fixture(&[ClockId::ClockidMonotonic as i32], &["mono"])
        .expect("a supported config may arrive after its event batch");

    let error =
        match finish_clock_fixture(&[ClockId::ClockidMonotonic as i32], &["unsupported-clock"]) {
            Ok(_) => panic!("an unknown config clock must fail admission"),
            Err(error) => error,
        };
    assert!(
        error.to_string().contains("clock"),
        "unknown-clock error must identify the clock contract: {error:#}"
    );
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
    use protobuf_source::BufferOptions;

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

    let mut capture = NativeHookCaptureFixture::new(BufferOptions::with_limits(1, 128))
        .expect("staged Native Hook capture is valid");
    assert_eq!(
        claim_profiler_file(&mut capture, &trace_file).expect("OHOSPROF sections decode and claim"),
        3
    );
    let dataset = capture
        .publish()
        .expect("boot config admits and publishes both eventful data envelopes");
    let dataset_path = dataset.path();
    let resolved = crate::resolve_dataset(dataset_path)
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
    let context = register_resolved_dataset(dataset_path)
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

fn finish_clock_fixture(event_clock_ids: &[i32], config_clocks: &[&str]) -> anyhow::Result<()> {
    finish_clock_fixture_with_events(event_clock_ids, config_clocks, true)
}

fn finish_empty_clock_fixture(
    event_clock_ids: &[i32],
    config_clocks: &[&str],
) -> anyhow::Result<()> {
    finish_clock_fixture_with_events(event_clock_ids, config_clocks, false)
}

fn finish_clock_fixture_with_events(
    event_clock_ids: &[i32],
    config_clocks: &[&str],
    has_event_element: bool,
) -> anyhow::Result<()> {
    use formats::hitrace::profiler::{PluginEnvelope, for_each_profiler_envelope_frame};
    use protobuf_source::BufferOptions;

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
    let mut capture = NativeHookCaptureFixture::new(BufferOptions::new(2))?;
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

fn claim_profiler_file(
    capture: &mut native_hook_source::NativeHookSourceCapture,
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
