//! [`Sso`]: connections, verified domains, discovery and the sign-in flows.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use chrono::{DateTime, TimeDelta, Utc};
use navree_auth::{AuthService, ClientMeta, User, user};
use navree_cache::Cache;
use navree_config::Config;
use navree_contracts::{AuditEntry, AuditKind, AuditSink, Clock, SystemClock};
use navree_crypto::{Encrypter, Hasher, random_token, sha256_hex};
use navree_db::{Db, Repo, Uow};
use navree_events::{Event, EventBus};
use navree_http::HttpOptions;
use navree_http_client::{HttpClient, HttpClientOptions};
use navree_kernel::{AppContext, BoxError, WeakAppContext};
use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QueryOrder, Set, Unchanged};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::OnceCell;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::entity::{sso_connection, sso_domain, sso_identity};
use crate::oidc::{self, Authorize, Exchange, IdTokenCheck, OidcCache};
use crate::saml::{
    AuthnRequest, IdpMetadata, RequestOptions, ResponseCheck, SamlError, ServiceProvider,
    validate_response,
};
use crate::{
    ConnectionSettings, ConnectionUpdate, NewConnection, OidcSettings, Protocol, Provisioning,
    SamlSettings, ServiceProviderInfo, SsoConnection, SsoDiscovery, SsoDomain, SsoDomainVerified,
    SsoError, SsoOptions, SsoOwner, SsoProfile, SsoUserProvisioned, TxtResolver,
};

/// Slugs the routes use as path segments.
const RESERVED_SLUGS: &[&str] = &[
    "connections",
    "domains",
    "saml",
    "oidc",
    "discover",
    "exchange",
];

/// Where to send the browser to sign in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct SsoRedirect {
    /// The identity provider's sign-in URL.
    pub url: String,
    /// The sign-in's state: keep it in the browser (session storage) and
    /// send it with `POST /sso/exchange`, so a code from someone else's
    /// sign-in is refused.
    pub state: String,
}

/// A finished sign-in: the user (found, linked or created) and where the
/// front end wanted to go.
#[derive(Clone, Debug)]
pub struct SsoSignIn {
    /// The user.
    pub user: User,
    /// The connection.
    pub connection_id: Uuid,
    /// The relative path given to the login route.
    pub return_to: Option<String>,
    /// Whether this sign-in created the user.
    pub created: bool,
    /// The state of the login that started it (none for an IdP-initiated
    /// sign-in).
    pub state: Option<String>,
}

/// The query of an OpenID Connect callback.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, ToSchema)]
pub struct OidcCallback {
    /// The authorization code.
    #[serde(default)]
    pub code: Option<String>,
    /// The state the login route created.
    #[serde(default)]
    pub state: Option<String>,
    /// The provider's error code (`access_denied`).
    #[serde(default)]
    pub error: Option<String>,
    /// The provider's error description.
    #[serde(default)]
    pub error_description: Option<String>,
}

/// A sign-in in progress, kept in the cache under its random key (the
/// SAML relay state or the OpenID Connect state) until it is used once.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct LoginState {
    connection_id: Uuid,
    request_id: Option<String>,
    nonce: Option<String>,
    verifier: Option<String>,
    return_to: Option<String>,
    exp: i64,
}

/// A finished sign-in waiting for `POST /sso/exchange`.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct PendingCode {
    user_id: Uuid,
    connection_id: Uuid,
    return_to: Option<String>,
    state: Option<String>,
}

/// The IdP metadata of a connection, with when it was read.
struct CachedMetadata {
    metadata: Arc<IdpMetadata>,
    fetched: Instant,
    version: DateTime<Utc>,
}

/// A connection row with its settings and rules decoded (the client secret
/// decrypted).
#[derive(Clone, Debug)]
pub(crate) struct Loaded {
    pub(crate) model: sso_connection::Model,
    pub(crate) owner: SsoOwner,
    pub(crate) settings: ConnectionSettings,
    pub(crate) provisioning: Provisioning,
}

/// Single sign-on: connections per organization or workspace, verified
/// email domains, and the SAML and OpenID Connect sign-in flows that end
/// with navree-auth tokens. Provided and exported by
/// [`SsoModule`](crate::SsoModule).
///
/// - Writes that take a [`Uow`] run in it; a sign-in's writes (the user,
///   the identity, the membership) run in a transaction of their own.
/// - Sign-in states, used assertion ids and one-time codes live in the
///   app's cache (`CacheModule`, or an in-process cache): with several app
///   instances, use a shared cache store.
pub struct Sso {
    options: Arc<SsoOptions>,
    base_url: Option<String>,
    api_prefix: String,
    db: Arc<Db>,
    auth: Arc<AuthService>,
    encrypter: Arc<Encrypter>,
    hasher: Arc<Hasher>,
    cache: Cache,
    clock: Arc<dyn Clock>,
    http: HttpClient,
    txt: Arc<dyn TxtResolver>,
    app: WeakAppContext,
    metadata: Mutex<HashMap<Uuid, CachedMetadata>>,
    oidc: OidcCache,
    no_password: OnceCell<String>,
}

impl std::fmt::Debug for Sso {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sso")
            .field("base_url", &self.base_url)
            .finish_non_exhaustive()
    }
}

fn check_return_to(return_to: &str) -> Result<(), SsoError> {
    let relative = return_to.starts_with('/')
        && !return_to.starts_with("//")
        && !return_to.contains('\\')
        && !return_to.chars().any(char::is_control);
    if relative {
        Ok(())
    } else {
        Err(SsoError::InvalidReturnTo)
    }
}

/// A domain, lower-cased, or a validation error.
fn normalize_domain(domain: &str) -> Result<String, SsoError> {
    let domain = domain.trim().trim_end_matches('.').to_ascii_lowercase();
    let label_ok = |label: &str| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    };
    // The last label is a top-level domain: never all digits (no IP
    // addresses).
    let tld_ok = domain
        .rsplit('.')
        .next()
        .is_some_and(|tld| tld.chars().any(|c| c.is_ascii_lowercase()));
    if domain.len() > 253
        || !domain.contains('.')
        || !domain.split('.').all(label_ok)
        || !tld_ok
    {
        return Err(SsoError::invalid(
            "domain",
            "domain must be a domain name such as acme.com",
        ));
    }
    Ok(domain)
}

/// A slug from a name: lower-case letters, digits and dashes.
fn slugify(name: &str) -> String {
    let mut slug = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
    }
    let slug = slug.trim_end_matches('-');
    let slug: String = slug.chars().take(48).collect();
    if slug.len() < 2 {
        "sso".to_owned()
    } else {
        slug.trim_end_matches('-').to_owned()
    }
}

fn check_slug(slug: &str) -> Result<(), SsoError> {
    let ok = (2..=64).contains(&slug.len())
        && slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !slug.starts_with('-')
        && !slug.ends_with('-')
        && Uuid::parse_str(slug).is_err()
        && !RESERVED_SLUGS.contains(&slug);
    if ok {
        Ok(())
    } else {
        Err(SsoError::invalid(
            "slug",
            "slug must be 2 to 64 lower-case letters, digits and dashes, and not a reserved word",
        ))
    }
}

/// An `https` URL, or `http` to this machine (development, tests).
fn trusted_url(value: &str) -> bool {
    url::Url::parse(value).is_ok_and(|url| match url.scheme() {
        "https" => true,
        "http" => matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")),
        _ => false,
    })
}

