//! [`TransportResolver`]: transports stored for a domain, a workspace, an
//! organization or the platform, and which one a message goes through.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use chrono::{DateTime, Utc};
use navree_db::record::{self, Filter};
use navree_db::{Record as _, Uow};
use navree_http_client::{HttpClient, HttpClientError, HttpClientOptions};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::Store;
use super::entity::TransportRow;
use crate::{MailError, MailScope, MailTransport, TransportConfig, TransportTier};

/// Who a stored transport belongs to: its tier and key.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(tag = "tier", rename_all = "snake_case")]
pub enum TransportOwner {
    /// Mail from `domain` (the sender's domain), sent for the owning
    /// workspace or organization (or for anyone, when it has neither).
    Domain {
        /// The sending domain.
        domain: String,
        /// The workspace that owns the domain.
        workspace_id: Option<Uuid>,
        /// The organization that owns the domain.
        organization_id: Option<Uuid>,
    },
    /// Mail sent for a workspace.
    Workspace {
        /// The workspace.
        workspace_id: Uuid,
    },
    /// Mail sent for an organization (and its workspaces).
    Organization {
        /// The organization.
        organization_id: Uuid,
    },
    /// Every message, ahead of the configured transport.
    Platform,
}

impl TransportOwner {
    /// A platform-wide transport for mail from `domain`.
    pub fn domain(domain: impl Into<String>) -> Self {
        TransportOwner::Domain {
            domain: normalize_domain(&domain.into()),
            workspace_id: None,
            organization_id: None,
        }
    }

    /// The transport for mail from `domain` sent for `workspace_id`.
    pub fn workspace_domain(domain: impl Into<String>, workspace_id: Uuid) -> Self {
        TransportOwner::Domain {
            domain: normalize_domain(&domain.into()),
            workspace_id: Some(workspace_id),
            organization_id: None,
        }
    }

    /// The transport for mail from `domain` sent for `organization_id`.
    pub fn organization_domain(domain: impl Into<String>, organization_id: Uuid) -> Self {
        TransportOwner::Domain {
            domain: normalize_domain(&domain.into()),
            workspace_id: None,
            organization_id: Some(organization_id),
        }
    }

    /// A workspace's transport.
    pub fn workspace(workspace_id: Uuid) -> Self {
        TransportOwner::Workspace { workspace_id }
    }

    /// An organization's transport.
    pub fn organization(organization_id: Uuid) -> Self {
        TransportOwner::Organization { organization_id }
    }

    /// The tier.
    pub fn tier(&self) -> TransportTier {
        match self {
            TransportOwner::Domain { .. } => TransportTier::Domain,
            TransportOwner::Workspace { .. } => TransportTier::Workspace,
            TransportOwner::Organization { .. } => TransportTier::Organization,
            TransportOwner::Platform => TransportTier::Platform,
        }
    }

    /// `(workspace_id, organization_id, domain)` as stored.
    fn columns(&self) -> (Option<Uuid>, Option<Uuid>, Option<String>) {
        match self {
            TransportOwner::Domain {
                domain,
                workspace_id,
                organization_id,
            } => (
                *workspace_id,
                *organization_id,
                Some(normalize_domain(domain)),
            ),
            TransportOwner::Workspace { workspace_id } => (Some(*workspace_id), None, None),
            TransportOwner::Organization { organization_id } => {
                (None, Some(*organization_id), None)
            }
            TransportOwner::Platform => (None, None, None),
        }
    }

    /// The row filter for this owner.
    fn filter(&self) -> Filter {
        let (workspace, organization, domain) = self.columns();
        Filter::all()
            .eq("tier", self.tier().as_str())
            .eq("workspace_id", workspace)
            .eq("organization_id", organization)
            .eq("domain", domain)
    }
}

fn normalize_domain(domain: &str) -> String {
    domain.trim().trim_end_matches('.').to_ascii_lowercase()
}

