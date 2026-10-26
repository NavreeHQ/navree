//! Compile-fail tests of `#[mcp_server]`: misuse gives readable errors that
//! point at the offending code (expected output in `tests/ui/*.stderr`;
//! regenerate with `TRYBUILD=overwrite` and review the diff).

#[test]
fn mcp_server_reports_misuse() {
    trybuild::TestCases::new().compile_fail("tests/ui/*.rs");
}
