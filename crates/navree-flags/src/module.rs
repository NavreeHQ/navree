//! [`FlagsModule`] and [`ModuleBuilderFlagsExt`].

use std::marker::PhantomData;
use std::sync::Arc;

use navree_cache::Cache;
use navree_config::ModuleBuilderConfigExt;
use navree_contracts::{Clock, SystemClock};
use navree_db::{Db, ModuleBuilderDbExt};
use navree_http::ModuleBuilderHttpExt;
use navree_kernel::{Factory, Injectable, Module, ModuleBuilder, ModuleId, ProviderKey, Resolver};

use crate::commands::commands;
use crate::controller::{FlagsAdminController, FlagsController};
use crate::error::map_flags_error;
use crate::migration::migrations;
use crate::registry::FlagRegistration;
use crate::service::Engine;
use crate::{
    DatabaseFlagStore, Flag, FlagDefinition, FlagRegistry, FlagStore, FlagStoreKind, Flags,
    FlagsError, FlagsOptions, MemoryFlagStore, PlanResolver,
};

/// Feature flags.
///
/// Global; provides and exports [`Flags`], [`FlagsOptions`],
/// [`FlagRegistry`] and the `dyn` [`FlagStore`], and contributes:
///
/// - the `flags` migrations (`feature_flags`, `feature_flag_overrides`);
/// - `GET {api_prefix}/flags` ([`FlagsController`], unless
///   `flags.routes` is false) and `{api_prefix}/admin/flags...`
///   ([`FlagsAdminController`], permission `flags.manage`, unless
///   `flags.admin_routes` is false);
/// - the commands `flags:list`, `flags:on`, `flags:off`, `flags:forget`,
///   `flags:rollout`, `flags:kill`, `flags:revive` and `flags:purge`;
/// - the mapping of [`FlagsError`]s returned from handlers to error
///   responses.
///
/// Register each flag with [`for_feature`](FlagsModule::for_feature) in
/// the module that owns it. The app must include `ConfigModule` (for
/// [`from_config`](FlagsModule::from_config) and the plain module) and,
/// with `flags.store = "database"` (the default), `DatabaseModule`. With
/// `CacheModule` in the app, flag state is cached there; with
/// `AuditModule`, every change is recorded; with `EventsModule`, every
/// change emits `flag.changed`; with `WorkspaceModule`, requests resolve
/// flags for their workspace too.
///
/// ```no_run
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_db::DatabaseModule;
/// use navree_flags::FlagsModule;
/// use navree_http::HttpModule;
/// use navree_kernel::Navree;
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// Navree::app()
///     .module(ConfigModule::for_root(ConfigSources::defaults()))
///     .module(HttpModule::from_config())
///     .module(DatabaseModule::from_config())
///     // CacheModule, AuthModule, AccessModule, WorkspaceModule, ...
///     .module(FlagsModule::from_config()) // section `flags`
///     .run()
/// # }
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct FlagsModule;

impl FlagsModule {
    /// The module with explicit options (configure it once, in the app
    /// root).
    pub fn for_root(options: FlagsOptions) -> impl Module {
        ConfiguredFlagsModule {
            options: Some(options),
        }
    }

    /// The module configured from the `flags` section of `Config` at boot.
    pub fn from_config() -> impl Module {
        ConfiguredFlagsModule { options: None }
    }

    /// Registers flag `F`. Import it in the module that owns the flag:
    ///
    /// ```ignore
    /// #[module(imports = [FlagsModule::for_feature::<NewPipeline>()], ..)]
    /// pub struct DealsModule;
    /// ```
    ///
    /// Two registered flag types with one name fail the boot, naming both.
    pub fn for_feature<F: Flag>() -> impl Module {
        FlagFeature::<F>(PhantomData)
    }
}

impl Module for FlagsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

/// `FlagsModule::for_root` / `from_config`: same module id, configured.
struct ConfiguredFlagsModule {
    /// `None`: read from configuration.
    options: Option<FlagsOptions>,
}

impl Module for ConfiguredFlagsModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<FlagsModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, self.options.clone());
    }
}

/// `FlagsModule::for_feature::<F>()`.
struct FlagFeature<F>(PhantomData<fn() -> F>);

