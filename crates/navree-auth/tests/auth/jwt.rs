//! The tokens of navree-auth on `navree_crypto::jose`: tokens minted by the
//! previous implementation (jsonwebtoken 11, committed as fixtures) still
//! verify, new tokens carry the same algorithm and claims, and a hostile
//! token of every kind we know is refused.

use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{TimeZone, Utc};
use navree_auth::{
    AuthError, JwtOptions, JwtService, LEEWAY_SECS, MAX_TOKEN_LEN, SessionClaims, SignInMethod,
    TokenType,
};
use navree_contracts::FixedClock;
use navree_crypto::jose::{Algorithm, Jws, SigningKey};
use ring::hmac;
use serde_json::{Map, Value, json};
use uuid::Uuid;

const SECRET: &[u8] = b"0123456789abcdef0123456789abcdef";

fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

fn now() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 25, 12, 0, 0).unwrap()
}

fn service_with(options: &JwtOptions) -> (JwtService, Arc<FixedClock>) {
    let clock = Arc::new(FixedClock::new(now()));
    (
        JwtService::new(SECRET, options).with_clock(clock.clone()),
        clock,
    )
}

fn service() -> (JwtService, Arc<FixedClock>) {
    service_with(&JwtOptions::default())
}

fn audience_options() -> JwtOptions {
    JwtOptions {
        issuer: "crm".to_owned(),
        audience: Some("crm-web".to_owned()),
        ..JwtOptions::default()
    }
}

fn hs256() -> SigningKey {
    SigningKey::hmac(Algorithm::HS256, SECRET).unwrap()
}

/// The claims of a valid access token for the default service.
fn claims() -> Map<String, Value> {
    let iat = now().timestamp();
    let Value::Object(map) = json!({
        "sub": Uuid::from_u128(0x0192_0000_0000_7000_8000_0000_0000_0042),
        "iat": iat,
        "exp": iat + 900,
        "iss": "navree",
        "jti": "0192abcdef",
        "typ": "access",
    }) else {
        unreachable!()
    };
    map
}

fn header() -> Value {
    json!({ "alg": "HS256", "typ": "JWT" })
}

/// `header.payload` signed with `signature` (anything: the caller chooses
/// whether it is right).
fn assemble(header: &[u8], payload: &[u8], sign: impl FnOnce(&str) -> Vec<u8>) -> String {
    let input = format!("{}.{}", b64(header), b64(payload));
    let signature = sign(&input);
    format!("{input}.{}", b64(&signature))
}

/// A token with this header and payload, signed with HMAC-SHA-256 under the
/// service's secret: everything that is wrong with it is in what it says.
fn forge(header: &Value, payload: &Value) -> String {
    forge_raw(
        header.to_string().as_bytes(),
        payload.to_string().as_bytes(),
    )
}

fn forge_raw(header: &[u8], payload: &[u8]) -> String {
    assemble(header, payload, |input| {
        hs256().sign(input.as_bytes()).unwrap()
    })
}

/// The HMAC of `input` under the service's secret with another hash.
fn hmac_with(algorithm: hmac::Algorithm, input: &str) -> Vec<u8> {
    hmac::sign(&hmac::Key::new(algorithm, SECRET), input.as_bytes())
        .as_ref()
        .to_vec()
}

fn assert_invalid(jwt: &JwtService, token: &str) {
    let shown: String = token.chars().take(120).collect();
    assert!(
        matches!(
            jwt.verify(token, TokenType::Access),
            Err(AuthError::InvalidToken)
        ),
        "accepted or wrong error for {shown}"
    );
}

fn assert_valid(jwt: &JwtService, token: &str) {
    assert!(
        jwt.verify(token, TokenType::Access).is_ok(),
        "refused {token}"
    );
}

// ---- tokens from the previous implementation ----------------------------------

