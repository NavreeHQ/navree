//! TOTP two-factor authentication on [`AuthService`]
//! (`navree_crypto::totp`, QR codes from `navree_crypto::qr`): setup,
//! confirmation with recovery codes, the login challenge and its
//! verification.
//!
//! - The TOTP secret (160 random bits, SHA-1, 6 digits, 30-second steps,
//!   one step of clock skew) is stored encrypted with the app's
//!   `Encrypter` in `users.two_factor_secret`.
//! - Recovery codes are shown once; `users.two_factor_recovery_codes`
//!   holds the encrypted list of their SHA-256 digests, and a code is
//!   removed when used.
//! - A TOTP code is accepted once: a code of the same or an earlier step
//!   than the last accepted one is refused (RFC 6238 §5.2).
//! - The login challenge is a JWT of type
//!   [`TokenType::TwoFactorChallenge`] (never an access token), valid for
//!   `auth.two_factor.challenge_ttl_secs` and accepted once.

use std::fmt;
use std::time::Duration;

use navree_contracts::{AuditEntry, AuditKind};
use navree_crypto::qr::{EcLevel, QrCode};
use navree_crypto::totp::{Secret, Totp};
use navree_crypto::{constant_time_eq, random_token, sha256_hex};
use navree_db::Uow;
use navree_openapi::ToSchema;
use navree_orm::Model as _;
use serde::Serialize;
use serde_json::json;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::service::entry;
use crate::{
    AuthError, AuthService, ClientMeta, LEEWAY_SECS, SignInAttempt, SignInMethod, TokenPair,
    TokenType, User, user,
};

/// Characters per half of a recovery code (`xxxxxxxxxx-xxxxxxxxxx`).
const RECOVERY_HALF_LEN: usize = 10;

/// The width the setup QR code is drawn at, at least, in pixels.
const QR_MIN_SIZE: u32 = 200;

/// How long the last accepted TOTP step of a user is remembered: longer
/// than any code stays valid (three 30-second steps).
const LAST_STEP_TTL: Duration = Duration::from_secs(300);

/// What `POST /auth/2fa/enable` returns: the new TOTP secret for the
/// user's authenticator app. Two-factor authentication is active once the
/// user confirms a code from it (`POST /auth/2fa/confirm`).
#[derive(Clone, Serialize, ToSchema)]
pub struct TwoFactorSetup {
    /// The secret, base32-encoded (for manual entry).
    pub secret: String,
    /// The `otpauth://totp/{issuer}:{email}?secret=..&issuer=..` URL.
    pub otpauth_url: String,
    /// The URL as a QR code, an SVG document.
    pub qr_svg: String,
}

/// The secret is a credential: never in debug output.
impl fmt::Debug for TwoFactorSetup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwoFactorSetup").finish_non_exhaustive()
    }
}

/// The second factor presented at `POST /auth/2fa/verify`.
#[derive(Clone, PartialEq, Eq)]
pub enum TwoFactorProof {
    /// A code from the authenticator app.
    Code(String),
    /// One of the recovery codes (single use; case-insensitive).
    RecoveryCode(String),
}

impl TwoFactorProof {
    /// `"totp"` or `"recovery_code"` (audit metadata).
    pub fn method(&self) -> &'static str {
        match self {
            TwoFactorProof::Code(_) => "totp",
            TwoFactorProof::RecoveryCode(_) => "recovery_code",
        }
    }
}

/// The codes are credentials: never in debug output.
impl fmt::Debug for TwoFactorProof {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TwoFactorProof::{}([redacted])", self.method())
    }
}

/// The result of [`AuthService::attempt_login`].
#[derive(Debug)]
pub enum LoginOutcome {
    /// Signed in: the user and a new token pair.
    Authenticated {
        /// The user.
        user: User,
        /// The new session's tokens.
        tokens: TokenPair,
    },
    /// The password was right; the account uses two-factor authentication.
    /// Finish with [`AuthService::verify_two_factor`].
    TwoFactorRequired {
        /// The user.
        user: User,
        /// The challenge token ([`TokenType::TwoFactorChallenge`]).
        challenge_token: String,
    },
}

/// Whether `user` has confirmed two-factor authentication (their logins
/// need a second factor).
pub fn two_factor_enabled(user: &User) -> bool {
    user.two_factor_confirmed_at.is_some()
}

