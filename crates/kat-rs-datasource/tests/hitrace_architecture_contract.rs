use std::fs;

#[test]
fn hitrace_parser_only_wires_direct_tables() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let hitrace_rs = fs::read_to_string(format!("{manifest_dir}/src/hitrace.rs"))
        .expect("hitrace parser source can be read");
    let derived_rs_path = format!("{manifest_dir}/src/hitrace/derived.rs");

    for marker in [
        "mod derived",
        "DerivedTables",
        "thread_state",
        "instant",
        "sched_slice",
        "raw_event",
    ] {
        assert!(
            !hitrace_rs.contains(marker),
            "{marker} should not be wired into the hitrace parser"
        );
    }
    assert!(
        !std::path::Path::new(&derived_rs_path).exists(),
        "derived tables should be added back in a separate slice"
    );
}

#[test]
fn direct_sched_table_builders_are_generated() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let hitrace_rs = fs::read_to_string(format!("{manifest_dir}/src/hitrace.rs"))
        .expect("hitrace parser source can be read");
    let ftrace_rs = fs::read_to_string(format!("{manifest_dir}/src/ftrace/mod.rs"))
        .expect("ftrace domain source can be read");
    let lib_rs =
        fs::read_to_string(format!("{manifest_dir}/src/lib.rs")).expect("lib source can be read");
    let generated_builders =
        fs::read_to_string(format!("{}/sched_table_builders.rs", env!("OUT_DIR")))
            .expect("generated sched table builders can be read");

    assert!(ftrace_rs.contains("SchedDirectTableBuilders::new()?"));
    assert!(!hitrace_rs.contains("SchedDirectTableBuilders"));
    assert!(!hitrace_rs.contains("DerivedTables::default()"));
    assert!(!hitrace_rs.contains("struct SchedRows"));
    assert!(!hitrace_rs.contains("sched_switch: TableBuilder<SchedSwitchRow>"));
    assert!(!hitrace_rs.contains("SchedSwitchRow::new(&meta, message)"));
    assert!(!lib_rs.contains("mod sched_rows"));

    assert!(!generated_builders.contains("pub(crate) trait SchedEventObserver"));
    assert!(
        generated_builders.contains("ftrace::{DirectEventTableBuilder, EventMeta, FtraceTable}")
    );
    assert!(!generated_builders.contains("hitrace::{DirectEventTableBuilder"));
    assert!(generated_builders.contains("pub(crate) struct SchedDirectTableBuilders"));
    assert!(generated_builders.contains("sched_switch: DirectEventTableBuilder"));
    assert!(
        generated_builders
            .contains("DirectEventTableBuilder::new::<SchedSwitchFormat>(\"sched_switch\")?")
    );
    assert!(generated_builders.contains("let meta = EventMeta::from_event(cpu, &event);"));
    assert!(generated_builders.contains("self.sched_switch.push(meta.clone(), message)?"));
    assert!(!generated_builders.contains("TableBuilder<EventRow<SchedSwitchFormat>>"));
    assert!(!generated_builders.contains("TableBuilder::new_from_sample"));
    assert!(!generated_builders.contains("SchedSwitchRow"));
    assert!(!generated_builders.contains("SchedEventMeta"));
    assert!(!generated_builders.contains("sched_rows"));
    assert!(!generated_builders.contains("observer.observe_sched_switch(&row);"));
}

#[test]
fn profiler_plugin_data_uses_table_builder() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let hitrace_rs = fs::read_to_string(format!("{manifest_dir}/src/hitrace.rs"))
        .expect("hitrace parser source can be read");
    let ftrace_rs = fs::read_to_string(format!("{manifest_dir}/src/ftrace/mod.rs"))
        .expect("ftrace domain source can be read");

    assert!(hitrace_rs.contains("mod table_builder;"));
    assert!(hitrace_rs.contains("use table_builder::TableBuilder;"));
    assert!(!ftrace_rs.contains(
        "pub(crate) use table_builder::{DirectEventTableBuilder, EventMeta, TableBuilder};"
    ));
    assert!(hitrace_rs.contains("TableBuilder::<ProfilerPluginData>::new(HITRACE_TABLE)?"));
    assert!(!hitrace_rs.contains("let mut profiler_batches = Vec::new();"));
    assert!(!hitrace_rs.contains("profiler_batches.push(batch);"));
    assert!(!hitrace_rs.contains("record_batch_from(messages)"));
}

