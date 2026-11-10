//! [`Encrypter`]: AES-256-GCM authenticated encryption with key rotation.

use std::fmt;

use aes_gcm::aead::{Aead as _, KeyInit as _, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::RngExt as _;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::{AppKey, CryptoError, Result, hmac_sha256};

/// The payload format version (the first segment of every payload).
const VERSION: &str = "v1";

/// AES-GCM nonce length in bytes (96 bits).
const NONCE_LEN: usize = 12;

/// AES-GCM tag length in bytes (128 bits).
const TAG_LEN: usize = 16;

/// Domain separation for [`Encrypter::derive_key`].
const DERIVE_DOMAIN: &str = "navree-crypto/derive/v1:";

/// Encrypts and decrypts values with the application key (AES-256-GCM).
///
/// Every payload uses a fresh random 96-bit nonce and is the compact,
/// URL- and cookie-safe string
///
/// ```text
/// v1.<kid>.<nonce>.<ciphertext+tag>     (base64url, no padding)
/// ```
///
/// where `kid` is the [key id](AppKey::kid) of the key that encrypted it.
/// The `v1.<kid>` header is authenticated as associated data, so no segment
/// can be changed without decryption failing.
///
/// **Key rotation:** new payloads always use the current key; payloads
/// encrypted with any of the [previous keys](Encrypter::with_previous_keys)
/// still decrypt. Rotate by moving the old key to `app.previous_keys` and
/// setting a new `app.key`.
///
/// `CryptoModule` provides it as a global singleton; cloning is cheap enough
/// for tests (it copies the expanded key schedules).
///
/// ```
/// use navree_crypto::{AppKey, Encrypter};
///
/// let old = AppKey::generate();
/// let payload = Encrypter::new(old.clone()).encrypt_string("remember me")?;
///
/// // After rotation the old key only decrypts.
/// let encrypter = Encrypter::new(AppKey::generate()).with_previous_keys([old]);
/// assert_eq!(encrypter.decrypt_string(&payload)?, "remember me");
/// # Ok::<(), navree_crypto::CryptoError>(())
/// ```
#[derive(Clone)]
pub struct Encrypter {
    /// The key new payloads are encrypted with; `None` when no key is configured.
    current: Option<Entry>,
    /// Keys that only decrypt.
    previous: Vec<Entry>,
}

/// A key and its expanded cipher.
#[derive(Clone)]
struct Entry {
    key: AppKey,
    cipher: Aes256Gcm,
}

impl Entry {
    fn new(key: AppKey) -> Self {
        let cipher = Aes256Gcm::new(key.expose_bytes().into());
        Self { key, cipher }
    }
}

impl Encrypter {
    /// An encrypter that encrypts new payloads with `key`.
    pub fn new(key: AppKey) -> Self {
        Self {
            current: Some(Entry::new(key)),
            previous: Vec::new(),
        }
    }

    /// An encrypter without a current key: every `encrypt*` call fails with
    /// [`CryptoError::MissingKey`] (used when `app.key` is not configured,
    /// so commands such as `key:generate` can still run).
    pub(crate) fn without_key() -> Self {
        Self {
            current: None,
            previous: Vec::new(),
        }
    }

    /// Adds keys that still decrypt payloads encrypted before a rotation
    /// (most recent first). They never encrypt.
    pub fn with_previous_keys(mut self, keys: impl IntoIterator<Item = AppKey>) -> Self {
        self.previous.extend(keys.into_iter().map(Entry::new));
        self
    }

    /// The [key id](AppKey::kid) new payloads are encrypted with, or `None`
    /// when no key is configured.
    pub fn current_kid(&self) -> Option<&str> {
        self.current.as_ref().map(|entry| entry.key.kid())
    }

    /// Encrypts `plaintext` with the current key into a
    /// `v1.<kid>.<nonce>.<ciphertext>` payload.
    ///
    /// Fails with [`CryptoError::MissingKey`] when no key is configured.
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<String> {
        let entry = self.current.as_ref().ok_or(CryptoError::MissingKey)?;
        let header = header(entry.key.kid());
        let mut nonce = [0u8; NONCE_LEN];
        rand::rng().fill(&mut nonce[..]);
        let ciphertext = entry
            .cipher
            .encrypt(
                &Nonce::from(nonce),
                Payload {
                    msg: plaintext,
                    aad: header.as_bytes(),
                },
            )
            .map_err(|_| CryptoError::Encrypt)?;
        Ok(format!(
            "{header}.{}.{}",
            URL_SAFE_NO_PAD.encode(nonce),
            URL_SAFE_NO_PAD.encode(ciphertext)
        ))
    }

    /// Decrypts a payload produced by [`encrypt`](Encrypter::encrypt) with the
    /// current key or any previous key.
    ///
    /// Fails with [`CryptoError::InvalidPayload`] when the payload is not
    /// well-formed and [`CryptoError::Decrypt`] when it was modified or its
    /// key is not configured.
    pub fn decrypt(&self, payload: &str) -> Result<Vec<u8>> {
        let mut segments = payload.split('.');
        let (Some(version), Some(kid), Some(nonce), Some(ciphertext), None) = (
            segments.next(),
            segments.next(),
            segments.next(),
            segments.next(),
            segments.next(),
        ) else {
            return Err(CryptoError::InvalidPayload {
                reason: "expected `v1.<kid>.<nonce>.<ciphertext>`",
            });
        };
        if version != VERSION {
            return Err(CryptoError::InvalidPayload {
                reason: "unsupported payload version",
            });
        }
        let nonce = URL_SAFE_NO_PAD
            .decode(nonce)
            .ok()
            .and_then(|bytes| <[u8; NONCE_LEN]>::try_from(bytes).ok())
            .ok_or(CryptoError::InvalidPayload {
                reason: "the nonce is not 12 base64url-encoded bytes",
            })?;
        let ciphertext = URL_SAFE_NO_PAD
            .decode(ciphertext)
            .ok()
            .filter(|bytes| bytes.len() >= TAG_LEN)
            .ok_or(CryptoError::InvalidPayload {
                reason: "the ciphertext is not base64url or is too short",
            })?;

        let header = header(kid);
        let nonce = Nonce::from(nonce);
        self.keys()
            .filter(|entry| entry.key.kid() == kid)
            .find_map(|entry| {
                entry
                    .cipher
                    .decrypt(
                        &nonce,
                        Payload {
                            msg: &ciphertext,
                            aad: header.as_bytes(),
                        },
                    )
                    .ok()
            })
            .ok_or(CryptoError::Decrypt)
    }

    /// Encrypts a UTF-8 string (see [`encrypt`](Encrypter::encrypt)).
    pub fn encrypt_string(&self, plaintext: &str) -> Result<String> {
        self.encrypt(plaintext.as_bytes())
    }

    /// Decrypts a payload holding UTF-8 text; [`CryptoError::NotUtf8`] when
    /// it holds other bytes.
    pub fn decrypt_string(&self, payload: &str) -> Result<String> {
        String::from_utf8(self.decrypt(payload)?).map_err(|_| CryptoError::NotUtf8)
    }

    /// Serializes `value` to JSON and encrypts it.
    pub fn encrypt_json<T: Serialize + ?Sized>(&self, value: &T) -> Result<String> {
        let json = serde_json::to_vec(value).map_err(json_error)?;
        self.encrypt(&json)
    }

    /// Decrypts a payload and deserializes it from JSON. The error never
    /// quotes the decrypted data.
    pub fn decrypt_json<T: DeserializeOwned>(&self, payload: &str) -> Result<T> {
        serde_json::from_slice(&self.decrypt(payload)?).map_err(json_error)
    }

    /// A 256-bit key for `purpose` (for example `"url-signing"`), derived from
    /// the current application key with HMAC-SHA-256. Use it to sign values
    /// with [`hmac_sha256`] without reusing the encryption key itself; the
    /// same purpose and key always give the same derived key.
    ///
    /// Fails with [`CryptoError::MissingKey`] when no key is configured.
    pub fn derive_key(&self, purpose: &str) -> Result<[u8; 32]> {
        let entry = self.current.as_ref().ok_or(CryptoError::MissingKey)?;
        Ok(hmac_sha256(
            entry.key.expose_bytes(),
            format!("{DERIVE_DOMAIN}{purpose}").as_bytes(),
        ))
    }

    /// The current key (if any), then the previous keys.
    fn keys(&self) -> impl Iterator<Item = &Entry> {
        self.current.iter().chain(&self.previous)
    }
}

/// The authenticated header `v1.<kid>`.
fn header(kid: &str) -> String {
    format!("{VERSION}.{kid}")
}

/// Keeps only the category of a JSON error: its message may quote data.
fn json_error(error: serde_json::Error) -> CryptoError {
    use serde_json::error::Category;
    let category = match error.classify() {
        Category::Io => "io",
        Category::Syntax => "syntax",
        Category::Data => "data",
        Category::Eof => "eof",
    };
    CryptoError::Json { category }
}

/// Shows the key ids only.
impl fmt::Debug for Encrypter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Encrypter")
            .field("current_kid", &self.current_kid())
            .field(
                "previous_kids",
                &self
                    .previous
                    .iter()
                    .map(|entry| entry.key.kid())
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}
