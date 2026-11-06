//! Named transports (`mail.transports`), `Mailer::via`, `Mailer::deliver`
//! and the `SendReport`.

use std::sync::Arc;

use navree_contracts::{JobContext, JobRegistration};
use navree_kernel::{App, Navree};
use navree_mail::{
    ArrayTransport, DeliveryStatus, FailoverTransport, LogTransport, MailError, MailModule,
    MailScope, MailTransport, Mailer, Message, SendMailJob, TemplateMail, TransportConfig,
    TransportTier,
};
use serde_json::json;

use crate::support::{FROM, MailTemplates, array_options, dispatcher, view_module};

fn hello() -> Message {
    Message::new()
        .to("anu@example.com")
        .subject("Hi")
        .text("Hi")
}

async fn boot(options: navree_mail::MailOptions) -> App {
    Navree::app()
        .module(view_module())
        .module(MailTemplates)
        .module(MailModule::for_root(options))
        .boot()
        .await
        .expect("boot")
}

fn array_of(transport: &Arc<dyn MailTransport>) -> ArrayTransport {
    (transport.as_ref() as &dyn std::any::Any)
        .downcast_ref::<ArrayTransport>()
        .expect("an array transport")
        .clone()
}

#[tokio::test]
async fn via_sends_through_a_named_transport() {
    let mut options = array_options();
    options
        .transports
        .insert("simulation".to_owned(), TransportConfig::Array);
    let app = boot(options).await;
    let mailer = app.ctx().get::<Mailer>().await.expect("mailer");
    let default = mailer.array_transport().expect("default").clone();
    let simulation = array_of(mailer.named_transport("simulation").expect("named"));

    let report = mailer
        .via("simulation")
        .deliver(&hello())
        .await
        .expect("deliver");

    assert_eq!(report.status, DeliveryStatus::Sent);
    assert_eq!(report.transport, Some("array"));
    assert_eq!(report.tier, Some(TransportTier::Config));
    assert_eq!(report.delivery_id, None);
    default.assert_nothing_sent();
    simulation.assert_sent(|m| m.subject == "Hi" && m.from.as_deref() == Some(FROM));
}

#[tokio::test]
async fn an_unknown_transport_name_is_a_config_error() {
    let app = boot(array_options()).await;
    let mailer = app.ctx().get::<Mailer>().await.expect("mailer");

    let error = mailer.via("missing").send(&hello()).await.unwrap_err();

    assert!(
        matches!(&error, MailError::Config(message) if message.contains("mail.transports.missing")),
        "{error}"
    );
}

#[tokio::test]
async fn named_transports_come_from_config() {
    let options: navree_mail::MailOptions = serde_json::from_value(json!({
        "from": FROM,
        "transport": "array",
        "transports": { "notifications": "log", "simulation": { "type": "array" } }
    }))
    .expect("options");
    let app = boot(options).await;
    let mailer = app.ctx().get::<Mailer>().await.expect("mailer");

    let notifications = mailer.named_transport("notifications").expect("named");
    assert!((notifications.as_ref() as &dyn std::any::Any).is::<LogTransport>());
    assert_eq!(notifications.name(), "log");
    assert!(mailer.named_transport("simulation").is_some());
}

#[tokio::test]
async fn an_invalid_named_transport_fails_the_boot_naming_its_key() {
    let mut options = array_options();
    options
        .transports
        .insert("broken".to_owned(), TransportConfig::Failover(Vec::new()));

    let error = Navree::app()
        .module(view_module())
        .module(MailModule::for_root(options))
        .boot()
        .await
        .unwrap_err();

    assert!(error.to_string().contains("`transports.broken`"), "{error}");
}

#[tokio::test]
async fn deliver_reports_the_default_transport() {
    let app = boot(array_options()).await;
    let mailer = app.ctx().get::<Mailer>().await.expect("mailer");

    let report = mailer
        .deliver(
            &TemplateMail::new("anu@example.com", "Welcome", "mail/welcome")
                .context(json!({ "name": "Anu" })),
        )
        .await
        .expect("deliver");

    assert_eq!(report.status, DeliveryStatus::Sent);
    assert_eq!(report.transport, Some("array"));
    assert_eq!(report.provider_id, None);
    assert!(report.suppressed.is_empty());
}

#[tokio::test]
async fn failover_reports_the_transport_that_delivered() {
    let array = ArrayTransport::new();
    let failover = FailoverTransport::new(vec![
        Arc::new(Broken),
        Arc::new(array.clone()) as Arc<dyn MailTransport>,
    ]);

    let sent = failover
        .deliver(&hello().from(FROM))
        .await
        .expect("deliver");

    assert_eq!(sent.transport, "array");
    assert_eq!(failover.name(), "failover");
    assert_eq!(failover.transports().len(), 2);
    array.assert_sent(|m| m.subject == "Hi");
}

#[tokio::test]
async fn queued_mail_keeps_its_scope_transport_and_template() {
    let (capturing, dispatcher) = dispatcher();
    let mut options = array_options();
    options
        .transports
        .insert("simulation".to_owned(), TransportConfig::Array);
    let app = Navree::app()
        .module(view_module())
        .module(MailTemplates)
        .module(MailModule::for_root(options))
        .override_value(dispatcher)
        .boot()
        .await
        .expect("boot");
    let mailer = app.ctx().get::<Mailer>().await.expect("mailer");
    let workspace = uuid::Uuid::now_v7();

    mailer
        .for_scope(&MailScope::workspace(workspace))
        .via("simulation")
        .queue(
            &TemplateMail::new("anu@example.com", "Welcome", "mail/welcome")
                .context(json!({ "name": "Anu", "locale": "ar" })),
        )
        .await
        .expect("queue");

    let envelope = capturing.envelopes().remove(0);
    let job: SendMailJob = serde_json::from_value(envelope.payload.clone()).expect("job");
    assert_eq!(job.scope, Some(MailScope::workspace(workspace)));
    assert_eq!(job.via.as_deref(), Some("simulation"));
    assert_eq!(job.template.as_deref(), Some("mail/welcome"));
    // Rendered once, before queueing.
    assert_eq!(
        job.message.html.as_deref(),
        Some(r#"<div lang="ar" dir="rtl"><p>Welcome, Anu!</p></div>"#)
    );

    // The worker sends it through the named transport, without rendering
    // it again.
    run_job(&app, envelope.payload).await;
    let simulation = array_of(mailer.named_transport("simulation").expect("named"));
    assert_eq!(simulation.sent()[0].html, job.message.html);
    mailer
        .array_transport()
        .expect("default")
        .assert_nothing_sent();
}

/// Runs the `mail.send` registration on `payload`.
async fn run_job(app: &App, payload: serde_json::Value) {
    let registration = app
        .ctx()
        .contributions::<JobRegistration>()
        .into_iter()
        .find(|r| r.value.name == "mail.send")
        .expect("mail.send is registered");
    let ctx = JobContext {
        app: app.ctx().clone(),
        job_id: "job-1".to_owned(),
        attempt: 1,
    };
    (registration.value.run)(payload, ctx)
        .await
        .expect("the job sends");
}

/// Always fails.
struct Broken;

impl MailTransport for Broken {
    fn send<'a>(
        &'a self,
        _message: &'a Message,
    ) -> navree_kernel::BoxFuture<'a, Result<(), MailError>> {
        Box::pin(async {
            Err(MailError::Transport {
                transport: "broken",
                source: "down".into(),
            })
        })
    }
}
