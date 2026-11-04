//! [`CryptoModule`] and [`CryptoOptions`].

use std::sync::Arc;

use navree_config::Config;
use navree_contracts::Secret;
use navree_kernel::{
    BoxError, CommandSpec, Factory, Module, ModuleBuilder, ModuleId, ProviderKey, Resolver,
};

use crate::{AppKey, Encrypter, Hasher, HashingOptions, generate_key};

/// Options for [`CryptoModule::for_root`]: the application key, the keys
/// that still decrypt after a rotation, and the password hashing parameters.
///
/// Keys are given in their `base64:<32 bytes>` form ([`generate_key`]) and
/// parsed when the app boots, so a malformed key fails the boot. `Debug`
/// redacts them.
///
/// ```
/// use navree_crypto::{CryptoOptions, HashingOptions, generate_key};
///
/// let options = CryptoOptions::new(generate_key())
///     .previous_key(generate_key())
///     .hashing(HashingOptions { memory_kib: 65_536, ..HashingOptions::default() });
/// ```
#[derive(Clone, Debug, Default)]
pub struct CryptoOptions {
    /// The current key (`base64:...`). Without one the app still boots (so
    /// `key:generate` can run), but every `encrypt*` call fails with
    /// [`CryptoError::MissingKey`](crate::CryptoError::MissingKey).
    pub key: Option<Secret<String>>,
    /// Keys that only decrypt, most recent first.
    pub previous_keys: Vec<Secret<String>>,
    /// Argon2id parameters and the hashing concurrency limit.
    pub hashing: HashingOptions,
}

impl CryptoOptions {
    /// Options with `key` (`base64:<32 bytes>`) as the current key and
    /// default hashing parameters.
    pub fn new(key: impl Into<String>) -> Self {
        Self {
            key: Some(Secret::new(key.into())),
            ..Self::default()
        }
    }

    /// Adds a previous key that still decrypts older payloads.
    pub fn previous_key(mut self, key: impl Into<String>) -> Self {
        self.previous_keys.push(Secret::new(key.into()));
        self
    }

    /// Sets the password hashing options.
    pub fn hashing(mut self, hashing: HashingOptions) -> Self {
        self.hashing = hashing;
        self
    }

    /// Reads the options from configuration:
    ///
    /// - `app.key` (`APP_KEY`); empty means not set;
    /// - `app.previous_keys` (`APP_PREVIOUS_KEYS`): a list, or a
    ///   comma-separated string;
    /// - section `hashing` ([`HashingOptions`], every field optional).
    ///
    /// Errors name the configuration key, never its value.
    pub fn from_config(config: &Config) -> Result<Self, BoxError> {
        Ok(Self {
            hashing: hashing_from_config(config)?,
            ..Self::keys_from_config(config)?
        })
    }

    /// The key options from configuration (hashing left at its defaults).
    fn keys_from_config(config: &Config) -> Result<Self, BoxError> {
        let app = config.app()?;
        Ok(Self {
            key: app.key,
            previous_keys: app.previous_keys,
            hashing: HashingOptions::default(),
        })
    }

