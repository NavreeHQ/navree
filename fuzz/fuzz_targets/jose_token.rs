//! `navree_crypto::jose`: compact JWS parsing and verification against a
//! fixed set of test keys.
//!
//! Byte 0 of the input picks the mode:
//!
//! 0. the rest is a token: it parses, or fails, and everything a parsed
//!    token offers (header, claims, verification with every key and key
//!    set) runs without a panic;
//! 1. the rest is a JSON value: signed with one of the keys, the token
//!    parses, verifies with that key and no other, is found by the key set,
//!    and its claims are the value that was signed;
//! 2. like 1, then one byte of the signed part changes: the token must no
//!    longer verify.
#![no_main]

use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use navree_crypto::jose::{Algorithm, JwkSet, Jws, SigningKey, VerifyingKey};
use navree_fuzz::jose as fixtures;
use serde_json::Value;

struct Fixed {
    signers: Vec<SigningKey>,
    verifiers: Vec<VerifyingKey>,
    sets: Vec<JwkSet>,
}

fn fixed() -> &'static Fixed {
    static FIXED: OnceLock<Fixed> = OnceLock::new();
    FIXED.get_or_init(|| {
        let signers = vec![
            SigningKey::hmac(Algorithm::HS256, [1_u8; 32]).unwrap(),
            SigningKey::hmac(Algorithm::HS384, [2_u8; 48]).unwrap(),
            SigningKey::hmac(Algorithm::HS512, [3_u8; 64]).unwrap(),
            SigningKey::ed25519_from_seed(&[7_u8; 32]).with_thumbprint_kid(),
            SigningKey::from_pem(fixtures::APPLE_P8).unwrap(),
            SigningKey::from_pem(fixtures::RSA_PRIVATE_PEM).unwrap(),
            SigningKey::from_pem(fixtures::RSA_PRIVATE_PEM)
                .unwrap()
                .with_algorithm(Algorithm::PS256)
                .unwrap(),
        ];
        let mut verifiers: Vec<VerifyingKey> =
            signers.iter().map(SigningKey::verifying_key).collect();
        verifiers.push(VerifyingKey::from_pem(fixtures::RSA_PUBLIC_PEM).unwrap());
        verifiers.push(VerifyingKey::from_pem(fixtures::P256_PUBLIC_PEM).unwrap());
        let public: JwkSet = signers.iter().filter_map(SigningKey::public_jwk).collect();
        let mut sets = vec![public];
        for json in [
            fixtures::GOOGLE_JWKS,
            fixtures::APPLE_JWKS,
            fixtures::MICROSOFT_JWKS,
        ] {
            sets.push(serde_json::from_str(json).unwrap());
        }
        Fixed {
            signers,
            verifiers,
            sets,
        }
    })
}

fn parse_and_verify(token: &str) {
    let fixed = fixed();
    let Ok(jws) = Jws::parse(token) else { return };
    let _ = jws.header();
    let _ = jws.payload();
    let _ = jws.claims::<Value>();
    assert!(token.starts_with(jws.signing_input()));
    for key in &fixed.verifiers {
        let _ = jws.verify(key);
    }
    for set in &fixed.sets {
        let _ = jws.verify_with(set);
    }
}

/// Signs `claims` with signer `index`.
fn sign(index: usize, claims: &Value) -> (&'static SigningKey, String) {
    let signer = &fixed().signers[index % fixed().signers.len()];
    let token = signer.sign_jwt(claims).expect("a key signs claims");
    (signer, token)
}

fn round_trip(data: &[u8]) {
    let Some((&index, json)) = data.split_first() else {
        return;
    };
    let Ok(claims) = serde_json::from_slice::<Value>(json) else {
        return;
    };
    let (signer, token) = sign(usize::from(index), &claims);
    let jws = Jws::parse(&token).expect("a signed token parses");
    jws.verify(&signer.verifying_key())
        .expect("a token verifies with its own key");
    assert_eq!(jws.header().alg, signer.algorithm());
    assert_eq!(
        jws.claims::<Value>().expect("the claims read back"),
        claims,
        "the claims round-trip"
    );
    let own = signer.verifying_key().to_jwk();
    let slot = usize::from(index) % fixed().signers.len();
    for (n, other) in fixed().verifiers.iter().enumerate() {
        let same = match (other.to_jwk(), &own) {
            (Some(a), Some(b)) => {
                (&a.kty, &a.n, &a.e, &a.crv, &a.x, &a.y) == (&b.kty, &b.n, &b.e, &b.crv, &b.x, &b.y)
            }
            (None, None) => n == slot,
            _ => false,
        };
        if !same {
            assert!(jws.verify(other).is_err(), "another key verified a token");
        }
    }
    if let Some(public) = signer.public_jwk() {
        let set: JwkSet = [public].into_iter().collect();
        assert!(jws.verify_with(&set).is_ok(), "the key set verifies it");
    }
}

fn tampered(data: &[u8]) {
    let Some((&index, rest)) = data.split_first() else {
        return;
    };
    let Some((&at, json)) = rest.split_first() else {
        return;
    };
    let Ok(claims) = serde_json::from_slice::<Value>(json) else {
        return;
    };
    let (signer, token) = sign(usize::from(index), &claims);
    let signed_len = token.rfind('.').expect("three parts");
    let mut bytes = token.into_bytes();
    let at = usize::from(at) % signed_len;
    bytes[at] ^= 0x01;
    let Ok(changed) = String::from_utf8(bytes) else {
        return;
    };
    if let Ok(jws) = Jws::parse(&changed) {
        assert!(
            jws.verify(&signer.verifying_key()).is_err(),
            "a token verified after its signed part changed"
        );
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else {
        return;
    };
    match mode % 3 {
        0 => {
            if let Ok(token) = std::str::from_utf8(rest) {
                parse_and_verify(token);
            }
        }
        1 => round_trip(rest),
        _ => tampered(rest),
    }
});
