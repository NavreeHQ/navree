//! Loading keys from PEM, PKCS#8 and JWK; generating and exporting them.

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use navree_crypto::jose::{Algorithm, JoseError, Jwk, Jws, SigningKey, VerifyingKey};
use serde_json::json;

use super::{fixture, json_fixture};

/// The DER inside a PEM document.
fn der(pem: &str) -> Vec<u8> {
    let body: String = pem.lines().filter(|l| !l.starts_with("-----")).collect();
    STANDARD.decode(body).unwrap()
}

#[test]
fn apple_p8_keys_load_and_sign_es256() {
    let pem = fixture("apple-authkey.p8");
    let key = SigningKey::from_pem(&pem).unwrap().with_kid("KEYID");
    assert_eq!(key.algorithm(), Algorithm::ES256);
    assert_eq!(key.kid(), Some("KEYID"));
    let token = key
        .sign_json(
            &key.header(),
            &json!({"iss": "TEAMID", "sub": "com.acme.web"}),
        )
        .unwrap();
    let jws = Jws::parse(&token).unwrap();
    assert_eq!(jws.header().kid.as_deref(), Some("KEYID"));
    jws.verify(&VerifyingKey::from_jwk(&key.public_jwk().unwrap()).unwrap())
        .unwrap();

    // The same key in one line with `\n` escapes (an environment variable),
    // without its armor lines, or as raw DER.
    let escaped = pem.trim().replace('\n', "\\n");
    let bare: String = pem.lines().filter(|l| !l.starts_with("-----")).collect();
    for variant in [
        SigningKey::from_pem(&escaped).unwrap(),
        SigningKey::from_pem(&bare).unwrap(),
        SigningKey::from_pkcs8_der(&der(&pem)).unwrap(),
    ] {
        assert_eq!(
            variant.public_jwk(),
            SigningKey::from_pem(&pem).unwrap().public_jwk()
        );
    }
}

#[test]
fn pkcs8_loading_detects_the_key_type() {
    let v = json_fixture("independent-vectors.json");
    let cases = [
        (
            v["ed25519"]["private_pem"].as_str().unwrap().to_owned(),
            Algorithm::EdDSA,
        ),
        (
            v["p256"]["apple_p8"].as_str().unwrap().to_owned(),
            Algorithm::ES256,
        ),
        (
            v["p384"]["private_pem"].as_str().unwrap().to_owned(),
            Algorithm::ES384,
        ),
        (fixture("rsa-2048.pem"), Algorithm::RS256),
    ];
    for (pem, alg) in cases {
        assert_eq!(SigningKey::from_pem(&pem).unwrap().algorithm(), alg);
        assert_eq!(
            SigningKey::from_pkcs8_der(&der(&pem)).unwrap().algorithm(),
            alg
        );
    }
}

/// Certificates made with `openssl req -x509 -key <fixture>` from the RSA
/// and P-256 fixture keys.
#[test]
fn certificates_give_their_public_key() {
    for (key, cert, alg) in [
        ("rsa-2048.pem", "rsa-2048.crt", Algorithm::RS256),
        ("apple-authkey.p8", "apple-authkey.crt", Algorithm::ES256),
    ] {
        let signer = SigningKey::from_pem(&fixture(key)).unwrap();
        let signature = signer.sign(b"signed by the certificate's key").unwrap();
        let pem = fixture(cert);
        for verifier in [
            VerifyingKey::from_pem(&pem).unwrap(),
            VerifyingKey::from_certificate_der(&der(&pem)).unwrap(),
        ] {
            assert_eq!(verifier.to_jwk().unwrap().n, signer.public_jwk().unwrap().n);
            verifier
                .verify(alg, b"signed by the certificate's key", &signature)
                .unwrap();
            assert!(verifier.verify(alg, b"something else", &signature).is_err());
        }
    }
    // A SubjectPublicKeyInfo is not a certificate, and a truncated
    // certificate is refused.
    let v = json_fixture("independent-vectors.json");
    let spki = der(v["p256"]["public_pem"].as_str().unwrap());
    assert!(VerifyingKey::from_certificate_der(&spki).is_err());
    let mut short = der(&fixture("rsa-2048.crt"));
    short.truncate(200);
    assert!(VerifyingKey::from_certificate_der(&short).is_err());
}

