//! `InboundMail::parse` on fixture `.eml` files.

use chrono::{TimeZone, Utc};
use navree_mail::inbound::{Address, Envelope};
use navree_mail::{InboundError, InboundMail};
use sha2::{Digest, Sha256};

use super::fixture;

#[test]
fn a_reply_reads_its_headers_addresses_and_text() {
    let raw = fixture("reply.eml");
    let mail = InboundMail::parse(raw.clone()).unwrap();

    assert_eq!(mail.source, "raw");
    assert_eq!(
        mail.message_id.as_deref(),
        Some("CAF2reply-0001@mail.example.com")
    );
    assert_eq!(mail.in_reply_to.as_deref(), Some("ticket-42@acme.test"));
    assert_eq!(
        mail.references,
        vec!["ticket-41@acme.test", "ticket-42@acme.test"]
    );
    assert_eq!(
        mail.from,
        Some(Address {
            name: Some("Ada Lovelace".into()),
            email: "ada@example.com".into()
        })
    );
    assert_eq!(mail.to[0].email, "reply+abc@in.acme.test");
    assert_eq!(mail.to[0].name.as_deref(), Some("Acme Support"));
    assert_eq!(mail.cc, vec![Address::new("grace@example.com")]);
    assert_eq!(mail.reply_to[0].email, "ada@home.example");
    assert_eq!(mail.subject.as_deref(), Some("Re: [#42] Printer on fire"));
    assert_eq!(
        mail.date,
        Some(Utc.with_ymd_and_hms(2026, 10, 27, 9, 14, 0).unwrap())
    );
    assert!(
        mail.text
            .as_deref()
            .unwrap()
            .starts_with("Thanks, the fire is out now.")
    );
    assert_eq!(mail.html, None);
    assert!(mail.attachments.is_empty());
    assert_eq!(mail.size, raw.len());
    assert_eq!(mail.raw().as_ref(), raw.as_slice());
    assert_eq!(mail.fingerprint, hex::encode(Sha256::digest(&raw)));
    assert_eq!(mail.envelope, Envelope::default());
    assert_eq!(mail.stored, None);
}

#[test]
fn headers_are_unfolded_and_found_in_any_case() {
    let mail = InboundMail::parse(fixture("reply.eml")).unwrap();

    assert_eq!(mail.header("x-ticket-client"), Some("Navree Mail Test"));
    assert_eq!(
        mail.header_all("X-Ticket-Client"),
        vec!["Navree Mail Test", "Second Value"]
    );
    assert_eq!(
        mail.header("received"),
        Some(
            "from mail.example.com (mail.example.com [203.0.113.7]) by mx.acme.test with ESMTPS id 4F2a; Tue, 27 Oct 2026 09:14:02 +0000"
        )
    );
    assert_eq!(mail.header("X-Missing"), None);
    assert_eq!(mail.headers[0].name, "Return-Path");
}

#[test]
fn recipients_list_the_envelope_then_the_delivery_headers() {
    let mut mail = InboundMail::parse(fixture("reply.eml")).unwrap();
    assert_eq!(
        mail.recipients(),
        vec!["reply+abc@in.acme.test", "grace@example.com"],
        "Delivered-To, then To and Cc, without duplicates"
    );

    mail.envelope.recipients = vec!["Hidden@In.Acme.Test".into()];
    assert_eq!(
        mail.recipients(),
        vec![
            "hidden@in.acme.test",
            "reply+abc@in.acme.test",
            "grace@example.com"
        ]
    );
}

#[test]
fn a_multipart_message_gives_text_html_inline_parts_and_files() {
    let mail = InboundMail::parse(fixture("report.eml")).unwrap();

    assert_eq!(
        mail.text.as_deref().map(str::trim),
        Some("The report is attached.")
    );
    assert!(
        mail.html
            .as_deref()
            .unwrap()
            .contains("<img src=\"cid:logo@example.com\">")
    );
    assert_eq!(mail.to.len(), 2);
    assert_eq!(mail.to[1].name.as_deref(), Some("Billing"));
    assert_eq!(mail.attachments.len(), 2);

    let logo = &mail.attachments[0];
    assert_eq!(logo.filename.as_deref(), Some("logo.png"));
    assert_eq!(logo.content_type, "image/png");
    assert_eq!(logo.content_id.as_deref(), Some("logo@example.com"));
    assert!(logo.inline);
    assert_eq!(&logo.content[..4], &[0x89, b'P', b'N', b'G']);

    let pdf = &mail.attachments[1];
    assert_eq!(pdf.filename.as_deref(), Some("q3.pdf"));
    assert_eq!(pdf.content_type, "application/pdf");
    assert!(!pdf.inline);
    assert_eq!(pdf.content.as_ref(), "%PDF-1.4\n%äüöß\n".as_bytes());
    assert_eq!(pdf.size(), pdf.content.len());
    assert!(!pdf.is_message());
    assert!(pdf.as_mail().is_err());
}

