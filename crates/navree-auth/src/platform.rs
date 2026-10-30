//! The platform audience on [`AuthService`]: platform administrators
//! (`auth.platform.roles`) sign in apart from the app, always with
//! two-factor authentication, into sessions whose access tokens are of type
//! [`TokenType::Platform`](crate::TokenType::Platform).
//!
//! - `POST /auth/platform/login` `{email, password}` checks the password
//!   and the role under its own throttle (`auth.platform.login_throttle_per_minute`)
//!   and answers a challenge (`{two_factor_required, challenge_token,
//!   expires_in}`); an administrator without two-factor authentication is
//!   refused with 403 `two_factor_required`.
//! - `POST /auth/platform/2fa/verify` finishes it with a code or a
//!   recovery code: a platform session (`auth.platform.session_ttl_secs`,
//!   not extended by refreshing; each refresh checks the role again).
//! - Both steps ask the [`SignInPolicy`](crate::SignInPolicy)s
//!   ([`SignInAudience::Platform`]). With `auth.platform.break_glass` (the
//!   default) a refusal doesn't stop an administrator whose password, role
//!   and second factor check out: the sign-in goes on and is audited as
//!   `auth.break_glass`. Without it, the refusal is the answer.
//! - Platform consoles guard their routes with
//!   [`platform_session`](crate::platform_session) or take a
//!   [`PlatformAdmin`].

use std::time::Duration;

use http::request::Parts;
use navree_contracts::AuditKind;
use navree_http::AppError;
use navree_http::extract::FromRequestParts;
use navree_kernel::AppContext;
use serde_json::json;
use uuid::Uuid;

use crate::service::{SessionKind, entry, normalize_email};
use crate::two_factor::two_factor_enabled;
use crate::{
    AccessClaims, AuthError, AuthService, ClientMeta, SignInAttempt, SignInAudience, SignInMethod,
    TokenPair, TokenType, TwoFactorProof, User,
};

impl AuthService {
    /// Whether `user_id` is a platform administrator: the platform audience
    /// is enabled and they hold one of `auth.platform.roles`
    /// (navree-access).
    pub async fn is_platform_admin(&self, user_id: Uuid) -> Result<bool, AuthError> {
        let platform = &self.options.platform;
        if !platform.enabled {
            return Ok(false);
        }
        let roles = self.roles_of(user_id).await?;
        Ok(roles.iter().any(|role| platform.roles.contains(role)))
    }

    /// The password step of a platform sign-in (`POST
    /// /auth/platform/login`): the platform throttle (per email and IP),
    /// the credentials and the role, then a challenge token of type
    /// [`TokenType::PlatformChallenge`] for
    /// [`verify_platform_two_factor`](AuthService::verify_platform_two_factor).
    /// Wrong credentials and non-administrators alike are
    /// [`AuthError::InvalidCredentials`] (audited as `auth.login_failed`
    /// with `flow: platform`); an administrator without two-factor
    /// authentication is [`AuthError::TwoFactorSetupRequired`].
    pub async fn platform_login(
        &self,
        email: &str,
        password: &str,
        meta: &ClientMeta,
    ) -> Result<String, AuthError> {
        if !self.options.platform.enabled {
            return Err(AuthError::Disabled("platform sign-in"));
        }
        // Apart from the app's login throttle.
        let key = format!(
            "{}:platform",
            self.platform_throttle.key(email, meta.ip.as_deref())
        );
        self.throttle_attempt(
            &self.platform_throttle,
            &key,
            meta,
            None,
            json!({ "flow": "platform_login", "email": normalize_email(email) }),
        )
        .await?;
        let user = match self.check_credentials(email, password).await? {
            Ok(user) if self.is_platform_admin(user.id).await? => user,
            outcome => {
                let known = match outcome {
                    Ok(user) => Some(user.id),
                    Err(known) => known,
                };
                tracing::warn!(
                    target: "navree::security",
                    ip = meta.ip.as_deref().unwrap_or("-"),
                    known_user = known.is_some(),
                    "failed platform sign-in"
                );
                let mut failed = entry(AuditKind::Security, "auth.login_failed", meta)
                    .metadata(json!({ "flow": "platform", "email": normalize_email(email) }));
                if let Some(user) = known {
                    failed = failed.actor(user);
                }
                self.audit(failed).await;
                return Err(AuthError::InvalidCredentials);
            }
        };
        if let Err(error) = self.platform_throttle.clear(&key).await {
            tracing::warn!(target: "navree::auth", %error, "could not reset the platform throttle");
        }
        self.platform_sign_in_policy(&user, meta, false).await?;
        if !two_factor_enabled(&user) {
            self.audit(
                entry(AuditKind::Security, "auth.login_failed", meta)
                    .actor(user.id)
                    .metadata(json!({ "flow": "platform", "reason": "two_factor_not_enabled" })),
            )
            .await;
            return Err(AuthError::TwoFactorSetupRequired);
        }
        let challenge = self.jwt.sign_challenge(
            user.id,
            TokenType::PlatformChallenge,
            Duration::from_secs(self.options.two_factor.challenge_ttl_secs),
            &SignInMethod::Password,
        )?;
        self.audit(
            entry(AuditKind::Auth, "auth.two_factor_challenge", meta)
                .actor(user.id)
                .metadata(json!({ "flow": "platform" })),
        )
        .await;
        Ok(challenge)
    }

