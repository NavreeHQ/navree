//! Browser sign-in on navree-session's session (feature `session`): the
//! [`SessionStrategy`] and the session flows of [`AuthService`].
//!
//! A browser signs in through the same steps as a token client (the
//! [`SignInPolicy`](crate::SignInPolicy)s, the login throttle, the password,
//! the second factor and its throttle, the audit trail), but what it gets
//! is a server-side session (a refresh-token family no client ever holds a
//! token of) bound to its navree-session session, never a token:
//!
//! - signing in regenerates the session (a new id and a new CSRF token,
//!   against session fixation) and records the user and that server-side
//!   session in it;
//! - every request checks on the server that the server-side session is
//!   still live, so signing out, ending the session from another device
//!   (`DELETE /auth/sessions/{id}`), changing or resetting the password and
//!   offboarding end it at once, for a copied cookie too;
//! - "remember me" keeps the session for `auth.session.remember_days`
//!   instead of `session.lifetime_minutes` idle, and every sign-in ends
//!   `auth.session.lifetime_hours` (or `remember_days`) after it started.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use http::Method;
use http::request::Parts;
use navree_contracts::{AuditKind, AuthVia, BoxFuture, Impersonator, Principal};
use navree_http::AppError;
use navree_orm::Model as _;
use navree_session::Session;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::refresh_token::Model as RefreshToken;
use crate::service::{PasswordStep, SessionKind, entry};
use crate::two_factor::two_factor_enabled;
use crate::{
    AccessClaims, ActorClaim, AuthError, AuthService, Claims, ClientMeta, ConfirmProof,
    SignInAttempt, SignInMethod, Strategy, TokenType, TwoFactorProof, User,
};

/// The session key holding the signed-in user of a browser.
pub const SESSION_USER_KEY: &str = "_auth";

/// The session key holding a sign-in that waits for its second factor.
pub const SESSION_CHALLENGE_KEY: &str = "_auth_challenge";

/// The session key holding the sign-in an impersonating administrator goes
/// back to.
const SESSION_STASH_KEY: &str = "_auth_stash";

/// Where browsers sign out (the `POST` route of the sign-out page):
/// a user who must set up two-factor authentication can still reach it.
pub const SIGN_OUT_PATH: &str = "/sign-out";

/// Where a user who must set up two-factor authentication does it (the
/// page of [`AuthPages`](crate::AuthPages); `GET` shows the secret, `POST`
/// confirms the first code).
pub const TWO_FACTOR_SETUP_PATH: &str = "/account/two-factor";

/// Where the setup page posts the first code (see [`TWO_FACTOR_SETUP_PATH`]).
pub(crate) const TWO_FACTOR_CONFIRM_PATH: &str = "/account/two-factor/confirm";

/// Where an impersonating administrator goes back to their own sign-in (the
/// `POST` route of [`AuthPages`](crate::AuthPages)).
pub const STOP_IMPERSONATING_PATH: &str = "/impersonate/stop";

/// What the session holds for a signed-in browser.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct SignedIn {
    /// The user.
    user: Uuid,
    /// The server-side session (a refresh-token family): live while the
    /// sign-in is.
    sid: Uuid,
    /// When the user last authenticated (Unix seconds): the sign-in, or a
    /// confirmation (`#[recent_auth]` routes read it).
    at: i64,
    /// The user must set up two-factor authentication first: the session
    /// reaches only the setup routes and signing out.
    #[serde(default)]
    setup: bool,
    /// "Remember me".
    #[serde(default)]
    remember: bool,
    /// The administrator acting as `user` (an impersonation); their own
    /// sign-in waits under `SESSION_STASH_KEY`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    impersonator: Option<Uuid>,
}

/// What the session holds while a sign-in waits for its second factor.
#[derive(Clone, Serialize, Deserialize)]
struct Pending {
    /// The challenge token of the first step (never sent to the browser).
    token: String,
    /// "Remember me", as ticked at the first step.
    remember: bool,
}

