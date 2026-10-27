//! [`IntegrationsModule`] and [`ModuleBuilderIntegrationsExt`].

use std::marker::PhantomData;
use std::sync::Arc;

use navree_config::ModuleBuilderConfigExt;
use navree_crypto::Encrypter;
use navree_db::{Db, ModuleBuilderDbExt};
use navree_events::{Listener, ModuleBuilderEventsExt};
use navree_http::ModuleBuilderHttpExt;
use navree_kernel::{
    AppContext, BoxError, BoxFuture, Factory, Injectable, KernelError, Module, ModuleBuilder,
    ModuleId, ProviderKey, Resolver,
};
use navree_schedule::ModuleBuilderScheduleExt;
use navree_workspace::events::{OrganizationDeleted, WorkspaceDeleted};

use crate::commands::commands;
use crate::controller::IntegrationsController;
use crate::error::map_integrations_error;
use crate::migration::migrations;
use crate::provider::ScopeSetRegistration;
use crate::runtime::Runtime;
use crate::{Integrations, IntegrationsError, IntegrationsOptions, Owner, ScopeSet};

/// Integrations: workspace and organization connections to third-party
/// APIs.
///
/// Global; provides and exports [`Integrations`] and
/// [`IntegrationsOptions`], and contributes:
///
/// - the routes under `{http.api_prefix}/integrations` unless
///   `integrations.routes` is false
///   ([`IntegrationsController`](crate::IntegrationsController); permission
///   `integrations.manage`);
/// - the `integrations` migration set (`integration_connections`);
/// - the commands `integrations:check` and `integrations:list`, and the
///   scheduled `integrations:check --due` every five minutes;
/// - listeners that forget a deleted workspace's or organization's
///   connections;
/// - the mapping of [`IntegrationsError`]s returned from handlers to error
///   responses.
///
/// The app must include `ConfigModule`, `CryptoModule` (credentials are
/// encrypted with the application key) and `DatabaseModule`.
/// `HttpClientModule` is used for provider calls when present (a default
/// client otherwise), `CacheModule` for token locks and used states (in
/// process otherwise), `WorkspaceModule` for workspace and organization
/// owners, `EventsModule` for the events, and the routes need an
/// authenticator and `AccessModule`.
///
/// ```no_run
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_crypto::CryptoModule;
/// use navree_db::DatabaseModule;
/// use navree_http::HttpModule;
/// use navree_http_client::HttpClientModule;
/// use navree_integrations::IntegrationsModule;
/// use navree_kernel::Navree;
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// Navree::app()
///     .module(ConfigModule::for_root(ConfigSources::defaults()))
///     .module(CryptoModule::from_config())
///     .module(HttpModule::from_config())
///     .module(HttpClientModule::from_config())
///     .module(DatabaseModule::from_config())
///     .module(IntegrationsModule::from_config()) // section `integrations`
///     .run()
/// # }
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct IntegrationsModule;

impl IntegrationsModule {
    /// The module with explicit options (configure it once, in the app
    /// root). Invalid options fail the boot.
    pub fn for_root(options: IntegrationsOptions) -> impl Module {
        ConfiguredIntegrationsModule {
            options: Some(options),
        }
    }

    /// The module configured from the `integrations` section of `Config`
    /// at boot.
    pub fn from_config() -> impl Module {
        ConfiguredIntegrationsModule { options: None }
    }

    /// Registers scope set `S` (import it in the feature module that needs
    /// it): it can then be connected, and named in
    /// [`Connection<P, S>`](crate::Connection).
    pub fn for_feature<S: ScopeSet>() -> impl Module {
        ScopeSetFeature::<S>(PhantomData)
    }
}

impl Module for IntegrationsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

/// `IntegrationsModule::for_root` / `from_config`: same module id,
/// configured.
struct ConfiguredIntegrationsModule {
    /// `None`: read from configuration.
    options: Option<IntegrationsOptions>,
}

impl Module for ConfiguredIntegrationsModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<IntegrationsModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, self.options.clone());
    }
}

