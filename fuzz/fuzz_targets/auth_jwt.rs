//! `navree_auth::JwtService`: the bearer-token check every request goes
//! through.
//!
//! Byte 0 picks the mode:
//!
//! 0. a token as text, with the service's checks;
//! 1. JSON claims signed with the service's own secret (the signature is
//!    good, so every claims check runs);
//! 2. claims built from numbers, with an independent oracle for the
//!    decision (accepted, expired or invalid).
#![no_main]

use std::sync::{Arc, OnceLock};

use chrono::{TimeZone, Utc};
use libfuzzer_sys::fuzz_target;
use navree_auth::{AuthError, JwtService, LEEWAY_SECS, TokenType};
use navree_contracts::FixedClock;
use navree_crypto::jose::{Algorithm, SigningKey};
use navree_auth::JwtOptions;
use serde_json::{Value, json};

const SECRET: &[u8] = b"fuzzing-secret-0123456789abcdef!";
const NOW: i64 = 1_792_929_600; // 2026-10-25T12:00:00Z

const TYPES: [TokenType; 5] = [
    TokenType::Access,
    TokenType::TwoFactorChallenge,
    TokenType::TwoFactorSetup,
    TokenType::Platform,
    TokenType::PlatformChallenge,
];

fn service() -> &'static (JwtService, SigningKey) {
    static SERVICE: OnceLock<(JwtService, SigningKey)> = OnceLock::new();
    SERVICE.get_or_init(|| {
        let clock = Arc::new(FixedClock::new(Utc.timestamp_opt(NOW, 0).unwrap()));
        (
            JwtService::new(SECRET, &JwtOptions::default()).with_clock(clock),
            SigningKey::hmac(Algorithm::HS256, SECRET).unwrap(),
        )
    })
}

fn accepted(claims: &navree_auth::Claims, expected: &[TokenType]) {
    assert_eq!(claims.iss, "navree");
    assert_eq!(claims.aud, None);
    assert!(expected.contains(&claims.typ));
    assert!(claims.iat <= NOW + LEEWAY_SECS);
    assert!(claims.exp.saturating_add(LEEWAY_SECS) >= NOW);
}

fuzz_target!(|data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else {
        return;
    };
    let (jwt, key) = service();
    match mode % 3 {
        0 => {
            if let Ok(token) = std::str::from_utf8(rest) {
                if let Ok(claims) = jwt.verify_any(token, &TYPES) {
                    accepted(&claims, &TYPES);
                }
                let _ = jwt.verify(token, TokenType::Access);
            }
        }
        1 => {
            let Ok(claims) = serde_json::from_slice::<Value>(rest) else {
                return;
            };
            let token = key.sign_jwt(&claims).expect("a key signs claims");
            if let Ok(claims) = jwt.verify_any(&token, &TYPES) {
                accepted(&claims, &TYPES);
            }
        }
        _ => {
            if rest.len() < 16 + 8 * 3 + 2 {
                return;
            }
            let (id, rest) = rest.split_at(16);
            let number = |at: usize| {
                let mut bytes = [0_u8; 8];
                bytes.copy_from_slice(&rest[at * 8..at * 8 + 8]);
                i64::from_le_bytes(bytes)
            };
            // Times near "now" are the interesting ones.
            let near = |n: i64| {
                if n % 4 == 0 {
                    n
                } else {
                    NOW + n % 200 - 100
                }
            };
            let (iat, exp, nbf) = (near(number(0)), near(number(1)), near(number(2)));
            let flags = rest[24];
            let typ = TYPES[usize::from(rest[25]) % TYPES.len()];
            let issuer = if flags & 1 == 0 { "navree" } else { "other" };
            let mut claims = json!({
                "sub": uuid::Uuid::from_slice(id).unwrap(),
                "iat": iat,
                "exp": exp,
                "iss": issuer,
                "jti": "fuzz",
                "typ": typ,
            });
            if flags & 2 != 0 {
                claims["aud"] = json!("someone");
            }
            if flags & 4 != 0 {
                claims["nbf"] = json!(nbf);
            }
            let token = key.sign_jwt(&claims).expect("a key signs claims");
            let result = jwt.verify_any(&token, &[TokenType::Access, TokenType::Platform]);
            let expected = [TokenType::Access, TokenType::Platform];
            let valid = issuer == "navree"
                && flags & 2 == 0
                && expected.contains(&typ)
                && iat <= NOW.saturating_add(LEEWAY_SECS)
                && (flags & 4 == 0 || nbf <= NOW.saturating_add(LEEWAY_SECS));
            match result {
                Ok(claims) => {
                    assert!(valid && exp.saturating_add(LEEWAY_SECS) >= NOW);
                    accepted(&claims, &expected);
                }
                Err(AuthError::TokenExpired) => {
                    assert!(valid && exp.saturating_add(LEEWAY_SECS) < NOW);
                }
                Err(AuthError::InvalidToken) => assert!(!valid),
                Err(other) => panic!("unexpected error {other:?}"),
            }
        }
    }
});
