//! The examples of `content/docs/tools/lighthouse.mdx`, written as an app
//! writes them (the facade, the prelude and the macros), compiled and run.
#![cfg(feature = "sqlite")]

use std::sync::Mutex;

use navree::BoxFuture;
use navree::lighthouse::{
    Alert, AlertChannel, AlertKind, Level, Lighthouse, LighthouseFake, LighthouseOptions, Report,
    Thresholds,
};
use navree::prelude::*;
use navree::testing::TestApp;

/// Why marking an invoice paid failed.
#[derive(Debug)]
pub enum InvoiceError {
    /// No invoice with that id.
    NotFound(String),
}

impl std::fmt::Display for InvoiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InvoiceError::NotFound(id) => write!(f, "invoice {id} not found"),
        }
    }
}

impl std::error::Error for InvoiceError {}

/// A payment provider's event.
pub struct PaymentEvent {
    invoice: String,
}

impl PaymentEvent {
    /// A paid event for `invoice`.
    pub fn paid(invoice: &str) -> PaymentEvent {
        PaymentEvent {
            invoice: invoice.to_owned(),
        }
    }
}

/// The invoices.
#[injectable]
pub struct InvoiceService;

impl InvoiceService {
    /// Marks the invoice of `event` paid.
    pub async fn mark_paid(&self, event: &PaymentEvent) -> std::result::Result<(), InvoiceError> {
        Err(InvoiceError::NotFound(event.invoice.clone()))
    }
}

// --- "Reporting errors yourself" --------------------------------------------

/// Handles a payment provider's webhooks.
#[injectable]
pub struct PaymentWebhooks {
    lighthouse: Arc<Lighthouse>,
    invoices: Arc<InvoiceService>,
}

impl PaymentWebhooks {
    /// Handles one event.
    pub async fn handle(&self, event: PaymentEvent) -> Result<NoContent> {
        if let Err(error) = self.invoices.mark_paid(&event).await {
            // Answer 204 so the provider stops retrying, but keep a record.
            self.lighthouse.report(&error);
        }
        Ok(NoContent)
    }

    /// The `Report` example.
    pub fn capture(&self, error: &InvoiceError, invoice_id: u64) {
        self.lighthouse.capture(
            Report::error(error)
                .level(Level::Warning)
                .context("invoice_id", invoice_id)
                .tag("provider", "acme-pay")
                .fingerprint(["acme-pay", "card-declined"])
                .backtrace(),
        );
        // No error value: something that should never happen.
        self.lighthouse
            .capture(Report::message("Payment event without a customer").level(Level::Warning));
        self.lighthouse.tag_current("plan", "pro");
    }
}

/// Billing.
#[module(providers = [InvoiceService, PaymentWebhooks])]
pub struct BillingModule;

// --- "Your own channels" ------------------------------------------------------

/// An SMS gateway (recording what it sends).
#[derive(Default)]
pub struct SmsGateway(Mutex<Vec<String>>);

impl SmsGateway {
    /// Sends `text` to `_to`.
    pub async fn send(&self, _to: &str, text: &str) -> std::result::Result<(), BoxError> {
        self.0.lock().unwrap().push(text.to_owned());
        Ok(())
    }
}

/// Alerts the on-call phone.
#[injectable]
pub struct OnCallSms {
    sms: Arc<SmsGateway>,
}

impl AlertChannel for OnCallSms {
    fn name(&self) -> &str {
        "on-call-sms"
    }

    fn wants(&self, alert: &Alert) -> bool {
        matches!(alert.kind, AlertKind::Regression | AlertKind::Spike)
    }

    fn send<'a>(&'a self, alert: &'a Alert) -> BoxFuture<'a, std::result::Result<(), BoxError>> {
        Box::pin(async move {
            self.sms
                .send(
                    "+15550100",
                    &format!("{}: {}", alert.kind, alert.issue.title),
                )
                .await?;
            Ok(())
        })
    }
}