/// The result of [`AuthService::sign_in_session`].
#[derive(Debug)]
pub enum SessionSignIn {
    /// The browser is signed in as the user.
    SignedIn(User),
    /// The password was right and the account uses two-factor
    /// authentication: the session holds the challenge (never the browser),
    /// and [`AuthService::verify_two_factor_session`] finishes the sign-in.
    TwoFactorRequired,
}

/// The signed-in user of a browser's session, as
/// [`AuthService::session_user`] reads it (without checking it on the
/// server; the [`SessionStrategy`] does that on every request).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionUser {
    /// The user.
    pub user_id: Uuid,
    /// The server-side session (`GET /auth/sessions` lists it with this id).
    pub session_id: Uuid,
    /// When the user last authenticated (Unix seconds).
    pub auth_time: i64,
    /// Whether the browser was signed in with "remember me".
    pub remember: bool,
    /// The administrator acting as the user, when this is an impersonation
    /// ([`AuthService::impersonate_session`]).
    pub impersonator: Option<Uuid>,
}

/// The routes a user who must set up two-factor authentication reaches
/// through the pages (method and path): the setup page and its post, the
/// way to stop impersonating and signing out.
pub(crate) fn page_setup_routes() -> Vec<(Method, String)> {
    vec![
        (Method::GET, TWO_FACTOR_SETUP_PATH.to_owned()),
        (Method::POST, TWO_FACTOR_SETUP_PATH.to_owned()),
        (Method::POST, TWO_FACTOR_CONFIRM_PATH.to_owned()),
        (Method::POST, SIGN_OUT_PATH.to_owned()),
    ]
}

impl AuthService {
    /// Signs a browser in with a password (the sign-in page): the same first
    /// step as [`attempt_login`](AuthService::attempt_login) (sign-in
    /// policies, the per email and IP throttle, the credentials, all
    /// audited), then, without two-factor authentication,
    /// [`start_session`](AuthService::start_session) and an `auth.login`
    /// audit entry with `metadata.via = "session"`. With it, the session
    /// keeps the challenge (regenerated first) and the answer is
    /// [`SessionSignIn::TwoFactorRequired`]; nothing signs the browser in
    /// before [`verify_two_factor_session`](AuthService::verify_two_factor_session).
    ///
    /// `remember` keeps the browser signed in for `auth.session.remember_days`.
    pub async fn sign_in_session(
        &self,
        session: &Session,
        email: &str,
        password: &str,
        remember: bool,
        meta: &ClientMeta,
    ) -> Result<SessionSignIn, AuthError> {
        let step = self.password_step(email, password, meta).await?;
        self.session_step(session, step, None, remember, meta).await
    }

    /// Signs a browser in with a magic link token (the page the link in the
    /// mail opens): the same steps as
    /// [`login_with_magic_link`](AuthService::login_with_magic_link) (the
    /// token works once; the sign-in policies and the audit trail apply; an
    /// unverified address becomes verified), then a session as
    /// [`sign_in_session`](AuthService::sign_in_session) starts it, or the
    /// waiting second factor.
    pub async fn login_with_magic_link_session(
        &self,
        session: &Session,
        token: &str,
        remember: bool,
        meta: &ClientMeta,
    ) -> Result<SessionSignIn, AuthError> {
        let (user, now) = self.claim_magic_link(token, meta).await?;
        let step = self
            .passwordless_step(user, SignInMethod::MagicLink, meta, now)
            .await?;
        self.session_step(
            session,
            step,
            Some(SignInMethod::MagicLink.name()),
            remember,
            meta,
        )
        .await
    }

    /// Signs a browser in with an emailed code: the same steps as
    /// [`login_with_code`](AuthService::login_with_code) (counted by the
    /// login throttle, burned after `auth.otp.max_attempts` wrong guesses),
    /// then a session, or the waiting second factor.
    pub async fn login_with_code_session(
        &self,
        session: &Session,
        email: &str,
        code: &str,
        remember: bool,
        meta: &ClientMeta,
    ) -> Result<SessionSignIn, AuthError> {
        let (user, now) = self.claim_login_code(email, code, meta).await?;
        let step = self
            .passwordless_step(user, SignInMethod::Otp, meta, now)
            .await?;
        self.session_step(
            session,
            step,
            Some(SignInMethod::Otp.name()),
            remember,
            meta,
        )
        .await
    }

