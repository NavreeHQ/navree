---
name: navree-mail-delivery
description: "Delivers mail for a multi-tenant Navree app: the SES, Mailgun, Postmark and Resend transports, named transports (`mailer.via`), transports per sending domain, workspace and organization (`TransportResolver`, `mailer.for_scope`), the delivery log fed by provider webhooks (`Deliveries`, `MailBounced`), suppressions, mail held for approval (`Outbox`), open and click tracking, DKIM signing and SPF/DKIM/DMARC checks (`mail:domains`). Use when sending through an email API, letting customers bring their own mail provider or domain, handling bounces and complaints, logging or tracking mail, holding mail for approval, or checking a sending domain."
---

# Mail delivery

Rendering and the plain transports are in `navree-mail-and-notifications`.
This skill covers what `navree-mail` adds for products that send mail for
their customers. All of it is in the `mail` facade feature.

## Steps: send through an email API

1. Set the transport, in the environment (secrets stay out of TOML):
   `APP_MAIL__TRANSPORT=ses://KEY_ID:SECRET@eu-west-1?configuration_set=events`
   (`%2F` for `/` and `%2B` for `+` in the secret; `ses://eu-west-1` reads
   `AWS_ACCESS_KEY_ID`/`AWS_SECRET_ACCESS_KEY`). Others:
   `mailgun://API_KEY@mg.acme.test?region=eu`,
   `postmark://TOKEN@api.postmarkapp.com?stream=outbound`,
   `resend://API_KEY@api.resend.com`.
2. More transports go under `[mail.transports.<name>]`; send through one with
   `mailer.via("campaigns").queue(&mail).await?`.
3. `mailer.deliver(&mail).await?` returns a `SendReport` (`status`,
   `transport`, `tier`, `provider_id`, `delivery_id`, `suppressed`).

## Steps: the delivery pipeline

1. Root modules: `CryptoModule` (encrypts stored transports and held mail
   with `APP_KEY`), `DatabaseModule`, `EventsModule`, `ViewModule`,
   `MailModule`. Migration set `mail` creates `mail_transports`,
   `mail_deliveries`, `mail_events`, `mail_suppressions`, `mail_outbox`.
2. `[mail.delivery] enabled = true` (boot fails without `DatabaseModule`).
   Keys: `suppress`, `auto_suppress`, `tenants`, `routes` (all default true).
3. Send for a tenant: `mailer.for_scope(&workspace.mail_scope()).queue(&mail)`
   (`CurrentWorkspace::mail_scope()`, `CurrentOrganization::mail_scope()`,
   `MailScope::from(&workspace)`, `MailScope::workspace(id).with_organization(id)`).
4. Provider webhooks: set a secret per provider and point the provider at
   `/api/mail/webhooks/<provider>`:
   `mail.webhooks.ses.token` (+ `topic_arns`; URL `...?token=`),
   `mail.webhooks.mailgun.signing_key`, `mail.webhooks.postmark.token`
   (basic-auth password or `?token=`), `mail.webhooks.resend.secret`
   (`whsec_...`). Bad signature: 401 `invalid_signature`.
5. React to `MailDelivered`, `MailBounced` (`permanent`, `detail`),
   `MailComplained`, `MailOpened`, `MailClicked` (`url`) with
   `m.listen::<MailBounced, MyListener>()`.

## Tenant transports

```rust
use navree::mail::{TransportConfig, TransportOwner, TransportResolver};

// body: a transport's table form, e.g. {"type": "smtp", "url": "smtps://..."}
transports.usable(&config).await?;              // connects/authenticates; 422 mail_transport_unusable
let stored = transports
    .set(&uow, TransportOwner::workspace(ws.id()), &config) // encrypted, replaces, enables
    .await?;
```

Tiers, most specific first: `domain` (sender's domain, owned by the
scope's workspace/organization or by nobody: `TransportOwner::domain`,
`workspace_domain`, `organization_domain`), `workspace`, `organization`,
`platform` (`TransportOwner::Platform`), then `config`. `via` skips the
stored tiers. Tenant tiers refuse `endpoint` and SMTP hosts on private
networks. Also `get`, `list`, `config(id)`, `set_enabled`, `remove`,
`check_stored(id)`.

