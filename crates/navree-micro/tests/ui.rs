//! Compile-fail tests for `#[message_handlers]`: misuse gives readable
//! errors that point at the offending code (expected output in
//! `tests/ui/*.stderr`; regenerate with `TRYBUILD=overwrite` and review the
//! diff).

#[test]
fn message_handlers_report_misuse() {
    trybuild::TestCases::new().compile_fail("tests/ui/*.rs");
}
