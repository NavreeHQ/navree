//! [`BillingModule`].

use std::future::Future;
use std::marker::PhantomData;
use std::sync::Arc;

use navree_config::{Config, ModuleBuilderConfigExt};
use navree_contracts::{Clock, ModuleBuilderJobsExt, SystemClock};
use navree_db::{Db, ModuleBuilderDbExt};
use navree_events::ModuleBuilderEventsExt;
use navree_http::ModuleBuilderHttpExt;
use navree_http_client::{HttpClient, HttpClientOptions};
use navree_kernel::{
    AppContext, BoxError, ConfiguredModule, Factory, Module, ModuleBuilder, ProviderKey, Resolver,
};
use navree_schedule::ModuleBuilderScheduleExt;
use navree_workspace::events::{MemberAdded, MemberRemoved};

use crate::billable::{Billable, BillableRegistration};
use crate::billing::Billing;
use crate::commands::commands;
use crate::config::{BillingOptions, ProviderKind};
use crate::controller::BillingController;
use crate::error::{BillingError, map_billing_error};
use crate::migration::migrations;
use crate::plan::Plans;
use crate::provider::BillingProvider;
use crate::provider::fake::BillingFake;
use crate::provider::paddle::PaddleProvider;
use crate::provider::registry::{self, ProviderContext, ProviderRegistration};
use crate::provider::stripe::StripeProvider;
use crate::seats::SeatSync;
use crate::webhook::ProcessBillingEvent;

/// Billing: subscriptions, trials, seats, metered usage, invoices, the
/// customer portal, one-time payments, payment links and refunds, with
/// Stripe, Paddle, the fake or a provider registered with
/// [`BillingModule::provider`].
///
/// Global; provides and exports [`Billing`] and [`BillingOptions`], and
/// contributes:
///
/// - the `billing` migrations (`billing_customers`, `subscriptions`,
///   `subscription_items`, `billing_usage`, `billing_events`; after `auth`
///   and `workspace`);
/// - the routes under `{http.api_prefix}/billing` ([`BillingController`]);
/// - the job `billing.process_event` ([`ProcessBillingEvent`]);
/// - the queued listener [`SeatSync`] on the workspace events `MemberAdded`
///   and `MemberRemoved`;
/// - the scheduled command `billing:report-usage`, every minute, when
///   `ScheduleModule` is in the app;
/// - the commands `billing:plans`, `billing:webhook`,
///   `billing:report-usage`, `billing:sync`, `billing:events` and
///   `billing:prune-events`;
/// - the mapping of [`BillingError`]s returned from handlers to error
///   responses.
///
/// The app must include `ConfigModule` and `DatabaseModule`. The routes
/// declare access, so with `billing.routes` on the app also needs an
/// authenticator (navree-auth's `AuthModule`) and, for workspace billing,
/// navree-access's route authorizer and `WorkspaceModule`. `QueueModule`
/// runs the webhook jobs (inline without it), `EventsModule` receives the
/// events and `HttpClientModule` provides the HTTP client (a default one is
/// built without it).
#[derive(Clone, Copy, Debug, Default)]
pub struct BillingModule;

impl BillingModule {
    /// The module with explicit options (configure it once, in the app
    /// root). Invalid options fail the boot.
    pub fn for_root(options: BillingOptions) -> impl Module {
        ConfiguredModule::<BillingModule, _>::for_root(options, register)
    }

    /// The module configured from the `billing` section of `Config` at
    /// boot.
    pub fn from_config() -> impl Module {
        ConfiguredModule::<BillingModule, BillingOptions>::from_config(register)
    }

    /// Registers `B` as a billable type: the routes and the `Subscribed`
    /// extractor act on it (see `billing.routes_billable`). Import it in
    /// the module that owns billing:
    /// `#[module(imports = [BillingModule::for_feature::<Workspace>()])]`.
    pub fn for_feature<B: Billable + 'static>() -> impl Module {
        BillableFeature::<B>(PhantomData)
    }

    /// Registers a payment provider under `name`: `billing.provider =
    /// "<name>"` selects it, and `factory` builds it at boot from a
    /// [`ProviderContext`] (the options, the app's HTTP client, the clock,
    /// the provider's own configuration section). A crate that ships a
    /// provider exports a module that imports this one.
    ///
    /// ```
    /// use std::sync::Arc;
    ///
    /// use navree_billing::{BillingModule, BillingProvider, ProviderContext, ProviderError,
    ///     WebhookError, WebhookEvent};
    /// use navree_kernel::BoxError;
    ///
    /// struct AcmePay;
    ///
    /// impl BillingProvider for AcmePay {
    ///     fn name(&self) -> &'static str {
    ///         "acme_pay"
    ///     }
    ///
    ///     fn verify_webhook(
    ///         &self,
    ///         _headers: &http::HeaderMap,
    ///         _body: &[u8],
    ///         _now: chrono::DateTime<chrono::Utc>,
    ///         _tolerance: std::time::Duration,
    ///     ) -> Result<(), WebhookError> {
    ///         Err(WebhookError::NoSecret)
    ///     }
    ///
    ///     fn parse_webhook(&self, _body: &[u8]) -> Result<WebhookEvent, ProviderError> {
    ///         Err(ProviderError::Unsupported { provider: "acme_pay", operation: "webhooks" })
    ///     }
    /// }
    ///
    /// let module = BillingModule::provider("acme_pay", |_context: ProviderContext| async {
    ///     Ok::<_, BoxError>(AcmePay)
    /// });
    /// # let _ = module;
    /// ```
    ///
    /// The built-in names (`stripe`, `paddle`, `fake`) cannot be taken, a
    /// name is registered once, and the provider's
    /// [`name`](BillingProvider::name) must be the registered name: each
    /// mistake fails the boot. Each provider type registers once (a second
    /// registration of the same type is the same module).
    pub fn provider<P, F, Fut>(name: &str, factory: F) -> impl Module
    where
        P: BillingProvider,
        F: Fn(ProviderContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<P, BoxError>> + Send + 'static,
    {
        let registration = ProviderRegistration::new(name, move |context| {
            let built = factory(context);
            async move { Ok(Arc::new(built.await?) as Arc<dyn BillingProvider>) }
        });
        ProviderFeature::<P>(registration, PhantomData)
    }
}