fn legacy() -> Value {
    let path = format!(
        "{}/tests/fixtures/jwt/legacy-tokens.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn legacy_service(fixture: &Value, which: &str) -> (JwtService, Arc<FixedClock>) {
    let clock = Arc::new(FixedClock::new(
        Utc.timestamp_opt(fixture["issued_at"].as_i64().unwrap(), 0)
            .unwrap(),
    ));
    let secret = fixture["secret"].as_str().unwrap().as_bytes();
    let options = if which == "crm" {
        audience_options()
    } else {
        JwtOptions::default()
    };
    (
        JwtService::new(secret, &options).with_clock(clock.clone()),
        clock,
    )
}

fn uuid(fixture: &Value, name: &str) -> Uuid {
    fixture[name].as_str().unwrap().parse().unwrap()
}

#[test]
fn tokens_minted_by_the_previous_implementation_still_verify() {
    let fixture = legacy();
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 13);
    let (user, admin, session) = (
        uuid(&fixture, "user"),
        uuid(&fixture, "impersonator"),
        uuid(&fixture, "session"),
    );
    let auth_time = fixture["auth_time"].as_i64().unwrap();
    let issued_at = fixture["issued_at"].as_i64().unwrap();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let token = case["token"].as_str().unwrap();
        let (jwt, clock) = legacy_service(&fixture, case["service"].as_str().unwrap());
        let expected_type = match name {
            "two_factor_setup" => TokenType::TwoFactorSetup,
            "platform" => TokenType::Platform,
            "platform_challenge" => TokenType::PlatformChallenge,
            n if n.starts_with("challenge_") => TokenType::TwoFactorChallenge,
            _ => TokenType::Access,
        };

        // Verifies on the day, and until the leeway after expiry.
        let claims = jwt
            .verify(token, expected_type)
            .unwrap_or_else(|error| panic!("{name}: {error:?}"));
        assert_eq!(claims.sub, user, "{name}");
        assert_eq!(claims.typ, expected_type, "{name}");
        assert_eq!(claims.iat, issued_at, "{name}");
        assert_eq!(claims.exp, issued_at + 900, "{name}");
        assert_eq!(
            claims.iss,
            if name == "access_audience" {
                "crm"
            } else {
                "navree"
            }
        );
        assert_eq!(
            claims.aud.as_deref(),
            (name == "access_audience").then_some("crm-web")
        );
        assert_eq!(claims.jti.len(), 32, "{name}");
        match name {
            "access_session" => {
                assert_eq!(claims.sid, Some(session));
                assert_eq!(claims.auth_time, Some(auth_time));
                assert_eq!(claims.act, None);
            }
            "access_impersonation" => {
                assert_eq!(claims.sid, Some(session));
                assert_eq!(claims.act.map(|actor| actor.sub), Some(admin));
            }
            "challenge_password" | "platform_challenge" => {
                assert_eq!(claims.sign_in, Some(SignInMethod::Password));
            }
            "challenge_magic_link" => assert_eq!(claims.sign_in, Some(SignInMethod::MagicLink)),
            "challenge_passkey" => assert_eq!(claims.sign_in, Some(SignInMethod::Passkey)),
            "challenge_social" => assert_eq!(
                claims.sign_in,
                Some(SignInMethod::Social {
                    provider: "github".to_owned()
                })
            ),
            "challenge_sso" => assert_eq!(
                claims.sign_in,
                Some(SignInMethod::Sso {
                    connection: "agra-okta".to_owned()
                })
            ),
            "challenge_exchange" => assert_eq!(
                claims.sign_in,
                Some(SignInMethod::TokenExchange {
                    issuer: "partner".to_owned()
                })
            ),
            _ => assert_eq!(claims.sign_in, None, "{name}"),
        }

        // Only its own type, and only its own issuer's secret.
        let other_type = if expected_type == TokenType::Access {
            TokenType::Platform
        } else {
            TokenType::Access
        };
        assert!(matches!(
            jwt.verify(token, other_type),
            Err(AuthError::InvalidToken)
        ));
        let stranger = JwtService::new(b"another-secret-another-secret-xx", &JwtOptions::default());
        assert!(stranger.verify_any(token, &[expected_type]).is_err());

        // The expiry is the same: valid through exp + leeway, then expired.
        clock.set(Utc.timestamp_opt(issued_at + 900 + LEEWAY_SECS, 0).unwrap());
        assert!(jwt.verify(token, expected_type).is_ok(), "{name}");
        clock.set(
            Utc.timestamp_opt(issued_at + 900 + LEEWAY_SECS + 1, 0)
                .unwrap(),
        );
        assert!(matches!(
            jwt.verify(token, expected_type),
            Err(AuthError::TokenExpired)
        ));
    }
}

