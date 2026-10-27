//! [`NotifyModule`].

use std::marker::PhantomData;
use std::sync::Arc;

use navree_config::{Config, ConfigSection};
use navree_contracts::ModuleBuilderJobsExt;
use navree_db::ModuleBuilderDbExt;
use navree_http::ModuleBuilderHttpExt;
use navree_kernel::{
    AppContext, BoxError, Factory, Module, ModuleBuilder, ModuleId, ProviderKey, Resolver,
};
use navree_schedule::ModuleBuilderScheduleExt;
use navree_telemetry::Telemetry;
use navree_validation::Country;
use navree_view::ModuleBuilderViewExt;

use crate::catalog::NotificationRegistration;
use crate::channels::ChannelDrivers;
use crate::controller::NotificationsController;
use crate::digest::DIGEST_TEMPLATES;
use crate::notifier::Parts;
use crate::{
    DeliveryLog, ModuleBuilderNotifyExt, Notification, NotificationCatalog,
    NotificationPreferences, Notifier, NotifyOptions, OptOuts, PushSubscriptions, Released,
    SendNotificationJob, commands, migration,
};

/// The global notification module: provides and exports [`Notifier`],
/// [`DeliveryLog`], [`OptOuts`] and [`PushSubscriptions`], registers job
/// `notify.send` ([`SendNotificationJob`]), command `notify:vapid`, the
/// routes and the `notify` migrations (tables `notifications`,
/// `notification_deliveries`, `notification_opt_outs`,
/// `push_subscriptions`).
///
/// - [`NotifyModule::for_root`] takes [`NotifyOptions`] in code;
/// - [`NotifyModule::from_config`] (and a plain `.module(NotifyModule)`)
///   reads section `notify` at boot, so the app must include `ConfigModule`.
///
/// The channels use what the app provides: the database channel (and the
/// delivery log, opt-outs and push subscriptions) needs `DatabaseModule`, the
/// mail channel `MailModule` (and `ViewModule`), the broadcast channel a
/// `dyn Broadcaster` (`RealtimeModule`), the SMS, WhatsApp and web push
/// channels a driver (`notify.sms.driver`, `notify.whatsapp.driver`,
/// `notify.web_push`, or the app's `dyn SmsDriver`...), and queueing a
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

    /// Registers notification type `N` (import it in the module that owns
    /// the notification): its category is listed by
    /// `GET /api/notifications/preferences` and accepted by its `PATCH`.
    /// The same as `m.notification::<N>()`
    /// ([`ModuleBuilderNotifyExt`](crate::ModuleBuilderNotifyExt)).
    pub fn for_feature<N: Notification>() -> impl Module {
        NotificationFeature::<N>(PhantomData)
    }
}

/// `NotifyModule::for_feature::<N>()`.
struct NotificationFeature<N>(PhantomData<fn() -> N>);

impl<N: Notification> Module for NotificationFeature<N> {
    fn id(&self) -> ModuleId {
        ModuleId::of::<NotificationFeature<N>>()
    }

