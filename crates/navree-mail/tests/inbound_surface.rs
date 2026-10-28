//! Every public item `docs/digging-deeper/inbound-mail` names (types,
//! fields, methods, configuration keys and defaults, the event, error codes
//! and the fake), so a rename fails the build or this test. The behavior is
//! exercised in `tests/mail/inbound`; the page's examples run in the
//! facade's `tests/mail_inbound_docs.rs`.

use bytes::Bytes;
use chrono::{DateTime, Utc};
use navree_config::{Config, ConfigSources};
use navree_contracts::Secret;
use navree_events::Event;
use navree_mail::inbound::{
    Address, Attachment, Compose, Envelope, Header, INVALID_MAIL_CODE, INVALID_REPLY_TOKEN_CODE,
    InboundController, InboundMailReceived, InboundStatus, MailboxPattern, MailgunInboundOptions,
    PostmarkInboundOptions, RawInboundOptions, SesInboundOptions, StoredMail, TOO_LARGE_CODE,
    Verdict, Verdicts,
};
use navree_mail::{
    Inbound, InboundError, InboundMail, InboundOptions, Incoming, MailFake, MailOptions,
    MailboxContext, MailboxHandler, ModuleBuilderMailExt, Received, ReplyTokens,
};
use serde_json::json;
use uuid::Uuid;

#[test]
fn the_configuration_keys_have_their_documented_defaults() {
    let InboundOptions {
        max_size,
        disk,
        path,
        reply_token_ttl_days,
        ses,
        mailgun,
        postmark,
        raw,
    } = InboundOptions::default();
    assert_eq!(max_size, 26_214_400);
    assert_eq!(disk, None);
    assert_eq!(path, "inbound");
    assert_eq!(reply_token_ttl_days, 0);

    let SesInboundOptions {
        token,
        topic_arns,
        disk,
        confirm_subscriptions,
    } = ses;
    assert!(token.is_none() && topic_arns.is_empty() && disk.is_none());
    assert!(confirm_subscriptions);
    let MailgunInboundOptions { signing_key } = mailgun;
    let PostmarkInboundOptions { token: postmark } = postmark;
    let RawInboundOptions { token: raw } = raw;
    let _: [Option<Secret<String>>; 3] = [signing_key, postmark, raw];

    // The documented TOML, every key at once.
    let config = Config::load(&ConfigSources::new().set(
        "mail",
        json!({
            "inbound": {
                "max_size": 26_214_400,
                "disk": "mail",
                "path": "inbound",
                "reply_token_ttl_days": 30,
                "ses": {
                    "token": "t",
                    "topic_arns": ["arn:aws:sns:eu-west-1:123456789012:inbound-mail"],
                    "disk": "ses-mail",
                    "confirm_subscriptions": false
                },
                "mailgun": { "signing_key": "k" },
                "postmark": { "token": "p" },
                "raw": { "token": "r" }
            }
        }),
    ))
    .unwrap();
    let options = config.typed::<MailOptions>().unwrap().inbound;
    assert_eq!(options.disk.as_deref(), Some("mail"));
    assert_eq!(options.reply_token_ttl_days, 30);
    assert_eq!(options.ses.disk.as_deref(), Some("ses-mail"));
    assert!(!options.ses.confirm_subscriptions);
    assert_eq!(options.raw.token.unwrap().expose(), "r");

    // Invalid settings name their key.
    for (inbound, key) in [
        (json!({ "max_size": 0 }), "inbound.max_size"),
        (json!({ "path": "/abs" }), "inbound.path"),
        (json!({ "disk": " " }), "inbound.disk"),
        (json!({ "raw": { "token": "" } }), "inbound.raw.token"),
    ] {
        let error = Config::load(&ConfigSources::new().set("mail", json!({ "inbound": inbound })))
            .unwrap()
            .typed::<MailOptions>()
            .unwrap_err()
            .to_string();
        assert!(error.contains(key), "{error}");
    }
}