fn register(m: &mut ModuleBuilder, options: Option<IntegrationsOptions>) {
    m.global();
    match options {
        Some(options) => m.factory(Factory::<IntegrationsOptions>::new(move |_| {
            let options = options.clone();
            async move {
                options.check().map_err(|errors| {
                    format!("invalid integrations options: {}", errors.join("; "))
                })?;
                Ok(Arc::new(options))
            }
        })),
        None => m.config::<IntegrationsOptions>(),
    };
    m.exports::<IntegrationsOptions>()
        .factory(
            Factory::<Integrations>::new(|r: Resolver| async move {
                let runtime = Runtime::build(&r.app(), None).await?;
                Ok(Arc::new(Integrations::real(runtime)))
            })
            .deps(vec![
                ProviderKey::of::<IntegrationsOptions>(),
                ProviderKey::of::<Encrypter>(),
                ProviderKey::of::<Db>(),
            ]),
        )
        .exports::<Integrations>()
        .lifecycle::<Integrations>()
        .controller::<IntegrationsController>()
        .migrations(migrations())
        .map_error::<IntegrationsError>(map_integrations_error)
        .listen::<WorkspaceDeleted, ForgetDeletedOwners>()
        .on::<OrganizationDeleted, ForgetDeletedOwners>()
        .schedule(|s| {
            s.command("integrations:check", &["--due"])
                .every_five_minutes()
                .without_overlapping();
        });
    for command in commands() {
        m.command(command);
    }
    // Connection tokens for navree-notify's Slack and Teams bot routes.
    #[cfg(feature = "notify")]
    m.factory(
        Factory::<dyn navree_notify::ConnectionTokens>::new(|r: Resolver| async move {
            let tokens = crate::IntegrationTokens::new(r.get::<Integrations>().await?);
            Ok(Arc::new(tokens) as Arc<dyn navree_notify::ConnectionTokens>)
        })
        .deps(vec![ProviderKey::of::<Integrations>()]),
    )
    .exports::<dyn navree_notify::ConnectionTokens>();
}

/// `IntegrationsModule::for_feature::<S>()`.
struct ScopeSetFeature<S>(PhantomData<fn() -> S>);

impl<S: ScopeSet> Module for ScopeSetFeature<S> {
    fn register(&self, m: &mut ModuleBuilder) {
        m.integration_scope_set::<S>();
    }
}

/// Integrations registrations on a [`ModuleBuilder`].
pub trait ModuleBuilderIntegrationsExt {
    /// Registers scope set `S` from this module (what
    /// [`IntegrationsModule::for_feature`] does, for a module's
    /// `register`).
    fn integration_scope_set<S: ScopeSet>(&mut self) -> &mut Self;
}

impl ModuleBuilderIntegrationsExt for ModuleBuilder {
    fn integration_scope_set<S: ScopeSet>(&mut self) -> &mut Self {
        self.contribute(ScopeSetRegistration::of::<S>())
    }
}

/// Forgets the connections of a deleted workspace or organization.
struct ForgetDeletedOwners;

impl Injectable for ForgetDeletedOwners {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(ForgetDeletedOwners) })
    }
}

impl Listener<WorkspaceDeleted> for ForgetDeletedOwners {
    fn handle<'a>(
        &'a self,
        event: &'a WorkspaceDeleted,
        ctx: &'a AppContext,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move { forget(ctx, Owner::Workspace(event.workspace_id)).await })
    }
}

impl Listener<OrganizationDeleted> for ForgetDeletedOwners {
    fn handle<'a>(
        &'a self,
        event: &'a OrganizationDeleted,
        ctx: &'a AppContext,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move { forget(ctx, Owner::Organization(event.organization_id)).await })
    }
}

async fn forget(ctx: &AppContext, owner: Owner) -> Result<(), BoxError> {
    let integrations = ctx.get::<Integrations>().await?;
    match integrations.forget(owner).await {
        Ok(_) | Err(IntegrationsError::NotBound) => Ok(()),
        Err(error) => Err(error.into()),
    }
}
