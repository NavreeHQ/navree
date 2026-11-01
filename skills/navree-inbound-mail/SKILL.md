---
name: navree-inbound-mail
description: "Receives mail in a Navree app: mailboxes routed by address (`m.mailbox(\"reply+{token}@in.acme.test\", handler)`, `mailbox_with::<H>`), `InboundMail` (headers, bodies, attachments, forwarded messages, `reply_text`, `is_auto_reply`), signed reply addresses (`ReplyTokens`), raw `.eml` files on a storage disk, the SES, Mailgun, Postmark and raw MIME sources under `/api/mail/inbound`, Microsoft 365 and Gmail mailboxes watched through integration connections (`MailboxWatches`), `InboundMailReceived`, and `MailFake::receive` in tests. Use when handling replies to notifications or tickets, a support or reporting address, forwarded phishing reports, or any mail the app receives."
---

# Inbound mail

Facade feature `mail-inbound` (in `full`; navree-mail's `inbound`). Sending
mail is in `navree-mail-and-notifications` and `navree-mail-delivery`.

## Steps: receive mail at an address

1. Modules: `MailModule` (and `ViewModule`), plus `CryptoModule` with
   `APP_KEY` for reply tokens, `StorageModule` for `mail.inbound.disk`,
   `EventsModule` for `InboundMailReceived`.
2. Register a mailbox in a module's `register` (`#[module(register = Self::wire)]`):

   ```rust
   m.mailbox("support@acme.test", |mail: InboundMail, ctx: MailboxContext| async move {
       ctx.get::<SupportInbox>().await?.open_conversation(&mail).await?;
       Ok(())
   });
   // or, with dependencies: #[injectable] struct H + impl MailboxHandler for H
   m.mailbox_with::<TicketReplies>("reply+{token}@in.acme.test");
   ```

3. Pick a source and set its secret (`.env`: `APP_MAIL__INBOUND__RAW__TOKEN=...`):
   - SES: `mail.inbound.ses.token` (+ `topic_arns`); subscribe
     `https://api/.../api/mail/inbound/ses?token=...` to the receipt rule's
     SNS topic (raw delivery off). S3 action: `mail.inbound.ses.disk` = an
     S3 disk on the bucket.
   - Mailgun: route `forward("https://.../api/mail/inbound/mailgun/mime")`;
     key `mail.inbound.mailgun.signing_key` (falls back to
     `mail.webhooks.mailgun.signing_key`).
   - Postmark: webhook `https://postmark:TOKEN@.../api/mail/inbound/postmark`,
     `mail.inbound.postmark.token`; turn on "Include raw email content".
   - Your server: `POST /api/mail/inbound/raw`, `Authorization: Bearer`,
     body `message/rfc822`, `X-Envelope-From`/`X-Envelope-To`.
4. Optional: `mail.inbound.disk = "mail"` keeps every message at
   `{path}/{yyyy}/{mm}/{dd}/{sha-256}.eml`; `Inbound::read(&stored)` reads it.

## Patterns and routing

- `support@acme.test` exact (any case); `reply+{token}@...` captures
  (`ctx.param("token")`, never across `@`); `*@in.acme.test`; `*` catch-all.
- Order: exact, then `{name}` patterns, then `*`, each in registration order.
  Recipients tried (`mail.recipients()`): the envelope when the source reports
  one, and only it; without one, `Delivered-To`, `X-Original-To`, `To`, `Cc`,
  which the sender wrote. One mailbox per message.
- Trust: `mail.from` is a claim; `mail.verdicts` (`dmarc == Some(Verdict::Pass)`,
  SES only: Mailgun, Postmark and raw MIME report none) or a reply token
  vouches. It is `None` unless the head is one `From` that is an address or a
  name and an address in angle brackets (no group, list, stray separator or
  bracket, quoted string beside the address, unquoted `@` in the name), with
  line ends that agree.
  Decoded `subject`, names, addresses, file names and ids hold no control
  characters; `headers`, `text` and `html` do keep them. A `filename` may hold
  `../`: use its last component.
- Outcomes: `Ok(())` handled; `Err(ctx.reject("why"))` or a bad token from
  `ctx.verified("token")?` → `rejected` (provider gets 200, no retry); any
  other error → 500, the provider retries; no match → `unrouted`.
  Handlers must be idempotent (`mail.message_id`, `mail.fingerprint`).

## Reply tokens

```rust
let reply_to = tokens.address_for("reply+{token}@in.acme.test", ticket.id)?; // Arc<ReplyTokens>
Message::new().to(email).reply_to(reply_to).subject("Re: ...").text(body);
// in the mailbox:
let ticket = ctx.verified_uuid("token")?;
```

`sign`/`verify` (text), `sign_uuid`/`verify_uuid` (47 chars), `scoped("x")`,
`mail.inbound.reply_token_ttl_days`. Key derived from `APP_KEY`
(`InboundError::MissingKey` without one). Local part max 64
(`AddressTooLong`).

## The message

`InboundMail`: `id`, `source`, `provider_id`, `envelope`, `verdicts`
(`spam`/`virus`/`spf`/`dkim`/`dmarc`, `is_flagged()`), `message_id`,
`in_reply_to`, `references`, `from`/`reply_to`/`to`/`cc` (`Address {name, email}`),
`subject`, `date`, `text`, `html`, `headers`, `attachments`, `size`,
`fingerprint`, `stored`, `received_at`; `header(name)`, `header_all(name)`,
`raw()`, `reply_text()`, `is_auto_reply()`. Forwarded messages:
`attachment.is_message()`, `attachment.as_mail()?`. `Inbound::receive` and
`Inbound::read` parse on the blocking pool (a panicking parser is
`Unparseable`, a 400); `InboundMail::parse`, `Incoming::parse` and
`as_mail` are sync and parse on the caller's thread, so wrap them in
`spawn_blocking` in async code when the bytes are untrusted. The reader is
Navree's own: `mail.inbound.max_parts` (1000), `max_depth` (32 nested
multiparts), `max_headers` (1000 fields) and `max_addresses` (1000 per
address field or `References`) refuse a message as
`Unparseable` while it is read; `Inbound::receive` and `Inbound::read`
apply them, `InboundMail::parse` the defaults. Text attachments are decoded
to UTF-8; unencoded attachments and forwarded messages share the raw
message's buffer.

