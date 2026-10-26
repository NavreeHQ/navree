//! [`JwtService`]: HS256 access tokens and their [`Claims`].

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

/// Signs and verifies HS256 JWTs (jsonwebtoken on RustCrypto). Provided and
/// exported by the auth module, keyed by `auth.jwt.secret` or a key derived
/// from the application key.
///
/// Verification accepts only HS256, checks the signature, `iss`, `aud`
/// (when configured), the token type and the expiry with
/// [`LEEWAY_SECS`] of leeway, against the app's `dyn Clock` when one is
/// registered.
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
        // Only the RustCrypto backend is compiled in; installing it
        // explicitly keeps working if another crate enables aws-lc too.
        let _ = jsonwebtoken::crypto::rust_crypto::DEFAULT_PROVIDER.install_default();
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
    use uuid::Uuid;

    use super::*;
    use crate::{AuthError, JwtOptions};

    const SECRET: &[u8] = b"0123456789abcdef0123456789abcdef";

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
