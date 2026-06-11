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
fn direct_sched_tables_use_streaming_table_builder() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let hitrace_rs = fs::read_to_string(format!("{manifest_dir}/src/hitrace.rs"))
        .expect("hitrace parser source can be read");

    assert!(hitrace_rs.contains("struct TableBuilder<T>"));
    assert!(hitrace_rs.contains("sched_switch: TableBuilder<SchedSwitchRow>"));
    assert!(hitrace_rs.contains("sched_wakeup: TableBuilder<SchedWakeupRow>"));

    for marker in [
        "sched_switch: Vec<SchedSwitchRow>",
        "sched_wakeup: Vec<SchedWakeupRow>",
        "sched_blocked_reason: Vec<SchedBlockedReasonRow>",
    ] {
        assert!(
            !hitrace_rs.contains(marker),
            "{marker} should use TableBuilder instead of Vec<Row>"
        );
    }
}
