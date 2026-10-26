//! HMAC helpers, Standard Webhooks keys and secrets.

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
use hmac::{KeyInit as _, Mac as _};
use rand::RngExt as _;
use sha1::Sha1;
use sha2::{Sha256, Sha512};
use subtle::ConstantTimeEq as _;

use crate::options::{HmacAlgorithm, SignatureEncoding};

/// The prefix of Standard Webhooks secrets.
pub const SECRET_PREFIX: &str = "whsec_";

/// Random bytes in a generated endpoint secret.
const SECRET_BYTES: usize = 24;

/// HMAC of the concatenation of `parts` under `key`.
pub(crate) fn hmac(algorithm: HmacAlgorithm, key: &[u8], parts: &[&[u8]]) -> Vec<u8> {
    macro_rules! mac {
        ($hash:ty) => {{
            let mut mac =
                <hmac::Hmac<$hash>>::new_from_slice(key).expect("HMAC accepts keys of any length");
            for part in parts {
                mac.update(part);
            }
            mac.finalize().into_bytes().to_vec()
        }};
    }
    match algorithm {
        HmacAlgorithm::Sha1 => mac!(Sha1),
        HmacAlgorithm::Sha256 => mac!(Sha256),
        HmacAlgorithm::Sha512 => mac!(Sha512),
    }
}

/// HMAC-SHA256 of the concatenation of `parts` under `key`.
pub(crate) fn hmac_sha256(key: &[u8], parts: &[&[u8]]) -> Vec<u8> {
    hmac(HmacAlgorithm::Sha256, key, parts)
}

/// Compares in time that depends only on the lengths.
pub(crate) fn same(a: &[u8], b: &[u8]) -> bool {
    a.ct_eq(b).into()
}

/// Decodes a signature written with `encoding`; `None` when it is not.
pub(crate) fn decode(encoding: SignatureEncoding, text: &str) -> Option<Vec<u8>> {
    let text = text.trim();
    match encoding {
        SignatureEncoding::Hex => hex::decode(text).ok(),
        SignatureEncoding::Base64 => [STANDARD, URL_SAFE, STANDARD_NO_PAD, URL_SAFE_NO_PAD]
            .iter()
            .find_map(|engine| engine.decode(text).ok()),
    }
}

/// Writes a signature with `encoding` (lower-case hex, padded standard
/// base64).
pub(crate) fn encode(encoding: SignatureEncoding, bytes: &[u8]) -> String {
    match encoding {
        SignatureEncoding::Hex => hex::encode(bytes),
        SignatureEncoding::Base64 => STANDARD.encode(bytes),
    }
}

/// The HMAC key of a Standard Webhooks secret: the bytes after `whsec_`,
/// base64-decoded. A secret that is not base64 is used as it is.
pub(crate) fn standard_key(secret: &str) -> Vec<u8> {
    let encoded = secret.strip_prefix(SECRET_PREFIX).unwrap_or(secret);
    STANDARD
        .decode(encoded)
        .unwrap_or_else(|_| secret.as_bytes().to_vec())
}

/// The Standard Webhooks signature (`v1,<base64>`) of `body` sent as
/// message `id` at `timestamp`.
pub(crate) fn standard_signature(key: &[u8], id: &str, timestamp: i64, body: &[u8]) -> String {
    let timestamp = timestamp.to_string();
    let mac = hmac_sha256(
        key,
        &[id.as_bytes(), b".", timestamp.as_bytes(), b".", body],
    );
    format!("v1,{}", STANDARD.encode(mac))
}

/// A new endpoint secret: `whsec_` and 24 random bytes in base64 (32
/// characters), from the operating system's generator.
pub fn generate_secret() -> String {
    let mut bytes = [0u8; SECRET_BYTES];
    rand::rng().fill(&mut bytes[..]);
    format!("{SECRET_PREFIX}{}", STANDARD.encode(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_webhooks_spec_vector() {
        let key = standard_key("whsec_MfKQ9r8GKYqrTwjUPD8ILPZIo2LaLaSw");
        assert_eq!(
            standard_signature(
                &key,
                "msg_p5jXN8AQM9LWM0D4loKWxJek",
                1_614_265_330,
                br#"{"test": 2432232314}"#
            ),
            "v1,g0hM9SsE+OTPJTGt/tmIKtSyZlE3uFJELVlNIOLJ1OE="
        );
    }

    #[test]
    fn hmac_algorithms_match_reference_values() {
        let body = br#"{"event":"invitee.created"}"#;
        assert_eq!(
            encode(
                SignatureEncoding::Base64,
                &hmac(
                    HmacAlgorithm::Sha1,
                    b"calendly-key",
                    &[b"1792000000.", body]
                )
            ),
            "hAv2f9h2daKeafdkvN5tM1rwk48="
        );
        assert_eq!(
            encode(
                SignatureEncoding::Hex,
                &hmac(
                    HmacAlgorithm::Sha256,
                    b"calendly-key",
                    &[b"1792000000.", body]
                )
            ),
            "4e4a42d353d812bbb128b5c8a34fad2cde6afb03c8d5b34d2059122122cb984f"
        );
        assert_eq!(
            encode(
                SignatureEncoding::Hex,
                &hmac(
                    HmacAlgorithm::Sha512,
                    b"slack-key",
                    &[b"v0:1792000000:token=abc"]
                )
            ),
            "d0cef74b692a955154e2f73d26e9a4d680f6a0c7797647437554c7a6db38366c7574cfb50821db5911aa9c3b9412769d9123caf0d90a2708e19516fb57bfbbe2"
        );
    }

    #[test]
    fn signatures_decode_from_either_alphabet_and_case() {
        assert_eq!(
            decode(SignatureEncoding::Hex, "0aFF"),
            Some(vec![0x0a, 0xff])
        );
        assert_eq!(decode(SignatureEncoding::Hex, "zz"), None);
        assert_eq!(
            decode(SignatureEncoding::Base64, "+/8="),
            Some(vec![0xfb, 0xff])
        );
        assert_eq!(
            decode(SignatureEncoding::Base64, "-_8"),
            Some(vec![0xfb, 0xff])
        );
    }

    #[test]
    fn generated_secrets_are_whsec_base64_of_24_random_bytes() {
        let secret = generate_secret();
        assert!(secret.starts_with("whsec_"), "{secret}");
        assert_eq!(secret.len(), "whsec_".len() + 32);
        assert_eq!(standard_key(&secret).len(), 24);
        assert_ne!(secret, generate_secret());
    }
}