/// New tokens say what the old ones said: the same header, the same claims
/// (only `jti` is random), and the same HMAC over the same bytes.
#[test]
fn new_tokens_carry_the_algorithm_and_claims_of_the_old_ones() {
    let fixture = legacy();
    let (user, admin, session) = (
        uuid(&fixture, "user"),
        uuid(&fixture, "impersonator"),
        uuid(&fixture, "session"),
    );
    let auth_time = fixture["auth_time"].as_i64().unwrap();
    let secret = fixture["secret"].as_str().unwrap().as_bytes();
    let ttl = Duration::from_secs(900);
    let mint = |name: &str, service: &str| -> String {
        let (jwt, _) = legacy_service(&fixture, service);
        let method =
            |m: SignInMethod| jwt.sign_challenge(user, TokenType::TwoFactorChallenge, ttl, &m);
        match name {
            "access" | "access_audience" => jwt.sign(user, TokenType::Access, ttl),
            "access_session" => jwt.sign_session(
                user,
                TokenType::Access,
                ttl,
                SessionClaims {
                    session_id: Some(session),
                    auth_time: Some(auth_time),
                    impersonator: None,
                },
            ),
            "access_impersonation" => jwt.sign_session(
                user,
                TokenType::Access,
                ttl,
                SessionClaims {
                    session_id: Some(session),
                    auth_time: None,
                    impersonator: Some(admin),
                },
            ),
            "two_factor_setup" => jwt.sign(user, TokenType::TwoFactorSetup, ttl),
            "platform" => jwt.sign(user, TokenType::Platform, ttl),
            "platform_challenge" => jwt.sign_challenge(
                user,
                TokenType::PlatformChallenge,
                ttl,
                &SignInMethod::Password,
            ),
            "challenge_password" => method(SignInMethod::Password),
            "challenge_magic_link" => method(SignInMethod::MagicLink),
            "challenge_passkey" => method(SignInMethod::Passkey),
            "challenge_social" => method(SignInMethod::Social {
                provider: "github".to_owned(),
            }),
            "challenge_sso" => method(SignInMethod::Sso {
                connection: "agra-okta".to_owned(),
            }),
            "challenge_exchange" => method(SignInMethod::TokenExchange {
                issuer: "partner".to_owned(),
            }),
            other => panic!("{other}"),
        }
        .unwrap()
    };
    let parts = |token: &str| -> (Value, Value) {
        let jws = Jws::parse(token).unwrap();
        let header = serde_json::to_value(jws.header()).unwrap();
        let mut payload: Value = jws.claims().unwrap();
        payload.as_object_mut().unwrap().remove("jti");
        (header, payload)
    };
    let key = SigningKey::hmac(Algorithm::HS256, secret).unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let old = case["token"].as_str().unwrap();
        let new = mint(name, case["service"].as_str().unwrap());
        assert_eq!(parts(old), parts(&new), "{name}");

        // jsonwebtoken's signature is the HMAC-SHA-256 of "header.payload".
        let old_jws = Jws::parse(old).unwrap();
        assert_eq!(
            key.sign(old_jws.signing_input().as_bytes()).unwrap(),
            old_jws.signature(),
            "{name}"
        );
    }
}

// ---- hostile tokens ------------------------------------------------------------

#[test]
fn a_token_the_service_signs_verifies() {
    let (jwt, _) = service();
    let user = Uuid::now_v7();
    let token = jwt.sign_access(user).unwrap();
    assert_eq!(jwt.verify(&token, TokenType::Access).unwrap().sub, user);
    // The forging helper makes tokens the service accepts, so a refusal
    // below is about what the token says.
    assert_valid(&jwt, &forge(&header(), &Value::Object(claims())));
}

#[test]
fn alg_none_is_never_accepted() {
    let (jwt, _) = service();
    let payload = Value::Object(claims());
    for alg in ["none", "None", "NONE", "nOnE"] {
        let header = json!({ "alg": alg, "typ": "JWT" });
        let input = format!(
            "{}.{}",
            b64(header.to_string().as_bytes()),
            b64(payload.to_string().as_bytes())
        );
        // Without a signature part, with an empty one, with a real HMAC and
        // with junk.
        assert_invalid(&jwt, &input);
        assert_invalid(&jwt, &format!("{input}."));
        assert_invalid(
            &jwt,
            &format!("{input}.{}", b64(&hmac_with(hmac::HMAC_SHA256, &input))),
        );
        assert_invalid(&jwt, &format!("{input}.{}", b64(&[0; 32])));
    }
    // No `alg` at all, or one that is not a string.
    for header in [
        json!({ "typ": "JWT" }),
        json!({ "alg": null }),
        json!({ "alg": 256 }),
        json!({ "alg": ["HS256"] }),
        json!({ "alg": "" }),
    ] {
        assert_invalid(&jwt, &forge(&header, &payload));
    }
}

