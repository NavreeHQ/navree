//! [`OAuthClients`]: registering, listing, rotating and revoking clients,
//! and authenticating them at the protocol endpoints.

use std::fmt;
use std::sync::Arc;

use navree_contracts::{AuditEntry, AuditKind};
use navree_crypto::constant_time_eq;
use navree_db::Uow;
use navree_orm::{Model as _, Record};
use serde_json::json;
use uuid::Uuid;

use crate::core::Core;
use crate::entity::{Client, access_token, refresh_token};
use crate::error::OAuthError;
use crate::scope::{OFFLINE_ACCESS, join_scopes};
use crate::token::{CLIENT_SECRET_PREFIX, check_redirect_uri, digest, new_token};

/// The `authorization_code` grant.
pub const GRANT_AUTHORIZATION_CODE: &str = "authorization_code";
/// The `client_credentials` grant.
pub const GRANT_CLIENT_CREDENTIALS: &str = "client_credentials";
/// The `refresh_token` grant.
pub const GRANT_REFRESH_TOKEN: &str = "refresh_token";
/// The device authorization grant (RFC 8628).
pub const GRANT_DEVICE_CODE: &str = "urn:ietf:params:oauth:grant-type:device_code";

/// The longest client name.
pub const MAX_CLIENT_NAME_LEN: usize = 255;

/// A client to register: build it with [`NewClient::confidential`] or
/// [`NewClient::public`] and the setters, then pass it to
/// [`OAuthClients::create`].
///
/// A client with redirect URIs (or no other grant) gets the
/// `authorization_code` grant; [`client_credentials`](Self::client_credentials),
/// [`device`](Self::device) and [`grant`](Self::grant) add others. Refresh
/// tokens come with the `offline_access` scope.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NewClient {
    /// The name shown on the consent page.
    pub name: String,
    /// Whether it is public (no secret; PKCE only).
    pub public: bool,
    /// The allowed redirect URIs.
    pub redirect_uris: Vec<String>,
    /// The scopes it may ask for.
    pub scopes: Vec<String>,
    /// The grants it may use; empty means `authorization_code`.
    pub grants: Vec<String>,
    /// The user a client credentials client acts as (or the developer who
    /// registered it).
    pub owner_id: Option<Uuid>,
    /// A first-party client: no consent screen.
    pub trusted: bool,
    /// Whether it may call the introspection endpoint.
    pub introspect: bool,
    /// The workspace its tokens are limited to.
    pub workspace_id: Option<Uuid>,
    /// A logo for the consent page.
    pub logo_url: Option<String>,
    /// The client's website.
    pub website: Option<String>,
}

impl NewClient {
    /// A confidential client (a server-side app with a secret).
    pub fn confidential(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }

    /// A public client (mobile, single-page or desktop app): no secret,
    /// PKCE only.
    pub fn public(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            public: true,
            ..Self::default()
        }
    }

    /// Adds an allowed redirect URI.
    pub fn redirect_uri(mut self, uri: impl Into<String>) -> Self {
        self.redirect_uris.push(uri.into());
        self
    }

    /// Sets the scopes it may ask for.
    pub fn scopes<I, S>(mut self, scopes: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.scopes = scopes.into_iter().map(Into::into).collect();
        self
    }

    /// Adds a grant (`authorization_code`, `client_credentials` or
    /// [`GRANT_DEVICE_CODE`]; `device_code` is accepted for the latter).
    pub fn grant(mut self, grant: impl Into<String>) -> Self {
        let grant = grant.into();
        let grant = if grant == "device_code" {
            GRANT_DEVICE_CODE.to_owned()
        } else {
            grant
        };
        if !self.grants.contains(&grant) {
            self.grants.push(grant);
        }
        self
    }

    /// A machine-to-machine client: the `client_credentials` grant, acting
    /// as its [`owner`](Self::owner).
    pub fn client_credentials(self) -> Self {
        self.grant(GRANT_CLIENT_CREDENTIALS)
    }

    /// Allows the device authorization grant.
    pub fn device(self) -> Self {
        self.grant(GRANT_DEVICE_CODE)
    }

    /// Sets the owner.
    pub fn owner(mut self, user_id: Uuid) -> Self {
        self.owner_id = Some(user_id);
        self
    }

    /// Makes it a first-party client: no consent screen.
    pub fn trusted(mut self) -> Self {
        self.trusted = true;
        self
    }

    /// Lets it call `POST /oauth/introspect`.
    pub fn introspect(mut self) -> Self {
        self.introspect = true;
        self
    }

    /// Limits its tokens to one workspace.
    pub fn workspace(mut self, workspace_id: Uuid) -> Self {
        self.workspace_id = Some(workspace_id);
        self
    }

    /// Sets the logo URL.
    pub fn logo_url(mut self, url: impl Into<String>) -> Self {
        self.logo_url = Some(url.into());
        self
    }

    /// Sets the website.
    pub fn website(mut self, url: impl Into<String>) -> Self {
        self.website = Some(url.into());
        self
    }

    /// The grants it gets: the ones named, plus `authorization_code` when
    /// it has redirect URIs, or no other grant and isn't an
    /// introspection-only client.
    fn effective_grants(&self) -> Vec<String> {
        let mut grants = self.grants.clone();
        let code = GRANT_AUTHORIZATION_CODE.to_owned();
        let only_introspects = self.introspect && grants.is_empty();
        if !grants.contains(&code)
            && (!self.redirect_uris.is_empty() || (grants.is_empty() && !only_introspects))
        {
            grants.insert(0, code);
        }
        grants
    }
}

