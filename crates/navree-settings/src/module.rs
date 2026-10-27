//! [`SettingsModule`] and [`ModuleBuilderSettingsExt`].

use std::marker::PhantomData;
use std::sync::Arc;

use navree_cache::Cache;
use navree_config::ModuleBuilderConfigExt;
use navree_contracts::Clock;
use navree_db::{Db, ModuleBuilderDbExt};
use navree_events::{Listener, ModuleBuilderEventsExt};
use navree_http::ModuleBuilderHttpExt;
use navree_kernel::{
    AppContext, BoxError, BoxFuture, Factory, Injectable, KernelError, Module, ModuleBuilder,
    ModuleId, ProviderKey, Resolver,
};
use navree_workspace::events::{
    OrganizationDeleted, WorkspaceAttached, WorkspaceDeleted, WorkspaceDetached,
};

use crate::commands::commands;
use crate::controller::SettingsController;
use crate::error::map_settings_error;
use crate::migration::migrations;
use crate::registry::SettingsRegistration;
use crate::service::Engine;
use crate::{
    DatabaseSettingsStore, MemorySettingsStore, ScopeKind, ScopeTarget, Settings,
    SettingsDefinition, SettingsGroup, SettingsOptions, SettingsRegistry, SettingsStore,
    SettingsStoreKind,
};

/// Typed settings with inheritance.
///
/// Global; provides and exports [`Settings`], [`SettingsOptions`],
/// [`SettingsRegistry`] and the `dyn` [`SettingsStore`], and contributes:
///
/// - the `settings` migrations (the `settings` table);
/// - `GET {api_prefix}/settings` and `GET`, `PATCH` and `DELETE
///   {api_prefix}/settings/{key}` ([`SettingsController`], unless
///   `settings.routes` is false);
/// - the commands `settings:list`, `settings:show`, `settings:set` and
///   `settings:inherit`;
/// - listeners that delete a deleted workspace's or organization's values
///   and forget a workspace's cached organization when it is attached or
///   detached (with `EventsModule`);
/// - the mapping of [`SettingsError`](crate::SettingsError)s returned from
///   handlers to error responses.
///
/// Register each settings type with
/// [`for_feature`](SettingsModule::for_feature) in the module that owns
/// it. The app must include `ConfigModule` and, with `settings.store =
/// "database"` (the default), `DatabaseModule`. With `CacheModule` in the
/// app, resolved values are cached there; with `AuditModule`, every change
/// is recorded; with `EventsModule`, every change emits `settings.changed`;
/// with `WorkspaceModule`, workspace scopes inherit their organization's
/// values.
///
/// ```no_run
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_db::DatabaseModule;
/// use navree_http::HttpModule;
/// use navree_kernel::Navree;
/// use navree_settings::SettingsModule;
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// Navree::app()
///     .module(ConfigModule::for_root(ConfigSources::defaults()))
///     .module(HttpModule::from_config())
///     .module(DatabaseModule::from_config())
///     // CacheModule, AuthModule, AccessModule, WorkspaceModule, ...
///     .module(SettingsModule::from_config()) // section `settings`
///     .run()
/// # }
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct SettingsModule;

impl SettingsModule {
    /// The module with explicit options (configure it once, in the app
    /// root).
    pub fn for_root(options: SettingsOptions) -> impl Module {
        ConfiguredSettingsModule {
            options: Some(options),
        }
    }

    /// The module configured from the `settings` section of `Config` at
    /// boot.
    pub fn from_config() -> impl Module {
        ConfiguredSettingsModule { options: None }
    }

    /// Registers settings type `T`. Import it in the module that owns the
    /// settings:
    ///
    /// ```ignore
    /// #[module(imports = [SettingsModule::for_feature::<PortalSettings>()], ..)]
    /// pub struct PortalModule;
    /// ```
    ///
    /// Two registered types with one key fail the boot, naming both, and so
    /// does a type with an invalid key, no level, or values that do not
    /// serialize to a JSON object.
    pub fn for_feature<T: SettingsGroup>() -> impl Module {
        SettingsFeature::<T>(PhantomData)
    }
}

impl Module for SettingsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

/// `SettingsModule::for_root` / `from_config`: same module id, configured.
struct ConfiguredSettingsModule {
    /// `None`: read from configuration.
    options: Option<SettingsOptions>,
}

impl Module for ConfiguredSettingsModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<SettingsModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, self.options.clone());
    }
}

/// `SettingsModule::for_feature::<T>()`.
struct SettingsFeature<T>(PhantomData<fn() -> T>);

impl<T: SettingsGroup> Module for SettingsFeature<T> {
    fn register(&self, m: &mut ModuleBuilder) {
        m.settings::<T>();
    }
}

