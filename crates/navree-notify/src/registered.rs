//! Drivers a module registers under a name: SMS and WhatsApp drivers that
//! `notify.sms.driver` and `notify.whatsapp.driver` select, and channels a
//! module adds ([`ModuleBuilderNotifyChannelsExt`]).

use std::future::Future;
use std::sync::Arc;

use navree_config::__private::{DriverRecord, build_driver, register_driver, register_in};
use navree_config::{DriverContext, DriverName, DriverSlot, ModuleBuilderConfigExt};
use navree_contracts::BoxFuture;
use navree_kernel::{AppContext, BoxError, Injectable, ModuleBuilder, Resolver};
use serde_json::Value;

use crate::{
    ChannelDriver, ChannelError, NotifyOptions, SentMessage, SmsDriver, SmsDriverKind,
    WhatsAppDriver, WhatsAppDriverKind,
};

/// The channels: the built-in ones, and the method that adds more. It is
/// not contributed (a channel is chosen in code, not in the
/// configuration); the notifier checks the registrations against it.
pub(crate) const CHANNEL_SLOT: DriverSlot = DriverSlot::new(
    "notify.channels",
    &[
        "mail",
        "database",
        "broadcast",
        "slack",
        "teams",
        "sms",
        "whatsapp",
        "web_push",
        "push",
    ],
    "m.notification_channel",
    "notification channel",
);

/// A channel a module adds, as the notifier sends through it.
pub(crate) trait ErasedChannel: Send + Sync {
    /// [`ChannelDriver::send`].
    fn send<'a>(
        &'a self,
        to: &'a str,
        message: &'a Value,
    ) -> BoxFuture<'a, Result<SentMessage, ChannelError>>;
}

impl<C: ChannelDriver> ErasedChannel for C {
    fn send<'a>(
        &'a self,
        to: &'a str,
        message: &'a Value,
    ) -> BoxFuture<'a, Result<SentMessage, ChannelError>> {
        ChannelDriver::send(self, to, message)
    }
}

/// What builds the channel a module added.
type ResolveChannel =
    fn(&AppContext) -> BoxFuture<'static, Result<Arc<dyn ErasedChannel>, BoxError>>;

/// A channel a module added with
/// [`notification_channel`](ModuleBuilderNotifyChannelsExt::notification_channel):
/// its name and how to get it from the app.
pub(crate) struct ChannelRegistration {
    pub(crate) name: DriverName,
    pub(crate) resolve: ResolveChannel,
}

/// The channel of type `C`, from the app's container.
fn resolve_channel<C: ChannelDriver + Injectable>(
    ctx: &AppContext,
) -> BoxFuture<'static, Result<Arc<dyn ErasedChannel>, BoxError>> {
    let ctx = ctx.clone();
    Box::pin(async move {
        let channel = ctx.get::<C>().await?;
        Ok(channel as Arc<dyn ErasedChannel>)
    })
}

/// The channel a module added under `name`, if one did.
pub(crate) fn registered_channel(
    ctx: &AppContext,
    name: DriverName,
) -> Option<Arc<ChannelRegistration>> {
    ctx.contributions::<ChannelRegistration>()
        .into_iter()
        .find(|registration| registration.value.name == name)
        .map(|registration| registration.value)
}

/// The selector `notify.sms.driver`, its built-in drivers (with the alias
/// `aws_sns`) and the method that registers more.
pub(crate) const SMS_SLOT: DriverSlot = DriverSlot::new(
    "notify.sms.driver",
    &["twilio", "vonage", "sns", "aws_sns", "log"],
    "m.sms_driver",
    "SMS driver",
);

/// The selector `notify.whatsapp.driver`, its built-in drivers and the
/// method that registers more.
pub(crate) const WHATSAPP_SLOT: DriverSlot = DriverSlot::new(
    "notify.whatsapp.driver",
    &["twilio", "meta", "log"],
    "m.whatsapp_driver",
    "WhatsApp driver",
);

