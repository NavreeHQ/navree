//! Issuing tokens and the grants of the token endpoint: authorization
//! code (with PKCE), client credentials, device code and refresh token
//! (rotation with reuse detection); revocation, introspection and the
//! lookups behind the authentication strategy.

use std::fmt;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use navree_auth::AuthService;
use navree_contracts::{AuditEntry, AuditKind};
use navree_db::Uow;
use navree_openapi::ToSchema;
use navree_orm::{Executor, Model as _, Record};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::clients::{
    GRANT_AUTHORIZATION_CODE, GRANT_CLIENT_CREDENTIALS, GRANT_DEVICE_CODE, wants_refresh,
};
use crate::core::{CachedToken, Core};
use crate::entity::{
    Client, access_token, client, code, device_code, personal_access_token, refresh_token,
};
use crate::error::OAuthError;
use crate::scope::{EMAIL, OFFLINE_ACCESS, OPENID, PROFILE, is_subset, join_scopes, parse_scope};
use crate::token::{
    ACCESS_TOKEN_PREFIX, PERSONAL_ACCESS_TOKEN_PREFIX, REFRESH_TOKEN_PREFIX, digest, new_token,
    pkce_matches, redirect_uri_matches,
};

/// How a token was obtained.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Grant {
    /// The authorization code flow.
    AuthorizationCode,
    /// The client credentials grant (the token acts as the client's owner).
    ClientCredentials,
    /// The device authorization grant.
    DeviceCode,
    /// A refresh of an earlier grant.
    RefreshToken,
    /// A personal access token.
    PersonalAccessToken,
}

impl Grant {
    /// `authorization_code`, `client_credentials`, `device_code`,
    /// `refresh_token` or `personal_access_token`.
    pub fn as_str(self) -> &'static str {
        match self {
            Grant::AuthorizationCode => "authorization_code",
            Grant::ClientCredentials => "client_credentials",
            Grant::DeviceCode => "device_code",
            Grant::RefreshToken => "refresh_token",
            Grant::PersonalAccessToken => "personal_access_token",
        }
    }

    /// The grant stored as `value`.
    pub fn parse(value: &str) -> Option<Grant> {
        Some(match value {
            "authorization_code" => Grant::AuthorizationCode,
            "client_credentials" => Grant::ClientCredentials,
            "device_code" => Grant::DeviceCode,
            "refresh_token" => Grant::RefreshToken,
            "personal_access_token" => Grant::PersonalAccessToken,
            _ => return None,
        })
    }
}

impl fmt::Display for Grant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The token endpoint's success response (RFC 6749 §5.1).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TokenResponse {
    /// The access token (`nva_...`).
    pub access_token: String,
    /// Always `Bearer`.
    pub token_type: String,
    /// Seconds until the access token expires.
    pub expires_in: u64,
    /// The refresh token (`nvr_...`), with `offline_access`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub refresh_token: Option<String>,
    /// The granted scopes, space-separated.
    pub scope: String,
    /// The OpenID Connect ID token, with `openid`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub id_token: Option<String>,
}

impl fmt::Debug for TokenResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TokenResponse")
            .field("token_type", &self.token_type)
            .field("expires_in", &self.expires_in)
            .field("scope", &self.scope)
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "[redacted]"),
            )
            .field("id_token", &self.id_token.is_some())
            .finish_non_exhaustive()
    }
}

/// What to issue.
pub(crate) struct Issue<'a> {
    pub(crate) client: &'a Client,
    pub(crate) user_id: Uuid,
    /// The scopes of the access token.
    pub(crate) scopes: Vec<String>,
    pub(crate) grant: Grant,
    pub(crate) code_id: Option<Uuid>,
    /// An existing refresh family (a refresh), with the original grant's
    /// scopes.
    pub(crate) family: Option<(Uuid, Vec<String>)>,
    /// Whether to issue a refresh token.
    pub(crate) refresh: bool,
    pub(crate) nonce: Option<String>,
}

