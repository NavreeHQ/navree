//! What `#[derive(Model)]`, `#[scopes]` and `#[test]` refuse, and how they
//! say it (`TRYBUILD=overwrite` rewrites the expected messages).

#[test]
fn macro_misuse_is_reported_precisely() {
    trybuild::TestCases::new().compile_fail("tests/ui/*.rs");
}