    /// Signs a browser in with the passkey assertion in `credential` (the
    /// browser's `PublicKeyCredential` JSON): the same steps as
    /// [`login_with_passkey`](AuthService::login_with_passkey), then a
    /// session, or the waiting second factor for an account whose passkey
    /// did not verify the user.
    pub async fn login_with_passkey_session(
        &self,
        session: &Session,
        credential: &serde_json::Value,
        remember: bool,
        meta: &ClientMeta,
    ) -> Result<SessionSignIn, AuthError> {
        let (step, _verified) = self.passkey_step(credential, meta).await?;
        self.session_step(
            session,
            step,
            Some(SignInMethod::Passkey.name()),
            remember,
            meta,
        )
        .await
    }

    /// What follows the first step of any browser sign-in: a session and an
    /// `auth.login` entry (with `metadata.via = "session"` and the sign-in
    /// `method` when it was not a password), or the second factor waiting in
    /// the session.
    async fn session_step(
        &self,
        session: &Session,
        step: PasswordStep,
        method: Option<&'static str>,
        remember: bool,
        meta: &ClientMeta,
    ) -> Result<SessionSignIn, AuthError> {
        match step {
            PasswordStep::Done(user) => {
                self.start_session(session, &user, remember, meta).await?;
                let mut metadata = json!({ "via": "session", "remember": remember });
                if let Some(method) = method {
                    metadata["method"] = json!(method);
                }
                self.audit(
                    entry(AuditKind::Auth, "auth.login", meta)
                        .actor(user.id)
                        .metadata(metadata),
                )
                .await;
                Ok(SessionSignIn::SignedIn(user))
            }
            PasswordStep::TwoFactor {
                challenge_token, ..
            } => {
                session.regenerate();
                session
                    .insert(
                        SESSION_CHALLENGE_KEY,
                        &Pending {
                            token: challenge_token,
                            remember,
                        },
                    )
                    .map_err(session_error)?;
                Ok(SessionSignIn::TwoFactorRequired)
            }
        }
    }

    /// Whether the session holds a sign-in waiting for its second factor
    /// (the two-factor page shows its form only then).
    pub fn two_factor_pending(&self, session: &Session) -> bool {
        session.get::<Pending>(SESSION_CHALLENGE_KEY).is_some()
    }

    /// Whether the browser is signed in but must set up two-factor
    /// authentication before it reaches anything else (the sign-in pages
    /// send it to [`TWO_FACTOR_SETUP_PATH`]). It stops being true once the
    /// first code is confirmed.
    pub fn two_factor_setup_required(&self, session: &Session) -> bool {
        session
            .get::<SignedIn>(SESSION_USER_KEY)
            .is_some_and(|signed_in| signed_in.setup)
    }

    /// Finishes a browser's two-factor sign-in with `proof`: the same
    /// second step as [`verify_two_factor`](AuthService::verify_two_factor)
    /// (the challenge accepted once, the sign-in policies asked again, the
    /// per user throttle, a wrong code audited as `auth.two_factor_failed`),
    /// then [`start_session`](AuthService::start_session) and an
    /// `auth.login` entry with `metadata.via = "session"` and
    /// `metadata.two_factor`. [`AuthError::InvalidToken`] when no sign-in
    /// waits (or its challenge expired: the session forgets it); a wrong
    /// code leaves it waiting.
    pub async fn verify_two_factor_session(
        &self,
        session: &Session,
        proof: TwoFactorProof,
        meta: &ClientMeta,
    ) -> Result<User, AuthError> {
        let pending = session
            .get::<Pending>(SESSION_CHALLENGE_KEY)
            .ok_or(AuthError::InvalidToken)?;
        let user = match self.two_factor_step(&pending.token, &proof, meta).await {
            Ok(user) => user,
            Err(error) => {
                if matches!(error, AuthError::InvalidToken | AuthError::TokenExpired) {
                    session.remove(SESSION_CHALLENGE_KEY);
                }
                return Err(error);
            }
        };
        self.start_session(session, &user, pending.remember, meta)
            .await?;
        self.audit(
            entry(AuditKind::Auth, "auth.login", meta)
                .actor(user.id)
                .metadata(json!({
                    "via": "session",
                    "remember": pending.remember,
                    "two_factor": proof.method(),
                })),
        )
        .await;
        Ok(user)
    }

