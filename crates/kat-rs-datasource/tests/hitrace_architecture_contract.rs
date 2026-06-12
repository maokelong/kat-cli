use std::fs;

fn source(path: &str) -> String {
    fs::read_to_string(format!("{}/{}", env!("CARGO_MANIFEST_DIR"), path))
        .unwrap_or_else(|error| panic!("{path} can be read: {error}"))
}

fn joined_sources(paths: &[&str]) -> String {
    paths
        .iter()
        .map(|path| source(path))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn datasource_uses_reviewer_layer_boundaries() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");

    for path in [
        "src/formats/mod.rs",
        "src/formats/hitrace/mod.rs",
        "src/formats/hitrace/file.rs",
        "src/formats/hitrace/profiler.rs",
        "src/formats/hitrace/segment.rs",
        "src/domains/mod.rs",
        "src/domains/ftrace/mod.rs",
        "src/domains/ftrace/event.rs",
        "src/domains/ftrace/packet.rs",
        "src/sinks/mod.rs",
        "src/sinks/arrow/mod.rs",
        "src/sinks/arrow/table_builder.rs",
        "src/catalog.rs",
    ] {
        assert!(
            std::path::Path::new(&format!("{manifest_dir}/{path}")).exists(),
            "{path} should exist"
        );
    }

    for path in ["src/hitrace.rs", "src/ftrace.rs", "src/ftrace/mod.rs"] {
        assert!(
            !std::path::Path::new(&format!("{manifest_dir}/{path}")).exists(),
            "{path} should not remain as a datasource center"
        );
    }
}

#[test]
fn hitrace_format_adapter_does_not_decode_ftrace_or_write_arrow() {
    let hitrace_sources = joined_sources(&[
        "src/formats/hitrace/mod.rs",
        "src/formats/hitrace/file.rs",
        "src/formats/hitrace/profiler.rs",
        "src/formats/hitrace/segment.rs",
    ]);

    assert!(hitrace_sources.contains("TraceRecordSink"));
    assert!(hitrace_sources.contains("decode_plugin_payload"));
    assert!(hitrace_sources.contains("for_each_len_prefixed_message::<ProfilerPluginData, _>"));

    for marker in [
        "TracePluginResult::decode",
        "SchedDirectTableBuilders",
        "ArrayBuilder",
        "RecordBatch",
        "MemTable",
        "thread_state",
        "sched_slice",
        "raw_event",
    ] {
        assert!(
            !hitrace_sources.contains(marker),
            "{marker} should not live in formats/hitrace"
        );
    }
}

#[test]
fn ftrace_domain_decodes_payload_to_neutral_records() {
    let ftrace_sources = joined_sources(&[
        "src/domains/ftrace/mod.rs",
        "src/domains/ftrace/event.rs",
        "src/domains/ftrace/packet.rs",
    ]);

    assert!(ftrace_sources.contains("TracePluginResult::decode"));
    assert!(ftrace_sources.contains("TraceRecord::FtraceEvent"));
    assert!(ftrace_sources.contains("FtraceEventRecord::new"));
    assert!(ftrace_sources.contains("pub(crate) const FTRACE_PLUGIN_NAME"));

    for marker in [
        "SchedDirectTableBuilders",
        "DirectEventTableBuilder",
        "ArrayBuilder",
        "RecordBatch",
    ] {
        assert!(
            !ftrace_sources.contains(marker),
            "{marker} should not live in domains/ftrace"
        );
    }
}

#[test]
fn arrow_sink_owns_record_to_table_conversion() {
    let sink_mod = source("src/sinks/arrow/mod.rs");
    let sink_table_builder = source("src/sinks/arrow/table_builder.rs");
    let catalog = source("src/catalog.rs");

    assert!(catalog.contains("pub(crate) enum TraceRecord"));
    assert!(catalog.contains("pub(crate) trait TraceRecordSink"));
    assert!(catalog.contains("pub(crate) struct TraceDataset"));
    assert!(catalog.contains("pub(crate) struct TraceTable"));

    assert!(sink_mod.contains("impl TraceRecordSink for ArrowSink"));
    assert!(sink_mod.contains("FtraceEventTableBuilders::new()?"));
    assert!(!sink_mod.contains("SchedDirectTableBuilders"));
    assert!(sink_mod.contains("TraceDataset::new(tables)"));
    assert!(sink_table_builder.contains("ArrayBuilder"));
    assert!(sink_table_builder.contains("pub(crate) struct DirectEventTableBuilder"));
    assert!(sink_table_builder.contains("pub(crate) struct EventMeta"));
}

#[test]
fn query_consumes_trace_dataset_catalog() {
    let query = source("src/query.rs");

    assert!(query.contains("TraceDataset"));
    assert!(query.contains("register_dataset(&ctx, sink.finish()?)?"));
    assert!(query.contains("hitrace::decode_file(path.as_ref(), &mut sink)?"));
    assert!(query.contains("ArrowSink::new()?"));

    for marker in ["load_hitrace_tables", "HITRACE_TABLE", "FtraceTables"] {
        assert!(
            !query.contains(marker),
            "{marker} should not be consumed directly by query"
        );
    }
}

#[test]
fn sched_generation_uses_arrow_sink_and_ftrace_records() {
    let build_rs = source("build.rs");
    let generated_builders = fs::read_to_string(format!(
        "{}/ftrace_event_table_builders.rs",
        env!("OUT_DIR")
    ))
    .expect("generated ftrace event table builders can be read");

    for marker in [
        "struct EventFamilySpec",
        "const FTRACE_EVENT_FAMILIES: &[EventFamilySpec]",
        "generate_ftrace_event_table_builders(&event_families)",
        "FtraceEventTableBuilders",
        "SchedEventFamilyTables",
        "domains::ftrace::FtraceEventRecord",
        "sinks::arrow::{DirectEventTableBuilder, EventMeta}",
        "catalog::TraceTable",
    ] {
        assert!(build_rs.contains(marker), "{marker} should exist");
        assert!(
            generated_builders.contains(marker)
                || !matches!(
                    marker,
                    "domains::ftrace::FtraceEventRecord"
                        | "sinks::arrow::{DirectEventTableBuilder, EventMeta}"
                        | "catalog::TraceTable"
                ),
            "{marker} should exist in generated builders when it is a generated import"
        );
    }

    assert!(
        generated_builders.contains(
            "pub(crate) fn push_event(&mut self, record: FtraceEventRecord) -> Result<()>"
        )
    );
    assert!(generated_builders.contains("self.sched.push_event(&record)?;"));
    assert!(generated_builders.contains("fn push_event(&mut self, record: &FtraceEventRecord)"));
    assert!(generated_builders.contains("let meta = EventMeta::from_record(record);"));
    assert!(generated_builders.contains("let event = &record.event;"));
    assert!(generated_builders.contains("event.sched_switch_format.clone()"));
    assert!(generated_builders.contains("self.sched_switch.push(meta.clone(), message)?"));

    for marker in [
        "crate::ftrace",
        "FtraceTable",
        "SchedDirectTableBuilders",
        "FtraceEvent,",
        "push_event(&mut self, cpu: u32",
        "EventMeta::from_event(cpu, &event)",
        "rows_file",
        "render_sched_rows",
        "SchedSwitchRow",
    ] {
        assert!(
            !generated_builders.contains(marker) && !build_rs.contains(marker),
            "{marker} should not remain in generated table builder plumbing"
        );
    }
}
