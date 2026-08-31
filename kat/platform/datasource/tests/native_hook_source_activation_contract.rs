use std::{collections::BTreeSet, fs, path::Path};

use arrow_array::types::{Int32Type, UInt32Type, UInt64Type};
use arrow_schema::DataType;
use prost::Message;
use tempfile::tempdir;

#[path = "native_hook_source_contract/fixture.rs"]
mod native_hook_fixture;
#[path = "support/mod.rs"]
mod support;
use native_hook_fixture::{
    full_native_hook_batches, full_native_hook_config, full_native_hook_relation_names,
};
use support::{Relation, assert_no_staging, profiler_section};

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

#[test]
fn decode_publishes_native_hook_descriptor_and_clock_relations_flat() {
    let root = tempdir().expect("temporary decode directory is created");
    let source = root.path().join("full-native-hook-topology.htrace");
    let destination = root.path().join("relations");
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

    let report = kat_datasource::decode_hitrace(&source, &destination)
        .expect("decode publishes Native Hook relations");

    assert_eq!(
        report.unsupported_plugins(),
        ["future-plugin", "nativehook-preview"]
    );
    assert_eq!(flat_relation_names(&destination), expected_relation_names());
    assert!(
        fs::read_dir(&destination)
            .expect("relation root can be listed")
            .all(|entry| entry
                .expect("relation entry can be read")
                .path()
                .extension()
                .is_some_and(|extension| extension == "parquet"))
    );

    let occurrences = Relation::open(&destination, "profiler_payload_occurrence");
    assert_eq!(
        occurrences.string_values("envelope_name"),
        [
            Some("nativehook".to_owned()),
            Some("hookdaemon".to_owned()),
            Some("nativehook_config".to_owned()),
            Some("hookdaemon_config".to_owned()),
        ]
    );
    assert_eq!(
        occurrences.primitive_values::<UInt64Type>("_kat_row_id"),
        [Some(0), Some(1), Some(2), Some(3)]
    );
    assert_eq!(
        occurrences.primitive_values::<UInt32Type>("status"),
        [Some(21), Some(22), Some(23), Some(24)]
    );
    assert_eq!(
        occurrences.primitive_values::<Int32Type>("clock_id"),
        [Some(7), Some(7), Some(7), Some(7)]
    );
    assert_eq!(
        occurrences.primitive_values::<UInt64Type>("tv_sec"),
        [Some(121), Some(122), Some(123), Some(124)]
    );
    assert_eq!(
        occurrences.primitive_values::<UInt64Type>("tv_nsec"),
        [Some(221), Some(222), Some(223), Some(224)]
    );
    assert_eq!(
        occurrences.string_values("version"),
        [
            Some("route-21".to_owned()),
            Some("route-22".to_owned()),
            Some("route-23".to_owned()),
            Some("route-24".to_owned()),
        ]
    );
    assert_eq!(
        occurrences.primitive_values::<UInt32Type>("sample_interval"),
        [Some(21), Some(22), Some(23), Some(24)]
    );

    let batches = Relation::open(&destination, "batch_native_hook_data");
    assert_eq!(
        batches.primitive_values::<UInt64Type>("_kat_row_id"),
        [Some(0), Some(1)]
    );
    assert_eq!(
        batches.primitive_values::<UInt64Type>("_kat_parent_row_id"),
        [Some(0), Some(1)]
    );

    let events = Relation::open(&destination, "batch_native_hook_data_events");
    assert_eq!(events.row_count(), 17);
    assert_eq!(
        events.primitive_values::<UInt64Type>("_kat_row_id"),
        (0_u64..17).map(Some).collect::<Vec<_>>()
    );
    assert_eq!(
        events.primitive_values::<UInt64Type>("_kat_parent_row_id"),
        (0..9)
            .map(|_| Some(0))
            .chain((0..8).map(|_| Some(1)))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        events.primitive_values::<UInt64Type>("_kat_repeated_index"),
        (0_u64..9)
            .map(Some)
            .chain((0_u64..8).map(Some))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        events.primitive_values::<UInt64Type>("tv_sec"),
        (100_u64..117).map(Some).collect::<Vec<_>>()
    );
    for (relation, expected_parent_ids) in [
        ("batch_native_hook_data_events_alloc_event", vec![0]),
        ("batch_native_hook_data_events_free_event", vec![1]),
        ("batch_native_hook_data_events_mmap_event", vec![2]),
        ("batch_native_hook_data_events_munmap_event", vec![3]),
        ("batch_native_hook_data_events_tag_event", vec![4]),
        ("batch_native_hook_data_events_file_path", vec![5]),
        ("batch_native_hook_data_events_symbol_name", vec![6]),
        ("batch_native_hook_data_events_thread_name_map", vec![7]),
        ("batch_native_hook_data_events_maps_info", vec![8]),
        ("batch_native_hook_data_events_symbol_tab", vec![9]),
        ("batch_native_hook_data_events_frame_map", vec![10, 11]),
        ("batch_native_hook_data_events_stack_map", vec![12]),
        ("batch_native_hook_data_events_statistics_event", vec![13]),
        ("batch_native_hook_data_events_trace_alloc_event", vec![14]),
        ("batch_native_hook_data_events_trace_free_event", vec![15]),
    ] {
        assert_eq!(
            Relation::open(&destination, relation)
                .primitive_values::<UInt64Type>("_kat_parent_row_id"),
            expected_parent_ids
                .into_iter()
                .map(Some)
                .collect::<Vec<_>>(),
            "oneof parentage drifted in {relation}"
        );
    }

    let alloc = Relation::open(&destination, "batch_native_hook_data_events_alloc_event");
    assert_eq!(alloc.primitive_values::<Int32Type>("pid"), [Some(1000)]);
    assert_eq!(alloc.primitive_values::<UInt64Type>("size"), [Some(64)]);
    let mmap = Relation::open(&destination, "batch_native_hook_data_events_mmap_event");
    assert_eq!(mmap.string_values("type"), [Some("file-backed".to_owned())]);
    let file_path = Relation::open(&destination, "batch_native_hook_data_events_file_path");
    assert_eq!(
        file_path.string_values("name"),
        [Some("/system/lib64/libfixture.so".to_owned())]
    );
    let maps = Relation::open(&destination, "batch_native_hook_data_events_maps_info");
    assert_eq!(maps.primitive_values::<UInt64Type>("start"), [Some(0x1800)]);
    assert_eq!(maps.primitive_values::<UInt64Type>("end"), [Some(0x18ff)]);
    let frame_map_ids = Relation::open(
        &destination,
        "batch_native_hook_data_events_stack_map_frame_map_id",
    );
    assert_eq!(
        frame_map_ids.primitive_values::<UInt64Type>("_kat_repeated_index"),
        [Some(0), Some(1)]
    );
    assert_eq!(
        frame_map_ids.primitive_values::<UInt64Type>("value"),
        [Some(501), Some(502)]
    );
    let stack_ips = Relation::open(&destination, "batch_native_hook_data_events_stack_map_ip");
    assert_eq!(
        stack_ips.primitive_values::<UInt64Type>("value"),
        [Some(0x2200), Some(0x2201), Some(0x2202)]
    );

    let symbol_table = Relation::open(&destination, "batch_native_hook_data_events_symbol_tab");
    let sym_table_field = symbol_table
        .schema()
        .field_with_name("sym_table")
        .expect("symbol table schema has sym_table");
    assert_eq!(sym_table_field.data_type(), &DataType::Binary);
    assert!(!sym_table_field.is_nullable());
    assert_eq!(
        symbol_table.primitive_values::<UInt64Type>("_kat_parent_row_id"),
        [Some(9)]
    );
    assert_eq!(
        symbol_table.binary_values("sym_table"),
        [Some(vec![0x00, 0xff, 0x80])]
    );
    assert_eq!(
        symbol_table.binary_values("str_table"),
        [Some(vec![0xfe, 0x00, 0x7f])]
    );

    let frame_maps = Relation::open(&destination, "batch_native_hook_data_events_frame_map");
    assert_eq!(frame_maps.row_count(), 2);
    let frame_field = frame_maps
        .schema()
        .field_with_name("frame")
        .expect("frame_map schema has frame");
    assert!(frame_field.is_nullable());
    let DataType::Struct(frame_children) = frame_field.data_type() else {
        panic!("frame must remain an Arrow Struct")
    };
    assert!(frame_children.iter().all(|child| child.is_nullable()));
    assert_eq!(frame_maps.struct_nulls("frame"), [true, false]);
    assert_eq!(
        frame_maps.struct_primitive_values::<UInt64Type>("frame", "ip"),
        [None, Some(10_050)]
    );

    let configs = Relation::open(&destination, "native_hook_config");
    assert_eq!(
        configs.primitive_values::<UInt64Type>("_kat_row_id"),
        [Some(0), Some(1)]
    );
    assert_eq!(
        configs.primitive_values::<UInt64Type>("_kat_parent_row_id"),
        [Some(2), Some(3)]
    );
    assert_eq!(
        configs.boolean_values("save_file"),
        [Some(true), Some(true)]
    );
    assert_eq!(
        configs.string_values("clock"),
        [Some("boot".to_owned()), Some("boot".to_owned())]
    );
    let expand_pids = Relation::open(&destination, "native_hook_config_expand_pids");
    assert_eq!(
        expand_pids.primitive_values::<UInt64Type>("_kat_parent_row_id"),
        [Some(0), Some(0), Some(1), Some(1)]
    );
    assert_eq!(
        expand_pids.primitive_values::<UInt64Type>("_kat_repeated_index"),
        [Some(0), Some(1), Some(0), Some(1)]
    );
    assert_eq!(
        expand_pids.primitive_values::<Int32Type>("value"),
        [Some(4242), Some(4343), Some(4242), Some(4343)]
    );

    let statistics = Relation::open(
        &destination,
        "batch_native_hook_data_events_statistics_event",
    );
    let statistics_type = statistics
        .schema()
        .field_with_name("type")
        .expect("statistics schema has type");
    assert_eq!(statistics_type.data_type(), &DataType::Int32);
    assert!(!statistics_type.is_nullable());
    assert_eq!(statistics.primitive_values::<Int32Type>("type"), [Some(6)]);
    let trace_free = Relation::open(
        &destination,
        "batch_native_hook_data_events_trace_free_event",
    );
    assert_eq!(
        trace_free.primitive_values::<Int32Type>("trace_type"),
        [Some(99)]
    );
    let symbols = Relation::open(&destination, "protobuf_enum_symbol");
    let origin_tables = symbols.string_values("origin_table");
    let origin_fields = symbols.string_values("origin_field_path");
    let enum_numbers = symbols.primitive_values::<Int32Type>("enum_number");
    let enum_symbols = symbols.string_values("enum_symbol");
    assert!((0..symbols.row_count()).any(|index| {
        origin_tables[index].as_deref() == Some("batch_native_hook_data_events_statistics_event")
            && origin_fields[index].as_deref() == Some("type")
            && enum_numbers[index] == Some(6)
            && enum_symbols[index].as_deref() == Some("GPU_VK")
    }));
    assert!(!(0..symbols.row_count()).any(|index| {
        origin_tables[index].as_deref() == Some("batch_native_hook_data_events_trace_free_event")
            && origin_fields[index].as_deref() == Some("trace_type")
            && enum_numbers[index] == Some(99)
    }));

    let clock_domains = Relation::open(&destination, "clock_domain");
    assert!(clock_domains.row_count() > 0);
    assert!(
        clock_domains
            .primitive_values::<UInt64Type>("ticks_per_second")
            .into_iter()
            .all(|value| value == Some(1_000_000_000))
    );
    assert_eq!(
        Relation::open(&destination, "clock_snapshot").row_count(),
        6
    );
}