impl AuthService {
    /// Starts the two-factor setup of `user_id` (`POST /auth/2fa/enable`):
    /// a new secret, stored encrypted and replacing any unconfirmed one.
    /// Logins are unaffected until [`confirm_two_factor`](AuthService::confirm_two_factor).
    /// [`AuthError::TwoFactorAlreadyEnabled`] once confirmed (disable it
    /// first). Audited as `auth.two_factor_enabled`.
    pub async fn enable_two_factor(
        &self,
        user_id: Uuid,
        meta: &ClientMeta,
    ) -> Result<TwoFactorSetup, AuthError> {
        let user = self.user_or_not_found(user_id).await?;
        if two_factor_enabled(&user) {
            return Err(AuthError::TwoFactorAlreadyEnabled);
        }
        let secret = Secret::generate().map_err(|error| AuthError::TwoFactor(error.to_string()))?;
        let encoded = secret.to_base32();
        let totp = self.totp(&user, secret)?;
        let otpauth_url = totp
            .to_url()
            .map_err(|error| AuthError::TwoFactor(error.to_string()))?;
        let qr_svg = QrCode::encode(otpauth_url.as_bytes(), EcLevel::M)
            .map_err(|error| AuthError::TwoFactor(error.to_string()))?
            .to_svg(QR_MIN_SIZE);
        let stored = self.encrypter.encrypt_string(&encoded)?;
        let now = self.clock.now();
        let audit = self.two_factor_entry("auth.two_factor_enabled", &user, meta);
        self.write(Some(user.id), |uow| async move {
            user::update(&uow, user.id, |user| {
                user.two_factor_secret = Some(stored);
                user.two_factor_recovery_codes = None;
                user.two_factor_confirmed_at = None;
                user.updated_at = now;
            })
            .await?;
            self.audit_after_commit(&uow, audit).await;
            Ok(())
        })
        .await?;
        Ok(TwoFactorSetup {
            secret: encoded,
            otpauth_url,
            qr_svg,
        })
    }

    /// Finishes the setup (`POST /auth/2fa/confirm`) with a code from the
    /// authenticator app: two-factor authentication is on from now, and
    /// the returned recovery codes (`auth.two_factor.recovery_codes` of
    /// them) are the only time they are shown. A wrong code is a field
    /// error on `code`; [`AuthError::TwoFactorNotEnabled`] without a
    /// pending setup. Audited as `auth.two_factor_confirmed`.
    pub async fn confirm_two_factor(
        &self,
        user_id: Uuid,
        code: &str,
        meta: &ClientMeta,
    ) -> Result<Vec<String>, AuthError> {
        let user = self.user_or_not_found(user_id).await?;
        if two_factor_enabled(&user) {
            return Err(AuthError::TwoFactorAlreadyEnabled);
        }
        if user.two_factor_secret.is_none() {
            return Err(AuthError::TwoFactorNotEnabled);
        }
        self.check_two_factor_code(&user, code, "code", meta, "two_factor_confirmation")
            .await?;
        let (codes, stored) = self.new_recovery_codes()?;
        let now = self.clock.now();
        let audit = self.two_factor_entry("auth.two_factor_confirmed", &user, meta);
        self.write(Some(user.id), |uow| async move {
            user::update(&uow, user.id, |user| {
                user.two_factor_recovery_codes = Some(stored);
                user.two_factor_confirmed_at = Some(now);
                user.updated_at = now;
            })
            .await?;
            self.audit_after_commit(&uow, audit).await;
            Ok(())
        })
        .await?;
        Ok(codes)
    }

    /// Turns two-factor authentication off for `user_id` (`POST
    /// /auth/2fa/disable`) after checking their password (a field error on
    /// `password` when wrong, counted by the login throttle). Removes the
    /// secret and the recovery codes, also of an unconfirmed setup.
    /// Audited as `auth.two_factor_disabled`.
    pub async fn disable_two_factor(
        &self,
        user_id: Uuid,
        password: &str,
        meta: &ClientMeta,
    ) -> Result<(), AuthError> {
        let user = self.user_or_not_found(user_id).await?;
        self.confirm_password(&user, password, "password", meta)
            .await?;
        if user.two_factor_secret.is_none() && !two_factor_enabled(&user) {
            return Ok(());
        }
        let now = self.clock.now();
        let audit = self.two_factor_entry("auth.two_factor_disabled", &user, meta);
        self.write(Some(user.id), |uow| async move {
            user::update(&uow, user.id, |user| {
                user.two_factor_secret = None;
                user.two_factor_recovery_codes = None;
                user.two_factor_confirmed_at = None;
                user.updated_at = now;
            })
            .await?;
            self.audit_after_commit(&uow, audit).await;
            Ok(())
        })
        .await
    }

