//! Sessions on [`AuthService`]: the user's refresh-token families, listed
//! and revoked, and step-up confirmation (`POST /auth/confirm`).

use std::collections::HashSet;
use std::fmt;
use std::time::Duration;

use chrono::{DateTime, Utc};
use navree_contracts::AuditKind;
use navree_openapi::ToSchema;
use navree_orm::Model as _;
use serde::Serialize;
use serde_json::json;
use uuid::Uuid;

use crate::refresh_token::Model as RefreshToken;
use crate::service::{PLATFORM_AUDIENCE, entry};
use crate::two_factor::two_factor_enabled;
use crate::{AuthError, AuthService, Claims, ClientMeta, SessionClaims, TokenType, USER_SUBJECT};

/// One of the user's sessions (a refresh-token family), as `GET
/// /auth/sessions` lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct SessionResource {
    /// The session id (`DELETE /auth/sessions/{id}`; the `sid` claim).
    pub id: Uuid,
    /// The client IP address of its last sign-in or refresh.
    pub ip: Option<String>,
    /// The client's `User-Agent` at its last sign-in or refresh.
    pub user_agent: Option<String>,
    /// A short description of the device, from the user agent
    /// (`"Firefox on Windows"`).
    pub device: Option<String>,
    /// When it was last used (its last sign-in or refresh).
    pub last_used_at: DateTime<Utc>,
    /// When the user last authenticated in it (signing in or confirming).
    pub authenticated_at: Option<DateTime<Utc>>,
    /// When it ends unless refreshed.
    pub expires_at: DateTime<Utc>,
    /// Whether the request was made with it.
    pub current: bool,
    /// For an impersonation session: the impersonating user.
    pub impersonator_id: Option<Uuid>,
    /// Whether it is a platform session.
    pub platform: bool,
}

/// What a step-up confirmation checks (`POST /auth/confirm`, `POST
/// /auth/confirm/passkey`, [`AuthService::confirm_action`]): the password, a
/// code from the authenticator app, or a passkey assertion.
#[derive(Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConfirmProof {
    /// The account password.
    Password(String),
    /// A current TOTP code (two-factor authentication must be on).
    Code(String),
    /// A passkey assertion (the `PublicKeyCredential` JSON) over a
    /// confirmation challenge
    /// ([`AuthService::confirmation_options`]), with user verification: a
    /// fingerprint, Face ID, Windows Hello or the device's PIN.
    Passkey(serde_json::Value),
}

impl ConfirmProof {
    /// `"password"`, `"totp"` or `"passkey"` (audit metadata).
    pub fn method(&self) -> &'static str {
        match self {
            ConfirmProof::Password(_) => "password",
            ConfirmProof::Code(_) => "totp",
            ConfirmProof::Passkey(_) => "passkey",
        }
    }
}

/// The proofs are credentials: never in debug output.
impl fmt::Debug for ConfirmProof {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ConfirmProof::{}([redacted])", self.method())
    }
}

/// What `POST /auth/confirm` returns: a new access token for the same
/// session with `auth_time` set to now.
#[derive(Clone, Serialize, ToSchema)]
pub struct ConfirmedToken {
    /// The new access token.
    pub access_token: String,
    /// Always `"Bearer"`.
    #[schema(example = "Bearer")]
    pub token_type: &'static str,
    /// Its lifetime in seconds.
    pub expires_in: u64,
    /// When the user confirmed (Unix seconds; the token's `auth_time`).
    pub auth_time: i64,
}

/// The token is a credential: never in debug output.
impl fmt::Debug for ConfirmedToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConfirmedToken")
            .field("access_token", &"[redacted]")
            .field("expires_in", &self.expires_in)
            .field("auth_time", &self.auth_time)
            .finish()
    }
}

impl AuthService {
    /// The live sessions of `user_id` (refresh-token families with an
    /// unexpired, unrevoked token), most recently used first; the one with
    /// id `current` is marked `current`.
    pub async fn sessions(
        &self,
        user_id: Uuid,
        current: Option<Uuid>,
    ) -> Result<Vec<SessionResource>, AuthError> {
        let now = self.clock.now();
        let live = RefreshToken::query()
            .where_(RefreshToken::USER_ID.eq(user_id))
            .where_null(RefreshToken::REVOKED_AT)
            .where_(RefreshToken::EXPIRES_AT.gt(now))
            .order_by_desc(RefreshToken::CREATED_AT)
            .order_by_desc(RefreshToken::ID)
            .get(&self.db)
            .await?;
        let mut seen = HashSet::new();
        Ok(live
            .into_models()
            .into_iter()
            .filter(|row| seen.insert(row.family_id))
            .map(|row| SessionResource {
                id: row.family_id,
                device: row.user_agent.as_deref().and_then(describe_device),
                ip: row.ip,
                user_agent: row.user_agent,
                last_used_at: row.created_at,
                authenticated_at: row.authenticated_at,
                expires_at: row.expires_at,
                current: Some(row.family_id) == current,
                impersonator_id: row.impersonator_id,
                platform: row.audience.as_deref() == Some(PLATFORM_AUDIENCE),
            })
            .collect())
    }