#[test]
fn algorithm_confusion_is_refused() {
    let (jwt, _) = service();
    let payload = Value::Object(claims());
    let payload_bytes = payload.to_string();

    // Another HMAC hash: the signature is a right HMAC of its kind under
    // the secret, but the service takes HS256 only.
    for (alg, hash) in [("HS384", hmac::HMAC_SHA384), ("HS512", hmac::HMAC_SHA512)] {
        let header = json!({ "alg": alg, "typ": "JWT" });
        let token = assemble(
            header.to_string().as_bytes(),
            payload_bytes.as_bytes(),
            |input| hmac_with(hash, input),
        );
        assert_invalid(&jwt, &token);
    }

    // Asymmetric algorithms named in the header with an HMAC of the secret
    // as the "signature" (the classic confusion), and with junk of the
    // size of a real signature.
    for alg in [
        "RS256", "RS384", "RS512", "PS256", "PS384", "PS512", "ES256", "ES384", "ES512", "EdDSA",
        "Ed25519",
    ] {
        let header = json!({ "alg": alg, "typ": "JWT" });
        let token = assemble(
            header.to_string().as_bytes(),
            payload_bytes.as_bytes(),
            |input| hmac_with(hmac::HMAC_SHA256, input),
        );
        assert_invalid(&jwt, &token);
        let junk = assemble(
            header.to_string().as_bytes(),
            payload_bytes.as_bytes(),
            |_| vec![7; 256],
        );
        assert_invalid(&jwt, &junk);
    }

    // Genuine EdDSA and ECDSA tokens, signed by keys of someone else's.
    for key in [SigningKey::generate_ed25519(), SigningKey::generate_es256()] {
        let token = key.sign_jwt(&payload).unwrap();
        assert_invalid(&jwt, &token);
    }

    // An RSA-looking public key used as the HMAC secret: here, the
    // service's own secret is what must match, and any other secret fails.
    for secret in [
        b"-----BEGIN PUBLIC KEY-----MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8A".as_slice(),
        &[0_u8; 32],
        &[0xFF; 64],
    ] {
        let key = SigningKey::hmac(Algorithm::HS256, secret).unwrap();
        assert_invalid(&jwt, &key.sign_jwt(&payload).unwrap());
    }

    // The right algorithm in a different case is not the algorithm.
    for alg in ["hs256", "Hs256", "HS256 ", " HS256"] {
        assert_invalid(&jwt, &forge(&json!({ "alg": alg }), &payload));
    }
}

#[test]
fn a_token_that_names_a_key_id_is_refused() {
    let (jwt, _) = service();
    let payload = Value::Object(claims());
    // The service has one key and its tokens carry no `kid`: a token that
    // names one, right signature or not, was made by someone else.
    for kid in [
        json!("other"),
        json!(""),
        json!("1"),
        json!("navree"),
        json!(7),
        json!(["a"]),
    ] {
        let header = json!({ "alg": "HS256", "typ": "JWT", "kid": kid });
        assert_invalid(&jwt, &forge(&header, &payload));
    }
    let key = SigningKey::hmac(Algorithm::HS256, SECRET)
        .unwrap()
        .with_kid("rotated-2025");
    assert_invalid(&jwt, &key.sign_jwt(&payload).unwrap());
    // Without the `kid`, the same signing key's token is fine.
    assert_valid(&jwt, &forge(&header(), &payload));
}

#[test]
fn critical_and_key_pointing_headers_are_not_followed() {
    let (jwt, _) = service();
    let payload = Value::Object(claims());
    for extra in [
        json!({ "crit": ["exp"], "exp": 1 }),
        json!({ "crit": [] }),
        json!({ "crit": "exp" }),
    ] {
        let mut header = header();
        header
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        assert_invalid(&jwt, &forge(&header, &payload));
    }
    // `jku`, `jwk`, `x5u`, `x5c` name keys to trust. They are not read: the
    // token verifies (or not) under the service's own secret alone.
    for (name, value) in [
        ("jku", json!("https://evil.example/jwks.json")),
        ("jwk", json!({ "kty": "oct", "k": b64(SECRET) })),
        ("x5u", json!("https://evil.example/cert.pem")),
        ("x5c", json!(["MIIB"])),
    ] {
        let mut header = header();
        header[name] = value.clone();
        assert_valid(&jwt, &forge(&header, &payload));
        // And a token signed with a key that header names is not accepted.
        let attacker = SigningKey::hmac(Algorithm::HS256, [9_u8; 32]).unwrap();
        let token = assemble(
            header.to_string().as_bytes(),
            payload.to_string().as_bytes(),
            |input| attacker.sign(input.as_bytes()).unwrap(),
        );
        assert_invalid(&jwt, &token);
    }
}