impl Sso {
    /// The service of `app` (needs the module's options, auth, the
    /// database and crypto).
    pub(crate) async fn build(app: &AppContext) -> Result<Sso, BoxError> {
        let options = app.get::<SsoOptions>().await?;
        let base_url = match &options.base_url {
            Some(url) => Some(url.clone()),
            None => match app.try_get::<Config>().await {
                Some(config) => config.get_string("app.url"),
                None => None,
            },
        }
        .map(|url| url.trim_end_matches('/').to_owned());
        let api_prefix = match app.try_get::<HttpOptions>().await {
            Some(http) => http.api_prefix.trim_end_matches('/').to_owned(),
            None => "/api".to_owned(),
        };
        let cache = match app.try_get::<Cache>().await {
            Some(cache) => (*cache).clone(),
            None => Cache::memory(),
        };
        let clock: Arc<dyn Clock> = match app.try_get::<dyn Clock>().await {
            Some(clock) => clock,
            None => Arc::new(SystemClock),
        };
        let http = match app.try_get::<HttpClient>().await {
            Some(http) => (*http).clone(),
            None => HttpClient::new(HttpClientOptions::default())?,
        };
        Ok(Sso {
            base_url,
            api_prefix,
            db: app.get::<Db>().await?,
            auth: app.get::<AuthService>().await?,
            encrypter: app.get::<Encrypter>().await?,
            hasher: app.get::<Hasher>().await?,
            cache,
            clock,
            http,
            txt: app.get::<dyn TxtResolver>().await?,
            app: app.downgrade(),
            metadata: Mutex::new(HashMap::new()),
            oidc: OidcCache::default(),
            no_password: OnceCell::new(),
            options,
        })
    }

    /// The module's options.
    pub fn options(&self) -> &SsoOptions {
        &self.options
    }

    fn now(&self) -> DateTime<Utc> {
        self.clock.now()
    }

    // -- URLs ----------------------------------------------------------------

    /// `{api_prefix}{routes_prefix}{path}`.
    fn route(&self, path: &str) -> String {
        format!(
            "{}{}{path}",
            self.api_prefix,
            self.options.routes_prefix.trim_end_matches('/')
        )
    }

    /// The absolute URL of route `path`.
    fn absolute(&self, path: &str) -> Result<String, SsoError> {
        let base = self.base_url.as_deref().ok_or_else(|| {
            SsoError::Config("set sso.base_url or app.url to build single sign-on URLs".to_owned())
        })?;
        Ok(format!("{base}{}", self.route(path)))
    }

    /// The login URL of `slug` (absolute when the base URL is known).
    fn login_url(&self, slug: &str) -> String {
        let path = format!("/{slug}/login");
        self.absolute(&path).unwrap_or_else(|_| self.route(&path))
    }

    /// Navree's side of a SAML connection: the metadata URL is the entity
    /// id; the assertion consumer service takes HTTP-POST.
    fn service_provider(&self, slug: &str) -> Result<ServiceProvider, SsoError> {
        Ok(ServiceProvider {
            entity_id: self.absolute(&format!("/saml/{slug}/metadata"))?,
            acs_url: self.absolute(&format!("/saml/{slug}/acs"))?,
        })
    }

    fn redirect_uri(&self, slug: &str) -> Result<String, SsoError> {
        self.absolute(&format!("/oidc/{slug}/callback"))
    }

    // -- connections ---------------------------------------------------------

    fn decode(&self, model: sso_connection::Model) -> Result<Loaded, SsoError> {
        let owner = SsoOwner::from_parts(&model.owner_type, model.owner_id).ok_or_else(|| {
            SsoError::Config(format!("connection {} has an invalid owner", model.id))
        })?;
        let mut settings: ConnectionSettings = serde_json::from_str(&model.settings)
            .map_err(|e| SsoError::Config(format!("connection {}: {e}", model.id)))?;
        if let ConnectionSettings::Oidc(oidc) = &mut settings
            && let Some(secret) = &oidc.client_secret
        {
            oidc.client_secret = Some(self.encrypter.decrypt_string(secret)?);
        }
        let provisioning = serde_json::from_str(&model.provisioning)
            .map_err(|e| SsoError::Config(format!("connection {}: {e}", model.id)))?;
        Ok(Loaded {
            model,
            owner,
            settings,
            provisioning,
        })
    }

    /// The settings as stored: the client secret encrypted.
    fn encode_settings(&self, settings: &ConnectionSettings) -> Result<String, SsoError> {
        let mut stored = settings.clone();
        if let ConnectionSettings::Oidc(oidc) = &mut stored
            && let Some(secret) = &oidc.client_secret
        {
            oidc.client_secret = Some(self.encrypter.encrypt_string(secret)?);
        }
        // `client_secret` is skipped when serializing the public form, so
        // the stored form is written by hand.
        let mut value =
            serde_json::to_value(&stored).map_err(|e| SsoError::Config(e.to_string()))?;
        if let (ConnectionSettings::Oidc(oidc), Value::Object(fields)) = (&stored, &mut value)
            && let Some(secret) = &oidc.client_secret
        {
            fields.insert("client_secret".to_owned(), Value::String(secret.clone()));
        }
        Ok(value.to_string())
    }

    pub(crate) fn present(&self, loaded: &Loaded) -> SsoConnection {
        let slug = &loaded.model.slug;
        let protocol = loaded.settings.protocol();
        let service_provider = match protocol {
            Protocol::Saml => {
                let sp = self.service_provider(slug).ok();
                ServiceProviderInfo {
                    login_url: self.login_url(slug),
                    entity_id: sp.as_ref().map(|sp| sp.entity_id.clone()),
                    acs_url: sp.as_ref().map(|sp| sp.acs_url.clone()),
                    metadata_url: sp.map(|sp| sp.entity_id),
                    redirect_uri: None,
                }
            }
            Protocol::Oidc => ServiceProviderInfo {
                login_url: self.login_url(slug),
                entity_id: None,
                acs_url: None,
                metadata_url: None,
                redirect_uri: self.redirect_uri(slug).ok(),
            },
        };
        SsoConnection {
            resource_type: "SsoConnection",
            id: loaded.model.id,
            owner: loaded.owner,
            slug: slug.clone(),
            name: loaded.model.name.clone(),
            protocol,
            enabled: loaded.model.enabled,
            enforce: loaded.model.enforce,
            has_client_secret: matches!(
                &loaded.settings,
                ConnectionSettings::Oidc(OidcSettings {
                    client_secret: Some(_),
                    ..
                })
            ),
            settings: loaded.settings.redacted(),
            provisioning: loaded.provisioning.clone(),
            service_provider,
            created_at: loaded.model.created_at,
            updated_at: loaded.model.updated_at,
        }
    }

    /// The connection with id or slug `key`.
    pub(crate) async fn load<C: ConnectionTrait>(
        &self,
        conn: &C,
        key: &str,
    ) -> Result<Loaded, SsoError> {
        let query = match Uuid::parse_str(key) {
            Ok(id) => sso_connection::Entity::find_by_id(id),
            Err(_) => sso_connection::Entity::find()
                .filter(sso_connection::Column::Slug.eq(key.to_ascii_lowercase())),
        };
        let model = query
            .one(conn)
            .await?
            .ok_or_else(|| SsoError::UnknownConnection(key.to_owned()))?;
        self.decode(model)
    }

    /// The connection with id or slug `key`.
    pub async fn connection(&self, key: &str) -> Result<SsoConnection, SsoError> {
        let loaded = self.load(&self.db.reader().await, key).await?;
        Ok(self.present(&loaded))
    }

    /// The connections of `owner` (every connection when `None`), oldest
    /// first.
    pub async fn connections(
        &self,
        owner: Option<SsoOwner>,
    ) -> Result<Vec<SsoConnection>, SsoError> {
        let mut query =
            sso_connection::Entity::find().order_by_asc(sso_connection::Column::CreatedAt);
        if let Some(owner) = owner {
            query = query.filter(sso_connection::Column::OwnerType.eq(owner.kind()));
            query = match owner.id() {
                Some(id) => query.filter(sso_connection::Column::OwnerId.eq(id)),
                None => query.filter(sso_connection::Column::OwnerId.is_null()),
            };
        }
        let rows = query.all(&self.db.reader().await).await?;
        rows.into_iter()
            .map(|row| self.decode(row).map(|loaded| self.present(&loaded)))
            .collect()
    }

