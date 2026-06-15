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
        "src/record.rs",
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

    for marker in [
        "TraceRecord::ProfilerSection",
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

    assert!(!catalog.contains("TraceRecord"));
    assert!(!catalog.contains("TraceRecordSink"));
    assert!(!catalog.contains("ProfilerSection"));
    assert!(!sink_mod.contains("ProfilerSection"));
    assert!(!sink_mod.contains("profiler_table_seen"));
    assert!(!sink_mod.contains("profiler_rows"));
    assert!(!sink_mod.contains("SchedDirectTableBuilders"));
    assert!(sink_table_builder.contains("ArrayBuilder"));
    assert!(sink_table_builder.contains("pub(crate) struct DirectEventTableBuilder"));
    assert!(sink_table_builder.contains("pub(crate) struct EventMeta"));
}

#[test]
fn query_consumes_trace_dataset_catalog() {
    let query = source("src/query.rs");

    for marker in ["load_hitrace_tables", "HITRACE_TABLE", "FtraceTables"] {
        assert!(
            !query.contains(marker),
            "{marker} should not be consumed directly by query"
        );
    }
}

#[test]
fn ftrace_event_family_generation_avoids_old_sched_entrypoint() {
    let build_rs = source("build.rs");
    let generated_builders = fs::read_to_string(format!(
        "{}/ftrace_event_table_builders.rs",
        env!("OUT_DIR")
    ))
    .expect("generated ftrace event table builders can be read");

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
