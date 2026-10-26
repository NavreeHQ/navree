//! [`JoseError`]: why a token or a key was refused.

/// Why a token was refused, or a key could not be read or used.
///
/// Messages never contain key material, and an algorithm name taken from
/// a token is shortened, so they are safe to log.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum JoseError {
    /// The token is not three base64url parts, or its header is not a JSON
    /// object with an `alg`, or it uses a critical extension.
    #[error("malformed token: {0}")]
    Malformed(&'static str),
    /// An algorithm that is not supported (`none` among them).
    #[error("unsupported algorithm `{}`", shorten(.0))]
    UnsupportedAlgorithm(String),
    /// No key matches the token: another type, curve, algorithm or key id.
    #[error("no matching key")]
    NoKey,
    /// The signature does not verify.
    #[error("bad signature")]
    BadSignature,
    /// A key could not be read, or does not fit what it was asked to do.
    #[error("invalid key: {0}")]
    InvalidKey(&'static str),
    /// The payload is not JSON of the requested type, or the claims to sign
    /// could not be serialized.
    #[error("the payload is not JSON of the requested type")]
    Payload,
    /// Signing failed (the system random number generator failed).
    #[error("signing failed")]
    Signing,
}

/// At most 32 characters of a name taken from a token.
fn shorten(name: &str) -> String {
    const MAX: usize = 32;
    if name.chars().count() <= MAX {
        name.to_owned()
    } else {
        let mut short: String = name.chars().take(MAX).collect();
        short.push('…');
        short
    }
}
