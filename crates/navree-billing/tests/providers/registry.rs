//! An open provider trait (CONN-101): a provider the app (or a crate)
//! registers with `BillingModule::provider` is selected by
//! `billing.provider`, reads its own section, takes UPI payments, and has
//! its webhooks checked by its own scheme (an HMAC of the body, with the
//! delivery id in a header, as Indian gateways do). Mistakes in the
//! registration fail the boot.

use std::collections::BTreeMap;
use std::time::Duration;

use chrono::{DateTime, Utc};
use hmac::{Hmac, KeyInit, Mac};
use http::{HeaderMap, HeaderValue};
use navree_billing::{
    Billing, BillingError, BillingModule, BillingProvider, Capability, Money, PaymentAction,
    PaymentMethod, PaymentRequest, PaymentStatus, ProviderContext, ProviderError, ProviderPayment,
    WebhookAction, WebhookError, WebhookEvent,
};
use navree_contracts::Secret;
use navree_db::DatabaseModule;
use navree_events::{EventBus, EventsModule};
use navree_kernel::{BoxError, BoxFuture};
use navree_testing::{TestApp, TestAppBuilder};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::Sha256;

/// `billing.acme_pay`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct AcmeOptions {
    webhook_secret: Option<Secret<String>>,
    vpa: String,
}

/// A UPI gateway shaped like the Indian ones: amounts in paise, UPI intent
/// links, notes as metadata, webhooks signed with an HMAC of the body.
struct AcmePay {
    secret: Option<Secret<String>>,
    vpa: String,
}

impl AcmePay {
    fn new(context: &ProviderContext) -> Result<AcmePay, BoxError> {
        let options: AcmeOptions = context.section("billing.acme_pay")?;
        if options.vpa.is_empty() {
            return Err("billing.acme_pay.vpa is required".into());
        }
        Ok(AcmePay {
            secret: options.webhook_secret,
            vpa: options.vpa,
        })
    }
}

fn hmac_hex(secret: &str, body: &[u8]) -> String {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(body);
    hex::encode(mac.finalize().into_bytes())
}

impl BillingProvider for AcmePay {
    fn name(&self) -> &'static str {
        "acme_pay"
    }

    fn capabilities(&self) -> &'static [Capability] {
        &[Capability::OneTimePayments, Capability::Upi]
    }

    fn create_payment<'a>(
        &'a self,
        request: &'a PaymentRequest,
    ) -> BoxFuture<'a, Result<ProviderPayment, ProviderError>> {
        Box::pin(async move {
            let url = format!(
                "upi://pay?pa={}&pn=Acme&am={}&cu={}&tr={}",
                self.vpa,
                request.amount.major(),
                request.amount.currency_code(),
                request.reference
            );
            Ok(ProviderPayment {
                id: format!("order_{}", request.reference),
                payment_id: None,
                link_id: None,
                reference: Some(request.reference.clone()),
                status: PaymentStatus::Pending,
                amount: request.amount.clone(),
                method: Some(PaymentMethod::Upi),
                customer_id: None,
                next_action: Some(PaymentAction::UpiIntent { url }),
                metadata: request.metadata.clone(),
            })
        })
    }

    fn webhook_event_id(&self, headers: &HeaderMap) -> Option<String> {
        headers
            .get("x-acme-event-id")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    }

    fn verify_webhook(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        _now: DateTime<Utc>,
        _tolerance: Duration,
    ) -> Result<(), WebhookError> {
        let secret = self.secret.as_ref().ok_or(WebhookError::NoSecret)?;
        let signature = headers
            .get("x-acme-signature")
            .ok_or(WebhookError::MissingSignature("x-acme-signature"))?
            .to_str()
            .map_err(|_| WebhookError::MalformedSignature("x-acme-signature"))?;
        if signature == hmac_hex(secret.expose(), body) {
            Ok(())
        } else {
            Err(WebhookError::SignatureMismatch)
        }
    }

    fn parse_webhook(&self, body: &[u8]) -> Result<WebhookEvent, ProviderError> {
        let decode = |message: &str| ProviderError::Decode {
            provider: "acme_pay",
            message: message.to_owned(),
        };
        let event: Value = serde_json::from_slice(body).map_err(|_| decode("not JSON"))?;
        let entity = &event["payload"]["payment"]["entity"];
        let notes: BTreeMap<String, String> =
            serde_json::from_value(entity["notes"].clone()).unwrap_or_default();
        let status = match event["event"].as_str() {
            Some("payment.captured") => PaymentStatus::Paid,
            Some("payment.failed") => PaymentStatus::Failed,
            _ => return Err(decode("unknown event")),
        };
        Ok(WebhookEvent {
            // The body has no id; `webhook_event_id` reads the header.
            id: String::new(),
            event_type: event["event"].as_str().unwrap_or_default().to_owned(),
            occurred_at: DateTime::from_timestamp(event["created_at"].as_i64().unwrap_or(0), 0)
                .ok_or_else(|| decode("bad created_at"))?,
            actions: vec![WebhookAction::Payment(ProviderPayment {
                id: entity["order_id"].as_str().unwrap_or_default().to_owned(),
                payment_id: entity["id"].as_str().map(str::to_owned),
                link_id: None,
                reference: notes.get("reference").cloned(),
                status,
                amount: Money::new(
                    entity["amount"].as_i64().unwrap_or_default(),
                    entity["currency"].as_str().unwrap_or_default(),
                ),
                method: entity["method"].as_str().and_then(|m| m.parse().ok()),
                customer_id: None,
                next_action: None,
                metadata: notes,
            })],
        })
    }
}

