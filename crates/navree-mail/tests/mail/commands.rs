//! The commands `mail:test`, `mail:suppressions` and `mail:domains`, and
//! `MailFake`.

use std::sync::Arc;

use navree_mail::{DkimSigner, DnsResolver, MailError, StaticResolver, Suppressions};

use crate::harness::mail_app;

#[tokio::test]
async fn mail_test_sends_a_test_message() {
    let mail = mail_app(|b| b).await;

    mail.app
        .run_command("mail:test", &["ops@acme.test", "--subject", "Is mail working?"])
        .await
        .expect("mail:test");

    mail.fake
        .assert_sent(|m| m.to == ["ops@acme.test"] && m.subject == "Is mail working?");
    let error = mail.app.run_command("mail:test", &[]).await.unwrap_err();
    assert!(error.to_string().contains("usage"), "{error}");
    let error = mail
        .app
        .run_command("mail:test", &["ops@acme.test", "--via", "nope"])
        .await
        .unwrap_err();
    assert!(error.to_string().contains("mail.transports.nope"), "{error}");
}

#[tokio::test]
async fn mail_suppressions_adds_lists_and_removes() {
    let mail = mail_app(|b| b).await;
    let suppressions = mail.get::<Suppressions>().await;

    mail.app
        .run_command("mail:suppressions", &["add", "eve@example.com", "--reason", "complaint"])
        .await
        .expect("add");
    assert!(suppressions.is_suppressed(None, "eve@example.com").await.unwrap());
    mail.app
        .run_command("mail:suppressions", &["list"])
        .await
        .expect("list");
    mail.app
        .run_command("mail:suppressions", &["remove", "eve@example.com"])
        .await
        .expect("remove");
    assert!(!suppressions.is_suppressed(None, "eve@example.com").await.unwrap());

    let error = mail
        .app
        .run_command("mail:suppressions", &["add", "eve@example.com", "--reason", "boredom"])
        .await
        .unwrap_err();
    assert!(error.to_string().contains("unknown suppression reason"), "{error}");
    let error = mail
        .app
        .run_command("mail:suppressions", &["purge"])
        .await
        .unwrap_err();
    assert!(error.to_string().contains("unknown action"), "{error}");
}

#[tokio::test]
async fn mail_domains_checks_a_domain() {
    let signer = DkimSigner::from_pem(
        "acme.test",
        "mail",
        include_str!("../fixtures/dkim-rsa-2048.pem"),
    )
    .unwrap();
    let dns = StaticResolver::new()
        .record("acme.test", "v=spf1 include:amazonses.com -all")
        .record("mail._domainkey.acme.test", &signer.dns_record())
        .record("_dmarc.acme.test", "v=DMARC1; p=reject; rua=mailto:d@acme.test");
    let mail = mail_app(|b| b.override_value::<dyn DnsResolver>(Arc::new(dns))).await;

    mail.app
        .run_command(
            "mail:domains",
            &["acme.test", "--selector", "mail", "--spf-include", "amazonses.com"],
        )
        .await
        .expect("ready");
    let error = mail
        .app
        .run_command("mail:domains", &["other.test"])
        .await
        .unwrap_err();
    assert!(error.to_string().contains("not ready"), "{error}");
}

#[tokio::test]
async fn the_fake_plays_the_provider() {
    let mail = mail_app(|b| b).await;
    let mailer = mail.mailer().await;
    mailer
        .send_message(navree_mail::Message::new().to("ada@example.com").cc("bob@example.com").subject("One").text("1"))
        .await
        .unwrap();
    mailer
        .send_message(navree_mail::Message::new().to("bob@example.com").subject("Two").text("2"))
        .await
        .unwrap();

    assert_eq!(mail.fake.sent().len(), 2);
    assert_eq!(mail.fake.sent_to("BOB@example.com").len(), 2);
    assert_eq!(mail.fake.sent_to("ada@example.com")[0].subject, "One");

    // The event goes to the last message sent to the recipient.
    let delivery = mail.fake.open(mail.app.ctx(), "bob@example.com").await.unwrap().unwrap();
    assert_eq!(delivery.provider_id.as_deref(), Some("fake-2"));
    assert_eq!(mail.fake.events()[0].provider_id.as_deref(), Some("fake-2"));

    let error = mail.fake.bounce(mail.app.ctx(), "nobody@example.com").await.unwrap_err();
    assert!(matches!(error, MailError::NotFound(_)));
}