#[test]
fn a_forwarded_message_is_an_attachment_that_parses() {
    let mail = InboundMail::parse(fixture("forwarded.eml")).unwrap();

    assert_eq!(
        mail.text.as_deref().map(str::trim),
        Some("This looks suspicious.")
    );
    let forwarded = &mail.attachments[0];
    assert!(forwarded.is_message());
    assert_eq!(forwarded.content_type, "message/rfc822");
    assert_eq!(
        forwarded.filename.as_deref(),
        Some("Your mailbox is full.eml")
    );

    let original = forwarded.as_mail().unwrap();
    assert_eq!(original.subject.as_deref(), Some("Your mailbox is full"));
    assert_eq!(
        original.from.as_ref().map(|a| a.email.as_str()),
        Some("helpdesk@bad.example")
    );
    assert!(
        original
            .html
            .as_deref()
            .unwrap()
            .contains("https://bad.example/login")
    );
    assert_eq!(original.message_id.as_deref(), Some("evil-1@bad.example"));
}

#[test]
fn encoded_words_and_legacy_charsets_are_decoded() {
    let mail = InboundMail::parse(fixture("encoded.eml")).unwrap();

    assert_eq!(
        mail.from.as_ref().and_then(|a| a.name.as_deref()),
        Some("Jürgen Müller")
    );
    assert_eq!(mail.subject.as_deref(), Some("Grüße aus München"));
    assert_eq!(mail.text.as_deref().map(str::trim), Some("Schöne Grüße!"));
}

#[test]
fn auto_replies_are_recognized() {
    assert!(
        InboundMail::parse(fixture("auto-reply.eml"))
            .unwrap()
            .is_auto_reply()
    );
    assert!(
        !InboundMail::parse(fixture("reply.eml"))
            .unwrap()
            .is_auto_reply()
    );
    for header in [
        "Auto-Submitted: auto-generated",
        "Precedence: bulk",
        "X-Autorespond: yes",
        "X-Auto-Response-Suppress: OOF, AutoReply",
        "Content-Type: multipart/report; report-type=delivery-status; boundary=x",
    ] {
        let raw = format!("From: a@b.test\nTo: c@d.test\n{header}\nSubject: s\n\nbody\n");
        assert!(InboundMail::parse(raw).unwrap().is_auto_reply(), "{header}");
    }
    let human = "From: a@b.test\nTo: c@d.test\nAuto-Submitted: no\nSubject: s\n\nbody\n";
    assert!(!InboundMail::parse(human).unwrap().is_auto_reply());
}

#[test]
fn the_reply_text_leaves_the_quoted_message_out() {
    let mail = InboundMail::parse(fixture("reply.eml")).unwrap();
    assert_eq!(
        mail.reply_text().as_deref(),
        Some("Thanks, the fire is out now.")
    );

    for body in [
        "Yes please.\n\n-----Original Message-----\nFrom: x@y.test\n",
        "Yes please.\n\nOn Tue, 27 Oct 2026, Acme Support <support@acme.test>\nwrote:\n> old",
        "Yes please.\n> quoted\n",
        "Yes please.\n\nFrom: Acme <support@acme.test>\nSent: Monday\n",
    ] {
        let raw = format!("From: a@b.test\nTo: c@d.test\nSubject: s\n\n{body}");
        assert_eq!(
            InboundMail::parse(raw).unwrap().reply_text().as_deref(),
            Some("Yes please."),
            "{body:?}"
        );
    }
}

#[test]
fn bytes_without_headers_do_not_parse() {
    assert!(matches!(
        InboundMail::parse(Vec::new()),
        Err(InboundError::Unparseable(_))
    ));
}

