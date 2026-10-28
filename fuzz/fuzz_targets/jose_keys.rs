//! `navree_crypto::jose`: reading keys. Public keys come from identity
//! providers (a JWKS, a PEM public key, an X.509 certificate); private keys
//! from configuration. Byte 0 of the input picks the reader.
//!
//! Invariant: a verifying key written as a JWK and read back writes the
//! same JWK, and a JWK set survives a JSON round trip.
#![no_main]

use libfuzzer_sys::fuzz_target;
use navree_crypto::jose::{Jwk, JwkSet, SigningKey, VerifyingKey};

fn check_verifying(key: &VerifyingKey) {
    let _ = format!("{key:?}");
    let _ = key.kid();
    let _ = key.algorithm();
    if let Some(jwk) = key.to_jwk() {
        let _ = jwk.thumbprint();
        let again = VerifyingKey::from_jwk(&jwk).expect("a key's own JWK reads back");
        assert_eq!(again.to_jwk(), Some(jwk), "the JWK round-trips");
    }
    // Verifying garbage never panics.
    for alg in navree_crypto::jose::Algorithm::ALL {
        let _ = key.verify(alg, b"message", b"signature");
        let _ = key.verify(alg, b"", &[0xFF; 600]);
    }
    let _ = key.verify_legacy_sha1(b"message", &[0; 256]);
}

fn check_signing(key: &SigningKey) {
    let _ = format!("{key:?}");
    let public = key.verifying_key();
    check_verifying(&public);
    // Whatever key was read signs, and the public half verifies.
    if let Ok(token) = key.sign_jwt(&serde_json::json!({ "sub": "fuzz" })) {
        let jws = navree_crypto::jose::Jws::parse(&token).expect("our token parses");
        jws.verify(&public).expect("our token verifies");
    }
    let _ = key.to_pkcs8_pem();
    let _ = key.public_jwk();
}

fuzz_target!(|data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else {
        return;
    };
    let text = std::str::from_utf8(rest).ok();
    match mode % 9 {
        0 => {
            if let Some(Ok(key)) = text.map(VerifyingKey::from_pem) {
                check_verifying(&key);
            }
        }
        1 => {
            if let Ok(key) = VerifyingKey::from_spki_der(rest) {
                check_verifying(&key);
            }
        }
        2 => {
            if let Ok(key) = VerifyingKey::from_certificate_der(rest) {
                check_verifying(&key);
            }
        }
        3 => {
            if let Some(Ok(key)) = text.map(SigningKey::from_pem) {
                check_signing(&key);
            }
        }
        4 => {
            if let Ok(key) = SigningKey::from_pkcs8_der(rest) {
                check_signing(&key);
            }
        }
        5 => {
            if let Ok(jwk) = serde_json::from_slice::<Jwk>(rest) {
                let _ = format!("{jwk:?}");
                let _ = jwk.thumbprint();
                let _ = jwk.to_public();
                let _ = jwk.algorithm();
                let _ = jwk.is_private();
                if let Ok(key) = VerifyingKey::from_jwk(&jwk) {
                    check_verifying(&key);
                }
                if let Ok(key) = SigningKey::from_jwk(&jwk) {
                    check_signing(&key);
                }
            }
        }
        6 => {
            if let Ok(set) = serde_json::from_slice::<JwkSet>(rest) {
                let text = serde_json::to_string(&set).expect("a set serializes");
                let again: JwkSet = serde_json::from_str(&text).expect("a written set reads");
                assert_eq!(again, set, "the set round-trips");
                for jwk in &set.keys {
                    if let Ok(key) = VerifyingKey::from_jwk(jwk) {
                        check_verifying(&key);
                    }
                }
            }
        }
        7 => {
            // The raw parts constructors, from a split of the input.
            let (a, b) = rest.split_at(rest.len() / 2);
            if let Ok(key) = VerifyingKey::rsa(a, b) {
                check_verifying(&key);
            }
            if let Ok(key) = VerifyingKey::ec(navree_crypto::jose::Algorithm::ES256, a, b) {
                check_verifying(&key);
            }
            if let Ok(key) = VerifyingKey::ec(navree_crypto::jose::Algorithm::ES384, a, b) {
                check_verifying(&key);
            }
            if let Ok(key) = VerifyingKey::ed25519(rest) {
                check_verifying(&key);
            }
        }
        _ => {
            if let Ok(key) = SigningKey::hmac(navree_crypto::jose::Algorithm::HS256, rest) {
                check_signing(&key);
            }
        }
    }
});