#[test]
fn decode_publishes_empty_default_native_hook_roots_without_child_relations() {
    let root = tempdir().expect("temporary decode directory is created");
    let source = root.path().join("empty-native-hook.htrace");
    let destination = root.path().join("relations");
    fs::write(
        &source,
        profiler_section([
            profiler_envelope(
                "nativehook",
                1,
                1,
                proto::kat::native_hook::BatchNativeHookData::default().encode_to_vec(),
            ),
            profiler_envelope(
                "nativehook_config",
                2,
                1,
                proto::kat::native_hook::NativeHookConfig::default().encode_to_vec(),
            ),
        ]),
    )
    .expect("empty/default Native Hook fixture is written");

    kat_datasource::decode_hitrace(&source, &destination)
        .expect("empty/default roots remain valid typed payloads");

    let occurrences = Relation::open(&destination, "profiler_payload_occurrence");
    assert_eq!(
        occurrences.primitive_values::<UInt64Type>("_kat_row_id"),
        [Some(0), Some(1)]
    );
    assert_eq!(
        occurrences.string_values("envelope_name"),
        [
            Some("nativehook".to_owned()),
            Some("nativehook_config".to_owned()),
        ]
    );
    let batches = Relation::open(&destination, "batch_native_hook_data");
    assert_eq!(batches.row_count(), 1);
    assert_eq!(
        batches.primitive_values::<UInt64Type>("_kat_row_id"),
        [Some(0)]
    );
    assert_eq!(
        batches.primitive_values::<UInt64Type>("_kat_parent_row_id"),
        [Some(0)]
    );
    let configs = Relation::open(&destination, "native_hook_config");
    assert_eq!(configs.row_count(), 1);
    assert_eq!(
        configs.primitive_values::<UInt64Type>("_kat_row_id"),
        [Some(0)]
    );
    assert_eq!(
        configs.primitive_values::<UInt64Type>("_kat_parent_row_id"),
        [Some(1)]
    );
    assert!(
        !destination
            .join("batch_native_hook_data_events.parquet")
            .exists()
    );
    assert!(
        !destination
            .join("native_hook_config_expand_pids.parquet")
            .exists()
    );
}

