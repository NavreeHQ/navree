//! # navree-crypto
//!
//! `CryptoModule`: AES-256-GCM [`Encrypter`] with key rotation, Argon2id
//! [`Hasher`], token helpers, JOSE helpers ([`jose`]), one-time passwords
//! ([`totp`]) and QR codes ([`qr`]).
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
//! - [`jose`] signs and verifies JSON Web Signatures (HS256, EdDSA, ES256,
//!   RS256 and their siblings) on `ring`, reads and publishes JSON Web Keys
//!   and key sets, and loads PKCS#8 keys such as Apple's `.p8` files.
//! - [`totp`] makes and checks time-based one-time passwords (RFC 6238 on
//!   HMAC from `ring`, compared in constant time, secrets wiped on drop) and
//!   writes and reads `otpauth://` key URIs.
//! - [`qr`] encodes text as a QR code (ISO/IEC 18004: versions 1 to 40,
//!   levels L, M, Q and H, numeric, alphanumeric and byte modes, the
//!   standard's mask penalties) and draws it as SVG.
//! - [`webpush`] encrypts Web Push messages (RFC 8291, `aes128gcm`) and
//!   signs their VAPID headers (RFC 8292) with a P-256
//!   [`VapidKey`](webpush::VapidKey).
//! - [`sns`] checks Amazon SNS messages: the text SNS signs, signature
//!   versions 1 and 2 against the signing certificate's key, and the URLs
//!   a certificate or a subscription confirmation may come from.
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
pub mod jose;
mod key;
mod key_command;
mod module;
pub mod qr;
pub mod sns;
mod tokens;
pub mod totp;
pub mod webpush;

pub use encrypter::Encrypter;
pub use error::{CryptoError, Result};
pub use hasher::{Hasher, HashingOptions};
pub use key::{AppKey, KEY_LEN, KEY_PREFIX, generate_key};
pub use module::{CryptoModule, CryptoOptions};
pub use tokens::{constant_time_eq, hmac_sha256, random_token, sha256_hex};