    /// Signs `user` in with `method` on the browser's session: the
    /// [`SignInPolicy`](crate::SignInPolicy)s
    /// ([`check_sign_in`](AuthService::check_sign_in)), then
    /// [`start_session`](AuthService::start_session). The session version of
    /// [`sign_in`](AuthService::sign_in), for sign-in flows of other crates
    /// and of the app (a magic link, social sign-in, single sign-on) that
    /// checked the person themselves. It does not ask for a second factor:
    /// a flow that should, hands out
    /// [`two_factor_challenge`](AuthService::two_factor_challenge) instead.
    pub async fn sign_in_to_session(
        &self,
        session: &Session,
        user: &User,
        method: SignInMethod,
        remember: bool,
        meta: &ClientMeta,
    ) -> Result<(), AuthError> {
        self.check_sign_in(&SignInAttempt::of(user, method.clone()), meta)
            .await?;
        self.start_session(session, user, remember, meta).await?;
        crate::telemetry::report_signed_in(user, &method);
        Ok(())
    }

    /// Leaves a sign-in of another flow (a social provider, single sign-on)
    /// waiting for its second factor in the browser's session: the
    /// [`SignInPolicy`](crate::SignInPolicy)s are asked about `method`, the
    /// session gets a new id and keeps the challenge (never the browser),
    /// and [`verify_two_factor_session`](AuthService::verify_two_factor_session)
    /// (`POST /auth/session/2fa/verify`) finishes the sign-in. The session
    /// version of handing out
    /// [`two_factor_challenge`](AuthService::two_factor_challenge).
    pub async fn await_two_factor_session(
        &self,
        session: &Session,
        user: &User,
        method: SignInMethod,
        remember: bool,
        meta: &ClientMeta,
    ) -> Result<(), AuthError> {
        self.check_sign_in(&SignInAttempt::of(user, method.clone()), meta)
            .await?;
        let token = self.two_factor_challenge(user, &method)?;
        session.regenerate();
        session
            .insert(SESSION_CHALLENGE_KEY, &Pending { token, remember })
            .map_err(session_error)?;
        Ok(())
    }

    /// Binds the browser's session to `user`, whose sign-in was already
    /// checked: starts a server-side session (listed by
    /// [`sessions`](AuthService::sessions), ended by every revocation),
    /// gives the session a new id and a new CSRF token (against session
    /// fixation), forgets any waiting challenge, and records the user, the
    /// server-side session and the time. `remember` makes the session live
    /// `auth.session.remember_days` without a request (else
    /// `session.lifetime_minutes`); the sign-in ends `remember_days`, or
    /// `auth.session.lifetime_hours`, after it started. A user who must
    /// set up two-factor authentication reaches only the setup routes until
    /// they do. A sign-in the browser held before (another account, say)
    /// ends on the server. It asks no [`SignInPolicy`](crate::SignInPolicy)
    /// and writes no `auth.login` entry: the caller's flow does both.
    pub async fn start_session(
        &self,
        session: &Session,
        user: &User,
        remember: bool,
        meta: &ClientMeta,
    ) -> Result<(), AuthError> {
        let previous = session.get::<SignedIn>(SESSION_USER_KEY);
        let stashed = session.get::<SignedIn>(SESSION_STASH_KEY);
        let max_age_secs = self.options.session.max_age_secs(remember);
        let (state, _secret) = self
            .open_family(user, meta, SessionKind::Browser { max_age_secs })
            .await?;
        let (typ, _) = self.session_token(user.id, Some(user), &state).await?;
        self.bind(
            session,
            SignedIn {
                user: user.id,
                sid: state.family_id,
                at: self.clock.now().timestamp(),
                setup: typ == TokenType::TwoFactorSetup,
                remember,
                impersonator: None,
            },
        )?;
        if let Some(previous) = previous {
            self.end_server_session(previous.user, previous.sid).await?;
        }
        if let Some(stashed) = stashed {
            self.end_server_session(stashed.user, stashed.sid).await?;
        }
        Ok(())
    }

