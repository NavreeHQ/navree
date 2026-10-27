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

/// Review Focus 2 (compile-time half): a route with no access declaration
/// does not compile.
#[test]
fn route_without_access_attribute_is_error() {
    trybuild::TestCases::new().compile_fail("tests/ui/route_without_access_attribute_is_error.rs");
}

#[test]
fn guard_alone_is_not_access() {
    trybuild::TestCases::new().compile_fail("tests/ui/guard_alone_is_not_access.rs");
}

#[test]
fn controller_reports_misuse() {
    trybuild::TestCases::new().compile_fail("tests/ui/controller/*.rs");
}

#[test]
fn flag_reports_misuse() {
    trybuild::TestCases::new().compile_fail("tests/ui/flag/*.rs");
}

/// `#[agent]` reports tool misuse (no doc comment, not async, an unknown
/// option, `from` on `#[can]`, two arguments values, an access attribute on
/// a method that is not a tool) and misplacement, before it needs navree-ai.
#[test]
fn agent_reports_misuse() {
    trybuild::TestCases::new().compile_fail("tests/ui/agent/*.rs");
}

#[test]
fn bad_duration_is_error() {
    trybuild::TestCases::new().compile_fail("tests/ui/bad_duration_is_error.rs");
}
