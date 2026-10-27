//! Interoperability: keys and tokens made by another implementation
//! (`fixtures/jose/generate.mjs`, on Node's crypto module) load, verify
//! and sign here.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use navree_crypto::jose::{Algorithm, Jwk, JwkSet, Jws, SigningKey, VerifyingKey};
use serde_json::Value;

use super::json_fixture;

fn vectors() -> Value {
    json_fixture("independent-vectors.json")
}

fn jwk(value: &Value) -> Jwk {
    serde_json::from_value(value.clone()).unwrap()
}

/// Verifies `token` with `key` and checks its claims.
fn verifies(token: &str, key: &VerifyingKey, claims: &Value) {
    let jws = Jws::parse(token).unwrap();
    jws.verify(key)
        .unwrap_or_else(|e| panic!("{}: {e}", jws.header().alg));
    assert_eq!(&jws.claims::<Value>().unwrap(), claims);
}

#[test]
fn eddsa_tokens_verify_with_the_public_pem_and_jwk() {
    let v = vectors();
    let ed = &v["ed25519"];
    let token = ed["jws"].as_str().unwrap();
    verifies(
        token,
        &VerifyingKey::from_pem(ed["public_pem"].as_str().unwrap()).unwrap(),
        &v["claims"],
    );
    verifies(
        token,
        &VerifyingKey::from_jwk(&jwk(&ed["public_jwk"])).unwrap(),
        &v["claims"],
    );
    assert_eq!(
        Jws::parse(token).unwrap().header().kid.as_deref(),
        Some("ed-1")
    );
}

#[test]
fn ed25519_private_keys_load_from_pkcs8_v1_pem_and_jwk() {
    let v = vectors();
    let ed = &v["ed25519"];
    let from_pem = SigningKey::from_pem(ed["private_pem"].as_str().unwrap()).unwrap();
    let from_jwk = SigningKey::from_jwk(&jwk(&ed["private_jwk"])).unwrap();
    assert_eq!(from_pem.algorithm(), Algorithm::EdDSA);
    assert_eq!(
        from_pem.public_jwk().unwrap().x,
        ed["public_jwk"]["x"].as_str().map(str::to_owned)
    );
    // Deterministic signatures: both keys sign alike, and the other
    // implementation's token is reproduced exactly.
    let token = ed["jws"].as_str().unwrap();
    let (input, signature) = token.rsplit_once('.').unwrap();
    for key in [&from_pem, &from_jwk] {
        assert_eq!(
            URL_SAFE_NO_PAD.encode(key.sign(input.as_bytes()).unwrap()),
            signature
        );
    }
    // Exported PKCS#8 is the same v1 document.
    assert_eq!(
        from_pem.to_pkcs8_pem().unwrap(),
        ed["private_pem"].as_str().unwrap()
    );
}

#[test]
fn es256_tokens_verify_and_apple_layout_keys_sign() {
    let v = vectors();
    let p256 = &v["p256"];
    let token = p256["jws"].as_str().unwrap();
    for key in [
        VerifyingKey::from_pem(p256["public_pem"].as_str().unwrap()).unwrap(),
        VerifyingKey::from_jwk(&jwk(&p256["public_jwk"])).unwrap(),
    ] {
        verifies(token, &key, &v["claims"]);
    }
    let public = VerifyingKey::from_jwk(&jwk(&p256["public_jwk"])).unwrap();
    // The plain PKCS#8, the `.p8` layout (with the curve in the inner key)
    // and the private JWK are the same key.
    for private in [
        SigningKey::from_pem(p256["private_pem"].as_str().unwrap()).unwrap(),
        SigningKey::from_pem(p256["apple_p8"].as_str().unwrap()).unwrap(),
        SigningKey::from_jwk(&jwk(&p256["private_jwk"])).unwrap(),
    ] {
        assert_eq!(private.algorithm(), Algorithm::ES256);
        let token = private.sign_jwt(&v["claims"]).unwrap();
        verifies(&token, &public, &v["claims"]);
        assert_eq!(
            private.public_jwk().unwrap().thumbprint().unwrap(),
            jwk(&p256["public_jwk"]).thumbprint().unwrap()
        );
    }
}

