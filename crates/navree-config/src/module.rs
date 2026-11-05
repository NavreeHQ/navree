//! [`ConfigModule`], typed [`ConfigSection`]s and [`ModuleBuilderConfigExt`].

use std::sync::Arc;

use navree_kernel::{
    AppContext, BoxError, BoxFuture, Factory, Lifecycle, Module, ModuleBuilder, ModuleId,
    ProviderKey, Resolver,
};
use serde::de::DeserializeOwned;

use crate::registry::{SectionRegistration, boot_error};
use crate::{AppOptions, Config, ConfigSources, commands};

/// The global module that loads [`Config`] at boot and exports it to every module.
///
/// ```no_run
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_kernel::Navree;
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// Navree::app()
///     .module(ConfigModule::for_root(ConfigSources::defaults()))
///     .run()
/// # }
/// ```
///
/// Used without `for_root` (`.module(ConfigModule)` or an import by type with
/// no configured instance anywhere in the app), it loads
/// [`ConfigSources::defaults`]. Sources are read when the app boots, on a
/// blocking thread; a malformed file fails the boot.
///
/// The module also:
///
/// - provides [`AppOptions`] (the `app` section, validated at boot) and the
///   [`ConfigSources`] the app loads from;
/// - warns at boot about a key that `.env` and a config file both set with
///   different values ([`Config::shadowed`]) and about a key or section that
///   nothing reads
///   ([`Config::unused`]);
/// - lists the problems of every registered section when the boot fails on
///   one of them;
/// - registers the offline commands `config:show` and `config:check`.
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

/// Global; provides and exports the singleton [`Config`], loaded from
/// `sources`, the `sources` themselves and the typed [`AppOptions`].
fn register(m: &mut ModuleBuilder, sources: ConfigSources) {
    let sources = Arc::new(sources);
    let loaded_from = sources.clone();
    m.global()
        .value_arc(sources.clone())
        .factory(Factory::<Config>::new(move |_| {
            let sources = loaded_from.clone();
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
        .factory(
            Factory::<ConfigDiagnostics>::new(|r: Resolver| async move {
                Ok(Arc::new(ConfigDiagnostics {
                    config: r.get::<Config>().await?,
                }))
            })
            .deps(vec![ProviderKey::of::<Config>()]),
        )
        .lifecycle::<ConfigDiagnostics>()
        .exports::<Config>()
        .exports::<ConfigSources>()
        .config::<AppOptions>()
        .exports::<AppOptions>()
        .command(commands::show())
        .command(commands::check());
}

/// Reports, once the app has booted, what loading the configuration noticed:
/// keys that `.env` and a config file both set with different values, and
/// keys and sections that
/// nothing reads. It waits until every module has built its sections (and so
/// read its keys), and for the logger to be installed.
struct ConfigDiagnostics {
    config: Arc<Config>,
}

impl Lifecycle for ConfigDiagnostics {
    fn on_bootstrap(&self, _ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move {
            for shadow in self.config.shadowed() {
                tracing::warn!(
                    target: "navree::config",
                    key = %shadow.key,
                    hidden = %shadow.hidden,
                    winner = %shadow.winner,
                    "{}",
                    shadow.message()
                );
            }
            for unused in self.config.unused() {
                tracing::warn!(
                    target: "navree::config",
                    key = %unused.key,
                    origin = %unused.origin,
                    "{}",
                    unused.message()
                );
            }
            Ok(())
        })
    }
}

/// A typed configuration section, registered with
/// [`ModuleBuilderConfigExt::config`], which a module calls in `register`.
///
/// ```
/// use navree_config::ConfigSection;
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

    /// The values the section has when nothing configures it, as JSON, for
    /// `config:show --defaults`. Leave out credentials: `None` (the default)
    /// publishes nothing. [`defaults_of`] builds it from `Default` and
    /// `Serialize` for a section that has no secret field.
    fn defaults() -> Option<serde_json::Value> {
        None
    }
}

/// The defaults of `T` as JSON, for [`ConfigSection::defaults`]. `T` must not
/// hold credentials: it has to implement `Serialize`.
///
/// ```
/// use navree_config::{ConfigSection, defaults_of};
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Default, Deserialize, Serialize)]
/// #[serde(default)]
/// struct Billing {
///     currency: String,
///     trial_days: u32,
/// }
///
/// impl ConfigSection for Billing {
///     const KEY: &'static str = "billing";
///
///     fn defaults() -> Option<serde_json::Value> {
///         defaults_of::<Self>()
///     }
/// }
///
/// assert_eq!(Billing::defaults().unwrap()["trial_days"], 0);
/// ```
pub fn defaults_of<T: Default + serde::Serialize>() -> Option<serde_json::Value> {
    serde_json::to_value(T::default()).ok()
}

/// Registers typed configuration sections on a [`ModuleBuilder`].
pub trait ModuleBuilderConfigExt {
    /// Registers `T` as a singleton provider of this module (not exported),
    /// built at boot from the section [`T::KEY`](ConfigSection::KEY) of the
    /// global [`Config`] and then validated. A missing section deserializes
    /// from an empty table, so `#[serde(default)]` fields apply. A
    /// deserialization or validation error fails the boot with a
    /// `KernelError::Construction` naming the section; when more than one
    /// registered section is invalid, the error lists all of them
    /// ([`ConfigError::Invalid`](crate::ConfigError::Invalid)).
    ///
    /// The section is also registered with the app
    /// ([`SectionRegistration`]), so `config:check` can read it without
    /// booting.
    fn config<T: ConfigSection>(&mut self) -> &mut Self;
}

impl ModuleBuilderConfigExt for ModuleBuilder {
    fn config<T: ConfigSection>(&mut self) -> &mut Self {
        self.contribute(SectionRegistration::of::<T>());
        self.factory(
            Factory::<T>::new(|r: Resolver| async move {
                let config = r.get::<Config>().await?;
                match config.typed::<T>() {
                    Ok(section) => Ok(Arc::new(section)),
                    Err(first) => {
                        let registrations: Vec<SectionRegistration> = r
                            .app()
                            .contributions::<SectionRegistration>()
                            .into_iter()
                            .map(|c| (*c.value).clone())
                            .collect();
                        Err(BoxError::from(boot_error(&config, &registrations, first)))
                    }
                }
            })
            .deps(vec![ProviderKey::of::<Config>()]),
        )
    }
}
