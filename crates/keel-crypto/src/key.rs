//! [`AppKey`]: the 256-bit application key, and [`generate_key`].

use std::fmt;

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use rand::TryRng as _;
use sha2::{Digest as _, Sha256};

use crate::{CryptoError, Result};

/// The prefix of an encoded application key (`base64:<32 bytes>`).
pub const KEY_PREFIX: &str = "base64:";

/// The length of an application key in bytes (AES-256).
pub const KEY_LEN: usize = 32;

/// Domain separation for key ids.
const KID_DOMAIN: &[u8] = b"keel-crypto/kid/v1";

/// Digest bytes in a key id (6 bytes = 8 base64url characters).
const KID_BYTES: usize = 6;

/// A 256-bit application key, parsed from its `base64:<32 bytes>` form.
///
/// `Debug` prints `AppKey([redacted])`; there is no `Display` or `Serialize`.
/// Get one from [`AppKey::parse`], [`AppKey::generate`] or [`generate_key`].
#[derive(Clone)]
pub struct AppKey {
    bytes: [u8; KEY_LEN],
    kid: String,
}

impl AppKey {
    /// Parses `base64:<standard base64 of exactly 32 bytes>`. Surrounding
    /// whitespace is ignored. The error never echoes the input.
    pub fn parse(encoded: &str) -> Result<AppKey> {
        let body = encoded
            .trim()
            .strip_prefix(KEY_PREFIX)
            .ok_or(CryptoError::InvalidKey {
                reason: "expected the form `base64:<32 bytes>` (run `key:generate`)",
            })?;
        let decoded = STANDARD.decode(body).map_err(|_| CryptoError::InvalidKey {
            reason: "the part after `base64:` is not valid base64",
        })?;
        let bytes =
            <[u8; KEY_LEN]>::try_from(decoded.as_slice()).map_err(|_| CryptoError::InvalidKey {
                reason: "a key must decode to exactly 32 bytes",
            })?;
        Ok(AppKey::from_bytes(bytes))
    }

    /// A key from raw bytes.
    pub fn from_bytes(bytes: [u8; KEY_LEN]) -> AppKey {
        let digest = Sha256::new()
            .chain_update(KID_DOMAIN)
            .chain_update(bytes)
            .finalize();
        let kid = URL_SAFE_NO_PAD.encode(&digest[..KID_BYTES]);
        AppKey { bytes, kid }
    }

    /// A new random key from the operating system's CSPRNG.
    ///
    /// # Panics
    ///
    /// If the operating system's random number generator fails (it does not
    /// on supported platforms).
    pub fn generate() -> AppKey {
        let mut bytes = [0u8; KEY_LEN];
        rand::rngs::SysRng
            .try_fill_bytes(&mut bytes)
            .expect("the operating system's random number generator failed");
        AppKey::from_bytes(bytes)
    }

    /// The key id stored in payloads to pick the decryption key: 8 URL-safe
    /// characters derived from a SHA-256 of the key (not reversible).
    pub fn kid(&self) -> &str {
        &self.kid
    }

    /// The `base64:<32 bytes>` form.
    pub(crate) fn encode(&self) -> String {
        format!("{KEY_PREFIX}{}", STANDARD.encode(self.bytes))
    }

    /// The raw key bytes. Call this only where the key is actually used.
    pub fn expose_bytes(&self) -> &[u8; KEY_LEN] {
        &self.bytes
    }
}

/// A new random application key in its `base64:<32 bytes>` form, ready for
/// `app.key` (what the `key:generate` command prints).
pub fn generate_key() -> String {
    AppKey::generate().encode()
}

impl fmt::Debug for AppKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AppKey([redacted])")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encoded(bytes: &[u8]) -> String {
        format!("{KEY_PREFIX}{}", STANDARD.encode(bytes))
    }

    #[test]
    fn key_parse_accepts_base64_prefixed_32_bytes() {
        let bytes = [7u8; KEY_LEN];
        let key = AppKey::parse(&encoded(&bytes)).expect("valid key");
        assert_eq!(key.expose_bytes(), &bytes);
        // Whitespace around the value (e.g. from a hand-edited `.env`) is ignored.
        let key = AppKey::parse(&format!("  {}\n", encoded(&bytes))).expect("trimmed");
        assert_eq!(key.expose_bytes(), &bytes);
    }

    #[test]
    fn key_parse_rejects_wrong_length() {
        for len in [0, 16, 31, 33, 64] {
            let input = encoded(&vec![1u8; len]);
            let error = AppKey::parse(&input).expect_err("wrong length must fail");
            assert!(
                matches!(error, CryptoError::InvalidKey { .. }),
                "{len}: {error:?}"
            );
            assert!(error.to_string().contains("32 bytes"), "{len}: {error}");
            let body = &input[KEY_PREFIX.len()..];
            assert!(body.is_empty() || !error.to_string().contains(body));
        }
    }

    #[test]
    fn key_parse_requires_prefix_and_valid_base64() {
        let raw = STANDARD.encode([3u8; KEY_LEN]);
        let error = AppKey::parse(&raw).expect_err("missing prefix");
        assert!(error.to_string().contains("base64:"), "{error}");
        assert!(!error.to_string().contains(&raw), "{error}");

        let error = AppKey::parse("base64:not*valid*base64!").expect_err("bad base64");
        assert!(matches!(error, CryptoError::InvalidKey { .. }), "{error:?}");
        assert!(!error.to_string().contains("not*valid"), "{error}");
    }

    #[test]
    fn generate_key_roundtrips_through_parse() {
        let first = generate_key();
        let second = generate_key();
        assert!(first.starts_with(KEY_PREFIX), "{first}");
        assert_ne!(first, second);
        let parsed = AppKey::parse(&first).expect("generated key parses");
        assert_ne!(parsed.expose_bytes(), &[0u8; KEY_LEN]);
        assert_ne!(
            AppKey::generate().expose_bytes(),
            AppKey::generate().expose_bytes()
        );
    }

    #[test]
    fn kid_is_stable_short_and_distinct_per_key() {
        let a = AppKey::from_bytes([1u8; KEY_LEN]);
        let b = AppKey::from_bytes([2u8; KEY_LEN]);
        assert_eq!(a.kid(), AppKey::from_bytes([1u8; KEY_LEN]).kid());
        assert_ne!(a.kid(), b.kid());
        assert_eq!(a.kid().len(), 8);
        assert!(
            a.kid()
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'),
            "{}",
            a.kid()
        );
    }

    #[test]
    fn key_debug_is_redacted() {
        let key = AppKey::from_bytes([42u8; KEY_LEN]);
        let debug = format!("{key:?}");
        assert!(debug.contains("redacted"), "{debug}");
        assert!(!debug.contains("42"), "{debug}");
    }
}
