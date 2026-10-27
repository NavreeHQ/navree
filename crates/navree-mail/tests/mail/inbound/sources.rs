//! The source routes: raw MIME, Amazon SES (SNS and S3 actions), Mailgun
//! routes and Postmark inbound.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use navree_mail::inbound::{InboundStatus, StoredMail, Verdict};
use navree_mail::{InboundError, Incoming};
use navree_storage::Storage;
use serde_json::{Value, json};

use super::{fixture, inbound_app};

const MAILBOXES: &[&str] = &["reply+{token}@in.acme.test", "support@acme.test"];

// ---------------------------------------------------------------------
// Raw MIME
// ---------------------------------------------------------------------

#[tokio::test]
async fn raw_mime_with_the_bearer_token_is_received() {
    let mail = inbound_app(MAILBOXES, |b| b.config("mail.inbound.raw.token", "s3cret")).await;

    let response = mail
        .app
        .client()
        .post("/api/mail/inbound/raw")
        .bearer("s3cret")
        .header("content-type", "message/rfc822")
        .header("x-envelope-from", "bounces@example.com")
        .header("x-envelope-to", "support@acme.test, other@acme.test")
        .body(fixture("report.eml"))
        .send()
        .await
        .assert_status(200);

    let body = response.json_value();
    assert_eq!(body["status"], "handled");
    assert_eq!(body["mailbox"], "support@acme.test");
    let seen = mail.seen();
    let received = &seen[0].2;
    assert_eq!(received.source, "raw");
    assert_eq!(
        received.envelope.sender.as_deref(),
        Some("bounces@example.com")
    );
    assert_eq!(
        received.envelope.recipients,
        vec!["support@acme.test", "other@acme.test"]
    );
    assert_eq!(received.attachments.len(), 2);
}

#[tokio::test]
async fn raw_mime_needs_the_token() {
    let mail = inbound_app(MAILBOXES, |b| b.config("mail.inbound.raw.token", "s3cret")).await;

    for request in [
        mail.app.client().post("/api/mail/inbound/raw"),
        mail.app.client().post("/api/mail/inbound/raw").bearer("wrong"),
        mail.app
            .client()
            .post("/api/mail/inbound/raw?token=wrong"),
    ] {
        request
            .body(fixture("reply.eml"))
            .send()
            .await
            .assert_error_code(401, "invalid_signature");
    }
    mail.app
        .client()
        .post("/api/mail/inbound/raw?token=s3cret")
        .body(fixture("reply.eml"))
        .send()
        .await
        .assert_status(200);
    assert_eq!(mail.seen().len(), 1);
}

#[tokio::test]
async fn sources_without_a_secret_are_not_mounted() {
    let mail = inbound_app(MAILBOXES, |b| b).await;

    for path in [
        "/api/mail/inbound/raw",
        "/api/mail/inbound/ses",
        "/api/mail/inbound/mailgun",
        "/api/mail/inbound/postmark",
    ] {
        mail.app
            .client()
            .post(path)
            .body("x")
            .send()
            .await
            .assert_status(404);
    }
}

#[tokio::test]
async fn oversized_and_unreadable_messages_get_clear_errors() {
    let mail = inbound_app(MAILBOXES, |b| {
        b.config("mail.inbound.raw.token", "s3cret")
            .config("mail.inbound.max_size", 2048)
    })
    .await;

    let big = Incoming::compose()
        .from("a@b.test")
        .to("support@acme.test")
        .text("x".repeat(3000))
        .to_bytes();
    mail.app
        .client()
        .post("/api/mail/inbound/raw")
        .bearer("s3cret")
        .body(big)
        .send()
        .await
        .assert_error_code(413, "mail_too_large");

    mail.app
        .client()
        .post("/api/mail/inbound/raw")
        .bearer("s3cret")
        .body(Vec::new())
        .send()
        .await
        .assert_error_code(400, "invalid_inbound_mail");
}

