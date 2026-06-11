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