/// `BillingModule::provider::<P>(name, factory)`: one module per provider
/// type.
struct ProviderFeature<P>(ProviderRegistration, PhantomData<fn() -> P>);

impl<P: BillingProvider> Module for ProviderFeature<P> {
    fn register(&self, m: &mut ModuleBuilder) {
        m.contribute(self.0.clone());
    }
}

impl Module for BillingModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

/// `BillingModule::for_feature::<B>()`.
struct BillableFeature<B>(PhantomData<fn() -> B>);

impl<B: Billable + 'static> Module for BillableFeature<B> {
    fn register(&self, m: &mut ModuleBuilder) {
        m.contribute(BillableRegistration::of::<B>());
    }
}

fn register(m: &mut ModuleBuilder, options: Option<BillingOptions>) {
    m.global();
    match options {
        Some(options) => m.factory(Factory::<BillingOptions>::new(move |_| {
            let options = options.clone();
            async move {
                options
                    .check()
                    .map_err(|errors| format!("invalid billing options: {}", errors.join("; ")))?;
                Ok(Arc::new(options))
            }
        })),
        None => m.config::<BillingOptions>(),
    };
    m.exports::<BillingOptions>()
        .factory(
            Factory::<Billing>::new(|r: Resolver| async move {
                let options = r.get::<BillingOptions>().await?;
                let db = r.get::<Db>().await?;
                let app = r.app();
                let clock = match app.try_get::<dyn Clock>().await {
                    Some(clock) => clock,
                    None => Arc::new(SystemClock),
                };
                let app_url = match app.try_get::<Config>().await {
                    Some(config) => config.app_url(),
                    None => None,
                };
                let (provider, fake) = provider(&options, &app, clock.clone()).await?;
                Ok(Arc::new(Billing::new(
                    db,
                    options,
                    provider,
                    fake,
                    clock,
                    app.downgrade(),
                    app_url.filter(|url| !url.trim().is_empty()),
                )))
            })
            .deps(vec![
                ProviderKey::of::<BillingOptions>(),
                ProviderKey::of::<Db>(),
            ]),
        )
        .exports::<Billing>()
        .controller::<BillingController>()
        .migrations(migrations())
        .job::<ProcessBillingEvent>()
        .listen::<MemberAdded, SeatSync>()
        .on::<MemberRemoved, SeatSync>()
        .schedule(|s| {
            s.command("billing:report-usage", &[])
                .every_minute()
                .without_overlapping();
        })
        .map_error::<BillingError>(map_billing_error);
    for command in commands() {
        m.command(command);
    }
}

/// The configured provider (and the fake, when it is the fake).
async fn provider(
    options: &Arc<BillingOptions>,
    app: &AppContext,
    clock: Arc<dyn Clock>,
) -> Result<(Arc<dyn BillingProvider>, Option<Arc<BillingFake>>), BoxError> {
    let client = || async {
        match app.try_get::<HttpClient>().await {
            Some(client) => Ok::<_, BoxError>((*client).clone()),
            None => Ok(HttpClient::new(HttpClientOptions::default())?),
        }
    };
    Ok(match &options.provider {
        ProviderKind::Registered(name) => {
            let registrations: Vec<ProviderRegistration> = app
                .contributions::<ProviderRegistration>()
                .into_iter()
                .map(|contributed| (*contributed.value).clone())
                .collect();
            let context = ProviderContext::new(
                app.clone(),
                name.clone(),
                options.clone(),
                client().await?,
                clock,
                app.try_get::<Config>().await,
            );
            (registry::build(name, &registrations, context).await?, None)
        }
        ProviderKind::Stripe => {
            let key = options.stripe.secret_key.clone().ok_or(
                "billing.stripe.secret_key is required with billing.provider = \"stripe\"",
            )?;
            let provider = StripeProvider::new(
                client().await?,
                key,
                options.stripe.webhook_secret.clone(),
                options.stripe.api_base.clone(),
            );
            (Arc::new(provider), None)
        }
        ProviderKind::Paddle => {
            let key =
                options.paddle.api_key.clone().ok_or(
                    "billing.paddle.api_key is required with billing.provider = \"paddle\"",
                )?;
            let provider = PaddleProvider::new(
                client().await?,
                key,
                options.paddle.webhook_secret.clone(),
                options.paddle.base_url(),
            );
            (Arc::new(provider), None)
        }
        ProviderKind::Fake => {
            let fake = Arc::new(BillingFake::new(
                clock,
                Plans::new(&options.plans),
                options.currency.clone(),
                app.downgrade(),
            ));
            (fake.clone() as Arc<dyn BillingProvider>, Some(fake))
        }
    })
}