    /// Of `ids` (session ids of `user_id`), the ones that are live: not
    /// signed out, not ended, not expired.
    pub async fn live_sessions(&self, user_id: Uuid, ids: &[Uuid]) -> Result<Vec<Uuid>, AuthError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let now = self.clock.now();
        let rows = RefreshToken::query()
            .where_(RefreshToken::USER_ID.eq(user_id))
            .where_(RefreshToken::FAMILY_ID.is_in(ids.iter().copied()))
            .where_null(RefreshToken::REVOKED_AT)
            .where_(RefreshToken::EXPIRES_AT.gt(now))
            .get(&self.db)
            .await?;
        let mut live: Vec<Uuid> = rows
            .into_models()
            .into_iter()
            .map(|row| row.family_id)
            .collect();
        live.sort();
        live.dedup();
        Ok(live)
    }

    /// What ends with a session (`Some`) or with every session of `user`
    /// (`None`) besides its tokens: with the `notify` feature, the push
    /// devices it registered.
    pub(crate) async fn session_ended(&self, user: Uuid, session: Option<Uuid>) {
        #[cfg(feature = "notify")]
        {
            let ended = match session {
                Some(session) => crate::push::Ended::Session(session),
                None => crate::push::Ended::All,
            };
            crate::push::end_devices(self, user, ended).await;
        }
        #[cfg(not(feature = "notify"))]
        let _ = (user, session);
    }

    /// Ends the session `session_id` of `user_id` (`DELETE
    /// /auth/sessions/{id}`): its refresh tokens stop working (its access
    /// tokens live until they expire). [`AuthError::SessionNotFound`] when
    /// it is not one of the user's live sessions. Audited as
    /// `auth.session_revoked`.
    pub async fn revoke_session(
        &self,
        user_id: Uuid,
        session_id: Uuid,
        meta: &ClientMeta,
    ) -> Result<(), AuthError> {
        let now = self.clock.now();
        let revoked = self
            .write(Some(user_id), |uow| async move {
                Ok(RefreshToken::query()
                    .where_(RefreshToken::FAMILY_ID.eq(session_id))
                    .where_(RefreshToken::USER_ID.eq(user_id))
                    .where_null(RefreshToken::REVOKED_AT)
                    .update(&uow, [RefreshToken::REVOKED_AT.set(now)])
                    .await?)
            })
            .await?;
        if revoked == 0 {
            return Err(AuthError::SessionNotFound);
        }
        self.session_ended(user_id, Some(session_id)).await;
        self.audit(
            entry(AuditKind::Auth, "auth.session_revoked", meta)
                .actor(user_id)
                .subject(USER_SUBJECT, user_id.to_string())
                .metadata(json!({ "session_id": session_id })),
        )
        .await;
        Ok(())
    }

    /// Step-up confirmation (`POST /auth/confirm`): checks `proof` (the
    /// password, counted by the login throttle; or a TOTP code, counted by
    /// the two-factor throttle) for the user of `claims`, records the time
    /// as the session's `authenticated_at` (so refreshed tokens keep it)
    /// and returns a new access token of the same session with `auth_time`
    /// now, which `#[recent_auth]` routes accept. Refused in an
    /// impersonation session ([`AuthError::ImpersonationForbidden`]); a
    /// wrong password or code is a field error. Audited as
    /// `auth.confirmed` with the method.
    pub async fn confirm(
        &self,
        claims: &Claims,
        proof: ConfirmProof,
        meta: &ClientMeta,
    ) -> Result<ConfirmedToken, AuthError> {
        let auth_time = self
            .confirm_identity(claims.sub, claims.sid, claims.act.is_some(), proof, meta)
            .await?;
        let (typ, ttl) = match claims.typ {
            TokenType::Platform => (
                TokenType::Platform,
                Duration::from_secs(self.options.platform.access_ttl_secs),
            ),
            TokenType::TwoFactorSetup => (TokenType::TwoFactorSetup, self.jwt.access_ttl()),
            _ => (TokenType::Access, self.jwt.access_ttl()),
        };
        let access_token = self.jwt.sign_session(
            claims.sub,
            typ,
            ttl,
            SessionClaims {
                session_id: claims.sid,
                auth_time: Some(auth_time),
                impersonator: None,
            },
        )?;
        Ok(ConfirmedToken {
            access_token,
            token_type: "Bearer",
            expires_in: ttl.as_secs(),
            auth_time,
        })
    }

    /// The step-up check behind [`confirm`](AuthService::confirm), for
    /// tokens and for the session cookie alike: `proof` for `user_id`, the
    /// time recorded as the `authenticated_at` of session `session_id`, and
    /// the `auth.confirmed` audit entry. The new `auth_time` (Unix seconds).
    pub(crate) async fn confirm_identity(
        &self,
        user_id: Uuid,
        session_id: Option<Uuid>,
        impersonating: bool,
        proof: ConfirmProof,
        meta: &ClientMeta,
    ) -> Result<i64, AuthError> {
        self.confirm_identity_for(user_id, session_id, impersonating, proof, None, meta)
            .await
    }

    /// Confirms the sensitive `action` for the user of `claims` with
    /// `proof`, as a step-up confirmation: a passkey assertion must answer a
    /// challenge of [`confirmation_options`](AuthService::confirmation_options)
    /// for the same action (its digest is computed again here from
    /// `action`), issued to this user and this session less than
    /// [`CONFIRM_TTL_SECS`](crate::CONFIRM_TTL_SECS) seconds ago and not
    /// answered before, signed by one of the user's passkeys with user
    /// verification. The password or a code (for clients without passkeys)
    /// are checked as [`confirm`](AuthService::confirm) checks them. It
    /// approves `action` only: the session's `authenticated_at` (what
    /// `#[recent_auth]` routes read) stays as it was, so a confirmation of
    /// one action is never a step-up for anything else. The time of the
    /// confirmation is returned in Unix seconds.
    ///
    /// A refused passkey is [`AuthError::ConfirmationRefused`] (403
    /// `confirmation_refused`), audited as `auth.confirmation_refused`; a
    /// confirmation is audited as `auth.confirmed` with the method and
    /// `metadata.action`. Refused in an impersonation session.
    ///
    /// ```no_run
    /// use navree_auth::{AccessClaims, AuthService, ClientMeta, ConfirmAction, ConfirmProof};
    /// # async fn approve(auth: &AuthService, claims: &AccessClaims, meta: &ClientMeta,
    /// #     credential: serde_json::Value) -> Result<(), navree_auth::AuthError> {
    /// // The same details the page asked the person to confirm.
    /// let action = ConfirmAction::new("payments.approve")
    ///     .field("payment", "PAY-2024")
    ///     .field("amount_paise", 10_000_000);
    /// auth.confirm_action(&claims.0, &action, ConfirmProof::Passkey(credential), meta)
    ///     .await?;
    /// // ... approve the payment.
    /// # Ok(()) }
    /// ```
    pub async fn confirm_action(
        &self,
        claims: &Claims,
        action: &crate::ConfirmAction,
        proof: ConfirmProof,
        meta: &ClientMeta,
    ) -> Result<i64, AuthError> {
        self.confirm_identity_for(
            claims.sub,
            claims.sid,
            claims.act.is_some(),
            proof,
            Some(action),
            meta,
        )
        .await
    }

    /// [`confirm_identity`](AuthService::confirm_identity) for `action`
    /// (`None`: a plain step-up).
    async fn confirm_identity_for(
        &self,
        user_id: Uuid,
        session_id: Option<Uuid>,
        impersonating: bool,
        proof: ConfirmProof,
        action: Option<&crate::ConfirmAction>,
        meta: &ClientMeta,
    ) -> Result<i64, AuthError> {
        if impersonating {
            return Err(AuthError::ImpersonationForbidden);
        }
        let user = self
            .find_user(user_id)
            .await?
            .ok_or(AuthError::UserNotFound)?;
        let mut passkey_id = None;
        match &proof {
            ConfirmProof::Password(password) => {
                self.confirm_password(&user, password, "password", meta)
                    .await?;
            }
            ConfirmProof::Code(code) => {
                if !two_factor_enabled(&user) {
                    return Err(AuthError::TwoFactorNotEnabled);
                }
                self.check_two_factor_code(&user, code, "code", meta, "confirmation")
                    .await?;
            }
            ConfirmProof::Passkey(credential) => {
                let step_up = crate::ConfirmAction::new(crate::passkeys::confirm::STEP_UP_ACTION);
                let checked = action.unwrap_or(&step_up);
                passkey_id = Some(
                    self.check_confirmation(&user, session_id, checked, credential, meta)
                        .await?,
                );
            }
        }
        let now = self.clock.now();
        // A confirmation of one action approves that action only: the
        // session's sign-in does not count as recent because of it.
        if let (Some(session_id), None) = (session_id, action) {
            let user_id = user.id;
            self.write(Some(user_id), |uow| async move {
                RefreshToken::query()
                    .where_(RefreshToken::FAMILY_ID.eq(session_id))
                    .where_(RefreshToken::USER_ID.eq(user_id))
                    .where_null(RefreshToken::REVOKED_AT)
                    .update(&uow, [RefreshToken::AUTHENTICATED_AT.set(now)])
                    .await?;
                Ok(())
            })
            .await?;
        }
        let mut metadata = json!({ "method": proof.method(), "session_id": session_id });
        if let Some(action) = action {
            metadata["action"] = json!(action.name());
        }
        if let Some(passkey_id) = passkey_id {
            metadata["passkey_id"] = json!(passkey_id);
        }
        self.audit(
            entry(AuditKind::Auth, "auth.confirmed", meta)
                .actor(user.id)
                .metadata(metadata),
        )
        .await;
        Ok(now.timestamp())
    }
}