    /// Signs the browser out: ends its server-side session (audited as
    /// `auth.logout` with `metadata.via = "session"`; the user's real-time
    /// subscriptions end too) and invalidates the session (a new id and
    /// CSRF token, every value gone, the old record deleted). A copy of the
    /// old cookie signs nobody in afterwards. Safe to call for a guest.
    pub async fn sign_out_session(
        &self,
        session: &Session,
        meta: &ClientMeta,
    ) -> Result<(), AuthError> {
        let signed_in = session.get::<SignedIn>(SESSION_USER_KEY);
        let stashed = session.get::<SignedIn>(SESSION_STASH_KEY);
        session.invalidate();
        let Some(signed_in) = signed_in else {
            return Ok(());
        };
        let (user_id, sid) = (signed_in.user, signed_in.sid);
        let revoked = self.end_server_session(user_id, sid).await?;
        // Signing out while impersonating ends the administrator's own
        // sign-in too.
        if let Some(stashed) = stashed {
            self.end_server_session(stashed.user, stashed.sid).await?;
        }
        self.audit(
            entry(AuditKind::Auth, "auth.logout", meta)
                .actor(user_id)
                .metadata(json!({ "via": "session", "all": false, "revoked": revoked })),
        )
        .await;
        crate::disconnect::end_user_subscriptions(&self.app, user_id).await;
        Ok(())
    }

    /// Changes the password of the browser's user, as
    /// [`change_password`](AuthService::change_password) does (the current
    /// password checked and counted by the login throttle, the policy, every
    /// other session revoked, a pending reset cancelled, audited as
    /// `auth.password_changed`), and keeps this browser signed in on a new
    /// server-side session with a new session id. [`AuthError::InvalidToken`]
    /// when the browser is not signed in.
    pub async fn change_password_session(
        &self,
        session: &Session,
        current_password: &str,
        new_password: &str,
        meta: &ClientMeta,
    ) -> Result<(), AuthError> {
        let signed_in = session
            .get::<SignedIn>(SESSION_USER_KEY)
            .ok_or(AuthError::InvalidToken)?;
        let max_age_secs = self.options.session.max_age_secs(signed_in.remember);
        let opened = meta.clone();
        let state = self
            .change_password_then(
                signed_in.user,
                current_password,
                new_password,
                meta,
                |user| async move {
                    let (state, _secret) = self
                        .open_family(&user, &opened, SessionKind::Browser { max_age_secs })
                        .await?;
                    Ok(state)
                },
            )
            .await?;
        self.bind(
            session,
            SignedIn {
                sid: state.family_id,
                at: self.clock.now().timestamp(),
                ..signed_in
            },
        )
    }

    /// Step-up confirmation for a browser (what `POST /auth/confirm` does
    /// for a session): checks `proof` as [`confirm`](AuthService::confirm)
    /// does and records the time as the sign-in's `auth_time`, which
    /// `#[recent_auth]` routes read. [`AuthError::InvalidToken`] when the
    /// browser is not signed in. The new `auth_time` (Unix seconds).
    pub async fn confirm_session(
        &self,
        session: &Session,
        proof: ConfirmProof,
        meta: &ClientMeta,
    ) -> Result<i64, AuthError> {
        let signed_in = session
            .get::<SignedIn>(SESSION_USER_KEY)
            .ok_or(AuthError::InvalidToken)?;
        let at = self
            .confirm_identity(signed_in.user, Some(signed_in.sid), false, proof, meta)
            .await?;
        session
            .insert(SESSION_USER_KEY, &SignedIn { at, ..signed_in })
            .map_err(session_error)?;
        Ok(at)
    }