    fn register(&self, m: &mut ModuleBuilder) {
        m.notification::<N>();
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

/// Global; provides the options (private), the notifier, the delivery log,
/// the opt-outs and the push subscriptions (exported), and registers the
/// job, the migrations, the routes and `notify:vapid`.
fn register(m: &mut ModuleBuilder, source: Source) {
    let deps = source.deps();
    let options_key = || vec![ProviderKey::of::<NotifyOptions>()];
    m.global()
        .factory(
            Factory::<NotifyOptions>::new(move |r| {
                let source = source.clone();
                async move { Ok(Arc::new(source.options(&r).await?)) }
            })
            .deps(deps),
        )
        .factory(
            Factory::<OptOuts>::new(|r| async move {
                let options = r.get::<NotifyOptions>().await?;
                Ok(Arc::new(OptOuts::new(
                    r.app().downgrade(),
                    region(&options),
                )))
            })
            .deps(options_key()),
        )
        .exports::<OptOuts>()
        .factory(
            Factory::<DeliveryLog>::new(|r| async move {
                let options = r.get::<NotifyOptions>().await?;
                let app = r.app().downgrade();
                let opt_outs = OptOuts::new(app.clone(), region(&options));
                Ok(Arc::new(DeliveryLog::new(app, opt_outs)))
            })
            .deps(options_key()),
        )
        .exports::<DeliveryLog>()
        .factory(Factory::<PushSubscriptions>::new(|r| async move {
            Ok(Arc::new(PushSubscriptions::new(r.app().downgrade())))
        }))
        .exports::<PushSubscriptions>()
        .factory(Factory::<NotificationCatalog>::new(|r| async move {
            let registrations = r.app().contributions::<NotificationRegistration>();
            let catalog = NotificationCatalog::new(
                registrations
                    .into_iter()
                    .map(|contributed| contributed.value.0),
            )
            .map_err(|message| format!("notify: {message}"))?;
            Ok(Arc::new(catalog))
        }))
        .exports::<NotificationCatalog>()
        .factory(
            Factory::<NotificationPreferences>::new(|r| async move {
                let options = r.get::<NotifyOptions>().await?;
                Ok(Arc::new(NotificationPreferences::new(
                    r.app().downgrade(),
                    options.preferences.clone(),
                )))
            })
            .deps(options_key()),
        )
        .exports::<NotificationPreferences>()
        .factory(
            Factory::<Notifier>::new(|r| async move {
                let options = r.get::<NotifyOptions>().await?;
                let app = r.app().downgrade();
                let drivers = ChannelDrivers::from_options(&options, &app)
                    .map_err(|message| format!("notify: {message}"))?;
                let parts = Parts {
                    telemetry: Telemetry::from_app(&r.app()).await,
                    drivers,
                    log: r.get::<DeliveryLog>().await?,
                    opt_outs: r.get::<OptOuts>().await?,
                    push: r.get::<PushSubscriptions>().await?,
                    preferences: r.get::<NotificationPreferences>().await?,
                    app,
                };
                Ok(Arc::new(Notifier::new(
                    NotifyOptions::clone(&options),
                    parts,
                )))
            })
            .deps(vec![
                ProviderKey::of::<NotifyOptions>(),
                ProviderKey::of::<DeliveryLog>(),
                ProviderKey::of::<OptOuts>(),
                ProviderKey::of::<PushSubscriptions>(),
                ProviderKey::of::<NotificationPreferences>(),
            ]),
        )
        .exports::<Notifier>()
        .job::<SendNotificationJob>()
        .migrations(migration::migrations())
        .controller::<NotificationsController>()
        .command(commands::vapid())
        .command(commands::release())
        // The built-in digest templates (a file of the same name wins).
        .templates(&DIGEST_TEMPLATES)
        // `notify:release` every minute; it only runs where the app runs
        // ScheduleModule's scheduler, and returns at once with
        // `notify.deferred.schedule = false`.
        .schedule(|s| {
            s.task(commands::RELEASE_COMMAND, release_on_schedule)
                .every_minute()
                .without_overlapping();
        });
}

/// The scheduled `notify:release`: skipped with
/// `notify.deferred.schedule = false`.
async fn release_on_schedule(ctx: AppContext) -> Result<(), BoxError> {
    let notifier = ctx.get::<Notifier>().await?;
    if !notifier.options().deferred.schedule {
        return Ok(());
    }
    let released = notifier.release_due().await?;
    if released != Released::default() {
        tracing::info!(
            target: "navree::notify",
            "{}",
            commands::release_summary(&released)
        );
    }
    Ok(())
}

/// `notify.sms.default_region` (validated with the options).
fn region(options: &NotifyOptions) -> Option<Country> {
    options
        .sms
        .default_region
        .as_deref()
        .and_then(|code| Country::parse(code).ok())
}