/// `"<browser> on <system>"` from a user agent, when either is recognised.
pub(crate) fn describe_device(user_agent: &str) -> Option<String> {
    let ua = user_agent.to_ascii_lowercase();
    let browser = if ua.contains("edg/") || ua.contains("edge/") {
        Some("Edge")
    } else if ua.contains("opr/") || ua.contains("opera") {
        Some("Opera")
    } else if ua.contains("firefox/") || ua.contains("fxios/") {
        Some("Firefox")
    } else if ua.contains("chrome/") || ua.contains("crios/") || ua.contains("chromium/") {
        Some("Chrome")
    } else if ua.contains("safari/") {
        Some("Safari")
    } else if ua.starts_with("curl/") {
        Some("curl")
    } else {
        None
    };
    let system = if ua.contains("iphone") || ua.contains("ipad") || ua.contains("ios") {
        Some("iOS")
    } else if ua.contains("android") {
        Some("Android")
    } else if ua.contains("windows") {
        Some("Windows")
    } else if ua.contains("mac os x") || ua.contains("macintosh") {
        Some("macOS")
    } else if ua.contains("cros") {
        Some("ChromeOS")
    } else if ua.contains("linux") {
        Some("Linux")
    } else {
        None
    };
    match (browser, system) {
        (Some(browser), Some(system)) => Some(format!("{browser} on {system}")),
        (Some(browser), None) => Some(browser.to_owned()),
        (None, Some(system)) => Some(system.to_owned()),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn devices_are_described_from_user_agents() {
        for (ua, expected) in [
            (
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36",
                Some("Chrome on Windows"),
            ),
            (
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36 Edg/130.0.0.0",
                Some("Edge on Windows"),
            ),
            (
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 14.6; rv:131.0) Gecko/20100101 Firefox/131.0",
                Some("Firefox on macOS"),
            ),
            (
                "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1",
                Some("Safari on iOS"),
            ),
            (
                "Mozilla/5.0 (Linux; Android 15; Pixel 9) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Mobile Safari/537.36",
                Some("Chrome on Android"),
            ),
            ("curl/8.9.1", Some("curl")),
            ("navree-tests", None),
        ] {
            assert_eq!(describe_device(ua).as_deref(), expected, "{ua}");
        }
    }

    #[test]
    fn proofs_never_show_their_secrets() {
        let debug = format!("{:?}", ConfirmProof::Password("hunter2".to_owned()));
        assert!(!debug.contains("hunter2"), "{debug}");
        assert_eq!(ConfirmProof::Code("1".to_owned()).method(), "totp");
        let passkey = ConfirmProof::Passkey(serde_json::json!({ "id": "c2VjcmV0" }));
        assert_eq!(passkey.method(), "passkey");
        assert!(!format!("{passkey:?}").contains("c2VjcmV0"));
    }
}