impl<F: Flag> Module for FlagFeature<F> {
    fn register(&self, m: &mut ModuleBuilder) {
        m.contribute(FlagRegistration(FlagDefinition::of::<F>()));
    }
}

fn register(m: &mut ModuleBuilder, options: Option<FlagsOptions>) {
    m.global();
    match options {
        Some(options) => m.factory(Factory::<FlagsOptions>::new(move |_| {
            let options = options.clone();
            async move { Ok(Arc::new(options)) }
        })),
        None => m.config::<FlagsOptions>(),
    };
    m.exports::<FlagsOptions>()
        .factory(Factory::<FlagRegistry>::new(|r: Resolver| async move {
            let registrations = r.app().contributions::<FlagRegistration>();
            let registry = FlagRegistry::new(
                registrations
                    .into_iter()
                    .map(|contributed| contributed.value.0),
            )?;
            Ok(Arc::new(registry))
        }))
        .exports::<FlagRegistry>()
        .factory(
            Factory::<dyn FlagStore>::new(|r: Resolver| async move {
                let options = r.get::<FlagsOptions>().await?;
                let store: Arc<dyn FlagStore> = match options.store {
                    FlagStoreKind::Memory => Arc::new(MemoryFlagStore::new()),
                    FlagStoreKind::Database => {
                        let db = r.app().try_get::<Db>().await.ok_or(
                            "flags.store = \"database\" needs DatabaseModule in the app \
                             (or set flags.store = \"memory\")",
                        )?;
                        Arc::new(DatabaseFlagStore::new(db))
                    }
                };
                Ok(store)
            })
            .deps(vec![ProviderKey::of::<FlagsOptions>()]),
        )
        .exports::<dyn FlagStore>()
        .factory(
            Factory::<Flags>::new(|r: Resolver| async move {
                let options = r.get::<FlagsOptions>().await?;
                let registry = r.get::<FlagRegistry>().await?;
                let store = r.get::<dyn FlagStore>().await?;
                let app = r.app();
                let clock = match app.try_get::<dyn Clock>().await {
                    Some(clock) => clock,
                    None => Arc::new(SystemClock) as Arc<dyn Clock>,
                };
                let db = match options.store {
                    FlagStoreKind::Database => app.try_get::<Db>().await,
                    FlagStoreKind::Memory => None,
                };
                Ok(Arc::new(Flags::engine(Engine {
                    registry,
                    store,
                    db,
                    cache: app.try_get::<Cache>().await,
                    clock,
                    app: app.downgrade(),
                    options,
                })))
            })
            .deps(vec![
                ProviderKey::of::<FlagsOptions>(),
                ProviderKey::of::<FlagRegistry>(),
                ProviderKey::of::<dyn FlagStore>(),
            ]),
        )
        .exports::<Flags>()
        .controller::<FlagsController>()
        .controller::<FlagsAdminController>()
        .migrations(migrations())
        .map_error::<FlagsError>(map_flags_error);
    for command in commands() {
        m.command(command);
    }
}

/// Flag registrations on a [`ModuleBuilder`].
pub trait ModuleBuilderFlagsExt {
    /// Registers flag `F` (what `FlagsModule::for_feature::<F>()` does as
    /// an import).
    fn flag<F: Flag>(&mut self) -> &mut Self;

    /// Registers provider `R` as the app's `dyn PlanResolver`, which
    /// rollouts by plan read (billing registers its own).
    fn plan_resolver<R: PlanResolver + Injectable>(&mut self) -> &mut Self;
}

impl ModuleBuilderFlagsExt for ModuleBuilder {
    fn flag<F: Flag>(&mut self) -> &mut Self {
        self.contribute(FlagRegistration(FlagDefinition::of::<F>()))
    }

    fn plan_resolver<R: PlanResolver + Injectable>(&mut self) -> &mut Self {
        self.provider::<R>()
            .factory(
                Factory::<dyn PlanResolver>::new(|r: Resolver| async move {
                    Ok(r.get::<R>().await? as Arc<dyn PlanResolver>)
                })
                .deps(vec![ProviderKey::of::<R>()]),
            )
            .exports::<dyn PlanResolver>()
    }
}
