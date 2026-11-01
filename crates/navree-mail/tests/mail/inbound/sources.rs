//! The source routes: raw MIME, Amazon SES (SNS and S3 actions), Mailgun
//! routes and Postmark inbound.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use navree_mail::inbound::{InboundStatus, StoredMail, Verdict, Verdicts};
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
        mail.app
            .client()
            .post("/api/mail/inbound/raw")
            .bearer("wrong"),
        mail.app.client().post("/api/mail/inbound/raw?token=wrong"),
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
    assert_eq!(
        received.subject.as_deref(),
        Some("Re: [#42] Printer on fire")
    );
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
    assert_eq!(
        received.message_id.as_deref(),
        Some("CAF2reply-0001@mail.example.com")
    );
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
    let notification: Value = serde_json::from_str(envelope["Message"].as_str().unwrap()).unwrap();

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
    assert_eq!(
        received.verdicts,
        Verdicts::default(),
        "Mailgun's spam flag and SPF result are not read: nothing tells them from the sender's"
    );
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

/// Mailgun sends the whole message as a text field of the form, so a field
/// may be as long as a message, well past a form's usual limit.
#[tokio::test]
async fn a_mailgun_mime_route_takes_a_message_longer_than_a_form_field() {
    let mail = inbound_app(MAILBOXES, |b| {
        b.config("mail.inbound.mailgun.signing_key", MAILGUN_KEY)
    })
    .await;
    let mut message = String::from_utf8(fixture("reply.eml")).unwrap();
    for _ in 0..30_000 {
        message.push_str(&"x".repeat(76));
        message.push_str("\r\n");
    }
    assert!(message.len() > 2 * 1024 * 1024);
    let mut fields = signed_fields(chrono::Utc::now().timestamp());
    fields.extend([
        ("recipient".to_owned(), "reply+abc@in.acme.test".to_owned()),
        ("sender".to_owned(), "ada@example.com".to_owned()),
        ("body-mime".to_owned(), message.clone()),
    ]);
    let (content_type, body) = multipart(
        &fields,
        ("attachment-1", "a.txt", "text/plain", b"attached"),
    );

    mail.app
        .client()
        .post("/api/mail/inbound/mailgun/mime")
        .header("content-type", content_type)
        .body(body)
        .send()
        .await
        .assert_status(200);

    assert_eq!(mail.seen()[0].2.raw().as_ref(), message.as_bytes());
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
        (
            "from".to_owned(),
            "Grace Hopper <grace@example.com>".to_owned(),
        ),
        ("subject".to_owned(), "Quarterly report".to_owned()),
        (
            "body-plain".to_owned(),
            "The report is attached.".to_owned(),
        ),
        (
            "body-html".to_owned(),
            "<p>The report is attached.</p>".to_owned(),
        ),
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
    assert_eq!(
        received.provider_id.as_deref(),
        Some("multi-0002@example.com")
    );
    assert_eq!(
        received.message_id.as_deref(),
        Some("multi-0002@example.com")
    );
    assert_eq!(
        received.from.as_ref().and_then(|a| a.name.as_deref()),
        Some("Grace Hopper")
    );
    assert_eq!(received.subject.as_deref(), Some("Quarterly report"));
    assert_eq!(
        received.header("Received"),
        Some("from mx.example.com by mxa.mailgun.org")
    );
    assert_eq!(
        received.text.as_deref().map(str::trim),
        Some("The report is attached.")
    );
    assert_eq!(
        received.html.as_deref().map(str::trim),
        Some("<p>The report is attached.</p>")
    );
    assert_eq!(received.attachments[0].filename.as_deref(), Some("q3.pdf"));
    assert_eq!(received.attachments[0].content.as_ref(), b"%PDF-1.4");
    assert_eq!(received.verdicts, Verdicts::default());
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

/// The fields Mailgun documents for a parsed route, signed.
fn documented_fields(headers: Value) -> Vec<(String, String)> {
    let mut form = signed_fields(chrono::Utc::now().timestamp());
    form.extend([
        ("recipient".to_owned(), "reply+abc@in.acme.test".to_owned()),
        ("sender".to_owned(), "a@evil.test".to_owned()),
        ("from".to_owned(), "a@evil.test".to_owned()),
        ("subject".to_owned(), "s".to_owned()),
        ("body-plain".to_owned(), "b".to_owned()),
        ("message-headers".to_owned(), headers.to_string()),
    ]);
    form
}

/// Mailgun's documented fields hold no verdict, and its docs say nothing
/// that tells its `X-Mailgun-*` headers from a sender's own copies, so a
/// sender who writes them gets no verdict.
#[tokio::test]
async fn mailgun_reports_no_verdict_the_sender_could_have_written() {
    let mail = inbound_app(MAILBOXES, |b| {
        b.config("mail.inbound.mailgun.signing_key", MAILGUN_KEY)
    })
    .await;
    let forged = [
        ("X-Mailgun-Sflag", "No"),
        ("X-Mailgun-Spf", "Pass"),
        ("X-Mailgun-Dkim-Check-Result", "Pass"),
    ];

    // The parsed route: the sender's headers are in `message-headers`.
    let headers: Vec<Value> = std::iter::once(json!(["From", "a@evil.test"]))
        .chain(forged.iter().map(|(name, value)| json!([name, value])))
        .collect();
    mail.app
        .client()
        .post("/api/mail/inbound/mailgun")
        .form(&documented_fields(Value::Array(headers)))
        .send()
        .await
        .assert_status(200);

    // Headers posted as form fields of their own, in any case.
    let mut flat = documented_fields(json!([["From", "a@evil.test"]]));
    flat.extend(forged.iter().map(|(n, v)| (n.to_string(), v.to_string())));
    flat.extend(
        forged
            .iter()
            .map(|(n, v)| (n.to_lowercase(), v.to_string())),
    );
    mail.app
        .client()
        .post("/api/mail/inbound/mailgun")
        .form(&flat)
        .send()
        .await
        .assert_status(200);

    // The whole message: the sender's headers are in `body-mime`.
    let raw = "From: a@evil.test\r\nTo: reply+abc@in.acme.test\r\n\
               X-Mailgun-Sflag: No\r\nX-Mailgun-Spf: Pass\r\n\
               X-Mailgun-Dkim-Check-Result: Pass\r\n\r\nb\r\n";
    let mut mime = signed_fields(chrono::Utc::now().timestamp());
    mime.extend([
        ("recipient".to_owned(), "reply+abc@in.acme.test".to_owned()),
        ("body-mime".to_owned(), raw.to_owned()),
    ]);
    mail.app
        .client()
        .post("/api/mail/inbound/mailgun/mime")
        .form(&mime)
        .send()
        .await
        .assert_status(200);

    let seen = mail.seen();
    assert_eq!(seen.len(), 3);
    for (_, _, received) in &seen {
        assert_eq!(
            received.verdicts,
            Verdicts::default(),
            "a verdict was read from what the sender wrote"
        );
    }
}

/// A form field is the one named exactly like a documented field. A name in
/// another case is a header Mailgun may post for the sender, and never the
/// envelope.
#[tokio::test]
async fn a_mailgun_field_in_another_case_is_not_the_envelope() {
    let mail = inbound_app(MAILBOXES, |b| {
        b.config("mail.inbound.mailgun.signing_key", MAILGUN_KEY)
    })
    .await;
    let mut form = vec![
        ("Recipient".to_owned(), "support@acme.test".to_owned()),
        ("Sender".to_owned(), "ceo@acme.test".to_owned()),
        ("From".to_owned(), "ceo@acme.test".to_owned()),
        ("Subject".to_owned(), "forged".to_owned()),
        ("Body-Plain".to_owned(), "forged".to_owned()),
    ];
    form.extend(documented_fields(json!([["From", "a@evil.test"]])));

    mail.app
        .client()
        .post("/api/mail/inbound/mailgun")
        .form(&form)
        .send()
        .await
        .assert_status(200);

    let seen = mail.seen();
    assert_eq!(
        seen[0].0, "reply+{token}@in.acme.test",
        "routed by `recipient`"
    );
    let received = &seen[0].2;
    assert_eq!(received.envelope.sender.as_deref(), Some("a@evil.test"));
    assert_eq!(received.envelope.recipients, vec!["reply+abc@in.acme.test"]);
    assert_eq!(received.subject.as_deref(), Some("s"));
    assert_eq!(received.text.as_deref().map(str::trim), Some("b"));
}

/// The signature check reads `signature`, `timestamp` and `token` by exact
/// name too: a header called `Token` can't change what is verified.
#[tokio::test]
async fn mailgun_signature_fields_are_found_by_exact_name() {
    let mail = inbound_app(MAILBOXES, |b| {
        b.config("mail.inbound.mailgun.signing_key", MAILGUN_KEY)
    })
    .await;
    let mut form = vec![
        ("Token".to_owned(), "x".repeat(50)),
        ("Signature".to_owned(), "0".repeat(64)),
        ("Timestamp".to_owned(), "1".to_owned()),
    ];
    form.extend(documented_fields(json!([["From", "a@evil.test"]])));

    mail.app
        .client()
        .post("/api/mail/inbound/mailgun")
        .form(&form)
        .send()
        .await
        .assert_status(200);
    assert_eq!(mail.seen().len(), 1);
}

/// A name the reader depends on that the form writes twice can't be told
/// apart from a header posted under that name: the form is refused, and a
/// repeated signature field finds no value.
#[tokio::test]
async fn a_mailgun_form_that_repeats_a_field_it_reads_is_refused() {
    let mail = inbound_app(MAILBOXES, |b| {
        b.config("mail.inbound.mailgun.signing_key", MAILGUN_KEY)
    })
    .await;
    for name in ["recipient", "sender", "from", "subject", "body-plain"] {
        let mut form = documented_fields(json!([["From", "a@evil.test"]]));
        form.push((name.to_owned(), "support@acme.test".to_owned()));
        mail.app
            .client()
            .post("/api/mail/inbound/mailgun")
            .form(&form)
            .send()
            .await
            .assert_error_code(400, "invalid_inbound_mail");
    }
    for name in ["timestamp", "token", "signature"] {
        let mut form = documented_fields(json!([["From", "a@evil.test"]]));
        let value = form.iter().find(|(n, _)| n == name).unwrap().1.clone();
        form.push((name.to_owned(), value));
        mail.app
            .client()
            .post("/api/mail/inbound/mailgun")
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
    let mail = inbound_app(MAILBOXES, |b| {
        b.config("mail.inbound.postmark.token", "pm-secret")
    })
    .await;

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
    let mail = inbound_app(MAILBOXES, |b| {
        b.config("mail.inbound.postmark.token", "pm-secret")
    })
    .await;

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
    assert_eq!(
        received.subject.as_deref(),
        Some("Re: [#42] Printer on fire")
    );
    assert_eq!(
        received.message_id.as_deref(),
        Some("CAF2reply-0001@mail.example.com")
    );
    assert_eq!(received.in_reply_to.as_deref(), Some("ticket-42@acme.test"));
    assert_eq!(
        received.reply_text().as_deref(),
        Some("Thanks, the fire is out now.")
    );
    assert_eq!(
        received.html.as_deref().map(str::trim),
        Some("<p>Thanks, the fire is out now.</p>")
    );
    assert_eq!(
        received.attachments[0].filename.as_deref(),
        Some("notes.txt")
    );
    assert_eq!(received.attachments[0].content.as_ref(), b"fire: out\n");
    assert_eq!(
        received.verdicts,
        Verdicts::default(),
        "the Headers list holds the sender's headers too: no verdict is read from it"
    );
    assert_eq!(
        received.date.map(|d| d.to_rfc3339()).as_deref(),
        Some("2026-10-27T09:14:00+00:00")
    );
}

#[tokio::test]
async fn postmark_needs_the_password() {
    let mail = inbound_app(MAILBOXES, |b| {
        b.config("mail.inbound.postmark.token", "pm-secret")
    })
    .await;

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

/// The sample payload with `change` applied to its top-level fields.
fn postmark_payload(change: impl FnOnce(&mut serde_json::Map<String, Value>)) -> Value {
    let mut payload: Value = serde_json::from_slice(&fixture("postmark.json")).unwrap();
    change(payload.as_object_mut().unwrap());
    payload
}

#[tokio::test]
async fn postmark_reports_no_verdict_from_its_headers_list() {
    let mail = inbound_app(MAILBOXES, |b| {
        b.config("mail.inbound.postmark.token", "pm-secret")
    })
    .await;
    // The list holds the message's own headers beside Postmark's, and the
    // docs don't say how to tell them apart.
    let payload = postmark_payload(|p| {
        p["Headers"] = json!([
            {"Name": "X-Spam-Status", "Value": "No"},
            {"Name": "Received-SPF", "Value": "pass (the sender wrote this)"},
        ]);
    });

    mail.app
        .client()
        .post("/api/mail/inbound/postmark?token=pm-secret")
        .json(&payload)
        .send()
        .await
        .assert_status(200);

    assert_eq!(
        mail.seen()[0].2.verdicts,
        Verdicts::default(),
        "a verdict was read from a header list the sender writes"
    );
}

/// Postmark reports the recipient the message was sent to and nothing about
/// `MAIL FROM`: the envelope holds no `From` or `To` the sender wrote.
#[tokio::test]
async fn postmark_envelope_holds_only_what_postmark_reports() {
    let mail = inbound_app(MAILBOXES, |b| {
        b.config("mail.inbound.postmark.token", "pm-secret")
    })
    .await;

    // Two `From` fields: `mail.from` is `None`, and so is the envelope sender.
    let raw = "From: ceo@acme.test\r\nFrom: a@evil.test\r\nTo: support@acme.test\r\n\r\nb\r\n";
    let two_from = postmark_payload(|p| {
        p.insert("RawEmail".to_owned(), json!(raw));
        p["From"] = json!("ceo@acme.test");
        p["FromFull"] = json!({"Email": "ceo@acme.test", "Name": "", "MailboxHash": ""});
    });
    // No `OriginalRecipient`: no envelope recipients, whatever `ToFull` says.
    let listed = json!([{"Email": "support@acme.test", "Name": "", "MailboxHash": ""}]);
    let no_original = postmark_payload(|p| {
        p.remove("OriginalRecipient");
        p["ToFull"] = listed.clone();
        p["CcFull"] = listed.clone();
        p["BccFull"] = listed.clone();
    });
    for payload in [&two_from, &no_original] {
        mail.app
            .client()
            .post("/api/mail/inbound/postmark?token=pm-secret")
            .json(payload)
            .send()
            .await
            .assert_status(200);
    }

    let seen = mail.seen();
    assert_eq!(seen[0].2.from, None, "two From fields");
    assert_eq!(seen[0].2.envelope.sender, None);
    assert_eq!(
        seen[0].2.envelope.recipients,
        vec!["reply+abc@in.acme.test"]
    );
    assert_eq!(seen[1].2.envelope.sender, None);
    assert!(seen[1].2.envelope.recipients.is_empty());
}