#[test]
fn bad_keys_are_refused_with_a_reason() {
    let v = json_fixture("independent-vectors.json");
    for (input, why) in [
        ("not a key", "not PEM"),
        ("", "empty"),
        (
            "-----BEGIN PRIVATE KEY-----\nAAAA\n-----END PRIVATE KEY-----",
            "not PKCS#8",
        ),
    ] {
        assert!(
            matches!(SigningKey::from_pem(input), Err(JoseError::InvalidKey(_))),
            "{why}"
        );
    }
    // A public key is not a signing key, and the other way round.
    let public = v["p256"]["public_pem"].as_str().unwrap();
    assert!(SigningKey::from_pem(public).is_err());
    assert!(VerifyingKey::from_pem(&fixture("apple-authkey.p8")).is_err());

    // A truncated document.
    let mut short = der(&fixture("apple-authkey.p8"));
    short.truncate(40);
    assert!(SigningKey::from_pkcs8_der(&short).is_err());

    // Errors never echo the input.
    let error =
        SigningKey::from_pem("-----BEGIN PRIVATE KEY-----\nU0VDUkVU\n-----END PRIVATE KEY-----")
            .unwrap_err()
            .to_string();
    assert!(!error.contains("U0VDUkVU"), "{error}");
}

#[test]
fn ed25519_keys_generate_and_export_as_pkcs8() {
    let key = SigningKey::generate_ed25519();
    assert_eq!(key.algorithm(), Algorithm::EdDSA);
    assert_ne!(
        key.public_jwk().unwrap().x,
        SigningKey::generate_ed25519().public_jwk().unwrap().x
    );
    // PKCS#8 v1 (RFC 8410): 48 bytes, which other tools read.
    let der = key.to_pkcs8_der().unwrap();
    assert_eq!(der.len(), 48);
    assert!(STANDARD.encode(&der).starts_with("MC4CAQAwBQYDK2VwBCIEI"));
    let pem = key.to_pkcs8_pem().unwrap();
    assert!(pem.starts_with("-----BEGIN PRIVATE KEY-----\n"));
    assert!(pem.ends_with("-----END PRIVATE KEY-----\n"));
    for again in [
        SigningKey::from_pkcs8_der(&der).unwrap(),
        SigningKey::from_pem(&pem).unwrap(),
    ] {
        assert_eq!(again.public_jwk(), key.public_jwk());
    }
    // The seed in the document is the seed.
    let seed: [u8; 32] = der[16..].try_into().unwrap();
    assert_eq!(
        SigningKey::ed25519_from_seed(&seed).public_jwk(),
        key.public_jwk()
    );
}

#[test]
fn pkcs8_v2_ed25519_documents_load_and_must_be_consistent() {
    // PKCS#8 v2 carries the public key too (what some tools write).
    let key = SigningKey::ed25519_from_seed(&[3; 32]);
    let public = URL_SAFE_NO_PAD
        .decode(key.public_jwk().unwrap().x.unwrap())
        .unwrap();
    let mut v2 = vec![0x30, 0x53, 0x02, 0x01, 0x01];
    v2.extend_from_slice(&[0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70]);
    v2.extend_from_slice(&[0x04, 0x22, 0x04, 0x20]);
    v2.extend_from_slice(&[3; 32]);
    v2.extend_from_slice(&[0xa1, 0x23, 0x03, 0x21, 0x00]);
    v2.extend_from_slice(&public);
    assert_eq!(
        SigningKey::from_pkcs8_der(&v2).unwrap().public_jwk(),
        key.public_jwk()
    );
    // A public key that does not belong to the seed.
    let last = v2.len() - 1;
    v2[last] ^= 1;
    assert!(SigningKey::from_pkcs8_der(&v2).is_err());
}

#[test]
fn es256_keys_generate_and_export_as_pkcs8() {
    let key = SigningKey::generate_es256();
    assert_eq!(key.algorithm(), Algorithm::ES256);
    let pem = key.to_pkcs8_pem().unwrap();
    let again = SigningKey::from_pem(&pem).unwrap();
    assert_eq!(again.public_jwk(), key.public_jwk());
    let token = again.sign_jwt(&json!({"sub": "1"})).unwrap();
    Jws::parse(&token)
        .unwrap()
        .verify(&key.verifying_key())
        .unwrap();
    // The Apple-style document exports in the plain layout and loads back.
    let apple = SigningKey::from_pem(&fixture("apple-authkey.p8")).unwrap();
    let exported = SigningKey::from_pem(&apple.to_pkcs8_pem().unwrap()).unwrap();
    assert_eq!(exported.public_jwk(), apple.public_jwk());
}

