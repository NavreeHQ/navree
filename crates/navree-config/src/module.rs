//! [`ConfigModule`], typed [`ConfigSection`]s and [`ModuleBuilderConfigExt`].

use std::sync::Arc;

use navree_kernel::{
    AppContext, BoxError, BoxFuture, ConfiguredModule, Factory, Lifecycle, Module, ModuleBuilder,
    ProviderKey, Resolver,
};
use serde::de::DeserializeOwned;

use crate::registry::{ConfigClaim, SectionRegistration, boot_error};
use crate::schema::SectionSchema;
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
/// - registers the offline commands `config:show`, `config:check`,
///   `config:schema` and `config:publish`.
#[derive(Clone, Copy, Debug, Default)]
pub struct ConfigModule;

impl ConfigModule {
    /// The module configured with `sources` (configure it once, in the app root).
    pub fn for_root(sources: ConfigSources) -> impl Module {
        ConfiguredModule::<ConfigModule, _>::for_root(sources, register_sources)
    }
}

impl Module for ConfigModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, ConfigSources::defaults());
    }
}

/// `for_root(sources)` registers with those sources; the plain module with
/// [`ConfigSources::defaults`].
fn register_sources(m: &mut ModuleBuilder, sources: Option<ConfigSources>) {
    register(m, sources.unwrap_or_else(ConfigSources::defaults));
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
                // A registered section is never a typo, also when an app
                // replaced the provider that reads it (a test's fake); nor
                // is a key a module claimed.
                let app = r.app();
                let mut claims: Vec<String> = app
                    .contributions::<SectionRegistration>()
                    .into_iter()
                    .map(|contributed| contributed.value.key().to_owned())
                    .collect();
                claims.extend(
                    app.contributions::<ConfigClaim>()
                        .into_iter()
                        .map(|contributed| contributed.value.key().to_owned()),
                );
                Ok(Arc::new(ConfigDiagnostics {
                    config: r.get::<Config>().await?,
                    claims,
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
        .command(commands::check())
        .command(commands::schema())
        .command(commands::publish());
}

/// Reports, once the app has booted, what loading the configuration noticed:
/// keys that `.env` and a config file both set with different values, keys
/// and sections that nothing reads, and, in an environment that behaves like
/// production, the settings the production profile fails. It waits until
/// every module has built its sections (and so read its keys), and for the
/// logger to be installed.
///
/// With `app.strict_config` the unknown keys and the failed settings stop
/// the boot instead of being logged.
struct ConfigDiagnostics {
    config: Arc<Config>,
    /// The keys of the sections the modules registered, and the keys they
    /// claimed.
    claims: Vec<String>,
}

impl Lifecycle for ConfigDiagnostics {
    fn on_bootstrap(&self, _ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move {
            let strict = self.config.app().is_ok_and(|app| app.strict_config);
            let mut problems: Vec<String> = Vec::new();
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
            let claims: Vec<&str> = self.claims.iter().map(String::as_str).collect();
            for unused in self.config.unused_with(&claims) {
                tracing::warn!(
                    target: "navree::config",
                    key = %unused.key,
                    origin = %unused.origin,
                    "{}",
                    unused.message()
                );
                problems.push(unused.message());
            }
            if self.config.behaves_like_production() {
                for finding in crate::check::production_profile(&self.config)
                    .into_iter()
                    .filter(|finding| finding.fail)
                {
                    tracing::error!(
                        target: "navree::config",
                        key = finding.key,
                        "unsafe in production: {}: {}",
                        finding.key,
                        finding.message
                    );
                    problems.push(format!("{}: {}", finding.key, finding.message));
                }
            }
            if strict && !problems.is_empty() {
                return Err(BoxError::from(format!(
                    "app.strict_config is on and the configuration has {} problem(s):\n  {}",
                    problems.len(),
                    problems.join("\n  ")
                )));
            }
            Ok(())
        })
    }
}

/// A typed configuration section, registered with
/// [`ModuleBuilderConfigExt::config`], which a module calls in `register`.
///
/// `#[derive(ConfigSection)]` writes the impl and describes the fields
/// (their names, types, rustdoc, secrets and defaults) for `config:show`,
/// `config:schema`, `config:publish` and the reference:
///
/// ```
/// use navree_config::ConfigSection;
/// use serde::Deserialize;
///
/// /// Billing settings.
/// #[derive(Default, Deserialize, ConfigSection)]
/// #[config(key = "billing", validate = Self::check)]
/// #[serde(default)]
/// struct BillingConfig {
///     /// The ISO code of the currency (`INR`).
///     currency: String,
///     /// Days of a free trial.
///     trial_days: u32,
/// }
///
/// impl BillingConfig {
///     fn check(&self) -> Result<(), String> {
///         if self.currency.len() == 3 {
///             Ok(())
///         } else {
///             Err("currency must be a 3-letter ISO code".into())
///         }
///     }
/// }
///
/// assert_eq!(BillingConfig::KEY, "billing");
/// assert_eq!(BillingConfig::schema().unwrap().fields[1].name, "trial_days");
/// assert_eq!(BillingConfig::defaults().unwrap()["trial_days"], 0);
/// ```
///
/// Written by hand, the impl needs only a key:
///
/// ```
/// use navree_config::ConfigSection;
/// use serde::Deserialize;
///
/// #[derive(Deserialize)]
/// struct BillingConfig {
///     currency: String,
/// }
///
/// impl ConfigSection for BillingConfig {
///     const KEY: &'static str = "billing";
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

    /// What the section says about itself: its fields, their types, rustdoc,
    /// credentials and environment fallbacks. `None` (the default) for a
    /// section written by hand; `#[derive(ConfigSection)]` fills it in.
    fn schema() -> Option<SectionSchema> {
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

    /// Registers `T` with the app ([`SectionRegistration`]) without building
    /// it: for a module that reads its section itself (it builds its options
    /// from [`Config::typed`], or from code), so `config:check`,
    /// `config:show` and the reference still know the section and its keys.
    fn config_section<T: ConfigSection>(&mut self) -> &mut Self;

    /// Claims `key` and the keys below it: the module reads them in a way
    /// the configuration cannot see, so `config:check` and the boot leave
    /// them out of the report on unknown keys. For a key that is not a
    /// registered section, such as `billing.<provider>`.
    fn config_claim(&mut self, key: &str) -> &mut Self;
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

    fn config_section<T: ConfigSection>(&mut self) -> &mut Self {
        self.contribute(SectionRegistration::of::<T>());
        self
    }

    fn config_claim(&mut self, key: &str) -> &mut Self {
        self.contribute(ConfigClaim::new(key));
        self
    }
}
