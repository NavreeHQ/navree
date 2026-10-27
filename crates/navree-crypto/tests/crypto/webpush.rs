//! Web Push: `aes128gcm` messages a browser can decrypt, and VAPID headers
//! a push service can verify.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use navree_crypto::hmac_sha256;
use navree_crypto::jose::{Algorithm, Jws, SigningKey, VerifyingKey};
use navree_crypto::webpush::{self, MAX_PAYLOAD, VapidKey, WebPushError};
use ring::aead::{AES_128_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use ring::agreement::{self, ECDH_P256, EphemeralPrivateKey, UnparsedPublicKey};
use ring::rand::SystemRandom;
use serde_json::Value;

/// A browser's side of a subscription: its key pair and auth secret.
struct Browser {
    private: EphemeralPrivateKey,
    public: Vec<u8>,
    auth: [u8; 16],
}

impl Browser {
    fn new() -> Self {
        let private = EphemeralPrivateKey::generate(&ECDH_P256, &SystemRandom::new()).unwrap();
        let public = private.compute_public_key().unwrap().as_ref().to_vec();
        Self {
            private,
            public,
            auth: *b"0123456789abcdef",
        }
    }

    /// What a browser does with a push body (RFC 8291 §3.4, RFC 8188).
    fn decrypt(self, body: &[u8]) -> Vec<u8> {
        let salt = &body[..16];
        let record_size = u32::from_be_bytes(body[16..20].try_into().unwrap());
        assert_eq!(record_size, 4096);
        let id_len = usize::from(body[20]);
        assert_eq!(id_len, 65);
        let as_public = &body[21..21 + id_len];
        let mut record = body[21 + id_len..].to_vec();
        let ecdh_secret = agreement::agree_ephemeral(
            self.private,
            &UnparsedPublicKey::new(&ECDH_P256, as_public),
            |secret| secret.to_vec(),
        )
        .unwrap();
        let prk_key = hmac_sha256(&self.auth, &ecdh_secret);
        let mut key_info = b"WebPush: info\0".to_vec();
        key_info.extend_from_slice(&self.public);
        key_info.extend_from_slice(as_public);
        key_info.push(1);
        let ikm = hmac_sha256(&prk_key, &key_info);
        let prk = hmac_sha256(salt, &ikm);
        let cek = &hmac_sha256(&prk, b"Content-Encoding: aes128gcm\0\x01")[..16];
        let nonce: [u8; 12] = hmac_sha256(&prk, b"Content-Encoding: nonce\0\x01")[..12]
            .try_into()
            .unwrap();
        let key = LessSafeKey::new(UnboundKey::new(&AES_128_GCM, cek).unwrap());
        let plain = key
            .open_in_place(
                Nonce::assume_unique_for_key(nonce),
                Aad::empty(),
                &mut record,
            )
            .expect("the record opens");
        assert_eq!(plain.last(), Some(&2), "the last record's delimiter");
        plain[..plain.len() - 1].to_vec()
    }
}

#[test]
fn a_browser_decrypts_what_encrypt_sealed_for_its_subscription() {
    let browser = Browser::new();
    let payload = br#"{"title":"Deal won","body":"Acme renewal was won"}"#;

    let body = webpush::encrypt(&browser.public, &browser.auth, payload).unwrap();

    assert_eq!(body.len(), 86 + payload.len() + 1 + 16);
    assert_eq!(browser.decrypt(&body), payload);
}

#[test]
fn every_message_has_its_own_salt_and_ephemeral_key() {
    let browser = Browser::new();
    let first = webpush::encrypt(&browser.public, &browser.auth, b"same").unwrap();
    let second = webpush::encrypt(&browser.public, &browser.auth, b"same").unwrap();
    assert_ne!(first[..16], second[..16], "salt");
    assert_ne!(first[21..86], second[21..86], "ephemeral key");
}

#[test]
fn payloads_beyond_one_record_are_refused() {
    let browser = Browser::new();
    assert_eq!(MAX_PAYLOAD, 3993);
    let fits = vec![b'a'; MAX_PAYLOAD];
    assert!(webpush::encrypt(&browser.public, &browser.auth, &fits).is_ok());
    let too_big = vec![b'a'; MAX_PAYLOAD + 1];
    assert_eq!(
        webpush::encrypt(&browser.public, &browser.auth, &too_big),
        Err(WebPushError::PayloadTooLarge {
            len: MAX_PAYLOAD + 1,
            max: MAX_PAYLOAD,
        })
    );
}

#[test]
fn a_p256dh_that_is_not_on_the_curve_is_refused() {
    let mut point = vec![4_u8];
    point.extend([7_u8; 64]);
    assert!(matches!(
        webpush::encrypt(&point, &[1; 16], b"hi"),
        Err(WebPushError::InvalidSubscription(_))
    ));
}

#[test]
fn subscription_keys_decode_padded_or_not() {
    assert_eq!(webpush::decode_key("AQID"), Some(vec![1, 2, 3]));
    assert_eq!(webpush::decode_key("AQI="), Some(vec![1, 2]));
    assert_eq!(webpush::decode_key("not base64!"), None);
}

/// The public key of a VAPID key as a jose verifying key.
fn verifier(vapid: &VapidKey) -> VerifyingKey {
    let point = vapid.public_key();
    VerifyingKey::ec(Algorithm::ES256, &point[1..33], &point[33..65]).unwrap()
}

#[test]
fn the_authorization_header_carries_an_es256_jwt_and_the_public_key() {
    let vapid = VapidKey::generate();

    let header = vapid
        .authorization(
            "https://updates.push.services.mozilla.com",
            "mailto:ops@acme.test",
            1_800_000_000,
        )
        .unwrap();

    let rest = header.strip_prefix("vapid t=").expect("vapid scheme");
    let (token, key) = rest.split_once(", k=").expect("t and k");
    assert_eq!(key, vapid.application_server_key());
    let jws = Jws::parse(token).unwrap();
    jws.verify(&verifier(&vapid)).expect("signed with the key");
    assert_eq!(jws.header().typ.as_deref(), Some("JWT"));
    let claims: Value = jws.claims().unwrap();
    assert_eq!(claims["aud"], "https://updates.push.services.mozilla.com");
    assert_eq!(claims["sub"], "mailto:ops@acme.test");
    assert_eq!(claims["exp"], 1_800_000_000);
}

#[test]
fn keys_round_trip_through_their_base64url_halves() {
    let vapid = VapidKey::generate();
    assert_eq!(vapid.public_key().len(), 65);
    assert_eq!(
        URL_SAFE_NO_PAD
            .decode(vapid.private_key())
            .unwrap()
            .len(),
        32
    );

    let loaded =
        VapidKey::from_base64url(&vapid.application_server_key(), &vapid.private_key()).unwrap();

    assert_eq!(loaded.application_server_key(), vapid.application_server_key());
    let token = loaded.sign("https://fcm.googleapis.com", "mailto:a@b.test", 1).unwrap();
    Jws::parse(&token).unwrap().verify(&verifier(&vapid)).unwrap();
}

#[test]
fn halves_of_different_keys_are_refused() {
    let one = VapidKey::generate();
    let other = VapidKey::generate();
    assert!(matches!(
        VapidKey::from_base64url(&one.application_server_key(), &other.private_key()),
        Err(WebPushError::InvalidKey(_))
    ));
    assert!(matches!(
        VapidKey::from_base64url("short", &one.private_key()),
        Err(WebPushError::InvalidKey(_))
    ));
    assert!(matches!(
        VapidKey::from_base64url(&one.application_server_key(), "%%%"),
        Err(WebPushError::InvalidKey(_))
    ));
}

#[test]
fn a_pem_p256_key_loads_and_other_keys_do_not() {
    let pem = SigningKey::generate_es256().to_pkcs8_pem().unwrap();
    let vapid = VapidKey::from_pem(&pem).unwrap();
    assert_eq!(vapid.public_key().len(), 65);

    let ed25519 = SigningKey::generate_ed25519().to_pkcs8_pem().unwrap();
    assert!(matches!(
        VapidKey::from_pem(&ed25519),
        Err(WebPushError::InvalidKey(_))
    ));
}

#[test]
fn debug_output_shows_only_the_public_key() {
    let vapid = VapidKey::generate();
    let debug = format!("{vapid:?}");
    assert!(debug.contains(&vapid.application_server_key()));
    assert!(!debug.contains(&vapid.private_key()));
}