#[tokio::test]
async fn a_failing_mailbox_answers_500_so_the_provider_retries() {
    struct Failing;
    impl navree_kernel::Module for Failing {
        fn register(&self, m: &mut navree_kernel::ModuleBuilder) {
            use navree_mail::ModuleBuilderMailExt;
            m.mailbox(
                "support@acme.test",
                |_mail: navree_mail::InboundMail, _ctx: navree_mail::MailboxContext| async move {
                    Err::<(), navree_kernel::BoxError>("down".into())
                },
            );
        }
    }
    let mail = inbound_app(&[], |b| {
        b.config("mail.inbound.raw.token", "s3cret").module(Failing)
    })
    .await;

    mail.app
        .client()
        .post("/api/mail/inbound/raw")
        .bearer("s3cret")
        .header("x-envelope-to", "support@acme.test")
        .body(fixture("reply.eml"))
        .send()
        .await
        .assert_status(500);
}

// ---------------------------------------------------------------------
// Amazon SES
// ---------------------------------------------------------------------

fn ses_app_config(b: navree_testing::TestAppBuilder) -> navree_testing::TestAppBuilder {
    b.config("mail.inbound.ses.token", "t0ken")
        .config(
            "mail.inbound.ses.topic_arns",
            vec!["arn:aws:sns:eu-west-1:123456789012:inbound-mail"],
        )
        .config("mail.inbound.ses.disk", "ses")
        .config("mail.inbound.ses.confirm_subscriptions", false)
}

#[tokio::test]
async fn ses_with_an_sns_action_delivers_the_message_in_the_notification() {
    let mail = inbound_app(MAILBOXES, ses_app_config).await;

    let response = mail
        .app
        .client()
        .post("/api/mail/inbound/ses?token=t0ken")
        .body(fixture("ses-sns-action.json"))
        .send()
        .await
        .assert_status(200);

    assert_eq!(response.json_value()["status"], "handled");
    let seen = mail.seen();
    let received = &seen[0].2;
    assert_eq!(seen[0].1, "reply+abc@in.acme.test");
    assert_eq!(received.source, "ses");
    assert_eq!(
        received.provider_id.as_deref(),
        Some("o3vrnil0e2ic28trm7dfhrc2v0clambda4nbp0g1")
    );
    assert_eq!(
        received.envelope.recipients,
        vec!["reply+abc@in.acme.test"],
        "the receipt rule's recipients, not every destination"
    );
    assert_eq!(received.envelope.sender.as_deref(), Some("ada@example.com"));
    assert_eq!(received.verdicts.spam, Some(Verdict::Pass));
    assert_eq!(received.verdicts.dkim, Some(Verdict::Gray));
    assert_eq!(received.verdicts.dmarc, Some(Verdict::Fail));
    assert_eq!(received.subject.as_deref(), Some("Re: [#42] Printer on fire"));
    assert_eq!(received.raw().as_ref(), fixture("reply.eml").as_slice());
}

#[tokio::test]
async fn ses_with_an_s3_action_reads_the_message_from_the_bucket_disk() {
    let mail = inbound_app(MAILBOXES, ses_app_config).await;
    let key = "incoming/o3vrnil0e2ic28trm7dfhrc2v0clambda4nbp0g1";
    mail.app
        .get::<Storage>()
        .await
        .disk("ses")
        .unwrap()
        .put(key, fixture("reply.eml"))
        .await
        .unwrap();

    let response = mail
        .app
        .client()
        .post("/api/mail/inbound/ses?token=t0ken")
        .body(fixture("ses-s3-action.json"))
        .send()
        .await
        .assert_status(200);

    let body = response.json_value();
    assert_eq!(body["stored"], json!({"disk": "ses", "path": key}));
    let received = &mail.seen()[0].2;
    assert_eq!(
        received.stored,
        Some(StoredMail {
            disk: "ses".into(),
            path: key.into()
        }),
        "the bucket keeps the raw message: it is not stored twice"
    );
    assert_eq!(received.message_id.as_deref(), Some("CAF2reply-0001@mail.example.com"));
}