/// A stored transport, without its settings (`mail_transports`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct StoredTransport {
    /// The id.
    pub id: Uuid,
    /// The tier.
    pub tier: TransportTier,
    /// The owning workspace.
    pub workspace_id: Option<Uuid>,
    /// The owning organization.
    pub organization_id: Option<Uuid>,
    /// The sending domain (`domain` tier).
    pub domain: Option<String>,
    /// The transport type (`smtp`, `ses`, ...).
    pub kind: String,
    /// Whether it is used.
    pub enabled: bool,
    /// When its `usable` check last passed.
    pub checked_at: Option<DateTime<Utc>>,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<&TransportRow> for StoredTransport {
    fn from(row: &TransportRow) -> Self {
        Self {
            id: row.id,
            tier: row.tier.parse().unwrap_or(TransportTier::Platform),
            workspace_id: row.workspace_id,
            organization_id: row.organization_id,
            domain: row.domain.clone(),
            kind: row.kind.clone(),
            enabled: row.enabled,
            checked_at: row.checked_at,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

/// The transport a message goes through, and where it came from.
#[derive(Clone)]
pub struct ResolvedTransport {
    /// The stored transport's id.
    pub id: Uuid,
    /// Its tier.
    pub tier: TransportTier,
    /// The built transport.
    pub transport: Arc<dyn MailTransport>,
}

impl fmt::Debug for ResolvedTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResolvedTransport")
            .field("id", &self.id)
            .field("tier", &self.tier)
            .field("transport", &self.transport.name())
            .finish()
    }
}

/// Built transports by stored transport id, with the `updated_at` they were
/// built from.
type Cache = HashMap<Uuid, (DateTime<Utc>, Arc<dyn MailTransport>)>;

/// Transports stored per sending domain, workspace, organization or for the
/// platform, and which one a message goes through: the sender's **domain**
/// (owned by the message's workspace or organization, or by nobody), then
/// the **workspace**, the **organization**, the **platform**, and finally
/// the configured `mail.transport` (or a named one with
/// [`Mailer::via`](crate::Mailer::via)).
///
/// Settings are stored encrypted with the app key (`CryptoModule`). A
/// tenant transport (domain, workspace or organization) may not set an
/// `endpoint`, and its SMTP host may not be a private, loopback or other
/// reserved address under the HTTP client's SSRF policy; call
/// [`usable`](TransportResolver::usable) before saving one, so a tenant
/// learns at once that its credentials do not work. Built transports are
/// cached (an SMTP connection pool is reused) until the row changes.
///
/// Provided and exported by [`MailModule`](crate::MailModule); needs
/// `DatabaseModule`. [`Mailer::for_scope`](crate::Mailer::for_scope)
/// resolves through it when `mail.delivery.enabled` (and
/// `mail.delivery.tenants`).
pub struct TransportResolver {
    store: Arc<Store>,
    cache: Mutex<Cache>,
}

impl fmt::Debug for TransportResolver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TransportResolver")
    }
}