#[test]
fn tampering_with_any_part_is_refused() {
    let (jwt, _) = service();
    let user = Uuid::from_u128(1);
    let token = jwt.sign_access(user).unwrap();
    let [header_part, payload_part, signature_part]: [&str; 3] =
        token.split('.').collect::<Vec<_>>().try_into().unwrap();

    // Header: another algorithm or a key id, under the old signature.
    for edited in [
        json!({ "alg": "HS384", "typ": "JWT" }),
        json!({ "alg": "none", "typ": "JWT" }),
        json!({ "alg": "HS256", "typ": "JWT", "kid": "x" }),
        json!({ "alg": "HS256", "typ": "JWT", "extra": 1 }),
        json!({ "alg": "HS256" }),
    ] {
        let edited = b64(edited.to_string().as_bytes());
        assert_invalid(&jwt, &format!("{edited}.{payload_part}.{signature_part}"));
    }

    // Payload: any claim changed under the old signature.
    let claims: Value = Jws::parse(&token).unwrap().claims().unwrap();
    for (name, value) in [
        ("sub", json!(Uuid::from_u128(2))),
        ("exp", json!(claims["exp"].as_i64().unwrap() + 86_400)),
        ("iat", json!(0)),
        ("typ", json!("platform")),
        ("jti", json!("another")),
        ("iss", json!("other")),
        ("sid", json!(Uuid::from_u128(3))),
        ("act", json!({ "sub": Uuid::from_u128(4) })),
    ] {
        let mut edited = claims.clone();
        edited[name] = value;
        let edited = b64(edited.to_string().as_bytes());
        assert_invalid(&jwt, &format!("{header_part}.{edited}.{signature_part}"));
    }

    // Signature: every one-character change, truncation, extension, empty
    // and the signature of another token.
    let other = jwt.sign_access(Uuid::from_u128(5)).unwrap();
    let other_signature = other.rsplit('.').next().unwrap();
    for (i, original) in signature_part.char_indices() {
        for substitute in ['A', 'B', '_', '-', '9'] {
            if substitute == original {
                continue;
            }
            let mut edited = signature_part.to_owned();
            edited.replace_range(i..=i, &substitute.to_string());
            assert_invalid(&jwt, &format!("{header_part}.{payload_part}.{edited}"));
        }
    }
    for edited in [
        &signature_part[..signature_part.len() - 1],
        &signature_part[..signature_part.len() / 2],
        "",
        other_signature,
        &format!("{signature_part}A"),
        &format!("{signature_part}{signature_part}"),
    ] {
        assert_invalid(&jwt, &format!("{header_part}.{payload_part}.{edited}"));
    }

    // Every single-character change anywhere in the token, and every
    // proper prefix of it.
    for (i, original) in token.char_indices() {
        if original == '.' {
            continue;
        }
        let substitute = if original == 'A' { 'B' } else { 'A' };
        let mut edited = token.clone();
        edited.replace_range(i..=i, &substitute.to_string());
        assert_invalid(&jwt, &edited);
    }
    for end in 0..token.len() {
        assert_invalid(&jwt, &token[..end]);
    }
    // Parts in the wrong order or repeated.
    assert_invalid(
        &jwt,
        &format!("{payload_part}.{header_part}.{signature_part}"),
    );
    assert_invalid(
        &jwt,
        &format!("{header_part}.{payload_part}.{signature_part}.{signature_part}"),
    );
    assert_invalid(
        &jwt,
        &format!("{header_part}.{header_part}.{signature_part}"),
    );
    assert_valid(&jwt, &token);
}

#[test]
fn a_token_signed_under_another_secret_is_refused() {
    let (jwt, _) = service();
    for secret in [
        b"another-secret-another-secret-xx".as_slice(),
        // One character off, a prefix of the real secret padded, the
        // real secret with something added.
        b"0123456789abcdef0123456789abcdeF",
        &SECRET[..31],
        b"0123456789abcdef0123456789abcdef0",
        &[0_u8; 32],
    ] {
        let other = JwtService::new(secret, &JwtOptions::default());
        let token = other.sign_access(Uuid::now_v7()).unwrap();
        assert!(other.verify(&token, TokenType::Access).is_ok());
        assert_invalid(&jwt, &token);
    }
    // A short secret is padded with zeros, like HMAC would: it works, and
    // differs from the secret it is a prefix of.
    let short = JwtService::new(b"short-secret", &JwtOptions::default());
    let token = short.sign_access(Uuid::now_v7()).unwrap();
    assert!(short.verify(&token, TokenType::Access).is_ok());
    assert_invalid(&jwt, &token);
}