#[test]
fn es384_tokens_verify_and_p384_keys_sign() {
    let v = vectors();
    let p384 = &v["p384"];
    let public = VerifyingKey::from_pem(p384["public_pem"].as_str().unwrap()).unwrap();
    assert_eq!(public.algorithm(), Some(Algorithm::ES384));
    verifies(p384["jws"].as_str().unwrap(), &public, &v["claims"]);
    verifies(
        p384["jws"].as_str().unwrap(),
        &VerifyingKey::from_jwk(&jwk(&p384["public_jwk"])).unwrap(),
        &v["claims"],
    );
    let private = SigningKey::from_pem(p384["private_pem"].as_str().unwrap()).unwrap();
    assert_eq!(private.algorithm(), Algorithm::ES384);
    let token = private.sign_jwt(&v["claims"]).unwrap();
    assert_eq!(Jws::parse(&token).unwrap().signature().len(), 96);
    verifies(&token, &public, &v["claims"]);
}

#[test]
fn rsa_pkcs1_and_pss_tokens_verify() {
    let v = vectors();
    let rsa = &v["rsa"];
    let from_pem = VerifyingKey::from_pem(rsa["public_pem"].as_str().unwrap()).unwrap();
    let from_jwk = VerifyingKey::from_jwk(&jwk(&rsa["public_jwk"])).unwrap();
    assert_eq!(
        from_pem.to_jwk().unwrap().thumbprint().unwrap(),
        jwk(&rsa["public_jwk"]).thumbprint().unwrap()
    );
    for alg in ["RS256", "RS384", "RS512", "PS256", "PS384", "PS512"] {
        let token = rsa["jws"][alg].as_str().unwrap();
        assert_eq!(Jws::parse(token).unwrap().header().alg.as_str(), alg);
        verifies(token, &from_pem, &v["claims"]);
        verifies(token, &from_jwk, &v["claims"]);
    }
    // Through a key set, as ID tokens are.
    let set = JwkSet::new([Jwk {
        kid: Some("rsa-1".into()),
        ..jwk(&rsa["public_jwk"])
    }]);
    for alg in ["RS256", "PS512"] {
        let jws = Jws::parse(rsa["jws"][alg].as_str().unwrap()).unwrap();
        assert_eq!(jws.verify_with(&set).unwrap().kid.as_deref(), Some("rsa-1"));
    }
}

#[test]
fn rsa_private_keys_sign_what_the_public_key_verifies() {
    let v = vectors();
    let rsa = &v["rsa"];
    let public = VerifyingKey::from_pem(rsa["public_pem"].as_str().unwrap()).unwrap();
    let key = SigningKey::from_pem(rsa["private_pem"].as_str().unwrap()).unwrap();
    assert_eq!(key.algorithm(), Algorithm::RS256);
    for alg in [
        Algorithm::RS256,
        Algorithm::RS512,
        Algorithm::PS256,
        Algorithm::PS384,
    ] {
        let key = SigningKey::from_pem(rsa["private_pem"].as_str().unwrap())
            .unwrap()
            .with_algorithm(alg)
            .unwrap();
        let token = key.sign_jwt(&v["claims"]).unwrap();
        assert_eq!(Jws::parse(&token).unwrap().header().alg, alg);
        verifies(&token, &public, &v["claims"]);
    }
}

#[test]
fn hmac_tokens_verify_for_each_hash() {
    let v = vectors();
    let secret = URL_SAFE_NO_PAD
        .decode(v["hmac"]["secret"].as_str().unwrap())
        .unwrap();
    for alg in [Algorithm::HS256, Algorithm::HS384, Algorithm::HS512] {
        let token = v["hmac"]["jws"][alg.as_str()].as_str().unwrap();
        verifies(
            token,
            &VerifyingKey::hmac(alg, &secret).unwrap(),
            &v["claims"],
        );
        // The same secret signs the same token (HMAC is deterministic, and
        // the header is the same JSON).
        let (input, signature) = token.rsplit_once('.').unwrap();
        let key = SigningKey::hmac(alg, &secret).unwrap();
        assert_eq!(
            URL_SAFE_NO_PAD.encode(key.sign(input.as_bytes()).unwrap()),
            signature
        );
    }
}