#[test]
fn the_message_has_its_documented_members() {
    let mail =
        InboundMail::parse("From: Ada <ada@example.com>\r\nTo: b@acme.test\r\n\r\nHi\r\n").unwrap();
    let InboundMail {
        id,
        source,
        provider_id,
        envelope,
        verdicts,
        message_id,
        in_reply_to,
        references,
        from,
        reply_to,
        to,
        cc,
        subject,
        date,
        text,
        html,
        headers,
        attachments,
        size,
        fingerprint,
        stored,
        received_at,
        ..
    } = mail.clone();
    let _: (Uuid, String, Option<String>, Envelope, Verdicts) =
        (id, source, provider_id, envelope, verdicts);
    let _: (Option<String>, Option<String>, Vec<String>) = (message_id, in_reply_to, references);
    let _: (Option<Address>, Vec<Address>, Vec<Address>, Vec<Address>) = (from, reply_to, to, cc);
    let _: (Option<String>, Option<DateTime<Utc>>) = (subject, date);
    let _: (Option<String>, Option<String>, Vec<Header>, Vec<Attachment>) =
        (text, html, headers, attachments);
    let _: (usize, String, Option<StoredMail>, DateTime<Utc>) =
        (size, fingerprint, stored, received_at);
    let _: &Bytes = mail.raw();
    let _: Option<&str> = mail.header("from");
    let _: Vec<&str> = mail.header_all("to");
    let _: Vec<String> = mail.recipients();
    let _: bool = mail.is_auto_reply();
    let _: Option<String> = mail.reply_text();

    let Address { name, email } = Address::new("x@y.test");
    let _: (Option<String>, String) = (name, email);
    let Header { name, value } = mail.headers[0].clone();
    let _: (String, String) = (name, value);
    let Envelope { sender, recipients } = Envelope::default();
    let _: (Option<String>, Vec<String>) = (sender, recipients);
    let Verdicts {
        spam,
        virus,
        spf,
        dkim,
        dmarc,
    } = Verdicts::default();
    let _: [Option<Verdict>; 5] = [spam, virus, spf, dkim, dmarc];
    for verdict in [
        Verdict::Pass,
        Verdict::Fail,
        Verdict::Gray,
        Verdict::ProcessingFailed,
        Verdict::Disabled,
    ] {
        let _ = serde_json::to_value(verdict).unwrap();
    }
    let StoredMail { disk, path } = StoredMail {
        disk: String::new(),
        path: String::new(),
    };
    let _: (String, String) = (disk, path);

    let attachment = Attachment {
        filename: None,
        content_type: "message/rfc822".into(),
        content_id: None,
        inline: false,
        content: Bytes::new(),
    };
    assert!(attachment.is_message());
    let _ = (attachment.size(), Attachment::as_mail);
}

#[test]
fn incoming_and_compose_have_their_documented_builders() {
    let Incoming {
        raw,
        source,
        provider_id,
        envelope,
        verdicts,
        stored,
    } = Incoming::new("imap", "From: a@b.test\r\n\r\nx")
        .provider_id("1")
        .recipient("support@acme.test")
        .sender("a@b.test")
        .verdicts(Verdicts::default())
        .stored(StoredMail {
            disk: "mail".into(),
            path: "x.eml".into(),
        });
    let _: (
        Bytes,
        String,
        Option<String>,
        Envelope,
        Verdicts,
        Option<StoredMail>,
    ) = (raw, source, provider_id, envelope, verdicts, stored);
    let _ = Incoming::from_postmark;
    let _ = Incoming::parse;

    let compose: Compose = Incoming::compose()
        .source("test")
        .from("a@b.test")
        .to("c@d.test")
        .cc("e@f.test")
        .reply_to("g@h.test")
        .envelope_to("i@j.test")
        .subject("s")
        .text("t")
        .html("<p>h</p>")
        .message_id("m@x")
        .in_reply_to("r@x")
        .reference("q@x")
        .date(Utc::now())
        .header("X-A", "1")
        .raw_header("Received", "from x")
        .attach("a.txt", "text/plain", "a")
        .inline("cid@x", "i.png", "image/png", vec![1u8])
        .forward("f.eml", "From: z@y.test\r\n\r\nz")
        .provider_id("p")
        .verdicts(Verdicts::default());
    let _: Bytes = compose.to_bytes();
    let _: Incoming = compose.build();
}

