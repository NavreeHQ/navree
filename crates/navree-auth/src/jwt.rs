//! [`JwtService`]: HS256 access tokens and their [`Claims`].

mod provider;

use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use jsonwebtoken::errors::ErrorKind;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use navree_contracts::{Clock, SystemClock};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{AuthError, JwtOptions};

/// Clock skew tolerated on `exp` and `iat`, in seconds.
pub const LEEWAY_SECS: i64 = 30;

/// The claims of a navree JWT.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Claims {
    /// The user id.
    pub sub: Uuid,
    /// Issued at (Unix seconds).
    pub iat: i64,
    /// Expires at (Unix seconds).
    pub exp: i64,
    /// The issuer (`auth.jwt.issuer`).
    pub iss: String,
    /// The audience (`auth.jwt.audience`), if configured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aud: Option<String>,
    /// A unique token id.
    pub jti: String,
    /// What the token may be used for.
    pub typ: TokenType,
}

/// What a JWT may be used for: [`verify`](JwtService::verify) rejects a
/// token of another type, so a two-factor challenge token is never an
/// access token.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TokenType {
    /// An access token (`Authorization: Bearer`).
    Access,
    /// A short-lived token proving the password step of a two-factor login.
    TwoFactorChallenge,
}

/// Signs and verifies HS256 JWTs (jsonwebtoken with an HMAC-only RustCrypto
/// provider). Provided and exported by the auth module, keyed by
/// `auth.jwt.secret` or a key derived from the application key.
///
/// Verification accepts only HS256, checks the signature (in constant
/// time), `iss`, `aud` (when configured), the token type and the expiry
/// with [`LEEWAY_SECS`] of leeway, against the app's `dyn Clock` when one
/// is registered.
///
/// Building one installs navree-auth's jsonwebtoken `CryptoProvider` for
/// the process unless one is installed already. It has no RSA, ECDSA or
/// EdDSA: in-process users of jsonwebtoken that need those install their
/// own provider before the auth module is built.
pub struct JwtService {
    encoding: EncodingKey,
    decoding: DecodingKey,
    validation: Validation,
    issuer: String,
    audience: Option<String>,
    access_ttl: Duration,
    clock: Arc<dyn Clock>,
}

impl JwtService {
    /// A service signing with `secret` (use at least 32 random bytes) and
    /// the issuer, audience and access-token lifetime of `options`.
    pub fn new(secret: &[u8], options: &JwtOptions) -> Self {
        // jsonwebtoken is built without a crypto backend: install the
        // HMAC-only one before any token is signed or verified.
        provider::install();
        let mut validation = Validation::new(Algorithm::HS256);
        // Expiry is checked against the app's clock in `verify`.
        validation.validate_exp = false;
        validation.validate_nbf = false;
        let mut required: HashSet<String> = ["exp", "iss", "sub"].map(str::to_owned).into();
        validation.set_issuer(&[options.issuer.as_str()]);
        match &options.audience {
            Some(audience) => {
                validation.set_audience(&[audience.as_str()]);
                required.insert("aud".to_owned());
            }
            // A token carrying an audience is rejected (jsonwebtoken's default).
            None => validation.aud = None,
        }
        validation.required_spec_claims = required;
        Self {
            encoding: EncodingKey::from_secret(secret),
            decoding: DecodingKey::from_secret(secret),
            validation,
            issuer: options.issuer.clone(),
            audience: options.audience.clone(),
            access_ttl: Duration::from_secs(options.access_ttl_secs),
            clock: Arc::new(SystemClock),
        }
    }

    /// The same service reading the time from `clock`.
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// The lifetime of access tokens (`auth.jwt.access_ttl_secs`).
    pub fn access_ttl(&self) -> Duration {
        self.access_ttl
    }

    /// A token for `user_id` of type `typ`, valid for `ttl`.
    pub fn sign(&self, user_id: Uuid, typ: TokenType, ttl: Duration) -> Result<String, AuthError> {
        let iat = self.clock.now().timestamp();
        let ttl = i64::try_from(ttl.as_secs()).unwrap_or(i64::MAX);
        let claims = Claims {
            sub: user_id,
            iat,
            exp: iat.saturating_add(ttl),
            iss: self.issuer.clone(),
            aud: self.audience.clone(),
            jti: Uuid::now_v7().simple().to_string(),
            typ,
        };
        jsonwebtoken::encode(&Header::new(Algorithm::HS256), &claims, &self.encoding)
            .map_err(|error| AuthError::Signing(error.to_string()))
    }