#[test]
fn rsa_keys_load_from_pkcs8_and_pkcs1() {
    let pem = fixture("rsa-2048.pem");
    let key = SigningKey::from_pem(&pem).unwrap();
    assert_eq!(key.to_pkcs8_pem().unwrap(), pem);
    let public = key.public_jwk().unwrap();
    assert_eq!(public.kty, "RSA");
    assert_eq!(public.e.as_deref(), Some("AQAB"));
    assert_eq!(
        URL_SAFE_NO_PAD.decode(public.n.unwrap()).unwrap().len(),
        256
    );

    // The PKCS#1 `RSA PRIVATE KEY` inside the PKCS#8 document.
    let pkcs8 = der(&pem);
    let pkcs1 = &pkcs8[26..];
    let pkcs1_pem = format!(
        "-----BEGIN RSA PRIVATE KEY-----\n{}\n-----END RSA PRIVATE KEY-----\n",
        STANDARD.encode(pkcs1)
    );
    assert_eq!(
        SigningKey::from_pem(&pkcs1_pem).unwrap().public_jwk(),
        key.public_jwk()
    );
    // Only the RSA family is allowed.
    assert!(key.clone().with_algorithm(Algorithm::ES256).is_err());
    assert!(
        SigningKey::generate_ed25519()
            .with_algorithm(Algorithm::ES256)
            .is_err()
    );
    assert!(
        SigningKey::generate_ed25519()
            .with_algorithm(Algorithm::EdDSA)
            .is_ok()
    );
}

#[test]
fn private_jwks_load_for_each_key_type() {
    let v = json_fixture("independent-vectors.json");
    let ed: Jwk = serde_json::from_value(v["ed25519"]["private_jwk"].clone()).unwrap();
    assert_eq!(
        SigningKey::from_jwk(&ed).unwrap().algorithm(),
        Algorithm::EdDSA
    );
    let ec: Jwk = serde_json::from_value(v["p256"]["private_jwk"].clone()).unwrap();
    assert_eq!(
        SigningKey::from_jwk(&ec).unwrap().algorithm(),
        Algorithm::ES256
    );

    // An oct key with an `alg` signs with it; without, HS256.
    let oct = Jwk {
        kty: "oct".into(),
        k: Some(URL_SAFE_NO_PAD.encode([1_u8; 64])),
        ..Jwk::default()
    };
    assert_eq!(
        SigningKey::from_jwk(&oct).unwrap().algorithm(),
        Algorithm::HS256
    );
    let hs512 = Jwk {
        alg: Some("HS512".into()),
        ..oct.clone()
    };
    assert_eq!(
        SigningKey::from_jwk(&hs512).unwrap().algorithm(),
        Algorithm::HS512
    );

    // The JWK's kid and alg carry over.
    let named = Jwk {
        kid: Some("ed".into()),
        alg: Some("EdDSA".into()),
        ..ed.clone()
    };
    assert_eq!(SigningKey::from_jwk(&named).unwrap().kid(), Some("ed"));
    assert_eq!(VerifyingKey::from_jwk(&named).unwrap().kid(), Some("ed"));

    // Public JWKs, wrong curves and inconsistent halves are refused.
    for bad in [
        ed.to_public(),
        Jwk {
            crv: Some("X25519".into()),
            ..ed.clone()
        },
        Jwk {
            x: ec.x.clone(),
            ..ed.clone()
        },
        Jwk {
            kty: "EC".into(),
            crv: Some("P-521".into()),
            ..ec.clone()
        },
        Jwk {
            d: Some("AAAA".into()),
            ..ec.clone()
        },
        Jwk {
            alg: Some("RS256".into()),
            ..ec.clone()
        },
        Jwk {
            kty: "dsa".into(),
            ..Jwk::default()
        },
    ] {
        assert!(SigningKey::from_jwk(&bad).is_err(), "{bad:?}");
    }
}