    /// The signed-in user the browser's session names, if any, as stored:
    /// not checked on the server (the [`SessionStrategy`] checks it on every
    /// request, and a handler's `CurrentPrincipal` is the checked answer).
    pub fn session_user(&self, session: &Session) -> Option<SessionUser> {
        session
            .get::<SignedIn>(SESSION_USER_KEY)
            .map(|signed_in| SessionUser {
                user_id: signed_in.user,
                session_id: signed_in.sid,
                auth_time: signed_in.at,
                remember: signed_in.remember,
                impersonator: signed_in.impersonator,
            })
    }

    /// Starts impersonating `target_id` in the browser's session (`POST
    /// /auth/impersonate/{user}` for a browser): the same checks and audit
    /// entry as [`impersonate`](AuthService::impersonate) (the route checks
    /// the permission `auth.impersonation.permission`). The session becomes
    /// the target's: a new server-side session that ends after
    /// `auth.impersonation.ttl_secs` however often it is used, no
    /// "remember me", no `auth_time` (so `#[recent_auth]` routes refuse it),
    /// the request carrying navree-contracts' `Impersonator`, and the
    /// administrator's own sign-in kept aside for
    /// [`stop_impersonating_session`](AuthService::stop_impersonating_session).
    /// [`AuthError::InvalidToken`] when the browser is not signed in.
    pub async fn impersonate_session(
        &self,
        session: &Session,
        target_id: Uuid,
        reason: &str,
        meta: &ClientMeta,
    ) -> Result<User, AuthError> {
        let current = session
            .get::<SignedIn>(SESSION_USER_KEY)
            .ok_or(AuthError::InvalidToken)?;
        if current.impersonator.is_some() {
            return Err(AuthError::ImpersonationForbidden);
        }
        let (target, reason) = self
            .impersonation_target(current.user, None, target_id, reason)
            .await?;
        let (state, _secret) = self
            .open_family(
                &target,
                meta,
                SessionKind::Impersonation {
                    impersonator: current.user,
                },
            )
            .await?;
        session.regenerate();
        session.remove(SESSION_CHALLENGE_KEY);
        session.clear_lifetime();
        session
            .insert(SESSION_STASH_KEY, &current)
            .map_err(session_error)?;
        session
            .insert(
                SESSION_USER_KEY,
                &SignedIn {
                    user: target.id,
                    sid: state.family_id,
                    at: self.clock.now().timestamp(),
                    setup: false,
                    remember: false,
                    impersonator: Some(current.user),
                },
            )
            .map_err(session_error)?;
        self.impersonation_started(current.user, &target, &reason, state.family_id, meta)
            .await;
        Ok(target)
    }

    /// Ends the impersonation of the browser's session (`DELETE
    /// /auth/impersonate` for a browser): the impersonation's server-side
    /// session is revoked, the session gets a new id and CSRF token, and the
    /// administrator is signed in as themselves again, when their own
    /// sign-in is still live (else the browser is a guest).
    /// [`AuthError::NotImpersonating`] for a session that is not
    /// impersonating, [`AuthError::InvalidToken`] for a guest. Audited as
    /// `auth.impersonation_ended`.
    pub async fn stop_impersonating_session(
        &self,
        session: &Session,
        meta: &ClientMeta,
    ) -> Result<(), AuthError> {
        let current = session
            .get::<SignedIn>(SESSION_USER_KEY)
            .ok_or(AuthError::InvalidToken)?;
        let Some(impersonator) = current.impersonator else {
            return Err(AuthError::NotImpersonating);
        };
        let original = session
            .get::<SignedIn>(SESSION_STASH_KEY)
            .filter(|original| original.user == impersonator);
        self.end_server_session(current.user, current.sid).await?;
        self.impersonation_ended(impersonator, current.user, Some(current.sid), meta)
            .await;
        session.regenerate();
        session.remove(SESSION_STASH_KEY);
        session.remove(SESSION_USER_KEY);
        if let Some(original) = original
            && self
                .live_session(original.user, original.sid)
                .await?
                .is_some()
        {
            if original.remember {
                session.set_lifetime(Duration::from_secs(self.options.session.max_age_secs(true)));
            }
            session
                .insert(SESSION_USER_KEY, &original)
                .map_err(session_error)?;
        }
        Ok(())
    }

