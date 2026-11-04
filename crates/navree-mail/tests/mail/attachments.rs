//! Outbound attachments on every API transport (CONN-102): SES as raw MIME,
//! Mailgun as a multipart form, Postmark and Resend as base64 JSON, each
//! within its provider's size limit; and Resend's idempotency key
//! (CONN-103). SMTP's MIME is checked in `smtp.rs` against a local server.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use navree_contracts::Secret;
use navree_mail::{
    Attachment, MAILGUN_MAX_MESSAGE_BYTES, MailError, MailTransport, MailgunConfig,
    MailgunTransport, Message, POSTMARK_MAX_MESSAGE_BYTES, PostmarkConfig, PostmarkTransport,
    RESEND_MAX_MESSAGE_BYTES, ResendConfig, ResendTransport, SES_MAX_MESSAGE_BYTES, SendMailJob,
    SesConfig, SesTransport,
};
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

/// A small PDF-like file.
const PDF: &[u8] = b"%PDF-1.7 INV-2026-0042";
/// A CSV with non-ASCII text.
const CSV: &[u8] = "city,rate\nLudhiana,18\nਅੰਮ੍ਰਿਤਸਰ,12\n".as_bytes();

fn invoice() -> Message {
    Message::new()
        .from("Grewal Traders <billing@grewal.example.in>")
        .to("Harpreet Kaur <harpreet@example.in>")
        .bcc("audit@grewal.example.in")
        .subject("Invoice INV-2026-0042")
        .text("Your invoice is attached.")
        .html("<p>Your invoice is attached.</p>")
        .header("X-Campaign", "invoices")
        .attach("INV-2026-0042.pdf", "application/pdf", PDF.to_vec())
        .attach("rates.csv", "text/csv; charset=utf-8", CSV.to_vec())
}

async fn server_answering(route: &str, body: Value) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(route))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;
    server
}

async fn only_request(server: &MockServer) -> Request {
    let mut requests = server.received_requests().await.expect("recording");
    assert_eq!(requests.len(), 1, "one request");
    requests.remove(0)
}

fn ses(server: &MockServer) -> SesTransport {
    SesTransport::new(SesConfig {
        region: "ap-south-1".to_owned(),
        access_key_id: "AKIDEXAMPLE".to_owned(),
        secret_access_key: Secret::new("wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".to_owned()),
        endpoint: Some(server.uri()),
        ..SesConfig::default()
    })
    .expect("ses")
}

fn mailgun(server: &MockServer) -> MailgunTransport {
    MailgunTransport::new(MailgunConfig {
        domain: "mg.grewal.example.in".to_owned(),
        api_key: Secret::new("key-3ax6xnjp29jd6fds4gc373sgvjxteol0".to_owned()),
        endpoint: Some(server.uri()),
        ..MailgunConfig::default()
    })
    .expect("mailgun")
}

fn postmark(server: &MockServer) -> PostmarkTransport {
    PostmarkTransport::new(PostmarkConfig {
        server_token: Secret::new("server-token".to_owned()),
        endpoint: Some(server.uri()),
        ..PostmarkConfig::default()
    })
    .expect("postmark")
}

fn resend(server: &MockServer) -> ResendTransport {
    ResendTransport::new(ResendConfig {
        api_key: Secret::new("re_123".to_owned()),
        endpoint: Some(server.uri()),
    })
    .expect("resend")
}

// CONN-102
#[tokio::test]
async fn ses_sends_a_message_with_attachments_as_raw_mime() {
    let server = server_answering(
        "/v2/email/outbound-emails",
        json!({ "MessageId": "0109-raw" }),
    )
    .await;

    let sent = ses(&server).deliver(&invoice()).await.expect("deliver");

    assert_eq!(sent.provider_id.as_deref(), Some("0109-raw"));
    let request = only_request(&server).await;
    let body: Value = serde_json::from_slice(&request.body).unwrap();
    assert!(body["Content"]["Simple"].is_null(), "{body}");
    assert_eq!(
        body["Destination"]["BccAddresses"],
        json!(["audit@grewal.example.in"])
    );
    let raw = STANDARD
        .decode(body["Content"]["Raw"]["Data"].as_str().expect("raw data"))
        .expect("base64");
    let raw = String::from_utf8_lossy(&raw);
    for expected in [
        "Subject: Invoice INV-2026-0042",
        "multipart/mixed",
        "multipart/alternative",
        "X-Campaign: invoices",
        "INV-2026-0042.pdf",
        "application/pdf",
        &STANDARD.encode(PDF),
    ] {
        assert!(raw.contains(expected), "missing {expected} in:\n{raw}");
    }
    assert!(
        !raw.contains("audit@grewal.example.in"),
        "Bcc leaked:\n{raw}"
    );
}