    /// Finishes a platform sign-in (`POST /auth/platform/2fa/verify`): the
    /// challenge of [`platform_login`](AuthService::platform_login) and a
    /// code or recovery code, like
    /// [`verify_two_factor`](AuthService::verify_two_factor), then a
    /// platform session. Audited as `auth.platform_login`.
    pub async fn verify_platform_two_factor(
        &self,
        challenge_token: &str,
        proof: TwoFactorProof,
        meta: &ClientMeta,
    ) -> Result<(User, TokenPair), AuthError> {
        if !self.options.platform.enabled {
            return Err(AuthError::Disabled("platform sign-in"));
        }
        let user = self
            .verify_challenge(challenge_token, TokenType::PlatformChallenge, &proof, meta)
            .await?;
        if !self.is_platform_admin(user.id).await? {
            return Err(AuthError::InvalidToken);
        }
        self.platform_sign_in_policy(&user, meta, true).await?;
        let (tokens, session_id) = self
            .open_session(&user, meta.clone(), SessionKind::Platform)
            .await?;
        self.audit(
            entry(AuditKind::Auth, "auth.platform_login", meta)
                .actor(user.id)
                .metadata(json!({ "two_factor": proof.method(), "session_id": session_id })),
        )
        .await;
        Ok((user, tokens))
    }
}

impl AuthService {
    /// Asks the sign-in policies about a platform sign-in of `user`, an
    /// administrator whose credentials checked out. A refusal stands
    /// without `auth.platform.break_glass`; with it the sign-in goes on,
    /// audited as `auth.break_glass` once the session starts (`finishing`).
    async fn platform_sign_in_policy(
        &self,
        user: &User,
        meta: &ClientMeta,
        finishing: bool,
    ) -> Result<(), AuthError> {
        let attempt =
            SignInAttempt::of(user, SignInMethod::Password).audience(SignInAudience::Platform);
        let (code, message) = match self.evaluate_sign_in(&attempt).await {
            Ok(()) => return Ok(()),
            Err(AuthError::SignInRefused { code, message }) => (code, message),
            Err(error) => return Err(error),
        };
        if !self.options.platform.break_glass {
            self.audit_refusal(&attempt, &code, meta).await;
            return Err(AuthError::SignInRefused { code, message });
        }
        if finishing {
            tracing::warn!(
                target: "navree::security",
                user_id = %user.id,
                ip = meta.ip.as_deref().unwrap_or("-"),
                code = code.as_str(),
                "break-glass platform sign-in past a sign-in policy"
            );
            self.audit(
                entry(AuditKind::Security, "auth.break_glass", meta)
                    .actor(user.id)
                    .metadata(json!({
                        "flow": "platform",
                        "email": attempt.email,
                        "refused_code": code,
                    })),
            )
            .await;
        }
        Ok(())
    }
}

/// The signed-in platform administrator, as a handler argument: the request
/// must carry a platform access token (403 `platform_session_required`
/// otherwise; 401 for guests).
///
/// ```
/// use navree_auth::PlatformAdmin;
/// use navree_http::Json;
///
/// async fn console(PlatformAdmin(admin): PlatformAdmin) -> Json<String> {
///     Json(format!("Signed in to the platform as {}", admin.email))
/// }
/// # let _ = navree_http::RouteDef::on(http::Method::GET, "/console", console);
/// ```
#[derive(Clone, Debug)]
pub struct PlatformAdmin(pub User);

impl FromRequestParts for PlatformAdmin {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        let claims = parts
            .extensions
            .get::<AccessClaims>()
            .cloned()
            .ok_or_else(AppError::unauthenticated)?;
        if !claims.is_platform() {
            return Err(AuthError::PlatformSessionRequired.into_app_error());
        }
        let ctx = parts
            .extensions
            .get::<AppContext>()
            .cloned()
            .ok_or_else(|| AppError::internal("PlatformAdmin used outside the navree pipeline"))?;
        let auth = ctx.get::<AuthService>().await.map_err(AppError::internal)?;
        let user = auth
            .find_user(claims.sub)
            .await
            .map_err(AuthError::into_app_error)?
            .ok_or_else(AppError::unauthenticated)?;
        Ok(PlatformAdmin(user))
    }
}