    /// Replaces the recovery codes of `user_id` (`POST
    /// /auth/2fa/recovery-codes`); the old ones stop working.
    /// [`AuthError::TwoFactorNotEnabled`] unless two-factor authentication
    /// is on. Audited as `auth.two_factor_recovery_codes_regenerated`.
    pub async fn regenerate_recovery_codes(
        &self,
        user_id: Uuid,
        meta: &ClientMeta,
    ) -> Result<Vec<String>, AuthError> {
        let user = self.user_or_not_found(user_id).await?;
        if !two_factor_enabled(&user) {
            return Err(AuthError::TwoFactorNotEnabled);
        }
        let (codes, stored) = self.new_recovery_codes()?;
        let now = self.clock.now();
        let audit =
            self.two_factor_entry("auth.two_factor_recovery_codes_regenerated", &user, meta);
        self.write(Some(user.id), |uow| async move {
            user::update(&uow, user.id, |user| {
                user.two_factor_recovery_codes = Some(stored);
                user.updated_at = now;
            })
            .await?;
            self.audit_after_commit(&uow, audit).await;
            Ok(())
        })
        .await?;
        Ok(codes)
    }

    /// Finishes a two-factor login (`POST /auth/2fa/verify`): checks the
    /// challenge token from [`attempt_login`](AuthService::attempt_login)
    /// ([`AuthError::InvalidToken`] / [`AuthError::TokenExpired`]; a
    /// challenge is accepted once), asks the [`SignInPolicy`](crate::SignInPolicy)s
    /// again about the method of the first step (a domain may have come to
    /// enforce single sign-on since), then checks `proof` and issues a
    /// token pair.
    /// A wrong or reused code is [`AuthError::InvalidTwoFactorCode`]
    /// (audited as `auth.two_factor_failed`); after
    /// `auth.two_factor.max_attempts_per_minute` of them the user's
    /// challenges get [`AuthError::TooManyAttempts`]. Audited as
    /// `auth.login` with `metadata.two_factor` (`totp` or
    /// `recovery_code`), plus `auth.two_factor_recovery_code_used`.
    pub async fn verify_two_factor(
        &self,
        challenge_token: &str,
        proof: TwoFactorProof,
        meta: &ClientMeta,
    ) -> Result<(User, TokenPair), AuthError> {
        let claims = self
            .jwt
            .verify(challenge_token, TokenType::TwoFactorChallenge)?;
        let user = self
            .find_user(claims.sub)
            .await?
            .ok_or(AuthError::InvalidToken)?;
        let method = claims.sign_in.unwrap_or(SignInMethod::Password);
        self.check_sign_in(&SignInAttempt::of(&user, method), meta)
            .await?;
        let user = self
            .verify_challenge(challenge_token, TokenType::TwoFactorChallenge, &proof, meta)
            .await?;
        let tokens = self.issue_tokens(&user, meta.clone()).await?;
        self.audit(
            entry(AuditKind::Auth, "auth.login", meta)
                .actor(user.id)
                .metadata(json!({ "two_factor": proof.method() })),
        )
        .await;
        Ok((user, tokens))
    }

    // -- internals ----------------------------------------------------------