    /// The owner exists: read through `uow`, so an organization or
    /// workspace created in the same unit of work counts.
    async fn check_owner(&self, uow: &Uow, owner: SsoOwner) -> Result<(), SsoError> {
        let Some(app) = self.app.upgrade() else {
            return Ok(());
        };
        let needs_workspaces = || {
            SsoError::invalid(
                "owner",
                "organization and workspace owners need WorkspaceModule in the app",
            )
        };
        let found = match owner {
            SsoOwner::App => true,
            SsoOwner::Workspace(id) => {
                if app.try_get::<navree_workspace::Workspaces>().await.is_none() {
                    return Err(needs_workspaces());
                }
                navree_workspace::entity::workspace::Entity::find_by_id(id)
                    .one(&*uow.tx().await?)
                    .await?
                    .is_some()
            }
            SsoOwner::Organization(id) => {
                if app.try_get::<navree_workspace::Organizations>().await.is_none() {
                    return Err(needs_workspaces());
                }
                // Without organizations (`workspace.organizations.enabled`)
                // the table may be missing: no such owner either way.
                navree_workspace::entity::organization::Entity::find_by_id(id)
                    .one(&*uow.tx().await?)
                    .await
                    .is_ok_and(|found| found.is_some())
            }
        };
        if found {
            Ok(())
        } else {
            Err(SsoError::invalid("owner", "owner does not exist"))
        }
    }

    fn check_settings(&self, settings: &ConnectionSettings) -> Result<(), SsoError> {
        match settings {
            ConnectionSettings::Saml(saml) => match (&saml.metadata_url, &saml.metadata_xml) {
                (Some(url), None) if trusted_url(url) => Ok(()),
                (Some(_), None) => Err(SsoError::invalid(
                    "settings.metadata_url",
                    "metadata_url must be an https URL",
                )),
                (None, Some(xml)) => IdpMetadata::parse(xml)
                    .map(drop)
                    .map_err(|error| SsoError::invalid("settings.metadata_xml", error.to_string())),
                _ => Err(SsoError::invalid(
                    "settings",
                    "give exactly one of metadata_url and metadata_xml",
                )),
            },
            ConnectionSettings::Oidc(oidc) => {
                if !trusted_url(&oidc.issuer) {
                    return Err(SsoError::invalid(
                        "settings.issuer",
                        "issuer must be an https URL",
                    ));
                }
                if oidc.client_id.trim().is_empty() {
                    return Err(SsoError::invalid(
                        "settings.client_id",
                        "client_id should not be empty",
                    ));
                }
                Ok(())
            }
        }
    }

    fn check_provisioning(provisioning: &Provisioning) -> Result<(), SsoError> {
        let owner_role = |role: &str| role.eq_ignore_ascii_case(navree_workspace::OWNER);
        if provisioning.default_role.as_deref().is_some_and(owner_role)
            || provisioning.roles.iter().any(|rule| owner_role(&rule.role))
        {
            return Err(SsoError::invalid(
                "provisioning.roles",
                "single sign-on cannot grant the owner role",
            ));
        }
        Ok(())
    }

    async fn slug_taken<C: ConnectionTrait>(
        conn: &C,
        slug: &str,
        except: Option<Uuid>,
    ) -> Result<bool, SsoError> {
        let mut query =
            sso_connection::Entity::find().filter(sso_connection::Column::Slug.eq(slug));
        if let Some(id) = except {
            query = query.filter(sso_connection::Column::Id.ne(id));
        }
        Ok(query.one(conn).await?.is_some())
    }

    /// Creates a connection in `uow`. Field errors
    /// ([`SsoError::Invalid`]) for an unknown owner, a taken or reserved
    /// slug, settings without metadata or an issuer, metadata that does not
    /// parse, or rules that grant the owner role.
    pub async fn create_connection(
        &self,
        uow: &Uow,
        input: NewConnection,
    ) -> Result<SsoConnection, SsoError> {
        let name = input.name.trim().to_owned();
        if name.is_empty() || name.chars().count() > 120 {
            return Err(SsoError::invalid(
                "name",
                "name must be 1 to 120 characters",
            ));
        }
        self.check_owner(uow, input.owner).await?;
        self.check_settings(&input.settings)?;
        Self::check_provisioning(&input.provisioning)?;
        let slug = {
            let tx = uow.tx().await?;
            match input.slug {
                Some(slug) => {
                    let slug = slug.trim().to_ascii_lowercase();
                    check_slug(&slug)?;
                    if Self::slug_taken(&*tx, &slug, None).await? {
                        return Err(SsoError::invalid("slug", "slug has already been taken"));
                    }
                    slug
                }
                None => {
                    let base = slugify(&name);
                    let base = if check_slug(&base).is_ok() {
                        base
                    } else {
                        format!("{base}-sso")
                    };
                    let mut candidate = base.clone();
                    let mut n = 2;
                    while Self::slug_taken(&*tx, &candidate, None).await? {
                        candidate = format!("{base}-{n}");
                        n += 1;
                    }
                    candidate
                }
            }
        };
        let now = self.now();
        let row = sso_connection::ActiveModel {
            id: Set(Uuid::now_v7()),
            owner_type: Set(input.owner.kind().to_owned()),
            owner_id: Set(input.owner.id()),
            slug: Set(slug),
            name: Set(name),
            protocol: Set(input.settings.protocol().as_str().to_owned()),
            enabled: Set(input.enabled),
            enforce: Set(input.enforce),
            settings: Set(self.encode_settings(&input.settings)?),
            provisioning: Set(serde_json::to_string(&input.provisioning)
                .map_err(|e| SsoError::Config(e.to_string()))?),
            created_at: Set(now),
            updated_at: Set(now),
        };
        let model = Repo::<sso_connection::Entity>::insert(uow, row).await?;
        Ok(self.present(&self.decode(model)?))
    }

    /// Changes a connection in `uow` (see [`ConnectionUpdate`]).
    pub async fn update_connection(
        &self,
        uow: &Uow,
        key: &str,
        changes: ConnectionUpdate,
    ) -> Result<SsoConnection, SsoError> {
        let current = {
            let tx = uow.tx().await?;
            self.load(&*tx, key).await?
        };
        let mut row = sso_connection::ActiveModel {
            id: Unchanged(current.model.id),
            updated_at: Set(self.now()),
            ..Default::default()
        };
        if let Some(name) = changes.name {
            let name = name.trim().to_owned();
            if name.is_empty() || name.chars().count() > 120 {
                return Err(SsoError::invalid(
                    "name",
                    "name must be 1 to 120 characters",
                ));
            }
            row.name = Set(name);
        }
        if let Some(slug) = changes.slug {
            let slug = slug.trim().to_ascii_lowercase();
            check_slug(&slug)?;
            let taken = {
                let tx = uow.tx().await?;
                Self::slug_taken(&*tx, &slug, Some(current.model.id)).await?
            };
            if taken {
                return Err(SsoError::invalid("slug", "slug has already been taken"));
            }
            row.slug = Set(slug);
        }
        if let Some(mut settings) = changes.settings {
            if settings.protocol() != current.settings.protocol() {
                return Err(SsoError::invalid(
                    "settings.protocol",
                    "a connection keeps its protocol; create another connection",
                ));
            }
            // A missing client secret keeps the stored one.
            if let (
                ConnectionSettings::Oidc(new),
                ConnectionSettings::Oidc(OidcSettings {
                    client_secret: Some(stored),
                    ..
                }),
            ) = (&mut settings, &current.settings)
                && new.client_secret.is_none()
            {
                new.client_secret = Some(stored.clone());
            }
            self.check_settings(&settings)?;
            row.settings = Set(self.encode_settings(&settings)?);
            self.forget_metadata(current.model.id);
        }
        if let Some(provisioning) = changes.provisioning {
            Self::check_provisioning(&provisioning)?;
            row.provisioning = Set(serde_json::to_string(&provisioning)
                .map_err(|e| SsoError::Config(e.to_string()))?);
        }
        if let Some(enabled) = changes.enabled {
            row.enabled = Set(enabled);
        }
        if changes.follow_default_enforcement {
            row.enforce = Set(None);
        } else if let Some(enforce) = changes.enforce {
            row.enforce = Set(Some(enforce));
        }
        let model = Repo::<sso_connection::Entity>::update(uow, row).await?;
        Ok(self.present(&self.decode(model)?))
    }

