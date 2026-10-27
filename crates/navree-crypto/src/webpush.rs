//! Web Push on `ring`: message encryption ([RFC 8291], `aes128gcm`) and
//! application server identification ([RFC 8292], VAPID).
//!
//! - [`encrypt`] encrypts a push message for one browser subscription: an
//!   ephemeral P-256 key agrees on a secret with the subscription's
//!   `p256dh` key, HKDF mixes in its `auth` secret, and AES-128-GCM seals
//!   the payload into one `aes128gcm` record (RFC 8188) whose header
//!   carries the salt and the ephemeral public key. Send the result as the
//!   request body with `Content-Encoding: aes128gcm`.
//! - [`VapidKey`] is the application server's P-256 key pair. Its
//!   [`application_server_key`](VapidKey::application_server_key) is what a
//!   browser's `pushManager.subscribe({ applicationServerKey })` takes, and
//!   [`authorization`](VapidKey::authorization) signs the ES256 JWT of the
//!   `Authorization: vapid t=..., k=...` header of each push request.
//!
//! Keys and secrets never appear in `Debug` output or error messages.
//!
//! ```
//! use navree_crypto::webpush::{self, VapidKey};
//!
//! let vapid = VapidKey::generate();
//! // Store both halves (`application_server_key()` and `private_key()`),
//! // and load them again with `VapidKey::from_base64url`.
//! let loaded = VapidKey::from_base64url(&vapid.application_server_key(), &vapid.private_key())?;
//! assert_eq!(loaded.application_server_key(), vapid.application_server_key());
//!
//! let header = loaded.authorization(
//!     "https://fcm.googleapis.com",
//!     "mailto:ops@example.com",
//!     1_800_000_000,
//! )?;
//! assert!(header.starts_with("vapid t="));
//! # Ok::<(), navree_crypto::webpush::WebPushError>(())
//! ```
//!
//! [RFC 8291]: https://www.rfc-editor.org/rfc/rfc8291
//! [RFC 8292]: https://www.rfc-editor.org/rfc/rfc8292