/// The protocol operations behind the endpoints.
pub(crate) struct Grants {
    pub(crate) core: Arc<Core>,
}

impl Grants {
    /// Issues an access token (plus a refresh token and an ID token when
    /// asked for) in `uow`.
    pub(crate) async fn issue(
        &self,
        uow: &Uow,
        issue: Issue<'_>,
    ) -> Result<TokenResponse, OAuthError> {
        let core = &self.core;
        let now = core.now();
        let ttl = core.options.access_token_ttl_secs;
        let access = new_token(ACCESS_TOKEN_PREFIX);
        let access_id = Uuid::now_v7();
        let family_id = issue.family.as_ref().map(|(family, _)| *family);
        let refresh_family = issue
            .refresh
            .then(|| family_id.unwrap_or_else(Uuid::now_v7));
        let access_row = access_token::Model {
            id: access_id,
            token_hash: digest(&access),
            client_id: issue.client.id,
            user_id: issue.user_id,
            scopes: join_scopes(&issue.scopes),
            grant: issue.grant.as_str().to_owned(),
            code_id: issue.code_id,
            family_id: refresh_family.or(family_id),
            workspace_id: issue.client.workspace_id,
            expires_at: core.after(ttl),
            revoked_at: None,
            created_at: now,
        };
        access_token::Model::insert(uow, [access_row]).await?;
        let refresh = match refresh_family {
            Some(family) => {
                let token = new_token(REFRESH_TOKEN_PREFIX);
                let grant_scopes = issue
                    .family
                    .as_ref()
                    .map_or_else(|| issue.scopes.clone(), |(_, scopes)| scopes.clone());
                let refresh_row = refresh_token::Model {
                    id: Uuid::now_v7(),
                    token_hash: digest(&token),
                    client_id: issue.client.id,
                    user_id: issue.user_id,
                    scopes: join_scopes(&grant_scopes),
                    family_id: family,
                    code_id: issue.code_id,
                    access_token_id: Some(access_id),
                    workspace_id: issue.client.workspace_id,
                    expires_at: core.after(core.options.refresh_token_ttl_secs),
                    used_at: None,
                    revoked_at: None,
                    created_at: now,
                };
                refresh_token::Model::insert(uow, [refresh_row]).await?;
                Some(token)
            }
            None => None,
        };
        let id_token = if issue.scopes.iter().any(|s| s == OPENID) {
            Some(
                self.id_token(issue.client, issue.user_id, &issue.scopes, issue.nonce, now)
                    .await?,
            )
        } else {
            None
        };
        Ok(TokenResponse {
            access_token: access,
            token_type: "Bearer".to_owned(),
            expires_in: ttl,
            refresh_token: refresh,
            scope: join_scopes(&issue.scopes),
            id_token,
        })
    }

    /// The claims of `user_id` for `scopes` (`sub`, plus `name` with
    /// `profile` and `email`/`email_verified` with `email`).
    pub(crate) async fn user_claims(
        &self,
        user_id: Uuid,
        scopes: &[String],
    ) -> Result<serde_json::Map<String, Value>, OAuthError> {
        let mut claims = serde_json::Map::new();
        claims.insert("sub".into(), json!(user_id));
        let wants_profile = scopes.iter().any(|s| s == PROFILE);
        let wants_email = scopes.iter().any(|s| s == EMAIL);
        if !wants_profile && !wants_email {
            return Ok(claims);
        }
        let auth = match self.core.app() {
            Some(app) => app.try_get::<AuthService>().await,
            None => None,
        };
        let Some(auth) = auth else {
            return Ok(claims);
        };
        let user = auth
            .find_user(user_id)
            .await
            .map_err(|error| OAuthError::Other(error.into()))?
            .ok_or_else(|| OAuthError::InvalidGrant("The user no longer exists".into()))?;
        if wants_profile {
            claims.insert("name".into(), json!(user.name));
        }
        if wants_email {
            claims.insert("email".into(), json!(user.email));
            claims.insert(
                "email_verified".into(),
                json!(user.email_verified_at.is_some()),
            );
        }
        Ok(claims)
    }