    /// Deletes a connection in `uow`, with its domains and identities (the
    /// users stay).
    pub async fn delete_connection(&self, uow: &Uow, key: &str) -> Result<(), SsoError> {
        let current = {
            let tx = uow.tx().await?;
            self.load(&*tx, key).await?
        };
        self.forget_metadata(current.model.id);
        Repo::<sso_connection::Entity>::delete(uow, current.model).await?;
        Ok(())
    }

    // -- domains -------------------------------------------------------------

    fn present_domain(&self, model: sso_domain::Model) -> SsoDomain {
        SsoDomain {
            resource_type: "SsoDomain",
            id: model.id,
            connection_id: model.connection_id,
            txt_name: model.domain.clone(),
            txt_value: format!("{}={}", self.options.domains.txt_prefix, model.token),
            domain: model.domain,
            verified: model.verified_at.is_some(),
            verified_at: model.verified_at,
            created_at: model.created_at,
        }
    }

    /// The domains of a connection.
    pub async fn domains(&self, connection: &str) -> Result<Vec<SsoDomain>, SsoError> {
        let reader = self.db.reader().await;
        let loaded = self.load(&reader, connection).await?;
        let rows = sso_domain::Entity::find()
            .filter(sso_domain::Column::ConnectionId.eq(loaded.model.id))
            .order_by_asc(sso_domain::Column::Domain)
            .all(&reader)
            .await?;
        Ok(rows
            .into_iter()
            .map(|row| self.present_domain(row))
            .collect())
    }

    /// Adds `domain` to a connection in `uow`, unverified: create the TXT
    /// record the result names, then call
    /// [`verify_domain`](Sso::verify_domain).
    pub async fn add_domain(
        &self,
        uow: &Uow,
        connection: &str,
        domain: &str,
    ) -> Result<SsoDomain, SsoError> {
        let domain = normalize_domain(domain)?;
        let loaded = {
            let tx = uow.tx().await?;
            let loaded = self.load(&*tx, connection).await?;
            let exists = sso_domain::Entity::find()
                .filter(sso_domain::Column::ConnectionId.eq(loaded.model.id))
                .filter(sso_domain::Column::Domain.eq(&domain))
                .one(&*tx)
                .await?;
            if exists.is_some() {
                return Err(SsoError::invalid(
                    "domain",
                    "domain has already been added to this connection",
                ));
            }
            loaded
        };
        let now = self.now();
        let row = sso_domain::ActiveModel {
            id: Set(Uuid::now_v7()),
            connection_id: Set(loaded.model.id),
            domain: Set(domain),
            token: Set(random_token(32)),
            verified_at: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        };
        Ok(self.present_domain(Repo::<sso_domain::Entity>::insert(uow, row).await?))
    }

    async fn find_domain<C: ConnectionTrait>(
        conn: &C,
        id: Uuid,
    ) -> Result<sso_domain::Model, SsoError> {
        sso_domain::Entity::find_by_id(id)
            .one(conn)
            .await?
            .ok_or(SsoError::DomainNotFound)
    }

    /// Looks the domain's TXT record up and marks it verified in `uow`.
    /// [`SsoError::DomainNotVerified`] when the record is missing,
    /// [`SsoError::DomainTaken`] when another connection verified it first.
    /// Emits [`SsoDomainVerified`] after the commit.
    pub async fn verify_domain(&self, uow: &Uow, id: Uuid) -> Result<SsoDomain, SsoError> {
        let domain = Self::find_domain(&*uow.tx().await?, id).await?;
        if domain.verified_at.is_some() {
            return Ok(self.present_domain(domain));
        }
        let expected = format!("{}={}", self.options.domains.txt_prefix, domain.token);
        let records = self.txt.txt(&domain.domain).await?;
        if !records.iter().any(|record| record.trim() == expected) {
            return Err(SsoError::DomainNotVerified(domain.domain));
        }
        self.mark_verified(uow, domain).await
    }

    /// Marks a domain verified in `uow` without a DNS lookup (operators,
    /// seeders and tests).
    pub async fn mark_domain_verified(&self, uow: &Uow, id: Uuid) -> Result<SsoDomain, SsoError> {
        let domain = Self::find_domain(&*uow.tx().await?, id).await?;
        if domain.verified_at.is_some() {
            return Ok(self.present_domain(domain));
        }
        self.mark_verified(uow, domain).await
    }

    async fn mark_verified(
        &self,
        uow: &Uow,
        domain: sso_domain::Model,
    ) -> Result<SsoDomain, SsoError> {
        let taken = sso_domain::Entity::find()
            .filter(sso_domain::Column::Domain.eq(&domain.domain))
            .filter(sso_domain::Column::Id.ne(domain.id))
            .filter(sso_domain::Column::VerifiedAt.is_not_null())
            .one(&*uow.tx().await?)
            .await?;
        if taken.is_some() {
            return Err(SsoError::DomainTaken(domain.domain));
        }
        let now = self.now();
        let event = SsoDomainVerified {
            connection_id: domain.connection_id,
            domain: domain.domain.clone(),
        };
        let row = sso_domain::ActiveModel {
            id: Unchanged(domain.id),
            verified_at: Set(Some(now)),
            updated_at: Set(now),
            ..Default::default()
        };
        let model = Repo::<sso_domain::Entity>::update(uow, row).await?;
        self.emit_after_commit(uow, event).await;
        Ok(self.present_domain(model))
    }

    /// Removes a domain in `uow`.
    pub async fn remove_domain(&self, uow: &Uow, id: Uuid) -> Result<(), SsoError> {
        let domain = Self::find_domain(&*uow.tx().await?, id).await?;
        Repo::<sso_domain::Entity>::delete(uow, domain).await?;
        Ok(())
    }

    /// The enabled connection whose verified domain is `domain`.
    async fn connection_for_domain(&self, domain: &str) -> Result<Option<Loaded>, SsoError> {
        let reader = self.db.reader().await;
        let verified = sso_domain::Entity::find()
            .filter(sso_domain::Column::Domain.eq(domain))
            .filter(sso_domain::Column::VerifiedAt.is_not_null())
            .one(&reader)
            .await?;
        let Some(verified) = verified else {
            return Ok(None);
        };
        let Some(model) = sso_connection::Entity::find_by_id(verified.connection_id)
            .one(&reader)
            .await?
        else {
            return Ok(None);
        };
        if !model.enabled {
            return Ok(None);
        }
        self.decode(model).map(Some)
    }

    fn enforced(&self, loaded: &Loaded) -> bool {
        loaded
            .model
            .enforce
            .unwrap_or(self.options.enforce_for_verified_domains)
    }