#[tokio::test]
async fn ses_checks_the_token_and_the_topic() {
    let mail = inbound_app(MAILBOXES, ses_app_config).await;

    mail.app
        .client()
        .post("/api/mail/inbound/ses?token=nope")
        .body(fixture("ses-sns-action.json"))
        .send()
        .await
        .assert_error_code(401, "invalid_signature");

    let mut other_topic: Value = serde_json::from_slice(&fixture("ses-sns-action.json")).unwrap();
    other_topic["TopicArn"] = json!("arn:aws:sns:eu-west-1:123456789012:someone-else");
    mail.app
        .client()
        .post("/api/mail/inbound/ses?token=t0ken")
        .json(&other_topic)
        .send()
        .await
        .assert_error_code(401, "invalid_signature");
    assert!(mail.seen().is_empty());
}

#[tokio::test]
async fn ses_subscription_confirmations_receive_nothing() {
    let mail = inbound_app(MAILBOXES, ses_app_config).await;

    mail.app
        .client()
        .post("/api/mail/inbound/ses?token=t0ken")
        .json(&json!({
            "Type": "SubscriptionConfirmation",
            "TopicArn": "arn:aws:sns:eu-west-1:123456789012:inbound-mail",
            "SubscribeURL": "https://sns.eu-west-1.amazonaws.com/?Action=ConfirmSubscription&Token=x",
        }))
        .send()
        .await
        .assert_status(200);

    assert!(mail.seen().is_empty());
}

#[tokio::test]
async fn ses_notifications_of_other_kinds_are_refused() {
    let mail = inbound_app(MAILBOXES, ses_app_config).await;
    let mut envelope: Value = serde_json::from_slice(&fixture("ses-sns-action.json")).unwrap();
    envelope["Message"] = json!(json!({"notificationType": "Bounce"}).to_string());

    mail.app
        .client()
        .post("/api/mail/inbound/ses?token=t0ken")
        .json(&envelope)
        .send()
        .await
        .assert_error_code(400, "invalid_inbound_mail");
}

#[tokio::test]
async fn receive_ses_serves_routes_that_verify_sns_themselves() {
    let mail = inbound_app(MAILBOXES, |b| b).await;
    let envelope: Value = serde_json::from_slice(&fixture("ses-sns-action.json")).unwrap();
    let notification: Value =
        serde_json::from_str(envelope["Message"].as_str().unwrap()).unwrap();

    let received = mail
        .inbound()
        .await
        .receive_ses(&notification)
        .await
        .unwrap();
    assert_eq!(received.status, InboundStatus::Handled);

    let mut s3: Value = serde_json::from_slice(&fixture("ses-s3-action.json")).unwrap();
    let s3: Value = serde_json::from_str(s3["Message"].take().as_str().unwrap()).unwrap();
    let error = mail.inbound().await.receive_ses(&s3).await.unwrap_err();
    assert!(
        matches!(&error, InboundError::Config(message) if message.contains("mail.inbound.ses.disk")),
        "{error}"
    );
}

// ---------------------------------------------------------------------
// Mailgun
// ---------------------------------------------------------------------

const MAILGUN_KEY: &str = "key-mailgun-signing";

fn signature(timestamp: &str, token: &str) -> String {
    hex::encode(navree_crypto::hmac_sha256(
        MAILGUN_KEY.as_bytes(),
        format!("{timestamp}{token}").as_bytes(),
    ))
}

fn signed_fields(timestamp: i64) -> Vec<(String, String)> {
    let timestamp = timestamp.to_string();
    let token = "0123456789abcdef0123456789abcdef0123456789abcdef50".to_owned();
    vec![
        ("signature".into(), signature(&timestamp, &token)),
        ("timestamp".into(), timestamp),
        ("token".into(), token),
    ]
}

#[tokio::test]
async fn a_mailgun_mime_route_delivers_the_whole_message() {
    let mail = inbound_app(MAILBOXES, |b| {
        b.config("mail.inbound.mailgun.signing_key", MAILGUN_KEY)
    })
    .await;
    let mut form = signed_fields(chrono::Utc::now().timestamp());
    form.extend([
        ("recipient".into(), "reply+abc@in.acme.test".into()),
        ("sender".into(), "ada@example.com".into()),
        ("X-Mailgun-Sflag".into(), "No".into()),
        ("X-Mailgun-Spf".into(), "Pass".into()),
        (
            "body-mime".into(),
            String::from_utf8(fixture("reply.eml")).unwrap(),
        ),
    ]);

    mail.app
        .client()
        .post("/api/mail/inbound/mailgun/mime")
        .form(&form)
        .send()
        .await
        .assert_status(200);

    let received = &mail.seen()[0].2;
    assert_eq!(received.source, "mailgun");
    assert_eq!(received.envelope.recipients, vec!["reply+abc@in.acme.test"]);
    assert_eq!(received.verdicts.spam, Some(Verdict::Pass));
    assert_eq!(received.verdicts.spf, Some(Verdict::Pass));
    assert_eq!(received.raw().as_ref(), fixture("reply.eml").as_slice());
}

