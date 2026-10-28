//! [`JwtService`]: the HS256 tokens of navree-auth (access, two-factor
//! challenge and setup, platform) and their [`Claims`], on
//! `navree_crypto::jose`.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use navree_contracts::{Clock, SystemClock};
use navree_crypto::jose::{Algorithm, Jws, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{AuthError, JwtOptions, SignInMethod};

/// Clock skew tolerated on `exp` and `iat`, in seconds.
pub const LEEWAY_SECS: i64 = 30;

/// HS256 keys are at least as long as the hash output.
const HS256_MIN_KEY_LEN: usize = 32;

/// The claims of a navree JWT.
///
/// Access tokens also carry their session: `sid` (the refresh-token family
/// they belong to), `auth_time` (when the user last proved who they are:
/// signing in or `POST /auth/confirm`) and, for an impersonation session,
/// `act` (RFC 8693 §4.1: who is acting as the user).
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
    /// The session (refresh-token family) the token belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sid: Option<Uuid>,
    /// When the user last authenticated (Unix seconds): signing in, or
    /// confirming their password or a code at `POST /auth/confirm`.
    /// `#[recent_auth]` routes check it. None for impersonation sessions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_time: Option<i64>,
    /// The user acting as `sub` (an impersonation session).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub act: Option<ActorClaim>,
    /// How the user signed in, on a two-factor challenge token: the second
    /// step asks the [`SignInPolicy`](crate::SignInPolicy)s about it again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sign_in: Option<SignInMethod>,
}

/// The `act` claim of an impersonation token: who is acting as the
/// token's subject (RFC 8693 §4.1).
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActorClaim {
    /// The impersonating user's id.
    pub sub: Uuid,
}

/// The session claims of an access token ([`Claims::sid`],
/// [`Claims::auth_time`] and [`Claims::act`]), for
/// [`JwtService::sign_session`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SessionClaims {
    /// The session (refresh-token family) id.
    pub session_id: Option<Uuid>,
    /// When the user last authenticated (Unix seconds).
    pub auth_time: Option<i64>,
    /// The impersonating user, for an impersonation session.
    pub impersonator: Option<Uuid>,
}

/// What a JWT may be used for: [`verify`](JwtService::verify) rejects a
/// token of another type, so a two-factor challenge token is never an
/// access token.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum TokenType {
    /// An access token (`Authorization: Bearer`).
    Access,
    /// A short-lived token proving the password step of a two-factor login.
    TwoFactorChallenge,
    /// The access token of a user who must set up two-factor
    /// authentication first (`auth.two_factor.required_for` or a
    /// [`TwoFactorPolicy`](crate::TwoFactorPolicy)): it reaches only the
    /// two-factor setup routes, `GET /auth/me` and `POST /auth/logout`;
    /// everything else answers 403 `two_factor_required`.
    TwoFactorSetup,
    /// An access token of the platform audience (`auth.platform`): a
    /// platform administrator signed in with two-factor authentication.
    Platform,
    /// The challenge token of a platform sign-in (password checked, second
    /// factor pending).
    PlatformChallenge,
}

impl TokenType {
    /// The snake-case name (`access`, `two_factor_setup`, ...).
    pub fn as_str(self) -> &'static str {
        match self {
            TokenType::Access => "access",
            TokenType::TwoFactorChallenge => "two_factor_challenge",
            TokenType::TwoFactorSetup => "two_factor_setup",
            TokenType::Platform => "platform",
            TokenType::PlatformChallenge => "platform_challenge",
        }
    }
}
/// The longest token [`JwtService::verify`] reads, in bytes. Tokens this
/// service signs are under 1 KiB; anything longer was not made by it and is
/// refused before it is parsed.
pub const MAX_TOKEN_LEN: usize = 8 * 1024;

