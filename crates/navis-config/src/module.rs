//! [`ConfigModule`], typed [`ConfigSection`]s and [`ModuleBuilderConfigExt`].

use std::sync::Arc;

use navis_kernel::{Factory, Module, ModuleBuilder, ModuleId, ProviderKey, Resolver};
use serde::de::DeserializeOwned;

use crate::{Config, ConfigSources};

/// The global module that loads [`Config`] at boot and exports it to every module.
///
/// ```no_run
/// use navis_config::{ConfigModule, ConfigSources};
/// use navis_kernel::Navis;
///
/// # fn main() -> Result<(), navis_kernel::BoxError> {
/// Navis::app()
///     .module(ConfigModule::for_root(ConfigSources::defaults()))
///     .run()
/// # }
/// ```
///
/// Used without `for_root` (`.module(ConfigModule)` or an import by type with
/// no configured instance anywhere in the app), it loads
/// [`ConfigSources::defaults`]. Sources are read when the app boots, on a
/// blocking thread; a malformed file fails the boot.
#[derive(Clone, Copy, Debug, Default)]
pub struct ConfigModule;

impl ConfigModule {
    /// The module configured with `sources` (configure it once, in the app root).
    pub fn for_root(sources: ConfigSources) -> impl Module {
        ConfiguredConfigModule { sources }
    }
}

impl Module for ConfigModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, ConfigSources::defaults());
    }
}

/// `ConfigModule::for_root(...)`: same module id, configured.
struct ConfiguredConfigModule {
    sources: ConfigSources,
}

impl Module for ConfiguredConfigModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<ConfigModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, self.sources.clone());
    }
}

/// Global; provides and exports the singleton [`Config`], loaded from `sources`.
fn register(m: &mut ModuleBuilder, sources: ConfigSources) {
    let sources = Arc::new(sources);
    m.global()
        .factory(Factory::<Config>::new(move |_| {
            let sources = sources.clone();
            async move {
                let loaded = {
                    let sources = sources.clone();
                    // Loading reads files: keep it off the async workers.
                    tokio::task::spawn_blocking(move || Config::load(&sources)).await??
                };
                tracing::debug!(sources = ?sources, "configuration loaded");
                Ok(Arc::new(loaded))
            }
        }))
        .exports::<Config>();
}

/// A typed configuration section, registered with
/// [`ModuleBuilderConfigExt::config`] (what `#[config(section = "...")]` generates).
///
/// ```
/// use navis_config::ConfigSection;
/// use serde::Deserialize;
///
/// #[derive(Deserialize)]
/// struct BillingConfig {
///     currency: String,
///     #[serde(default)]
///     trial_days: u32,
/// }
///
/// impl ConfigSection for BillingConfig {
///     const KEY: &'static str = "billing";
///
///     fn validate(&self) -> Result<(), String> {
///         if self.currency.len() == 3 {
///             Ok(())
///         } else {
///             Err("currency must be a 3-letter ISO code".into())
///         }
///     }
/// }
/// ```
pub trait ConfigSection: DeserializeOwned + Send + Sync + 'static {
    /// The section key, a dotted path into the configuration (`"billing"`,
    /// `"mail.smtp"`).
    const KEY: &'static str;

    /// Checks the deserialized values; an error fails the boot, naming
    /// [`KEY`](ConfigSection::KEY). The message must not echo secret values.
    fn validate(&self) -> Result<(), String> {
        Ok(())
    }
}

/// Registers typed configuration sections on a [`ModuleBuilder`].
pub trait ModuleBuilderConfigExt {
    /// Registers `T` as a singleton provider of this module (not exported),
    /// built at boot from the section [`T::KEY`](ConfigSection::KEY) of the
    /// global [`Config`] and then validated. A missing section deserializes
    /// from an empty table, so `#[serde(default)]` fields apply. A
    /// deserialization or validation error fails the boot with a
    /// `KernelError::Construction` naming the section.
    fn config<T: ConfigSection>(&mut self) -> &mut Self;
}

impl ModuleBuilderConfigExt for ModuleBuilder {
    fn config<T: ConfigSection>(&mut self) -> &mut Self {
        self.factory(
            Factory::<T>::new(|r: Resolver| async move {
                let config = r.get::<Config>().await?;
                Ok(Arc::new(config.typed::<T>()?))
            })
            .deps(vec![ProviderKey::of::<Config>()]),
        )
    }
}
