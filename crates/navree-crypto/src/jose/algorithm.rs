//! [`Algorithm`]: the JWS algorithms Navree signs and verifies.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::JoseError;

/// A JWS algorithm (RFC 7518 §3, RFC 8037 §3.1), named as in the `alg`
/// header.
///
/// `none` is not an algorithm here: a token that names it never parses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Algorithm {
    /// HMAC with SHA-256.
    HS256,
    /// HMAC with SHA-384.
    HS384,
    /// HMAC with SHA-512.
    HS512,
    /// RSASSA-PKCS1-v1_5 with SHA-256.
    RS256,
    /// RSASSA-PKCS1-v1_5 with SHA-384.
    RS384,
    /// RSASSA-PKCS1-v1_5 with SHA-512.
    RS512,
    /// RSASSA-PSS with SHA-256 (and a 32-byte salt).
    PS256,
    /// RSASSA-PSS with SHA-384 (and a 48-byte salt).
    PS384,
    /// RSASSA-PSS with SHA-512 (and a 64-byte salt).
    PS512,
    /// ECDSA on P-256 with SHA-256.
    ES256,
    /// ECDSA on P-384 with SHA-384.
    ES384,
    /// Ed25519.
    EdDSA,
}

impl Algorithm {
    /// Every algorithm, in the order of the JWA registry.
    pub const ALL: [Algorithm; 12] = [
        Algorithm::HS256,
        Algorithm::HS384,
        Algorithm::HS512,
        Algorithm::RS256,
        Algorithm::RS384,
        Algorithm::RS512,
        Algorithm::PS256,
        Algorithm::PS384,
        Algorithm::PS512,
        Algorithm::ES256,
        Algorithm::ES384,
        Algorithm::EdDSA,
    ];

    /// The registered name (`"ES256"`).
    pub const fn as_str(self) -> &'static str {
        match self {
            Algorithm::HS256 => "HS256",
            Algorithm::HS384 => "HS384",
            Algorithm::HS512 => "HS512",
            Algorithm::RS256 => "RS256",
            Algorithm::RS384 => "RS384",
            Algorithm::RS512 => "RS512",
            Algorithm::PS256 => "PS256",
            Algorithm::PS384 => "PS384",
            Algorithm::PS512 => "PS512",
            Algorithm::ES256 => "ES256",
            Algorithm::ES384 => "ES384",
            Algorithm::EdDSA => "EdDSA",
        }
    }

    /// The `kty` of the keys that use it: `oct`, `RSA`, `EC` or `OKP`.
    pub const fn key_type(self) -> &'static str {
        match self {
            Algorithm::HS256 | Algorithm::HS384 | Algorithm::HS512 => "oct",
            Algorithm::RS256
            | Algorithm::RS384
            | Algorithm::RS512
            | Algorithm::PS256
            | Algorithm::PS384
            | Algorithm::PS512 => "RSA",
            Algorithm::ES256 | Algorithm::ES384 => "EC",
            Algorithm::EdDSA => "OKP",
        }
    }

    /// Whether it uses a shared secret (the HMAC algorithms).
    pub const fn is_symmetric(self) -> bool {
        matches!(self, Algorithm::HS256 | Algorithm::HS384 | Algorithm::HS512)
    }

    /// The minimum secret length of an HMAC algorithm: its hash output
    /// (RFC 7518 §3.2).
    pub(crate) const fn hmac_len(self) -> Option<usize> {
        match self {
            Algorithm::HS256 => Some(32),
            Algorithm::HS384 => Some(48),
            Algorithm::HS512 => Some(64),
            _ => None,
        }
    }

    /// Whether it is an RSA algorithm.
    pub(crate) const fn is_rsa(self) -> bool {
        matches!(
            self,
            Algorithm::RS256
                | Algorithm::RS384
                | Algorithm::RS512
                | Algorithm::PS256
                | Algorithm::PS384
                | Algorithm::PS512
        )
    }
}

impl FromStr for Algorithm {
    type Err = JoseError;

    /// Parses a registered name; names are case-sensitive.
    fn from_str(name: &str) -> Result<Self, Self::Err> {
        Algorithm::ALL
            .into_iter()
            .find(|alg| alg.as_str() == name)
            .ok_or_else(|| JoseError::UnsupportedAlgorithm(name.to_owned()))
    }
}

impl fmt::Display for Algorithm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for Algorithm {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Algorithm {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        name.parse().map_err(serde::de::Error::custom)
    }
}