    /// Checks a challenge token of type `typ` (accepted once) and `proof`
    /// for its user; see [`verify_two_factor`](AuthService::verify_two_factor).
    /// Audits a used recovery code.
    pub(crate) async fn verify_challenge(
        &self,
        challenge_token: &str,
        typ: TokenType,
        proof: &TwoFactorProof,
        meta: &ClientMeta,
    ) -> Result<User, AuthError> {
        let claims = self.jwt.verify(challenge_token, typ)?;
        let key = throttle_key(claims.sub);
        // Counted before the code is checked, so parallel guesses cannot
        // get past the limit (all that guards the 6-digit code space).
        self.throttle_attempt(
            &self.two_factor_throttle,
            &key,
            meta,
            Some(claims.sub),
            json!({ "flow": "two_factor" }),
        )
        .await?;
        let user = self
            .find_user(claims.sub)
            .await?
            .filter(two_factor_enabled)
            .ok_or(AuthError::InvalidToken)?;
        let used = format!("navree-auth:2fa:challenge:{}", claims.jti);
        if self.cache.has(&used).await? {
            return Err(AuthError::InvalidToken);
        }
        let valid = match proof {
            TwoFactorProof::Code(code) => self.check_totp(&user, code).await?,
            TwoFactorProof::RecoveryCode(code) => self.use_recovery_code(&user, code).await?,
        };
        if !valid {
            tracing::warn!(
                target: "navree::security",
                user_id = %user.id,
                ip = meta.ip.as_deref().unwrap_or("-"),
                method = proof.method(),
                "invalid two-factor code"
            );
            self.audit(
                entry(AuditKind::Security, "auth.two_factor_failed", meta)
                    .actor(user.id)
                    .metadata(json!({ "method": proof.method() })),
            )
            .await;
            return Err(AuthError::InvalidTwoFactorCode);
        }
        // The challenge lives at most its lifetime plus the JWT leeway.
        let lifetime = Duration::from_secs(
            self.options
                .two_factor
                .challenge_ttl_secs
                .saturating_add(LEEWAY_SECS.unsigned_abs()),
        );
        if !self.cache.add(&used, &true, lifetime).await? {
            return Err(AuthError::InvalidToken);
        }
        if let Err(error) = self.two_factor_throttle.clear(&key).await {
            tracing::warn!(target: "navree::auth", %error, "could not reset the two-factor throttle");
        }
        if matches!(proof, TwoFactorProof::RecoveryCode(_)) {
            self.audit(
                entry(
                    AuditKind::Security,
                    "auth.two_factor_recovery_code_used",
                    meta,
                )
                .actor(user.id),
            )
            .await;
        }
        Ok(user)
    }

    /// Checks a TOTP `code` of `user` against the two-factor throttle
    /// (counted before the check, a correct code gives the slot back): a
    /// field error on `field` when wrong, audited as
    /// `auth.two_factor_failed` with `flow`.
    pub(crate) async fn check_two_factor_code(
        &self,
        user: &User,
        code: &str,
        field: &'static str,
        meta: &ClientMeta,
        flow: &str,
    ) -> Result<(), AuthError> {
        let key = throttle_key(user.id);
        // Counted before the code is checked (parallel guesses included);
        // a correct code gives the slot back.
        self.throttle_attempt(
            &self.two_factor_throttle,
            &key,
            meta,
            Some(user.id),
            json!({ "flow": flow }),
        )
        .await?;
        if !self.check_totp(user, code).await? {
            if flow != "two_factor_confirmation" {
                self.audit(
                    entry(AuditKind::Security, "auth.two_factor_failed", meta)
                        .actor(user.id)
                        .metadata(json!({ "method": "totp", "flow": flow })),
                )
                .await;
            }
            return Err(AuthError::Invalid {
                field,
                message: format!("{field} is invalid"),
            });
        }
        if let Err(error) = self.two_factor_throttle.release(&key).await {
            tracing::warn!(target: "navree::auth", %error, "could not release a two-factor attempt");
        }
        Ok(())
    }

    /// The challenge token of a two-factor login for `user` signing in
    /// with `method`.
    pub(crate) fn sign_challenge(
        &self,
        user: &User,
        method: &SignInMethod,
    ) -> Result<String, AuthError> {
        self.jwt.sign_challenge(
            user.id,
            TokenType::TwoFactorChallenge,
            Duration::from_secs(self.options.two_factor.challenge_ttl_secs),
            method,
        )
    }

    async fn user_or_not_found(&self, user_id: Uuid) -> Result<User, AuthError> {
        self.find_user(user_id)
            .await?
            .ok_or(AuthError::UserNotFound)
    }

    /// The TOTP of `user` with `secret`.
    fn totp(&self, user: &User, secret: Secret) -> Result<Totp, AuthError> {
        Totp::builder(secret)
            .issuer(self.options.two_factor.issuer.as_str())
            .account(user.email.replace(':', ""))
            .build()
            .map_err(|error| AuthError::TwoFactor(error.to_string()))
    }

