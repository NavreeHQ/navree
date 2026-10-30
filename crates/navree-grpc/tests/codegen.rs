//! Code generation without a system `protoc`: protox compiles the test
//! protos in pure Rust and tonic-prost-build generates the messages, the
//! server trait and the client, exactly as the docs' `build.rs` does.
//!
//! The output is checked in under `tests/support/pb/` (so the other tests
//! need no build script); this test regenerates it and fails when the
//! checked-in copy is stale. Rewrite it with
//! `NAVREE_GRPC_REGENERATE=1 cargo test -p navree-grpc --test codegen`.

use std::path::{Path, PathBuf};

use prost::Message;

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// What the docs' `build.rs` does, writing into `out` instead of `OUT_DIR`.
fn generate(out: &Path) {
    let proto_root = manifest_dir().join("tests/proto");
    let fds = protox::compile(
        ["crm/deals/v1/deals.proto", "navree/test/v1/ping.proto"],
        [&proto_root],
    )
    .expect("protox compiles the protos");
    std::fs::write(out.join("deals_descriptor.bin"), fds.encode_to_vec())
        .expect("descriptor set written");
    tonic_prost_build::configure()
        .out_dir(out)
        .emit_rerun_if_changed(false)
        .type_attribute(
            ".crm.deals.v1.MoveDealRequest",
            "#[derive(navree_validation::Validate)]",
        )
        .field_attribute(".crm.deals.v1.MoveDealRequest.id", "#[validate(skip)]")
        .field_attribute(
            ".crm.deals.v1.MoveDealRequest.stage",
            "#[validate(length(min = 1))]",
        )
        .field_attribute(
            ".crm.deals.v1.MoveDealRequest.position",
            "#[validate(range(min = 0))]",
        )
        .field_attribute(
            ".crm.deals.v1.MoveDealRequest.lost_reason",
            "#[validate(length(max = 1000))]",
        )
        .compile_fds(fds)
        .expect("tonic-prost-build generates the code");
}

fn normalized(path: &Path) -> Vec<u8> {
    let bytes = std::fs::read(path).unwrap_or_default();
    if path.extension().is_some_and(|e| e == "rs") {
        String::from_utf8_lossy(&bytes)
            .replace("\r\n", "\n")
            .into_bytes()
    } else {
        bytes
    }
}

#[test]
fn generated_code_is_current() {
    let checked_in = manifest_dir().join("tests/support/pb");
    if std::env::var_os("NAVREE_GRPC_REGENERATE").is_some() {
        std::fs::create_dir_all(&checked_in).unwrap();
        generate(&checked_in);
        return;
    }
    let fresh = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    generate(fresh.path());
    for name in [
        "crm.deals.v1.rs",
        "navree.test.v1.rs",
        "deals_descriptor.bin",
    ] {
        assert!(
            normalized(&fresh.path().join(name)) == normalized(&checked_in.join(name)),
            "tests/support/pb/{name} is stale: run NAVREE_GRPC_REGENERATE=1 cargo test -p navree-grpc --test codegen"
        );
    }
}
