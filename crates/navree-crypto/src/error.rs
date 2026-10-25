//! [`CryptoError`]: everything that can go wrong in navree-crypto.

/// The result type of navree-crypto.
pub type Result<T, E = CryptoError> = std::result::Result<T, E>;

/// An error raised by navree-crypto.
///
/// Messages never contain key material, plaintexts, passwords or decrypted
/// data, so they are safe to log.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CryptoError {
    /// An application key is malformed.
    #[error("invalid application key: {reason}")]
    InvalidKey {
        /// What is wrong (never the key itself).
        reason: &'static str,
    },
    /// No application key is configured, so nothing can be encrypted.
    #[error(
        "no application key is configured: set `app.key` (environment `APP_KEY`) to the output of `key:generate`"
    )]
    MissingKey,
    /// A payload is not of the form `v1.<kid>.<nonce>.<ciphertext>`.
    #[error("invalid encrypted payload: {reason}")]
    InvalidPayload {
        /// What is wrong.
        reason: &'static str,
    },
    /// A payload was tampered with, or was encrypted with a key that is no
    /// longer configured.
    #[error("the payload could not be decrypted: it was modified or its key is not configured")]
    Decrypt,
    /// The plaintext is too long for AES-GCM.
    #[error("the plaintext could not be encrypted: it is too long")]
    Encrypt,
    /// A decrypted payload is not UTF-8 text.
    #[error("the decrypted payload is not valid UTF-8")]
    NotUtf8,
    /// A value could not be serialized to JSON, or a decrypted payload is
    /// not JSON of the requested type.
    #[error("the payload JSON is invalid for the requested type ({category} error)")]
    Json {
        /// The kind of JSON error (`syntax`, `data`, `eof` or `io`); the
        /// underlying message is dropped because it may quote decrypted data.
        category: &'static str,
    },
    /// A stored password hash is malformed or not an Argon2 hash.
    #[error("the stored password hash is malformed or not an Argon2 hash")]
    InvalidHash,
    /// The configured hashing parameters are invalid.
    #[error("invalid password hashing options: {reason}")]
    InvalidHashingOptions {
        /// What is wrong.
        reason: String,
    },
    /// Hashing failed (out of memory, or the hashing task panicked or was
    /// cancelled).
    #[error("password hashing failed: {reason}")]
    Hashing {
        /// What went wrong (never the password).
        reason: String,
    },
}