fn register(m: &mut ModuleBuilder, options: Option<SettingsOptions>) {
    m.global();
    match options {
        Some(options) => m.factory(Factory::<SettingsOptions>::new(move |_| {
            let options = options.clone();
            async move { Ok(Arc::new(options)) }
        })),
        None => m.config::<SettingsOptions>(),
    };
    m.exports::<SettingsOptions>()
        .factory(Factory::<SettingsRegistry>::new(|r: Resolver| async move {
            let registrations = r.app().contributions::<SettingsRegistration>();
            let registry = SettingsRegistry::new(
                registrations
                    .into_iter()
                    .map(|contributed| contributed.value.0),
            )?;
            Ok(Arc::new(registry))
        }))
        .exports::<SettingsRegistry>()
        .factory(
            Factory::<dyn SettingsStore>::new(|r: Resolver| async move {
                let options = r.get::<SettingsOptions>().await?;
                let store: Arc<dyn SettingsStore> = match options.store {
                    SettingsStoreKind::Memory => Arc::new(MemorySettingsStore::new()),
                    SettingsStoreKind::Database => {
                        let db = r.app().try_get::<Db>().await.ok_or(
                            "settings.store = \"database\" needs DatabaseModule in the app \
                             (or set settings.store = \"memory\")",
                        )?;
                        Arc::new(DatabaseSettingsStore::new(db))
                    }
                };
                Ok(store)
            })
            .deps(vec![ProviderKey::of::<SettingsOptions>()]),
        )
        .exports::<dyn SettingsStore>()
        .factory(
            Factory::<Settings>::new(|r: Resolver| async move {
                let options = r.get::<SettingsOptions>().await?;
                let registry = r.get::<SettingsRegistry>().await?;
                let store = r.get::<dyn SettingsStore>().await?;
                let app = r.app();
                Ok(Arc::new(Settings::engine(Engine {
                    registry,
                    store,
                    transactional: options.store == SettingsStoreKind::Database,
                    options,
                    cache: app.try_get::<Cache>().await,
                    clock: app.try_get::<dyn Clock>().await,
                    app: Some(app.downgrade()),
                    fake: None,
                })))
            })
            .deps(vec![
                ProviderKey::of::<SettingsOptions>(),
                ProviderKey::of::<SettingsRegistry>(),
                ProviderKey::of::<dyn SettingsStore>(),
            ]),
        )
        .exports::<Settings>()
        .controller::<SettingsController>()
        .migrations(migrations())
        .map_error::<crate::SettingsError>(map_settings_error)
        .listen::<WorkspaceDeleted, ScopeListener>()
        .on::<OrganizationDeleted, ScopeListener>()
        .on::<WorkspaceAttached, ScopeListener>()
        .on::<WorkspaceDetached, ScopeListener>();
    for command in commands() {
        m.command(command);
    }
    // How notification mail looks, for navree-notify's `Notifier`.
    #[cfg(feature = "notify")]
    m.settings::<crate::notify::NotificationTemplates>()
        .factory(
            Factory::<dyn navree_notify::TemplateOverrides>::new(|r: Resolver| async move {
                let settings = r.get::<Settings>().await?;
                Ok(Arc::new(crate::notify::SettingsTemplateOverrides::new(settings))
                    as Arc<dyn navree_notify::TemplateOverrides>)
            })
            .deps(vec![ProviderKey::of::<Settings>()]),
        )
        .exports::<dyn navree_notify::TemplateOverrides>();
}

/// Settings registrations on a [`ModuleBuilder`].
pub trait ModuleBuilderSettingsExt {
    /// Registers settings type `T` (what
    /// `SettingsModule::for_feature::<T>()` does as an import).
    fn settings<T: SettingsGroup>(&mut self) -> &mut Self;
}

impl ModuleBuilderSettingsExt for ModuleBuilder {
    fn settings<T: SettingsGroup>(&mut self) -> &mut Self {
        self.contribute(SettingsRegistration(SettingsDefinition::of::<T>()))
    }
}

/// Keeps stored values in step with workspaces and organizations: deletes
/// a deleted one's values and forgets the cached organization of a
/// workspace that moved.
struct ScopeListener;

impl Injectable for ScopeListener {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(ScopeListener) })
    }
}

impl Listener<WorkspaceDeleted> for ScopeListener {
    fn handle<'a>(
        &'a self,
        event: &'a WorkspaceDeleted,
        ctx: &'a AppContext,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            let settings = ctx.get::<Settings>().await?;
            settings
                .forget_scope(ScopeTarget::new(ScopeKind::Workspace, event.workspace_id))
                .await?;
            Ok(())
        })
    }
}

impl Listener<OrganizationDeleted> for ScopeListener {
    fn handle<'a>(
        &'a self,
        event: &'a OrganizationDeleted,
        ctx: &'a AppContext,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            let settings = ctx.get::<Settings>().await?;
            settings
                .forget_scope(ScopeTarget::new(
                    ScopeKind::Organization,
                    event.organization_id,
                ))
                .await?;
            for workspace in &event.detached {
                settings.forget_organization_of(*workspace).await;
            }
            Ok(())
        })
    }
}

impl Listener<WorkspaceAttached> for ScopeListener {
    fn handle<'a>(
        &'a self,
        event: &'a WorkspaceAttached,
        ctx: &'a AppContext,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            let settings = ctx.get::<Settings>().await?;
            settings.forget_organization_of(event.workspace_id).await;
            Ok(())
        })
    }
}

impl Listener<WorkspaceDetached> for ScopeListener {
    fn handle<'a>(
        &'a self,
        event: &'a WorkspaceDetached,
        ctx: &'a AppContext,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            let settings = ctx.get::<Settings>().await?;
            settings.forget_organization_of(event.workspace_id).await;
            Ok(())
        })
    }
}
