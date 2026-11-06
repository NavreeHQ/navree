//! The delivery pipeline: the log, provider events, suppressions and the
//! outbox.

use std::sync::Arc;

use navree_db::PageQuery;
use navree_kernel::BoxFuture;
use navree_mail::{
    Deliveries, DeliveryFilter, DeliveryStatus, MailError, MailEventKind, MailModule, MailScope,
    MailTransport, Mailable, Message, Outbox, OutboxStatus, ProviderEvent, SuppressionReason,
    Suppressions, TemplateMail, TransportTier,
};
use navree_testing::TestApp;
use serde_json::json;
use sha2::{Digest as _, Sha256};
use uuid::Uuid;

use crate::harness::{MailApp, mail_app};
use crate::support::{FROM, view_module};

fn welcome(to: &str) -> TemplateMail {
    TemplateMail::new(to, "Welcome", "mail/welcome").context(json!({ "name": "Anu" }))
}

fn hash(address: &str) -> String {
    hex::encode(Sha256::digest(address.as_bytes()))
}

async fn app() -> MailApp {
    mail_app(|b| b).await
}

#[tokio::test]
async fn every_message_is_logged_with_the_providers_id() {
    let mail = app().await;

    let report = mail
        .mailer()
        .await
        .deliver(&welcome("Anu <Anu@Example.com>"))
        .await
        .expect("deliver");

    assert_eq!(report.status, DeliveryStatus::Sent);
    assert_eq!(report.transport, Some("fake"));
    assert_eq!(report.tier, Some(TransportTier::Config));
    assert_eq!(report.provider_id.as_deref(), Some("fake-1"));
    let id = report.delivery_id.expect("a delivery id");
    let delivery = mail.get::<Deliveries>().await.find(id).await.expect("find");
    assert_eq!(delivery.status, DeliveryStatus::Sent);
    assert_eq!(delivery.transport, "fake");
    assert_eq!(delivery.provider_id.as_deref(), Some("fake-1"));
    assert_eq!(delivery.template.as_deref(), Some("mail/welcome"));
    assert_eq!(delivery.subject, "Welcome");
    assert_eq!(delivery.sender, "app@example.com");
    assert_eq!(delivery.recipient_hash, hash("anu@example.com"));
    assert_eq!(delivery.recipients, 1);
    mail.fake.assert_sent(|m| m.subject == "Welcome");
}

/// Always fails.
struct Broken;

impl MailTransport for Broken {
    fn send<'a>(&'a self, _message: &'a Message) -> BoxFuture<'a, Result<(), MailError>> {
        Box::pin(async {
            Err(MailError::Transport {
                transport: "broken",
                source: "connection refused".into(),
            })
        })
    }
}

#[tokio::test]
async fn a_refused_message_is_logged_as_failed() {
    let mail = mail_app(|b| b.override_value::<dyn MailTransport>(Arc::new(Broken))).await;

    let error = mail.mailer().await.send(&welcome("anu@example.com")).await;

    assert!(error.is_err());
    let page = mail
        .get::<Deliveries>()
        .await
        .list(
            &DeliveryFilter {
                status: Some(DeliveryStatus::Failed),
                ..DeliveryFilter::default()
            },
            PageQuery::new(1, 10),
        )
        .await
        .expect("list");
    assert_eq!(page.data.len(), 1);
    assert!(
        page.data[0]
            .error
            .as_deref()
            .is_some_and(|e| e.contains("connection refused"))
    );
}

