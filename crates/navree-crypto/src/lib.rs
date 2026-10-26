//! # navree-crypto
//!
//! `CryptoModule`: AES-256-GCM [`Encrypter`] with key rotation, Argon2id
//! [`Hasher`] and token helpers.
//!
//! - [`Encrypter`] encrypts bytes, strings and JSON values into compact,
//!   URL- and cookie-safe `v1.<kid>.<nonce>.<ciphertext>` payloads (random
//!   96-bit nonce per payload, header authenticated) and decrypts with the
//!   current key or any previous key, so keys can be rotated without
//!   invalidating existing cookies and stored values.
//! - [`Hasher`] hashes and verifies passwords with Argon2id on Tokio's
//!   blocking pool, behind a semaphore, and tells when a stored hash uses
//!   outdated parameters ([`Hasher::needs_rehash`]).
//! - [`random_token`], [`sha256_hex`], [`hmac_sha256`], [`constant_time_eq`]
//!   and [`generate_key`] cover API tokens, reset links and signatures;
//!   [`Encrypter::derive_key`] gives per-purpose signing keys.
//! - [`CryptoModule`] is global and exports `Encrypter` and `Hasher`;
//!   configure it with [`CryptoModule::for_root`] or
//!   [`CryptoModule::from_config`] (`app.key` / `APP_KEY`,
//!   `app.previous_keys`, section `hashing`). It also registers the
//!   `key:generate` command, which prints a new `base64:<32 bytes>` key.
//!
//! Keys, plaintexts and passwords never appear in `Debug` output or error
//! messages.
//!
//! ```
//! use navree_crypto::{AppKey, Encrypter, generate_key, random_token, sha256_hex};
//!
//! let encrypter = Encrypter::new(AppKey::parse(&generate_key())?);
//! let payload = encrypter.encrypt_json(&serde_json::json!({"user_id": 7}))?;
//! let value: serde_json::Value = encrypter.decrypt_json(&payload)?;
//! assert_eq!(value["user_id"], 7);
//!
//! // An API token: show `plain` once, store only its digest.
//! let plain = random_token(40);
//! let stored = sha256_hex(plain.as_bytes());
//! assert_eq!(stored.len(), 64);
//! # Ok::<(), navree_crypto::CryptoError>(())
//! ```
#![forbid(unsafe_code)]

mod encrypter;
mod error;
mod hasher;
mod key;
mod module;
mod tokens;

pub use encrypter::Encrypter;
pub use error::{CryptoError, Result};
pub use hasher::{Hasher, HashingOptions};
pub use key::{AppKey, KEY_LEN, KEY_PREFIX, generate_key};
pub use module::{CryptoModule, CryptoOptions};
pub use tokens::{constant_time_eq, hmac_sha256, random_token, sha256_hex};
