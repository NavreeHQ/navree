//! [`PersonalAccessTokens`]: `nvp_` tokens users create for scripts.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use navree_contracts::{AuditEntry, AuditKind};
use navree_db::Uow;
use navree_orm::Model as _;
use serde::Serialize;
use serde_json::json;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::core::Core;
use crate::entity::PersonalAccessToken;
use crate::error::OAuthError;
use crate::scope::{is_built_in, join_scopes};
use crate::token::{PERSONAL_ACCESS_TOKEN_PREFIX, digest, new_token};

/// The longest token name.
pub const MAX_TOKEN_NAME_LEN: usize = 255;

/// One day, in seconds.
const DAY_SECS: u64 = 86_400;

/// A personal access token just created: the record and the token itself
/// (`nvp_...`), which is shown once and stored only as a digest.
#[derive(Clone)]
pub struct CreatedPersonalAccessToken {
    /// The token's record.
    pub record: PersonalAccessToken,
    /// The token to show the user once.
    pub token: String,
}

impl fmt::Debug for CreatedPersonalAccessToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CreatedPersonalAccessToken")
            .field("record", &self.record)
            .field("token", &"[redacted]")
            .finish()
    }
}

/// A personal access token as the API returns it (`token` only right
/// after creation).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct PersonalAccessTokenResource {
    /// The id.
    pub id: Uuid,
    /// The name.
    pub name: String,
    /// Its scopes.
    pub scopes: Vec<String>,
    /// The token, in the creation response only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// When it expires; `null` never.
    pub expires_at: Option<DateTime<Utc>>,
    /// When it was last used.
    pub last_used_at: Option<DateTime<Utc>>,
    /// When it was created.
    pub created_at: DateTime<Utc>,
}

impl From<&PersonalAccessToken> for PersonalAccessTokenResource {
    fn from(record: &PersonalAccessToken) -> Self {
        Self {
            id: record.id,
            name: record.name.clone(),
            scopes: record.scope_list(),
            token: None,
            expires_at: record.expires_at,
            last_used_at: record.last_used_at,
            created_at: record.created_at,
        }
    }
}

impl From<&CreatedPersonalAccessToken> for PersonalAccessTokenResource {
    fn from(created: &CreatedPersonalAccessToken) -> Self {
        Self {
            token: Some(created.token.clone()),
            ..Self::from(&created.record)
        }
    }
}

/// Creates, lists and revokes personal access tokens. Provided and
/// exported by `OAuthServerModule`.
///
/// A personal access token authenticates exactly like an OAuth access
/// token: as its user, narrowed to its scopes' permissions.
///
/// ```ignore
/// let thirty_days = Duration::from_secs(30 * 86_400);
/// let created = pats
///     .create(&uow, user.id, "CI deploy check", ["deals:read"], Some(thirty_days))
///     .await?;
/// println!("{}", created.token); // nvp_...
/// ```
pub struct PersonalAccessTokens {
    core: Arc<Core>,
}

impl PersonalAccessTokens {
    pub(crate) fn new(core: Arc<Core>) -> Self {
        Self { core }
    }