/// A provider that calls itself something else.
struct Liar;

impl BillingProvider for Liar {
    fn name(&self) -> &'static str {
        "someone_else"
    }

    fn verify_webhook(
        &self,
        _: &HeaderMap,
        _: &[u8],
        _: DateTime<Utc>,
        _: Duration,
    ) -> Result<(), WebhookError> {
        Err(WebhookError::NoSecret)
    }

    fn parse_webhook(&self, _: &[u8]) -> Result<WebhookEvent, ProviderError> {
        Err(ProviderError::Unsupported {
            provider: "someone_else",
            operation: "webhooks",
        })
    }
}

fn acme_pay() -> impl navree_kernel::Module {
    BillingModule::provider("acme_pay", |context: ProviderContext| async move {
        AcmePay::new(&context)
    })
}

fn app() -> TestAppBuilder {
    crate::servers::database(TestApp::builder())
        .config("billing.routes", false)
        .config("billing.provider", "acme_pay")
        .config("billing.acme_pay.vpa", "acmepay@icici")
        .config("billing.acme_pay.webhook_secret", "acme_whsec_1")
        .module(DatabaseModule::from_config())
        .module(EventsModule::for_root())
        .module(BillingModule::from_config())
}

fn captured(reference: &str) -> Value {
    json!({
        "entity": "event",
        "event": "payment.captured",
        "created_at": 1_793_005_200,
        "payload": { "payment": { "entity": {
            "id": "pay_Q8upi1", "order_id": format!("order_{reference}"), "amount": 149_900,
            "currency": "INR", "method": "upi", "vpa": "harpreet@okhdfcbank",
            "notes": { "reference": reference, "billable_type": "user",
                       "billable_id": "0199a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b" }
        } } }
    })
}

fn signed(body: &[u8], event_id: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-acme-signature",
        HeaderValue::from_str(&hmac_hex("acme_whsec_1", body)).unwrap(),
    );
    headers.insert("x-acme-event-id", HeaderValue::from_str(event_id).unwrap());
    headers
}