#[test]
fn native_hook_clock_contract_failures_leave_no_destination_or_staging() {
    let root = tempdir().expect("temporary decode directory is created");
    let batch = proto::kat::native_hook::BatchNativeHookData {
        events: vec![proto::kat::native_hook::NativeHookData {
            tv_sec: 7,
            tv_nsec: 8,
            event: None,
        }],
    };
    let cases = [
        (
            "missing-config",
            vec![profiler_envelope(
                "nativehook",
                1,
                7,
                batch.clone().encode_to_vec(),
            )],
            "require a Native Hook config clock",
        ),
        (
            "unsupported-config",
            vec![
                profiler_envelope("nativehook", 1, 7, batch.clone().encode_to_vec()),
                profiler_envelope(
                    "nativehook_config",
                    2,
                    7,
                    full_native_hook_config("unsupported-clock").encode_to_vec(),
                ),
            ],
            "unsupported Native Hook config clock",
        ),
        (
            "conflicting-config",
            vec![
                profiler_envelope("nativehook", 1, 7, batch.clone().encode_to_vec()),
                profiler_envelope(
                    "nativehook_config",
                    2,
                    7,
                    full_native_hook_config("boot").encode_to_vec(),
                ),
                profiler_envelope(
                    "hookdaemon_config",
                    3,
                    7,
                    full_native_hook_config("mono").encode_to_vec(),
                ),
            ],
            "conflicting Native Hook config clocks",
        ),
        (
            "mismatched-envelope",
            vec![
                profiler_envelope("nativehook", 1, 1, batch.encode_to_vec()),
                profiler_envelope(
                    "nativehook_config",
                    2,
                    7,
                    full_native_hook_config("boot").encode_to_vec(),
                ),
            ],
            "expects profiler envelope clock_id 7",
        ),
    ];

    for (case, envelopes, expected) in cases {
        let source = root.path().join(format!("{case}.htrace"));
        let destination = root.path().join(format!("{case}-relations"));
        fs::write(&source, profiler_section(envelopes)).expect("clock fixture is written");

        let error = kat_datasource::decode_hitrace(&source, &destination)
            .expect_err("invalid Native Hook clock contract is rejected");

        assert!(
            error.to_string().contains(expected),
            "unexpected {case} error: {error:#}"
        );
        assert!(!destination.exists(), "{case} must not publish output");
        assert_no_staging(root.path());
    }
}