    /// Creates a token for `user_id` named `name` with `scopes` (the app's
    /// scopes from `oauth.scopes`; the built-in OpenID scopes mean nothing
    /// here). `lifetime` defaults to the longest allowed
    /// (`oauth.personal_access_tokens.max_days`, or never expiring when it
    /// is `0`); a longer one is an error. Audited as `oauth.pat_created`.
    pub async fn create<I, S>(
        &self,
        uow: &Uow,
        user_id: Uuid,
        name: &str,
        scopes: I,
        lifetime: Option<Duration>,
    ) -> Result<CreatedPersonalAccessToken, OAuthError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let name = name.trim();
        if name.is_empty() {
            return Err(OAuthError::invalid("name", "name should not be empty"));
        }
        if name.chars().count() > MAX_TOKEN_NAME_LEN {
            return Err(OAuthError::invalid(
                "name",
                format!("name must be at most {MAX_TOKEN_NAME_LEN} characters"),
            ));
        }
        let mut list: Vec<String> = Vec::new();
        for scope in scopes {
            let scope = scope.into();
            if !list.contains(&scope) {
                list.push(scope);
            }
        }
        if list.is_empty() {
            return Err(OAuthError::invalid("scopes", "scopes should not be empty"));
        }
        if let Some(bad) = list
            .iter()
            .find(|scope| is_built_in(scope) || !self.core.scopes.contains(scope))
        {
            return Err(OAuthError::invalid(
                "scopes",
                format!("scope `{bad}` is not available for personal access tokens"),
            ));
        }
        let max_days = u64::from(self.core.options.personal_access_tokens.max_days);
        let lifetime = match lifetime {
            Some(lifetime) if lifetime.is_zero() => {
                return Err(OAuthError::invalid(
                    "expires_in_days",
                    "expires_in_days must be at least 1",
                ));
            }
            Some(lifetime) if max_days > 0 && lifetime.as_secs() > max_days * DAY_SECS => {
                return Err(OAuthError::invalid(
                    "expires_in_days",
                    format!("expires_in_days must be at most {max_days}"),
                ));
            }
            Some(lifetime) => Some(lifetime),
            None if max_days > 0 => Some(Duration::from_secs(max_days * DAY_SECS)),
            None => None,
        };
        let now = self.core.now();
        let expires_at = lifetime.map(|lifetime| {
            let delta = TimeDelta::from_std(lifetime).unwrap_or(TimeDelta::MAX);
            now.checked_add_signed(delta)
                .unwrap_or(DateTime::<Utc>::MAX_UTC)
        });
        let token = new_token(PERSONAL_ACCESS_TOKEN_PREFIX);
        let record = PersonalAccessToken {
            id: Uuid::now_v7(),
            user_id,
            name: name.to_owned(),
            token_hash: digest(&token),
            scopes: join_scopes(&list),
            expires_at,
            last_used_at: None,
            revoked_at: None,
            created_at: now,
        };
        PersonalAccessToken::insert(uow, [record.clone()]).await?;
        let entry = AuditEntry::new(AuditKind::Auth, "oauth.pat_created")
            .actor(user_id)
            .subject("PersonalAccessToken", record.id.to_string())
            .metadata(json!({ "name": record.name, "scopes": list, "expires_at": expires_at }));
        self.core.audit_after_commit(uow, entry).await;
        Ok(CreatedPersonalAccessToken { record, token })
    }

    /// `user_id`'s tokens that are not revoked, newest first.
    pub async fn list(&self, user_id: Uuid) -> Result<Vec<PersonalAccessToken>, OAuthError> {
        Ok(PersonalAccessToken::query()
            .where_(PersonalAccessToken::USER_ID.eq(user_id))
            .where_null(PersonalAccessToken::REVOKED_AT)
            .order_by_desc(PersonalAccessToken::CREATED_AT)
            .get(&self.core.db)
            .await?
            .into_models())
    }

    /// Revokes `user_id`'s token `id` (404 for someone else's or an
    /// unknown one). Audited as `oauth.pat_revoked`.
    pub async fn revoke(&self, uow: &Uow, user_id: Uuid, id: Uuid) -> Result<(), OAuthError> {
        let record = PersonalAccessToken::find(uow, id)
            .await?
            .filter(|record| record.user_id == user_id && record.revoked_at.is_none())
            .ok_or(OAuthError::NotFound("Personal access token not found."))?;
        let hash = record.token_hash.clone();
        PersonalAccessToken::query()
            .where_key(id)
            .update(uow, [PersonalAccessToken::REVOKED_AT.set(self.core.now())])
            .await?;
        let core = self.core.clone();
        uow.after_commit(move || Box::pin(async move { core.forget([hash]) }));
        let entry = AuditEntry::new(AuditKind::Auth, "oauth.pat_revoked")
            .actor(user_id)
            .subject("PersonalAccessToken", id.to_string())
            .metadata(json!({ "name": record.name }));
        self.core.audit_after_commit(uow, entry).await;
        Ok(())
    }

    /// Records that token `id` was used now (best effort, outside any
    /// request transaction).
    pub(crate) async fn touch(&self, id: Uuid) {
        let now = self.core.now();
        let result = PersonalAccessToken::query()
            .where_key(id)
            .update(&self.core.db, [PersonalAccessToken::LAST_USED_AT.set(now)])
            .await;
        if let Err(error) = result {
            tracing::debug!(target: "navree::oauth", %error, "could not record a personal access token's last use");
        }
    }
}

impl fmt::Debug for PersonalAccessTokens {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PersonalAccessTokens")
            .finish_non_exhaustive()
    }
}