/// Registers notification channels and SMS and WhatsApp drivers on a
/// [`ModuleBuilder`]: `use navree::plugin::prelude::*;` (or this trait)
/// brings the methods. A channel is shown on [`ChannelDriver`]; an SMS
/// driver here.
///
/// ```no_run
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_kernel::{BoxFuture, Module, ModuleBuilder, Navree};
/// use navree_notify::{
///     ChannelError, ModuleBuilderNotifyChannelsExt, NotifyModule, SentMessage, SmsDriver,
///     SmsMessage,
/// };
/// use serde::Deserialize;
///
/// /// The settings of the driver: `[notify.sandesh]`.
/// #[derive(Default, Deserialize)]
/// #[serde(default)]
/// struct SandeshSettings {
///     sender_id: String,
/// }
///
/// /// Texts through the Sandesh gateway.
/// struct Sandesh {
///     sender_id: String,
/// }
///
/// impl SmsDriver for Sandesh {
///     fn send<'a>(
///         &'a self,
///         to: &'a str,
///         message: &'a SmsMessage,
///     ) -> BoxFuture<'a, Result<SentMessage, ChannelError>> {
///         Box::pin(async move {
///             // Post `message.body` to `to` as `self.sender_id` here.
///             let _ = (to, &message.body, &self.sender_id);
///             Ok(SentMessage::new("sandesh"))
///         })
///     }
/// }
///
/// struct SandeshModule;
///
/// impl Module for SandeshModule {
///     fn register(&self, m: &mut ModuleBuilder) {
///         // Selected with `notify.sms.driver = "sandesh"`.
///         m.sms_driver("sandesh", |ctx| async move {
///             let settings: SandeshSettings = ctx.settings()?;
///             Ok(Sandesh { sender_id: settings.sender_id })
///         });
///     }
/// }
///
/// fn main() -> Result<(), navree_kernel::BoxError> {
///     Navree::app()
///         .module(ConfigModule::for_root(ConfigSources::defaults()))
///         .module(NotifyModule::from_config())
///         .module(SandeshModule)
///         .run()
/// }
/// ```
pub trait ModuleBuilderNotifyChannelsExt {
    /// Adds the delivery channel `C` under the name `C::NAME`: the container
    /// builds `C` (it is registered as this module's provider), and a
    /// notification lists it as `Channel::named(C::NAME)`. Its settings are
    /// `notify.{NAME}`.
    ///
    /// A name that is not a lower-case word, a built-in channel's name
    /// (`mail`, `database`, `broadcast`, `slack`, `teams`, `sms`,
    /// `whatsapp`, `web_push`, `push`), a key of the `notify` section and a
    /// name two modules add fail the boot, naming `m.notification_channel`
    /// and the module. A notification that lists a channel no module adds
    /// fails that delivery, which is recorded as failed.
    fn notification_channel<C: ChannelDriver + Injectable>(&mut self) -> &mut Self;