// CONN-102
#[tokio::test]
async fn ses_without_attachments_still_sends_simple_content() {
    let server = server_answering(
        "/v2/email/outbound-emails",
        json!({ "MessageId": "0109-simple" }),
    )
    .await;
    let mut message = invoice();
    message.attachments.clear();

    ses(&server).deliver(&message).await.expect("deliver");

    let body: Value = serde_json::from_slice(&only_request(&server).await.body).unwrap();
    assert!(body["Content"]["Raw"].is_null());
    assert_eq!(
        body["Content"]["Simple"]["Subject"]["Data"],
        "Invoice INV-2026-0042"
    );
}

// CONN-102
#[tokio::test]
async fn mailgun_posts_attachments_as_a_multipart_form() {
    let server = server_answering(
        "/v3/mg.grewal.example.in/messages",
        json!({ "id": "<20261105.1@mg.grewal.example.in>", "message": "Queued. Thank you." }),
    )
    .await;

    let sent = mailgun(&server).deliver(&invoice()).await.expect("deliver");

    assert_eq!(
        sent.provider_id.as_deref(),
        Some("20261105.1@mg.grewal.example.in")
    );
    let request = only_request(&server).await;
    let content_type = request
        .headers
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let boundary = content_type
        .strip_prefix("multipart/form-data; boundary=")
        .unwrap_or_else(|| panic!("{content_type}"));
    let body = request.body.as_slice();
    let text = String::from_utf8_lossy(body);
    for expected in [
        format!("--{boundary}\r\nContent-Disposition: form-data; name=\"subject\"\r\n\r\nInvoice INV-2026-0042\r\n"),
        "name=\"to\"\r\n\r\nHarpreet Kaur <harpreet@example.in>\r\n".to_owned(),
        "name=\"h:X-Campaign\"\r\n\r\ninvoices\r\n".to_owned(),
        "Content-Disposition: form-data; name=\"attachment\"; filename=\"INV-2026-0042.pdf\"\r\nContent-Type: application/pdf\r\n\r\n%PDF-1.7 INV-2026-0042\r\n".to_owned(),
        "filename=\"rates.csv\"\r\nContent-Type: text/csv; charset=utf-8\r\n\r\n".to_owned(),
        format!("--{boundary}--\r\n"),
    ] {
        assert!(text.contains(&expected), "missing {expected:?} in:\n{text}");
    }
    // The CSV's UTF-8 bytes go as they are.
    assert!(body.windows(CSV.len()).any(|window| window == CSV));
}

// CONN-102
#[tokio::test]
async fn postmark_sends_attachments_as_base64() {
    let server = server_answering(
        "/email",
        json!({ "ErrorCode": 0, "Message": "OK", "MessageID": "b7bc2f4a" }),
    )
    .await;

    postmark(&server)
        .deliver(&invoice())
        .await
        .expect("deliver");

    let body: Value = serde_json::from_slice(&only_request(&server).await.body).unwrap();
    assert_eq!(
        body["Attachments"],
        json!([
            { "Name": "INV-2026-0042.pdf", "Content": STANDARD.encode(PDF), "ContentType": "application/pdf" },
            { "Name": "rates.csv", "Content": STANDARD.encode(CSV), "ContentType": "text/csv; charset=utf-8" }
        ])
    );
}

// CONN-102, CONN-103
#[tokio::test]
async fn resend_sends_attachments_and_the_idempotency_key() {
    let server = server_answering("/emails", json!({ "id": "49a3999c" })).await;

    resend(&server)
        .deliver(&invoice().idempotency_key("navree-mail-0199a1b2"))
        .await
        .expect("deliver");

    let request = only_request(&server).await;
    assert_eq!(
        request
            .headers
            .get("idempotency-key")
            .map(|value| value.to_str().unwrap()),
        Some("navree-mail-0199a1b2")
    );
    let body: Value = serde_json::from_slice(&request.body).unwrap();
    assert_eq!(
        body["attachments"],
        json!([
            { "filename": "INV-2026-0042.pdf", "content": STANDARD.encode(PDF), "content_type": "application/pdf" },
            { "filename": "rates.csv", "content": STANDARD.encode(CSV), "content_type": "text/csv; charset=utf-8" }
        ])
    );
}