// CONN-101
#[tokio::test]
async fn a_registered_provider_takes_upi_payments_and_its_own_webhooks() {
    let app = app().module(acme_pay()).build().await;
    let billing = app.get::<Billing>().await;
    assert_eq!(billing.provider().name(), "acme_pay");
    assert!(billing.supports(Capability::Upi));
    assert!(!billing.supports(Capability::Subscriptions));

    let payment = billing
        .payment(Money::inr(149_900), "Chai subscription box, Mohali")
        .reference("INV-2026-0042")
        .customer_phone("+919876543210")
        .methods([PaymentMethod::Upi])
        .create()
        .await
        .unwrap();
    assert_eq!(
        payment.next_action,
        Some(PaymentAction::UpiIntent {
            url: "upi://pay?pa=acmepay@icici&pn=Acme&am=1499.00&cu=INR&tr=INV-2026-0042".into()
        })
    );
    // What the provider does not do answers Unsupported.
    let error = billing
        .payment_link(Money::inr(100), "Tea")
        .create()
        .await
        .unwrap_err();
    assert!(
        matches!(
            error,
            BillingError::Provider(ProviderError::Unsupported { .. })
        ),
        "{error:?}"
    );

    let mut paid = app
        .get::<EventBus>()
        .await
        .subscribe_json("payment.updated");
    let body = serde_json::to_vec(&captured("INV-2026-0042")).unwrap();
    let receipt = billing
        .receive_webhook(&signed(&body, "evt_hdr_1"), &body)
        .await
        .unwrap();
    assert!(!receipt.duplicate);
    assert_eq!(receipt.provider_event_id, "evt_hdr_1", "the header's id");
    let event = paid.try_recv().unwrap();
    assert_eq!(event["provider"], "acme_pay");
    assert_eq!(event["status"], "paid");
    assert_eq!(event["method"], "upi");
    assert_eq!(event["reference"], "INV-2026-0042");
    assert_eq!(event["payment_id"], "pay_Q8upi1");
    assert_eq!(
        event["amount"],
        json!({ "amount": 149_900, "currency": "inr" })
    );
    assert_eq!(event["billable_type"], "user");

    // A redelivery has the same delivery id: stored and processed once.
    let pretty = serde_json::to_vec_pretty(&captured("INV-2026-0042")).unwrap();
    let again = billing
        .receive_webhook(&signed(&pretty, "evt_hdr_1"), &pretty)
        .await
        .unwrap();
    assert!(again.duplicate);
    assert!(paid.try_recv().is_err());

    // Its own scheme refuses a body it did not sign.
    let mut forged = signed(&body, "evt_hdr_2");
    forged.insert("x-acme-signature", HeaderValue::from_static("00ff"));
    let error = billing.receive_webhook(&forged, &body).await.unwrap_err();
    assert!(
        matches!(
            error,
            BillingError::InvalidWebhook(WebhookError::SignatureMismatch)
        ),
        "{error:?}"
    );
}

async fn boot_error(builder: TestAppBuilder) -> String {
    builder
        .try_build()
        .await
        .map(|_| ())
        .expect_err("the boot must fail")
        .to_string()
}

// CONN-101
#[tokio::test]
async fn registration_mistakes_fail_the_boot() {
    let error = boot_error(app()).await;
    assert!(error.contains("billing.provider is `acme_pay`"), "{error}");
    assert!(error.contains("BillingModule::provider"), "{error}");

    let error = boot_error(app().module(BillingModule::provider(
        "stripe",
        |_: ProviderContext| async { Ok::<_, BoxError>(Liar) },
    )))
    .await;
    assert!(error.contains("built-in"), "{error}");

    let error = boot_error(app().module(BillingModule::provider(
        "acme_pay",
        |_: ProviderContext| async { Ok::<_, BoxError>(Liar) },
    )))
    .await;
    assert!(error.contains("names itself `someone_else`"), "{error}");

    let error = boot_error(app().module(acme_pay()).module(BillingModule::provider(
        "acme_pay",
        |_: ProviderContext| async { Ok::<_, BoxError>(Liar) },
    )))
    .await;
    assert!(error.contains("registered 2 times"), "{error}");

    // The provider's own checks fail the boot too.
    let error = boot_error(app().config("billing.acme_pay.vpa", "").module(acme_pay())).await;
    assert!(error.contains("billing.acme_pay.vpa"), "{error}");
}
