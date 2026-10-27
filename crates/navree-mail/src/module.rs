//! [`MailModule`].

use std::sync::Arc;

use navree_config::{Config, ConfigSection};
use navree_contracts::{Clock, ModuleBuilderJobsExt, SystemClock};
use navree_db::{Db, ModuleBuilderDbExt};
use navree_http::{ModuleBuilderHttpExt, UrlSigner};
use navree_kernel::{BoxError, Factory, Module, ModuleBuilder, ModuleId, ProviderKey, Resolver};
use navree_telemetry::Telemetry;
use navree_view::Views;

use crate::commands::commands;
use crate::delivery::controller::{MailController, map_mail_error};
use crate::delivery::migration::migrations;
use crate::delivery::tracking::Tracking;
use crate::delivery::{Pipeline, Store};
use crate::{
    Deliveries, DnsResolver, MailError, MailOptions, MailTransport, Mailer, Outbox, SendMailJob,
    SendingDomains, Suppressions, SystemResolver, TransportResolver,
};

/// The global mail module: provides and exports [`Mailer`] and
/// `Arc<dyn MailTransport>`, and registers job `mail.send`
/// ([`SendMailJob`]) with the queue.
///
/// - [`MailModule::for_root`] takes [`MailOptions`] in code;
/// - [`MailModule::from_config`] (and a plain `.module(MailModule)` or an
///   import with no configured instance in the app) reads section `mail` at
///   boot, so the app must include `ConfigModule`.
///
/// The app must include `ViewModule` (mailables render with its `Views`).
/// Invalid options (sender, SMTP URL, empty failover) fail the boot. The
/// transport is built at boot but connects on the first send.
///
/// ```no_run
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_kernel::Navree;
/// use navree_mail::MailModule;
/// use navree_view::ViewModule;
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// // .env: APP_MAIL__FROM="Acme <hello@acme.test>"
/// //       APP_MAIL__TRANSPORT=smtps://user:pass@smtp.acme.test
/// Navree::app()
///     .module(ConfigModule::for_root(ConfigSources::defaults()))
///     .module(ViewModule::from_config())
///     .module(MailModule::from_config())
///     .run()
/// # }
/// ```
///
/// In tests, use `TransportConfig::Array` and read the messages with
/// [`Mailer::array_transport`], or swap the transport:
/// `.override_value::<dyn MailTransport>(Arc::new(array.clone()))`.
#[derive(Clone, Copy, Debug, Default)]
pub struct MailModule;

impl MailModule {
    /// The module configured with `options` (configure it once, in the app root).
    pub fn for_root(options: MailOptions) -> impl Module {
        ConfiguredMailModule {
            source: Source::Options(Arc::new(options)),
        }
    }

    /// The module configured from section `mail` of the app's `Config` at
    /// boot (see [`MailOptions`]).
    pub fn from_config() -> impl Module {
        ConfiguredMailModule {
            source: Source::Config,
        }
    }
}

impl Module for MailModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, Source::Config);
    }
}

/// `MailModule::for_root(...)` / `from_config()`: same module id, configured.
struct ConfiguredMailModule {
    source: Source,
}

impl Module for ConfiguredMailModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<MailModule>()
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
    Options(Arc<MailOptions>),
    Config,
}