#[test]
fn native_hook_event_values_survive_the_default_flush_boundary() {
    const FIRST_BATCH_EVENTS: usize = 8_193;
    const SECOND_BATCH_EVENTS: usize = 2;

    let root = tempdir().expect("temporary decode directory is created");
    let source = root.path().join("native-hook-flush.htrace");
    let destination = root.path().join("relations");
    let event = |index: usize| {
        use proto::kat::native_hook::{MemTagEvent, NativeHookData, native_hook_data::Event};

        NativeHookData {
            tv_sec: index as u64,
            tv_nsec: 10_000 + index as u64,
            event: Some(Event::TagEvent(MemTagEvent {
                addr: index as u64,
                size: 64,
                tag: format!("tag-{index}"),
                pid: 42,
            })),
        }
    };
    let first_batch = proto::kat::native_hook::BatchNativeHookData {
        events: (0..FIRST_BATCH_EVENTS).map(event).collect(),
    };
    let second_batch = proto::kat::native_hook::BatchNativeHookData {
        events: (FIRST_BATCH_EVENTS..FIRST_BATCH_EVENTS + SECOND_BATCH_EVENTS)
            .map(event)
            .collect(),
    };
    fs::write(
        &source,
        profiler_section([
            profiler_envelope("nativehook", 1, 7, first_batch.encode_to_vec()),
            profiler_envelope("hookdaemon", 2, 7, second_batch.encode_to_vec()),
            profiler_envelope(
                "nativehook_config",
                3,
                7,
                full_native_hook_config("boot").encode_to_vec(),
            ),
        ]),
    )
    .expect("cross-flush Native Hook fixture is written");

    kat_datasource::decode_hitrace(&source, &destination)
        .expect("values remain valid across the default flush boundary");

    let events = Relation::open(&destination, "batch_native_hook_data_events");
    assert_eq!(events.row_count(), FIRST_BATCH_EVENTS + SECOND_BATCH_EVENTS);
    assert_eq!(
        events
            .schema()
            .fields()
            .iter()
            .map(|field| (
                field.name().as_str(),
                field.data_type().clone(),
                field.is_nullable()
            ))
            .collect::<Vec<_>>(),
        [
            ("_kat_row_id", DataType::UInt64, false),
            ("_kat_parent_row_id", DataType::UInt64, false),
            ("_kat_repeated_index", DataType::UInt64, false),
            ("tv_sec", DataType::UInt64, false),
            ("tv_nsec", DataType::UInt64, false),
        ]
    );
    let row_ids = events.primitive_values::<UInt64Type>("_kat_row_id");
    let parent_ids = events.primitive_values::<UInt64Type>("_kat_parent_row_id");
    let repeated = events.primitive_values::<UInt64Type>("_kat_repeated_index");
    let seconds = events.primitive_values::<UInt64Type>("tv_sec");
    let nanoseconds = events.primitive_values::<UInt64Type>("tv_nsec");
    assert_eq!(
        row_ids,
        (0_u64..FIRST_BATCH_EVENTS as u64 + SECOND_BATCH_EVENTS as u64)
            .map(Some)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        parent_ids,
        (0..FIRST_BATCH_EVENTS)
            .map(|_| Some(0))
            .chain((0..SECOND_BATCH_EVENTS).map(|_| Some(1)))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        repeated,
        (0_u64..FIRST_BATCH_EVENTS as u64)
            .map(Some)
            .chain((0_u64..SECOND_BATCH_EVENTS as u64).map(Some))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        seconds,
        (0_u64..FIRST_BATCH_EVENTS as u64 + SECOND_BATCH_EVENTS as u64)
            .map(Some)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        nanoseconds,
        (10_000_u64..10_000 + FIRST_BATCH_EVENTS as u64 + SECOND_BATCH_EVENTS as u64)
            .map(Some)
            .collect::<Vec<_>>()
    );

    let tags = Relation::open(&destination, "batch_native_hook_data_events_tag_event");
    assert_eq!(tags.row_count(), FIRST_BATCH_EVENTS + SECOND_BATCH_EVENTS);
    assert_eq!(
        tags.schema()
            .fields()
            .iter()
            .map(|field| (
                field.name().as_str(),
                field.data_type().clone(),
                field.is_nullable()
            ))
            .collect::<Vec<_>>(),
        [
            ("_kat_parent_row_id", DataType::UInt64, false),
            ("addr", DataType::UInt64, false),
            ("size", DataType::UInt64, false),
            ("tag", DataType::Utf8, false),
            ("pid", DataType::Int32, false),
        ]
    );
    assert_eq!(
        tags.primitive_values::<UInt64Type>("_kat_parent_row_id"),
        (0_u64..FIRST_BATCH_EVENTS as u64 + SECOND_BATCH_EVENTS as u64)
            .map(Some)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        tags.primitive_values::<UInt64Type>("addr"),
        (0_u64..FIRST_BATCH_EVENTS as u64 + SECOND_BATCH_EVENTS as u64)
            .map(Some)
            .collect::<Vec<_>>()
    );
    assert!(
        tags.primitive_values::<UInt64Type>("size")
            .into_iter()
            .all(|value| value == Some(64))
    );
    assert!(
        tags.primitive_values::<Int32Type>("pid")
            .into_iter()
            .all(|value| value == Some(42))
    );
    assert_eq!(
        tags.string_values("tag"),
        (0..FIRST_BATCH_EVENTS + SECOND_BATCH_EVENTS)
            .map(|index| Some(format!("tag-{index}")))
            .collect::<Vec<_>>()
    );
}

fn flat_relation_names(root: &Path) -> BTreeSet<String> {
    fs::read_dir(root)
        .expect("flat relation root can be listed")
        .map(|entry| {
            entry
                .expect("relation entry can be read")
                .path()
                .file_stem()
                .and_then(|name| name.to_str())
                .expect("relation name is Unicode")
                .to_owned()
        })
        .collect()
}

fn expected_relation_names() -> BTreeSet<String> {
    let mut names = full_native_hook_relation_names()
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