    // -- internals ----------------------------------------------------------

    /// Regenerates `session` and records `signed_in` in it, with the
    /// lifetime "remember me" asks for.
    fn bind(&self, session: &Session, signed_in: SignedIn) -> Result<(), AuthError> {
        session.regenerate();
        session.remove(SESSION_CHALLENGE_KEY);
        session.remove(SESSION_STASH_KEY);
        if signed_in.remember {
            session.set_lifetime(Duration::from_secs(self.options.session.max_age_secs(true)));
        } else {
            session.clear_lifetime();
        }
        session
            .insert(SESSION_USER_KEY, &signed_in)
            .map_err(session_error)
    }

    /// Revokes the server-side session `sid` of `user`; how many of its
    /// rows were live.
    async fn end_server_session(&self, user: Uuid, sid: Uuid) -> Result<u64, AuthError> {
        let now = self.clock.now();
        let revoked = self
            .write(Some(user), |uow| async move {
                Ok(RefreshToken::query()
                    .where_(RefreshToken::FAMILY_ID.eq(sid))
                    .where_(RefreshToken::USER_ID.eq(user))
                    .where_null(RefreshToken::REVOKED_AT)
                    .update(&uow, [RefreshToken::REVOKED_AT.set(now)])
                    .await?)
            })
            .await?;
        self.session_ended(user, Some(sid)).await;
        Ok(revoked)
    }

    /// When the server-side session `sid` of `user` ends, if it is live
    /// (not revoked, not expired).
    async fn live_session(&self, user: Uuid, sid: Uuid) -> Result<Option<i64>, AuthError> {
        let now = self.clock.now();
        Ok(RefreshToken::query()
            .where_(RefreshToken::FAMILY_ID.eq(sid))
            .where_(RefreshToken::USER_ID.eq(user))
            .where_null(RefreshToken::REVOKED_AT)
            .where_(RefreshToken::EXPIRES_AT.gt(now))
            .first(&self.db)
            .await?
            .map(|row| row.into_inner().expires_at.timestamp()))
    }
}

/// A session that could not hold a value (it can't be JSON: never for the
/// values stored here).
fn session_error(error: navree_session::SessionError) -> AuthError {
    AuthError::Session(error.to_string())
}

/// The session strategy (`"session"`): a request whose navree-session
/// session names a signed-in user is authenticated as that user,
/// [`AuthVia::Session`], once the server-side session is checked to be live
/// (not signed out, not ended from another device, not revoked by a password
/// change, a reset or offboarding, not past its end). A session whose
/// server-side session is no longer live forgets the user, and the request
/// goes on as a guest: a stale cookie never locks anyone out of the sign-in
/// page.
///
/// It stores [`AccessClaims`] in the request as the JWT strategy does (the
/// user, the session id as `sid`, the sign-in or confirmation time as
/// `auth_time`, no `jti`), so the same guards hold for both:
/// `.authenticated()`, [`recent_auth`](crate::recent_auth),
/// [`not_while_impersonating`](crate::not_while_impersonating),
/// [`platform_session`](crate::platform_session) (a browser session is never
/// one) and the two-factor setup rule (a user who must set up two-factor
/// authentication reaches only the setup routes and signing out; 403
/// `two_factor_required` elsewhere).
///
/// The auth module registers it after the [`JwtStrategy`](crate::JwtStrategy)
/// when the `session` feature is on, so a request with a bearer token is
/// the token's; it does nothing without `SessionModule` in the app.
pub struct SessionStrategy {
    service: Arc<AuthService>,
    setup_routes: Arc<[(Method, String)]>,
}

