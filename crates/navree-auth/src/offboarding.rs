//! Offboarding: ending every credential of a user, or of every user of an
//! email domain (when single sign-on enforcement turns on, say).

use std::collections::BTreeMap;

use navree_contracts::{AuditEntry, AuditKind, CredentialRevoker};
use navree_orm::Model as _;
use serde_json::json;
use uuid::Uuid;

use crate::refresh_token::Model as RefreshToken;
use crate::service::invalid;
use crate::{AuthError, AuthService, User, email_domain};

/// How many users are read at a time when looking for a domain's users.
const PAGE: u64 = 500;

/// What [`AuthService::revoke_credentials_of_domain`] and
/// [`AuthService::revoke_all_credentials`] ended.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct CredentialsRevoked {
    /// How many users were cut off.
    pub users: u64,
    /// The credentials revoked, by kind: `"refresh_tokens"` (the sign-in
    /// sessions of navree-auth) and whatever the other modules report
    /// (navree-oauth: `"personal_access_tokens"`, `"oauth_access_tokens"`,
    /// `"oauth_refresh_tokens"`, `"oauth_codes"`, `"oauth_device_codes"`,
    /// `"oauth_authorizations"`).
    pub by_kind: BTreeMap<&'static str, u64>,
}

impl CredentialsRevoked {
    /// How many credentials of `kind` were revoked.
    pub fn revoked(&self, kind: &str) -> u64 {
        self.by_kind.get(kind).copied().unwrap_or(0)
    }

    /// How many credentials were revoked in all.
    pub fn total(&self) -> u64 {
        self.by_kind.values().sum()
    }
}

impl AuthService {
    /// Ends every credential of every user whose email is on `domain`, so
    /// nothing they were given before keeps working: the sign-in sessions
    /// (refresh tokens) and real-time subscriptions of navree-auth, then
    /// whatever the other modules registered as a
    /// [`CredentialRevoker`] hold (navree-oauth: personal access tokens,
    /// OAuth access and refresh tokens, unused authorization codes and
    /// device codes, and the remembered consent of every app).
    ///
    /// This is the call to make when single sign-on enforcement turns on
    /// for a domain, so that sessions and tokens from before the switch end
    /// with it. `domain` is read the way [`SignInAttempt::domain`] is
    /// (`ACME.test.` is `acme.test`), and users are matched the same way
    /// whatever their email's spelling. Reading the users scans the
    /// users' emails in pages, which suits an offboarding, not a request.
    ///
    /// Access tokens issued by navree-auth are stateless: one already in
    /// a client's hands works until it expires
    /// (`auth.access_ttl_secs`, 15 minutes by default); it cannot be
    /// refreshed any more. Every other kind stops at once.
    ///
    /// Every revoker runs even when one fails; the first failure is
    /// returned afterwards ([`AuthError::Revocation`]), and the call can
    /// be repeated. Audited as `auth.credentials_revoked`.
    ///
    /// ```ignore
    /// let ended = auth.revoke_credentials_of_domain("acme.test").await?;
    /// tracing::info!(users = ended.users, credentials = ended.total(), "offboarded");
    /// ```
    pub async fn revoke_credentials_of_domain(
        &self,
        domain: &str,
    ) -> Result<CredentialsRevoked, AuthError> {
        let Some(domain) = email_domain(&format!("@{}", domain.trim())) else {
            return Err(invalid("domain", "required", &[]));
        };
        let users = self.user_ids_of_domain(&domain).await?;
        self.revoke_credentials_of(&users, Some(&domain)).await
    }

    /// Ends every credential of `user_id`, like
    /// [`revoke_credentials_of_domain`](AuthService::revoke_credentials_of_domain)
    /// does for a domain's users (the account itself stays).
    pub async fn revoke_all_credentials(
        &self,
        user_id: Uuid,
    ) -> Result<CredentialsRevoked, AuthError> {
        self.revoke_credentials_of(&[user_id], None).await
    }

    /// The ids of the users whose email domain is `domain` (normalized).
    async fn user_ids_of_domain(&self, domain: &str) -> Result<Vec<Uuid>, AuthError> {
        let mut found = Vec::new();
        let mut last: Option<Uuid> = None;
        loop {
            let mut query = User::query();
            if let Some(last) = last {
                query = query.where_(User::ID.gt(last));
            }
            let page = query
                .order_by(User::ID)
                .limit(PAGE)
                .get(&self.db)
                .await?
                .into_models();
            let Some(end) = page.last().map(|user| user.id) else {
                break;
            };
            found.extend(
                page.iter()
                    .filter(|user| email_domain(&user.email).as_deref() == Some(domain))
                    .map(|user| user.id),
            );
            if (page.len() as u64) < PAGE {
                break;
            }
            last = Some(end);
        }
        Ok(found)
    }

    async fn revoke_credentials_of(
        &self,
        users: &[Uuid],
        domain: Option<&str>,
    ) -> Result<CredentialsRevoked, AuthError> {
        let mut report = CredentialsRevoked {
            users: users.len() as u64,
            by_kind: BTreeMap::new(),
        };
        if users.is_empty() {
            return Ok(report);
        }
        let now = self.clock.now();
        let ids = users.to_vec();
        let sessions = self
            .own(None, None, |uow| async move {
                Ok(RefreshToken::query()
                    .where_(RefreshToken::USER_ID.is_in(ids))
                    .where_null(RefreshToken::REVOKED_AT)
                    .update(&uow, [RefreshToken::REVOKED_AT.set(now)])
                    .await?)
            })
            .await?;
        report.by_kind.insert("refresh_tokens", sessions);
        for user in users {
            crate::disconnect::end_user_subscriptions(&self.app, *user).await;
        }

        let mut failure = None;
        if let Some(app) = self.app.upgrade() {
            for revoker in app.get_all::<dyn CredentialRevoker>().await? {
                match revoker.revoke_for_users(users).await {
                    Ok(counts) => {
                        for (kind, count) in counts {
                            *report.by_kind.entry(kind).or_default() += count;
                        }
                    }
                    Err(error) => {
                        tracing::error!(
                            target: "navree::auth",
                            module = revoker.name(),
                            %error,
                            "a module could not revoke the credentials of offboarded users"
                        );
                        failure.get_or_insert(AuthError::Revocation {
                            module: revoker.name(),
                            source: error,
                        });
                    }
                }
            }
        }

        let mut entry =
            AuditEntry::new(AuditKind::Security, "auth.credentials_revoked").metadata(json!({
                "domain": domain,
                "users": report.users,
                "revoked": report.by_kind,
                "complete": failure.is_none(),
            }));
        if let Some(user) = users.first().filter(|_| users.len() == 1) {
            entry = entry.subject("User", user.to_string());
        }
        self.audit(entry).await;
        match failure {
            Some(error) => Err(error),
            None => Ok(report),
        }
    }
}
