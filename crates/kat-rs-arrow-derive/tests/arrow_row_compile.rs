#[test]
fn arrow_row_compile_cases() {
    let tests = trybuild::TestCases::new();
    tests.pass("tests/ui/plain_struct.rs");
    tests.pass("tests/ui/prost_struct.rs");
    tests.compile_fail("tests/ui/unsupported_field.rs");
}