#[test]
fn expired_tokens_are_refused_after_the_leeway() {
    let (jwt, clock) = service();
    let token = jwt
        .sign(Uuid::now_v7(), TokenType::Access, Duration::from_secs(900))
        .unwrap();
    clock.advance(Duration::from_secs(900 + LEEWAY_SECS as u64));
    assert_valid(&jwt, &token);
    clock.advance(Duration::from_secs(1));
    assert!(matches!(
        jwt.verify(&token, TokenType::Access),
        Err(AuthError::TokenExpired)
    ));
    clock.advance(Duration::from_secs(10 * 365 * 86_400));
    assert!(matches!(
        jwt.verify(&token, TokenType::Access),
        Err(AuthError::TokenExpired)
    ));

    // `exp` in the past in a forged-with-the-right-key token.
    let (jwt, _) = service();
    let mut payload = claims();
    for exp in [now().timestamp() - LEEWAY_SECS - 1, 0, -1, i64::MIN] {
        payload.insert("exp".into(), json!(exp));
        let result = jwt.verify(
            &forge(&header(), &Value::Object(payload.clone())),
            TokenType::Access,
        );
        assert!(
            matches!(
                result,
                Err(AuthError::TokenExpired) | Err(AuthError::InvalidToken)
            ),
            "{exp}: {result:?}"
        );
    }
    payload.insert("exp".into(), json!(now().timestamp() - LEEWAY_SECS));
    assert_valid(&jwt, &forge(&header(), &Value::Object(payload)));

    // An expired token that is also wrong in another way is invalid, not
    // "expired": the clock is not consulted until everything else holds.
    let mut payload = claims();
    payload.insert("exp".into(), json!(0));
    payload.insert("iss".into(), json!("other"));
    assert_invalid(&jwt, &forge(&header(), &Value::Object(payload)));
}

#[test]
fn tokens_that_are_not_yet_valid_are_refused() {
    let (jwt, clock) = service();
    let now_ts = now().timestamp();

    // Issued in the future: through the leeway is fine, beyond it is not.
    let mut payload = claims();
    for (iat, ok) in [
        (now_ts + LEEWAY_SECS, true),
        (now_ts + LEEWAY_SECS + 1, false),
        (now_ts + 3_600, false),
        (i64::MAX, false),
    ] {
        payload.insert("iat".into(), json!(iat));
        payload.insert("exp".into(), json!(iat.saturating_add(900)));
        let token = forge(&header(), &Value::Object(payload.clone()));
        assert_eq!(
            jwt.verify(&token, TokenType::Access).is_ok(),
            ok,
            "iat {iat}"
        );
        if !ok {
            assert_invalid(&jwt, &token);
        }
    }

    // A token whose own service clock was ahead of ours.
    clock.advance(Duration::from_secs(3_600));
    let token = jwt.sign_access(Uuid::now_v7()).unwrap();
    clock.set(now());
    assert_invalid(&jwt, &token);

    // `nbf`, when a token has one.
    for (nbf, ok) in [
        (json!(now_ts), true),
        (json!(now_ts - 10), true),
        (json!(0), true),
        (json!(-5), true),
        (json!(now_ts + LEEWAY_SECS), true),
        (json!(now_ts + LEEWAY_SECS + 1), false),
        (json!(now_ts + 86_400), false),
        (json!("soon"), false),
        (json!(1.5), false),
        (json!(null), false),
        (json!([]), false),
    ] {
        let mut payload = claims();
        payload.insert("nbf".into(), nbf.clone());
        let token = forge(&header(), &Value::Object(payload));
        assert_eq!(
            jwt.verify(&token, TokenType::Access).is_ok(),
            ok,
            "nbf {nbf}"
        );
    }
}

#[test]
fn the_issuer_and_audience_must_match() {
    // No audience configured: the token must not carry one.
    let (jwt, _) = service();
    for iss in ["other", "Navree", "navree ", "", "navree\u{0}", "navree/"] {
        let mut payload = claims();
        payload.insert("iss".into(), json!(iss));
        assert_invalid(&jwt, &forge(&header(), &Value::Object(payload)));
    }
    for aud in [
        json!("crm-web"),
        json!("navree"),
        json!(["navree"]),
        json!(""),
    ] {
        let mut payload = claims();
        payload.insert("aud".into(), aud.clone());
        assert_invalid(&jwt, &forge(&header(), &Value::Object(payload)));
    }

    // Audience configured: the token must carry exactly that one.
    let (crm, _) = service_with(&audience_options());
    let mut good = claims();
    good.insert("iss".into(), json!("crm"));
    good.insert("aud".into(), json!("crm-web"));
    assert_valid(&crm, &forge(&header(), &Value::Object(good.clone())));
    for (name, value) in [
        ("aud", Some(json!("mobile"))),
        ("aud", Some(json!("CRM-web"))),
        ("aud", Some(json!("crm-web "))),
        ("aud", Some(json!(["crm-web"]))),
        ("aud", Some(json!(["crm-web", "mobile"]))),
        ("aud", Some(json!(""))),
        ("aud", Some(json!(null))),
        ("aud", None),
        ("iss", Some(json!("navree"))),
        ("iss", Some(json!("crm2"))),
        ("iss", None),
    ] {
        let mut payload = good.clone();
        match value {
            Some(value) => payload.insert(name.into(), value),
            None => payload.remove(name),
        };
        assert_invalid(&crm, &forge(&header(), &Value::Object(payload)));
    }
    // The default service's tokens are not the CRM's, and the reverse.
    let (plain, _) = service();
    let navree_token = plain.sign_access(Uuid::now_v7()).unwrap();
    let crm_token = crm.sign_access(Uuid::now_v7()).unwrap();
    assert_invalid(&crm, &navree_token);
    assert_invalid(&plain, &crm_token);
}