#[test]
fn file_names_and_names_in_any_encoding_are_decoded() {
    let mut raw = b"From: =?iso-8859-1?Q?J".to_vec();
    raw.push(0xfc);
    raw.extend_from_slice(
        concat!(
            "rgen?= <j@example.de>\r\n",
            "To: support@acme.test\r\n",
            "Subject: =?utf-8?B?R3LDvMOfZQ==?= =?utf-8?q?_aus_M=C3=BCnchen?=\r\n",
            "Content-Type: multipart/mixed; boundary=\"b\"\r\n",
            "\r\n",
            "--b\r\n",
            "Content-Type: text/plain; charset=utf-8\r\n",
            "\r\n",
            "Hallo\r\n",
            "--b\r\n",
            "Content-Type: application/pdf; name=\"=?utf-8?B?w6TDtsO8LnBkZg==?=\"\r\n",
            "Content-Disposition: attachment;\r\n",
            " filename*0*=utf-8''Gr%C3%BC%C3;\r\n",
            " filename*1*=%9Fe.pdf\r\n",
            "Content-Transfer-Encoding: base64\r\n",
            "\r\n",
            "JVBERg==\r\n",
            "--b--\r\n",
        )
        .as_bytes(),
    );
    let mail = InboundMail::parse(raw).unwrap();

    assert_eq!(
        mail.from.as_ref().and_then(|a| a.name.as_deref()),
        Some("Jürgen"),
        "an 8-bit byte inside an encoded word is read in its charset"
    );
    assert_eq!(mail.subject.as_deref(), Some("Grüße aus München"));
    assert_eq!(mail.text.as_deref(), Some("Hallo"));
    let pdf = &mail.attachments[0];
    assert_eq!(pdf.filename.as_deref(), Some("Grüße.pdf"));
    assert_eq!(pdf.content.as_ref(), b"%PDF");
}

#[test]
fn an_html_only_message_has_its_text_read_from_the_html() {
    let mail = InboundMail::parse(
        "From: a@b.test\nContent-Type: text/html; charset=utf-8\n\n<html><head><style>p{}</style></head><body><p>Hello <b>there</b>,</p><p>see&nbsp;you &amp; bye<br>Ada</p></body></html>",
    )
    .unwrap();
    assert_eq!(
        mail.text.as_deref(),
        Some("Hello there,\nsee\u{a0}you & bye\nAda\n")
    );
    assert!(mail.html.as_deref().unwrap().starts_with("<html>"));
    assert!(mail.attachments.is_empty());
}

#[test]
fn header_fields_that_are_not_utf8_are_kept() {
    let mut raw = b"From: a@b.test\nX-Legacy: caf".to_vec();
    raw.extend_from_slice(b"\xe9\n\nbody\n");
    let mail = InboundMail::parse(raw).unwrap();
    assert_eq!(mail.header("X-Legacy"), Some("caf\u{fffd}"));
}

/// The reader's limits at reduced scale: each refuses the message as
/// `Unparseable` while it is read, before what it counts is held.
#[test]
fn messages_that_declare_too_much_are_unparseable() {
    let unparseable = |raw: String, key: &str| match InboundMail::parse(raw) {
        Err(InboundError::Unparseable(why)) => assert!(why.contains(key), "{why}"),
        other => panic!("{key}: {other:?}"),
    };

    // 1,001 empty parts: a few bytes each to write, a record each to hold.
    unparseable(
        format!(
            "Content-Type: multipart/mixed; boundary=b\n\n{}--b--\n",
            "--b\n\n".repeat(1001)
        ),
        "mail.inbound.max_parts",
    );
    // 33 multiparts nested in one another.
    let mut deep = String::from("Subject: deep\n");
    for level in 0..33 {
        deep.push_str(&format!(
            "Content-Type: multipart/mixed; boundary=b{level}\n\n--b{level}\n"
        ));
    }
    unparseable(deep, "mail.inbound.max_depth");
    // 1,001 header fields.
    unparseable("X: y\n".repeat(1001) + "\nbody", "mail.inbound.max_headers");
    // 1,001 addresses in To, and 1,001 ids in References.
    unparseable(
        format!("From: a@b.test\nTo: {}\n\nbody", "a@b.test,".repeat(1001)),
        "mail.inbound.max_addresses",
    );
    unparseable(
        format!(
            "From: a@b.test\nReferences: {}\n\nbody",
            "<a@b>".repeat(1001)
        ),
        "mail.inbound.max_addresses",
    );

    // At the limits the message is read.
    let mail = InboundMail::parse(format!(
        "From: a@b.test\nTo: {}\nReferences: {}\n\nbody",
        "a@b.test,".repeat(1000),
        "<a@b> ".repeat(1000)
    ))
    .unwrap();
    assert_eq!((mail.to.len(), mail.references.len()), (1000, 1000));
}