#[tokio::test]
async fn a_hard_bounce_suppresses_the_address_and_later_mail_is_not_sent() {
    let mail = app().await;
    let mut bounced = mail.events("mail.bounced").await;
    let first = mail
        .mailer()
        .await
        .deliver(&welcome("anu@example.com"))
        .await
        .expect("deliver");

    let delivery = mail
        .fake
        .bounce(mail.app.ctx(), "anu@example.com")
        .await
        .expect("record")
        .expect("the delivery is found");

    assert_eq!(delivery.id, first.delivery_id.unwrap());
    assert_eq!(delivery.status, DeliveryStatus::Bounced);
    let events = mail
        .get::<Deliveries>()
        .await
        .events(delivery.id)
        .await
        .expect("events");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, MailEventKind::Bounced);
    assert_eq!(events[0].bounce_type.as_deref(), Some("hard"));
    assert_eq!(events[0].provider, "fake");
    assert_eq!(
        events[0].recipient_hash.as_deref(),
        Some(hash("anu@example.com").as_str())
    );
    let event = bounced.try_recv().expect("MailBounced emitted");
    assert_eq!(event["permanent"], true);
    assert_eq!(event["recipient"], "anu@example.com");
    assert_eq!(event["delivery_id"], json!(delivery.id));

    let suppressions = mail.get::<Suppressions>().await;
    assert!(
        suppressions
            .is_suppressed(None, "ANU@example.com")
            .await
            .unwrap()
    );

    // The next message is not sent, and is logged as suppressed.
    let second = mail
        .mailer()
        .await
        .deliver(&welcome("Anu <anu@example.com>"))
        .await
        .expect("deliver");
    assert_eq!(second.status, DeliveryStatus::Suppressed);
    assert_eq!(second.suppressed, ["Anu <anu@example.com>"]);
    assert_eq!(mail.fake.sent().len(), 1);
    let logged = mail
        .get::<Deliveries>()
        .await
        .find(second.delivery_id.unwrap())
        .await
        .unwrap();
    assert_eq!(logged.status, DeliveryStatus::Suppressed);
}

