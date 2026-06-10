#[test]
fn arrow_row_compile_cases() {
    let tests = trybuild::TestCases::new();
    tests.pass("tests/fixtures/arrow_row/plain_struct.rs");
    tests.pass("tests/fixtures/arrow_row/prost_struct.rs");
    tests.compile_fail("tests/fixtures/arrow_row/unsupported_field.rs");
}
