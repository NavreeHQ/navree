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
        "without an envelope: Delivered-To, then To and Cc, without duplicates"
    );

    mail.envelope.recipients = vec!["Hidden@In.Acme.Test".into()];
    assert_eq!(
        mail.recipients(),
        vec!["hidden@in.acme.test"],
        "the envelope alone, when there is one"
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
        "Yes please.\n\nOn Tue, 27 Oct 2026, Agra Support <support@acme.test>\nwrote:\n> old",
        "Yes please.\n> quoted\n",
        "Yes please.\n\nFrom: Agra <support@acme.test>\nSent: Monday\n",
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
        "From: a@b.test\nContent-Type: text/html; charset=utf-8\n\n<html><head><style>p{}</style></head><body><p>Hello <b>there</b>,</p><p>see&nbsp;you &amp; bye<br>Anu</p></body></html>",
    )
    .unwrap();
    assert_eq!(
        mail.text.as_deref(),
        Some("Hello there,\nsee\u{a0}you & bye\nAnu\n")
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

/// Who a message is from is the sender's say, and a provider's verdict vouches
/// for one `From` only: whatever another reader could take for a different
/// one leaves `mail.from` empty, so a handler can't trust the wrong address.
#[test]
fn a_from_that_could_be_read_two_ways_is_no_from() {
    for raw in [
        &b"From: a@evil.test\r\nFrom: ceo@acme.test\r\n\r\nb"[..],
        b"From: ceo@acme.test\r\nFrom: a@evil.test\r\n\r\nb",
        b"From: <ceo@acme.test> <a@evil.test>\r\n\r\nb",
        b"From: <ceo@acme.test,a@evil.test>\r\n\r\nb",
        b"From: ceo@acme.test, a@evil.test\r\n\r\nb",
        b"From: ceo@acme.test a@evil.test\r\n\r\nb",
        b"From: <@a.test,@b.test:ceo@acme.test>\r\n\r\nb",
        b"From: \"ceo@acme.test\"@evil.test\r\n\r\nb",
        // A field behind a line end another reader may not see, or may see
        // where this reader does not: the provider may have read another.
        b"X-A: 1\nFrom: ceo@acme.test\r\nSubject: s\r\n\r\nb",
        b"X-A: 1\rFrom: a@evil.test\r\nFrom: ceo@acme.test\r\n\r\nb",
        b"From: ceo@acme.test\r\n\r\r\nFrom: a@evil.test\r\n\r\nb",
        // A form feed is not a blank line: both fields are read.
        b"From: ceo@acme.test\r\n\x0c\r\nFrom: a@evil.test\r\n\r\nb",
    ] {
        let mail = InboundMail::parse(raw.to_vec()).unwrap();
        assert_eq!(mail.from, None, "{:?}", String::from_utf8_lossy(raw));
    }
    let mail =
        InboundMail::parse(b"X-A: 1\nFrom: anu@acme.test\nSubject: s\n\nb".to_vec()).unwrap();
    assert_eq!(
        mail.from.unwrap().email,
        "anu@acme.test",
        "bare LFs throughout"
    );
    let mail = InboundMail::parse(b"From: Anu <anu@acme.test>\r\n\r\nb".to_vec()).unwrap();
    assert_eq!(mail.from.unwrap().email, "anu@acme.test");
}

/// A `From` is an address, or a name and an address in angle brackets, and
/// nothing else: a group, a stray separator or bracket, a quoted string beside
/// an address, a route. Another reader splits these differently (`a@evil.test`
/// opens a group here, so the address is `ceo@acme.test`; a reader that checks
/// the first address checks `evil.test`).
#[test]
fn a_from_that_is_not_an_address_or_a_named_address_is_no_from() {
    for value in [
        "a@evil.test:ceo@acme.test",
        "a@evil.test:ceo@acme.test;",
        "a@evil.test: ceo@acme.test;",
        "a@evil.test:;ceo@acme.test",
        "evil:;ceo@acme.test",
        "<a@evil.test>:ceo@acme.test",
        "\"a@evil.test\":ceo@acme.test",
        "Team: ceo@acme.test;",
        "undisclosed-recipients:;",
        "ceo@acme.test\"@evil.test",
        "ceo@acme.test\"x\"",
        "\"x\"ceo@acme.test",
        "\"ceo@acme.test\"",
        "ceo@acme.test>",
        ">ceo@acme.test",
        "ceo@acme.test)",
        ")ceo@acme.test",
        "ceo@acme.test,",
        ",ceo@acme.test",
        "ceo@acme.test;",
        ";ceo@acme.test",
        "ceo@acme.test:",
        ":ceo@acme.test",
        "Anu <ceo@acme.test",
        "Anu ceo@acme.test>",
        "Anu <ceo@acme.test> x",
        "Anu <ceo@acme.test>,",
        "Anu, <ceo@acme.test>",
        "Anu: <ceo@acme.test>",
        "Anu; <ceo@acme.test>",
        "Anu > <ceo@acme.test>",
        "Anu ) <ceo@acme.test>",
        "Anu (x <ceo@acme.test>",
        "Anu \"x <ceo@acme.test>",
        "ceo@acme.test (unclosed",
        "<ceo@acme.test(x)>",
        "<ceo@(x)acme.test>",
        "<ceo@ acme.test>",
        "< ceo@acme.test >",
        "<@evil.test:ceo@acme.test>",
        "<a@evil.test,ceo@acme.test>",
        "x\\@evil.test@acme.test",
        "=?utf-8?Q?ceo=40acme.test?=",
        "ceo@=?utf-8?Q?acme.test?=",
        "ceo@acme.test =?utf-8?Q?x?=",
        // A display name with an address in it, unquoted: some readers take
        // it for the address.
        "a@evil.test <ceo@acme.test>",
        "ceo@acme.test <a@evil.test>",
        "ceo@acme.test<a@evil.test>",
        "a@evil.test<ceo@acme.test>",
        "@<ceo@acme.test>",
        "\\<ceo@acme.test>",
        "[x] <ceo@acme.test>",
        // Labels that are empty, which no reader takes for the same address.
        "ceo@acme.test.",
        "ceo@.acme.test",
        ".ceo@acme.test",
        "ceo..x@acme.test",
        "ceo@acme..test",
        "ceo@",
        "@acme.test",
    ] {
        let raw = format!("From: {value}\r\nSubject: s\r\n\r\nb\r\n");
        let mail = InboundMail::parse(raw.into_bytes()).unwrap();
        assert_eq!(mail.from, None, "{value:?}");
    }
}

/// What is still a `From`: the two forms, with comments and blanks between
/// their tokens.
#[test]
fn an_address_or_a_named_address_is_a_from() {
    for (value, name, email) in [
        ("ceo@acme.test", None, "ceo@acme.test"),
        ("  ceo@acme.test  ", None, "ceo@acme.test"),
        ("<ceo@acme.test>", None, "ceo@acme.test"),
        ("Anu <ceo@acme.test>", Some("Anu"), "ceo@acme.test"),
        (
            "Anu Lakhotia<ceo@acme.test>",
            Some("Anu Lakhotia"),
            "ceo@acme.test",
        ),
        ("Dr. Anu <ceo@acme.test>", Some("Dr. Anu"), "ceo@acme.test"),
        (
            "\"Lakhotia, Anu\" <ceo@acme.test>",
            Some("Lakhotia, Anu"),
            "ceo@acme.test",
        ),
        (
            "\"Anu \\\"The Countess\\\"\" <ceo@acme.test>",
            Some("Anu \"The Countess\""),
            "ceo@acme.test",
        ),
        (
            "=?utf-8?Q?J=C3=BCrgen?= <ceo@acme.test>",
            Some("Jürgen"),
            "ceo@acme.test",
        ),
        ("ceo@acme.test (Anu)", Some("Anu"), "ceo@acme.test"),
        ("(Anu) ceo@acme.test", Some("Anu"), "ceo@acme.test"),
        (
            "Anu (Work) <ceo@acme.test>",
            Some("Anu (Work)"),
            "ceo@acme.test",
        ),
        (
            "Anu <ceo@acme.test> (Work)",
            Some("Anu (Work)"),
            "ceo@acme.test",
        ),
        // A name nobody checks, even when it looks like an address, once quoted.
        (
            "\"a@evil.test\" <ceo@acme.test>",
            Some("a@evil.test"),
            "ceo@acme.test",
        ),
        (
            "\"ceo@acme.test\" <a@evil.test>",
            Some("ceo@acme.test"),
            "a@evil.test",
        ),
        (
            "Anu\r\n Lakhotia <ceo@acme.test>",
            Some("Anu Lakhotia"),
            "ceo@acme.test",
        ),
    ] {
        let raw = format!("From: {value}\r\nSubject: s\r\n\r\nb\r\n");
        let mail = InboundMail::parse(raw.into_bytes()).unwrap();
        let from = mail.from.unwrap_or_else(|| panic!("no From for {value:?}"));
        assert_eq!(from.email, email, "{value:?}");
        assert_eq!(from.name.as_deref(), name, "{value:?}");
    }
}

/// Decoded values go into logs, file paths and raw headers: no line breaks or
/// other control characters come out of the reader.
#[test]
fn decoded_values_hold_no_control_characters() {
    let mail = InboundMail::parse(
        b"From: =?utf-8?Q?Hi=0D=0ABcc:_v@x.test?= <a@b.test>\r\n\
          Subject: =?utf-8?Q?Hi=0D=0ABcc:_victim@x.test=00?=\r\n\r\nb"
            .to_vec(),
    )
    .unwrap();
    assert_eq!(mail.subject.as_deref(), Some("Hi  Bcc: victim@x.test "));
    let from = mail.from.unwrap();
    assert_eq!(from.name.as_deref(), Some("Hi  Bcc: v@x.test"));
    assert_eq!(from.email, "a@b.test");

    let mail =
        InboundMail::parse(b"To: =?utf-8?Q?a=0D=0Ab=40acme.test?= (x)\r\n\r\nb".to_vec()).unwrap();
    assert!(
        !mail.to.iter().any(|to| to.email.contains(['\r', '\n'])),
        "{:?}",
        mail.to
    );

    for (raw, name) in [
        (
            &b"From: a@b.test\r\nContent-Type: application/pdf; name=\"=?utf-8?Q?..=2F..=2Fetc=2Fpasswd=00.pdf?=\"\r\n\r\nb"[..],
            "../../etc/passwd.pdf",
        ),
        (
            b"From: a@b.test\r\nContent-Type: application/pdf\r\nContent-Disposition: attachment; filename*=utf-8''..%2F..%0D%0Ax\r\n\r\nb",
            "../..x",
        ),
    ] {
        let mail = InboundMail::parse(raw.to_vec()).unwrap();
        assert_eq!(mail.attachments[0].filename.as_deref(), Some(name));
    }
}
