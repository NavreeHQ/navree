//! What `#[derive(Validate)]` refuses, and how it says it
//! (`TRYBUILD=overwrite` rewrites the expected messages).

#[test]
fn derive_misuse_is_reported_precisely() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/ui/fields.rs");
    cases.compile_fail("tests/ui/rule_syntax.rs");
    // rustc lists the types a rule accepts, and `html` adds one, so this
    // message is only the same with the crate's default features.
    #[cfg(not(feature = "html"))]
    cases.compile_fail("tests/ui/rule_types.rs");
}
