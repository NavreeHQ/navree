//! [`WebhooksModule`] and [`ModuleBuilderWebhooksExt`].

use std::borrow::Cow;
use std::marker::PhantomData;
use std::sync::Arc;

use navree_config::ModuleBuilderConfigExt;
use navree_contracts::ModuleBuilderJobsExt;
use navree_db::ModuleBuilderDbExt;
use navree_events::{Listener, ModuleBuilderEventsExt};
use navree_http::ModuleBuilderHttpExt;
use navree_kernel::{
    AppContext, BoxError, BoxFuture, Factory, Injectable, KernelError, Module, ModuleBuilder,
    ModuleId, ProviderKey, Resolver,
};
use navree_workspace::events::WorkspaceDeleted;

use crate::commands::commands;
use crate::controller::WebhooksController;
use crate::error::map_webhooks_error;
use crate::event::{FanOutListener, WebhookTypeRegistration};
use crate::jobs::{Deliver, FanOut};
use crate::migration::migrations;
use crate::{WebhookEvent, Webhooks, WebhooksError, WebhooksOptions};

/// Webhooks: signed outgoing deliveries and verified incoming requests.
///
/// Global; provides and exports [`Webhooks`] and [`WebhooksOptions`], and
/// contributes:
///
/// - the `webhooks` migrations (`webhook_endpoints`, `webhook_deliveries`,
///   `webhook_attempts`, `webhook_receipts`);
/// - the jobs `webhooks.fan_out` ([`FanOut`]) and `webhooks.deliver`
///   ([`Deliver`]);
/// - the routes under `{http.api_prefix}/webhooks` when `webhooks.routes`
///   is true and the app has `DatabaseModule`
///   ([`WebhooksController`](crate::WebhooksController); permission
///   `webhooks.manage`);
/// - the commands `webhooks:endpoints`, `webhooks:test`, `webhooks:retry`
///   and `webhooks:prune`;
/// - a listener that deletes a workspace's endpoints when the workspace is
///   deleted;
/// - the mapping of [`WebhooksError`]s returned from handlers to error
///   responses.
///
/// Outgoing delivery needs `DatabaseModule`, `QueueModule`,
/// `EventsModule` (for events that become webhooks) and
/// `HttpClientModule`; with routes on, also `AccessModule` and an
/// authenticator. Verifying incoming webhooks with
/// [`VerifiedWebhook`](crate::VerifiedWebhook) needs only this module.
///
/// ```no_run
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_db::DatabaseModule;
/// use navree_events::EventsModule;
/// use navree_http::HttpModule;
/// use navree_http_client::HttpClientModule;
/// use navree_kernel::Navree;
/// use navree_queue::QueueModule;
/// use navree_webhooks::WebhooksModule;
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// Navree::app()
///     .module(ConfigModule::for_root(ConfigSources::defaults()))
///     .module(HttpModule::from_config())
///     .module(DatabaseModule::from_config())
///     .module(EventsModule::for_root())
///     .module(QueueModule::from_config())
///     .module(HttpClientModule::from_config())
///     .module(WebhooksModule::from_config()) // section `webhooks`
///     .run()
/// # }
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct WebhooksModule;

impl WebhooksModule {
    /// The module with explicit options (configure it once, in the app
    /// root). Invalid options fail the boot.
    pub fn for_root(options: WebhooksOptions) -> impl Module {
        ConfiguredWebhooksModule {
            options: Some(options),
        }
    }

    /// The module configured from the `webhooks` section of `Config` at
    /// boot.
    pub fn from_config() -> impl Module {
        ConfiguredWebhooksModule { options: None }
    }

    /// Makes event `E` a webhook: import it in the module that owns the
    /// event (`imports = [WebhooksModule::for_feature::<DealWon>()]`). From
    /// then on every emitted `E` is queued for the endpoints subscribed to
    /// [`E::NAME`](navree_events::Event::NAME) in its workspace, and the
    /// type appears in [`Webhooks::event_types`].
    pub fn for_feature<E: WebhookEvent>() -> impl Module {
        WebhookFeature::<E>(PhantomData)
    }
}