#[test]
fn services_tokens_and_patterns_have_their_documented_methods() {
    let _ = Inbound::receive;
    let _ = Inbound::receive_ses;
    let _ = Inbound::read;
    let _ = Inbound::mailbox_for;
    let _ = Inbound::mailboxes;
    let _ = Inbound::options;
    let _ = Inbound::tokens;

    let tokens = ReplyTokens::new(b"key").with_ttl_days(0);
    let _ = tokens.scoped("tickets");
    let _ = tokens.has_key();
    let _ = ReplyTokens::with_clock;
    let token = tokens.sign("v").unwrap();
    assert_eq!(tokens.verify(&token).unwrap(), "v");
    let id = tokens.sign_uuid(Uuid::nil()).unwrap();
    assert_eq!(tokens.verify_uuid(&id).unwrap(), Uuid::nil());
    let _ = tokens.address("reply+{token}@in.acme.test", "v").unwrap();
    let _ = tokens
        .address_for("reply+{token}@in.acme.test", Uuid::nil())
        .unwrap();

    let pattern = MailboxPattern::parse("reply+{token}@in.acme.test").unwrap();
    assert_eq!(pattern.as_str(), "reply+{token}@in.acme.test");
    assert_eq!(pattern.params(), ["token"]);
    assert!(pattern.matches("reply+x@in.acme.test").is_some());

    let _ = MailboxContext::pattern;
    let _ = MailboxContext::address;
    let _ = MailboxContext::param;
    let _ = MailboxContext::verified;
    let _ = MailboxContext::verified_uuid;
    let _ = MailboxContext::tokens;
    let _ = MailboxContext::app;
    let _ = |ctx: &MailboxContext| ctx.reject("closed");
    fn _ext(m: &mut navree_kernel::ModuleBuilder) {
        m.mailbox(
            "support@acme.test",
            |_mail: InboundMail, _ctx: MailboxContext| async move { Ok(()) },
        );
    }
    fn _handler<H: MailboxHandler>() {}
    let _: Option<InboundController> = None;

    let _ = MailFake::receive;
    let _ = MailFake::received;
}

#[test]
fn the_event_statuses_and_errors_have_their_documented_names() {
    assert_eq!(InboundMailReceived::NAME, "mail.inbound.received");
    let event = InboundMailReceived {
        id: Uuid::nil(),
        source: "ses".into(),
        provider_id: None,
        message_id: None,
        from: None,
        recipients: Vec::new(),
        subject: None,
        size: 0,
        attachments: 0,
        fingerprint: String::new(),
        status: InboundStatus::Handled,
        mailbox: None,
        address: None,
        stored: None,
        received_at: Utc::now(),
    };
    assert_eq!(serde_json::to_value(&event).unwrap()["status"], "handled");
    for (status, name) in [
        (InboundStatus::Handled, "handled"),
        (InboundStatus::Unrouted, "unrouted"),
        (InboundStatus::Rejected, "rejected"),
    ] {
        assert_eq!(serde_json::to_value(status).unwrap(), json!(name));
    }
    let Received {
        id,
        status,
        mailbox,
        address,
        reason,
        stored,
    } = Received {
        id: Uuid::nil(),
        status: InboundStatus::Unrouted,
        mailbox: None,
        address: None,
        reason: None,
        stored: None,
    };
    let _ = (id, status, mailbox, address, reason, stored);

    assert_eq!(TOO_LARGE_CODE, "mail_too_large");
    assert_eq!(INVALID_MAIL_CODE, "invalid_inbound_mail");
    assert_eq!(INVALID_REPLY_TOKEN_CODE, "invalid_reply_token");
    let cases = [
        (
            InboundError::TooLarge { size: 2, max: 1 },
            413,
            TOO_LARGE_CODE,
        ),
        (
            InboundError::Unparseable("x".into()),
            400,
            INVALID_MAIL_CODE,
        ),
        (
            InboundError::Malformed {
                provider: "postmark",
                reason: "x".into(),
            },
            400,
            INVALID_MAIL_CODE,
        ),
        (InboundError::InvalidToken, 400, INVALID_REPLY_TOKEN_CODE),
        (InboundError::ExpiredToken, 400, INVALID_REPLY_TOKEN_CODE),
    ];
    for (error, status, code) in cases {
        let body = error.to_app_error().unwrap().body();
        assert_eq!(body["statusCode"], json!(status), "{body}");
        assert_eq!(body["code"], json!(code), "{body}");
        assert!(navree_http::codes::is_valid(code));
    }
    for error in [
        InboundError::MissingKey,
        InboundError::AddressTooLong { local: "x".into() },
        InboundError::Rejected("x".into()),
        InboundError::Config("x".into()),
    ] {
        assert!(error.to_app_error().is_none(), "{error}");
    }
    assert!(InboundError::Rejected("x".into()).is_rejection());
    assert!(InboundError::InvalidToken.is_rejection());
    assert!(!InboundError::MissingKey.is_rejection());
}