impl Source {
    /// The validated options.
    async fn options(&self, r: &Resolver) -> Result<MailOptions, BoxError> {
        match self {
            Source::Options(options) => {
                options
                    .validate()
                    .map_err(|message| format!("mail options are invalid: {message}"))?;
                Ok(MailOptions::clone(options))
            }
            Source::Config => Ok(r.get::<Config>().await?.typed::<MailOptions>()?),
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

/// Global; provides the options (private), the transport, the mailer, the
/// delivery services and the domain checks (exported), and registers the
/// send job, the `mail` migrations, the routes, the commands and the error
/// mapping.
fn register(m: &mut ModuleBuilder, source: Source) {
    let deps = source.deps();
    m.global()
        .factory(
            Factory::<MailOptions>::new(move |r| {
                let source = source.clone();
                async move { Ok(Arc::new(source.options(&r).await?)) }
            })
            .deps(deps),
        )
        .factory(
            Factory::<dyn MailTransport>::new(|r| async move {
                let options = r.get::<MailOptions>().await?;
                Ok(options.transport.build()?)
            })
            .deps(vec![ProviderKey::of::<MailOptions>()]),
        )
        .factory(
            Factory::<Store>::new(|r| async move {
                let options = r.get::<MailOptions>().await?;
                Ok(Arc::new(Store::new(&r.app(), options)))
            })
            .deps(vec![ProviderKey::of::<MailOptions>()]),
        )
        .factory(
            Factory::<Suppressions>::new(|r| async move {
                Ok(Arc::new(Suppressions::new(r.get::<Store>().await?)))
            })
            .deps(vec![ProviderKey::of::<Store>()]),
        )
        .factory(
            Factory::<Deliveries>::new(|r| async move {
                Ok(Arc::new(Deliveries::new(
                    r.get::<Store>().await?,
                    r.get::<Suppressions>().await?,
                )))
            })
            .deps(vec![
                ProviderKey::of::<Store>(),
                ProviderKey::of::<Suppressions>(),
            ]),
        )
        .factory(
            Factory::<Outbox>::new(|r| async move {
                Ok(Arc::new(Outbox::new(r.get::<Store>().await?)))
            })
            .deps(vec![ProviderKey::of::<Store>()]),
        )
        .factory(
            Factory::<TransportResolver>::new(|r| async move {
                Ok(Arc::new(TransportResolver::new(r.get::<Store>().await?)))
            })
            .deps(vec![ProviderKey::of::<Store>()]),
        )
        .factory(
            Factory::<Mailer>::new(|r| async move {
                let options = r.get::<MailOptions>().await?;
                let transport = r.get::<dyn MailTransport>().await?;
                let views = r.get::<Views>().await?;
                let telemetry = Telemetry::from_app(&r.app()).await;
                let mut mailer = Mailer::new(transport, views, options.from.clone())
                    .queued_through(r.app().downgrade(), options.queue.clone())
                    .with_render(options.render.clone())
                    .with_telemetry(telemetry);
                for (name, config) in &options.transports {
                    mailer = mailer.with_transport(name.clone(), config.build()?);
                }
                if options.delivery.enabled {
                    mailer = mailer.with_pipeline(Arc::new(pipeline(&r, &options).await?));
                }
                Ok(Arc::new(mailer))
            })
            .deps(vec![
                ProviderKey::of::<MailOptions>(),
                ProviderKey::of::<dyn MailTransport>(),
                ProviderKey::of::<Views>(),
                ProviderKey::of::<Store>(),
                ProviderKey::of::<Suppressions>(),
                ProviderKey::of::<Outbox>(),
                ProviderKey::of::<TransportResolver>(),
            ]),
        )
        .exports::<Mailer>()
        .exports::<dyn MailTransport>()
        .exports::<Deliveries>()
        .exports::<Suppressions>()
        .exports::<Outbox>()
        .exports::<TransportResolver>()
        .factory(Factory::<dyn DnsResolver>::new(|_| async move {
            Ok(Arc::new(SystemResolver::new()) as Arc<dyn DnsResolver>)
        }))
        .factory(
            Factory::<SendingDomains>::new(|r| async move {
                Ok(Arc::new(SendingDomains::new(r.get::<dyn DnsResolver>().await?)))
            })
            .deps(vec![ProviderKey::of::<dyn DnsResolver>()]),
        )
        .exports::<dyn DnsResolver>()
        .exports::<SendingDomains>()
        .job::<SendMailJob>()
        .controller::<MailController>()
        .migrations(migrations())
        .map_error::<MailError>(map_mail_error);
    for command in commands() {
        m.command(command);
    }
}

/// The delivery pipeline (`mail.delivery.enabled`): needs `DatabaseModule`;
/// tracking also needs the app's `UrlSigner` (`HttpModule`) and a base URL.
async fn pipeline(r: &Resolver, options: &MailOptions) -> Result<Pipeline, BoxError> {
    let app = r.app();
    if app.try_get::<Db>().await.is_none() {
        return Err("`mail.delivery.enabled` needs DatabaseModule (the mail tables)".into());
    }
    let tracking = if options.tracking.is_on() {
        let signer = app.try_get::<UrlSigner>().await.ok_or(
            "mail tracking needs HttpModule: its UrlSigner signs the tracked links",
        )?;
        let base = match options
            .tracking
            .base_url
            .clone()
            .filter(|base| !base.is_empty())
        {
            Some(base) => base,
            None => app
                .try_get::<Config>()
                .await
                .and_then(|config| config.get_string("app.url"))
                .filter(|url| !url.is_empty())
                .ok_or("mail tracking needs `mail.tracking.base_url` or `app.url`")?,
        };
        let clock: Arc<dyn Clock> = match app.try_get::<dyn Clock>().await {
            Some(clock) => clock,
            None => Arc::new(SystemClock),
        };
        Some(Tracking::new(
            signer,
            clock,
            &base,
            options.tracking.ttl_days,
            options.tracking.opens,
            options.tracking.clicks,
        ))
    } else {
        None
    };
    Ok(Pipeline::new(
        r.get::<Store>().await?,
        r.get::<Suppressions>().await?,
        r.get::<Outbox>().await?,
        r.get::<TransportResolver>().await?,
        tracking,
    ))
}
