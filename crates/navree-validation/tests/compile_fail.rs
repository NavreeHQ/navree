//! What `#[derive(Validate)]` refuses, and how it says it
//! (`TRYBUILD=overwrite` rewrites the expected messages).

#[test]
fn derive_misuse_is_reported_precisely() {
    trybuild::TestCases::new().compile_fail("tests/ui/*.rs");
}
