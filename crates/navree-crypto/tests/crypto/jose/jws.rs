//! Signing, parsing and verification rules of compact JWS.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use navree_crypto::jose::{Algorithm, Header, JoseError, Jwk, JwkSet, Jws, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::fixture;

/// A compact JWS with `header` and `claims`, and an empty signature.
fn unsigned(header: &Value, claims: &Value) -> String {
    format!(
        "{}.{}.",
        URL_SAFE_NO_PAD.encode(header.to_string()),
        URL_SAFE_NO_PAD.encode(claims.to_string())
    )
}

/// `token` with its payload replaced by `claims` (the signature kept).
fn tamper(token: &str, claims: &Value) -> String {
    let parts: Vec<&str> = token.split('.').collect();
    format!(
        "{}.{}.{}",
        parts[0],
        URL_SAFE_NO_PAD.encode(claims.to_string()),
        parts[2]
    )
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Claims {
    sub: String,
    exp: i64,
}

#[test]
fn algorithms_have_their_registered_names() {
    let names: Vec<&str> = Algorithm::ALL.iter().map(|alg| alg.as_str()).collect();
    assert_eq!(
        names,
        [
            "HS256", "HS384", "HS512", "RS256", "RS384", "RS512", "PS256", "PS384", "PS512",
            "ES256", "ES384", "EdDSA"
        ]
    );
    for alg in Algorithm::ALL {
        assert_eq!(alg.as_str().parse::<Algorithm>(), Ok(alg));
        assert_eq!(alg.to_string(), alg.as_str());
        assert_eq!(serde_json::to_value(alg).unwrap(), alg.as_str());
    }
    assert_eq!(Algorithm::ES256.key_type(), "EC");
    assert_eq!(Algorithm::PS384.key_type(), "RSA");
    assert_eq!(Algorithm::EdDSA.key_type(), "OKP");
    assert_eq!(Algorithm::HS512.key_type(), "oct");
    assert!(Algorithm::HS256.is_symmetric());
    assert!(!Algorithm::RS256.is_symmetric());
    for name in ["none", "None", "hs256", "ES512", "RSA-OAEP", ""] {
        assert_eq!(
            name.parse::<Algorithm>(),
            Err(JoseError::UnsupportedAlgorithm(name.to_owned())),
            "{name}"
        );
    }
}

#[test]
fn jwts_round_trip_for_every_signing_algorithm() {
    let claims = Claims {
        sub: "42".into(),
        exp: 4_102_444_800,
    };
    let keys = [
        SigningKey::hmac(Algorithm::HS256, [1_u8; 32]).unwrap(),
        SigningKey::hmac(Algorithm::HS384, [2_u8; 48]).unwrap(),
        SigningKey::hmac(Algorithm::HS512, [3_u8; 64]).unwrap(),
        SigningKey::generate_ed25519(),
        SigningKey::generate_es256(),
        SigningKey::from_pem(&fixture("rsa-2048.pem")).unwrap(),
    ];
    for key in keys {
        let key = key.with_kid("k1");
        let token = key.sign_jwt(&claims).unwrap();
        let jws = Jws::parse(&token).unwrap();
        assert_eq!(jws.header().alg, key.algorithm());
        assert_eq!(jws.header().kid.as_deref(), Some("k1"));
        assert_eq!(jws.header().typ.as_deref(), Some("JWT"));
        assert_eq!(jws.claims::<Claims>().unwrap(), claims);
        jws.verify(&key.verifying_key()).unwrap();
        // A changed payload no longer verifies.
        let forged = Jws::parse(&tamper(&token, &json!({"sub": "1", "exp": 0}))).unwrap();
        assert_eq!(
            forged.verify(&key.verifying_key()),
            Err(JoseError::BadSignature),
            "{}",
            key.algorithm()
        );
    }
}

#[test]
fn headers_serialize_in_a_fixed_order_with_extra_parameters() {
    let header = Header::new(Algorithm::ES256)
        .with_kid("ABC123")
        .with_typ("JWT")
        .with_param("x5t", "thumb");
    assert_eq!(
        serde_json::to_string(&header).unwrap(),
        r#"{"alg":"ES256","kid":"ABC123","typ":"JWT","x5t":"thumb"}"#
    );
    // Reserved names go to their fields.
    let header = Header::new(Algorithm::EdDSA).with_param("kid", "k9");
    assert_eq!(header.kid.as_deref(), Some("k9"));
    assert!(header.extra.is_empty());

    // `SigningKey::header` is `{alg, kid}`, and sign_json signs exactly it.
    let key = SigningKey::generate_es256().with_kid("KEYID");
    assert_eq!(
        serde_json::to_value(key.header()).unwrap(),
        json!({"alg": "ES256", "kid": "KEYID"})
    );
    let token = key.sign_json(&key.header(), &json!({"iss": "TEAM"})).unwrap();
    let encoded_header = token.split('.').next().unwrap();
    assert_eq!(
        URL_SAFE_NO_PAD.decode(encoded_header).unwrap(),
        br#"{"alg":"ES256","kid":"KEYID"}"#
    );
    let jws = Jws::parse(&token).unwrap();
    assert_eq!(jws.header().typ, None);
    jws.verify(&key.verifying_key()).unwrap();
}

#[test]
fn a_header_must_name_the_keys_algorithm() {
    let key = SigningKey::generate_ed25519();
    assert_eq!(
        key.sign_jws(&Header::new(Algorithm::ES256), b"x"),
        Err(JoseError::NoKey)
    );
}

#[test]
fn unknown_header_parameters_are_kept() {
    let key = SigningKey::hmac(Algorithm::HS256, [9_u8; 32]).unwrap();
    let header = Header::new(Algorithm::HS256).with_param("x-custom", json!({"a": 1}));
    let token = key.sign_jws(&header, b"payload").unwrap();
    let jws = Jws::parse(&token).unwrap();
    assert_eq!(jws.header().extra["x-custom"], json!({"a": 1}));
    assert_eq!(jws.header(), &header);
    assert_eq!(jws.payload(), b"payload");
}

#[test]
fn none_and_unknown_algorithms_are_refused_when_parsing() {
    for alg in ["none", "HS1", "RSA-OAEP"] {
        let token = unsigned(&json!({"alg": alg}), &json!({"sub": "1"}));
        assert_eq!(
            Jws::parse(&token).unwrap_err(),
            JoseError::UnsupportedAlgorithm(alg.to_owned())
        );
    }
}

#[test]
fn critical_extensions_are_refused() {
    let token = unsigned(
        &json!({"alg": "HS256", "b64": false, "crit": ["b64"]}),
        &json!({}),
    );
    assert!(matches!(
        Jws::parse(&token),
        Err(JoseError::Malformed(_))
    ));
}

#[test]
fn malformed_tokens_are_refused() {
    let header = URL_SAFE_NO_PAD.encode(r#"{"alg":"HS256"}"#);
    let payload = URL_SAFE_NO_PAD.encode(r#"{"sub":"1"}"#);
    for token in [
        String::new(),
        "a.b".to_owned(),
        "a.b.c.d".to_owned(),
        format!("!!.{payload}."),
        format!("{header}.!!."),
        format!("{header}.{payload}.!!"),
        // Not JSON, not an object, no `alg`, a duplicate `alg`.
        format!("{}.{payload}.", URL_SAFE_NO_PAD.encode("nope")),
        format!("{}.{payload}.", URL_SAFE_NO_PAD.encode("[1]")),
        format!("{}.{payload}.", URL_SAFE_NO_PAD.encode(r#"{"kid":"k"}"#)),
        format!(
            "{}.{payload}.",
            URL_SAFE_NO_PAD.encode(r#"{"alg":"HS256","alg":"none"}"#)
        ),
    ] {
        assert!(
            matches!(Jws::parse(&token), Err(JoseError::Malformed(_))),
            "{token}"
        );
    }
}

#[test]
fn claims_must_be_json_of_the_requested_type() {
    let key = SigningKey::hmac(Algorithm::HS256, [5_u8; 32]).unwrap();
    let token = key.sign_jws(&key.header(), b"[1, 2]").unwrap();
    let jws = Jws::parse(&token).unwrap();
    assert_eq!(jws.claims::<Vec<u8>>().unwrap(), [1, 2]);
    assert_eq!(
        jws.claims::<serde_json::Map<String, Value>>(),
        Err(JoseError::Payload)
    );
    assert_eq!(jws.claims::<Claims>(), Err(JoseError::Payload));
}

#[test]
fn a_key_verifies_only_its_own_algorithms() {
    let ed = SigningKey::generate_ed25519();
    let es = SigningKey::generate_es256();
    let hs = SigningKey::hmac(Algorithm::HS256, [4_u8; 32]).unwrap();
    let token = ed.sign_jwt(&json!({})).unwrap();
    let jws = Jws::parse(&token).unwrap();
    assert_eq!(jws.verify(&es.verifying_key()), Err(JoseError::NoKey));
    assert_eq!(jws.verify(&hs.verifying_key()), Err(JoseError::NoKey));

    assert!(ed.verifying_key().accepts(Algorithm::EdDSA));
    assert!(!ed.verifying_key().accepts(Algorithm::ES256));
    assert!(es.verifying_key().accepts(Algorithm::ES256));
    assert!(!es.verifying_key().accepts(Algorithm::ES384));
    assert!(hs.verifying_key().accepts(Algorithm::HS256));
    assert!(!hs.verifying_key().accepts(Algorithm::HS512));

    // An RSA key accepts the RSA family unless its algorithm is pinned.
    let rsa = SigningKey::from_pem(&fixture("rsa-2048.pem")).unwrap().verifying_key();
    assert!(rsa.accepts(Algorithm::RS256) && rsa.accepts(Algorithm::PS512));
    assert!(!rsa.accepts(Algorithm::ES256));
    let pinned = rsa.with_algorithm(Algorithm::PS256).unwrap();
    assert!(pinned.accepts(Algorithm::PS256) && !pinned.accepts(Algorithm::RS256));
    assert!(ed.verifying_key().with_algorithm(Algorithm::RS256).is_err());
}

#[test]
fn an_hmac_token_never_verifies_with_public_key_material() {
    // The classic confusion: an attacker signs HS256 with the bytes of a
    // published RSA key. An RSA key never verifies HMAC.
    let rsa = SigningKey::from_pem(&fixture("rsa-2048.pem")).unwrap();
    let public = rsa.public_jwk().unwrap();
    let n = URL_SAFE_NO_PAD.decode(public.n.as_deref().unwrap()).unwrap();
    let forged = SigningKey::hmac(Algorithm::HS256, &n)
        .unwrap()
        .sign_jwt(&json!({"sub": "admin"}))
        .unwrap();
    let jws = Jws::parse(&forged).unwrap();
    assert_eq!(jws.verify(&rsa.verifying_key()), Err(JoseError::NoKey));
    assert_eq!(
        jws.verify_with(&JwkSet::new([public])),
        Err(JoseError::UnsupportedAlgorithm("HS256".into()))
    );
}

#[test]
fn hmac_secrets_shorter_than_the_hash_are_refused() {
    for (alg, len) in [
        (Algorithm::HS256, 32),
        (Algorithm::HS384, 48),
        (Algorithm::HS512, 64),
    ] {
        assert!(SigningKey::hmac(alg, vec![0_u8; len]).is_ok());
        assert_eq!(
            SigningKey::hmac(alg, vec![0_u8; len - 1]).unwrap_err(),
            JoseError::InvalidKey("an HMAC secret must be at least as long as the hash output")
        );
        assert!(VerifyingKey::hmac(alg, vec![0_u8; len - 1]).is_err());
    }
    assert!(SigningKey::hmac(Algorithm::RS256, [0_u8; 64]).is_err());
}

#[test]
fn key_sets_pick_the_key_by_kid_then_by_type() {
    let (k1, k2) = (
        SigningKey::generate_ed25519().with_kid("k1"),
        SigningKey::generate_ed25519().with_kid("k2"),
    );
    let es = SigningKey::generate_es256().with_kid("e1");
    let set: JwkSet = [&k1, &k2, &es]
        .into_iter()
        .filter_map(SigningKey::public_jwk)
        .collect();
    for key in [&k1, &k2, &es] {
        let jws = Jws::parse(&key.sign_jwt(&json!({})).unwrap()).unwrap();
        assert_eq!(jws.verify_with(&set).unwrap().kid.as_deref(), key.kid());
    }

    // An unknown kid: no key, even if another key would verify.
    let stranger = SigningKey::generate_ed25519().with_kid("k3");
    let jws = Jws::parse(&stranger.sign_jwt(&json!({})).unwrap()).unwrap();
    assert_eq!(jws.verify_with(&set), Err(JoseError::NoKey));

    // Without a kid, every key of the type is tried.
    let unnamed = SigningKey::ed25519_from_seed(&[8; 32]);
    let token = unnamed.sign_jwt(&json!({})).unwrap();
    let jws = Jws::parse(&token).unwrap();
    assert_eq!(jws.header().kid, None);
    assert_eq!(jws.verify_with(&set), Err(JoseError::BadSignature));
    let mut with_it = set.clone();
    with_it.keys.push(unnamed.public_jwk().unwrap());
    assert!(jws.verify_with(&with_it).is_ok());
    assert_eq!(jws.verify_with(&JwkSet::default()), Err(JoseError::NoKey));
}

#[test]
fn key_sets_skip_keys_for_other_uses_and_algorithms() {
    let key = SigningKey::generate_es256().with_kid("k");
    let jws = Jws::parse(&key.sign_jwt(&json!({})).unwrap()).unwrap();
    let public = key.public_jwk().unwrap();
    assert_eq!(public.use_.as_deref(), Some("sig"));
    assert_eq!(public.alg.as_deref(), Some("ES256"));

    let variants = [
        Jwk { use_: Some("enc".into()), ..public.clone() },
        Jwk { alg: Some("ES384".into()), ..public.clone() },
        Jwk { key_ops: Some(vec!["encrypt".into()]), ..public.clone() },
        Jwk { kty: "RSA".into(), ..public.clone() },
    ];
    for jwk in variants {
        assert_eq!(jws.verify_with(&JwkSet::new([jwk.clone()])), Err(JoseError::NoKey), "{jwk:?}");
    }
    let usable = [
        Jwk { use_: None, alg: None, ..public.clone() },
        Jwk { key_ops: Some(vec!["verify".into()]), ..public.clone() },
    ];
    for jwk in usable {
        assert!(jws.verify_with(&JwkSet::new([jwk])).is_ok());
    }
    // A key of the right type on the wrong curve.
    let p384 = Jwk { crv: Some("P-384".into()), alg: None, ..public };
    assert!(matches!(
        jws.verify_with(&JwkSet::new([p384])),
        Err(JoseError::InvalidKey(_))
    ));
}

#[test]
fn key_sets_never_verify_symmetric_algorithms() {
    let key = SigningKey::hmac(Algorithm::HS256, [6_u8; 32]).unwrap();
    let jws = Jws::parse(&key.sign_jwt(&json!({})).unwrap()).unwrap();
    let oct = Jwk {
        kty: "oct".into(),
        k: Some(URL_SAFE_NO_PAD.encode([6_u8; 32])),
        ..Jwk::default()
    };
    assert_eq!(
        jws.verify_with(&JwkSet::new([oct.clone()])),
        Err(JoseError::UnsupportedAlgorithm("HS256".into()))
    );
    // A shared secret verifies only when handed over explicitly.
    jws.verify(&VerifyingKey::from_jwk(&oct).unwrap()).unwrap();
}

#[test]
fn errors_never_contain_key_material() {
    let secret = "super-secret-value-that-must-not-leak-0123456789";
    let key = SigningKey::hmac(Algorithm::HS256, secret).unwrap().with_kid("k");
    let debug = format!("{key:?} {:?}", key.verifying_key());
    assert!(!debug.contains(secret), "{debug}");
    assert!(debug.contains("HS256"));

    let ed = SigningKey::ed25519_from_seed(&[42; 32]);
    let pem = ed.to_pkcs8_pem().unwrap();
    let body = pem.lines().nth(1).unwrap();
    assert!(!format!("{ed:?}").contains(body));

    let private = Jwk {
        kty: "OKP".into(),
        crv: Some("Ed25519".into()),
        d: Some("PRIVATE-D".into()),
        x: Some("PUBLIC-X".into()),
        ..Jwk::default()
    };
    let debug = format!("{private:?}");
    assert!(!debug.contains("PRIVATE-D") && debug.contains("PUBLIC-X"), "{debug}");

    let long = "A".repeat(500);
    let message = JoseError::UnsupportedAlgorithm(long.clone()).to_string();
    assert!(message.len() < 100, "{message}");
    let token = unsigned(&json!({"alg": long}), &json!({}));
    assert!(Jws::parse(&token).unwrap_err().to_string().len() < 100);
}