## Log, suppressions, outbox

- `Deliveries`: `find`, `list(&DeliveryFilter { workspace_id, status, recipient, template }, page)`,
  `events(id)`, `record(ProviderEvent)` (for your own sources;
  `ProviderEvent::from_ses/from_mailgun/from_postmark/from_resend`). The log
  stores a SHA-256 of the first recipient, not the address.
- `Suppressions`: `add(&uow, Some(ws), email, SuppressionReason::Unsubscribed, None)`
  (idempotent), `is_suppressed(Some(ws), email)`, `remove`, `remove_email`,
  `list`. `None` workspace = everywhere. Hard bounces and complaints add one
  automatically.
- Hold: `TemplateMail::new(..).hold("reason")` (`Mailable::hold`) or
  `Message::hold`; `Outbox::list/find/message/release(id, by)/discard(id, by)`;
  release or discard of a non-held message is `MailError::Conflict` (409).
- Admin routes (permission `mail.manage`): `/api/mail/deliveries`,
  `/deliveries/{id}`, `/suppressions` (GET, POST, DELETE `{id}`), `/outbox`,
  `/outbox/{id}/release`, `/outbox/{id}/discard`, `/transports/usable`.

## Tracking, DKIM, domains

- `[mail.tracking] opens = true, clicks = true, base_url = "https://links.acme.test"`
  (default `app.url`; needs the pipeline). Links are signed with the app's
  `UrlSigner`; `data-no-track` on an `<a>` skips it.
- DKIM for SMTP: `[mail.transport.dkim] domain, selector, private_key`
  (RSA ≥ 2048 or Ed25519 PEM). `DkimSigner::from_pem(..)?.dns_record()`
  prints the TXT record for `record_name()`.
- `navree mail:domains acme.test --selector mail --spf-include amazonses.com`
  (or `SendingDomains::check(domain, &DomainExpectations::new().selector(..))`).
- `navree mail:test ops@acme.test --workspace <id>`,
  `navree mail:suppressions add|remove|list <email> [--workspace <id>]`.

## Test

```rust
let fake = MailFake::new();
let app = TestApp::builder()
    .temp_database()
    .config("app.key", navree::crypto::generate_key())
    .config("mail.delivery.enabled", true)
    .config("mail.delivery.routes", false)   // the admin routes need AccessModule
    .module(CryptoModule::from_config())
    .module(DatabaseModule::from_config())
    .module(ViewModule::from_config())
    .module(MailModule::from_config())
    .override_value::<dyn MailTransport>(fake.transport())
    .build()
    .await;
// ... send ...
fake.bounce(app.ctx(), "ada@example.com").await?;   // also deliver, soft_bounce, complain, open, click
assert!(app.get::<Suppressions>().await.is_suppressed(None, "ada@example.com").await?);
```

API transports: set `endpoint` to a wiremock server (its host is let
through the SSRF policy). Domain checks: override `dyn DnsResolver` with
`StaticResolver::new().record(name, txt)`.

## Common mistakes

- `hold`, the log or tracking without `mail.delivery.enabled`
  (`MailError::Missing`, or a boot error for tracking).
- No `APP_KEY`: tenant transports and held mail can't be stored.
- Expecting `for_scope` to resolve tenant transports with
  `mail.delivery.enabled = false`: the scope is then ignored.
- Webhook routes 404: their secret isn't set, or the pipeline is off.
- Boot error "declares permissions:mail.manage but no `dyn RouteAuthorizer`":
  the admin routes need `AccessModule` (and an authenticator); set
  `mail.delivery.routes = false` where the app has none.
- Signed tracking links break when `http.signing_key`/`app.key` changes.

## Docs

- https://navree.com/docs/digging-deeper/mail-delivery/
- https://navree.com/docs/digging-deeper/mail/
