//! [`NotifyModule`].

use std::marker::PhantomData;
use std::sync::Arc;

use navree_config::{Config, ConfigSection, ModuleBuilderConfigExt};
use navree_contracts::{ModuleBuilderJobsExt, ModuleBuilderRevocationExt};
use navree_db::ModuleBuilderDbExt;
use navree_http::ModuleBuilderHttpExt;
use navree_kernel::{
    AppContext, BoxError, ConfiguredModule, Factory, Module, ModuleBuilder, ModuleId, ProviderKey,
    Resolver,
};
use navree_schedule::ModuleBuilderScheduleExt;
use navree_telemetry::Telemetry;
use navree_validation::Country;
use navree_view::ModuleBuilderViewExt;

use crate::catalog::NotificationRegistration;
use crate::channels::ChannelDrivers;
use crate::controller::NotificationsController;
use crate::devices::{DeviceRevoker, RegisteredApps};
use crate::digest::DIGEST_TEMPLATES;
use crate::notifier::Parts;
use crate::{
    Channel, DeliveryLog, ModuleBuilderNotifyExt, Notification, NotificationCatalog,
    NotificationPreferences, Notifier, NotifyOptions, OptOuts, PushDevices, PushOptions,
    PushSubscriptions, Released, SendNotificationJob, commands, migration,
};

/// The global notification module: provides and exports [`Notifier`],
/// [`DeliveryLog`], [`OptOuts`], [`PushSubscriptions`], [`PushDevices`],
/// [`NotificationPreferences`] and [`NotificationCatalog`], registers job
/// `notify.send` ([`SendNotificationJob`]), commands `notify:vapid` and
/// `notify:release` (scheduled every minute), the built-in digest templates
/// (`notify/digest`), the routes, a `dyn CredentialRevoker` that deletes
/// the push devices of offboarded users, and the `notify` migrations
/// (tables `notifications`, `notification_deliveries`,
/// `notification_opt_outs`, `push_subscriptions`, `push_devices`,
/// `notification_preferences`, `notification_deferrals`).
///
/// The push channel's services are read from section `notify.push`
/// ([`PushOptions`]) of the app's `Config` at boot, whichever way the module
/// is configured.
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
        ConfiguredModule::<NotifyModule, _>::for_root(options, register)
    }

    /// The module configured from section `notify` of the app's `Config`
    /// at boot (see [`NotifyOptions`]).
    pub fn from_config() -> impl Module {
        ConfiguredModule::<NotifyModule, NotifyOptions>::from_config(register)
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
        register(m, None);
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
/// job, the migrations, the routes and `notify:vapid`. `Some` options come
/// from code; `None` reads section `notify` of the configuration at boot.
fn register(m: &mut ModuleBuilder, options: Option<NotifyOptions>) {
    let source = match options {
        Some(options) => Source::Options(Arc::new(options)),
        None => Source::Config,
    };
    // The section is read in `Source::options`, not by `m.config`, so it is
    // registered here for `config:check` and the reference.
    if matches!(source, Source::Config) {
        m.config_section::<NotifyOptions>();
    }
    m.config_section::<PushOptions>();
    m.credential_revoker::<DeviceRevoker>();
    let mut deps = source.deps();
    deps.push(ProviderKey::of::<PushOptions>());
    let options_key = || vec![ProviderKey::of::<NotifyOptions>()];
    m.global()
        .factory(
            Factory::<NotifyOptions>::new(move |r| {
                let source = source.clone();
                async move {
                    let mut options = source.options(&r).await?;
                    // An app that can't push to native apps lists no switch
                    // and no hold for that channel; `push` still parses.
                    let push = r.get::<PushOptions>().await?;
                    if !can_push(&r, &push).await {
                        options
                            .preferences
                            .channels
                            .retain(|channel| *channel != Channel::Push);
                        options
                            .deferred
                            .channels
                            .retain(|channel| *channel != Channel::Push);
                    }
                    Ok(Arc::new(options))
                }
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
        .factory(Factory::<PushOptions>::new(|r| async move {
            let options = match r.app().try_get::<Config>().await {
                Some(config) => config.typed::<PushOptions>()?,
                None => PushOptions::default(),
            };
            Ok(Arc::new(options))
        }))
        .factory(
            Factory::<PushDevices>::new(|r| async move {
                let push = r.get::<PushOptions>().await?;
                let apps = RegisteredApps {
                    ios: push.apns.bundle_ids.clone(),
                    android: push.fcm.packages.clone(),
                };
                let served = can_push(&r, &push).await;
                Ok(Arc::new(PushDevices::new(r.app().downgrade(), apps, served)))
            })
            .deps(vec![ProviderKey::of::<PushOptions>()]),
        )
        .exports::<PushDevices>()
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
                let push = r.get::<PushOptions>().await?;
                let app = r.app().downgrade();
                let drivers = ChannelDrivers::from_options(&options, &push, &app)
                    .map_err(|message| format!("notify: {message}"))?;
                let parts = Parts {
                    telemetry: Telemetry::from_app(&r.app()).await,
                    drivers,
                    log: r.get::<DeliveryLog>().await?,
                    opt_outs: r.get::<OptOuts>().await?,
                    push: r.get::<PushSubscriptions>().await?,
                    devices: r.get::<PushDevices>().await?,
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
                ProviderKey::of::<PushOptions>(),
                ProviderKey::of::<PushDevices>(),
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
    #[cfg(feature = "ui")]
    m.controller::<crate::ui::NotificationBellController>();
    // A native app learns where its device's push token goes from the shell document, once
    // the routes are on.
    #[cfg(feature = "ui")]
    m.multi(
        Factory::<dyn navree_ui::NativeShellPart>::new(|r: Resolver| async move {
            let notifier = r.get::<Notifier>().await?;
            let devices = r.get::<PushDevices>().await?;
            let api_prefix = match r.app().try_get::<navree_http::HttpOptions>().await {
                Some(http) => http.api_prefix.clone(),
                None => String::new(),
            };
            Ok(Arc::new(NativeRoutes {
                on: notifier.options().routes && devices.served(),
                part: crate::ui::PushShellPart::new(&api_prefix),
            }) as Arc<dyn navree_ui::NativeShellPart>)
        })
        .deps(vec![
            ProviderKey::of::<Notifier>(),
            ProviderKey::of::<PushDevices>(),
        ]),
    );
}

/// Whether the app can push to native apps: Apple's or Firebase's service
/// is configured in `notify.push`, or a `dyn PushDriver` is registered.
async fn can_push(r: &Resolver, push: &PushOptions) -> bool {
    push.apns.is_configured()
        || push.fcm.is_configured()
        || r.app().try_get::<dyn crate::PushDriver>().await.is_some()
}

/// The push route in the shell document, only while the notification routes are on and the
/// app can push.
#[cfg(feature = "ui")]
struct NativeRoutes {
    on: bool,
    part: crate::ui::PushShellPart,
}

#[cfg(feature = "ui")]
impl navree_ui::NativeShellPart for NativeRoutes {
    fn contribute(
        &self,
        shell: &mut navree_ui::NativeShellParts,
        client: &navree_ui::NativeClient,
    ) {
        if self.on {
            self.part.contribute(shell, client);
        }
    }
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
