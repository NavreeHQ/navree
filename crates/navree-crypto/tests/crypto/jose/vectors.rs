//! Published test vectors: RFC 7515 Appendix A.1 (HS256), A.2 (RS256) and
//! A.3 (ES256), RFC 7638 §3.1 (thumbprints) and RFC 8037 Appendix A
//! (Ed25519).

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use navree_crypto::jose::{Algorithm, Header, JoseError, Jwk, Jws, SigningKey, VerifyingKey};
use serde_json::Value;

use super::json_fixture;

fn vector(name: &str) -> Value {
    json_fixture("rfc-vectors.json")[name].clone()
}

fn jwk(value: &Value) -> Jwk {
    serde_json::from_value(value.clone()).unwrap()
}

/// The payload of the RFC 7515 examples, byte for byte (CRLF line breaks).
const RFC7515_PAYLOAD: &[u8] =
    b"{\"iss\":\"joe\",\r\n \"exp\":1300819380,\r\n \"http://example.com/is_root\":true}";

#[test]
fn rfc7515_a1_hs256_signature_matches_and_verifies() {
    let v = vector("rfc7515_a1");
    let token = v["jws"].as_str().unwrap();
    let key = SigningKey::from_jwk(&jwk(&v["jwk"])).unwrap();
    assert_eq!(key.algorithm(), Algorithm::HS256);

    // HMAC is deterministic: signing the same input gives the same signature.
    let (input, signature) = token.rsplit_once('.').unwrap();
    assert_eq!(
        URL_SAFE_NO_PAD.encode(key.sign(input.as_bytes()).unwrap()),
        signature
    );

    let jws = Jws::parse(token).unwrap();
    assert_eq!(jws.header().alg, Algorithm::HS256);
    assert_eq!(jws.header().typ.as_deref(), Some("JWT"));
    assert_eq!(jws.payload(), RFC7515_PAYLOAD);
    assert_eq!(jws.signing_input(), input);
    let claims: Value = jws.claims().unwrap();
    assert_eq!(claims["iss"], "joe");
    assert_eq!(claims["http://example.com/is_root"], true);

    let verifying = VerifyingKey::from_jwk(&jwk(&v["jwk"])).unwrap();
    jws.verify(&verifying).unwrap();
    jws.verify(&key.verifying_key()).unwrap();

    // Another secret of the same length does not verify.
    let other = VerifyingKey::hmac(Algorithm::HS256, [7_u8; 64]).unwrap();
    assert_eq!(jws.verify(&other), Err(JoseError::BadSignature));
}

#[test]
fn rfc7515_a2_rs256_signature_matches_and_verifies() {
    let v = vector("rfc7515_a2");
    let token = v["jws"].as_str().unwrap();
    let private = jwk(&v["jwk"]);
    let key = SigningKey::from_jwk(&private).unwrap();
    assert_eq!(key.algorithm(), Algorithm::RS256);

    // RSASSA-PKCS1-v1_5 is deterministic too.
    let (input, signature) = token.rsplit_once('.').unwrap();
    assert_eq!(
        URL_SAFE_NO_PAD.encode(key.sign(input.as_bytes()).unwrap()),
        signature
    );
    let header = Header::new(Algorithm::RS256);
    assert_eq!(key.sign_jws(&header, RFC7515_PAYLOAD).unwrap(), token);

    // Verify-only: the public members.
    let public = private.to_public();
    assert!(!public.is_private());
    let jws = Jws::parse(token).unwrap();
    jws.verify(&VerifyingKey::from_jwk(&public).unwrap())
        .unwrap();

    // The same key from its raw components (`RsaPublicKeyComponents`).
    let n = URL_SAFE_NO_PAD.decode(public.n.as_deref().unwrap()).unwrap();
    let e = URL_SAFE_NO_PAD.decode(public.e.as_deref().unwrap()).unwrap();
    jws.verify(&VerifyingKey::rsa(&n, &e).unwrap()).unwrap();
}

#[test]
fn rfc7515_a3_es256_verifies_and_the_private_key_signs() {
    let v = vector("rfc7515_a3");
    let token = v["jws"].as_str().unwrap();
    let private = jwk(&v["jwk"]);
    let jws = Jws::parse(token).unwrap();
    assert_eq!(jws.header().alg, Algorithm::ES256);
    jws.verify(&VerifyingKey::from_jwk(&private.to_public()).unwrap())
        .unwrap();

    // ECDSA signatures are randomized: a new signature differs but verifies.
    let key = SigningKey::from_jwk(&private).unwrap();
    assert_eq!(key.algorithm(), Algorithm::ES256);
    let fresh = key
        .sign_jws(&Header::new(Algorithm::ES256), RFC7515_PAYLOAD)
        .unwrap();
    assert_ne!(fresh, token);
    let fresh = Jws::parse(&fresh).unwrap();
    assert_eq!(fresh.signature().len(), 64, "R || S, 32 bytes each");
    fresh
        .verify(&VerifyingKey::from_jwk(&private).unwrap())
        .unwrap();
}

#[test]
fn rfc7638_thumbprint_of_an_rsa_key() {
    let v = vector("rfc7638_3_1");
    let key = jwk(&v["jwk"]);
    assert_eq!(key.thumbprint().unwrap(), v["thumbprint"]);
    // Members outside the required ones (alg, kid) do not change it.
    let bare = Jwk {
        alg: None,
        kid: None,
        ..key.clone()
    };
    assert_eq!(bare.thumbprint().unwrap(), v["thumbprint"]);
}

#[test]
fn rfc8037_ed25519_keys_thumbprint_and_signature() {
    let v = vector("rfc8037_a");
    let private = jwk(&v["private_jwk"]);
    assert!(private.is_private());

    // A.1 and A.2: the private key and its public half.
    let key = SigningKey::from_jwk(&private).unwrap();
    assert_eq!(key.algorithm(), Algorithm::EdDSA);
    let public = key.public_jwk().unwrap();
    assert_eq!(public.kty, "OKP");
    assert_eq!(public.crv.as_deref(), Some("Ed25519"));
    assert_eq!(public.x, private.x);
    assert_eq!(public.d, None);

    // A.3: the thumbprint, of the private and of the public JWK alike.
    assert_eq!(private.thumbprint().unwrap(), v["thumbprint"]);
    assert_eq!(public.thumbprint().unwrap(), v["thumbprint"]);
    assert_eq!(
        key.clone().with_thumbprint_kid().kid(),
        v["thumbprint"].as_str()
    );

    // A.4: Ed25519 is deterministic, so the whole token matches.
    let token = v["jws"].as_str().unwrap();
    let signed = key
        .sign_jws(
            &Header::new(Algorithm::EdDSA),
            b"Example of Ed25519 signing",
        )
        .unwrap();
    assert_eq!(signed, token);

    // A.5: validation.
    let jws = Jws::parse(token).unwrap();
    assert_eq!(jws.payload(), b"Example of Ed25519 signing");
    jws.verify(&VerifyingKey::from_jwk(&public).unwrap())
        .unwrap();

    // The same key from its seed.
    let seed: [u8; 32] = URL_SAFE_NO_PAD
        .decode(private.d.as_deref().unwrap())
        .unwrap()
        .try_into()
        .unwrap();
    assert_eq!(
        SigningKey::ed25519_from_seed(&seed)
            .sign_jws(
                &Header::new(Algorithm::EdDSA),
                b"Example of Ed25519 signing"
            )
            .unwrap(),
        token
    );
}
