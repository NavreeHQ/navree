//! `Message` headers, metadata, locale and hold; `Mailable::hold` and
//! `TemplateMail`'s locale.

use std::sync::Arc;

use navree_mail::{ArrayTransport, MailError, Mailable, Mailer, Message, TemplateMail};
use navree_view::Views;
use serde_json::json;

fn base() -> Message {
    Message::new()
        .from("App <app@example.com>")
        .to("ada@example.com")
        .subject("Hello")
        .text("Hi")
}

#[test]
fn extra_headers_are_checked() {
    base()
        .header("List-Unsubscribe", "<mailto:unsubscribe@example.com>")
        .header("X-Campaign", "spring")
        .validate()
        .expect("valid headers");

    for (name, value) in [
        ("Subject", "Injected"),
        ("content-type", "text/plain"),
        ("Bcc", "eve@example.com"),
        ("DKIM-Signature", "v=1"),
    ] {
        let error = base().header(name, value).validate().unwrap_err();
        assert!(
            matches!(&error, MailError::Header { name: n, reason } if n == name && reason.contains("transport writes")),
            "{name}: {error}"
        );
    }

    let error = base()
        .header("X-Note", "one\r\nBcc: eve@example.com")
        .validate()
        .unwrap_err();
    assert!(matches!(error, MailError::Header { reason, .. } if reason.contains("line breaks")));

    for bad in ["X Note", "X:Note", ""] {
        let error = base().header(bad, "v").validate().unwrap_err();
        assert!(matches!(error, MailError::Header { .. }), "{bad:?}");
    }
}

#[test]
fn recipients_and_sender_domain() {
    let message = Message::new()
        .from("Acme <Hello@ACME.test>")
        .to("ada@example.com")
        .cc("bob@example.com")
        .bcc("log@example.com");
    assert_eq!(
        message.recipients().collect::<Vec<_>>(),
        ["ada@example.com", "bob@example.com", "log@example.com"]
    );
    assert_eq!(message.sender_domain().as_deref(), Some("acme.test"));
    assert_eq!(Message::new().sender_domain(), None);
}

#[test]
fn new_fields_are_left_out_of_the_json_when_unused() {
    let json = serde_json::to_value(base()).expect("json");
    for key in ["headers", "metadata", "locale", "hold"] {
        assert!(json.get(key).is_none(), "{key} in {json}");
    }

    let full = base()
        .header("X-Campaign", "spring")
        .metadata("campaign", "spring")
        .locale("ar")
        .hold("Needs approval");
    let json = serde_json::to_value(&full).expect("json");
    assert_eq!(json["headers"], json!({ "X-Campaign": "spring" }));
    assert_eq!(json["metadata"], json!({ "campaign": "spring" }));
    assert_eq!(json["locale"], "ar");
    assert_eq!(json["hold"], "Needs approval");
    let back: Message = serde_json::from_value(json).expect("back");
    assert_eq!(back, full);
}

#[test]
fn template_mail_takes_its_locale_from_the_context_and_names_its_template() {
    let views = Views::from_templates([("mail/hello.html", "<p>{{ name }}</p>")]).expect("views");
    let mail = TemplateMail::new("ada@example.com", "Hi", "mail/hello")
        .context(json!({ "name": "Ada", "locale": "ar-EG" }));

    let message = mail.build(&views).expect("build");
    assert_eq!(message.locale.as_deref(), Some("ar-EG"));
    assert_eq!(mail.template(), Some("mail/hello"));

    let without = TemplateMail::new("ada@example.com", "Hi", "mail/hello")
        .context(json!({ "name": "Ada" }))
        .build(&views)
        .expect("build");
    assert_eq!(without.locale, None);
}

#[test]
fn hold_wraps_a_mailable() {
    let views = Views::from_templates([("mail/hello.txt", "Hi")]).expect("views");
    let held = TemplateMail::new("all@example.com", "Results", "mail/hello").hold("Big send");

    assert_eq!(held.reason(), "Big send");
    assert_eq!(held.template(), Some("mail/hello"));
    let message = held.build(&views).expect("build");
    assert_eq!(message.hold.as_deref(), Some("Big send"));
    assert_eq!(held.into_inner().subject, "Results");
}

#[tokio::test]
async fn holding_without_the_delivery_pipeline_is_an_error() {
    let transport = ArrayTransport::new();
    let views = Arc::new(Views::from_templates([("x.txt", "x")]).expect("views"));
    let mailer = Mailer::new(Arc::new(transport.clone()), views, "app@example.com");

    let error = mailer
        .send(&base().hold("Needs approval"))
        .await
        .unwrap_err();

    assert!(
        matches!(error, MailError::Missing(message) if message.contains("mail.delivery.enabled"))
    );
    transport.assert_nothing_sent();
}
