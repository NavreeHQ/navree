//! Compile-fail tests: macro misuse must give readable errors that point at
//! the offending code (expected output in `tests/ui/**/*.stderr`; regenerate
//! with `TRYBUILD=overwrite` and review the diff).

#[test]
fn injectable_rejects_tuple_struct() {
    trybuild::TestCases::new().compile_fail("tests/ui/injectable_rejects_tuple_struct.rs");
}

#[test]
fn injectable_reports_misuse() {
    trybuild::TestCases::new().compile_fail("tests/ui/injectable/*.rs");
}

#[test]
fn module_rejects_unknown_key() {
    trybuild::TestCases::new().compile_fail("tests/ui/module_rejects_unknown_key.rs");
}

#[test]
fn module_reports_misuse() {
    trybuild::TestCases::new().compile_fail("tests/ui/module/*.rs");
}