    /// Whether `code` is a current TOTP code of `user` newer than the last
    /// one accepted; records it as the last one.
    async fn check_totp(&self, user: &User, code: &str) -> Result<bool, AuthError> {
        let Some(stored) = &user.two_factor_secret else {
            return Ok(false);
        };
        let encoded = Zeroizing::new(self.encrypter.decrypt_string(stored)?);
        let secret = Secret::from_base32(&encoded)
            .map_err(|error| AuthError::TwoFactor(error.to_string()))?;
        let totp = self.totp(user, secret)?;
        let code: String = code.chars().filter(|c| !c.is_whitespace()).collect();
        let now = u64::try_from(self.clock.now().timestamp()).unwrap_or(0);
        let Some(step) = totp.check(&code, now) else {
            return Ok(false);
        };
        let last_key = format!("navree-auth:2fa:last-step:{}", user.id);
        if self
            .cache
            .get::<u64>(&last_key)
            .await?
            .is_some_and(|last| step <= last)
        {
            return Ok(false);
        }
        // Two requests racing with the same code: only one claims the step.
        let step_key = format!("navree-auth:2fa:step:{}:{step}", user.id);
        if !self.cache.add(&step_key, &true, LAST_STEP_TTL).await? {
            return Ok(false);
        }
        self.cache.put(&last_key, &step, LAST_STEP_TTL).await?;
        Ok(true)
    }

    /// Removes `code` from `user`'s recovery codes; `false` if it is not
    /// one of them (or a concurrent request used it first).
    async fn use_recovery_code(&self, user: &User, code: &str) -> Result<bool, AuthError> {
        let Some(stored) = user.two_factor_recovery_codes.clone() else {
            return Ok(false);
        };
        let digests: Vec<String> = self.encrypter.decrypt_json(&stored)?;
        let presented = recovery_digest(code);
        let Some(index) = digests
            .iter()
            .position(|digest| constant_time_eq(digest.as_bytes(), presented.as_bytes()))
        else {
            return Ok(false);
        };
        let mut rest = digests;
        rest.remove(index);
        let remaining = self.encrypter.encrypt_json(&rest)?;
        let now = self.clock.now();
        let user_id = user.id;
        self.write(Some(user_id), |uow: Uow| async move {
            // Only if nobody changed the codes meanwhile: each code works once.
            let claimed = User::query()
                .where_key(user_id)
                .where_(User::TWO_FACTOR_RECOVERY_CODES.eq(stored))
                .update(
                    &uow,
                    [
                        User::TWO_FACTOR_RECOVERY_CODES.set(remaining),
                        User::UPDATED_AT.set(now),
                    ],
                )
                .await?;
            Ok(claimed == 1)
        })
        .await
    }

    /// New recovery codes and their encrypted digests.
    fn new_recovery_codes(&self) -> Result<(Vec<String>, String), AuthError> {
        let codes: Vec<String> = (0..self.options.two_factor.recovery_codes)
            .map(|_| {
                format!(
                    "{}-{}",
                    random_token(RECOVERY_HALF_LEN).to_lowercase(),
                    random_token(RECOVERY_HALF_LEN).to_lowercase()
                )
            })
            .collect();
        let digests: Vec<String> = codes.iter().map(|code| recovery_digest(code)).collect();
        let stored = self.encrypter.encrypt_json(&digests)?;
        Ok((codes, stored))
    }

    fn two_factor_entry(&self, action: &str, user: &User, meta: &ClientMeta) -> AuditEntry {
        entry(AuditKind::Auth, action, meta)
            .actor(user.id)
            .subject(crate::USER_SUBJECT, user.id.to_string())
    }
}

/// The throttle key of a user's two-factor codes.
fn throttle_key(user_id: Uuid) -> String {
    format!("navree-auth:2fa:{user_id}")
}

/// SHA-256 of a recovery code, trimmed and lower-cased.
fn recovery_digest(code: &str) -> String {
    sha256_hex(code.trim().to_lowercase().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_digests_ignore_case_and_surrounding_space() {
        assert_eq!(
            recovery_digest(" ABCDE12345-fghij67890 "),
            recovery_digest("abcde12345-fghij67890")
        );
        assert_ne!(
            recovery_digest("abcde12345-fghij67890"),
            recovery_digest("abcde12345-fghij67891")
        );
    }

    #[test]
    fn proofs_never_show_their_codes() {
        let debug = format!(
            "{:?}",
            TwoFactorProof::RecoveryCode("secret-code".to_owned())
        );
        assert!(!debug.contains("secret-code"), "{debug}");
        assert_eq!(TwoFactorProof::Code("1".to_owned()).method(), "totp");
    }
}