/// A client just created: the record and, for a confidential client, its
/// secret (shown once; only its digest is stored).
#[derive(Clone)]
pub struct CreatedClient {
    /// The client.
    pub client: Client,
    /// The secret (`nvcs_...`); `None` for public clients.
    pub secret: Option<String>,
}

impl fmt::Debug for CreatedClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CreatedClient")
            .field("client", &self.client)
            .field("secret", &self.secret.as_ref().map(|_| "[redacted]"))
            .finish()
    }
}

/// How a client presented itself at a protocol endpoint. `Debug` never
/// shows the secret.
#[derive(Clone, Default)]
pub(crate) struct ClientCredentials {
    pub(crate) client_id: Option<String>,
    pub(crate) client_secret: Option<String>,
    /// Whether they came in an `Authorization: Basic` header.
    pub(crate) basic: bool,
}

impl fmt::Debug for ClientCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientCredentials")
            .field("client_id", &self.client_id)
            .field(
                "client_secret",
                &self.client_secret.as_ref().map(|_| "[redacted]"),
            )
            .field("basic", &self.basic)
            .finish()
    }
}

/// Registers and manages OAuth clients. Provided and exported by
/// `OAuthServerModule`.
///
/// ```ignore
/// let created = clients
///     .create(
///         &uow,
///         NewClient::confidential("Anu's reporting script")
///             .owner(user.id)
///             .redirect_uri("https://reports.lakhotia.io/callback")
///             .scopes(["deals:read"]),
///     )
///     .await?;
/// // created.client.id, and created.secret (shown once)
/// ```
pub struct OAuthClients {
    core: Arc<Core>,
}

impl OAuthClients {
    pub(crate) fn new(core: Arc<Core>) -> Self {
        Self { core }
    }

    /// Registers `new`. Every problem is an [`OAuthError::Invalid`]
    /// naming the field: an empty name, an invalid redirect URI (an
    /// absolute URI; `https`, loopback `http` or a reverse-domain scheme),
    /// an unknown scope, an unknown grant, a client credentials client
    /// that is public or has no owner, an authorization code client
    /// without a redirect URI. Audited as `oauth.client_created`.
    pub async fn create(&self, uow: &Uow, new: NewClient) -> Result<CreatedClient, OAuthError> {
        self.check(&new)?;
        let now = self.core.now();
        let secret = (!new.public).then(|| new_token(CLIENT_SECRET_PREFIX));
        let model = Client {
            id: Uuid::now_v7(),
            name: new.name.trim().to_owned(),
            secret_hash: secret.as_deref().map(digest),
            redirect_uris: new.redirect_uris.join(" "),
            scopes: join_scopes(&dedup(&new.scopes)),
            grants: new.effective_grants().join(" "),
            owner_id: new.owner_id,
            trusted: new.trusted,
            introspect: new.introspect,
            workspace_id: new.workspace_id,
            logo_url: new.logo_url.clone(),
            website: new.website.clone(),
            created_at: now,
            updated_at: now,
            revoked_at: None,
        };
        Client::insert(uow, [model.clone()]).await?;
        let mut entry = AuditEntry::new(AuditKind::Security, "oauth.client_created")
            .subject("OAuthClient", model.id.to_string())
            .metadata(json!({
                "client_id": model.id,
                "name": model.name,
                "public": model.is_public(),
                "scopes": model.scope_list(),
                "grants": model.grant_list(),
            }));
        if let Some(actor) = uow.actor().or(model.owner_id) {
            entry = entry.actor(actor);
        }
        self.core.audit_after_commit(uow, entry).await;
        Ok(CreatedClient {
            client: model,
            secret,
        })
    }

