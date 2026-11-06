//! [`HelmModule`] and [`ModuleBuilderHelmExt`].

use std::future::Future;
use std::sync::Arc;

use navree_access::{AccessModule, Subject};
use navree_config::ModuleBuilderConfigExt;
use navree_contracts::ModuleBuilderJobsExt;
use navree_http::{AppError, ModuleBuilderHttpExt};
use navree_kernel::{
    AppContext, ConfiguredModule, Factory, Injectable, Module, ModuleBuilder, ProviderKey,
    Resolver,
};

use crate::controller::HelmController;
use crate::core::Core;
use crate::page::PageRegistration;
use crate::registry::ResourceRegistration;
use crate::workspace::{StaffEntry, StaffOrganizationEntry, StaffOrganizationPolicy, StaffPolicy};
use crate::{HelmActionJob, HelmOptions, HelmPage, Resource, session};

/// The subject that guards the panel: `view` on `Helm` (permission
/// `helm.view`) lets a user in.
#[derive(Clone, Copy, Debug, Default)]
pub struct Helm;

impl Subject for Helm {
    const SUBJECT: &'static str = "Helm";
    const PERMISSION_PREFIX: &'static str = "helm";
}

/// The admin panel, served at `helm.path` (`/admin`).
///
/// Global; provides and exports [`HelmOptions`], registers the subject
/// [`Helm`] (prefix `helm`), the panel's routes, its request layer (the
/// CSRF token of the browser's session, error pages and a strict Content
/// Security Policy) and the `helm.action` job of queued actions. It has no
/// tables.
///
/// The app needs `DatabaseModule`, `AccessModule` (every check) and
/// `CryptoModule`. People sign in to the panel the way they sign in to the
/// rest of the app, on the app's browser session: `SessionModule`,
/// `AuthModule`, `UiModule` and `AuthPages` (the sign-in pages). A guest is
/// sent to the sign-in page, and comes back to the page they asked for.
/// Resources and pages are registered by feature modules with
/// [`ModuleBuilderHelmExt`].
///
/// ```no_run
/// use navree_access::{AccessModule, AccessOptions};
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_db::DatabaseModule;
/// use navree_helm::HelmModule;
/// use navree_http::HttpModule;
/// use navree_kernel::Navree;
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// Navree::app()
///     .module(ConfigModule::for_root(ConfigSources::defaults()))
///     .module(HttpModule::from_config())
///     .module(DatabaseModule::from_config())
///     .module(AccessModule::for_root(AccessOptions::default()))
///     // CryptoModule, SessionModule, AuthModule, UiModule, AuthPages,
///     // your feature modules ...
///     .module(HelmModule::from_config()) // section `helm`
///     .run()
/// # }
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct HelmModule;

impl HelmModule {
    /// The module with explicit options (invalid options fail the boot).
    pub fn for_root(options: HelmOptions) -> impl Module {
        ConfiguredModule::<HelmModule, _>::for_root(options, register)
    }

    /// The module configured from the `helm` section of `Config` at boot.
    pub fn from_config() -> impl Module {
        ConfiguredModule::<HelmModule, HelmOptions>::from_config(register)
    }
}

impl Module for HelmModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

fn register(m: &mut ModuleBuilder, options: Option<HelmOptions>) {
    m.global();
    match options {
        Some(options) => m.factory(Factory::<HelmOptions>::new(move |_| {
            let options = options.clone();
            async move {
                options
                    .check()
                    .map_err(|errors| format!("invalid helm options: {}", errors.join("; ")))?;
                Ok(Arc::new(options))
            }
        })),
        None => m.config::<HelmOptions>(),
    };
    m.exports::<HelmOptions>()
        .imports(AccessModule::for_subject::<Helm>())
        // The panel's error pages are the toolkit's, and the sign-in page's
        // address (`ui.sign_in`) is its setting.
        .import(navree_ui::UiOnDemand)
        .contribute(navree_ui::UiNeed::when(|_ctx: AppContext| async move { true }))
        .factory(
            Factory::<Core>::new(|r: Resolver| async move {
                let options = r.get::<HelmOptions>().await?;
                Ok(Arc::new(Core::new(r.app(), options).await?))
            })
            .deps(vec![ProviderKey::of::<HelmOptions>()]),
        )
        .controller::<HelmController>()
        .http_layer(session::layer())
        .job::<HelmActionJob>();
}

/// Helm registrations on a [`ModuleBuilder`].
pub trait ModuleBuilderHelmExt {
    /// Registers resource `R` as a provider of this module and adds it to
    /// the panel. Its model's subject is registered with navree-access
    /// (`AccessModule::for_feature`), so permissions on its prefix map to
    /// it.
    fn resource<R: Resource + Injectable>(&mut self) -> &mut Self;

    /// Adds a custom page to the panel.
    fn helm_page(&mut self, page: HelmPage) -> &mut Self;

    /// Lets the users `may_enter` says work in workspaces they are not members
    /// of: support staff, an operator. Without this nobody does; a panel user
    /// works only in the workspaces they are a member of. Each request made
    /// that way is audited. Register it once.
    ///
    /// Inside a workspace entered this way a user keeps their app-wide
    /// abilities. In an app without `WorkspaceModule` this is the only way a
    /// request can name a workspace, and nothing checks that the workspace
    /// exists unless `may_enter` does.
    ///
    /// ```no_run
    /// # use navree_helm::ModuleBuilderHelmExt;
    /// # fn register(m: &mut navree_kernel::ModuleBuilder) {
    /// m.helm_staff_workspaces(|entry| async move {
    ///     Ok(entry.access.ability.can("support", "Workspace", None))
    /// });
    /// # }
    /// ```
    fn helm_staff_workspaces<F, Fut>(&mut self, may_enter: F) -> &mut Self
    where
        F: Fn(StaffEntry) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<bool, AppError>> + Send + 'static;

    /// Lets the users `may_enter` says work with the settings of organizations
    /// they are not members of, on the Settings screens. Without this nobody does.
    /// Each request made that way writes a `helm.organization.staff` audit entry,
    /// reads included. Register it once.
    fn helm_staff_organizations<F, Fut>(&mut self, may_enter: F) -> &mut Self
    where
        F: Fn(StaffOrganizationEntry) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<bool, AppError>> + Send + 'static;
}

impl ModuleBuilderHelmExt for ModuleBuilder {
    fn resource<R: Resource + Injectable>(&mut self) -> &mut Self {
        let module = self.module_id();
        self.provider::<R>()
            .imports(AccessModule::for_feature::<R::Model>())
            .contribute(ResourceRegistration::of::<R>(module))
    }

    fn helm_page(&mut self, page: HelmPage) -> &mut Self {
        let module = self.module_id();
        self.contribute(PageRegistration { page, module })
    }

    fn helm_staff_workspaces<F, Fut>(&mut self, may_enter: F) -> &mut Self
    where
        F: Fn(StaffEntry) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<bool, AppError>> + Send + 'static,
    {
        self.contribute(StaffPolicy::new(may_enter))
    }

    fn helm_staff_organizations<F, Fut>(&mut self, may_enter: F) -> &mut Self
    where
        F: Fn(StaffOrganizationEntry) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<bool, AppError>> + Send + 'static,
    {
        self.contribute(StaffOrganizationPolicy::new(may_enter))
    }
}