// CONN-103
#[tokio::test]
async fn resend_sends_no_key_for_a_message_without_one() {
    let server = server_answering("/emails", json!({ "id": "49a3999d" })).await;

    resend(&server).deliver(&invoice()).await.expect("deliver");

    assert!(
        only_request(&server)
            .await
            .headers
            .get("idempotency-key")
            .is_none()
    );
}

/// `invoice()` with one more file of `bytes` bytes.
fn with_file_of(bytes: usize) -> Message {
    invoice().attach("scan.png", "image/png", vec![7_u8; bytes])
}

// CONN-102
#[tokio::test]
async fn every_api_transport_refuses_a_message_over_its_limit_before_sending() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .mount(&server)
        .await;
    let transports: [(&str, Box<dyn MailTransport>, usize); 4] = [
        ("ses", Box::new(ses(&server)), SES_MAX_MESSAGE_BYTES),
        (
            "mailgun",
            Box::new(mailgun(&server)),
            MAILGUN_MAX_MESSAGE_BYTES,
        ),
        (
            "postmark",
            Box::new(postmark(&server)),
            POSTMARK_MAX_MESSAGE_BYTES,
        ),
        (
            "resend",
            Box::new(resend(&server)),
            RESEND_MAX_MESSAGE_BYTES,
        ),
    ];
    for (name, transport, limit) in transports {
        // Raw bytes grow by a third in base64: three quarters of the limit
        // is just over it once encoded.
        let message = with_file_of(limit / 4 * 3 + 3);
        let error = transport.deliver(&message).await.expect_err(name);
        match error {
            MailError::TooLarge {
                transport,
                size,
                limit: refused,
            } => {
                assert_eq!(transport, name);
                assert_eq!(refused, limit);
                assert!(size > limit, "{name}: {size}");
            }
            other => panic!("{name}: {other:?}"),
        }
    }
    assert!(
        server.received_requests().await.unwrap().is_empty(),
        "nothing was sent"
    );
}

// CONN-102
#[test]
fn blocked_or_malformed_attachments_fail_validation() {
    let base = || Message::new().from("a@example.in").to("b@example.in");
    assert!(
        base()
            .attach("ok.pdf", "application/pdf", PDF.to_vec())
            .validate()
            .is_ok()
    );
    for message in [
        base().attach("setup.exe", "application/octet-stream", vec![1]),
        base().attach("../secrets.txt", "text/plain", vec![1]),
        base().attach("a.pdf", "not a type", vec![1]),
        base().attachment(Attachment::new("", "text/plain", vec![1])),
    ] {
        assert!(
            matches!(message.validate(), Err(MailError::Attachment { .. })),
            "{:?}",
            message.attachments
        );
    }
}

// CONN-102
#[test]
fn a_queued_job_carries_its_attachments() {
    let message = invoice().idempotency_key("navree-mail-0199a1b2");
    let job = SendMailJob::new(message.clone());
    let payload = serde_json::to_value(&job).unwrap();
    assert_eq!(
        payload["message"]["attachments"][0]["content"],
        STANDARD.encode(PDF)
    );
    assert_eq!(
        payload["message"]["idempotency_key"],
        "navree-mail-0199a1b2"
    );
    let back: SendMailJob = serde_json::from_value(payload).unwrap();
    assert_eq!(back.message, message);
    // A message without attachments serializes as before.
    let plain = serde_json::to_value(Message::new().subject("Hi")).unwrap();
    assert!(plain.get("attachments").is_none() && plain.get("idempotency_key").is_none());
}

// CONN-102
#[test]
fn files_are_attached_from_disk_with_their_type() {
    let dir = std::env::temp_dir().join(format!("navree-mail-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("INV-2026-0042.pdf");
    std::fs::write(&path, PDF).unwrap();
    let message = Message::new().attach_from_disk(&path).unwrap();
    assert_eq!(message.attachments[0].filename, "INV-2026-0042.pdf");
    assert_eq!(message.attachments[0].content_type, "application/pdf");
    assert_eq!(message.attachments[0].content, PDF);
    assert!(matches!(
        Message::new().attach_from_disk(dir.join("missing.pdf")),
        Err(MailError::Attachment { .. })
    ));
    std::fs::remove_dir_all(&dir).unwrap();
}

// CONN-103
#[test]
fn a_malformed_idempotency_key_fails_validation() {
    let base = || Message::new().from("a@example.in").to("b@example.in");
    assert!(
        base()
            .idempotency_key("navree-mail-0199")
            .validate()
            .is_ok()
    );
    for key in ["", "two words", "line\nbreak"] {
        assert!(
            matches!(
                base().idempotency_key(key).validate(),
                Err(MailError::Header { .. })
            ),
            "{key:?}"
        );
    }
}
