//! [`HelmModule`] and [`ModuleBuilderHelmExt`].

use std::sync::Arc;

use navree_access::{AccessModule, Subject};
use navree_config::ModuleBuilderConfigExt;
use navree_contracts::ModuleBuilderJobsExt;
use navree_http::ModuleBuilderHttpExt;
use navree_kernel::{Factory, Injectable, Module, ModuleBuilder, ModuleId, ProviderKey, Resolver};

use crate::controller::HelmController;
use crate::core::Core;
use crate::page::PageRegistration;
use crate::registry::ResourceRegistration;
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
/// sign-in cookie, CSRF, error pages and a strict Content Security Policy)
/// and the `helm.action` job of queued actions. It has no tables.
///
/// The app needs `DatabaseModule`, `AccessModule` (every check), an
/// authenticator (`AuthModule`, whose users sign in to the panel) and
/// `CryptoModule` (the encrypted session cookie). Resources and pages are
/// registered by feature modules with [`ModuleBuilderHelmExt`].
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
///     // CryptoModule, AuthModule, your feature modules ...
///     .module(HelmModule::from_config()) // section `helm`
///     .run()
/// # }
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct HelmModule;

impl HelmModule {
    /// The module with explicit options (invalid options fail the boot).
    pub fn for_root(options: HelmOptions) -> impl Module {
        ConfiguredHelmModule {
            options: Some(options),
        }
    }

    /// The module configured from the `helm` section of `Config` at boot.
    pub fn from_config() -> impl Module {
        ConfiguredHelmModule { options: None }
    }
}

impl Module for HelmModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

/// `HelmModule::for_root` / `from_config`: same module id, configured.
struct ConfiguredHelmModule {
    options: Option<HelmOptions>,
}

impl Module for ConfiguredHelmModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<HelmModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, self.options.clone());
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
}