    /// An access token for `user_id`, valid for [`access_ttl`](JwtService::access_ttl).
    pub fn sign_access(&self, user_id: Uuid) -> Result<String, AuthError> {
        self.sign(user_id, TokenType::Access, self.access_ttl)
    }

    /// The claims of `token` if it is a valid, unexpired token of type
    /// `expected`: [`AuthError::TokenExpired`] once it has expired,
    /// [`AuthError::InvalidToken`] for anything else.
    pub fn verify(&self, token: &str, expected: TokenType) -> Result<Claims, AuthError> {
        let data = jsonwebtoken::decode::<Claims>(token, &self.decoding, &self.validation)
            .map_err(|error| match error.kind() {
                ErrorKind::ExpiredSignature => AuthError::TokenExpired,
                _ => AuthError::InvalidToken,
            })?;
        let claims = data.claims;
        let now = self.clock.now().timestamp();
        if claims.typ != expected || claims.iat > now.saturating_add(LEEWAY_SECS) {
            return Err(AuthError::InvalidToken);
        }
        if claims.exp.saturating_add(LEEWAY_SECS) < now {
            return Err(AuthError::TokenExpired);
        }
        Ok(claims)
    }
}

impl fmt::Debug for JwtService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JwtService")
            .field("issuer", &self.issuer)
            .field("audience", &self.audience)
            .field("access_ttl", &self.access_ttl)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use chrono::{TimeZone, Utc};
    use navree_contracts::FixedClock;
    use serde_json::json;
    use uuid::Uuid;

    use super::*;
    use crate::{AuthError, JwtOptions};

    const SECRET: &[u8] = b"0123456789abcdef0123456789abcdef";

    /// Every algorithm jsonwebtoken knows besides the HMAC family.
    const NOT_HMAC: [Algorithm; 9] = [
        Algorithm::ES256,
        Algorithm::ES384,
        Algorithm::RS256,
        Algorithm::RS384,
        Algorithm::RS512,
        Algorithm::PS256,
        Algorithm::PS384,
        Algorithm::PS512,
        Algorithm::EdDSA,
    ];

    fn service() -> JwtService {
        JwtService::new(SECRET, &JwtOptions::default())
    }

    fn clocked() -> (JwtService, Arc<FixedClock>) {
        let clock = Arc::new(FixedClock::new(
            Utc.with_ymd_and_hms(2026, 10, 25, 12, 0, 0).unwrap(),
        ));
        (service().with_clock(clock.clone()), clock)
    }

    #[test]
    fn sign_and_verify_round_trip() {
        let (jwt, clock) = clocked();
        let user = Uuid::now_v7();
        let token = jwt
            .sign(user, TokenType::Access, Duration::from_secs(900))
            .unwrap();
        let claims = jwt.verify(&token, TokenType::Access).unwrap();
        assert_eq!(claims.sub, user);
        assert_eq!(claims.typ, TokenType::Access);
        assert_eq!(claims.iss, "navree");
        assert_eq!(claims.aud, None);
        assert_eq!(claims.iat, clock.now().timestamp());
        assert_eq!(claims.exp, claims.iat + 900);
        assert!(!claims.jti.is_empty());
        let other = jwt
            .sign(user, TokenType::Access, Duration::from_secs(900))
            .unwrap();
        assert_ne!(
            jwt.verify(&other, TokenType::Access).unwrap().jti,
            claims.jti
        );
    }

    #[test]
    fn claims_serialize_typ_in_snake_case_and_skip_missing_audience() {
        let claims = Claims {
            sub: Uuid::nil(),
            iat: 1,
            exp: 2,
            iss: "navree".to_owned(),
            aud: None,
            jti: "j".to_owned(),
            typ: TokenType::TwoFactorChallenge,
        };
        let value = serde_json::to_value(&claims).unwrap();
        assert_eq!(value["typ"], "two_factor_challenge");
        assert!(value.get("aud").is_none());
    }

    #[test]
    fn a_token_of_another_type_is_rejected() {
        let jwt = service();
        let challenge = jwt
            .sign(
                Uuid::now_v7(),
                TokenType::TwoFactorChallenge,
                Duration::from_secs(300),
            )
            .unwrap();
        assert!(matches!(
            jwt.verify(&challenge, TokenType::Access),
            Err(AuthError::InvalidToken)
        ));
        assert!(
            jwt.verify(&challenge, TokenType::TwoFactorChallenge)
                .is_ok()
        );
    }

    #[test]
    fn expiry_follows_the_clock_with_30_seconds_leeway() {
        let (jwt, clock) = clocked();
        let token = jwt
            .sign(Uuid::now_v7(), TokenType::Access, Duration::from_secs(60))
            .unwrap();
        clock.advance(Duration::from_secs(60 + 29));
        assert!(jwt.verify(&token, TokenType::Access).is_ok());
        clock.advance(Duration::from_secs(2));
        assert!(matches!(
            jwt.verify(&token, TokenType::Access),
            Err(AuthError::TokenExpired)
        ));
    }

    #[test]
    fn a_token_issued_in_the_future_is_rejected() {
        let (jwt, clock) = clocked();
        clock.advance(Duration::from_secs(120));
        let token = jwt
            .sign(Uuid::now_v7(), TokenType::Access, Duration::from_secs(900))
            .unwrap();
        clock.set(clock.now() - chrono::TimeDelta::seconds(120));
        assert!(matches!(
            jwt.verify(&token, TokenType::Access),
            Err(AuthError::InvalidToken)
        ));
    }

    #[test]
    fn tampered_and_foreign_tokens_are_rejected() {
        let jwt = service();
        let token = jwt
            .sign(Uuid::now_v7(), TokenType::Access, Duration::from_secs(900))
            .unwrap();
        let mut parts: Vec<String> = token.split('.').map(str::to_owned).collect();
        assert_eq!(parts.len(), 3);

        // Another user id in the payload, same signature.
        let forged_claims = Claims {
            sub: Uuid::now_v7(),
            ..jwt.verify(&token, TokenType::Access).unwrap()
        };
        parts[1] = b64(&serde_json::to_vec(&forged_claims).unwrap());
        let forged = parts.join(".");
        assert!(matches!(
            jwt.verify(&forged, TokenType::Access),
            Err(AuthError::InvalidToken)
        ));

        // `alg: none` without a signature.
        let none = format!("{}.{}.", b64(br#"{"alg":"none","typ":"JWT"}"#), parts[1]);
        assert!(jwt.verify(&none, TokenType::Access).is_err());

        // Signed with another secret.
        let other = JwtService::new(b"another-secret-another-secret-xx", &JwtOptions::default());
        let foreign = other
            .sign(Uuid::now_v7(), TokenType::Access, Duration::from_secs(900))
            .unwrap();
        assert!(matches!(
            jwt.verify(&foreign, TokenType::Access),
            Err(AuthError::InvalidToken)
        ));

        for garbage in ["", "not-a-jwt", "a.b.c"] {
            assert!(matches!(
                jwt.verify(garbage, TokenType::Access),
                Err(AuthError::InvalidToken)
            ));
        }
    }

    #[test]
    fn issuer_and_audience_are_checked() {
        let options = JwtOptions {
            issuer: "crm".to_owned(),
            audience: Some("crm-web".to_owned()),
            ..JwtOptions::default()
        };
        let crm = JwtService::new(SECRET, &options);
        let token = crm
            .sign(Uuid::now_v7(), TokenType::Access, Duration::from_secs(900))
            .unwrap();
        let claims = crm.verify(&token, TokenType::Access).unwrap();
        assert_eq!(claims.aud.as_deref(), Some("crm-web"));

        // Same secret, other issuer: rejected both ways.
        let navree = service();
        assert!(matches!(
            navree.verify(&token, TokenType::Access),
            Err(AuthError::InvalidToken)
        ));
        let navree_token = navree
            .sign(Uuid::now_v7(), TokenType::Access, Duration::from_secs(900))
            .unwrap();
        assert!(matches!(
            crm.verify(&navree_token, TokenType::Access),
            Err(AuthError::InvalidToken)
        ));

        // Same issuer, other audience.
        let other_audience = JwtService::new(
            SECRET,
            &JwtOptions {
                audience: Some("mobile".to_owned()),
                ..options
            },
        );
        assert!(matches!(
            other_audience.verify(&token, TokenType::Access),
            Err(AuthError::InvalidToken)
        ));
    }

    #[test]
    fn debug_output_hides_the_keys() {
        let debug = format!("{:?}", service());
        assert!(!debug.contains("0123456789abcdef"), "{debug}");
    }

    #[test]
    fn rs256_and_unsigned_tokens_are_rejected() {
        let jwt = service();
        let token = jwt.sign_access(Uuid::now_v7()).unwrap();
        let payload = token.split('.').nth(1).unwrap();

        // Another algorithm in the header, signed with an HMAC of the secret
        // (algorithm confusion) or with signature-sized garbage.
        for alg in ["RS256", "RS512", "PS256", "ES256", "ES384", "EdDSA"] {
            let header = format!(r#"{{"alg":"{alg}","typ":"JWT"}}"#);
            let message = format!("{}.{payload}", b64(header.as_bytes()));
            let hmac = jsonwebtoken::crypto::sign(
                message.as_bytes(),
                &EncodingKey::from_secret(SECRET),
                Algorithm::HS256,
            )
            .unwrap();
            assert_invalid(&jwt, &format!("{message}.{hmac}"));
            assert_invalid(&jwt, &format!("{message}.{}", b64(&[7; 256])));
        }

        // Unsigned: `alg: none` (any spelling), or HS256 without a signature.
        for header in [
            r#"{"alg":"none","typ":"JWT"}"#,
            r#"{"alg":"None"}"#,
            r#"{"alg":"NONE"}"#,
            r#"{"alg":"HS256","typ":"JWT"}"#,
        ] {
            let message = format!("{}.{payload}", b64(header.as_bytes()));
            assert_invalid(&jwt, &format!("{message}."));
            assert_invalid(&jwt, &message);
        }

        // HS512 with the right secret: verification is pinned to HS256.
        let claims = jwt.verify(&token, TokenType::Access).unwrap();
        let hs512 = jsonwebtoken::encode(
            &Header::new(Algorithm::HS512),
            &claims,
            &EncodingKey::from_secret(SECRET),
        )
        .unwrap();
        assert_invalid(&jwt, &hs512);
    }

    #[test]
    fn hs256_tokens_round_trip() {
        let jwt = service();
        let user = Uuid::now_v7();
        let token = jwt.sign_access(user).unwrap();
        let header = jsonwebtoken::decode_header(&token).unwrap();
        assert_eq!(header.alg, Algorithm::HS256);
        assert_eq!(header.typ.as_deref(), Some("JWT"));
        assert_eq!(jwt.verify(&token, TokenType::Access).unwrap().sub, user);

        // Another instance with the same secret (another app server).
        let replica = JwtService::new(SECRET, &JwtOptions::default());
        assert_eq!(replica.verify(&token, TokenType::Access).unwrap().sub, user);
    }

    #[test]
    fn a_token_signed_with_another_secret_is_rejected() {
        let jwt = service();
        for secret in [
            b"another-secret-another-secret-xx".as_slice(),
            // One character off, and a prefix of the real secret.
            b"0123456789abcdef0123456789abcdeF",
            &SECRET[..31],
        ] {
            let other = JwtService::new(secret, &JwtOptions::default());
            let token = other.sign_access(Uuid::now_v7()).unwrap();
            assert!(other.verify(&token, TokenType::Access).is_ok());
            assert_invalid(&jwt, &token);
        }
    }

    #[test]
    fn tampered_tokens_are_rejected() {
        let jwt = service();
        let token = jwt.sign_access(Uuid::now_v7()).unwrap();
        let other_token = jwt.sign_access(Uuid::now_v7()).unwrap();
        let claims = jwt.verify(&token, TokenType::Access).unwrap();
        let [header, payload, signature]: [&str; 3] =
            token.split('.').collect::<Vec<_>>().try_into().unwrap();

        // Edited claims under the original signature.
        for forged in [
            Claims {
                sub: Uuid::now_v7(),
                ..claims.clone()
            },
            Claims {
                exp: claims.exp + 86_400,
                ..claims.clone()
            },
            Claims {
                jti: "another".to_owned(),
                ..claims.clone()
            },
        ] {
            let forged = b64(&serde_json::to_vec(&forged).unwrap());
            assert_invalid(&jwt, &format!("{header}.{forged}.{signature}"));
        }

        // An edited header under the original signature.
        for alg in ["HS384", "HS512"] {
            let edited = b64(format!(r#"{{"typ":"JWT","alg":"{alg}"}}"#).as_bytes());
            assert_invalid(&jwt, &format!("{edited}.{payload}.{signature}"));
        }

        // Edited signatures: one character, truncated, extended, empty,
        // another token's.
        let flipped = match signature.as_bytes()[0] {
            b'A' => format!("B{}", &signature[1..]),
            _ => format!("A{}", &signature[1..]),
        };
        let other_signature = other_token.rsplit('.').next().unwrap();
        for edited in [
            flipped.as_str(),
            &signature[..signature.len() - 1],
            &format!("{signature}AA"),
            "",
            other_signature,
        ] {
            assert_invalid(&jwt, &format!("{header}.{payload}.{edited}"));
        }

        // Rearranged or extra segments.
        assert_invalid(&jwt, &format!("{payload}.{header}.{signature}"));
        assert_invalid(&jwt, &format!("{header}.{payload}.{signature}.{signature}"));
        assert!(jwt.verify(&token, TokenType::Access).is_ok());
    }

    /// `jsonwebtoken::crypto::{sign, verify}` go straight to the installed
    /// provider, without the algorithm checks of `encode` and `decode`.
    #[test]
    fn the_jwt_crypto_provider_refuses_every_algorithm_but_hmac() {
        let _ = service();
        let message = b"header.payload";
        for alg in NOT_HMAC {
            for key in [
                EncodingKey::from_secret(SECRET),
                EncodingKey::from_rsa_der(b"0"),
                EncodingKey::from_ec_der(b"0"),
                EncodingKey::from_ed_der(b"0"),
            ] {
                let Err(error) = jsonwebtoken::crypto::sign(message, &key, alg) else {
                    panic!("signed with {alg:?}");
                };
                assert_unsupported(&error, alg);
            }
            for key in [
                DecodingKey::from_secret(SECRET),
                DecodingKey::from_rsa_der(b"0"),
                DecodingKey::from_ec_der(b"0"),
                DecodingKey::from_ed_der(b"0"),
            ] {
                let Err(error) = jsonwebtoken::crypto::verify("c2ln", message, &key, alg) else {
                    panic!("verified with {alg:?}");
                };
                assert_unsupported(&error, alg);
            }
        }
    }

    #[test]
    fn jsonwebtoken_cannot_sign_or_verify_rs256_even_when_asked() {
        let _ = service();
        let header = b64(br#"{"alg":"RS256","typ":"JWT"}"#);
        let token = format!("{header}.{}.{}", b64(br#"{"sub":"x"}"#), b64(&[7; 256]));
        let mut validation = Validation::new(Algorithm::RS256);
        validation.required_spec_claims.clear();
        validation.validate_exp = false;
        let decoded = jsonwebtoken::decode::<serde_json::Value>(
            &token,
            &DecodingKey::from_rsa_der(b"0"),
            &validation,
        );
        let Err(error) = decoded else {
            panic!("an RS256 token was accepted");
        };
        assert_unsupported(&error, Algorithm::RS256);

        let signed = jsonwebtoken::encode(
            &Header::new(Algorithm::RS256),
            &json!({"sub": "x"}),
            &EncodingKey::from_rsa_der(b"0"),
        );
        let Err(error) = signed else {
            panic!("an RS256 token was signed");
        };
        assert_unsupported(&error, Algorithm::RS256);
    }

    #[test]
    fn hmac_signatures_match_known_answers_and_round_trip() {
        let _ = service();
        // RFC 7515 appendix A.1 (HS256); HS384 and HS512 over the same key
        // and message, computed with Python's `hmac`.
        let key = EncodingKey::from_urlsafe_base64_secret(
            "AyM1SysPpbyDfgZld3umj1qzKObwVMkoqQ-EstJQLr_T-1qS0gZH75aKtMN3Yj0iPS4hcgUuTwjAzZr1Z9CAow==",
        )
        .unwrap();
        let message = "eyJ0eXAiOiJKV1QiLA0KICJhbGciOiJIUzI1NiJ9.\
            eyJpc3MiOiJqb2UiLA0KICJleHAiOjEzMDA4MTkzODAsDQogImh0dHA6Ly9leGFtcGxlLmNvbS9pc19yb290Ijp0cnVlfQ";
        let decoding = DecodingKey::from_secret(key.as_bytes());
        for (alg, expected) in [
            (
                Algorithm::HS256,
                "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk",
            ),
            (
                Algorithm::HS384,
                "azVX8-2q_DEdR5zRuXcG0qXAudGzchZHmWo-uKNQi_S5q9lpGrlH7hnDyIKEhZmX",
            ),
            (
                Algorithm::HS512,
                "MNHB8V8aB0FknfHyFENJHJkpilOknuKGNCH5xJc_m1eUHtYzi0zNjSqbQ5IK9BYQb9_dyIzws4Zjt3AcsuKZAg",
            ),
        ] {
            let signature = jsonwebtoken::crypto::sign(message.as_bytes(), &key, alg).unwrap();
            assert_eq!(signature, expected, "{alg:?}");
            let verify = |signature: &str| {
                matches!(
                    jsonwebtoken::crypto::verify(signature, message.as_bytes(), &decoding, alg),
                    Ok(true)
                )
            };
            assert!(verify(expected), "{alg:?}");
            assert!(!verify(&expected[1..]), "{alg:?}");
            assert!(!verify(""), "{alg:?}");

            // Through `encode` and `decode`.
            let mut validation = Validation::new(alg);
            validation.required_spec_claims.clear();
            validation.validate_exp = false;
            let token = jsonwebtoken::encode(
                &Header::new(alg),
                &json!({"sub": "joe"}),
                &EncodingKey::from_secret(SECRET),
            )
            .unwrap();
            let data = jsonwebtoken::decode::<serde_json::Value>(
                &token,
                &DecodingKey::from_secret(SECRET),
                &validation,
            )
            .unwrap();
            assert_eq!(data.header.alg, alg);
            assert_eq!(data.claims["sub"], "joe");
            let wrong = jsonwebtoken::decode::<serde_json::Value>(
                &token,
                &DecodingKey::from_secret(b"another-secret-another-secret-xx"),
                &validation,
            );
            assert!(
                matches!(
                    wrong.map(|_| ()).unwrap_err().kind(),
                    ErrorKind::InvalidSignature
                ),
                "{alg:?}"
            );
        }
    }

    fn assert_invalid(jwt: &JwtService, token: &str) {
        assert!(
            matches!(
                jwt.verify(token, TokenType::Access),
                Err(AuthError::InvalidToken)
            ),
            "accepted {token}"
        );
    }

    fn assert_unsupported(error: &jsonwebtoken::errors::Error, alg: Algorithm) {
        assert!(
            matches!(error.kind(), ErrorKind::UnsupportedAlgorithm),
            "{alg:?}: {error:?}"
        );
    }

    fn b64(bytes: &[u8]) -> String {
        use base64_url::encode;
        encode(bytes)
    }

    /// Unpadded base64url, enough for forging test tokens.
    mod base64_url {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

        pub(super) fn encode(bytes: &[u8]) -> String {
            let mut out = String::new();
            for chunk in bytes.chunks(3) {
                let b = [
                    chunk[0],
                    chunk.get(1).copied().unwrap_or(0),
                    chunk.get(2).copied().unwrap_or(0),
                ];
                let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
                let chars = chunk.len() + 1;
                for i in 0..chars {
                    let index = (n >> (18 - 6 * i)) & 63;
                    out.push(ALPHABET[index as usize] as char);
                }
            }
            out
        }
    }
}