impl TransportResolver {
    pub(crate) fn new(store: Arc<Store>) -> Self {
        Self {
            store,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Stores `config` for `owner` (replacing its transport, if any) and
    /// enables it. The settings are encrypted; a tenant transport is
    /// checked as described above (not connected to: call
    /// [`usable`](TransportResolver::usable) for that).
    pub async fn set(
        &self,
        uow: &Uow,
        owner: TransportOwner,
        config: &TransportConfig,
    ) -> Result<StoredTransport, MailError> {
        if owner.tier() != TransportTier::Platform {
            self.check_tenant(config).await?;
        }
        config.build()?;
        if let TransportOwner::Domain { domain, .. } = &owner
            && (domain.is_empty() || !domain.contains('.'))
        {
            return Err(MailError::Config(format!(
                "`{domain}` is not a domain name"
            )));
        }
        let sealed = self.store.seal(&config.to_json().to_string()).await?;
        let now = self.store.now().await;
        let existing = TransportRow::select()
            .filter(owner.filter())
            .for_update()
            .one(uow)
            .await?;
        let row = match existing {
            Some(existing) => {
                let row = TransportRow {
                    kind: config.kind().to_owned(),
                    config: sealed,
                    enabled: true,
                    checked_at: None,
                    updated_at: now,
                    ..existing.clone()
                };
                record::update_recorded(uow, &existing, &row).await?
            }
            None => {
                let (workspace_id, organization_id, domain) = owner.columns();
                let row = TransportRow {
                    id: Uuid::now_v7(),
                    tier: owner.tier().as_str().to_owned(),
                    workspace_id,
                    organization_id,
                    domain,
                    kind: config.kind().to_owned(),
                    config: sealed,
                    enabled: true,
                    checked_at: None,
                    created_at: now,
                    updated_at: now,
                };
                record::insert_recorded(uow, &row).await?
            }
        };
        self.forget(row.id);
        Ok(StoredTransport::from(&row))
    }

    /// `owner`'s stored transport.
    pub async fn get(&self, owner: &TransportOwner) -> Result<Option<StoredTransport>, MailError> {
        let db = self.store.db().await?;
        Ok(TransportRow::select()
            .filter(owner.filter())
            .one(&db.read_executor().await)
            .await?
            .map(|row| StoredTransport::from(&row)))
    }

    /// The decrypted settings of the stored transport `id`.
    pub async fn config(&self, id: Uuid) -> Result<TransportConfig, MailError> {
        let row = self.row(id).await?;
        self.decrypt(&row).await
    }

    /// Removes `owner`'s transport; returns whether there was one.
    pub async fn remove(&self, uow: &Uow, owner: &TransportOwner) -> Result<bool, MailError> {
        let existing = TransportRow::select()
            .filter(owner.filter())
            .one(uow)
            .await?;
        match existing {
            Some(row) => {
                self.forget(row.id);
                record::delete_recorded(uow, &row).await?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Turns the stored transport `id` on or off (an off transport is
    /// skipped when resolving).
    pub async fn set_enabled(
        &self,
        uow: &Uow,
        id: Uuid,
        enabled: bool,
    ) -> Result<StoredTransport, MailError> {
        let existing = TransportRow::select()
            .eq("id", id)
            .for_update()
            .one(uow)
            .await?
            .ok_or(MailError::NotFound("mail transport"))?;
        let row = TransportRow {
            enabled,
            updated_at: self.store.now().await,
            ..existing.clone()
        };
        let row = record::update_recorded(uow, &existing, &row).await?;
        self.forget(id);
        Ok(StoredTransport::from(&row))
    }

    /// Every stored transport (of `tier`, when given), oldest first.
    pub async fn list(
        &self,
        tier: Option<TransportTier>,
    ) -> Result<Vec<StoredTransport>, MailError> {
        let db = self.store.db().await?;
        let mut select = TransportRow::select();
        if let Some(tier) = tier {
            select = select.eq("tier", tier.as_str());
        }
        Ok(select
            .order_by("created_at")
            .order_by("id")
            .all(&db.read_executor().await)
            .await?
            .iter()
            .map(StoredTransport::from)
            .collect())
    }

    /// Whether `config` can be used by a tenant: it passes the tenant
    /// checks, builds, and its transport's [`check`](MailTransport::check)
    /// succeeds (SMTP connects and authenticates; an API transport makes a
    /// read-only call). The error says what failed
    /// ([`MailError::Unusable`], [`MailError::Config`]).
    pub async fn usable(&self, config: &TransportConfig) -> Result<(), MailError> {
        self.check_tenant(config).await?;
        config.build()?.check().await
    }

    /// Runs [`usable`](TransportResolver::usable) on the stored transport
    /// `id` and, when it passes, records the time in `checked_at`.
    pub async fn check_stored(&self, id: Uuid) -> Result<StoredTransport, MailError> {
        let row = self.row(id).await?;
        let config = self.decrypt(&row).await?;
        if row.tier != TransportTier::Platform.as_str() {
            self.check_tenant(&config).await?;
        }
        config.build()?.check().await?;
        let now = self.store.now().await;
        self.store
            .in_uow(|uow| async move {
                let existing = TransportRow::select()
                    .eq("id", id)
                    .for_update()
                    .one(&uow)
                    .await?
                    .ok_or(MailError::NotFound("mail transport"))?;
                let checked = TransportRow {
                    checked_at: Some(now),
                    ..existing.clone()
                };
                let checked = record::update_recorded(&uow, &existing, &checked).await?;
                Ok(StoredTransport::from(&checked))
            })
            .await
    }

    /// The stored transport a message from `sender_domain` sent for `scope`
    /// goes through, most specific tier first; `None` when no enabled
    /// transport applies (the configured one is used).
    pub async fn resolve(
        &self,
        scope: &MailScope,
        sender_domain: Option<&str>,
    ) -> Result<Option<ResolvedTransport>, MailError> {
        let db = self.store.db().await?;
        let mut any = Filter::any().eq("tier", TransportTier::Platform.as_str());
        if let Some(domain) = sender_domain {
            any = any.add(
                Filter::all()
                    .eq("tier", TransportTier::Domain.as_str())
                    .eq("domain", normalize_domain(domain)),
            );
        }
        if let Some(workspace) = scope.workspace_id {
            any = any.add(
                Filter::all()
                    .eq("tier", TransportTier::Workspace.as_str())
                    .eq("workspace_id", workspace),
            );
        }
        if let Some(organization) = scope.organization_id {
            any = any.add(
                Filter::all()
                    .eq("tier", TransportTier::Organization.as_str())
                    .eq("organization_id", organization),
            );
        }
        let rows = TransportRow::select()
            .eq("enabled", true)
            .filter(any)
            .all(&db.read_executor().await)
            .await?;
        let applies = |row: &TransportRow| -> bool {
            if row.tier != TransportTier::Domain.as_str() {
                return true;
            }
            match (row.workspace_id, row.organization_id) {
                (None, None) => true,
                (Some(owner), _) if scope.workspace_id == Some(owner) => true,
                (_, Some(owner)) if scope.organization_id == Some(owner) => true,
                _ => false,
            }
        };
        for tier in [
            TransportTier::Domain,
            TransportTier::Workspace,
            TransportTier::Organization,
            TransportTier::Platform,
        ] {
            // A domain owned by the workspace wins over one owned by nobody.
            let mut candidates: Vec<&TransportRow> = rows
                .iter()
                .filter(|row| row.tier == tier.as_str() && applies(row))
                .collect();
            candidates
                .sort_by_key(|row| (row.workspace_id.is_none(), row.organization_id.is_none()));
            if let Some(row) = candidates.first() {
                let transport = self.built(row).await?;
                return Ok(Some(ResolvedTransport {
                    id: row.id,
                    tier,
                    transport,
                }));
            }
        }
        Ok(None)
    }

    /// The built transport of `row`, from the cache when `row` has not
    /// changed.
    async fn built(&self, row: &TransportRow) -> Result<Arc<dyn MailTransport>, MailError> {
        if let Some((updated_at, transport)) = self.cache().get(&row.id)
            && *updated_at == row.updated_at
        {
            return Ok(transport.clone());
        }
        let config = self.decrypt(row).await?;
        let transport = config.build()?;
        self.cache()
            .insert(row.id, (row.updated_at, transport.clone()));
        Ok(transport)
    }

    fn cache(&self) -> std::sync::MutexGuard<'_, Cache> {
        self.cache.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn forget(&self, id: Uuid) {
        self.cache().remove(&id);
    }

    async fn decrypt(&self, row: &TransportRow) -> Result<TransportConfig, MailError> {
        let json = self.store.open(&row.config).await?;
        serde_json::from_str(&json).map_err(|error| MailError::Store {
            source: Box::new(error),
        })
    }

    async fn row(&self, id: Uuid) -> Result<TransportRow, MailError> {
        let db = self.store.db().await?;
        record::find::<TransportRow, _>(&db.read_executor().await, id)
            .await?
            .ok_or(MailError::NotFound("mail transport"))
    }

    /// The checks a tenant's settings must pass: no `endpoint`, no SMTP
    /// host on a private or reserved network.
    async fn check_tenant(&self, config: &TransportConfig) -> Result<(), MailError> {
        if config.has_endpoint() {
            return Err(MailError::Config(
                "a tenant transport cannot set `endpoint`".to_owned(),
            ));
        }
        let hosts = config.smtp_hosts();
        if hosts.is_empty() {
            return Ok(());
        }
        let client = match self.store.app().ok() {
            Some(app) => match app.try_get::<HttpClient>().await {
                Some(client) => (*client).clone(),
                None => default_client()?,
            },
            None => default_client()?,
        };
        for host in hosts {
            match client.check_destination(&format!("https://{host}/")).await {
                Ok(()) | Err(HttpClientError::Connect { .. }) => {}
                Err(HttpClientError::Blocked { .. }) => {
                    return Err(MailError::Unusable(format!(
                        "the SMTP host `{host}` is a private or reserved address"
                    )));
                }
                Err(error) => {
                    return Err(MailError::Config(format!(
                        "invalid SMTP host `{host}`: {error}"
                    )));
                }
            }
        }
        Ok(())
    }
}

fn default_client() -> Result<HttpClient, MailError> {
    HttpClient::new(HttpClientOptions::default())
        .map_err(|error| MailError::Config(format!("cannot build the HTTP client: {error}")))
}
