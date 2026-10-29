//! What the [`Domains`](crate::Domains) service works with: the options,
//! the database, the drivers and the app's services.

use std::future::Future;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use navree_config::Config;
use navree_contracts::{Clock, SystemClock};
use navree_crypto::Encrypter;
use navree_db::{Db, Uow};
use navree_events::{Event, EventBus};
use navree_kernel::{AppContext, WeakAppContext};
use navree_workspace::{ResolverKind, WorkspaceConfig};

use crate::acme::AcmeClient;
use crate::certs::DomainCertificates;
use crate::dns::DnsLookup;
use crate::host::{host_without_port, normalize_host};
use crate::options::DomainsOptions;
use crate::probe::CertificateProbe;
use crate::provider::{CloudflareDns, DnsProvider};
use crate::{CurrentDomain, DomainsError};

/// The bound service's state.
pub(crate) struct Runtime {
    pub(crate) options: Arc<DomainsOptions>,
    pub(crate) db: Arc<Db>,
    pub(crate) encrypter: Option<Arc<Encrypter>>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) app: WeakAppContext,
    pub(crate) dns: Arc<dyn DnsLookup>,
    pub(crate) provider: Option<Arc<dyn DnsProvider>>,
    pub(crate) acme: Arc<dyn AcmeClient>,
    pub(crate) probe: Arc<dyn CertificateProbe>,
    /// Zones no workspace may claim: `domains.reserved`, the host of
    /// `app.url`, `workspace.base_domain` and `domains.target`.
    pub(crate) reserved: Vec<String>,
    /// The parsed `domains.addresses`.
    pub(crate) addresses: Vec<IpAddr>,
    /// `workspace.resolvers` lists `domain`.
    pub(crate) resolves_workspaces: bool,
    /// Verified hosts (and misses), for `domains.cache_secs`.
    pub(crate) cache: Option<navree_bounded::Cache<String, Option<CurrentDomain>>>,
    pub(crate) certificates: Arc<DomainCertificates>,
}

impl Runtime {
    /// Resolves everything from `app`.
    pub(crate) async fn build(app: &AppContext) -> Result<Runtime, DomainsError> {
        let options = app.get::<DomainsOptions>().await?;
        let clock = match app.try_get::<dyn Clock>().await {
            Some(clock) => clock,
            None => Arc::new(SystemClock),
        };
        let workspaces = app.try_get::<WorkspaceConfig>().await;
        let mut reserved: Vec<String> = options
            .reserved
            .iter()
            .filter_map(|host| normalize_host(host).ok())
            .collect();
        if let Some(config) = app.try_get::<Config>().await
            && let Some(url) = config.get_string("app.url")
            && let Ok(parsed) = url::Url::parse(url.trim())
            && let Some(host) = parsed.host_str().and_then(host_without_port)
            && normalize_host(&host).is_ok()
        {
            reserved.push(host);
        }
        if let Some(base) = workspaces
            .as_ref()
            .and_then(|config| config.base_domain.as_deref())
            .and_then(host_without_port)
        {
            reserved.push(base);
        }
        if let Some(target) = options
            .target
            .as_deref()
            .and_then(|target| normalize_host(target).ok())
        {
            reserved.push(target);
        }
        reserved.sort();
        reserved.dedup();
        let provider = match app.try_get::<dyn DnsProvider>().await {
            Some(provider) => Some(provider),
            None => match options.dns.provider.as_deref().map(str::trim) {
                Some("cloudflare") => {
                    Some(Arc::new(CloudflareDns::new(&options.dns.cloudflare)?)
                        as Arc<dyn DnsProvider>)
                }
                _ => None,
            },
        };
        let cache = (options.cache_secs > 0).then(|| {
            navree_bounded::Cache::builder(100_000)
                .time_to_live(Duration::from_secs(options.cache_secs))
                .build()
        });
        Ok(Runtime {
            db: app.get::<Db>().await?,
            encrypter: app.try_get::<Encrypter>().await,
            clock,
            app: app.downgrade(),
            dns: app.get::<dyn DnsLookup>().await?,
            provider,
            acme: app.get::<dyn AcmeClient>().await?,
            probe: app.get::<dyn CertificateProbe>().await?,
            reserved,
            addresses: options.ip_addresses(),
            resolves_workspaces: workspaces
                .as_ref()
                .is_some_and(|config| config.resolvers.contains(&ResolverKind::Domain)),
            cache,
            certificates: Arc::new(DomainCertificates::new()),
            options,
        })
    }