impl SessionStrategy {
    /// The strategy checking sessions with `service`.
    pub fn new(service: Arc<AuthService>) -> Self {
        Self {
            service,
            setup_routes: Arc::from(Vec::new()),
        }
    }

    /// The routes (method and full path) a user who must set up two-factor
    /// authentication reaches. The auth module passes the JWT strategy's
    /// and `POST /sign-out`.
    pub fn with_setup_routes(mut self, routes: impl IntoIterator<Item = (Method, String)>) -> Self {
        self.setup_routes = routes.into_iter().collect();
        self
    }

    fn is_setup_route(&self, parts: &Parts) -> bool {
        let path = parts.uri.path().trim_end_matches('/');
        self.setup_routes
            .iter()
            .any(|(method, route)| *method == parts.method && route == path)
    }

    async fn principal(&self, parts: &mut Parts) -> Result<Option<Principal>, AppError> {
        let Some(session) = parts.extensions.get::<Session>().cloned() else {
            return Ok(None);
        };
        let Some(mut signed_in) = session.get::<SignedIn>(SESSION_USER_KEY) else {
            return Ok(None);
        };
        let service = &self.service;
        let ends_at = service
            .live_session(signed_in.user, signed_in.sid)
            .await
            .map_err(AuthError::into_app_error)?;
        let Some(ends_at) = ends_at else {
            tracing::info!(
                target: "navree::security",
                user_id = %signed_in.user,
                "a browser session whose sign-in ended was used; it is signed out"
            );
            session.remove(SESSION_USER_KEY);
            session.remove(SESSION_STASH_KEY);
            session.regenerate();
            return Ok(None);
        };
        if signed_in.setup {
            // Set up since? Then the session reaches everything from now on.
            let user = service
                .find_user(signed_in.user)
                .await
                .map_err(AuthError::into_app_error)?;
            if user.as_ref().is_some_and(two_factor_enabled) {
                signed_in.setup = false;
                session
                    .insert(SESSION_USER_KEY, &signed_in)
                    .map_err(|error| session_error(error).into_app_error())?;
            } else if !self.is_setup_route(parts) {
                return Err(AuthError::TwoFactorSetupRequired.into_app_error());
            }
        }
        let jwt = service.jwt();
        parts.extensions.insert(AccessClaims(Claims {
            sub: signed_in.user,
            iat: signed_in.at,
            exp: ends_at,
            iss: jwt.issuer().to_owned(),
            aud: jwt.audience().map(str::to_owned),
            jti: String::new(),
            typ: if signed_in.setup {
                TokenType::TwoFactorSetup
            } else {
                TokenType::Access
            },
            sid: Some(signed_in.sid),
            // An impersonation has no sign-in time of its own: routes that
            // want a recent sign-in refuse it, as for a token.
            auth_time: signed_in.impersonator.is_none().then_some(signed_in.at),
            act: signed_in.impersonator.map(|sub| ActorClaim { sub }),
            sign_in: None,
        }));
        if let Some(sub) = signed_in.impersonator {
            parts.extensions.insert(Impersonator(sub));
        }
        Ok(Some(Principal {
            user_id: signed_in.user,
            via: AuthVia::Session,
            token_abilities: None,
            session_id: Some(signed_in.sid.to_string()),
        }))
    }
}

impl fmt::Debug for SessionStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionStrategy").finish_non_exhaustive()
    }
}

impl Strategy for SessionStrategy {
    fn name(&self) -> &'static str {
        "session"
    }

    fn authenticate<'a>(
        &'a self,
        parts: &'a mut Parts,
    ) -> BoxFuture<'a, Result<Option<Principal>, AppError>> {
        Box::pin(self.principal(parts))
    }
}