use std::fmt;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::aead::{AES_128_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use ring::agreement::{self, ECDH_P256, EphemeralPrivateKey, UnparsedPublicKey};
use ring::rand::{SecureRandom as _, SystemRandom};
use serde_json::json;

use crate::jose::{Algorithm, Header, SigningKey};
use crate::tokens::hmac_sha256;

/// The record size written in the `aes128gcm` header.
pub const RECORD_SIZE: u32 = 4096;

/// The largest payload [`encrypt`] accepts: one 4096-byte record less the
/// header (86 bytes), the AES-GCM tag (16) and the padding delimiter (1).
pub const MAX_PAYLOAD: usize = RECORD_SIZE as usize - HEADER_LEN - TAG_LEN - 1;

/// The `aes128gcm` header: salt (16), record size (4), key id length (1)
/// and the ephemeral public key (65).
const HEADER_LEN: usize = 16 + 4 + 1 + POINT_LEN;
/// An uncompressed P-256 point.
const POINT_LEN: usize = 65;
/// The AES-GCM tag.
const TAG_LEN: usize = 16;
/// The shortest `auth` secret browsers send (RFC 8291: 16 bytes).
const AUTH_LEN: usize = 16;

/// Why a push message could not be encrypted or signed. Messages never
/// contain keys, secrets or payloads.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum WebPushError {
    /// A VAPID key is malformed (not base64url, not a P-256 key pair).
    #[error("invalid VAPID key: {0}")]
    InvalidKey(&'static str),
    /// A subscription's `p256dh` or `auth` key is malformed.
    #[error("invalid push subscription: {0}")]
    InvalidSubscription(&'static str),
    /// The payload does not fit in one push message.
    #[error("the push payload is {len} bytes; the limit is {max}")]
    PayloadTooLarge {
        /// The payload's length.
        len: usize,
        /// [`MAX_PAYLOAD`].
        max: usize,
    },
    /// The operating system's random number generator or `ring` failed.
    #[error("the push message could not be encrypted")]
    Crypto,
}

/// Encrypts `payload` for a browser push subscription whose keys are
/// `p256dh` (the uncompressed P-256 public key, 65 bytes) and `auth` (16
/// bytes), both as the browser gives them in `subscription.toJSON().keys`
/// once base64url-decoded. Returns the `aes128gcm` body: a header with a
/// random salt and a fresh ephemeral public key, then one sealed record.
pub fn encrypt(p256dh: &[u8], auth: &[u8], payload: &[u8]) -> Result<Vec<u8>, WebPushError> {
    check_subscription(p256dh, auth)?;
    if payload.len() > MAX_PAYLOAD {
        return Err(WebPushError::PayloadTooLarge {
            len: payload.len(),
            max: MAX_PAYLOAD,
        });
    }
    let rng = SystemRandom::new();
    let ephemeral =
        EphemeralPrivateKey::generate(&ECDH_P256, &rng).map_err(|_| WebPushError::Crypto)?;
    let as_public = ephemeral
        .compute_public_key()
        .map_err(|_| WebPushError::Crypto)?;
    let mut salt = [0_u8; 16];
    rng.fill(&mut salt).map_err(|_| WebPushError::Crypto)?;
    let ecdh_secret = agreement::agree_ephemeral(
        ephemeral,
        &UnparsedPublicKey::new(&ECDH_P256, p256dh),
        |secret| secret.to_vec(),
    )
    .map_err(|_| WebPushError::InvalidSubscription("`p256dh` is not a P-256 public key"))?;
    seal(
        &ecdh_secret,
        &Keys {
            ua_public: p256dh,
            as_public: as_public.as_ref(),
            auth,
            salt: &salt,
        },
        payload,
    )
}

/// Decodes a base64url member of a subscription (`p256dh`, `auth`),
/// padded or not.
pub fn decode_key(text: &str) -> Option<Vec<u8>> {
    URL_SAFE_NO_PAD
        .decode(text.trim().trim_end_matches('='))
        .ok()
}

/// Checks the shape of a subscription's keys.
fn check_subscription(p256dh: &[u8], auth: &[u8]) -> Result<(), WebPushError> {
    if p256dh.len() != POINT_LEN || p256dh[0] != 0x04 {
        return Err(WebPushError::InvalidSubscription(
            "`p256dh` is not an uncompressed P-256 public key",
        ));
    }
    if auth.len() < AUTH_LEN {
        return Err(WebPushError::InvalidSubscription(
            "`auth` is shorter than 16 bytes",
        ));
    }
    Ok(())
}

/// The keys of one message.
struct Keys<'a> {
    ua_public: &'a [u8],
    as_public: &'a [u8],
    auth: &'a [u8],
    salt: &'a [u8; 16],
}

/// RFC 8291 §3.3-3.4 and RFC 8188 §2: derives the content key and nonce
/// from the ECDH secret and seals `payload` in one record (delimiter
/// `0x02`, no padding) after the header.
fn seal(ecdh_secret: &[u8], keys: &Keys<'_>, payload: &[u8]) -> Result<Vec<u8>, WebPushError> {
    let (cek, nonce) = derive(ecdh_secret, keys);
    let key = UnboundKey::new(&AES_128_GCM, &cek).map_err(|_| WebPushError::Crypto)?;
    let key = LessSafeKey::new(key);
    let mut record = Vec::with_capacity(payload.len() + 1 + TAG_LEN);
    record.extend_from_slice(payload);
    record.push(0x02);
    key.seal_in_place_append_tag(
        Nonce::assume_unique_for_key(nonce),
        Aad::empty(),
        &mut record,
    )
    .map_err(|_| WebPushError::Crypto)?;
    let mut body = Vec::with_capacity(HEADER_LEN + record.len());
    body.extend_from_slice(keys.salt);
    body.extend_from_slice(&RECORD_SIZE.to_be_bytes());
    body.push(POINT_LEN as u8);
    body.extend_from_slice(keys.as_public);
    body.extend_from_slice(&record);
    Ok(body)
}

/// The content encryption key (16 bytes) and the nonce (12 bytes).
fn derive(ecdh_secret: &[u8], keys: &Keys<'_>) -> ([u8; 16], [u8; 12]) {
    // PRK_key = HMAC(auth_secret, ecdh_secret); IKM = HKDF-Expand(PRK_key,
    // "WebPush: info" 0x00 ua_public as_public, 32).
    let prk_key = hmac_sha256(keys.auth, ecdh_secret);
    let mut key_info = b"WebPush: info\0".to_vec();
    key_info.extend_from_slice(keys.ua_public);
    key_info.extend_from_slice(keys.as_public);
    key_info.push(0x01);
    let ikm = hmac_sha256(&prk_key, &key_info);
    // PRK = HKDF-Extract(salt, IKM); CEK and NONCE expand from it.
    let prk = hmac_sha256(keys.salt, &ikm);
    let cek_block = hmac_sha256(&prk, b"Content-Encoding: aes128gcm\0\x01");
    let nonce_block = hmac_sha256(&prk, b"Content-Encoding: nonce\0\x01");
    let mut cek = [0_u8; 16];
    cek.copy_from_slice(&cek_block[..16]);
    let mut nonce = [0_u8; 12];
    nonce.copy_from_slice(&nonce_block[..12]);
    (cek, nonce)
}

/// The application server's VAPID key pair (P-256, ES256).
///
/// Generate one with [`generate`](VapidKey::generate) (or the `notify:vapid`
/// command of navree-notify) and keep both halves: browsers subscribe with
/// the public [`application_server_key`](VapidKey::application_server_key),
/// and a new key invalidates every existing subscription.
#[derive(Clone)]
pub struct VapidKey {
    key: SigningKey,
    public: Vec<u8>,
}

impl VapidKey {
    /// A new random key pair.
    pub fn generate() -> Self {
        Self::from_signing_key(SigningKey::generate_es256())
            .expect("a generated ES256 key is a P-256 key pair")
    }

    /// Loads a key pair from its base64url halves: `public_key` the
    /// uncompressed point (65 bytes, the application server key) and
    /// `private_key` the private scalar (32 bytes), the form Web Push
    /// libraries print.
    pub fn from_base64url(public_key: &str, private_key: &str) -> Result<Self, WebPushError> {
        let public = decode_key(public_key)
            .ok_or(WebPushError::InvalidKey("the public key is not base64url"))?;
        let private = decode_key(private_key)
            .ok_or(WebPushError::InvalidKey("the private key is not base64url"))?;
        if public.len() != POINT_LEN || public[0] != 0x04 {
            return Err(WebPushError::InvalidKey(
                "the public key is not an uncompressed P-256 point",
            ));
        }
        if private.len() != 32 {
            return Err(WebPushError::InvalidKey(
                "the private key is not a 32-byte P-256 scalar",
            ));
        }
        let key = SigningKey::es256_from_parts(&private, &public)
            .map_err(|_| WebPushError::InvalidKey("the halves are not one P-256 key pair"))?;
        Self::from_signing_key(key)
    }

    /// Loads a P-256 private key from PEM (`PRIVATE KEY` or
    /// `EC PRIVATE KEY` with its public key).
    pub fn from_pem(pem: &str) -> Result<Self, WebPushError> {
        let key = SigningKey::from_pem(pem)
            .map_err(|_| WebPushError::InvalidKey("not a PEM P-256 private key"))?;
        Self::from_signing_key(key)
    }

    fn from_signing_key(key: SigningKey) -> Result<Self, WebPushError> {
        let (_, public) = key
            .p256_parts()
            .ok_or(WebPushError::InvalidKey("not a P-256 key"))?;
        let public = public.to_vec();
        Ok(Self { key, public })
    }

    /// The public key, an uncompressed P-256 point (65 bytes).
    pub fn public_key(&self) -> &[u8] {
        &self.public
    }

    /// The public key in base64url without padding: the
    /// `applicationServerKey` browsers subscribe with, and the `k` of the
    /// `Authorization` header.
    pub fn application_server_key(&self) -> String {
        URL_SAFE_NO_PAD.encode(&self.public)
    }

    /// The private scalar in base64url without padding (32 bytes), to
    /// store next to the public key.
    pub fn private_key(&self) -> String {
        let (d, _) = self.key.p256_parts().expect("a VapidKey holds a P-256 key");
        URL_SAFE_NO_PAD.encode(d)
    }

    /// The signed VAPID JWT for the push service at `audience` (the origin
    /// of the subscription's endpoint, `https://fcm.googleapis.com`), with
    /// `subject` (a `mailto:` or `https:` contact) and `expires_at` (Unix
    /// seconds; push services refuse more than 24 hours ahead).
    pub fn sign(
        &self,
        audience: &str,
        subject: &str,
        expires_at: i64,
    ) -> Result<String, WebPushError> {
        let header = Header::new(Algorithm::ES256).with_typ("JWT");
        let claims = json!({ "aud": audience, "exp": expires_at, "sub": subject });
        self.key
            .sign_json(&header, &claims)
            .map_err(|_| WebPushError::Crypto)
    }

    /// The `Authorization` header of a push request: `vapid t=<jwt>,
    /// k=<application server key>` (see [`sign`](VapidKey::sign)).
    pub fn authorization(
        &self,
        audience: &str,
        subject: &str,
        expires_at: i64,
    ) -> Result<String, WebPushError> {
        let token = self.sign(audience, subject, expires_at)?;
        Ok(format!(
            "vapid t={token}, k={}",
            self.application_server_key()
        ))
    }
}

impl fmt::Debug for VapidKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VapidKey")
            .field("application_server_key", &self.application_server_key())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b64(text: &str) -> Vec<u8> {
        decode_key(text).expect("base64url")
    }

    /// RFC 8291, Appendix A: the content key, nonce and message for a
    /// known ECDH secret, salt and keys.
    #[test]
    fn the_rfc_8291_example_encrypts_to_the_published_message() {
        let ua_public = b64(
            "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4",
        );
        let as_public = b64(
            "BP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A8",
        );
        let auth = b64("BTBZMqHH6r4Tts7J_aSIgg");
        let salt: [u8; 16] = b64("DGv6ra1nlYgDCS1FRnbzlw").try_into().unwrap();
        let ecdh_secret = b64("kyrL1jIIOHEzg3sM2ZWRHDRB62YACZhhSlknJ672kSs");
        let keys = Keys {
            ua_public: &ua_public,
            as_public: &as_public,
            auth: &auth,
            salt: &salt,
        };

        let (cek, nonce) = derive(&ecdh_secret, &keys);
        assert_eq!(URL_SAFE_NO_PAD.encode(cek), "oIhVW04MRdy2XN9CiKLxTg");
        assert_eq!(URL_SAFE_NO_PAD.encode(nonce), "4h_95klXJ5E_qnoN");

        let body = seal(
            &ecdh_secret,
            &keys,
            b"When I grow up, I want to be a watermelon",
        )
        .unwrap();
        assert_eq!(
            URL_SAFE_NO_PAD.encode(&body),
            "DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A_yl95bQpu6cVPTpK4Mqgkf1CXztLVBSt2Ks3oZwbuwXPXLWyouBWLVWGNWQexSgSxsj_Qulcy4a-fN"
        );
    }

    #[test]
    fn subscriptions_with_malformed_keys_are_refused() {
        let point = [4_u8; 65];
        assert!(matches!(
            encrypt(&point[..64], &[0; 16], b"hi"),
            Err(WebPushError::InvalidSubscription(_))
        ));
        assert!(matches!(
            encrypt(&point, &[0; 8], b"hi"),
            Err(WebPushError::InvalidSubscription(_))
        ));
    }
}