/// A multipart body with `fields` and one file.
fn multipart(fields: &[(String, String)], file: (&str, &str, &str, &[u8])) -> (String, Vec<u8>) {
    let boundary = "----navree-test-boundary";
    let mut body = Vec::new();
    for (name, value) in fields {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    }
    let (field, filename, content_type, content) = file;
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"{field}\"; filename=\"{filename}\"\r\nContent-Type: {content_type}\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(content);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

#[tokio::test]
async fn a_mailgun_route_with_parsed_fields_is_rebuilt() {
    let mail = inbound_app(MAILBOXES, |b| {
        b.config("mail.webhooks.mailgun.signing_key", MAILGUN_KEY)
    })
    .await;
    let mut fields = signed_fields(chrono::Utc::now().timestamp());
    fields.extend([
        ("recipient".to_owned(), "support@acme.test".to_owned()),
        ("sender".to_owned(), "grace@example.com".to_owned()),
        ("from".to_owned(), "Grace Hopper <grace@example.com>".to_owned()),
        ("subject".to_owned(), "Quarterly report".to_owned()),
        ("body-plain".to_owned(), "The report is attached.".to_owned()),
        ("body-html".to_owned(), "<p>The report is attached.</p>".to_owned()),
        ("X-Mailgun-Sflag".to_owned(), "Yes".to_owned()),
        (
            "message-headers".to_owned(),
            json!([
                ["Received", "from mx.example.com by mxa.mailgun.org"],
                ["Message-Id", "<multi-0002@example.com>"],
                ["To", "support@acme.test"],
                ["Content-Type", "multipart/mixed; boundary=\"old\""],
            ])
            .to_string(),
        ),
        ("attachment-count".to_owned(), "1".to_owned()),
    ]);
    let (content_type, body) = multipart(
        &fields,
        ("attachment-1", "q3.pdf", "application/pdf", b"%PDF-1.4"),
    );

    mail.app
        .client()
        .post("/api/mail/inbound/mailgun")
        .header("content-type", content_type)
        .body(body)
        .send()
        .await
        .assert_status(200);

    let seen = mail.seen();
    let received = &seen[0].2;
    assert_eq!(seen[0].0, "support@acme.test");
    assert_eq!(received.source, "mailgun");
    assert_eq!(received.provider_id.as_deref(), Some("multi-0002@example.com"));
    assert_eq!(received.message_id.as_deref(), Some("multi-0002@example.com"));
    assert_eq!(
        received.from.as_ref().and_then(|a| a.name.as_deref()),
        Some("Grace Hopper")
    );
    assert_eq!(received.subject.as_deref(), Some("Quarterly report"));
    assert_eq!(
        received.header("Received"),
        Some("from mx.example.com by mxa.mailgun.org")
    );
    assert_eq!(received.text.as_deref().map(str::trim), Some("The report is attached."));
    assert_eq!(
        received.html.as_deref().map(str::trim),
        Some("<p>The report is attached.</p>")
    );
    assert_eq!(received.attachments[0].filename.as_deref(), Some("q3.pdf"));
    assert_eq!(received.attachments[0].content.as_ref(), b"%PDF-1.4");
    assert_eq!(received.verdicts.spam, Some(Verdict::Fail));
}

#[tokio::test]
async fn mailgun_signatures_are_checked() {
    let mail = inbound_app(MAILBOXES, |b| {
        b.config("mail.inbound.mailgun.signing_key", MAILGUN_KEY)
    })
    .await;
    let body_mime = (
        "body-mime".to_owned(),
        String::from_utf8(fixture("reply.eml")).unwrap(),
    );

    let mut forged = signed_fields(chrono::Utc::now().timestamp());
    forged[0].1 = "0".repeat(64);
    forged.push(body_mime.clone());
    let mut stale = signed_fields(chrono::Utc::now().timestamp() - 3600);
    stale.push(body_mime.clone());
    let unsigned = vec![body_mime];

    for form in [forged, stale, unsigned] {
        mail.app
            .client()
            .post("/api/mail/inbound/mailgun/mime")
            .form(&form)
            .send()
            .await
            .assert_error_code(401, "invalid_signature");
    }
    assert!(mail.seen().is_empty());
}

// ---------------------------------------------------------------------
// Postmark
// ---------------------------------------------------------------------

fn basic(password: &str) -> String {
    format!("Basic {}", STANDARD.encode(format!("postmark:{password}")))
}

#[tokio::test]
async fn postmark_with_the_raw_email_delivers_it_as_sent() {
    let mail = inbound_app(MAILBOXES, |b| b.config("mail.inbound.postmark.token", "pm-secret")).await;

    mail.app
        .client()
        .post("/api/mail/inbound/postmark")
        .header("authorization", basic("pm-secret"))
        .body(fixture("postmark-raw.json"))
        .send()
        .await
        .assert_status(200);

    let received = &mail.seen()[0].2;
    assert_eq!(received.source, "postmark");
    assert_eq!(
        received.provider_id.as_deref(),
        Some("73e6d360-66eb-11e1-8e72-a8904824019b")
    );
    assert_eq!(received.raw().as_ref(), fixture("reply.eml").as_slice());
    assert_eq!(received.envelope.recipients, vec!["reply+abc@in.acme.test"]);
}

#[tokio::test]
async fn postmark_without_the_raw_email_is_rebuilt_from_its_fields() {
    let mail = inbound_app(MAILBOXES, |b| b.config("mail.inbound.postmark.token", "pm-secret")).await;

    mail.app
        .client()
        .post("/api/mail/inbound/postmark?token=pm-secret")
        .body(fixture("postmark.json"))
        .send()
        .await
        .assert_status(200);

    let seen = mail.seen();
    let received = &seen[0].2;
    assert_eq!(seen[0].1, "reply+abc@in.acme.test");
    assert_eq!(
        received.from.as_ref().map(ToString::to_string).as_deref(),
        Some("Ada Lovelace <ada@example.com>")
    );
    assert_eq!(received.to[0].email, "reply+abc@in.acme.test");
    assert_eq!(received.cc[0].email, "grace@example.com");
    assert_eq!(received.subject.as_deref(), Some("Re: [#42] Printer on fire"));
    assert_eq!(
        received.message_id.as_deref(),
        Some("CAF2reply-0001@mail.example.com")
    );
    assert_eq!(received.in_reply_to.as_deref(), Some("ticket-42@acme.test"));
    assert_eq!(received.reply_text().as_deref(), Some("Thanks, the fire is out now."));
    assert_eq!(
        received.html.as_deref().map(str::trim),
        Some("<p>Thanks, the fire is out now.</p>")
    );
    assert_eq!(received.attachments[0].filename.as_deref(), Some("notes.txt"));
    assert_eq!(received.attachments[0].content.as_ref(), b"fire: out\n");
    assert_eq!(received.verdicts.spam, Some(Verdict::Pass));
    assert_eq!(received.verdicts.spf, Some(Verdict::Pass));
    assert_eq!(
        received.date.map(|d| d.to_rfc3339()).as_deref(),
        Some("2026-10-27T09:14:00+00:00")
    );
}

#[tokio::test]
async fn postmark_needs_the_password() {
    let mail = inbound_app(MAILBOXES, |b| b.config("mail.inbound.postmark.token", "pm-secret")).await;

    mail.app
        .client()
        .post("/api/mail/inbound/postmark")
        .header("authorization", basic("wrong"))
        .body(fixture("postmark.json"))
        .send()
        .await
        .assert_error_code(401, "invalid_signature");
    mail.app
        .client()
        .post("/api/mail/inbound/postmark?token=pm-secret")
        .json(&json!({"not": "postmark"}))
        .send()
        .await
        .assert_error_code(400, "invalid_inbound_mail");
}