    async fn id_token(
        &self,
        client: &Client,
        user_id: Uuid,
        scopes: &[String],
        nonce: Option<String>,
        now: DateTime<Utc>,
    ) -> Result<String, OAuthError> {
        let mut claims = self.user_claims(user_id, scopes).await?;
        claims.insert("iss".into(), json!(self.core.issuer));
        claims.insert("aud".into(), json!(client.id));
        claims.insert("iat".into(), json!(now.timestamp()));
        claims.insert(
            "exp".into(),
            json!(
                self.core
                    .after(self.core.options.access_token_ttl_secs)
                    .timestamp()
            ),
        );
        if let Some(nonce) = nonce {
            claims.insert("nonce".into(), json!(nonce));
        }
        Ok(self.core.keys.sign(&Value::Object(claims)))
    }

    /// `grant_type=authorization_code`.
    pub(crate) async fn exchange_code(
        &self,
        uow: &Uow,
        client: &Client,
        code: &str,
        redirect_uri: Option<&str>,
        verifier: Option<&str>,
    ) -> Result<TokenResponse, OAuthError> {
        if !client.allows(GRANT_AUTHORIZATION_CODE) {
            return Err(OAuthError::UnauthorizedClient(
                "This client may not use the authorization code grant".into(),
            ));
        }
        let row = code::Model::query()
            .where_(code::Model::CODE_HASH.eq(digest(code)))
            .first(uow)
            .await?
            .ok_or_else(|| OAuthError::InvalidGrant("The authorization code is invalid".into()))?;
        if row.client_id != client.id {
            return Err(OAuthError::InvalidGrant(
                "The authorization code was issued to another client".into(),
            ));
        }
        let now = self.core.now();
        if row.used_at.is_some() {
            self.revoke_code_tokens(uow, row.id, now).await?;
            return Err(OAuthError::InvalidGrant(
                "The authorization code was already used".into(),
            ));
        }
        if row.expires_at <= now {
            return Err(OAuthError::InvalidGrant(
                "The authorization code has expired".into(),
            ));
        }
        if let Some(uri) = redirect_uri
            && !redirect_uri_matches(&row.redirect_uri, uri)
        {
            return Err(OAuthError::InvalidGrant(
                "redirect_uri does not match the authorization request".into(),
            ));
        }
        let verifier = verifier
            .ok_or_else(|| OAuthError::InvalidRequest("code_verifier is required (PKCE)".into()))?;
        if !pkce_matches(verifier, &row.code_challenge) {
            return Err(OAuthError::InvalidGrant(
                "code_verifier does not match the code challenge".into(),
            ));
        }
        let claimed = code::Model::query()
            .where_key(row.id)
            .where_null(code::Model::USED_AT)
            .update(uow, [code::Model::USED_AT.set(now)])
            .await?;
        if claimed != 1 {
            self.revoke_code_tokens(uow, row.id, now).await?;
            return Err(OAuthError::InvalidGrant(
                "The authorization code was already used".into(),
            ));
        }
        let scopes = parse_scope(&row.scopes);
        self.issue(
            uow,
            Issue {
                client,
                user_id: row.user_id,
                refresh: wants_refresh(&scopes),
                scopes,
                grant: Grant::AuthorizationCode,
                code_id: Some(row.id),
                family: None,
                nonce: row.nonce.clone(),
            },
        )
        .await
    }

