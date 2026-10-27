//! `Incoming::compose`: messages built field by field parse back the same.

use chrono::{TimeZone, Utc};
use navree_mail::Incoming;
use navree_mail::inbound::{StoredMail, Verdict, Verdicts};

#[test]
fn a_composed_message_parses_back() {
    let incoming = Incoming::compose()
        .from("Jürgen Müller <juergen@example.de>")
        .to("reply+abc@in.acme.test")
        .cc("Grace <grace@example.com>")
        .reply_to("home@example.de")
        .subject("Grüße zum Ticket")
        .text("Danke schön!\nBis bald.")
        .html("<p>Danke schön!</p>")
        .message_id("<m-1@example.de>")
        .in_reply_to("ticket-42@acme.test")
        .reference("ticket-41@acme.test")
        .reference("<ticket-42@acme.test>")
        .date(Utc.with_ymd_and_hms(2026, 10, 27, 9, 0, 0).unwrap())
        .header("X-Ticket", "42")
        .attach("notes.txt", "text/plain", "fire: out\n")
        .inline("logo@acme.test", "logo.png", "image/png", vec![0x89, b'P'])
        .build();

    assert_eq!(incoming.source, "test");
    assert_eq!(
        incoming.envelope.recipients,
        vec!["reply+abc@in.acme.test", "grace@example.com"]
    );
    assert_eq!(incoming.envelope.sender.as_deref(), Some("juergen@example.de"));

    let mail = incoming.parse().unwrap();
    assert_eq!(mail.source, "test");
    assert_eq!(
        mail.from.as_ref().and_then(|a| a.name.as_deref()),
        Some("Jürgen Müller")
    );
    assert_eq!(mail.to[0].email, "reply+abc@in.acme.test");
    assert_eq!(mail.cc[0].name.as_deref(), Some("Grace"));
    assert_eq!(mail.reply_to[0].email, "home@example.de");
    assert_eq!(mail.subject.as_deref(), Some("Grüße zum Ticket"));
    assert_eq!(
        mail.text.as_deref().map(str::trim),
        Some("Danke schön!\r\nBis bald.")
    );
    assert_eq!(mail.html.as_deref().map(str::trim), Some("<p>Danke schön!</p>"));
    assert_eq!(mail.message_id.as_deref(), Some("m-1@example.de"));
    assert_eq!(mail.in_reply_to.as_deref(), Some("ticket-42@acme.test"));
    assert_eq!(
        mail.references,
        vec!["ticket-41@acme.test", "ticket-42@acme.test"]
    );
    assert_eq!(
        mail.date,
        Some(Utc.with_ymd_and_hms(2026, 10, 27, 9, 0, 0).unwrap())
    );
    assert_eq!(mail.header("X-Ticket"), Some("42"));
    assert_eq!(mail.attachments.len(), 2);
    assert_eq!(mail.attachments[0].filename.as_deref(), Some("notes.txt"));
    assert_eq!(mail.attachments[0].content.as_ref(), b"fire: out\n");
    assert!(mail.attachments[1].inline);
    assert_eq!(
        mail.attachments[1].content_id.as_deref(),
        Some("logo@acme.test")
    );
}

#[test]
fn envelope_recipients_override_the_headers() {
    let incoming = Incoming::compose()
        .from("ada@example.com")
        .to("all@acme.test")
        .envelope_to("hidden@in.acme.test")
        .text("hi")
        .build();

    assert_eq!(incoming.envelope.recipients, vec!["hidden@in.acme.test"]);
    let mail = incoming.parse().unwrap();
    assert_eq!(
        mail.recipients(),
        vec!["hidden@in.acme.test", "all@acme.test"]
    );
}

#[test]
fn a_forwarded_message_is_attached_whole() {
    let original = Incoming::compose()
        .from("helpdesk@bad.example")
        .to("joan@acme.test")
        .subject("Your mailbox is full")
        .html("<a href=\"https://bad.example/login\">here</a>")
        .to_bytes();

    let mail = Incoming::compose()
        .from("joan@acme.test")
        .to("phish@in.acme.test")
        .subject("Fwd: Your mailbox is full")
        .text("This looks suspicious.")
        .forward("original.eml", original)
        .build()
        .parse()
        .unwrap();

    let forwarded = mail.attachments[0].as_mail().unwrap();
    assert_eq!(forwarded.subject.as_deref(), Some("Your mailbox is full"));
    assert!(forwarded.html.unwrap().contains("bad.example/login"));
}

#[test]
fn provider_details_carry_over() {
    let verdicts = Verdicts {
        spam: Some(Verdict::Fail),
        ..Verdicts::default()
    };
    let incoming = Incoming::compose()
        .source("custom")
        .from("a@b.test")
        .to("c@d.test")
        .provider_id("p-1")
        .verdicts(verdicts)
        .stored(StoredMail {
            disk: "mail".into(),
            path: "x.eml".into(),
        })
        .build();

    let mail = incoming.parse().unwrap();
    assert_eq!(mail.source, "custom");
    assert_eq!(mail.provider_id.as_deref(), Some("p-1"));
    assert!(mail.verdicts.is_flagged());
    assert_eq!(mail.stored.unwrap().path, "x.eml");
}

#[test]
fn raw_headers_win_over_the_fields() {
    let mail = Incoming::compose()
        .raw_header("From", "Original <original@example.com>")
        .raw_header("Received", "from mx by mx")
        .raw_header("Content-Type", "multipart/evil; boundary=x")
        .from("ignored@example.com")
        .to("c@d.test")
        .text("body")
        .build()
        .parse()
        .unwrap();

    assert_eq!(
        mail.from.as_ref().map(|a| a.email.as_str()),
        Some("original@example.com")
    );
    assert_eq!(mail.header_all("From").len(), 1);
    assert_eq!(mail.header("Received"), Some("from mx by mx"));
    assert_eq!(mail.text.as_deref().map(str::trim), Some("body"));
}

#[test]
fn incoming_builders_set_the_envelope() {
    let incoming = Incoming::new("raw", "From: a@b.test\r\n\r\nhi")
        .sender("bounce@b.test")
        .recipient("x@in.acme.test")
        .recipient("y@in.acme.test")
        .provider_id("id-9");

    assert_eq!(incoming.envelope.sender.as_deref(), Some("bounce@b.test"));
    assert_eq!(incoming.envelope.recipients.len(), 2);
    let debug = format!("{incoming:?}");
    assert!(!debug.contains("hi"), "{debug}");
}