    /// Registers an SMS driver under `name` (a lower-case letter followed
    /// by up to 63 lower-case letters, digits or `_`):
    /// `notify.sms.driver = "{name}"` selects it, and `factory` builds it at
    /// boot from its [`DriverContext`], whose settings are `notify.{name}`.
    ///
    /// A name that is not such a word, a built-in driver (`twilio`,
    /// `vonage`, `sns`, `aws_sns`, `log`), a key of the `notify` section
    /// (`slack`, `meta`, `push`) and a name two modules register fail the boot,
    /// naming `m.sms_driver` and the module, also when another driver is
    /// selected; `config:check` says the same. A factory that fails fails
    /// the boot. A factory's error is shown in the boot error as written, so
    /// it must not contain a secret from the driver's settings. An app's own
    /// unnamed `dyn SmsDriver` (a fake in a test) still wins when a message
    /// is sent.
    fn sms_driver<S, F, Fut>(&mut self, name: &'static str, factory: F) -> &mut Self
    where
        S: SmsDriver,
        F: Fn(DriverContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<S, BoxError>> + Send + 'static;

    /// Registers a WhatsApp driver under `name`, as
    /// [`sms_driver`](ModuleBuilderNotifyChannelsExt::sms_driver) does for
    /// SMS: `notify.whatsapp.driver = "{name}"` selects it, its settings are
    /// `notify.{name}`, and the built-in drivers are `twilio`, `meta` and
    /// `log`.
    fn whatsapp_driver<W, F, Fut>(&mut self, name: &'static str, factory: F) -> &mut Self
    where
        W: WhatsAppDriver,
        F: Fn(DriverContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<W, BoxError>> + Send + 'static;
}

impl ModuleBuilderNotifyChannelsExt for ModuleBuilder {
    fn notification_channel<C: ChannelDriver + Injectable>(&mut self) -> &mut Self {
        self.provider::<C>();
        match register_in(CHANNEL_SLOT.key(), C::NAME) {
            Ok(name) => self
                .contribute(DriverRecord::new(CHANNEL_SLOT.key(), C::NAME, None))
                .contribute(ChannelRegistration {
                    name,
                    resolve: resolve_channel::<C>,
                })
                .config_claim(&format!("notify.{}", C::NAME)),
            Err(error) => {
                self.contribute(DriverRecord::new(CHANNEL_SLOT.key(), C::NAME, Some(error)))
            }
        }
    }

    fn sms_driver<S, F, Fut>(&mut self, name: &'static str, factory: F) -> &mut Self
    where
        S: SmsDriver,
        F: Fn(DriverContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<S, BoxError>> + Send + 'static,
    {
        register_driver::<dyn SmsDriver, _, _>(self, &SMS_SLOT, Some("notify"), name, move |ctx| {
            let built = factory(ctx);
            async move {
                built
                    .await
                    .map(|driver| Arc::new(driver) as Arc<dyn SmsDriver>)
            }
        })
    }

    fn whatsapp_driver<W, F, Fut>(&mut self, name: &'static str, factory: F) -> &mut Self
    where
        W: WhatsAppDriver,
        F: Fn(DriverContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<W, BoxError>> + Send + 'static,
    {
        register_driver::<dyn WhatsAppDriver, _, _>(
            self,
            &WHATSAPP_SLOT,
            Some("notify"),
            name,
            move |ctx| {
                let built = factory(ctx);
                async move {
                    built
                        .await
                        .map(|driver| Arc::new(driver) as Arc<dyn WhatsAppDriver>)
                }
            },
        )
    }
}

/// The SMS and WhatsApp drivers a module registers that the options
/// select; `None` for a built-in or no driver.
pub(crate) struct RegisteredDrivers {
    pub(crate) sms: Option<Arc<dyn SmsDriver>>,
    pub(crate) whatsapp: Option<Arc<dyn WhatsAppDriver>>,
}

/// The [`RegisteredDrivers`] `options` select, built with their settings
/// (`notify.{name}`). A name no module registers fails, naming the method.
pub(crate) async fn registered_drivers(
    r: &Resolver,
    options: &NotifyOptions,
) -> Result<RegisteredDrivers, BoxError> {
    let sms = match options.sms.driver.map(SmsDriverKind::resolved) {
        Some(SmsDriverKind::Registered(name)) => Some(
            build_driver::<dyn SmsDriver>(
                r,
                &SMS_SLOT,
                "notify.sms.driver",
                name,
                format!("notify.{name}"),
                None,
                None,
            )
            .await?,
        ),
        _ => None,
    };
    let whatsapp = match options.whatsapp.driver.map(WhatsAppDriverKind::resolved) {
        Some(WhatsAppDriverKind::Registered(name)) => Some(
            build_driver::<dyn WhatsAppDriver>(
                r,
                &WHATSAPP_SLOT,
                "notify.whatsapp.driver",
                name,
                format!("notify.{name}"),
                None,
                None,
            )
            .await?,
        ),
        _ => None,
    };
    Ok(RegisteredDrivers { sms, whatsapp })
}
