use std::fs;

#[test]
fn derived_table_code_lives_outside_hitrace_parser() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let hitrace_rs = fs::read_to_string(format!("{manifest_dir}/src/hitrace.rs"))
        .expect("hitrace parser source can be read");
    let derived_rs = fs::read_to_string(format!("{manifest_dir}/src/hitrace/derived.rs"))
        .expect("derived table source can be read");

    for marker in [
        "struct ThreadStateRow",
        "struct ThreadStateBuilder",
        "struct InstantRow",
        "impl ThreadStateBuilder",
        "impl InstantRow",
    ] {
        assert!(
            !hitrace_rs.contains(marker),
            "{marker} should live in hitrace/derived.rs"
        );
        assert!(
            derived_rs.contains(marker),
            "{marker} should be defined in hitrace/derived.rs"
        );
    }
}

#[test]
fn direct_sched_table_builders_are_generated() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let hitrace_rs = fs::read_to_string(format!("{manifest_dir}/src/hitrace.rs"))
        .expect("hitrace parser source can be read");
    let generated_builders =
        fs::read_to_string(format!("{}/sched_table_builders.rs", env!("OUT_DIR")))
            .expect("generated sched table builders can be read");

    assert!(hitrace_rs.contains("SchedDirectTableBuilders::new()?"));
    assert!(hitrace_rs.contains("DerivedTables::default()"));
    assert!(!hitrace_rs.contains("struct SchedRows"));
    assert!(!hitrace_rs.contains("sched_switch: TableBuilder<SchedSwitchRow>"));
    assert!(!hitrace_rs.contains("SchedSwitchRow::new(&meta, message)"));

    assert!(generated_builders.contains("pub(crate) trait SchedEventObserver"));
    assert!(generated_builders.contains("pub(crate) struct SchedDirectTableBuilders"));
    assert!(generated_builders.contains("sched_switch: TableBuilder<SchedSwitchRow>"));
    assert!(generated_builders.contains("observer.observe_sched_switch(&row);"));
}

#[test]
fn profiler_plugin_data_uses_table_builder() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let hitrace_rs = fs::read_to_string(format!("{manifest_dir}/src/hitrace.rs"))
        .expect("hitrace parser source can be read");

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
        "generate_event_family_code(&SCHED_FAMILY)",
        "fn generate_event_family_code(family: &EventFamilySpec)",
        "fn render_event_rows(family: &EventFamilySpec, messages: &[ProtoMessage])",
        "fn render_event_table_builders(family: &EventFamilySpec, messages: &[ProtoMessage])",
    ] {
        assert!(build_rs.contains(marker), "{marker} should exist");
    }

    for marker in [
        "fn generate_sched_code",
        "fn render_sched_rows",
        "fn render_sched_table_builders",
    ] {
        assert!(!build_rs.contains(marker), "{marker} should be generalized");
    }
}