    /// Revokes every token issued from authorization code `code_id` (it
    /// was presented twice).
    async fn revoke_code_tokens(
        &self,
        uow: &Uow,
        code_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<(), OAuthError> {
        access_token::Model::query()
            .where_(access_token::Model::CODE_ID.eq(code_id))
            .where_null(access_token::Model::REVOKED_AT)
            .update(uow, [access_token::Model::REVOKED_AT.set(now)])
            .await?;
        refresh_token::Model::query()
            .where_(refresh_token::Model::CODE_ID.eq(code_id))
            .where_null(refresh_token::Model::REVOKED_AT)
            .update(uow, [refresh_token::Model::REVOKED_AT.set(now)])
            .await?;
        self.core.forget_all();
        tracing::warn!(target: "navree::security", %code_id, "an authorization code was presented twice; its tokens were revoked");
        Ok(())
    }

    /// `grant_type=client_credentials`: a token acting as the client's
    /// owner, narrowed to the requested scopes (default: the client's).
    pub(crate) async fn client_credentials(
        &self,
        uow: &Uow,
        client: &Client,
        scope: Option<&str>,
    ) -> Result<TokenResponse, OAuthError> {
        if !client.allows(GRANT_CLIENT_CREDENTIALS) || client.is_public() {
            return Err(OAuthError::UnauthorizedClient(
                "This client may not use the client credentials grant".into(),
            ));
        }
        let owner = client.owner_id.ok_or_else(|| {
            OAuthError::UnauthorizedClient("This client has no owner to act as".into())
        })?;
        let allowed = client.scope_list();
        let scopes: Vec<String> = match scope {
            Some(scope) => parse_scope(scope),
            None => allowed.clone(),
        }
        .into_iter()
        .filter(|s| s != OFFLINE_ACCESS && s != OPENID)
        .collect();
        if !is_subset(&scopes, &allowed) {
            return Err(OAuthError::InvalidScope(
                "The client may not ask for one of these scopes".into(),
            ));
        }
        self.issue(
            uow,
            Issue {
                client,
                user_id: owner,
                scopes,
                grant: Grant::ClientCredentials,
                code_id: None,
                family: None,
                refresh: false,
                nonce: None,
            },
        )
        .await
    }

    /// `grant_type=urn:ietf:params:oauth:grant-type:device_code`.
    pub(crate) async fn device_code(
        &self,
        uow: &Uow,
        client: &Client,
        device_code: &str,
    ) -> Result<TokenResponse, OAuthError> {
        if !client.allows(GRANT_DEVICE_CODE) {
            return Err(OAuthError::UnauthorizedClient(
                "This client may not use the device authorization grant".into(),
            ));
        }
        let row = device_code::Model::query()
            .where_(device_code::Model::DEVICE_CODE_HASH.eq(digest(device_code)))
            .first(uow)
            .await?
            .map(Record::into_inner)
            .filter(|row| row.client_id == client.id)
            .ok_or_else(|| OAuthError::InvalidGrant("The device code is invalid".into()))?;
        let now = self.core.now();
        if row.used_at.is_some() {
            return Err(OAuthError::InvalidGrant(
                "The device code was already used".into(),
            ));
        }
        if row.expires_at <= now {
            return Err(OAuthError::ExpiredToken);
        }
        let interval = row.interval_secs;
        let too_fast = row
            .last_polled_at
            .is_some_and(|last| (now - last).num_seconds() < interval);
        // Only the polling columns change, in statements of their own: the
        // device code's digest never reaches a change event.
        let polled = device_code::Model::query().where_key(row.id);
        if too_fast {
            // RFC 8628 §3.5: every slow_down adds five seconds.
            polled
                .update(
                    uow,
                    [
                        device_code::Model::LAST_POLLED_AT.set(now),
                        device_code::Model::INTERVAL_SECS.set(interval + 5),
                    ],
                )
                .await?;
            return Err(OAuthError::SlowDown);
        }
        match row.status.as_str() {
            "approved" => {}
            "denied" => {
                polled
                    .update(
                        uow,
                        [
                            device_code::Model::LAST_POLLED_AT.set(now),
                            device_code::Model::USED_AT.set(now),
                        ],
                    )
                    .await?;
                return Err(OAuthError::AccessDenied(
                    "The user denied the request".into(),
                ));
            }
            _ => {
                polled
                    .update(uow, [device_code::Model::LAST_POLLED_AT.set(now)])
                    .await?;
                return Err(OAuthError::AuthorizationPending);
            }
        }
        let user_id = row
            .user_id
            .ok_or_else(|| OAuthError::InvalidGrant("The device code is invalid".into()))?;
        polled
            .update(
                uow,
                [
                    device_code::Model::LAST_POLLED_AT.set(now),
                    device_code::Model::USED_AT.set(now),
                ],
            )
            .await?;
        let scopes = parse_scope(&row.scopes);
        self.issue(
            uow,
            Issue {
                client,
                user_id,
                refresh: wants_refresh(&scopes),
                scopes,
                grant: Grant::DeviceCode,
                code_id: None,
                family: None,
                nonce: None,
            },
        )
        .await
    }

    /// `grant_type=refresh_token`: single use, rotated; a used token that
    /// comes back revokes its family.
    pub(crate) async fn refresh(
        &self,
        uow: &Uow,
        client: &Client,
        token: &str,
        scope: Option<&str>,
    ) -> Result<TokenResponse, OAuthError> {
        let invalid = || OAuthError::InvalidGrant("The refresh token is invalid".into());
        let row = refresh_token::Model::query()
            .where_(refresh_token::Model::TOKEN_HASH.eq(digest(token)))
            .first(uow)
            .await?
            .filter(|row| row.client_id == client.id)
            .ok_or_else(invalid)?;
        let now = self.core.now();
        if row.revoked_at.is_some() {
            return Err(invalid());
        }
        if row.used_at.is_some() {
            self.revoke_family(uow, row.family_id, now).await?;
            self.core
                .audit(
                    AuditEntry::new(AuditKind::Security, "oauth.refresh_reuse_detected")
                        .actor(row.user_id)
                        .subject("OAuthClient", client.id.to_string())
                        .metadata(json!({ "client_id": client.id, "family_id": row.family_id })),
                )
                .await;
            tracing::warn!(target: "navree::security", client_id = %client.id, family_id = %row.family_id, "a used refresh token came back; its family was revoked");
            return Err(invalid());
        }
        if row.expires_at <= now {
            return Err(OAuthError::InvalidGrant(
                "The refresh token has expired".into(),
            ));
        }
        let granted = parse_scope(&row.scopes);
        let scopes = match scope {
            Some(scope) => {
                let requested = parse_scope(scope);
                if !is_subset(&requested, &granted) {
                    return Err(OAuthError::InvalidScope(
                        "A refresh can't ask for scopes the original grant didn't have".into(),
                    ));
                }
                requested
            }
            None => granted.clone(),
        };
        let claimed = refresh_token::Model::query()
            .where_key(row.id)
            .where_null(refresh_token::Model::USED_AT)
            .update(uow, [refresh_token::Model::USED_AT.set(now)])
            .await?;
        if claimed != 1 {
            self.revoke_family(uow, row.family_id, now).await?;
            return Err(invalid());
        }
        self.issue(
            uow,
            Issue {
                client,
                user_id: row.user_id,
                scopes,
                grant: Grant::RefreshToken,
                code_id: row.code_id,
                family: Some((row.family_id, granted)),
                refresh: true,
                nonce: None,
            },
        )
        .await
    }

    /// Revokes refresh family `family_id` and its access tokens.
    pub(crate) async fn revoke_family<E: Executor + ?Sized>(
        &self,
        exec: &E,
        family_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<(), OAuthError> {
        refresh_token::Model::query()
            .where_(refresh_token::Model::FAMILY_ID.eq(family_id))
            .where_null(refresh_token::Model::REVOKED_AT)
            .update(exec, [refresh_token::Model::REVOKED_AT.set(now)])
            .await?;
        access_token::Model::query()
            .where_(access_token::Model::FAMILY_ID.eq(family_id))
            .where_null(access_token::Model::REVOKED_AT)
            .update(exec, [access_token::Model::REVOKED_AT.set(now)])
            .await?;
        self.core.forget_all();
        Ok(())
    }

    /// RFC 7009: revokes `token` when `client` holds it (a refresh token
    /// with its whole family). Unknown tokens and other clients' tokens are
    /// ignored.
    pub(crate) async fn revoke(
        &self,
        uow: &Uow,
        client: &Client,
        token: &str,
    ) -> Result<(), OAuthError> {
        let hash = digest(token);
        let now = self.core.now();
        if token.starts_with(REFRESH_TOKEN_PREFIX) {
            if let Some(row) = refresh_token::Model::query()
                .where_(refresh_token::Model::TOKEN_HASH.eq(hash.as_str()))
                .first(uow)
                .await?
                .filter(|row| row.client_id == client.id)
            {
                self.revoke_family(uow, row.family_id, now).await?;
            }
        } else if token.starts_with(ACCESS_TOKEN_PREFIX)
            && let Some(row) = access_token::Model::query()
                .where_(access_token::Model::TOKEN_HASH.eq(hash.as_str()))
                .first(uow)
                .await?
                .filter(|row| row.client_id == client.id && row.revoked_at.is_none())
        {
            access_token::Model::query()
                .where_key(row.id)
                .update(uow, [access_token::Model::REVOKED_AT.set(now)])
                .await?;
            self.core.forget([hash]);
        }
        Ok(())
    }

    /// RFC 7662: what an introspecting client learns about `token`.
    pub(crate) async fn introspect(&self, token: &str) -> Result<Value, OAuthError> {
        let inactive = || json!({ "active": false });
        let hash = digest(token);
        let now = self.core.now();
        let db = &self.core.db;
        let (user_id, client_id, scopes, exp, iat, token_type) =
            if token.starts_with(ACCESS_TOKEN_PREFIX) {
                let Some(row) = access_token::Model::query()
                    .where_(access_token::Model::TOKEN_HASH.eq(hash.as_str()))
                    .first(db)
                    .await?
                    .map(Record::into_inner)
                    .filter(|row| row.revoked_at.is_none() && row.expires_at > now)
                else {
                    return Ok(inactive());
                };
                (
                    row.user_id,
                    Some(row.client_id),
                    row.scopes,
                    Some(row.expires_at),
                    row.created_at,
                    "Bearer",
                )
            } else if token.starts_with(REFRESH_TOKEN_PREFIX) {
                let Some(row) = refresh_token::Model::query()
                    .where_(refresh_token::Model::TOKEN_HASH.eq(hash.as_str()))
                    .first(db)
                    .await?
                    .map(Record::into_inner)
                    .filter(|row| {
                        row.revoked_at.is_none() && row.used_at.is_none() && row.expires_at > now
                    })
                else {
                    return Ok(inactive());
                };
                (
                    row.user_id,
                    Some(row.client_id),
                    row.scopes,
                    Some(row.expires_at),
                    row.created_at,
                    "refresh_token",
                )
            } else if token.starts_with(PERSONAL_ACCESS_TOKEN_PREFIX) {
                let Some(row) = personal_access_token::Model::query()
                    .where_(personal_access_token::Model::TOKEN_HASH.eq(hash.as_str()))
                    .first(db)
                    .await?
                    .map(Record::into_inner)
                    .filter(|row| {
                        row.revoked_at.is_none() && row.expires_at.is_none_or(|exp| exp > now)
                    })
                else {
                    return Ok(inactive());
                };
                (
                    row.user_id,
                    None,
                    row.scopes,
                    row.expires_at,
                    row.created_at,
                    "Bearer",
                )
            } else {
                return Ok(inactive());
            };
        if let Some(client_id) = client_id {
            let client = client::Model::find(db, client_id).await?;
            if client.is_none_or(|client| client.is_revoked()) {
                return Ok(inactive());
            }
        }
        let mut body = json!({
            "active": true,
            "scope": scopes,
            "sub": user_id,
            "iss": self.core.issuer,
            "iat": iat.timestamp(),
            "token_type": token_type,
        });
        if let Some(exp) = exp {
            body["exp"] = json!(exp.timestamp());
        }
        if let Some(client_id) = client_id {
            body["client_id"] = json!(client_id);
            body["aud"] = json!(client_id);
        }
        if let Some(app) = self.core.app()
            && let Some(auth) = app.try_get::<AuthService>().await
            && let Ok(Some(user)) = auth.find_user(user_id).await
        {
            body["username"] = json!(user.email);
        }
        Ok(body)
    }

    /// The valid OAuth access token or personal access token `token`, for
    /// the authentication strategy (cached per instance).
    pub(crate) async fn lookup(&self, token: &str) -> Result<Option<CachedToken>, OAuthError> {
        let hash = digest(token);
        let found = match &self.core.cache {
            Some(cache) => match cache.get(&hash) {
                Some(found) => Some(found),
                None => {
                    let loaded = self.load(token, &hash).await?;
                    if let Some(found) = &loaded {
                        cache.insert(hash.clone(), found.clone());
                    }
                    loaded
                }
            },
            None => self.load(token, &hash).await?,
        };
        let now = self.core.now();
        Ok(found.filter(|found| found.expires_at.is_none_or(|exp| exp > now)))
    }

    async fn load(&self, token: &str, hash: &str) -> Result<Option<CachedToken>, OAuthError> {
        let db = &self.core.db;
        if token.starts_with(ACCESS_TOKEN_PREFIX) {
            let Some(row) = access_token::Model::query()
                .where_(access_token::Model::TOKEN_HASH.eq(hash))
                .first(db)
                .await?
                .filter(|row| row.revoked_at.is_none())
            else {
                return Ok(None);
            };
            let Some(client) = client::Model::find(db, row.client_id)
                .await?
                .map(Record::into_inner)
                .filter(|client| !client.is_revoked())
            else {
                return Ok(None);
            };
            return Ok(Some(CachedToken {
                token_id: row.id,
                user_id: row.user_id,
                client_id: Some(client.id),
                client_name: client.name,
                scopes: parse_scope(&row.scopes),
                grant: Grant::parse(&row.grant).unwrap_or(Grant::AuthorizationCode),
                workspace_id: row.workspace_id,
                expires_at: Some(row.expires_at),
                last_used_at: None,
            }));
        }
        if token.starts_with(PERSONAL_ACCESS_TOKEN_PREFIX) {
            let Some(row) = personal_access_token::Model::query()
                .where_(personal_access_token::Model::TOKEN_HASH.eq(hash))
                .first(db)
                .await?
                .filter(|row| row.revoked_at.is_none())
            else {
                return Ok(None);
            };
            return Ok(Some(CachedToken {
                token_id: row.id,
                user_id: row.user_id,
                client_id: None,
                client_name: row.name.clone(),
                scopes: row.scope_list(),
                grant: Grant::PersonalAccessToken,
                workspace_id: None,
                expires_at: row.expires_at,
                last_used_at: row.last_used_at,
            }));
        }
        Ok(None)
    }
}

impl fmt::Debug for Grants {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Grants").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grants_print_and_parse() {
        for grant in [
            Grant::AuthorizationCode,
            Grant::ClientCredentials,
            Grant::DeviceCode,
            Grant::RefreshToken,
            Grant::PersonalAccessToken,
        ] {
            assert_eq!(Grant::parse(grant.as_str()), Some(grant));
            assert_eq!(grant.to_string(), grant.as_str());
        }
        assert_eq!(Grant::parse("password"), None);
    }

    #[test]
    fn token_responses_hide_secrets_in_debug() {
        let response = TokenResponse {
            access_token: "nva_secret".into(),
            token_type: "Bearer".into(),
            expires_in: 3600,
            refresh_token: Some("nvr_secret".into()),
            scope: "deals:read".into(),
            id_token: None,
        };
        let debug = format!("{response:?}");
        assert!(!debug.contains("secret"), "{debug}");
        let json = serde_json::to_value(&response).unwrap();
        assert_eq!(json["refresh_token"], "nvr_secret");
        assert!(json.get("id_token").is_none());
    }
}