#[test]
fn every_required_claim_is_required() {
    let (jwt, _) = service();
    for name in ["sub", "iat", "exp", "iss", "jti", "typ"] {
        let mut payload = claims();
        payload.remove(name);
        assert_invalid(&jwt, &forge(&header(), &Value::Object(payload)));
        let mut payload = claims();
        payload.insert(name.into(), Value::Null);
        assert_invalid(&jwt, &forge(&header(), &Value::Object(payload)));
    }
    let (crm, _) = service_with(&audience_options());
    let mut payload = claims();
    payload.insert("iss".into(), json!("crm"));
    assert_invalid(&crm, &forge(&header(), &Value::Object(payload)));

    // Claims of the wrong kind.
    for (name, value) in [
        ("sub", json!("not-a-uuid")),
        ("sub", json!(42)),
        ("sub", json!("")),
        ("iat", json!("1792929600")),
        ("iat", json!(1_792_929_600.5)),
        ("exp", json!("2000000000")),
        ("exp", json!(2_000_000_000.5)),
        ("exp", json!(true)),
        ("jti", json!(7)),
        ("typ", json!("root")),
        ("typ", json!("Access")),
        ("typ", json!("")),
        ("typ", json!(1)),
        ("iss", json!(5)),
        ("sid", json!("not-a-uuid")),
        ("act", json!("admin")),
        ("sign_in", json!({ "method": "carrier_pigeon" })),
        ("sign_in", json!("password")),
    ] {
        let mut payload = claims();
        payload.insert(name.into(), value.clone());
        assert_invalid(&jwt, &forge(&header(), &Value::Object(payload)));
    }

    // The payload must be a JSON object.
    for payload in [
        "[]",
        "null",
        "\"token\"",
        "7",
        "true",
        "",
        "{",
        "{\"sub\":",
        "{}",
    ] {
        assert_invalid(
            &jwt,
            &forge_raw(header().to_string().as_bytes(), payload.as_bytes()),
        );
    }
}

#[test]
fn only_the_expected_types_are_accepted() {
    let (jwt, _) = service();
    let user = Uuid::now_v7();
    let ttl = Duration::from_secs(300);
    let challenge = jwt
        .sign_challenge(
            user,
            TokenType::TwoFactorChallenge,
            ttl,
            &SignInMethod::Password,
        )
        .unwrap();
    assert_invalid(&jwt, &challenge);
    assert!(
        jwt.verify(&challenge, TokenType::TwoFactorChallenge)
            .is_ok()
    );
    assert!(
        jwt.verify(&challenge, TokenType::PlatformChallenge)
            .is_err()
    );
    let setup = jwt.sign(user, TokenType::TwoFactorSetup, ttl).unwrap();
    assert_invalid(&jwt, &setup);
    assert!(
        jwt.verify_any(&setup, &[TokenType::Access, TokenType::TwoFactorSetup])
            .is_ok()
    );
    assert!(jwt.verify_any(&setup, &[]).is_err());
}