#[tokio::test]
async fn soft_bounces_and_deferrals_change_nothing() {
    let mail = app().await;
    mail.mailer()
        .await
        .send(&welcome("anu@example.com"))
        .await
        .unwrap();

    let delivery = mail
        .fake
        .soft_bounce(mail.app.ctx(), "anu@example.com")
        .await
        .unwrap()
        .unwrap();

    assert_eq!(delivery.status, DeliveryStatus::Sent);
    let suppressions = mail.get::<Suppressions>().await;
    assert!(
        !suppressions
            .is_suppressed(None, "anu@example.com")
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn a_complaint_suppresses_with_its_reason() {
    let mail = app().await;
    mail.mailer()
        .await
        .send(&welcome("anu@example.com"))
        .await
        .unwrap();

    let delivery = mail
        .fake
        .complain(mail.app.ctx(), "anu@example.com")
        .await
        .unwrap()
        .unwrap();

    assert_eq!(delivery.status, DeliveryStatus::Complained);
    let page = mail
        .get::<Suppressions>()
        .await
        .list(None, Some("anu@example.com"), PageQuery::new(1, 10))
        .await
        .unwrap();
    assert_eq!(page.data[0].reason, SuppressionReason::Complaint);
    assert_eq!(page.data[0].delivery_id, Some(delivery.id));
}

#[tokio::test]
async fn deliveries_opens_and_clicks_are_stamped() {
    let mail = app().await;
    let mut clicked = mail.events("mail.clicked").await;
    mail.mailer()
        .await
        .send(&welcome("anu@example.com"))
        .await
        .unwrap();

    let delivered = mail
        .fake
        .deliver(mail.app.ctx(), "anu@example.com")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(delivered.status, DeliveryStatus::Delivered);
    assert_eq!(delivered.opened_at, None);

    let clicked_delivery = mail
        .fake
        .click(mail.app.ctx(), "anu@example.com", "https://acme.test/start")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(clicked_delivery.status, DeliveryStatus::Delivered);
    assert!(
        clicked_delivery.opened_at.is_some(),
        "a click implies an open"
    );
    assert!(clicked_delivery.clicked_at.is_some());
    assert_eq!(
        clicked.try_recv().unwrap()["url"],
        "https://acme.test/start"
    );
    assert_eq!(mail.fake.events().len(), 2);
}

#[tokio::test]
async fn auto_suppression_can_be_turned_off() {
    let mail = mail_app(|b| b.config("mail.delivery.auto_suppress", false)).await;
    mail.mailer()
        .await
        .send(&welcome("anu@example.com"))
        .await
        .unwrap();

    mail.fake
        .bounce(mail.app.ctx(), "anu@example.com")
        .await
        .unwrap();

    let suppressions = mail.get::<Suppressions>().await;
    assert!(
        !suppressions
            .is_suppressed(None, "anu@example.com")
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn events_for_unknown_messages_are_still_recorded() {
    let mail = app().await;

    let found = mail
        .get::<Deliveries>()
        .await
        .record(
            ProviderEvent::new("ses", "unknown-id", MailEventKind::Complained)
                .recipient("eve@example.com"),
        )
        .await
        .unwrap();

    assert_eq!(found, None);
    let suppressions = mail.get::<Suppressions>().await;
    assert!(
        suppressions
            .is_suppressed(None, "eve@example.com")
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn suppressions_apply_per_workspace_and_everywhere() {
    let mail = app().await;
    let (a, b) = (Uuid::now_v7(), Uuid::now_v7());
    let suppressions = mail.get::<Suppressions>().await;
    let uow = mail.uow().await;
    suppressions
        .add(
            &uow,
            Some(a),
            "anu@example.com",
            SuppressionReason::Unsubscribed,
            None,
        )
        .await
        .unwrap();
    suppressions
        .add(
            &uow,
            None,
            "eve@example.com",
            SuppressionReason::Manual,
            Some("spam trap".into()),
        )
        .await
        .unwrap();
    // Adding again returns the existing row.
    let again = suppressions
        .add(
            &uow,
            Some(a),
            "Anu@Example.com",
            SuppressionReason::Manual,
            None,
        )
        .await
        .unwrap();
    assert_eq!(again.reason, SuppressionReason::Unsubscribed);
    uow.commit().await.unwrap();
    let mailer = mail.mailer().await;

    let in_a = mailer
        .for_scope(&MailScope::workspace(a))
        .deliver(
            &Message::new()
                .to("anu@example.com")
                .to("bob@example.com")
                .subject("Hi")
                .text("Hi"),
        )
        .await
        .unwrap();
    assert_eq!(in_a.status, DeliveryStatus::Sent);
    assert_eq!(in_a.suppressed, ["anu@example.com"]);
    assert_eq!(mail.fake.sent()[0].to, ["bob@example.com"]);

    let in_b = mailer
        .for_scope(&MailScope::workspace(b))
        .deliver(&welcome("anu@example.com"))
        .await
        .unwrap();
    assert_eq!(in_b.status, DeliveryStatus::Sent);

    let everywhere = mailer
        .for_scope(&MailScope::workspace(b))
        .deliver(&welcome("eve@example.com"))
        .await
        .unwrap();
    assert_eq!(everywhere.status, DeliveryStatus::Suppressed);

    // Logged under the workspace.
    let page = mail
        .get::<Deliveries>()
        .await
        .list(
            &DeliveryFilter {
                workspace_id: Some(a),
                ..DeliveryFilter::default()
            },
            PageQuery::new(1, 10),
        )
        .await
        .unwrap();
    assert_eq!(page.data.len(), 1);
    assert_eq!(page.data[0].workspace_id, Some(a));

    // Removing a suppression.
    let uow = mail.uow().await;
    assert!(
        suppressions
            .remove_email(&uow, Some(a), "anu@example.com")
            .await
            .unwrap()
    );
    assert!(
        !suppressions
            .remove_email(&uow, Some(a), "anu@example.com")
            .await
            .unwrap()
    );
    uow.commit().await.unwrap();
    assert!(
        !suppressions
            .is_suppressed(Some(a), "anu@example.com")
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn suppression_checks_can_be_turned_off() {
    let mail = mail_app(|b| b.config("mail.delivery.suppress", false)).await;
    let uow = mail.uow().await;
    mail.get::<Suppressions>()
        .await
        .add(
            &uow,
            None,
            "anu@example.com",
            SuppressionReason::Manual,
            None,
        )
        .await
        .unwrap();
    uow.commit().await.unwrap();

    let report = mail
        .mailer()
        .await
        .deliver(&welcome("anu@example.com"))
        .await
        .unwrap();

    assert_eq!(report.status, DeliveryStatus::Sent);
}

#[tokio::test]
async fn held_mail_waits_in_the_outbox_until_released() {
    let mail = app().await;
    let workspace = Uuid::now_v7();

    let report = mail
        .mailer()
        .await
        .for_scope(&MailScope::workspace(workspace))
        .deliver(&welcome("all@acme.test").hold("Mail to every employee needs approval"))
        .await
        .unwrap();

    assert_eq!(report.status, DeliveryStatus::Held);
    mail.fake.assert_nothing_sent();
    let outbox = mail.get::<Outbox>().await;
    let id = report.outbox_id.expect("an outbox id");
    let held = outbox.find(id).await.unwrap();
    assert_eq!(held.status, OutboxStatus::Held);
    assert_eq!(held.reason, "Mail to every employee needs approval");
    assert_eq!(held.recipients, ["all@acme.test"]);
    assert_eq!(held.workspace_id, Some(workspace));
    assert_eq!(held.template.as_deref(), Some("mail/welcome"));
    let preview = outbox.message(id).await.unwrap();
    assert_eq!(preview.html.as_deref(), Some("<p>Welcome, Anu!</p>"));
    let listed = outbox
        .list(Some(OutboxStatus::Held), None, PageQuery::new(1, 10))
        .await
        .unwrap();
    assert_eq!(listed.data.len(), 1);

    let admin = Uuid::now_v7();
    let released = outbox.release(id, Some(admin)).await.unwrap();

    assert_eq!(released.status, DeliveryStatus::Sent);
    mail.fake
        .assert_sent(|m| m.subject == "Welcome" && m.hold.is_some());
    let held = outbox.find(id).await.unwrap();
    assert_eq!(held.status, OutboxStatus::Released);
    assert_eq!(held.decided_by, Some(admin));
    assert_eq!(held.delivery_id, released.delivery_id);
    let delivery = mail
        .get::<Deliveries>()
        .await
        .find(released.delivery_id.unwrap())
        .await
        .unwrap();
    assert_eq!(delivery.workspace_id, Some(workspace));

    let again = outbox.release(id, Some(admin)).await.unwrap_err();
    assert!(matches!(again, MailError::Conflict(_)), "{again}");
}

#[tokio::test]
async fn discarded_mail_is_never_sent() {
    let mail = app().await;
    mail.mailer()
        .await
        .queue(&welcome("all@acme.test").hold("Needs approval"))
        .await
        .unwrap();
    let outbox = mail.get::<Outbox>().await;
    let id = outbox
        .list(None, None, PageQuery::new(1, 10))
        .await
        .unwrap()
        .data[0]
        .id;

    let discarded = outbox.discard(id, None).await.unwrap();

    assert_eq!(discarded.status, OutboxStatus::Discarded);
    assert!(matches!(
        outbox.release(id, None).await,
        Err(MailError::Conflict(_))
    ));
    assert!(matches!(
        outbox.discard(id, None).await,
        Err(MailError::Conflict(_))
    ));
    assert!(matches!(
        outbox.find(Uuid::now_v7()).await,
        Err(MailError::NotFound(_))
    ));
    mail.fake.assert_nothing_sent();
}

#[tokio::test]
async fn held_mail_needs_an_app_key() {
    let mail = mail_app(|b| b.config("app.key", "")).await;

    let error = mail
        .mailer()
        .await
        .send(&welcome("all@acme.test").hold("Needs approval"))
        .await
        .unwrap_err();

    assert!(error.to_string().contains("APP_KEY"), "{error}");
}

#[tokio::test]
async fn delivery_without_a_database_fails_the_boot() {
    let Err(error) = TestApp::builder()
        .config("mail.delivery.enabled", true)
        .module(view_module())
        .module(MailModule::from_config())
        .try_build()
        .await
    else {
        panic!("the boot should fail");
    };

    assert!(error.to_string().contains("DatabaseModule"), "{error}");
}

#[tokio::test]
async fn without_the_pipeline_nothing_is_logged() {
    let mail = mail_app(|b| b.config("mail.delivery.enabled", false)).await;

    let report = mail
        .mailer()
        .await
        .deliver(&welcome("anu@example.com"))
        .await
        .unwrap();

    assert_eq!(report.delivery_id, None);
    let page = mail
        .get::<Deliveries>()
        .await
        .list(&DeliveryFilter::default(), PageQuery::new(1, 10))
        .await
        .unwrap();
    assert!(page.data.is_empty());
    assert_eq!(FROM, "App <app@example.com>");
}