    /// Where `email` signs in: the enabled connection of its verified
    /// domain, if any, and whether a password is still allowed.
    pub async fn discover(&self, email: &str) -> Result<SsoDiscovery, SsoError> {
        let email = email.trim().to_lowercase();
        let domain = email.rsplit_once('@').map(|(_, d)| d).unwrap_or_default();
        let found = if domain.is_empty() {
            None
        } else {
            self.connection_for_domain(domain).await?
        };
        Ok(match found {
            Some(loaded) => SsoDiscovery {
                sso: true,
                connection: Some(loaded.model.slug.clone()),
                name: Some(loaded.model.name.clone()),
                protocol: Some(loaded.settings.protocol()),
                login_url: Some(self.login_url(&loaded.model.slug)),
                password_allowed: !self.enforced(&loaded),
            },
            None => SsoDiscovery {
                sso: false,
                connection: None,
                name: None,
                protocol: None,
                login_url: None,
                password_allowed: true,
            },
        })
    }

    /// The connection that keeps `email` off passwords, if any.
    pub(crate) async fn enforcing_connection(
        &self,
        email: &str,
    ) -> Result<Option<String>, SsoError> {
        let Some((_, domain)) = email.rsplit_once('@') else {
            return Ok(None);
        };
        Ok(self
            .connection_for_domain(domain)
            .await?
            .filter(|loaded| self.enforced(loaded))
            .map(|loaded| loaded.model.name))
    }

    // -- sign-in -------------------------------------------------------------

    fn state_key(key: &str) -> String {
        format!("navree-sso:state:{}", sha256_hex(key.as_bytes()))
    }

    /// The value under `key`, removed: only the caller whose `forget`
    /// removes the entry gets it, so two concurrent uses never both succeed.
    async fn take<T: for<'de> Deserialize<'de>>(&self, key: &str) -> Result<Option<T>, SsoError> {
        let Some(value) = self.cache.get::<T>(key).await? else {
            return Ok(None);
        };
        Ok(self.cache.forget(key).await?.then_some(value))
    }

    fn enabled(loaded: Loaded) -> Result<Loaded, SsoError> {
        if loaded.model.enabled {
            Ok(loaded)
        } else {
            Err(SsoError::ConnectionDisabled)
        }
    }

    /// Starts a sign-in with connection `key`: the identity provider's URL
    /// to send the browser to (a SAML request with the HTTP-Redirect
    /// binding, or an OpenID Connect authorization request with PKCE and a
    /// nonce). `return_to` (a relative path) comes back with the sign-in.
    pub async fn login(
        &self,
        key: &str,
        return_to: Option<String>,
    ) -> Result<SsoRedirect, SsoError> {
        if let Some(return_to) = &return_to {
            check_return_to(return_to)?;
        }
        let loaded = Self::enabled(self.load(&self.db.reader().await, key).await?)?;
        let relay = random_token(32);
        let now = self.now();
        let ttl = self.options.state_ttl_secs;
        let mut state = LoginState {
            connection_id: loaded.model.id,
            request_id: None,
            nonce: None,
            verifier: None,
            return_to,
            exp: now
                .timestamp()
                .saturating_add(i64::try_from(ttl).unwrap_or(i64::MAX)),
        };
        let url = match &loaded.settings {
            ConnectionSettings::Saml(saml) => {
                let idp = self.idp_metadata(&loaded, saml).await?;
                let sp = self.service_provider(&loaded.model.slug)?;
                let request = AuthnRequest::new(
                    &sp,
                    &idp,
                    &relay,
                    now,
                    &RequestOptions {
                        force_authn: saml.force_authn,
                        name_id_format: saml.name_id_format.clone(),
                    },
                )?;
                state.request_id = Some(request.id);
                request.url
            }
            ConnectionSettings::Oidc(settings) => {
                let discovery = oidc::discover(&self.http, &self.oidc, &settings.issuer).await?;
                let nonce = random_token(32);
                let verifier = random_token(64);
                let scopes = if settings.scopes.is_empty() {
                    self.options.oidc.scopes.clone()
                } else {
                    settings.scopes.clone()
                };
                let url = oidc::authorize_url(
                    &discovery,
                    &Authorize {
                        client_id: &settings.client_id,
                        redirect_uri: &self.redirect_uri(&loaded.model.slug)?,
                        scopes: &scopes,
                        state: &relay,
                        nonce: &nonce,
                        verifier: &verifier,
                    },
                )?;
                state.nonce = Some(nonce);
                state.verifier = Some(verifier);
                url
            }
        };
        self.cache
            .put(&Self::state_key(&relay), &state, Duration::from_secs(ttl))
            .await?;
        Ok(SsoRedirect { url, state: relay })
    }

    /// The state of `key`, which works once and must be for `connection`.
    async fn take_state(
        &self,
        key: &str,
        connection: Uuid,
    ) -> Result<Option<LoginState>, SsoError> {
        let Some(state) = self.take::<LoginState>(&Self::state_key(key)).await? else {
            return Ok(None);
        };
        if state.connection_id != connection || state.exp <= self.now().timestamp() {
            return Err(SsoError::InvalidState);
        }
        Ok(Some(state))
    }

    /// The service provider metadata XML of SAML connection `key`.
    pub async fn saml_metadata(&self, key: &str) -> Result<String, SsoError> {
        let loaded = self.load(&self.db.reader().await, key).await?;
        if loaded.settings.protocol() != Protocol::Saml {
            return Err(SsoError::UnknownConnection(key.to_owned()));
        }
        Ok(self.service_provider(&loaded.model.slug)?.metadata_xml())
    }

