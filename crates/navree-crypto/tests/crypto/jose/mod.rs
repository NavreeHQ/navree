//! `navree_crypto::jose`: JSON Web Signatures, JSON Web Keys and key sets,
//! PKCS#8 keys.
//!
//! - `vectors`: the RFC 7515 (A.1, A.2, A.3), RFC 7638 and RFC 8037 (A.1 to
//!   A.4) examples.
//! - `independent`: keys and tokens made by another implementation (Node's
//!   crypto module, see `fixtures/jose/generate.mjs`).
//! - `jws`: signing, parsing and verification rules.
//! - `keys`: PEM, PKCS#8 and JWK loading, generation and export.
//! - `jwks`: key sets, including the published sets of real providers.

mod independent;
mod jwks;
mod jws;
mod keys;
mod vectors;

use serde_json::Value;

/// A fixture of `tests/fixtures/jose`.
pub fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/jose/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// A JSON fixture of `tests/fixtures/jose`.
pub fn json_fixture(name: &str) -> Value {
    serde_json::from_str(&fixture(name)).unwrap()
}