    /// Builds the [`Encrypter`]; errors name the offending option.
    fn encrypter(&self) -> Result<Encrypter, BoxError> {
        let previous = self
            .previous_keys
            .iter()
            .map(|key| AppKey::parse(key.expose()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("`app.previous_keys` contains an invalid key: {error}"))?;
        let encrypter = match &self.key {
            Some(key) => Encrypter::new(
                AppKey::parse(key.expose()).map_err(|error| format!("`app.key`: {error}"))?,
            ),
            None => {
                tracing::warn!(
                    "no application key is configured (`app.key` / APP_KEY): encryption is \
                     disabled until one is set; run `key:generate` to create one"
                );
                Encrypter::without_key()
            }
        };
        Ok(encrypter.with_previous_keys(previous))
    }

    /// Builds the [`Hasher`]; errors name the `hashing` options.
    fn hasher(&self) -> Result<Hasher, BoxError> {
        Hasher::new(&self.hashing).map_err(|error| format!("`hashing`: {error}").into())
    }
}

/// Section `hashing` (absent: the defaults).
fn hashing_from_config(config: &Config) -> Result<HashingOptions, BoxError> {
    Ok(config.section_or_default::<HashingOptions>("hashing")?)
}

/// The global crypto module: provides and exports [`Encrypter`] and
/// [`Hasher`], and registers the `key:generate` command.
///
/// - [`CryptoModule::for_root`] takes [`CryptoOptions`] in code;
/// - [`CryptoModule::from_config`] (and a plain `.module(CryptoModule)` or
///   import with no configured instance in the app) reads them at boot with
///   [`CryptoOptions::from_config`], so the app must include `ConfigModule`.
///
/// ```no_run
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_crypto::CryptoModule;
/// use navree_kernel::Navree;
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// // .env: APP_KEY=base64:...   (generate one with `<app> key:generate`)
/// Navree::app()
///     .module(ConfigModule::for_root(ConfigSources::defaults()))
///     .module(CryptoModule::from_config())
///     .run()
/// # }
/// ```
///
/// A malformed key or invalid hashing parameters fail the boot, naming the
/// option. A missing key does not: the app boots (so `key:generate` can run)
/// with a warning, and encryption fails until a key is set.
#[derive(Clone, Copy, Debug, Default)]
pub struct CryptoModule;

impl CryptoModule {
    /// The module configured with `options` (configure it once, in the app root).
    pub fn for_root(options: CryptoOptions) -> impl Module {
        ConfiguredCryptoModule {
            source: Source::Options(Arc::new(options)),
        }
    }

    /// The module configured from the app's `Config` at boot (see
    /// [`CryptoOptions::from_config`]).
    pub fn from_config() -> impl Module {
        ConfiguredCryptoModule {
            source: Source::Config,
        }
    }
}

impl Module for CryptoModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, Source::Config);
    }
}

/// Where the options come from.
#[derive(Clone)]
enum Source {
    Options(Arc<CryptoOptions>),
    Config,
}

impl Source {
    /// Builds the [`Encrypter`] (reading only the key options from `Config`).
    async fn encrypter(&self, r: &Resolver) -> Result<Encrypter, BoxError> {
        match self {
            Source::Options(options) => options.encrypter(),
            Source::Config => {
                CryptoOptions::keys_from_config(&*r.get::<Config>().await?)?.encrypter()
            }
        }
    }

    /// Builds the [`Hasher`] (reading only section `hashing` from `Config`).
    async fn hasher(&self, r: &Resolver) -> Result<Hasher, BoxError> {
        match self {
            Source::Options(options) => options.hasher(),
            Source::Config => CryptoOptions {
                hashing: hashing_from_config(&*r.get::<Config>().await?)?,
                ..CryptoOptions::default()
            }
            .hasher(),
        }
    }

    /// The keys the factories resolve.
    fn deps(&self) -> Vec<ProviderKey> {
        match self {
            Source::Options(_) => Vec::new(),
            Source::Config => vec![ProviderKey::of::<Config>()],
        }
    }
}

/// `CryptoModule::for_root(...)` / `from_config()`: same module id, configured.
struct ConfiguredCryptoModule {
    source: Source,
}

impl Module for ConfiguredCryptoModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<CryptoModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, self.source.clone());
    }
}

/// Global; provides and exports `Encrypter` and `Hasher`; adds `key:generate`.
fn register(m: &mut ModuleBuilder, source: Source) {
    let deps = source.deps();
    let encrypter_source = source.clone();
    m.global()
        .factory(
            Factory::<Encrypter>::new(move |r| {
                let source = encrypter_source.clone();
                async move { Ok(Arc::new(source.encrypter(&r).await?)) }
            })
            .deps(deps.clone()),
        )
        .exports::<Encrypter>()
        .factory(
            Factory::<Hasher>::new(move |r| {
                let source = source.clone();
                async move { Ok(Arc::new(source.hasher(&r).await?)) }
            })
            .deps(deps),
        )
        .exports::<Hasher>()
        .command(CommandSpec::new(
            "key:generate",
            "Print a new application key for `app.key` (APP_KEY)",
            |_, _| async {
                println!("{}", generate_key());
                Ok(())
            },
        ));
}