    fn forget_metadata(&self, id: Uuid) {
        self.metadata
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&id);
    }

    /// The identity provider's metadata: parsed from the stored XML, or
    /// fetched from `metadata_url` and kept `sso.saml.metadata_ttl_secs`.
    async fn idp_metadata(
        &self,
        loaded: &Loaded,
        saml: &SamlSettings,
    ) -> Result<Arc<IdpMetadata>, SsoError> {
        let ttl = Duration::from_secs(self.options.saml.metadata_ttl_secs);
        if let Some(cached) = self
            .metadata
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&loaded.model.id)
            && cached.version == loaded.model.updated_at
            && (saml.metadata_url.is_none() || cached.fetched.elapsed() < ttl)
        {
            return Ok(cached.metadata.clone());
        }
        let metadata = match (&saml.metadata_xml, &saml.metadata_url) {
            (Some(xml), _) => IdpMetadata::parse(xml)?,
            (None, Some(url)) => {
                let response = self
                    .http
                    .get(url)
                    .header("accept", "application/samlmetadata+xml, application/xml")
                    .send()
                    .await
                    .map_err(|e| SsoError::ProviderUnavailable(format!("SAML metadata: {e}")))?;
                if !response.is_success() {
                    return Err(SsoError::ProviderUnavailable(format!(
                        "SAML metadata: answered {}",
                        response.status().as_u16()
                    )));
                }
                IdpMetadata::parse(&response.text())?
            }
            (None, None) => {
                return Err(SsoError::Config(
                    "the SAML connection has no metadata".to_owned(),
                ));
            }
        };
        let metadata = Arc::new(metadata);
        self.metadata
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(
                loaded.model.id,
                CachedMetadata {
                    metadata: metadata.clone(),
                    fetched: Instant::now(),
                    version: loaded.model.updated_at,
                },
            );
        Ok(metadata)
    }

    /// Finishes a SAML sign-in at the assertion consumer service of
    /// connection `key`: the relay state (once), the response's checks (see
    /// [`validate_response`]), the assertion id (once), then the user
    /// (found, linked or provisioned).
    pub async fn complete_saml(
        &self,
        key: &str,
        saml_response: &str,
        relay_state: Option<&str>,
        meta: &ClientMeta,
    ) -> Result<SsoSignIn, SsoError> {
        let loaded = Self::enabled(self.load(&self.db.reader().await, key).await?)?;
        let ConnectionSettings::Saml(saml) = &loaded.settings else {
            return Err(SsoError::UnknownConnection(key.to_owned()));
        };
        if saml_response.len() > self.options.saml.max_response_bytes / 3 * 4 + 4 {
            return Err(SamlError::Invalid("the response is too large".to_owned()).into());
        }
        let state = match relay_state.filter(|r| !r.is_empty()) {
            Some(relay) => self.take_state(relay, loaded.model.id).await?,
            None => None,
        };
        let idp = self.idp_metadata(&loaded, saml).await?;
        let sp = self.service_provider(&loaded.model.slug)?;
        let now = self.now();
        let skew = Duration::from_secs(self.options.saml.clock_skew_secs);
        let check = ResponseCheck {
            sp: &sp,
            idp: &idp,
            request_id: state.as_ref().and_then(|s| s.request_id.as_deref()),
            now,
            clock_skew: skew,
            allow_idp_initiated: self.options.saml.allow_idp_initiated,
            require_signed_assertion: saml.require_signed_assertion,
        };
        let assertion = match validate_response(saml_response, &check) {
            Ok(assertion) => assertion,
            // A response to a request whose state is gone (expired, used).
            Err(SamlError::InResponseToMismatch) if state.is_none() => {
                return Err(SsoError::InvalidState);
            }
            Err(error) => {
                tracing::warn!(
                    target: "navree::security",
                    connection = %loaded.model.slug,
                    %error,
                    "SAML response refused"
                );
                return Err(error.into());
            }
        };
        // An assertion works once: remember its id until it expires.
        let remaining = (assertion.not_on_or_after - now)
            + TimeDelta::from_std(skew).unwrap_or(TimeDelta::zero());
        let remaining =
            Duration::from_secs(u64::try_from(remaining.num_seconds()).unwrap_or(1).max(1));
        let first_use = self
            .cache
            .add(
                &format!(
                    "navree-sso:assertion:{}:{}",
                    loaded.model.id,
                    sha256_hex(assertion.id.as_bytes())
                ),
                &1_u8,
                remaining,
            )
            .await?;
        if !first_use {
            tracing::warn!(target: "navree::security", connection = %loaded.model.slug, "SAML assertion replayed");
            return Err(SsoError::Replay);
        }
        let return_to = match &state {
            Some(state) => state.return_to.clone(),
            // IdP-initiated: a relative RelayState is where to go.
            None => relay_state
                .filter(|relay| check_return_to(relay).is_ok())
                .map(str::to_owned),
        };
        let profile = SsoProfile::from_saml(&assertion, &loaded.provisioning.attributes);
        let started = state.is_some().then(|| relay_state.unwrap_or_default().to_owned());
        self.finish(&loaded, profile, return_to, started, meta).await
    }

    /// Finishes an OpenID Connect sign-in at the callback of connection
    /// `key`: the state (once), the code exchange, the ID token checks,
    /// then the user (found, linked or provisioned).
    pub async fn complete_oidc(
        &self,
        key: &str,
        callback: &OidcCallback,
        meta: &ClientMeta,
    ) -> Result<SsoSignIn, SsoError> {
        let loaded = Self::enabled(self.load(&self.db.reader().await, key).await?)?;
        let ConnectionSettings::Oidc(settings) = &loaded.settings else {
            return Err(SsoError::UnknownConnection(key.to_owned()));
        };
        let state = match callback.state.as_deref().filter(|s| !s.is_empty()) {
            Some(state) => self
                .take_state(state, loaded.model.id)
                .await?
                .ok_or(SsoError::InvalidState)?,
            None => return Err(SsoError::InvalidState),
        };
        if let Some(error) = &callback.error {
            let detail = callback
                .error_description
                .as_deref()
                .map(|d| format!("{error}: {d}"))
                .unwrap_or_else(|| error.clone());
            return Err(SsoError::Provider(detail));
        }
        let code = callback
            .code
            .as_deref()
            .filter(|c| !c.is_empty())
            .ok_or_else(|| SsoError::Provider("no authorization code".to_owned()))?;
        let (nonce, verifier) = match (&state.nonce, &state.verifier) {
            (Some(nonce), Some(verifier)) => (nonce, verifier),
            _ => return Err(SsoError::InvalidState),
        };
        let discovery = oidc::discover(&self.http, &self.oidc, &settings.issuer).await?;
        let redirect_uri = self.redirect_uri(&loaded.model.slug)?;
        let tokens = oidc::exchange(
            &self.http,
            &discovery,
            &Exchange {
                client_id: &settings.client_id,
                client_secret: settings.client_secret.as_deref(),
                code,
                redirect_uri: &redirect_uri,
                verifier,
            },
        )
        .await?;
        let mut claims = oidc::verify_id_token(
            &self.http,
            &self.oidc,
            &discovery,
            &tokens.id_token,
            &IdTokenCheck {
                client_id: &settings.client_id,
                nonce,
                now: self.now(),
                skew: Duration::from_secs(self.options.oidc.clock_skew_secs),
            },
        )
        .await
        .inspect_err(|error| {
            tracing::warn!(target: "navree::security", connection = %loaded.model.slug, %error, "ID token refused");
        })?;
        let mapping = &loaded.provisioning.attributes;
        if !mapping.email.iter().any(|claim| claims.contains_key(claim))
            && let Some(access_token) = &tokens.access_token
        {
            oidc::merge_userinfo(&self.http, &discovery, access_token, &mut claims).await?;
        }
        let profile = SsoProfile::from_claims(&claims, mapping);
        self.finish(&loaded, profile, state.return_to, callback.state.clone(), meta)
            .await
    }

    /// Signs `profile` in with connection `key`, as the flows do after
    /// their checks: for profiles an app verified itself (a directory
    /// import, say).
    pub async fn sign_in_profile(
        &self,
        key: &str,
        profile: SsoProfile,
        meta: &ClientMeta,
    ) -> Result<SsoSignIn, SsoError> {
        let loaded = Self::enabled(self.load(&self.db.reader().await, key).await?)?;
        self.finish(&loaded, profile, None, None, meta).await
    }

    async fn finish(
        &self,
        loaded: &Loaded,
        profile: SsoProfile,
        return_to: Option<String>,
        state: Option<String>,
        meta: &ClientMeta,
    ) -> Result<SsoSignIn, SsoError> {
        if profile.subject.trim().is_empty() {
            return Err(SamlError::Invalid("the identity has no subject".to_owned()).into());
        }
        let (user, created) = self
            .own(meta.request_id.clone(), |uow| async move {
                self.provision(&uow, loaded, &profile, meta).await
            })
            .await?;
        self.audit(
            entry_for(AuditKind::Auth, "sso.login", meta)
                .actor(user.id)
                .subject("User", user.id.to_string())
                .metadata(json!({ "connection": loaded.model.slug, "created": created })),
        )
        .await;
        Ok(SsoSignIn {
            user,
            connection_id: loaded.model.id,
            return_to,
            created,
            state,
        })
    }

    /// The user of `profile`: linked by identity, linked by a verified
    /// email, or created; then their membership of the connection's owner.
    async fn provision(
        &self,
        uow: &Uow,
        loaded: &Loaded,
        profile: &SsoProfile,
        meta: &ClientMeta,
    ) -> Result<(User, bool), SsoError> {
        let now = self.now();
        let connection_id = loaded.model.id;
        let identity = {
            let tx = uow.tx().await?;
            sso_identity::Entity::find()
                .filter(sso_identity::Column::ConnectionId.eq(connection_id))
                .filter(sso_identity::Column::Subject.eq(&profile.subject))
                .one(&*tx)
                .await?
        };
        let attributes = profile.attributes.to_string();
        let (user, created) = match identity {
            Some(identity) => {
                let user = {
                    let tx = uow.tx().await?;
                    user::Entity::find_by_id(identity.user_id).one(&*tx).await?
                }
                .ok_or_else(|| SsoError::Config("a linked user is missing".to_owned()))?;
                let row = sso_identity::ActiveModel {
                    id: Unchanged(identity.id),
                    email: Set(profile.email.clone().or(identity.email)),
                    attributes: Set(attributes),
                    last_login_at: Set(now),
                    updated_at: Set(now),
                    ..Default::default()
                };
                Repo::<sso_identity::Entity>::update(uow, row).await?;
                (user, false)
            }
            None => {
                let email = profile.email.clone().ok_or(SsoError::MissingEmail)?;
                let domain = profile.email_domain().unwrap_or_default().to_owned();
                let (verified, existing) = {
                    let tx = uow.tx().await?;
                    let verified = sso_domain::Entity::find()
                        .filter(sso_domain::Column::ConnectionId.eq(connection_id))
                        .filter(sso_domain::Column::Domain.eq(&domain))
                        .filter(sso_domain::Column::VerifiedAt.is_not_null())
                        .one(&*tx)
                        .await?
                        .is_some();
                    let existing = user::Entity::find()
                        .filter(user::Column::Email.eq(&email))
                        .one(&*tx)
                        .await?;
                    (verified, existing)
                };
                // The identity provider vouches for the email only within
                // the connection's verified domains, and an account links
                // only when its owner proved the address too (an account
                // someone registered with an address they don't own, or
                // created through another connection, never links).
                let trusted = verified && profile.email_verified != Some(false);
                let (user, created) = match existing {
                    Some(user) if trusted && user.email_verified_at.is_some() => (user, false),
                    Some(_) => return Err(SsoError::EmailTaken),
                    None if !loaded.provisioning.jit => return Err(SsoError::ProvisioningDisabled),
                    None if !trusted && !loaded.provisioning.allow_unverified_emails => {
                        return Err(SsoError::UnverifiedDomain(domain));
                    }
                    None => (self.create_user(uow, profile, &email, trusted).await?, true),
                };
                let row = sso_identity::ActiveModel {
                    id: Set(Uuid::now_v7()),
                    connection_id: Set(connection_id),
                    user_id: Set(user.id),
                    subject: Set(profile.subject.clone()),
                    email: Set(Some(email)),
                    attributes: Set(attributes),
                    last_login_at: Set(now),
                    created_at: Set(now),
                    updated_at: Set(now),
                };
                Repo::<sso_identity::Entity>::insert(uow, row).await?;
                if created {
                    let mut audit = AuditEntry::new(AuditKind::Auth, "auth.register")
                        .actor(user.id)
                        .subject("User", user.id.to_string())
                        .metadata(json!({ "connection": loaded.model.slug }));
                    if let Some(request_id) = &meta.request_id {
                        audit = audit.request_id(request_id.clone());
                    }
                    self.audit_after_commit(uow, audit).await;
                }
                (user, created)
            }
        };
        let role = self
            .membership(uow, loaded, user.id, &profile.groups)
            .await?;
        if created || role.is_some() {
            self.emit_after_commit(
                uow,
                SsoUserProvisioned {
                    connection_id,
                    user_id: user.id,
                    owner: loaded.owner,
                    role,
                    created,
                },
            )
            .await;
        }
        Ok((user, created))
    }

    /// Creates a user with the profile's name and email and no password.
    async fn create_user(
        &self,
        uow: &Uow,
        profile: &SsoProfile,
        email: &str,
        verified: bool,
    ) -> Result<User, SsoError> {
        let password_hash = self.no_password_hash().await?;
        let now = self.now();
        let name = profile
            .name
            .clone()
            .unwrap_or_else(|| email.split('@').next().unwrap_or(email).to_owned());
        let row = user::ActiveModel {
            id: Set(Uuid::now_v7()),
            name: Set(name),
            email: Set(email.to_owned()),
            password_hash: Set(password_hash),
            email_verified_at: Set(verified.then_some(now)),
            two_factor_secret: Set(None),
            two_factor_recovery_codes: Set(None),
            two_factor_confirmed_at: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        };
        Ok(Repo::<user::Entity>::insert(uow, row).await?)
    }

    /// An Argon2id PHC string with the hasher's parameters whose digest is
    /// all zero bytes: no password verifies against it.
    async fn no_password_hash(&self) -> Result<String, SsoError> {
        let template = self
            .no_password
            .get_or_try_init(|| async {
                let hash = self.hasher.hash(&random_token(32)).await?;
                let (prefix, digest) = hash.rsplit_once('$').ok_or_else(|| {
                    SsoError::Config("the password hasher returned no PHC string".to_owned())
                })?;
                Ok::<_, SsoError>(format!("{prefix}${}", "A".repeat(digest.len())))
            })
            .await?;
        Ok(template.clone())
    }

    /// Makes `user` a member of the connection's owner with the role of
    /// their groups: `Ok(Some(role))` when a membership was created or its
    /// role changed, `Ok(None)` when nothing changed.
    async fn membership(
        &self,
        uow: &Uow,
        loaded: &Loaded,
        user: Uuid,
        groups: &[String],
    ) -> Result<Option<String>, SsoError> {
        if loaded.owner == SsoOwner::App {
            return Ok(None);
        }
        let role = loaded
            .provisioning
            .role_for(groups)
            .ok_or(SsoError::NotAssigned)?;
        let app = self
            .app
            .upgrade()
            .ok_or_else(|| SsoError::Config("the app has shut down".to_owned()))?;
        match loaded.owner {
            SsoOwner::App => Ok(None),
            SsoOwner::Workspace(workspace) => {
                let members = app.get::<navree_workspace::Members>().await?;
                let role_value = navree_workspace::WorkspaceRole::parse(&role);
                match members.find(workspace, user).await? {
                    None => {
                        members.add(uow, workspace, user, role_value).await?;
                        Ok(Some(role))
                    }
                    Some(member) if loaded.provisioning.sync_roles && member.role != role => {
                        match members.change_role(uow, workspace, user, role_value).await {
                            Ok(_) => Ok(Some(role)),
                            // The owner keeps their role.
                            Err(navree_workspace::WorkspaceError::OwnerRequired) => Ok(None),
                            Err(error) => Err(error.into()),
                        }
                    }
                    Some(_) => Ok(None),
                }
            }
            SsoOwner::Organization(organization) => {
                let members = app.get::<navree_workspace::OrgMembers>().await?;
                let role_value = navree_workspace::OrgRole::parse(&role);
                match members.find(organization, user).await? {
                    None => {
                        members
                            .add(
                                uow,
                                organization,
                                user,
                                role_value,
                                navree_workspace::WorkspaceSelector::All,
                            )
                            .await?;
                        Ok(Some(role))
                    }
                    Some(member) if loaded.provisioning.sync_roles && member.role != role => {
                        match members
                            .update(uow, organization, user, Some(role_value), None)
                            .await
                        {
                            Ok(_) => Ok(Some(role)),
                            Err(navree_workspace::WorkspaceError::OrganizationOwnerRequired) => {
                                Ok(None)
                            }
                            Err(error) => Err(error.into()),
                        }
                    }
                    Some(_) => Ok(None),
                }
            }
        }
    }

    // -- finishing -----------------------------------------------------------

    /// A one-time code for a finished sign-in (`?code=` on the front end's
    /// `complete_url`), exchanged with [`exchange`](Sso::exchange) within
    /// `sso.code_ttl_secs`.
    pub async fn issue_code(&self, sign_in: &SsoSignIn) -> Result<String, SsoError> {
        let code = random_token(43);
        self.cache
            .put(
                &format!("navree-sso:code:{}", sha256_hex(code.as_bytes())),
                &PendingCode {
                    user_id: sign_in.user.id,
                    connection_id: sign_in.connection_id,
                    return_to: sign_in.return_to.clone(),
                    state: sign_in.state.clone(),
                },
                Duration::from_secs(self.options.code_ttl_secs),
            )
            .await?;
        Ok(code)
    }

    /// Exchanges a one-time code for the user, a navree-auth token pair and
    /// the return path. [`SsoError::InvalidCode`] for an unknown, expired
    /// or used code, or when `state` is given and is not the state of the
    /// login that started the sign-in (a code from someone else's
    /// sign-in: login CSRF).
    pub async fn exchange(
        &self,
        code: &str,
        state: Option<&str>,
        meta: &ClientMeta,
    ) -> Result<(User, navree_auth::TokenPair, Option<String>), SsoError> {
        let pending: PendingCode = self
            .take(&format!(
                "navree-sso:code:{}",
                sha256_hex(code.trim().as_bytes())
            ))
            .await?
            .ok_or(SsoError::InvalidCode)?;
        if let Some(state) = state
            && !pending
                .state
                .as_deref()
                .is_some_and(|started| navree_crypto::constant_time_eq(started.as_bytes(), state.as_bytes()))
        {
            return Err(SsoError::InvalidCode);
        }
        let user = self
            .auth
            .find_user(pending.user_id)
            .await?
            .ok_or(SsoError::InvalidCode)?;
        let tokens = self.auth.issue_tokens(&user, meta.clone()).await?;
        Ok((user, tokens, pending.return_to))
    }

    /// The login JSON for `user` (like `POST /auth/login`'s), plus
    /// `return_to`.
    pub async fn login_body(
        &self,
        user: &User,
        tokens: &navree_auth::TokenPair,
        return_to: Option<String>,
    ) -> Result<Value, SsoError> {
        let mut body = serde_json::to_value(tokens).map_err(|e| SsoError::Config(e.to_string()))?;
        if let Value::Object(fields) = &mut body {
            fields.insert("user".to_owned(), self.auth.present(user).await?);
            if let Some(return_to) = return_to {
                fields.insert("return_to".to_owned(), Value::String(return_to));
            }
        }
        Ok(body)
    }

    /// Issues tokens for a finished sign-in at once (no `complete_url`).
    pub(crate) async fn tokens_for(
        &self,
        sign_in: &SsoSignIn,
        meta: &ClientMeta,
    ) -> Result<Value, SsoError> {
        let tokens = self.auth.issue_tokens(&sign_in.user, meta.clone()).await?;
        self.login_body(&sign_in.user, &tokens, sign_in.return_to.clone())
            .await
    }

    /// The front end's completion URL with `params`, if one is configured.
    pub(crate) fn complete_url(&self, params: &[(&str, &str)]) -> Option<String> {
        let base = self.options.complete_url.as_deref()?;
        let mut url = url::Url::parse(base).ok()?;
        {
            let mut query = url.query_pairs_mut();
            for (name, value) in params {
                query.append_pair(name, value);
            }
        }
        Some(url.to_string())
    }

    // -- plumbing ------------------------------------------------------------

    /// Runs `f` in a unit of work of its own, committed when `f` succeeds:
    /// a sign-in that fails half way leaves nothing behind, even when its
    /// route answers with a redirect.
    async fn own<T, F, Fut>(&self, request_id: Option<String>, f: F) -> Result<T, SsoError>
    where
        F: FnOnce(Uow) -> Fut,
        Fut: Future<Output = Result<T, SsoError>>,
    {
        let uow = self.db.uow(None, request_id).await;
        match uow.scope(f(uow.clone())).await {
            Ok(value) => {
                uow.commit().await?;
                Ok(value)
            }
            Err(error) => {
                if let Err(rollback) = uow.rollback().await {
                    tracing::warn!(target: "navree::sso", error = %rollback, "rollback failed");
                }
                Err(error)
            }
        }
    }

    async fn audit_sink(&self) -> Option<Arc<dyn AuditSink>> {
        self.app.upgrade()?.try_get::<dyn AuditSink>().await
    }

    async fn audit(&self, entry: AuditEntry) {
        if let Some(sink) = self.audit_sink().await {
            record(sink, entry).await;
        }
    }

    async fn audit_after_commit(&self, uow: &Uow, entry: AuditEntry) {
        if let Some(sink) = self.audit_sink().await {
            uow.after_commit(move || Box::pin(record(sink, entry)));
        }
    }

    async fn emit_after_commit<E: Event>(&self, uow: &Uow, event: E) {
        let Some(app) = self.app.upgrade() else {
            return;
        };
        let Some(bus) = app.try_get::<EventBus>().await else {
            return;
        };
        uow.after_commit(move || {
            Box::pin(async move {
                if let Err(error) = bus.emit(event).await {
                    tracing::warn!(target: "navree::sso", event = E::NAME, %error, "a single sign-on event listener failed");
                }
            })
        });
    }
}