/// Operations.
#[module(register = Self::wire)]
pub struct OpsModule;

impl OpsModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.value(SmsGateway::default());
        m.alert_channel::<OnCallSms>();
    }
}

// --- "Signing in and access" ---------------------------------------------------

/// The abilities function.
pub fn define(ctx: &AbilityCtx, rules: &mut Rules) {
    ctx.grant_permissions(rules);

    if let Some(user) = &ctx.user
        && user.has_role("ops")
    {
        rules.can("view", "Lighthouse");
        rules.can("resolve", "Lighthouse");
    }
}

#[test]
fn the_abilities_example_grants_view_and_resolve() {
    let mut rules = Rules::new();
    define(
        &AbilityCtx::for_user(uuid::Uuid::now_v7(), vec!["ops".into()], vec![]),
        &mut rules,
    );
    let ability = rules.build();
    assert!(ability.can("view", "Lighthouse", None));
    assert!(ability.can("resolve", "Lighthouse", None));
}

// --- "Testing" -----------------------------------------------------------------

#[tokio::test]
async fn a_payment_for_an_unknown_invoice_is_reported() {
    let fake = LighthouseFake::new();
    let app = TestApp::builder()
        .temp_database()
        .module(DatabaseModule::from_config())
        .module(LighthouseModule::from_config())
        .module(BillingModule)
        .override_value(Arc::new(fake.lighthouse()))
        .build()
        .await;

    let webhooks = app.get::<PaymentWebhooks>().await;
    webhooks
        .handle(PaymentEvent::paid("inv_missing"))
        .await
        .unwrap();

    fake.assert_reported::<InvoiceError>(|report| report.message.contains("not found"));
    fake.assert_reported_times(1);
    assert_eq!(fake.reports()[0].level, Level::Error);

    webhooks.capture(&InvoiceError::NotFound("inv_1".into()), 1042);
    fake.assert_reported_times(3);
    assert_eq!(fake.reports()[1].tags["provider"], "acme-pay");
    app.shutdown().await.unwrap();
}

// --- "Configuration" and "Your own channels" ----------------------------------

#[tokio::test]
async fn options_in_code_and_a_channel_of_your_own() {
    let app = TestApp::builder()
        .temp_database()
        .module(DatabaseModule::from_config())
        .module(LighthouseModule::for_root(LighthouseOptions {
            retention_days: 30,
            thresholds: Thresholds {
                slow_request_ms: 500,
                ..Thresholds::default()
            },
            dashboard: false,
            ..LighthouseOptions::default()
        }))
        .module(OpsModule)
        .build()
        .await;
    let lighthouse = app.get::<Lighthouse>().await;
    assert_eq!(lighthouse.options().retention_days, 30);
    let results = lighthouse.test_alerts(Some("on-call-sms")).await.unwrap();
    assert!(results[0].result.is_ok());
    let sms = app.get::<SmsGateway>().await;
    assert_eq!(
        sms.0.lock().unwrap().as_slice(),
        ["new_issue: Test alert from Lighthouse"]
    );
    app.run_command("lighthouse:prune", &["--days", "7"])
        .await
        .unwrap();
    app.shutdown().await.unwrap();
}

/// The app root of "Installation", with the modules it builds on.
fn app(sources: ConfigSources) -> AppBuilder {
    Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(LoggerModule::from_config())
        .module(CryptoModule::from_config())
        .module(HttpModule::from_config())
        .module(DatabaseModule::from_config())
        .module(QueueModule::from_config())
        .module(AuthModule::from_config())
        .module(AccessModule::for_root(AccessOptions {
            abilities: define,
            super_admin_role: Some("admin".to_owned()),
        }))
        .module(LighthouseModule::from_config())
}

#[test]
fn the_app_root_validates() {
    app(ConfigSources::new())
        .validate()
        .expect("every module's imports and providers line up");
}
