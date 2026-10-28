//! [`DomainsModule`].

use std::sync::Arc;

use axum::middleware::from_fn;
use navree_config::ModuleBuilderConfigExt;
use navree_db::{Db, ModuleBuilderDbExt};
use navree_events::{Listener, ModuleBuilderEventsExt};
use navree_http::{GlobalLayer, LayerSlot, ModuleBuilderHttpExt};
use navree_kernel::{
    AppContext, BoxError, BoxFuture, Factory, Injectable, KernelError, Lifecycle, Module,
    ModuleBuilder, ModuleId, ProviderKey, Resolver,
};
use navree_schedule::ModuleBuilderScheduleExt;
use navree_workspace::events::WorkspaceDeleted;

use crate::acme::{AcmeClient, InstantAcme};
use crate::commands::commands;
use crate::controller::{DomainsController, DomainsPublicController};
use crate::current::mark_domain;
use crate::dns::{DnsLookup, SystemDns};
use crate::error::map_domains_error;
use crate::migration::migrations;
use crate::options::{DomainsOptions, TlsMode};
use crate::probe::{CertificateProbe, TlsProbe};
use crate::tls::HttpsServer;
use crate::{Domains, DomainsError};

/// Custom domains: workspaces on their own domains.
///
/// Global; provides and exports [`Domains`], [`DomainsOptions`] and the
/// `dyn` drivers [`DnsLookup`] (a [`SystemDns`]), [`AcmeClient`] (an
/// [`InstantAcme`]) and [`CertificateProbe`] (a [`TlsProbe`]), and
/// contributes:
///
/// - the `domains` migration set (`custom_domains`,
///   `custom_domain_certificates`, `custom_domain_challenges`,
///   `custom_domain_acme_accounts`);
/// - the routes under `{http.api_prefix}/domains` unless `domains.routes`
///   is false ([`DomainsController`](crate::DomainsController); permission
///   `domains.manage`), and the public ask endpoint (`tls = "on_demand"`)
///   or ACME challenge answers (`tls = "acme"`)
///   ([`DomainsPublicController`](crate::DomainsPublicController));
/// - a request layer that marks requests to a verified domain for the
///   `domain` workspace resolver (with `domain` in `workspace.resolvers`);
/// - the commands `domains:list`, `domains:verify` and
///   `domains:certificates`; `domains:verify --due` every minute and
///   `domains:certificates --due` hourly;
/// - with `tls = "acme"` and `domains.acme.https_addr`, an HTTPS listener
///   next to `serve` ([`HttpsServer`](crate::HttpsServer));
/// - a listener that removes a deleted workspace's domains;
/// - the mapping of [`DomainsError`]s returned from handlers to error
///   responses.
///
/// The app must include `ConfigModule` and `DatabaseModule`;
/// `WorkspaceModule` for the routes and the `domain` resolver,
/// `CryptoModule` with an application key for `tls = "acme"`,
/// `ScheduleModule` for the schedule, `EventsModule` for the events, and
/// the routes need an authenticator and `AccessModule`.
///
/// ```no_run
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_db::DatabaseModule;
/// use navree_domains::DomainsModule;
/// use navree_http::HttpModule;
/// use navree_kernel::Navree;
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// Navree::app()
///     .module(ConfigModule::for_root(ConfigSources::defaults()))
///     .module(HttpModule::from_config())
///     .module(DatabaseModule::from_config())
///     .module(DomainsModule::from_config()) // section `domains`
///     .run()
/// # }
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct DomainsModule;

impl DomainsModule {
    /// The module with explicit options (configure it once, in the app
    /// root). Invalid options fail the boot.
    pub fn for_root(options: DomainsOptions) -> impl Module {
        ConfiguredDomainsModule {
            options: Some(options),
        }
    }

    /// The module configured from the `domains` section of `Config` at
    /// boot.
    pub fn from_config() -> impl Module {
        ConfiguredDomainsModule { options: None }
    }
}

impl Module for DomainsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

/// `DomainsModule::for_root` / `from_config`: same module id, configured.
struct ConfiguredDomainsModule {
    options: Option<DomainsOptions>,
}

impl Module for ConfiguredDomainsModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<DomainsModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, self.options.clone());
    }
}

