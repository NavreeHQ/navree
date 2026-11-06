//! The mail providers' own test and sandbox modes: one real request to each,
//! with an attachment where the provider takes one. Opt in: a test runs only
//! when the keys for its provider are set, and prints `SKIPPED` and passes
//! otherwise, so a fork or a machine without the accounts is not told it failed.
//! They are `#[ignore]`d too; run them with `-- --ignored` (the `sandbox` job of
//! `.github/workflows/integrations.yml` does, with the keys from the
//! repository's secrets).
//!
//! | Provider | Mode | Variables |
//! |---|---|---|
//! | Postmark | the test token `POSTMARK_API_TEST`: the request is checked, nothing is sent | none |
//! | Resend | any API key sends to `delivered@resend.dev`, which Resend accepts and drops | `NAVREE_SANDBOX_RESEND_KEY` |
//! | Mailgun | a sandbox domain, to a recipient authorized on it | `NAVREE_SANDBOX_MAILGUN_KEY`, `_DOMAIN`, `_TO`, optionally `_REGION` (`us` or `eu`) |
//! | Amazon SES | an account in the sandbox, to the mailbox simulator `success@simulator.amazonses.com` | `NAVREE_SANDBOX_SES_KEY_ID`, `_SECRET`, `_REGION`, `_FROM` (a verified address) |
//!
//! What a sandbox cannot show: webhook delivery needs a public address for the
//! provider to call, and a spend cap is the provider's (these send one message
//! each; Resend, Mailgun and SES count it against the free tier).

use navree_contracts::Secret;
use navree_mail::{
    MailTransport, MailgunConfig, MailgunRegion, MailgunTransport, Message, PostmarkConfig,
    PostmarkTransport, ResendConfig, ResendTransport, SesConfig, SesTransport,
};

const PDF: &[u8] = b"%PDF-1.7 INV-2026-0042";

/// The value of `variable` when it is set and not empty.
fn sandbox(variable: &str, test: &str) -> Option<String> {
    match std::env::var(variable) {
        Ok(value) if !value.trim().is_empty() => Some(value.trim().to_owned()),
        _ => {
            println!("SKIPPED {test}: set {variable}");
            None
        }
    }
}

/// A message with an HTML body and a small PDF.
fn invoice(from: &str, to: &str, subject: &str) -> Message {
    Message::new()
        .from(from)
        .to(to)
        .subject(subject)
        .text("Your invoice is attached. Sandbox test from Navree.")
        .html("<p>Your invoice is attached. Sandbox test from Navree.</p>")
        .attach("INV-2026-0042.pdf", "application/pdf", PDF.to_vec())
}

#[tokio::test]
#[ignore = "calls the providers' sandboxes"]
async fn sandbox_postmark_accepts_the_request_with_its_test_token() {
    let transport = PostmarkTransport::new(PostmarkConfig {
        server_token: Secret::new("POSTMARK_API_TEST".to_owned()),
        ..PostmarkConfig::default()
    })
    .expect("a transport");

    let sent = transport
        .deliver(&invoice(
            "Grewal Traders <billing@grewal.example.in>",
            "Harpreet Kaur <harpreet@example.in>",
            "Invoice INV-2026-0042",
        ))
        .await
        .expect("Postmark accepts the request");

    assert!(sent.provider_id.is_some(), "{sent:?}");
}

#[tokio::test]
#[ignore = "calls the providers' sandboxes"]
async fn sandbox_resend_accepts_a_message_for_its_test_address() {
    let test = "sandbox_resend_accepts_a_message_for_its_test_address";
    let Some(key) = sandbox("NAVREE_SANDBOX_RESEND_KEY", test) else {
        return;
    };
    let transport = ResendTransport::new(ResendConfig {
        api_key: Secret::new(key),
        endpoint: None,
    })
    .expect("a transport");

    let sent = transport
        .deliver(&invoice(
            "Navree <onboarding@resend.dev>",
            "delivered@resend.dev",
            "Invoice INV-2026-0042",
        ))
        .await
        .expect("Resend accepts the message");

    assert!(sent.provider_id.is_some(), "{sent:?}");
}

#[tokio::test]
#[ignore = "calls the providers' sandboxes"]
async fn sandbox_mailgun_accepts_a_message_on_its_sandbox_domain() {
    let test = "sandbox_mailgun_accepts_a_message_on_its_sandbox_domain";
    let (Some(key), Some(domain), Some(to)) = (
        sandbox("NAVREE_SANDBOX_MAILGUN_KEY", test),
        sandbox("NAVREE_SANDBOX_MAILGUN_DOMAIN", test),
        sandbox("NAVREE_SANDBOX_MAILGUN_TO", test),
    ) else {
        return;
    };
    let region = match std::env::var("NAVREE_SANDBOX_MAILGUN_REGION").as_deref() {
        Ok("eu") => MailgunRegion::Eu,
        _ => MailgunRegion::Us,
    };
    let transport = MailgunTransport::new(MailgunConfig {
        domain: domain.clone(),
        api_key: Secret::new(key),
        region,
        ..MailgunConfig::default()
    })
    .expect("a transport");

    let sent = transport
        .deliver(&invoice(
            &format!("Navree <postmaster@{domain}>"),
            &to,
            "Invoice INV-2026-0042",
        ))
        .await
        .expect("Mailgun accepts the message");

    assert!(sent.provider_id.is_some(), "{sent:?}");
}

#[tokio::test]
#[ignore = "calls the providers' sandboxes"]
async fn sandbox_ses_accepts_a_raw_message_for_the_mailbox_simulator() {
    let test = "sandbox_ses_accepts_a_raw_message_for_the_mailbox_simulator";
    let (Some(key_id), Some(secret), Some(region), Some(from)) = (
        sandbox("NAVREE_SANDBOX_SES_KEY_ID", test),
        sandbox("NAVREE_SANDBOX_SES_SECRET", test),
        sandbox("NAVREE_SANDBOX_SES_REGION", test),
        sandbox("NAVREE_SANDBOX_SES_FROM", test),
    ) else {
        return;
    };
    let transport = SesTransport::new(SesConfig {
        region,
        access_key_id: key_id,
        secret_access_key: Secret::new(secret),
        ..SesConfig::default()
    })
    .expect("a transport");

    transport
        .check()
        .await
        .expect("the credentials read the account");
    // With an attachment SES takes the message as raw MIME.
    let sent = transport
        .deliver(&invoice(
            &from,
            "success@simulator.amazonses.com",
            "Invoice INV-2026-0042",
        ))
        .await
        .expect("SES accepts the message");

    assert!(
        sent.provider_id.as_deref().is_some_and(|id| !id.is_empty()),
        "{sent:?}"
    );
}