    fn check(&self, new: &NewClient) -> Result<(), OAuthError> {
        let name = new.name.trim();
        if name.is_empty() {
            return Err(OAuthError::invalid("name", "name should not be empty"));
        }
        if name.chars().count() > MAX_CLIENT_NAME_LEN {
            return Err(OAuthError::invalid(
                "name",
                format!("name must be at most {MAX_CLIENT_NAME_LEN} characters"),
            ));
        }
        for uri in &new.redirect_uris {
            if let Err(reason) = check_redirect_uri(uri) {
                return Err(OAuthError::invalid(
                    "redirect_uris",
                    format!("redirect URI `{uri}` {reason}"),
                ));
            }
            if uri.contains(' ') {
                return Err(OAuthError::invalid(
                    "redirect_uris",
                    format!("redirect URI `{uri}` must not contain spaces"),
                ));
            }
        }
        if let Some(unknown) = self.core.scopes.unknown(&new.scopes) {
            return Err(OAuthError::invalid(
                "scopes",
                format!("scope `{unknown}` is not defined (oauth.scopes)"),
            ));
        }
        let grants = new.effective_grants();
        for grant in &grants {
            if ![
                GRANT_AUTHORIZATION_CODE,
                GRANT_CLIENT_CREDENTIALS,
                GRANT_DEVICE_CODE,
            ]
            .contains(&grant.as_str())
            {
                return Err(OAuthError::invalid(
                    "grants",
                    format!(
                        "grant `{grant}` must be one of authorization_code, client_credentials, device_code"
                    ),
                ));
            }
        }
        if grants.iter().any(|g| g == GRANT_AUTHORIZATION_CODE) && new.redirect_uris.is_empty() {
            return Err(OAuthError::invalid(
                "redirect_uris",
                "an authorization code client needs at least one redirect URI",
            ));
        }
        if grants.iter().any(|g| g == GRANT_CLIENT_CREDENTIALS) {
            if new.public {
                return Err(OAuthError::invalid(
                    "grants",
                    "a public client can't use client_credentials",
                ));
            }
            if new.owner_id.is_none() {
                return Err(OAuthError::invalid(
                    "owner",
                    "a client credentials client needs an owner (the user it acts as)",
                ));
            }
        }
        for (field, url) in [("logo_url", &new.logo_url), ("website", &new.website)] {
            if let Some(url) = url
                && !crate::config::is_http_url(url)
            {
                return Err(OAuthError::invalid(
                    field,
                    format!("{field} must be an absolute http(s) URL"),
                ));
            }
        }
        Ok(())
    }

    /// The client with `id` (revoked ones included).
    pub async fn find(&self, id: Uuid) -> Result<Option<Client>, OAuthError> {
        Ok(Client::find(&self.core.db, id)
            .await?
            .map(Record::into_inner))
    }

    /// Every client, oldest first (revoked ones included).
    pub async fn list(&self) -> Result<Vec<Client>, OAuthError> {
        Ok(Client::query()
            .order_by(Client::CREATED_AT)
            .get(&self.core.db)
            .await?
            .into_models())
    }

    /// The clients `user_id` owns (a developer portal's list).
    pub async fn owned_by(&self, user_id: Uuid) -> Result<Vec<Client>, OAuthError> {
        Ok(Client::query()
            .where_(Client::OWNER_ID.eq(user_id))
            .where_null(Client::REVOKED_AT)
            .order_by(Client::CREATED_AT)
            .get(&self.core.db)
            .await?
            .into_models())
    }

