//! Telemetry: every mail handed to the transport is a `MailSent` event.

use std::sync::Arc;

use navree_kernel::Navree;
use navree_mail::{ArrayTransport, MailModule, Mailer, Message, TemplateMail};
use navree_telemetry::{Telemetry, TelemetryRecorder};
use navree_view::Views;
use serde_json::json;

use crate::support::{FROM, MailTemplates, array_options, view_module};

fn views() -> Arc<Views> {
    Arc::new(
        Views::from_templates([
            ("mail/welcome.html", "<p>Welcome, {{ name }}!</p>"),
            ("mail/welcome.txt", "Welcome, {{ name }}!"),
        ])
        .expect("templates"),
    )
}

fn recorded_mailer() -> (Mailer, TelemetryRecorder) {
    let recorder = TelemetryRecorder::new();
    let telemetry = Telemetry::with_sinks(vec![Arc::new(recorder.clone())]);
    let mailer =
        Mailer::new(Arc::new(ArrayTransport::new()), views(), FROM).with_telemetry(telemetry);
    (mailer, recorder)
}

#[tokio::test]
async fn sent_mail_is_reported_with_its_rendered_message() {
    let (mailer, recorder) = recorded_mailer();
    let mail = TemplateMail::new("anu@example.com", "Welcome", "mail/welcome")
        .context(json!({ "name": "Anu" }));
    mailer.send(&mail).await.expect("sent");

    let mails = recorder.mails();
    assert_eq!(mails.len(), 1);
    let sent = &mails[0];
    assert_eq!(sent.to, ["anu@example.com"]);
    assert_eq!(sent.from.as_deref(), Some(FROM));
    assert_eq!(sent.subject, "Welcome");
    assert_eq!(sent.text.as_deref(), Some("Welcome, Anu!"));
    assert_eq!(sent.html.as_deref(), Some("<p>Welcome, Anu!</p>"));
    assert_eq!(sent.transport, "array");
}

#[tokio::test]
async fn mail_that_is_not_sent_is_not_reported() {
    let (mailer, recorder) = recorded_mailer();
    let invalid = Message::new().subject("No recipient").text("hi");
    assert!(mailer.send_message(invalid).await.is_err());
    assert!(recorder.mails().is_empty());
}

#[tokio::test]
async fn queued_mail_sent_at_once_is_reported() {
    let (mailer, recorder) = recorded_mailer();
    let mail = TemplateMail::new("anu@example.com", "Welcome", "mail/welcome")
        .context(json!({ "name": "Anu" }));
    mailer
        .queue(&mail)
        .await
        .expect("sent without a dispatcher");
    assert_eq!(recorder.mails().len(), 1);
}

#[tokio::test]
async fn the_module_mailer_reports_to_the_app() {
    let recorder = TelemetryRecorder::new();
    let app = Navree::app()
        .module(view_module())
        .module(MailTemplates)
        .module(MailModule::for_root(array_options()))
        .module(recorder.module())
        .boot()
        .await
        .unwrap();
    let mailer = app.ctx().get::<Mailer>().await.unwrap();
    mailer
        .send_message(
            Message::new()
                .to("anu@example.com")
                .subject("Hi")
                .text("Hello"),
        )
        .await
        .unwrap();
    assert_eq!(recorder.mails()[0].transport, "array");
    app.shutdown().await.unwrap();
}