    pub(crate) fn now(&self) -> DateTime<Utc> {
        self.clock.now()
    }

    /// Seals a private key or account credentials with the application key.
    pub(crate) fn seal(&self, secret: &str) -> Result<String, DomainsError> {
        let encrypter = self.encrypter.as_ref().ok_or_else(|| {
            DomainsError::Config(
                "storing certificates needs CryptoModule with an application key".to_owned(),
            )
        })?;
        Ok(encrypter.encrypt_string(secret)?)
    }

    /// Opens a sealed value.
    pub(crate) fn open(&self, sealed: &str) -> Result<String, DomainsError> {
        let encrypter = self.encrypter.as_ref().ok_or_else(|| {
            DomainsError::Config(
                "reading certificates needs CryptoModule with an application key".to_owned(),
            )
        })?;
        Ok(encrypter.decrypt_string(sealed)?)
    }

    /// Emits `event` once `uow` commits, when the app has an `EventBus`.
    pub(crate) async fn emit_after_commit<E: Event>(&self, uow: &Uow, event: E) {
        let Some(app) = self.app.upgrade() else {
            return;
        };
        let Some(bus) = app.try_get::<EventBus>().await else {
            return;
        };
        uow.after_commit(move || {
            Box::pin(async move {
                if let Err(error) = bus.emit(event).await {
                    tracing::warn!(target: "navree::domains", event = E::NAME, %error, "a domains event listener failed");
                }
            })
        });
    }

    /// Emits `event` now, when the app has an `EventBus` (outside a unit of
    /// work).
    pub(crate) async fn emit<E: Event>(&self, event: E) {
        let Some(app) = self.app.upgrade() else {
            return;
        };
        let Some(bus) = app.try_get::<EventBus>().await else {
            return;
        };
        if let Err(error) = bus.emit(event).await {
            tracing::warn!(target: "navree::domains", event = E::NAME, %error, "a domains event listener failed");
        }
    }

    /// Forgets the cached answer for `host`.
    pub(crate) fn forget(&self, host: &str) {
        if let Some(cache) = &self.cache {
            cache.invalidate(host);
        }
    }

    /// Runs `f` in the task's unfinished unit of work (a request's), or else
    /// in a new one committed when `f` succeeds.
    pub(crate) async fn write<T, F, Fut>(
        &self,
        actor: Option<uuid::Uuid>,
        f: F,
    ) -> Result<T, DomainsError>
    where
        F: FnOnce(Uow) -> Fut,
        Fut: Future<Output = Result<T, DomainsError>>,
    {
        if let Some(uow) = Uow::current()
            && !uow.is_finished().await
        {
            return f(uow).await;
        }
        self.own_unit(actor, f).await
    }

    /// Runs `f` in a unit of work of its own, committed when `f` succeeds.
    async fn own_unit<T, F, Fut>(&self, actor: Option<uuid::Uuid>, f: F) -> Result<T, DomainsError>
    where
        F: FnOnce(Uow) -> Fut,
        Fut: Future<Output = Result<T, DomainsError>>,
    {
        let uow = self.db.uow(actor, None).await;
        match uow.scope(f(uow.clone())).await {
            Ok(value) => {
                uow.commit().await?;
                Ok(value)
            }
            Err(error) => {
                if let Err(rollback) = uow.rollback().await {
                    tracing::warn!(target: "navree::domains", error = %rollback, "rollback failed");
                }
                Err(error)
            }
        }
    }
}