    /// Gives confidential client `id` a new secret (the old one stops
    /// working at once) and returns it. 404 for an unknown or revoked
    /// client, 400 for a public one.
    pub async fn rotate_secret(&self, uow: &Uow, id: Uuid) -> Result<String, OAuthError> {
        let client = self.active(uow, id).await?;
        if client.is_public() {
            return Err(OAuthError::invalid(
                "client",
                "a public client has no secret",
            ));
        }
        let secret = new_token(CLIENT_SECRET_PREFIX);
        Client::query()
            .where_key(client.id)
            .update(
                uow,
                [
                    Client::SECRET_HASH.set(digest(&secret)),
                    Client::UPDATED_AT.set(self.core.now()),
                ],
            )
            .await?;
        Ok(secret)
    }

    /// Revokes client `id` and every token it holds. Audited as
    /// `oauth.client_revoked`.
    pub async fn revoke(&self, uow: &Uow, id: Uuid) -> Result<(), OAuthError> {
        let client = self.active(uow, id).await?;
        let now = self.core.now();
        Client::query()
            .where_key(id)
            .update(
                uow,
                [Client::REVOKED_AT.set(now), Client::UPDATED_AT.set(now)],
            )
            .await?;
        access_token::Model::query()
            .where_(access_token::Model::CLIENT_ID.eq(id))
            .where_null(access_token::Model::REVOKED_AT)
            .update(uow, [access_token::Model::REVOKED_AT.set(now)])
            .await?;
        refresh_token::Model::query()
            .where_(refresh_token::Model::CLIENT_ID.eq(id))
            .where_null(refresh_token::Model::REVOKED_AT)
            .update(uow, [refresh_token::Model::REVOKED_AT.set(now)])
            .await?;
        let core = self.core.clone();
        uow.after_commit(move || {
            Box::pin(async move {
                core.forget_all();
            })
        });
        let mut entry = AuditEntry::new(AuditKind::Security, "oauth.client_revoked")
            .subject("OAuthClient", id.to_string())
            .metadata(json!({ "client_id": id, "name": client.name }));
        if let Some(actor) = uow.actor() {
            entry = entry.actor(actor);
        }
        self.core.audit_after_commit(uow, entry).await;
        Ok(())
    }

    /// The client `id`, unless unknown or revoked (404).
    async fn active(&self, uow: &Uow, id: Uuid) -> Result<Client, OAuthError> {
        Client::find(uow, id)
            .await?
            .map(Record::into_inner)
            .filter(|client| !client.is_revoked())
            .ok_or(OAuthError::NotFound("OAuth client not found."))
    }

    /// The client that `credentials` name, authenticated: a confidential
    /// client must present its secret; a public client presents its id
    /// alone. `invalid_client` otherwise.
    pub(crate) async fn authenticate(
        &self,
        credentials: &ClientCredentials,
    ) -> Result<Client, OAuthError> {
        let invalid = || OAuthError::InvalidClient("Client authentication failed".into());
        let id = credentials
            .client_id
            .as_deref()
            .ok_or_else(|| OAuthError::InvalidClient("The request names no client".into()))?;
        let id = Uuid::parse_str(id).map_err(|_| invalid())?;
        let client = self
            .find(id)
            .await?
            .filter(|client| !client.is_revoked())
            .ok_or_else(invalid)?;
        match (&client.secret_hash, &credentials.client_secret) {
            (None, None) => Ok(client),
            (Some(stored), Some(secret)) => {
                if constant_time_eq(stored.as_bytes(), digest(secret).as_bytes()) {
                    Ok(client)
                } else {
                    Err(invalid())
                }
            }
            // A public client must not claim a secret; a confidential one
            // must present it.
            _ => Err(invalid()),
        }
    }
}

impl fmt::Debug for OAuthClients {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OAuthClients").finish_non_exhaustive()
    }
}

/// `scopes` without duplicates, in order.
fn dedup(scopes: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(scopes.len());
    for scope in scopes {
        if !out.contains(scope) {
            out.push(scope.clone());
        }
    }
    out
}

/// Whether `scopes` ask for a refresh token.
pub(crate) fn wants_refresh(scopes: &[String]) -> bool {
    scopes.iter().any(|s| s == OFFLINE_ACCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presented_credentials_never_show_the_secret_in_debug_output() {
        let credentials = ClientCredentials {
            client_id: Some("crm-web".to_owned()),
            client_secret: Some("nvcs_f00dfeed".to_owned()),
            basic: true,
        };
        let printed = format!("{credentials:?}");
        assert!(!printed.contains("f00dfeed"), "{printed}");
        assert!(printed.contains("crm-web"), "{printed}");
    }
}