/// Signs and verifies the service's JWTs: HMAC-SHA-256 (`HS256`) compact
/// JWS on [`navree_crypto::jose`] (HMAC from `navree-crypto`, compared in
/// constant time). Provided and exported by the auth module, keyed by
/// `auth.jwt.secret` or a key derived from the application key.
///
/// # What a token is
///
/// - **Header**: `{"alg":"HS256","typ":"JWT"}`. There is no `kid`: the
///   service has one signing key, and a token that names a key id was not
///   made by it.
/// - **Claims**: [`Claims`] (`sub`, `iat`, `exp`, `iss`, `aud` when
///   configured, `jti`, `typ`, and the optional `sid`, `auth_time`, `act`
///   and `sign_in`).
/// - **Key rotation**: refresh tokens are opaque and stored, not JWTs, so
///   changing `auth.jwt.secret` (or the application key it is derived from)
///   invalidates only the access, challenge and setup tokens in flight,
///   which live minutes; sessions continue with their next refresh.
///
/// # What verification checks
///
/// In this order, each failure being [`AuthError::InvalidToken`]: the token
/// is at most [`MAX_TOKEN_LEN`] bytes and three unpadded base64url parts;
/// the header is JSON with `alg` `HS256` (so never `none`, never an RSA,
/// ECDSA or EdDSA algorithm, never another HMAC hash), no `kid` and no
/// `crit`; the HMAC matches, in constant time; the payload is JSON with
/// every required claim; `nbf`, if present, has passed (with
/// [`LEEWAY_SECS`] of leeway); `iss` equals the issuer; `aud` equals the
/// configured audience (and is absent when none is configured); `typ` is
/// one of the expected types; `iat` is not more than [`LEEWAY_SECS`] ahead
/// of the clock. Only then is `exp` checked, with the same leeway, and an
/// expired token is [`AuthError::TokenExpired`]. The clock is the app's
/// `dyn Clock` when one is registered.
pub struct JwtService {
    signing: SigningKey,
    verifying: VerifyingKey,
    issuer: String,
    audience: Option<String>,
    access_ttl: Duration,
    clock: Arc<dyn Clock>,
}

impl JwtService {
    /// A service signing with `secret` (use at least 32 random bytes) and
    /// the issuer, audience and access-token lifetime of `options`.
    ///
    /// HMAC treats a key shorter than its block as the key followed by
    /// zeros, so a secret under 32 bytes (which `AuthOptions` refuses to
    /// configure) is padded to 32 with zeros: it signs exactly what a
    /// service built on that short secret always signed, and is no
    /// stronger.
    pub fn new(secret: &[u8], options: &JwtOptions) -> Self {
        let mut key = Zeroizing::new(secret.to_vec());
        if key.len() < HS256_MIN_KEY_LEN {
            key.resize(HS256_MIN_KEY_LEN, 0);
        }
        let signing = SigningKey::hmac(Algorithm::HS256, key.as_slice())
            .expect("a secret of 32 bytes or more is an HS256 key");
        Self {
            verifying: signing.verifying_key(),
            signing,
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
        self.sign_session(user_id, typ, ttl, SessionClaims::default())
    }

    /// A token for `user_id` of type `typ`, valid for `ttl`, carrying the
    /// session claims `session` (`sid`, `auth_time`, `act`).
    pub fn sign_session(
        &self,
        user_id: Uuid,
        typ: TokenType,
        ttl: Duration,
        session: SessionClaims,
    ) -> Result<String, AuthError> {
        self.encode(user_id, typ, ttl, session, None)
    }

    /// A challenge token (`typ` [`TokenType::TwoFactorChallenge`] or
    /// [`TokenType::PlatformChallenge`]) for `user_id`, valid for `ttl`,
    /// carrying how the user signed in (`sign_in`).
    pub fn sign_challenge(
        &self,
        user_id: Uuid,
        typ: TokenType,
        ttl: Duration,
        method: &SignInMethod,
    ) -> Result<String, AuthError> {
        self.encode(
            user_id,
            typ,
            ttl,
            SessionClaims::default(),
            Some(method.clone()),
        )
    }

    fn encode(
        &self,
        user_id: Uuid,
        typ: TokenType,
        ttl: Duration,
        session: SessionClaims,
        sign_in: Option<SignInMethod>,
    ) -> Result<String, AuthError> {
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
            sid: session.session_id,
            auth_time: session.auth_time,
            act: session.impersonator.map(|sub| ActorClaim { sub }),
            sign_in,
        };
        self.signing
            .sign_jwt(&claims)
            .map_err(|error| AuthError::Signing(error.to_string()))
    }

    /// The current time of the service's clock, in Unix seconds.
    pub fn now(&self) -> i64 {
        self.clock.now().timestamp()
    }

    /// An access token for `user_id`, valid for [`access_ttl`](JwtService::access_ttl).
    pub fn sign_access(&self, user_id: Uuid) -> Result<String, AuthError> {
        self.sign(user_id, TokenType::Access, self.access_ttl)
    }

    /// The claims of `token` if it is a valid, unexpired token of type
    /// `expected`: [`AuthError::TokenExpired`] once it has expired,
    /// [`AuthError::InvalidToken`] for anything else.
    pub fn verify(&self, token: &str, expected: TokenType) -> Result<Claims, AuthError> {
        self.verify_any(token, &[expected])
    }