#[test]
fn oversized_tokens_are_refused_before_they_are_read() {
    let (jwt, _) = service();
    // A token the service signed, with a padding claim: fine up to the
    // limit, refused over it, whatever the signature says.
    let padded = |extra: usize| {
        let mut payload = claims();
        payload.insert("pad".into(), json!("x".repeat(extra)));
        forge(&header(), &Value::Object(payload))
    };
    let base = padded(0).len();
    let fits = ((MAX_TOKEN_LEN - base) * 3 / 4).saturating_sub(8);
    let ok = padded(fits);
    assert!(
        ok.len() <= MAX_TOKEN_LEN && ok.len() > MAX_TOKEN_LEN - 64,
        "{}",
        ok.len()
    );
    assert_valid(&jwt, &ok);
    for extra in [fits + 200, 20_000, 1 << 20] {
        let token = padded(extra);
        assert!(token.len() > MAX_TOKEN_LEN);
        assert_invalid(&jwt, &token);
    }

    // Junk of a size to hurt: a megabyte or more in one part or in each.
    let big = "A".repeat(1 << 20);
    for token in [
        format!("{big}.{big}.{big}"),
        format!("{big}.b.c"),
        format!("a.{big}.c"),
        format!("a.b.{big}"),
        big.clone(),
        ".".repeat(1 << 20),
    ] {
        assert_invalid(&jwt, &token);
    }
    // A header of 100 000 members, a payload nested 3 000 deep, and
    // numbers of 5 000 digits, each properly signed.
    let members: String = (0..100_000).map(|i| format!("\"k{i}\":1,")).collect();
    assert_invalid(
        &jwt,
        &forge_raw(format!("{{{members}\"alg\":\"HS256\"}}").as_bytes(), b"{}"),
    );
    let nested = format!("{}{}", "[".repeat(3_000), "]".repeat(3_000));
    assert_invalid(
        &jwt,
        &forge_raw(header().to_string().as_bytes(), nested.as_bytes()),
    );
    let nested_object = format!("{}1{}", "{\"a\":".repeat(3_000), "}".repeat(3_000));
    assert_invalid(
        &jwt,
        &forge_raw(header().to_string().as_bytes(), nested_object.as_bytes()),
    );
    let digits = "9".repeat(5_000);
    assert_invalid(
        &jwt,
        &forge_raw(
            header().to_string().as_bytes(),
            format!("{{\"exp\":{digits},\"sub\":1}}").as_bytes(),
        ),
    );
}

#[test]
fn malformed_tokens_are_refused() {
    let (jwt, _) = service();
    let token = jwt.sign_access(Uuid::now_v7()).unwrap();
    let [h, p, s]: [&str; 3] = token.split('.').collect::<Vec<_>>().try_into().unwrap();
    let cases: Vec<String> = vec![
        String::new(),
        ".".into(),
        "..".into(),
        "...".into(),
        "a".into(),
        "a.b".into(),
        "a.b.c".into(),
        "a.b.c.d".into(),
        format!("{h}.{p}"),
        format!("{h}.{p}."),
        format!(".{p}.{s}"),
        format!("{h}..{s}"),
        format!("{h}.{p}.{s}."),
        format!(".{h}.{p}.{s}"),
        // Whitespace and control characters around or inside.
        format!(" {token}"),
        format!("{token} "),
        format!("{token}\n"),
        format!("\t{token}"),
        format!("{token}\0"),
        format!("{h}. {p}.{s}"),
        format!("{h}.{p}.{s}\r\n"),
        // Padded base64url, standard base64, other alphabets.
        format!("{token}="),
        format!("{token}=="),
        format!("{h}=.{p}.{s}"),
        format!("{h}.{p}=.{s}"),
        format!("{h}.{p}.{s}="),
        format!("{token}+"),
        format!("{h}.{p}.{s}/"),
        format!("{h}.{p}.é{s}"),
        format!("{h}.{p}.{s}é"),
        "Bearer ".to_owned() + &token,
        format!("\"{token}\""),
        // Parts that decode but are not JSON, or not objects.
        format!("{}.{p}.{s}", b64(b"not json")),
        format!("{}.{p}.{s}", b64(b"[]")),
        format!("{}.{p}.{s}", b64(b"null")),
        format!("{}.{p}.{s}", b64(b"\"HS256\"")),
        format!("{}.{p}.{s}", b64(b"")),
        format!("{}.{p}.{s}", b64(b"{\"alg\":\"HS256\"")),
        format!("{}.{p}.{s}", b64(&[0xFF, 0xFE, 0xFD])),
        format!("{h}.{}.{s}", b64(b"not json")),
        format!("{h}.{}.{s}", b64(b"")),
        format!("{h}.{}.{s}", b64(&[0xFF; 20])),
        // Not base64url.
        format!("!!!.{p}.{s}"),
        format!("{h}.!!!.{s}"),
        format!("{h}.{p}.!!!"),
        format!("{h}.{p}.A"),
    ];
    for case in &cases {
        assert_invalid(&jwt, case);
    }
    // Signed headers and payloads that are broken JSON.
    for (header, payload) in [
        (&b"not json"[..], &b"{}"[..]),
        (b"[]", b"{}"),
        (b"{\"alg\":\"HS256\"", b"{}"),
        (b"\xFF\xFE", b"{}"),
        (br#"{"alg":"HS256","alg":"none"}"#, b"{}"),
        (b"{}", b"{}"),
    ] {
        assert_invalid(&jwt, &forge_raw(header, payload));
    }
    assert_valid(&jwt, &token);
}

#[test]
fn the_debug_output_hides_the_key() {
    let (jwt, _) = service();
    let debug = format!("{jwt:?}");
    assert!(!debug.contains("0123456789abcdef"), "{debug}");
    assert!(!debug.contains(&b64(SECRET)), "{debug}");
}