fn register(m: &mut ModuleBuilder, options: Option<DomainsOptions>) {
    m.global();
    match options {
        Some(options) => m.factory(Factory::<DomainsOptions>::new(move |_| {
            let options = options.clone();
            async move {
                options
                    .check()
                    .map_err(|errors| format!("invalid domains options: {}", errors.join("; ")))?;
                Ok(Arc::new(options))
            }
        })),
        None => m.config::<DomainsOptions>(),
    };
    m.exports::<DomainsOptions>()
        .factory(Factory::<dyn DnsLookup>::new(|_| async {
            Ok(Arc::new(SystemDns::new()) as Arc<dyn DnsLookup>)
        }))
        .exports::<dyn DnsLookup>()
        .factory(
            Factory::<dyn AcmeClient>::new(|r: Resolver| async move {
                let options = r.get::<DomainsOptions>().await?;
                let client = match (&options.acme.ca_file, options.tls) {
                    (Some(path), TlsMode::Acme) => {
                        let pem = std::fs::read(path).map_err(|error| {
                            format!("domains.acme.ca_file `{path}` can't be read: {error}")
                        })?;
                        InstantAcme::with_root_pem(&pem)?
                    }
                    _ => InstantAcme::new()?,
                };
                Ok(Arc::new(client) as Arc<dyn AcmeClient>)
            })
            .deps(vec![ProviderKey::of::<DomainsOptions>()]),
        )
        .exports::<dyn AcmeClient>()
        .factory(Factory::<dyn CertificateProbe>::new(|_| async {
            Ok(Arc::new(TlsProbe::new()) as Arc<dyn CertificateProbe>)
        }))
        .exports::<dyn CertificateProbe>()
        .factory(
            Factory::<Domains>::new(|r: Resolver| async move {
                Ok(Arc::new(Domains::build(&r.app()).await?))
            })
            .deps(vec![
                ProviderKey::of::<DomainsOptions>(),
                ProviderKey::of::<Db>(),
                ProviderKey::of::<dyn DnsLookup>(),
                ProviderKey::of::<dyn AcmeClient>(),
                ProviderKey::of::<dyn CertificateProbe>(),
            ]),
        )
        .exports::<Domains>()
        .provider::<HttpsLauncher>()
        .lifecycle::<HttpsLauncher>()
        .controller::<DomainsController>()
        .controller::<DomainsPublicController>()
        .http_layer(GlobalLayer::new(LayerSlot::Context, "domains-host", |_ctx, router| {
            Ok(router.layer(from_fn(mark_domain)))
        }))
        .migrations(migrations())
        .map_error::<DomainsError>(map_domains_error)
        .listen::<WorkspaceDeleted, RemoveDeletedWorkspaces>()
        .schedule(|s| {
            s.command("domains:verify", &["--due"])
                .every_minute()
                .without_overlapping();
            s.command("domains:certificates", &["--due"])
                .hourly()
                .without_overlapping();
        });
    for command in commands() {
        m.command(command);
    }
}

/// Starts the HTTPS listener when the app runs `serve` with
/// `domains.tls = "acme"` and `domains.acme.https_addr`.
struct HttpsLauncher;

impl Injectable for HttpsLauncher {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(HttpsLauncher) })
    }
}

impl Lifecycle for HttpsLauncher {
    fn on_bootstrap(&self, ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move {
            if ctx.command().map(|command| command.name) != Some("serve") {
                return Ok(());
            }
            let options = ctx.get::<DomainsOptions>().await?;
            let Some(addr) = options.acme.https_addr.clone() else {
                return Ok(());
            };
            if options.tls != TlsMode::Acme {
                return Ok(());
            }
            let server = HttpsServer::bind(&ctx, &addr).await?;
            ctx.tracker().spawn(async move {
                if let Err(error) = server.serve().await {
                    tracing::error!(target: "navree::domains", %error, "the HTTPS server stopped");
                }
            });
            Ok(())
        })
    }
}

/// Removes the domains of a deleted workspace.
struct RemoveDeletedWorkspaces;

impl Injectable for RemoveDeletedWorkspaces {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(RemoveDeletedWorkspaces) })
    }
}

impl Listener<WorkspaceDeleted> for RemoveDeletedWorkspaces {
    fn handle<'a>(
        &'a self,
        event: &'a WorkspaceDeleted,
        ctx: &'a AppContext,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            let domains = ctx.get::<Domains>().await?;
            domains.remove_workspace(event.workspace_id).await?;
            Ok(())
        })
    }
}
