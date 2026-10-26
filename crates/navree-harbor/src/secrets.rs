//! Encrypted secrets: AES-256-GCM under the key in `harbor.key` (or
//! `NAVREE_HARBOR_KEY`), stored in `[secrets]` as `harbor:v1:<payload>`.
//!
//! The payload is the 12-byte nonce followed by the ciphertext and its tag,
//! in URL-safe base64 without padding. The secret's name is the associated
//! data, so a value copied under another name does not decrypt.

use std::fmt;

use aes_gcm::aead::{Aead as _, KeyInit as _, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use rand::TryRng as _;

use crate::error::{HarborError, Result};

/// The prefix of every encrypted value.
pub const SECRET_PREFIX: &str = "harbor:v1:";

/// The prefix of a key's text form (`base64:<32 bytes>`), as for `APP_KEY`.
pub const KEY_PREFIX: &str = "base64:";

const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;

/// The 256-bit key that encrypts `[secrets]`.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretKey {
    bytes: [u8; KEY_LEN],
}

impl fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretKey(..)")
    }
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    rand::rngs::SysRng
        .try_fill_bytes(&mut bytes)
        .expect("the operating system's random number generator failed");
    bytes
}

impl SecretKey {
    /// A new random key.
    pub fn generate() -> Self {
        Self {
            bytes: random_bytes(),
        }
    }

    /// Parses `base64:<standard base64 of 32 bytes>`; surrounding
    /// whitespace (a trailing newline in the file) is ignored.
    pub fn parse(text: &str) -> Result<Self> {
        let text = text.trim();
        let encoded = text.strip_prefix(KEY_PREFIX).ok_or_else(|| {
            HarborError::BadKey("it must start with `base64:` (the form harbor:init writes)".into())
        })?;
        let bytes = STANDARD
            .decode(encoded)
            .map_err(|e| HarborError::BadKey(format!("not base64: {e}")))?;
        let bytes: [u8; KEY_LEN] = bytes.try_into().map_err(|b: Vec<u8>| {
            HarborError::BadKey(format!("it must be 32 bytes, got {}", b.len()))
        })?;
        Ok(Self { bytes })
    }

    /// The `base64:<32 bytes>` form written to `harbor.key`.
    pub fn encode(&self) -> String {
        format!("{KEY_PREFIX}{}", STANDARD.encode(self.bytes))
    }

    fn cipher(&self) -> Aes256Gcm {
        Aes256Gcm::new((&self.bytes).into())
    }

    /// Encrypts `value` stored under `name` into `harbor:v1:<payload>`.
    pub fn encrypt(&self, name: &str, value: &str) -> String {
        let nonce: [u8; NONCE_LEN] = random_bytes();
        let ciphertext = self
            .cipher()
            .encrypt(
                &Nonce::from(nonce),
                Payload {
                    msg: value.as_bytes(),
                    aad: name.as_bytes(),
                },
            )
            .expect("AES-GCM encryption of an in-memory value cannot fail");
        let mut payload = nonce.to_vec();
        payload.extend_from_slice(&ciphertext);
        format!("{SECRET_PREFIX}{}", URL_SAFE_NO_PAD.encode(payload))
    }

    /// Decrypts the value stored under `name`.
    pub fn decrypt(&self, name: &str, stored: &str) -> Result<String> {
        let error = || HarborError::Decrypt {
            name: name.to_owned(),
        };
        let encoded = stored
            .trim()
            .strip_prefix(SECRET_PREFIX)
            .ok_or_else(error)?;
        let payload = URL_SAFE_NO_PAD.decode(encoded).map_err(|_| error())?;
        if payload.len() < NONCE_LEN + 16 {
            return Err(error());
        }
        let (nonce, ciphertext) = payload.split_at(NONCE_LEN);
        let nonce: [u8; NONCE_LEN] = nonce.try_into().map_err(|_| error())?;
        let plaintext = self
            .cipher()
            .decrypt(
                &Nonce::from(nonce),
                Payload {
                    msg: ciphertext,
                    aad: name.as_bytes(),
                },
            )
            .map_err(|_| error())?;
        String::from_utf8(plaintext).map_err(|_| error())
    }
}

/// A new application key for `APP_KEY`: `base64:<32 random bytes>`, the form
/// `key:generate` prints.
pub fn generate_app_key() -> String {
    format!("{KEY_PREFIX}{}", STANDARD.encode(random_bytes::<KEY_LEN>()))
}

/// A random password of `len` letters and digits (no quoting needed in
/// URLs, SQL or configuration files).
pub fn generate_password(len: usize) -> String {
    use rand::distr::{Alphanumeric, SampleString as _};
    Alphanumeric.sample_string(&mut rand::rng(), len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_round_trip_under_their_name() {
        let key = SecretKey::generate();
        let stored = key.encrypt("APP_KEY", "base64:abc");
        assert!(stored.starts_with("harbor:v1:"), "{stored}");
        assert!(!stored.contains("abc"));
        assert_eq!(key.decrypt("APP_KEY", &stored).unwrap(), "base64:abc");
    }

    #[test]
    fn every_encryption_uses_a_new_nonce() {
        let key = SecretKey::generate();
        assert_ne!(key.encrypt("A", "same"), key.encrypt("A", "same"));
    }

    #[test]
    fn a_value_does_not_decrypt_under_another_name() {
        let key = SecretKey::generate();
        let stored = key.encrypt("STRIPE_SECRET", "sk_live_1");
        let error = key.decrypt("APP_KEY", &stored).unwrap_err();
        assert!(matches!(error, HarborError::Decrypt { ref name } if name == "APP_KEY"));
    }

    #[test]
    fn a_value_does_not_decrypt_with_another_key() {
        let stored = SecretKey::generate().encrypt("A", "x");
        assert!(SecretKey::generate().decrypt("A", &stored).is_err());
    }

    #[test]
    fn tampered_or_malformed_values_are_rejected() {
        let key = SecretKey::generate();
        let stored = key.encrypt("A", "value");
        let mut tampered = stored.clone();
        let last = tampered.pop().unwrap();
        tampered.push(if last == 'A' { 'B' } else { 'A' });
        assert!(key.decrypt("A", &tampered).is_err());
        assert!(key.decrypt("A", "plain").is_err());
        assert!(key.decrypt("A", "harbor:v1:!!").is_err());
        assert!(key.decrypt("A", "harbor:v1:AAAA").is_err());
    }

    #[test]
    fn keys_round_trip_through_their_text_form() {
        let key = SecretKey::generate();
        let text = key.encode();
        assert!(text.starts_with("base64:"));
        assert_eq!(text.len(), "base64:".len() + 44);
        assert_eq!(SecretKey::parse(&format!("{text}\n")).unwrap(), key);
        assert_eq!(format!("{key:?}"), "SecretKey(..)");
    }

    #[test]
    fn bad_keys_are_rejected() {
        assert!(SecretKey::parse("abc").is_err());
        assert!(SecretKey::parse("base64:!!!").is_err());
        assert!(SecretKey::parse("base64:YWJj").is_err());
    }

    #[test]
    fn app_keys_and_passwords() {
        let key = generate_app_key();
        assert!(key.starts_with("base64:"));
        assert_eq!(STANDARD.decode(&key[7..]).unwrap().len(), 32);
        assert_ne!(generate_app_key(), key);
        let password = generate_password(32);
        assert_eq!(password.len(), 32);
        assert!(password.chars().all(|c| c.is_ascii_alphanumeric()));
    }
}
