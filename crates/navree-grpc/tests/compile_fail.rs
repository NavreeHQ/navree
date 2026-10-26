//! Compile-fail tests: `#[grpc_service]` misuse gives readable errors that
//! point at the offending code (expected output in `tests/ui/*.stderr`;
//! regenerate with `TRYBUILD=overwrite` and review the diff).

/// Deny by default: a method with no access declaration does not compile.
#[test]
fn grpc_service_reports_misuse() {
    trybuild::TestCases::new().compile_fail("tests/ui/*.rs");
}
