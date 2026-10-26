//! `Mailer::queue`: renders now, dispatches job `mail.send`; the job sends.

use std::sync::Arc;

use navis_contracts::{JobContext, JobRegistration};
use navis_kernel::{App, Navis};
use navis_mail::{MailError, MailModule, MailOptions, Mailer, Message, TemplateMail};
use serde_json::json;

use crate::support::{FROM, MailTemplates, array_options, dispatcher, view_module};

fn welcome() -> TemplateMail {
    TemplateMail::new("ada@example.com", "Welcome", "mail/welcome")
        .context(json!({ "name": "Ada" }))
}

async fn mailer(app: &App) -> Arc<Mailer> {
    app.ctx().get::<Mailer>().await.expect("Mailer")
}

/// Runs the `mail.send` registration on `payload`, as a queue worker would.
async fn run_send_job(app: &App, payload: serde_json::Value) {
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

#[tokio::test]
async fn queue_dispatches_send_job_and_job_sends() {
    let (capturing, dispatcher) = dispatcher();
    let app = Navis::app()
        .module(view_module())
        .module(MailTemplates)
        .module(MailModule::for_root(MailOptions {
            queue: Some("mail".to_owned()),
            ..array_options()
        }))
        .override_value(dispatcher)
        .boot()
        .await
        .expect("boot");
    let mailer = mailer(&app).await;
    let array = mailer.array_transport().expect("array").clone();

    mailer.queue(&welcome()).await.expect("queue");

    // Rendered now, delivered later.
    array.assert_nothing_sent();
    let envelopes = capturing.envelopes();
    assert_eq!(envelopes.len(), 1);
    let envelope = &envelopes[0];
    assert_eq!(envelope.job_type, "mail.send");
    assert_eq!(envelope.options.queue, "mail");
    let expected = Message::new()
        .from(FROM)
        .to("ada@example.com")
        .subject("Welcome")
        .html("<p>Welcome, Ada!</p>")
        .text("Welcome, Ada!");
    assert_eq!(envelope.payload["message"], json!(expected));

    run_send_job(&app, envelope.payload.clone()).await;

    assert_eq!(array.sent(), vec![expected]);
}

#[tokio::test]
async fn queue_uses_the_jobs_default_queue_when_none_is_configured() {
    let (capturing, dispatcher) = dispatcher();
    let app = Navis::app()
        .module(view_module())
        .module(MailTemplates)
        .module(MailModule::for_root(array_options()))
        .override_value(dispatcher)
        .boot()
        .await
        .expect("boot");

    mailer(&app).await.queue(&welcome()).await.expect("queue");

    assert_eq!(capturing.envelopes()[0].options.queue, "default");
}

#[tokio::test]
async fn queue_sends_now_without_a_dispatcher() {
    let app = Navis::app()
        .module(view_module())
        .module(MailTemplates)
        .module(MailModule::for_root(array_options()))
        .boot()
        .await
        .expect("boot");
    let mailer = mailer(&app).await;

    mailer.queue(&welcome()).await.expect("queue");

    mailer
        .array_transport()
        .expect("array")
        .assert_sent(|m| m.subject == "Welcome");
}

#[tokio::test]
async fn queue_validates_before_dispatching() {
    let (capturing, dispatcher) = dispatcher();
    let app = Navis::app()
        .module(view_module())
        .module(MailModule::for_root(array_options()))
        .override_value(dispatcher)
        .boot()
        .await
        .expect("boot");

    let error = mailer(&app)
        .await
        .queue(&Message::new().to("nobody").subject("x").text("x"))
        .await
        .expect_err("invalid recipient");

    assert!(
        matches!(error, MailError::InvalidAddress { field: "to", .. }),
        "{error}"
    );
    assert!(capturing.envelopes().is_empty());
}
