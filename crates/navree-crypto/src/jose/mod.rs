//! JSON Web Signatures and JSON Web Keys on `ring`.
//!
//! - [`SigningKey`] signs compact JWS and JWTs with HS256/384/512, EdDSA
//!   (Ed25519), ES256/384 and RS256/384/512 or PS256/384/512. It loads PEM
//!   and PKCS#8 keys (Apple's `.p8` files among them), private JWKs and
//!   HMAC secrets, generates Ed25519 and P-256 keys, and exports PKCS#8.
//! - [`VerifyingKey`] verifies them: public keys from a JWK, a PEM
//!   SubjectPublicKeyInfo, an X.509 certificate or raw parts (RSA `n` and
//!   `e`, EC coordinates, Ed25519 bytes), and HMAC secrets. A key verifies
//!   only its own algorithms (and, for senders that still use it, RSA with
//!   SHA-1 through [`VerifyingKey::verify_legacy_sha1`]).
//! - [`Jws`] parses a compact token; [`Jws::verify`] checks it with one
//!   key and [`Jws::verify_with`] with a key set such as an identity
//!   provider's JWKS.
//! - [`Jwk`] and [`JwkSet`] read and publish keys, with RFC 7638
//!   thumbprints as key ids.
//!
//! `alg: none` and critical header extensions never parse, a key set never
//! verifies an HMAC token, and keys never appear in `Debug` output or error
//! messages. Claims checks (`exp`, `aud`, `iss`) are the caller's: this
//! module only says who signed the token.
//!
//! ```
//! use navree_crypto::jose::{JwkSet, Jws, SigningKey};
//! use serde_json::json;
//!
//! // Sign with an Ed25519 key whose key id is its thumbprint.
//! let key = SigningKey::generate_ed25519().with_thumbprint_kid();
//! let token = key.sign_jwt(&json!({ "sub": "42", "aud": "api" }))?;
//!
//! // Publish the public key...
//! let jwks: JwkSet = key.public_jwk().into_iter().collect();
//! let published = serde_json::to_string(&jwks).unwrap();
//!
//! // ...and verify against the published set.
//! let jwks: JwkSet = serde_json::from_str(&published).unwrap();
//! let jws = Jws::parse(&token)?;
//! jws.verify_with(&jwks)?;
//! let claims: serde_json::Value = jws.claims()?;
//! assert_eq!(claims["sub"], "42");
//! # Ok::<(), navree_crypto::jose::JoseError>(())
//! ```

mod algorithm;
mod der;
mod error;
mod jwk;
mod jws;
mod keys;

pub use algorithm::Algorithm;
pub use error::JoseError;
pub use jwk::{Jwk, JwkSet};
pub use jws::{Header, Jws};
pub use keys::{SigningKey, VerifyingKey};