impl Module for WebhooksModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

/// `WebhooksModule::for_root` / `from_config`: same module id, configured.
struct ConfiguredWebhooksModule {
    /// `None`: read from configuration.
    options: Option<WebhooksOptions>,
}

impl Module for ConfiguredWebhooksModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<WebhooksModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, self.options.clone());
    }
}

fn register(m: &mut ModuleBuilder, options: Option<WebhooksOptions>) {
    m.global();
    match options {
        Some(options) => m.factory(Factory::<WebhooksOptions>::new(move |_| {
            let options = options.clone();
            async move {
                navree_config::ConfigSection::validate(&options)
                    .map_err(|message| format!("invalid webhooks options: {message}"))?;
                Ok(Arc::new(options))
            }
        })),
        None => m.config::<WebhooksOptions>(),
    };
    m.exports::<WebhooksOptions>()
        .factory(
            Factory::<Webhooks>::new(|r: Resolver| async move {
                let options = r.get::<WebhooksOptions>().await?;
                Ok(Arc::new(Webhooks::new(&r.app(), options)))
            })
            .deps(vec![ProviderKey::of::<WebhooksOptions>()]),
        )
        .exports::<Webhooks>()
        .job::<FanOut>()
        .job::<Deliver>()
        .controller::<WebhooksController>()
        .migrations(migrations())
        .map_error::<WebhooksError>(map_webhooks_error)
        .listen::<WorkspaceDeleted, DropWorkspaceEndpoints>();
    for command in commands() {
        m.command(command);
    }
}

/// `WebhooksModule::for_feature::<E>()`.
struct WebhookFeature<E>(PhantomData<fn() -> E>);

impl<E: WebhookEvent> Module for WebhookFeature<E> {
    fn register(&self, m: &mut ModuleBuilder) {
        m.webhook_event::<E>();
    }
}

/// Webhook registrations on a [`ModuleBuilder`].
pub trait ModuleBuilderWebhooksExt {
    /// Makes event `E` a webhook from this module (what
    /// [`WebhooksModule::for_feature`] does, for a module's `register`).
    fn webhook_event<E: WebhookEvent>(&mut self) -> &mut Self;

    /// Registers an event type that is dispatched by name with
    /// [`Webhooks::dispatch_json`] rather than emitted as an event:
    /// `m.webhook_type("export.ready", "A CSV export finished.")`.
    fn webhook_type(
        &mut self,
        name: impl Into<Cow<'static, str>>,
        description: impl Into<Cow<'static, str>>,
    ) -> &mut Self;
}

impl ModuleBuilderWebhooksExt for ModuleBuilder {
    fn webhook_event<E: WebhookEvent>(&mut self) -> &mut Self {
        self.listen::<E, FanOutListener<E>>()
            .contribute(WebhookTypeRegistration::event::<E>())
    }

    fn webhook_type(
        &mut self,
        name: impl Into<Cow<'static, str>>,
        description: impl Into<Cow<'static, str>>,
    ) -> &mut Self {
        self.contribute(WebhookTypeRegistration::named(
            name.into(),
            description.into(),
        ))
    }
}

/// Deletes a deleted workspace's endpoints (with their deliveries).
struct DropWorkspaceEndpoints;

impl Injectable for DropWorkspaceEndpoints {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(DropWorkspaceEndpoints) })
    }
}

impl Listener<WorkspaceDeleted> for DropWorkspaceEndpoints {
    fn handle<'a>(
        &'a self,
        event: &'a WorkspaceDeleted,
        ctx: &'a AppContext,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            let webhooks = ctx.get::<Webhooks>().await?;
            match webhooks.drop_workspace(event.workspace_id).await {
                Ok(_) | Err(WebhooksError::Missing(_)) => Ok(()),
                Err(error) => Err(error.into()),
            }
        })
    }
}