    /// Like [`verify`](JwtService::verify), accepting a token of any of
    /// the `expected` types. See the checks in the type's documentation.
    pub fn verify_any(&self, token: &str, expected: &[TokenType]) -> Result<Claims, AuthError> {
        // Our own tokens are short and unpadded; the signature covers the
        // text as sent, so a padded copy is a different string for the
        // same token and is not accepted.
        if token.len() > MAX_TOKEN_LEN || token.contains('=') {
            return Err(AuthError::InvalidToken);
        }
        let jws = Jws::parse(token).map_err(|_| AuthError::InvalidToken)?;
        let header = jws.header();
        if header.alg != Algorithm::HS256 || header.kid.is_some() {
            return Err(AuthError::InvalidToken);
        }
        jws.verify(&self.verifying)
            .map_err(|_| AuthError::InvalidToken)?;

        // The signature is good: the payload is ours.
        let payload: serde_json::Value = jws.claims().map_err(|_| AuthError::InvalidToken)?;
        let now = self.clock.now().timestamp();
        if let Some(not_before) = payload.get("nbf") {
            match not_before.as_i64() {
                Some(nbf) if nbf <= now.saturating_add(LEEWAY_SECS) => {}
                _ => return Err(AuthError::InvalidToken),
            }
        }
        let claims: Claims =
            serde_json::from_value(payload).map_err(|_| AuthError::InvalidToken)?;
        if claims.iss != self.issuer
            || claims.aud != self.audience
            || !expected.contains(&claims.typ)
            || claims.iat > now.saturating_add(LEEWAY_SECS)
        {
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
            sid: None,
            auth_time: None,
            act: None,
            sign_in: None,
        };
        let value = serde_json::to_value(&claims).unwrap();
        assert_eq!(value["typ"], "two_factor_challenge");
        assert!(value.get("aud").is_none());
        for absent in ["sid", "auth_time", "act", "sign_in"] {
            assert!(value.get(absent).is_none(), "{absent}");
        }
        for typ in [
            TokenType::Access,
            TokenType::TwoFactorChallenge,
            TokenType::TwoFactorSetup,
            TokenType::Platform,
            TokenType::PlatformChallenge,
        ] {
            assert_eq!(serde_json::to_value(typ).unwrap(), typ.as_str());
        }
    }

    #[test]
    fn session_claims_round_trip() {
        let (jwt, clock) = clocked();
        let (user, admin, session) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
        let auth_time = clock.now().timestamp() - 60;
        let token = jwt
            .sign_session(
                user,
                TokenType::Access,
                Duration::from_secs(900),
                SessionClaims {
                    session_id: Some(session),
                    auth_time: Some(auth_time),
                    impersonator: Some(admin),
                },
            )
            .unwrap();
        let claims = jwt.verify(&token, TokenType::Access).unwrap();
        assert_eq!(claims.sid, Some(session));
        assert_eq!(claims.auth_time, Some(auth_time));
        assert_eq!(claims.act, Some(ActorClaim { sub: admin }));
        assert_eq!(jwt.now(), clock.now().timestamp());

        // A token without them (older tokens, challenges) still verifies.
        let plain = jwt.sign_access(user).unwrap();
        let claims = jwt.verify(&plain, TokenType::Access).unwrap();
        assert_eq!(
            (claims.sid, claims.auth_time, claims.act),
            (None, None, None)
        );
    }

    #[test]
    fn verify_any_accepts_only_the_listed_types() {
        let jwt = service();
        let platform = jwt
            .sign(Uuid::now_v7(), TokenType::Platform, Duration::from_secs(60))
            .unwrap();
        assert!(
            jwt.verify_any(&platform, &[TokenType::Access, TokenType::Platform])
                .is_ok()
        );
        assert!(matches!(
            jwt.verify_any(&platform, &[TokenType::Access, TokenType::TwoFactorSetup]),
            Err(AuthError::InvalidToken)
        ));
        assert!(matches!(
            jwt.verify_any(&platform, &[]),
            Err(AuthError::InvalidToken)
        ));
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
    fn hs256_tokens_round_trip() {
        let jwt = service();
        let user = Uuid::now_v7();
        let token = jwt.sign_access(user).unwrap();
        let header = Jws::parse(&token).unwrap().header().clone();
        assert_eq!(header.alg, Algorithm::HS256);
        assert_eq!(header.typ.as_deref(), Some("JWT"));
        assert_eq!(header.kid, None);
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

    fn assert_invalid(jwt: &JwtService, token: &str) {
        assert!(
            matches!(
                jwt.verify(token, TokenType::Access),
                Err(AuthError::InvalidToken)
            ),
            "accepted {token}"
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