#[test]
fn public_keys_load_from_raw_components() {
    let ed = SigningKey::ed25519_from_seed(&[5; 32]);
    let x = URL_SAFE_NO_PAD
        .decode(ed.public_jwk().unwrap().x.unwrap())
        .unwrap();
    let token = ed.sign_jwt(&json!({})).unwrap();
    Jws::parse(&token)
        .unwrap()
        .verify(&VerifyingKey::ed25519(&x).unwrap())
        .unwrap();
    assert!(VerifyingKey::ed25519(&x[..31]).is_err());

    let es = SigningKey::generate_es256();
    let jwk = es.public_jwk().unwrap();
    let (x, y) = (
        URL_SAFE_NO_PAD.decode(jwk.x.unwrap()).unwrap(),
        URL_SAFE_NO_PAD.decode(jwk.y.unwrap()).unwrap(),
    );
    let key = VerifyingKey::ec(Algorithm::ES256, &x, &y).unwrap();
    let token = es.sign_jwt(&json!({})).unwrap();
    Jws::parse(&token).unwrap().verify(&key).unwrap();
    assert!(VerifyingKey::ec(Algorithm::ES384, &x, &y).is_err());
    assert!(VerifyingKey::ec(Algorithm::RS256, &x, &y).is_err());

    // RSA moduli outside 2048 to 8192 bits are refused.
    assert!(VerifyingKey::rsa(&[0xff; 128], &[1, 0, 1]).is_err());
    assert!(VerifyingKey::rsa(&[0xff; 256], &[1, 0, 1]).is_ok());
    // Leading zeros do not count.
    let mut padded = vec![0, 0];
    padded.extend_from_slice(&[0xff; 256]);
    assert_eq!(
        VerifyingKey::rsa(&padded, &[1, 0, 1])
            .unwrap()
            .to_jwk()
            .unwrap()
            .n,
        VerifyingKey::rsa(&[0xff; 256], &[1, 0, 1])
            .unwrap()
            .to_jwk()
            .unwrap()
            .n
    );
}

#[test]
fn ecdsa_signatures_in_der_form_verify() {
    // WebAuthn sends ES256 signatures DER-encoded.
    let key = SigningKey::generate_es256();
    let message = b"authenticator data || client data hash";
    let fixed = key.sign(message).unwrap();
    let der = fixed_to_der(&fixed);
    let public = key.verifying_key();
    public.verify_der(Algorithm::ES256, message, &der).unwrap();
    assert_eq!(
        public.verify_der(Algorithm::ES256, b"other", &der),
        Err(JoseError::BadSignature)
    );
    assert_eq!(
        public.verify(Algorithm::ES256, message, &der),
        Err(JoseError::BadSignature),
        "the JWS form is fixed-width"
    );
    // Ed25519 has one form.
    let ed = SigningKey::generate_ed25519();
    let signature = ed.sign(message).unwrap();
    ed.verifying_key()
        .verify_der(Algorithm::EdDSA, message, &signature)
        .unwrap();
}

/// `R || S` as a DER `ECDSA-Sig-Value`.
fn fixed_to_der(fixed: &[u8]) -> Vec<u8> {
    let int = |bytes: &[u8]| {
        let trimmed: Vec<u8> = bytes.iter().copied().skip_while(|b| *b == 0).collect();
        let mut value = if trimmed.first().is_some_and(|b| b & 0x80 != 0) {
            vec![0]
        } else {
            Vec::new()
        };
        value.extend(trimmed);
        let mut out = vec![0x02, u8::try_from(value.len()).unwrap()];
        out.extend(value);
        out
    };
    let (r, s) = fixed.split_at(fixed.len() / 2);
    let body = [int(r), int(s)].concat();
    let mut out = vec![0x30, u8::try_from(body.len()).unwrap()];
    out.extend(body);
    out
}

#[test]
fn public_jwks_and_thumbprint_kids() {
    let key = SigningKey::ed25519_from_seed(&[1; 32]);
    assert_eq!(key.kid(), None);
    let named = key.clone().with_thumbprint_kid();
    let thumbprint = key.public_jwk().unwrap().thumbprint().unwrap();
    assert_eq!(named.kid(), Some(thumbprint.as_str()));
    let jwk = named.public_jwk().unwrap();
    assert_eq!(
        serde_json::to_value(&jwk).unwrap(),
        json!({
            "kty": "OKP",
            "use": "sig",
            "alg": "EdDSA",
            "kid": thumbprint,
            "crv": "Ed25519",
            "x": jwk.x.clone().unwrap(),
        })
    );
    assert_eq!(named.verifying_key().to_jwk(), Some(jwk));
    // HMAC keys have no public half and keep their kid.
    let hs = SigningKey::hmac(Algorithm::HS256, [0_u8; 32]).unwrap();
    assert_eq!(hs.public_jwk(), None);
    assert_eq!(hs.verifying_key().to_jwk(), None);
    assert_eq!(hs.to_pkcs8_der(), None);
    assert_eq!(hs.with_thumbprint_kid().kid(), None);
}