async fn record(sink: Arc<dyn AuditSink>, entry: AuditEntry) {
    let action = entry.action.clone();
    if let Err(error) = sink.record(entry).await {
        tracing::warn!(target: "navree::audit", %error, action, "could not write a single sign-on audit entry");
    }
}

/// An audit entry with the client's address, user agent and request id.
fn entry_for(kind: AuditKind, action: &str, meta: &ClientMeta) -> AuditEntry {
    let mut entry = AuditEntry::new(kind, action);
    if let Some(ip) = &meta.ip {
        entry = entry.ip(ip.clone());
    }
    if let Some(user_agent) = &meta.user_agent {
        entry = entry.user_agent(user_agent.chars().take(512).collect::<String>());
    }
    if let Some(request_id) = &meta.request_id {
        entry = entry.request_id(request_id.clone());
    }
    entry
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn return_paths_must_be_relative() {
        for ok in ["/", "/dashboard?tab=1", "/w/acme/deals#top"] {
            assert!(check_return_to(ok).is_ok(), "{ok}");
        }
        for bad in [
            "https://evil.test",
            "//evil.test",
            "/\\evil.test",
            "dashboard",
            "/a\nb",
        ] {
            assert!(check_return_to(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn domains_are_normalized_and_checked() {
        assert_eq!(normalize_domain(" Acme.COM. ").unwrap(), "acme.com");
        assert_eq!(
            normalize_domain("mail.acme-corp.co.uk").unwrap(),
            "mail.acme-corp.co.uk"
        );
        for bad in [
            "localhost",
            "acme..com",
            "-acme.com",
            "acme.com/x",
            "a@b.com",
            "ünï.com",
            "10.0.0.1",
        ] {
            assert!(normalize_domain(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn slugs_are_derived_and_checked() {
        assert_eq!(slugify("Acme Okta (prod)"), "acme-okta-prod");
        assert_eq!(slugify("!"), "sso");
        assert!(check_slug("acme-okta").is_ok());
        for bad in [
            "a",
            "Acme",
            "-acme",
            "connections",
            "saml",
            &Uuid::nil().to_string(),
        ] {
            assert!(check_slug(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn metadata_and_issuer_urls_must_be_https_unless_local() {
        assert!(trusted_url("https://idp.acme.test/metadata"));
        assert!(trusted_url("http://127.0.0.1:8080/metadata"));
        assert!(trusted_url("http://localhost/metadata"));
        assert!(!trusted_url("http://idp.acme.test/metadata"));
        assert!(!trusted_url("file:///etc/passwd"));
    }
}