## Microsoft 365 and Gmail mailboxes

With `integrations` on too: `[integrations.mailboxes] graph_notification_url,
gmail_topic, gmail_token, renew_before_hours`; `MailboxWatches::watch(connection_id,
"security@contoso.example")`. Graph needs `Mail.Read` (application);
Gmail `gmail.readonly` (service account with domain-wide delegation).
Routes `/api/mail/inbound/graph` (validationToken echo, `clientState`
check) and `/api/mail/inbound/gmail?token=` (Pub/Sub push).
`integrations:mailboxes [--renew|--sync <id>]` (renew runs hourly).

## Test

```rust
let fake = MailFake::new();
let received = fake
    .receive(app.ctx(), Incoming::compose().from("ada@example.com").to(address).text("Thanks").build())
    .await?;
assert_eq!(received.status, InboundStatus::Handled);
```

`Incoming::compose()`: `from`, `to`, `cc`, `envelope_to`, `subject`, `text`,
`html`, `header`, `attach`, `inline`, `forward`, `in_reply_to`, `verdicts`.
Fixtures: `Incoming::new("raw", std::fs::read("tests/fixtures/reply.eml")?)`.
Routes: post the provider's payload with the configured secret.

## Common mistakes

- Route 404: its secret isn't set (each source mounts only with one).
- Answering an auto-reply: check `mail.is_auto_reply()` before you reply.
- Expecting every recipient's mailbox to run: one mailbox takes a message.
- Not idempotent: a 500 makes the provider deliver again.
- `mail.inbound.disk` without `StorageModule`, or naming an unknown disk:
  boot error.
- Rotating `APP_KEY` invalidates old reply addresses.
- SES route trusts its URL token and topic list; for SNS signatures use
  `VerifiedWebhook<AwsSns>` and `Inbound::receive_ses`.

## Docs

- https://navree.com/docs/digging-deeper/inbound-mail/
- https://navree.com/docs/packages/integrations/
