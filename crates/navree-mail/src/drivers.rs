//! Mail transports a module registers under a name:
//! [`ModuleBuilderMailTransportExt`], and how [`MailModule`](crate::MailModule)
//! builds a [`TransportConfig`] that names one.

use std::future::Future;
use std::sync::Arc;

use navree_config::__private::{build_driver, register_driver};
use navree_config::{DriverContext, DriverSlot};
use navree_kernel::{AppContext, BoxError, BoxFuture, ModuleBuilder};

use crate::{FailoverTransport, MailError, MailTransport, TransportConfig};

/// The selector `mail.transport`, its built-in types (the short forms and
/// the table types) and the method that registers more.
pub(crate) const SLOT: DriverSlot = DriverSlot::new(
    "mail.transport",
    &[
        "log", "array", "smtp", "smtps", "ses", "mailgun", "postmark", "resend", "failover",
    ],
    "m.mail_transport",
    "mail transport",
);

/// The registrations of mail transports, checked once at boot whichever
/// transport is selected (and also when a test swaps the transport).
pub(crate) struct TransportRegistrations;

/// Registers mail transports on a [`ModuleBuilder`]: `use
/// navree::plugin::prelude::*;` (or this trait) brings the method.
///
/// ```no_run
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_kernel::{BoxFuture, Module, ModuleBuilder, Navree};
/// use navree_mail::{MailError, MailModule, MailTransport, Message, ModuleBuilderMailTransportExt};
/// use navree_view::ViewModule;
/// use serde::Deserialize;
///
/// /// The settings of the transport: `[mail.dakghar]`.
/// #[derive(Default, Deserialize)]
/// #[serde(default)]
/// struct DakgharSettings {
///     sender_id: String,
/// }
///
/// /// Hands each message to the Dakghar gateway.
/// struct Dakghar {
///     sender_id: String,
/// }
///
/// impl MailTransport for Dakghar {
///     fn send<'a>(&'a self, message: &'a Message) -> BoxFuture<'a, Result<(), MailError>> {
///         Box::pin(async move {
///             // Post `message` to the gateway as `self.sender_id` here.
///             let _ = (&self.sender_id, &message.subject);
///             Ok(())
///         })
///     }
///
///     fn name(&self) -> &'static str {
///         "dakghar"
///     }
/// }
///
/// struct DakgharModule;
///
/// impl Module for DakgharModule {
///     fn register(&self, m: &mut ModuleBuilder) {
///         // Selected with `mail.transport = "dakghar"`.
///         m.mail_transport("dakghar", |ctx| async move {
///             let settings: DakgharSettings = ctx.settings()?;
///             Ok(Dakghar { sender_id: settings.sender_id })
///         });
///     }
/// }
///
/// fn main() -> Result<(), navree_kernel::BoxError> {
///     Navree::app()
///         .module(ConfigModule::for_root(ConfigSources::defaults()))
///         .module(ViewModule::from_config())
///         .module(MailModule::from_config())
///         .module(DakgharModule)
///         .run()
/// }
/// ```
pub trait ModuleBuilderMailTransportExt {
    /// Registers a mail transport under `name` (a lower-case letter
    /// followed by up to 63 lower-case letters, digits or `_`):
    /// `mail.transport = "{name}"`, a failover's list and
    /// `mail.transports.{lane}` select it by that name, and `factory` builds
    /// it at boot from its [`DriverContext`], whose settings are
    /// `mail.{name}`.
    ///
    /// A name that is not such a word, a built-in type (`log`, `array`,
    /// `smtp`, `smtps`, `ses`, `mailgun`, `postmark`, `resend`,
    /// `failover`), a key of the `mail` section (`from`) and a name two
    /// modules register fail the boot, naming `m.mail_transport` and the
    /// module, also when another transport is selected; `config:check`
    /// says the same. A factory that fails fails the boot, naming the
    /// transport and the module. A factory's error is shown in the boot error
    /// as written, so it must not contain a secret from the driver's
    /// settings. A tenant's stored transport cannot name a registered
    /// transport.
    fn mail_transport<T, F, Fut>(&mut self, name: &'static str, factory: F) -> &mut Self
    where
        T: MailTransport,
        F: Fn(DriverContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<T, BoxError>> + Send + 'static;
}

impl ModuleBuilderMailTransportExt for ModuleBuilder {
    fn mail_transport<T, F, Fut>(&mut self, name: &'static str, factory: F) -> &mut Self
    where
        T: MailTransport,
        F: Fn(DriverContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<T, BoxError>> + Send + 'static,
    {
        register_driver::<dyn MailTransport, _, _>(self, &SLOT, Some("mail"), name, move |ctx| {
            let built = factory(ctx);
            async move {
                built
                    .await
                    .map(|transport| Arc::new(transport) as Arc<dyn MailTransport>)
            }
        })
    }
}

/// Builds the transport `config` describes, at `key` (`mail.transport`,
/// `mail.transports.otp`): a transport a module registers through its
/// registration, a failover member by member, a built-in one with
/// [`TransportConfig::build`].
pub(crate) fn build_transport<'a>(
    config: &'a TransportConfig,
    app: &'a AppContext,
    key: &'a str,
) -> BoxFuture<'a, Result<Arc<dyn MailTransport>, BoxError>> {
    Box::pin(async move {
        match config {
            // A built-in name given in code is that built-in transport.
            TransportConfig::Registered(name) if name.as_str() == "log" => {
                Ok(TransportConfig::Log.build()?)
            }
            TransportConfig::Registered(name) if name.as_str() == "array" => {
                Ok(TransportConfig::Array.build()?)
            }
            TransportConfig::Registered(name) => {
                build_driver::<dyn MailTransport>(
                    app,
                    &SLOT,
                    key,
                    *name,
                    format!("mail.{name}"),
                    None,
                )
                .await
            }
            TransportConfig::Failover(transports) => {
                if transports.is_empty() {
                    return Err(MailError::Config(
                        "a failover transport needs at least one transport".to_owned(),
                    )
                    .into());
                }
                let mut built = Vec::with_capacity(transports.len());
                for transport in transports {
                    built.push(build_transport(transport, app, key).await?);
                }
                Ok(Arc::new(FailoverTransport::new(built)) as Arc<dyn MailTransport>)
            }
            other => Ok(other.build()?),
        }
    })
}
