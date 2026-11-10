//! `SesTransport` against Moto's SES emulator: the v2 `SendEmail` request,
//! signed with SigV4, reaches a server that implements the API, and the
//! message it keeps is read back from the emulator.
//!
//! These run in the integration workflow, which starts Moto's server with
//! `scripts/integration-services.sh start mail` (variable
//! `NAVREE_TEST_SES_URL`); they are `#[ignore]`d in the other suites. Run them
//! with `-- --ignored`. A missing variable fails the test when
//! `NAVREE_TEST_REQUIRE_SERVICES=1` and prints `SKIPPED` otherwise.
//!
//! Moto does not check signatures, so this proves the request shape, the raw
//! MIME message with its attachments, and the acceptance by an implementation
//! of the API that is not hand-written here. It is an emulator: real SES (a
//! sandbox account with a verified address) is the opt-in run in
//! `sandbox.rs`.

use std::time::{Duration, Instant};

use navree_contracts::Secret;
use navree_mail::{MailTransport, Message, SesConfig, SesTransport};
use serde_json::{Value, json};

use crate::mailpit::{client, service, unique};
use crate::support::attachment_bytes;

/// The region the emulator keeps identities and messages under.
const REGION: &str = "us-east-1";
const SENDER: &str = "billing@grewal.example.in";

const PDF: &[u8] = b"%PDF-1.7 INV-2026-0042";
const CSV: &[u8] = "city,rate\nLudhiana,18\n".as_bytes();

fn transport(endpoint: &str) -> SesTransport {
    SesTransport::new(SesConfig {
        region: REGION.to_owned(),
        access_key_id: "test".to_owned(),
        secret_access_key: Secret::new("test".to_owned()),
        endpoint: Some(endpoint.to_owned()),
        ..SesConfig::default()
    })
    .expect("a transport")
}

/// Creates the identity `address` as a sender (SES refuses mail from
/// addresses that are not verified; the emulator may already accept them).
async fn verified(endpoint: &str, address: &str) {
    let response = client()
        .post(format!("{endpoint}/v2/email/identities"))
        .header(
            "authorization",
            format!(
                "AWS4-HMAC-SHA256 Credential=test/20260101/{REGION}/ses/aws4_request, SignedHeaders=host, Signature=00"
            ),
        )
        .json(&json!({ "EmailIdentity": address }))
        .send()
        .await
        .expect("the emulator answers");
    // 400 or 409: the identity exists already.
    assert!(
        response.is_success() || matches!(response.status().as_u16(), 400 | 409),
        "the emulator refused the identity: {} {}",
        response.status(),
        response.text()
    );
}

/// The message the emulator kept whose subject is `subject`, once it is
/// there. Moto lists what it was sent at `/moto-api/data.json`: a simple
/// message under `Message` (`subject`, `body`, `source`, `destinations`) and a
/// raw one under `RawMessage` (`raw_data`, `source`, `destinations`). The
/// raw entry gets its MIME text under `RawData` as well.
async fn kept(endpoint: &str, subject: &str) -> Value {
    let http = client();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let body = http
            .get(format!("{endpoint}/moto-api/data.json"))
            .send()
            .await
            .expect("the emulator answers")
            .error_for_status()
            .expect("the list is there")
            .json::<Value>()
            .expect("JSON");
        let simple = body["ses"]["Message"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|message| message["subject"].as_str() == Some(subject))
            .cloned();
        let raw = body["ses"]["RawMessage"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|message| {
                message["raw_data"]
                    .as_str()
                    .is_some_and(|raw| raw.contains(subject))
            })
            .map(|message| {
                let mut kept = message.clone();
                kept["RawData"] = kept["raw_data"].clone();
                kept
            });
        if let Some(message) = simple.or(raw) {
            return message;
        }
        assert!(
            Instant::now() < deadline,
            "the emulator never kept `{subject}`: {body}"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

fn message(subject: &str) -> Message {
    Message::new()
        .from("Grewal Traders <billing@grewal.example.in>")
        .to("Harpreet Kaur <harpreet@example.in>")
        .cc("accounts@example.in")
        .bcc("records@grewal.example.in")
        .subject(subject)
        .text("Your invoice is attached.")
        .html("<p>Your invoice is attached.</p>")
}

/// The addresses the emulator kept in the envelope: the visible recipients
/// and the blind copy.
fn assert_envelope(kept: &Value) {
    let recipients = kept["destinations"].to_string();
    for address in [
        "harpreet@example.in",
        "accounts@example.in",
        "records@grewal.example.in",
    ] {
        assert!(recipients.contains(address), "{address} not in {kept}");
    }
    assert!(
        kept["source"]
            .as_str()
            .is_some_and(|source| source.contains(SENDER)),
        "{kept}"
    );
}

#[tokio::test]
#[ignore = "needs the servers of scripts/integration-services.sh"]
async fn ses_delivers_a_message_with_attachments_as_raw_mime() {
    let test = "ses_delivers_a_message_with_attachments_as_raw_mime";
    let Some(endpoint) = service("NAVREE_TEST_SES_URL", test) else {
        return;
    };
    verified(&endpoint, SENDER).await;
    let subject = unique("Invoice INV-2026-0042");
    let message = message(&subject)
        .attach("INV-2026-0042.pdf", "application/pdf", PDF.to_vec())
        .attach("rates.csv", "text/csv; charset=utf-8", CSV.to_vec());

    let sent = transport(&endpoint)
        .deliver(&message)
        .await
        .expect("the emulator accepts the message");

    assert!(
        sent.provider_id.as_deref().is_some_and(|id| !id.is_empty()),
        "{sent:?}"
    );
    let kept = kept(&endpoint, &subject).await;
    let raw = kept["RawData"]
        .as_str()
        .unwrap_or_else(|| panic!("the emulator kept no raw message: {kept}"));
    for expected in [
        "multipart/mixed",
        "multipart/alternative",
        "Subject: ",
        "Your invoice is attached.",
        "INV-2026-0042.pdf",
        "application/pdf",
        "filename=\"rates.csv\"",
    ] {
        assert!(raw.contains(expected), "missing {expected} in:\n{raw}");
    }
    assert_eq!(attachment_bytes(raw, "INV-2026-0042.pdf"), PDF);
    assert_eq!(attachment_bytes(raw, "rates.csv"), CSV);
    // The blind copy went in the envelope only.
    assert!(
        !raw.contains("records@grewal.example.in"),
        "Bcc leaked:\n{raw}"
    );
    assert_envelope(&kept);
}

#[tokio::test]
#[ignore = "needs the servers of scripts/integration-services.sh"]
async fn ses_delivers_simple_content_without_attachments() {
    let test = "ses_delivers_simple_content_without_attachments";
    let Some(endpoint) = service("NAVREE_TEST_SES_URL", test) else {
        return;
    };
    verified(&endpoint, SENDER).await;
    let subject = unique("Simple content for Harpreet");

    transport(&endpoint)
        .send(&message(&subject).header("X-Campaign", "invoices"))
        .await
        .expect("the emulator accepts the message");

    let kept = kept(&endpoint, &subject).await;
    assert_envelope(&kept);
    // Moto keeps the HTML body when a message has both parts.
    let body = kept["body"].as_str().unwrap_or_default();
    assert!(
        body.contains("<p>Your invoice is attached.</p>"),
        "the body is not the one sent: {kept}"
    );
}
