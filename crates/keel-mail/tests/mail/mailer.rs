//! `Mailer::send` and `Mailer::send_message` with templates and validation.

use std::sync::Arc;

use keel_mail::{ArrayTransport, MailError, Mailable, Mailer, Message, TemplateMail};
use keel_view::{ViewError, Views};
use serde_json::json;

const FROM: &str = "App <app@example.com>";

fn views() -> Arc<Views> {
    Arc::new(
        Views::from_templates([
            ("mail/welcome.html", "<p>Welcome, {{ name }}!</p>"),
            ("mail/welcome.txt", "Welcome, {{ name }}!"),
            ("mail/receipt.txt", "Receipt for {{ name }}"),
            ("mail/promo.html", "<b>{{ offer }}</b>"),
        ])
        .expect("templates"),
    )
}

fn mailer() -> (Mailer, ArrayTransport) {
    let transport = ArrayTransport::new();
    let mailer = Mailer::new(Arc::new(transport.clone()), views(), FROM);
    (mailer, transport)
}

#[tokio::test]
async fn send_renders_template_into_message() {
    let (mailer, transport) = mailer();
    let mail = TemplateMail::new("ada@example.com", "Welcome", "mail/welcome")
        .context(json!({ "name": "Ada & Co" }));

    mailer.send(&mail).await.expect("send");

    assert_eq!(
        transport.sent(),
        vec![
            Message::new()
                .from(FROM)
                .to("ada@example.com")
                .subject("Welcome")
                .html("<p>Welcome, Ada &amp; Co!</p>")
                .text("Welcome, Ada & Co!")
        ]
    );
}

#[test]
fn template_mail_renders_only_the_parts_that_exist() {
    let views = views();

    let text_only = TemplateMail::new("ada@example.com", "Receipt", "mail/receipt")
        .context(json!({ "name": "Ada" }))
        .build(&views)
        .expect("text only");
    assert_eq!(text_only.text.as_deref(), Some("Receipt for Ada"));
    assert_eq!(text_only.html, None);

    let html_only = TemplateMail::new("ada@example.com", "Promo", "mail/promo")
        .context(json!({ "offer": "50%" }))
        .build(&views)
        .expect("html only");
    assert_eq!(html_only.html.as_deref(), Some("<b>50%</b>"));
    assert_eq!(html_only.text, None);
}

#[test]
fn template_mail_with_an_extension_renders_that_template() {
    let message = TemplateMail::new("ada@example.com", "Welcome", "mail/welcome.txt")
        .context(json!({ "name": "Ada" }))
        .build(&views())
        .expect("build");

    assert_eq!(message.text.as_deref(), Some("Welcome, Ada!"));
    assert_eq!(message.html, None);
}

#[tokio::test]
async fn send_fails_for_a_missing_template_and_sends_nothing() {
    let (mailer, transport) = mailer();
    let mail = TemplateMail::new("ada@example.com", "Gone", "mail/missing");

    let error = mailer.send(&mail).await.expect_err("missing template");

    assert!(
        matches!(error, MailError::View(ViewError::NotFound { .. })),
        "{error}"
    );
    transport.assert_nothing_sent();
}

#[tokio::test]
async fn send_message_fills_in_the_default_sender_only_when_missing() {
    let (mailer, transport) = mailer();

    mailer
        .send_message(Message::new().to("ada@example.com").subject("A").text("a"))
        .await
        .expect("default sender");
    mailer
        .send_message(
            Message::new()
                .from("billing@example.com")
                .to("ada@example.com")
                .subject("B")
                .text("b"),
        )
        .await
        .expect("explicit sender");

    let sent = transport.sent();
    assert_eq!(sent[0].from.as_deref(), Some(FROM));
    assert_eq!(sent[1].from.as_deref(), Some("billing@example.com"));
}

#[tokio::test]
async fn send_message_rejects_invalid_messages_before_the_transport() {
    let (mailer, transport) = mailer();

    let error = mailer
        .send_message(Message::new().to("not an address").subject("x").text("x"))
        .await
        .expect_err("invalid recipient");
    assert!(
        matches!(&error, MailError::InvalidAddress { field: "to", address, .. } if address == "not an address"),
        "{error}"
    );

    let error = mailer
        .send_message(Message::new().subject("x").text("x"))
        .await
        .expect_err("no recipients");
    assert!(matches!(error, MailError::NoRecipients), "{error}");

    let error = mailer
        .send_message(
            Message::new()
                .to("ada@example.com")
                .subject("Hi\r\nBcc: victim@example.com")
                .text("x"),
        )
        .await
        .expect_err("header injection");
    assert!(
        matches!(error, MailError::InvalidHeader { field: "subject" }),
        "{error}"
    );

    transport.assert_nothing_sent();
}

#[test]
fn validate_names_the_field_holding_a_bad_address() {
    let valid = || Message::new().from(FROM).to("ada@example.com").subject("x");
    for (message, expected) in [
        (valid().from("bad"), "from"),
        (valid().cc("bad"), "cc"),
        (valid().bcc("bad"), "bcc"),
        (valid().reply_to("bad"), "reply_to"),
    ] {
        match message.validate() {
            Err(MailError::InvalidAddress { field, .. }) => assert_eq!(field, expected),
            other => panic!("{expected}: unexpected {other:?}"),
        }
    }
    // A bcc-only message has a recipient.
    let bcc_only = Message::new()
        .from(FROM)
        .bcc("ada@example.com")
        .subject("x");
    bcc_only.validate().expect("bcc only");
}

#[tokio::test]
async fn send_message_without_any_sender_fails() {
    let transport = ArrayTransport::new();
    let mailer = Mailer::new(Arc::new(transport.clone()), views(), "");

    let error = mailer
        .send_message(Message::new().to("ada@example.com").subject("x").text("x"))
        .await
        .expect_err("no sender");

    assert!(matches!(error, MailError::MissingSender), "{error}");
    transport.assert_nothing_sent();
}

#[tokio::test]
async fn send_accepts_a_message_as_a_mailable() {
    let (mailer, transport) = mailer();
    let message = Message::new()
        .to("ada@example.com")
        .subject("Raw")
        .text("raw");

    mailer.send(&message).await.expect("send");

    transport.assert_sent(|m| m.subject == "Raw" && m.from.as_deref() == Some(FROM));
}
