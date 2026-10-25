//! [`NotifyModule`].

use std::sync::Arc;

use keel_config::{Config, ConfigSection};
use keel_contracts::ModuleBuilderJobsExt;
use keel_db::ModuleBuilderDbExt;
use keel_http::ModuleBuilderHttpExt;
use keel_kernel::{BoxError, Factory, Module, ModuleBuilder, ModuleId, ProviderKey, Resolver};

use crate::controller::NotificationsController;
use crate::{Notifier, NotifyOptions, SendNotificationJob, migration};

/// The global notification module: provides and exports [`Notifier`],
/// registers job `notify.send` ([`SendNotificationJob`]) and the `notify`
/// migrations (table `notifications`).
///
/// - [`NotifyModule::for_root`] takes [`NotifyOptions`] in code;
/// - [`NotifyModule::from_config`] (and a plain `.module(NotifyModule)`)
///   reads section `notify` at boot, so the app must include `ConfigModule`.
///
/// The channels use what the app provides: the database channel needs
/// `DatabaseModule`, the mail channel `MailModule` (and `ViewModule`), the
/// broadcast channel a `dyn Broadcaster` (`RealtimeModule`), and queueing a
/// `dyn Dispatcher` (`QueueModule`). They are looked up when a notification
/// is sent, so a missing one fails only the channels that need it.
#[derive(Clone, Copy, Debug, Default)]
pub struct NotifyModule;

impl NotifyModule {
    /// The module configured with `options` (configure it once, in the app root).
    pub fn for_root(options: NotifyOptions) -> impl Module {
        ConfiguredNotifyModule {
            source: Source::Options(Arc::new(options)),
        }
    }

    /// The module configured from section `notify` of the app's `Config`
    /// at boot (see [`NotifyOptions`]).
    pub fn from_config() -> impl Module {
        ConfiguredNotifyModule {
            source: Source::Config,
        }
    }
}

impl Module for NotifyModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, Source::Config);
    }
}

/// `NotifyModule::for_root(...)` / `from_config()`: same module id, configured.
struct ConfiguredNotifyModule {
    source: Source,
}

impl Module for ConfiguredNotifyModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<NotifyModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, self.source.clone());
    }
}

/// Where the options come from.
#[derive(Clone)]
enum Source {
    Options(Arc<NotifyOptions>),
    Config,
}

impl Source {
    /// The validated options.
    async fn options(&self, r: &Resolver) -> Result<NotifyOptions, BoxError> {
        match self {
            Source::Options(options) => {
                options
                    .validate()
                    .map_err(|message| format!("notify options are invalid: {message}"))?;
                Ok(NotifyOptions::clone(options))
            }
            Source::Config => Ok(r.get::<Config>().await?.typed::<NotifyOptions>()?),
        }
    }

    /// The keys the options factory resolves.
    fn deps(&self) -> Vec<ProviderKey> {
        match self {
            Source::Options(_) => Vec::new(),
            Source::Config => vec![ProviderKey::of::<Config>()],
        }
    }
}

/// Global; provides the options (private) and the notifier (exported), and
/// registers the job and the migrations.
fn register(m: &mut ModuleBuilder, source: Source) {
    let deps = source.deps();
    m.global()
        .factory(
            Factory::<NotifyOptions>::new(move |r| {
                let source = source.clone();
                async move { Ok(Arc::new(source.options(&r).await?)) }
            })
            .deps(deps),
        )
        .factory(
            Factory::<Notifier>::new(|r| async move {
                let options = r.get::<NotifyOptions>().await?;
                Ok(Arc::new(Notifier::new(
                    NotifyOptions::clone(&options),
                    r.app().downgrade(),
                )))
            })
            .deps(vec![ProviderKey::of::<NotifyOptions>()]),
        )
        .exports::<Notifier>()
        .job::<SendNotificationJob>()
        .migrations(migration::migrations())
        .controller::<NotificationsController>();
}
