//! `Encrypter`: AES-256-GCM payloads, key rotation and tamper detection.

use std::collections::HashSet;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use navis_crypto::{AppKey, CryptoError, Encrypter, KEY_LEN};
use serde::{Deserialize, Serialize};

fn key(byte: u8) -> AppKey {
    AppKey::from_bytes([byte; KEY_LEN])
}

/// The four dot-separated segments of a payload.
fn segments(payload: &str) -> Vec<&str> {
    payload.split('.').collect()
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Remember {
    user_id: u64,
    token: String,
}

#[test]
fn encrypt_decrypt_roundtrip() {
    let encrypter = Encrypter::new(key(1));

    for plaintext in [&b""[..], b"x", b"hello world", &[0u8, 255, 1, 254][..]] {
        let payload = encrypter.encrypt(plaintext).expect("encrypt");
        assert_eq!(encrypter.decrypt(&payload).expect("decrypt"), plaintext);
    }

    let payload = encrypter.encrypt_string("s3cret value").expect("encrypt");
    assert!(!payload.contains("s3cret"));
    assert_eq!(
        encrypter.decrypt_string(&payload).expect("decrypt"),
        "s3cret value"
    );

    let value = Remember {
        user_id: 7,
        token: "abc".to_owned(),
    };
    let payload = encrypter.encrypt_json(&value).expect("encrypt");
    assert_eq!(
        encrypter
            .decrypt_json::<Remember>(&payload)
            .expect("decrypt"),
        value
    );
}

#[test]
fn payload_is_compact_url_safe_and_names_the_key() {
    let k = key(1);
    let kid = k.kid().to_owned();
    let encrypter = Encrypter::new(k);
    let payload = encrypter.encrypt(b"cookie value").expect("encrypt");

    let parts = segments(&payload);
    assert_eq!(parts.len(), 4, "{payload}");
    assert_eq!(parts[0], "v1");
    assert_eq!(parts[1], kid);
    assert_eq!(URL_SAFE_NO_PAD.decode(parts[2]).expect("nonce").len(), 12);
    // Ciphertext plus the 16-byte GCM tag.
    assert_eq!(
        URL_SAFE_NO_PAD.decode(parts[3]).expect("ciphertext").len(),
        b"cookie value".len() + 16
    );
    assert!(
        payload
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.')),
        "{payload}"
    );
    assert_eq!(encrypter.current_kid(), Some(kid.as_str()));
}

#[test]
fn decrypt_with_previous_key_after_rotation() {
    let old = Encrypter::new(key(1));
    let old_payload = old.encrypt(b"before rotation").expect("encrypt");

    let rotated = Encrypter::new(key(2)).with_previous_keys([key(1)]);
    assert_eq!(
        rotated
            .decrypt(&old_payload)
            .expect("previous key decrypts"),
        b"before rotation"
    );

    // New payloads use the current key only.
    let new_payload = rotated.encrypt(b"after rotation").expect("encrypt");
    assert_eq!(segments(&new_payload)[1], key(2).kid());
    assert!(old.decrypt(&new_payload).is_err());

    // Once the old key is dropped, its payloads no longer decrypt.
    let error = Encrypter::new(key(2))
        .decrypt(&old_payload)
        .expect_err("unknown key");
    assert!(matches!(error, CryptoError::Decrypt), "{error:?}");
}

#[test]
fn tampered_ciphertext_fails() {
    let encrypter = Encrypter::new(key(1)).with_previous_keys([key(2)]);
    let payload = encrypter.encrypt(b"amount=100").expect("encrypt");
    let parts = segments(&payload);

    let flip = |segment: &str| -> String {
        let mut bytes = URL_SAFE_NO_PAD.decode(segment).expect("decode");
        bytes[0] ^= 0x01;
        URL_SAFE_NO_PAD.encode(bytes)
    };
    let mut tampered = vec![
        // Flipped ciphertext bit, flipped tag bit, flipped nonce bit.
        format!("{}.{}.{}.{}", parts[0], parts[1], parts[2], flip(parts[3])),
        {
            let mut ct = URL_SAFE_NO_PAD.decode(parts[3]).expect("decode");
            let last = ct.len() - 1;
            ct[last] ^= 0x80;
            format!(
                "{}.{}.{}.{}",
                parts[0],
                parts[1],
                parts[2],
                URL_SAFE_NO_PAD.encode(ct)
            )
        },
        format!("{}.{}.{}.{}", parts[0], parts[1], flip(parts[2]), parts[3]),
        // The payload re-labelled with the other configured key's id.
        format!("{}.{}.{}.{}", parts[0], key(2).kid(), parts[2], parts[3]),
        // Truncated ciphertext, missing or extra segments, wrong version.
        format!(
            "{}.{}.{}.{}",
            parts[0],
            parts[1],
            parts[2],
            &parts[3][..parts[3].len() - 4]
        ),
        format!("{}.{}.{}", parts[0], parts[1], parts[3]),
        format!("{payload}.extra"),
        format!("v2.{}.{}.{}", parts[1], parts[2], parts[3]),
        String::new(),
        "not a payload".to_owned(),
    ];
    tampered.push(payload.replace('.', "_"));

    for bad in &tampered {
        assert!(encrypter.decrypt(bad).is_err(), "accepted: {bad}");
    }
    assert_eq!(
        encrypter.decrypt(&payload).expect("original"),
        b"amount=100"
    );
}

#[test]
fn nonces_unique() {
    let encrypter = Encrypter::new(key(1));
    let mut nonces = HashSet::new();
    let mut ciphertexts = HashSet::new();
    for _ in 0..2_000 {
        let payload = encrypter.encrypt(b"same plaintext").expect("encrypt");
        let parts = segments(&payload);
        assert!(nonces.insert(parts[2].to_owned()), "nonce reused");
        assert!(
            ciphertexts.insert(parts[3].to_owned()),
            "ciphertext repeated"
        );
    }
}

#[test]
fn decrypt_errors_do_not_echo_plaintext() {
    let encrypter = Encrypter::new(key(1));
    let payload = encrypter
        .encrypt_json(&serde_json::json!({"user_id": "hunter2-secret"}))
        .expect("encrypt");
    let error = encrypter
        .decrypt_json::<Remember>(&payload)
        .expect_err("wrong shape");
    assert!(matches!(error, CryptoError::Json { .. }), "{error:?}");
    assert!(!error.to_string().contains("hunter2"), "{error}");
    assert!(!format!("{error:?}").contains("hunter2"), "{error:?}");

    let payload = encrypter.encrypt(&[0xff, 0xfe, 0x00]).expect("encrypt");
    let error = encrypter.decrypt_string(&payload).expect_err("not UTF-8");
    assert!(matches!(error, CryptoError::NotUtf8), "{error:?}");
}

#[test]
fn derive_key_is_deterministic_and_separated_by_purpose_and_key() {
    let encrypter = Encrypter::new(key(1));
    let signing = encrypter.derive_key("url-signing").expect("derive");
    assert_eq!(
        signing,
        encrypter.derive_key("url-signing").expect("derive")
    );
    assert_ne!(signing, encrypter.derive_key("storage").expect("derive"));
    assert_ne!(signing, *key(1).expose_bytes());
    assert_ne!(
        signing,
        Encrypter::new(key(2))
            .derive_key("url-signing")
            .expect("derive")
    );
}

#[test]
fn debug_shows_key_ids_only() {
    let encrypter = Encrypter::new(key(1)).with_previous_keys([key(2)]);
    let debug = format!("{encrypter:?}");
    assert!(debug.contains(key(1).kid()), "{debug}");
    assert!(debug.contains(key(2).kid()), "{debug}");
    assert!(!debug.contains("[1, 1"), "{debug}");
}