#[test]
fn profiler_plugin_data_streams_len_prefixed_messages() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let hitrace_rs = fs::read_to_string(format!("{manifest_dir}/src/hitrace.rs"))
        .expect("hitrace parser source can be read");

    assert!(hitrace_rs.contains("for_each_len_prefixed_message::<ProfilerPluginData, _>"));
    assert!(!hitrace_rs.contains("fn decode_len_prefixed_messages"));
    assert!(!hitrace_rs.contains("let messages = decode_len_prefixed_messages"));
    assert!(!hitrace_rs.contains("fn decode_sched_rows"));
    assert!(!hitrace_rs.contains("messages: &[ProfilerPluginData]"));
}

#[test]
fn sched_generation_uses_event_family_generator() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let build_rs =
        fs::read_to_string(format!("{manifest_dir}/build.rs")).expect("build script can be read");

    for marker in [
        "struct EventFamilySpec",
        "const SCHED_FAMILY: EventFamilySpec",
        "generate_event_family_code(&SCHED_FAMILY, &sched_messages)",
        "fn generate_event_family_code(",
        "fn render_event_table_builders(family: &EventFamilySpec, messages: &[ProtoMessage])",
    ] {
        assert!(build_rs.contains(marker), "{marker} should exist");
    }

    for marker in [
        "rows_file",
        "meta_name",
        "rust_type:",
        "fn render_event_rows",
        "fn render_row_struct",
        "fn rust_type",
        "fn generate_sched_code",
        "fn render_sched_rows",
        "fn render_sched_table_builders",
    ] {
        assert!(!build_rs.contains(marker), "{marker} should be generalized");
    }
}

#[test]
fn ftrace_domain_owns_payload_decode_and_sched_tables() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let hitrace_rs = fs::read_to_string(format!("{manifest_dir}/src/hitrace.rs"))
        .expect("hitrace parser source can be read");
    let ftrace_rs = fs::read_to_string(format!("{manifest_dir}/src/ftrace/mod.rs"))
        .expect("ftrace domain source can be read");
    let build_rs =
        fs::read_to_string(format!("{manifest_dir}/build.rs")).expect("build script can be read");
    let generated_builders =
        fs::read_to_string(format!("{}/sched_table_builders.rs", env!("OUT_DIR")))
            .expect("generated sched table builders can be read");

    assert!(hitrace_rs.contains("FtraceTables::new()?"));
    assert!(hitrace_rs.contains("ftrace_tables.push_plugin_payload("));
    assert!(
        !std::path::Path::new(&format!("{manifest_dir}/src/ftrace.rs")).exists(),
        "ftrace domain entry should live in src/ftrace/mod.rs"
    );
    assert!(!hitrace_rs.contains("TracePluginResult"));
    assert!(!hitrace_rs.contains("SchedDirectTableBuilders"));
    assert!(!hitrace_rs.contains("decode_sched_message"));

    assert!(ftrace_rs.contains("TracePluginResult::decode"));
    assert!(ftrace_rs.contains("SchedDirectTableBuilders::new()?"));
    assert!(ftrace_rs.contains("pub(crate) struct FtraceTables"));
    assert!(ftrace_rs.contains("pub(crate) struct FtraceTable"));

    assert!(build_rs.contains("ftrace::{DirectEventTableBuilder, EventMeta, FtraceTable}"));
    assert!(
        generated_builders.contains("ftrace::{DirectEventTableBuilder, EventMeta, FtraceTable}")
    );
    assert!(!generated_builders.contains("crate::hitrace::{DirectEventTableBuilder"));
}
