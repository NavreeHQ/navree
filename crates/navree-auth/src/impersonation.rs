//! Impersonation on [`AuthService`]: an administrator signs in as another
//! user for support, in a session whose tokens carry the `act` claim.
//!
//! - `POST /auth/impersonate/{user}` (permission `users.impersonate`, a
//!   reason required) starts an impersonation session: a refresh-token
//!   family with `impersonator_id`, ending after
//!   `auth.impersonation.ttl_secs` however often it is refreshed, whose
//!   access tokens carry `act: {sub: <admin>}` and no `auth_time`.
//! - Requests of the session are the target user's (`CurrentUser`), with
//!   navree-contracts' `Impersonator` in the request: [`Actor`](crate::Actor)
//!   names both, the unit of work carries the impersonator and the audit
//!   trail records it on every entry (`impersonator_id`).
//! - `#[not_while_impersonating]` routes refuse the session (the auth
//!   module's account-security routes among them), and `#[recent_auth]`
//!   routes always do (no `auth_time`).
//! - `DELETE /auth/impersonate` ends it.

use navree_contracts::AuditKind;
use serde_json::json;
use uuid::Uuid;

use crate::service::{SessionKind, entry, revoke_family};
use crate::{AuthError, AuthService, Claims, ClientMeta, TokenPair, USER_SUBJECT, User};

/// The longest accepted impersonation reason, in characters.
pub const MAX_IMPERSONATION_REASON: usize = 500;

impl AuthService {
    /// Starts impersonating `target_id` as `impersonator_id` (`POST
    /// /auth/impersonate/{user}`; the route checks the permission
    /// `auth.impersonation.permission`), for `reason` (required, at most
    /// [`MAX_IMPERSONATION_REASON`] characters). Refused with
    /// [`AuthError::ImpersonationForbidden`] from an impersonation session
    /// (`impersonator_claims` with `act`), [`AuthError::CannotImpersonate`]
    /// for oneself, a platform administrator, or a user who may impersonate
    /// others; [`AuthError::UserNotFound`] for an unknown target. Returns the
    /// target and the session's tokens. Audited as
    /// `auth.impersonation_started` (actor: the impersonator; subject: the
    /// target; metadata: the reason and the session id).
    pub async fn impersonate(
        &self,
        impersonator_id: Uuid,
        impersonator_claims: Option<&Claims>,
        target_id: Uuid,
        reason: &str,
        meta: &ClientMeta,
    ) -> Result<(User, TokenPair), AuthError> {
        if !self.options.impersonation.enabled {
            return Err(AuthError::Disabled("impersonation"));
        }
        if impersonator_claims.is_some_and(|claims| claims.act.is_some()) {
            return Err(AuthError::ImpersonationForbidden);
        }
        let reason = reason.trim();
        if reason.is_empty() {
            return Err(AuthError::Invalid {
                field: "reason",
                message: "reason should not be empty".to_owned(),
            });
        }
        if reason.chars().count() > MAX_IMPERSONATION_REASON {
            return Err(AuthError::Invalid {
                field: "reason",
                message: format!(
                    "reason must be shorter than or equal to {MAX_IMPERSONATION_REASON} characters"
                ),
            });
        }
        if target_id == impersonator_id {
            return Err(AuthError::CannotImpersonate("yourself"));
        }
        let target = self
            .find_user(target_id)
            .await?
            .ok_or(AuthError::UserNotFound)?;
        if self.is_platform_admin(target.id).await? {
            return Err(AuthError::CannotImpersonate("a platform administrator"));
        }
        if self.may_impersonate(target.id).await? {
            return Err(AuthError::CannotImpersonate(
                "a user who may impersonate others",
            ));
        }
        let (tokens, session_id) = self
            .open_session(
                &target,
                meta.clone(),
                SessionKind::Impersonation {
                    impersonator: impersonator_id,
                },
            )
            .await?;
        tracing::info!(
            target: "navree::security",
            impersonator_id = %impersonator_id,
            user_id = %target.id,
            %session_id,
            "impersonation started"
        );
        self.audit(
            entry(AuditKind::Security, "auth.impersonation_started", meta)
                .actor(impersonator_id)
                .impersonator(impersonator_id)
                .subject(USER_SUBJECT, target.id.to_string())
                .metadata(json!({ "reason": reason, "session_id": session_id })),
        )
        .await;
        Ok((target, tokens))
    }

    /// Ends the impersonation session of `claims` (`DELETE
    /// /auth/impersonate`): its refresh tokens are revoked (its access
    /// tokens live until they expire; the client switches back to its own
    /// tokens). [`AuthError::NotImpersonating`] for a token without `act`.
    /// Audited as `auth.impersonation_ended`.
    pub async fn stop_impersonating(
        &self,
        claims: &Claims,
        meta: &ClientMeta,
    ) -> Result<(), AuthError> {
        let Some(act) = claims.act else {
            return Err(AuthError::NotImpersonating);
        };
        let now = self.clock.now();
        if let Some(session_id) = claims.sid {
            self.write(Some(claims.sub), |uow| async move {
                revoke_family(&uow, session_id, now).await
            })
            .await?;
        }
        self.audit(
            entry(AuditKind::Security, "auth.impersonation_ended", meta)
                .actor(act.sub)
                .impersonator(act.sub)
                .subject(USER_SUBJECT, claims.sub.to_string())
                .metadata(json!({ "session_id": claims.sid })),
        )
        .await;
        Ok(())
    }

    /// Whether `user_id` holds the impersonation permission (navree-access,
    /// when `AccessModule` is in the app).
    async fn may_impersonate(&self, user_id: Uuid) -> Result<bool, AuthError> {
        let Some(app) = self.app.upgrade() else {
            return Err(AuthError::ShutDown);
        };
        let Some(access) = app.try_get::<navree_access::AccessService>().await else {
            return Ok(false);
        };
        let permissions = access
            .permissions_of(user_id)
            .await
            .map_err(|error| AuthError::Extension(error.into()))?;
        let wanted = &self.options.impersonation.permission;
        Ok(permissions
            .iter()
            .any(|permission| permission == wanted || permission == "*"))
    }
}
