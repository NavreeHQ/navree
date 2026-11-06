---
name: navree-webhooks
description: "Webhooks with navree-webhooks: `WebhooksModule` and the `webhooks` config section, outgoing webhooks from events (`WebhookEvent`, `WebhooksModule::for_feature::<E>()`, `m.webhook_type`, `Webhooks::dispatch_json`), endpoints and secrets (`create_endpoint`, `rotate_secret`, `whsec_` secrets), Standard Webhooks signing (`webhook-id`, `webhook-timestamp`, `webhook-signature`), the retry schedule, disabling failing endpoints, the delivery log and the `/api/webhooks` routes, and incoming webhooks verified with `VerifiedWebhook<S>` (`Stripe`, `GitHub`, `Paddle`, `StandardWebhooks`, `AwsSns`, `GooglePubSub`, `MicrosoftGraph`, `Twilio`, `Slack`, `Mailgun`, `Postmark`, `Vonage`, `Meta`, `Hmac`, custom `WebhookVerifier`s with `verify_incoming`, handshakes, `mark_processed`). Use when sending webhooks to customers' URLs, letting users manage webhook endpoints, receiving Stripe, GitHub, Paddle, SES-over-SNS, Pub/Sub or Gmail push, Graph change notifications, Twilio, Slack, Mailgun, Postmark or other signed callbacks, testing deliveries or signatures, or debugging a webhook that is not sent, not retried, refused with 400 `Invalid webhook signature`, or an endpoint that got disabled."
---

# Webhooks

`navree::webhooks` (crate `navree-webhooks`, facade feature `webhooks`, in
`full`, so on by default) does both directions: it **sends** signed webhooks
from your events to endpoints your users register, and it **verifies**
incoming webhooks before your handler runs. Everything below is in the code;
when in doubt read `crates/navree-webhooks/src` and its tests in
`crates/navree-webhooks/tests/webhooks/`.

## Steps: send an event as a webhook

1. Add the module to the app root. Outgoing delivery needs `DatabaseModule`,
   `QueueModule` (with a worker for `webhooks.queue`), `EventsModule` and
   `HttpClientModule`. `CryptoModule` with `APP_KEY` encrypts endpoint
   secrets at rest (without it they are stored as they are, with a warning).
   The routes (on by default) need `AccessModule` and an authenticator:

```rust,ignore
Navree::app()
    .module(ConfigModule::for_root(sources))
    .module(CryptoModule::from_config())
    .module(HttpModule::from_config())
    .module(DatabaseModule::from_config())
    .module(EventsModule::for_root())
    .module(QueueModule::from_config())
    .module(HttpClientModule::from_config())
    .module(AuthModule::from_config())
    .module(AccessModule::for_root(AccessOptions::default()))
    .module(WebhooksModule::from_config()) // section `webhooks`
    .module(DealsModule)
```

2. Implement `WebhookEvent` for the event (it already implements
   `navree::events::Event`; its `NAME` is the webhook type):

```rust,ignore
use navree::BoxFuture;
use navree::prelude::*;
use navree::webhooks::{WebhookContext, WebhookEvent};
use serde_json::{Value, json};
use uuid::Uuid;

impl WebhookEvent for DealWon {
    const DESCRIPTION: &'static str = "A deal moved to won.";

    // Default `None`: app-wide endpoints only. `Some(ws)`: that workspace's endpoints only.
    fn workspace_id(&self) -> Option<Uuid> {
        Some(self.workspace_id)
    }

    // Default: the event itself as JSON. Runs once, on a queue worker.
    fn payload<'a>(&'a self, ctx: &'a WebhookContext) -> BoxFuture<'a, Result<Value, BoxError>> {
        Box::pin(async move {
            let deal = Deal::find(ctx.db(), self.deal_id).await?; // a navree-orm model
            Ok(json!({ "deal": deal }))
        })
    }
}
```

3. Register it in the module that owns the event:

```rust,ignore
#[module(imports = [WebhooksModule::for_feature::<DealWon>()], providers = [DealService])]
pub struct DealsModule;
```

   (or `m.webhook_event::<DealWon>()` inside a `register = Self::wire`
   function; do one or the other, not both, or every event fans out twice).

4. Emit the event as usual (`events.emit(DealWon { .. }).await?`). In a
   request handler the fan-out job joins the request's unit of work, so a
   request that fails sends nothing.

5. Webhooks that are not events: register the name, then dispatch by name.

```rust,ignore
impl ExportsModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.webhook_type("export.ready", "A CSV export finished.");
    }
}

let message_id = webhooks
    .dispatch_json("export.ready", json!({ "export_id": id }))
    .workspace(workspace_id) // omit for app-wide endpoints
    .send()
    .await?;
```

## Endpoints

```rust,ignore
use navree::webhooks::{EndpointChanges, NewEndpoint, Webhooks};

let created = webhooks
    .create_endpoint(&uow, NewEndpoint::new("https://hooks.example.com/navree")
        .events(["deal.won", "deal.lost"]) // registered types, or "*"
        .workspace(workspace_id)
        .description("Revenue dashboard")
        .header("Authorization", "Bearer abc"))
    .await?;
// created.secret ("whsec_...") is shown only here.
webhooks.update_endpoint(&uow, id, EndpointChanges::new().events(["*"])).await?;
let new_secret = webhooks.rotate_secret(&uow, id).await?; // old one signs 24 h more
webhooks.disable(&uow, id).await?; webhooks.enable(&uow, id).await?;
webhooks.delete_endpoint(&uow, id).await?; // drops its deliveries
let attempt = webhooks.send_test(id).await?; // `webhook.test`, at once
```

Checks on create and update (a 400 on the field over HTTP): `https` unless
`webhooks.require_https = false`; no `user:pass@` in the URL; the URL must not
point or resolve to a private, loopback or metadata address
(`http_client.ssrf.allow_hosts` lists exceptions); events non-empty and
registered (`*` = all); headers valid and not reserved (`Host`,
`Content-Type`, `Content-Length`, `User-Agent`, `Connection`,
`Transfer-Encoding`, `TE`, any `webhook-*`). Deleting a workspace deletes its
endpoints.

## What is sent

`POST`, `Content-Type: application/json`, `User-Agent: Navree-Webhooks/1`,
body `{"type","timestamp","data"}` (`timestamp` = when the event happened,
RFC 3339), and:

- `webhook-id`: `msg_<32 hex>`, the same for every endpoint of one event and
  every retry.
- `webhook-timestamp`: Unix seconds of this attempt (from `dyn Clock`).
- `webhook-signature`: `v1,<base64 HMAC-SHA256 of "{id}.{timestamp}.{body}">`
  keyed with the base64-decoded bytes after `whsec_`; two entries separated
  by a space for 24 h after `rotate_secret`.

## Retries, disabling, the log

- Attempt `n + 1` waits `RETRY_SCHEDULE[n]` after attempt `n`: 0, 5 s, 5 min,
  30 min, 2 h, 5 h, 10 h, 10 h (`MAX_ATTEMPTS` = 8, about 27 h). Success = 2xx
  within `webhooks.timeout_secs`. The delivery job re-queues itself with
  that delay; the queue's own retries only cover crashes and database
  errors.
- After the last attempt: status `failed`, event `WebhookDeliveryFailed`
  (`webhook.delivery_failed`).
- `410 Gone`: endpoint disabled at once (`disabled_reason = "gone"`).
  Failing with no success for `webhooks.disable_after_days` (5; 0 = never):
  disabled (`"failing"`). Both emit `WebhookEndpointDisabled`
  (`webhook.endpoint_disabled`). Pending deliveries of a disabled endpoint
  fail when they come due. `enable` clears the failure clock.
- `webhooks.deliveries(endpoint_id, PageQuery)` (newest first),
  `delivery(id)`, `attempts(delivery_id)` (status code, first 4 KB of the
  body, error, duration). `retry(delivery_id)` gives a **failed** delivery
  one more attempt (else `NotRetryable`, 409); `retry_failed()` all of them.
- Test messages (`send_test`) are logged but never touch endpoint health.

## Routes

`webhooks.routes = true` (default) and `DatabaseModule` in the app mount
`/api/webhooks/...`, all with permission `webhooks.manage`:
`GET event-types`, `GET|POST endpoints`, `GET|PATCH|DELETE endpoints/{id}`
(`PATCH {url?, events?, description?, headers?, enabled?}`),
`POST endpoints/{id}/rotate-secret`, `POST endpoints/{id}/test`,
`GET endpoints/{id}/deliveries`, `POST deliveries/{id}/retry`. With
`WorkspaceModule` in the app they run behind `InWorkspace::guard()` and act
on the request's workspace (`X-Workspace`), and the permission is checked
in that workspace: grant it with
`[workspace.roles.admin] permissions = ["webhooks.manage"]` (owners have
it). Without AccessModule and an authenticator, set `webhooks.routes =
false` or the boot fails.

## Steps: verify an incoming webhook

1. Put the secret in `.env`:
   `APP_WEBHOOKS__INCOMING__GITHUB__SECRET=...` (source names: `stripe`,
   `github`, `paddle`, `standard`, `microsoft_graph` (the `clientState`),
   `twilio` (auth token), `slack` (signing secret), `mailgun` (webhook
   signing key), `postmark` (`username:password`), or your own). `aws_sns`
   and `google_pubsub` have no secret; they need `topics` and
   `service_account` (step 4).
2. Take `VerifiedWebhook<S>` as the **last** argument of a `#[public]`
   route; dedupe with `mark_processed` in the same unit of work:

```rust,ignore
use navree::webhooks::{GitHub, VerifiedWebhook};

#[post("/")]
#[public]
async fn receive(&self, uow: Uow, hook: VerifiedWebhook<GitHub>) -> Result<NoContent> {
    if !hook.mark_processed(&uow).await? {
        return Ok(NoContent); // already handled
    }
    if hook.event_type() == Some("issues") {
        let event: IssueEvent = hook.json()?; // 400 when it doesn't match
        // ...
    }
    Ok(NoContent)
}
```

3. A sender of your own:

```rust,ignore
pub struct MailEvents;
impl WebhookSource for MailEvents {
    const NAME: &'static str = "mail_events"; // webhooks.incoming.mail_events.secret
    type Verifier = StandardWebhooks;          // or Stripe, GitHub, Paddle, Hmac
}
```

   A custom HMAC scheme uses `type Verifier = Hmac;` plus config:

```toml
[webhooks.incoming.calendly]
secret = ""                     # APP_WEBHOOKS__INCOMING__CALENDLY__SECRET
header = "Calendly-Webhook-Signature"
algorithm = "sha256"            # sha1 | sha256 | sha512
encoding = "hex"                # hex | base64
format = "t={timestamp},v1={signature}"   # {signature} once, {timestamp} at most once
signed = "{timestamp}.{body}"            # {timestamp}, {id}, {body}
# timestamp_header, id_header, event_header, tolerance_secs: optional
```

| Source | Header | Signed | Key | `id()` / `event_type()` |
|---|---|---|---|---|
| `Stripe` | `Stripe-Signature: t=..,v1=<hex>` | `{t}.{body}` | secret as is | body `id` / `type` |
| `GitHub` | `X-Hub-Signature-256: sha256=<hex>` | body | secret as is | `X-GitHub-Delivery` / `X-GitHub-Event` |
| `Paddle` | `Paddle-Signature: ts=..;h1=<hex>` | `{ts}:{body}` | secret as is | body `event_id` / `event_type` |
| `StandardWebhooks` | `webhook-signature: v1,<b64>` | `{id}.{ts}.{body}` | base64 after `whsec_` | `webhook-id` / body `type` |

| `AwsSns` | body `Signature` (RSA, v1 SHA-1 / v2 SHA-256) | `SnsMessage::string_to_sign()` | cert at `SigningCertURL` (SNS host, `.pem`, cached 24 h) | `MessageId` / `Type` |
| `GooglePubSub` | `Authorization: Bearer <OIDC JWT>` | RS256 JWT | Google's JWKS (`GooglePubSub::KEYS_URL`, cached) | `message.messageId` / none |
| `MicrosoftGraph` | body `value[].clientState` | (nothing signed) | `clientState` = secret | body SHA-256 / `lifecycleEvent` or `changeType` |
| `Twilio` | `X-Twilio-Signature: <b64>` | URL + sorted form params (or URL with `bodySHA256`) | auth token, HMAC-SHA1 | `I-Twilio-Idempotency-Token` / none |
| `Slack` | `X-Slack-Signature: v0=<hex>` | `v0:{X-Slack-Request-Timestamp}:{body}` | signing secret | `event_id` / `event.type` or `type` |
| `Mailgun` | body `signature` (JSON object or form fields) | `{timestamp}{token}` | webhook signing key | `event-data.id` or token / `event-data.event` |
| `Postmark` | `Authorization: Basic` | (nothing signed) | `username:password` | `MessageID` / `RecordType` |
| `Vonage` | `Authorization: Bearer <HS256 JWT>` | JWT; `payload_hash` = body SHA-256, `iat` in tolerance | signature secret | `jti` / body `status` or `message_type` |
| `Meta` | `X-Hub-Signature-256: sha256=<hex>` | body | app secret (+ `verify_token` for the `GET hub.challenge` handshake) | body SHA-256 / body `object` |

With the crate feature `notify` (facade `webhooks` + `notify`), configured
`twilio`, `vonage` and `meta` sources also get `POST /api/webhooks/notify/twilio`,
`/vonage` and `GET`/`POST /meta`, which apply SMS/WhatsApp statuses and STOP
replies to navree-notify's delivery log (see `navree-notification-channels`).

`id()` falls back to the hex SHA-256 of the body. GitHub, Twilio, Postmark,
Meta and Graph have no timestamp (`timestamp()` is `None`), so no age check; the
SNS `Timestamp` must be within the source's own `tolerance_secs`, else an hour
(SNS retries a delivery for up to an hour; an older message is a replay).
Answers: 400 `Invalid webhook signature` (bad, missing or old signature,
wrong topic, audience or account; the reason is logged at debug), 413 over
`webhooks.incoming.max_body_bytes`, 503 when an SNS certificate or Google's
keys cannot be fetched (the sender retries), 500 when the source lacks its
secret or required setting (the log names the key). A source table with an
empty secret fails the boot unless it sets `topics` or `service_account`.

4. Keyless senders (AWS SNS, Google Pub/Sub) fetch the sender's public keys
   with the app's `HttpClient` (a default one without `HttpClientModule`).
   The setting that ties requests to *your* subscription is required:

```toml
[webhooks.incoming.aws_sns]
topics = ["arn:aws:sns:eu-west-1:123456789012:ses-events"] # env: comma-separated
auto_confirm = true   # SubscriptionConfirmation confirmed, UnsubscribeConfirmation acked, handler not run

[webhooks.incoming.google_pubsub]
service_account = "gmail-push@project.iam.gserviceaccount.com"
audience = "https://api.example.com/api/v1/mail/gmail-push" # default: public URL (`url`, else rebuilt)
```

```rust,ignore
let message: SnsMessage = hook.json()?;           // SES event: message.message_json::<SesEvent>()
let push: PubSubPush = hook.json()?;               // Gmail: push.message.data_json::<GmailChange>()
let batch: GraphNotifications = hook.json()?;      // batch.value[i].resource / lifecycle_event
let status: TwilioStatus = hook.form()?;           // form bodies: Twilio, Slack commands, Mailgun routes
```

5. Handshakes are answered by the extractor (200 `text/plain`); the handler
   never runs: Graph `?validationToken=` (echoed), Slack
   `url_verification` (its `challenge`), SNS confirmations (see
   `auto_confirm`). Twilio signs the URL it called: behind a proxy set
   `webhooks.incoming.twilio.url` to the public route URL (the query string
   is appended), or make the proxy send `X-Forwarded-Proto` and
   `X-Forwarded-Host`. Leave SNS raw message delivery off (raw messages are
   unsigned).

6. A scheme of your own that needs the URL, fetched keys or a handshake:
   implement `WebhookVerifier::verify_incoming(&IncomingRequest) ->
   Result<Verdict, VerifyError>` (`request.query("x")`, `request.url()`,
   `request.keys().await?`), returning `Verdict::Verified(..)` or
   `Verdict::Handshake(Handshake::text(..))`; override `missing_setting`
   when the required key is not `secret`.

## Testing templates

Outgoing (fake client + memory queue + fixed clock):

```rust,ignore
let (client, server) = HttpClient::fake();
server.on(Method::POST, "https://hooks.example.com/*", FakeResponse::new(500));
let clock = Arc::new(FixedClock::new(start));
let app = TestApp::builder()
    .temp_database()
    .config("queue.driver", "memory")
    .config("webhooks.routes", false)
    .module(DatabaseModule::from_config())
    .module(EventsModule::for_root())
    .module(QueueModule::from_config())
    .module(HttpClientModule::from_config())
    .module(WebhooksModule::from_config())
    .module(DealsModule)
    .override_value(Arc::new(client))
    .override_value::<dyn Clock>(clock.clone())
    .build()
    .await;
// create an endpoint in a committed uow, emit, then:
app.get::<Queue>().await.run_pending().await.unwrap();      // 1st attempt (500)
clock.advance(RETRY_SCHEDULE[1]);
app.get::<Queue>().await.run_pending().await.unwrap();      // 2nd attempt
let request = server.recorded().pop().unwrap();
StandardWebhooks::verify(&created.secret, request.headers(), request.body(), None).unwrap();
```

Without sending: `override_value(Arc::new(fake.queue()))` with a
`QueueFake`, then `fake.assert_dispatched::<FanOut>(|job| job.event_type == "deal.won")`.

Incoming: sign like the sender does, then post.

```rust,ignore
.config("webhooks.incoming.github.secret", "test-secret")
// ...
app.client().post("/api/v1/integrations/github")
    .header("X-Hub-Signature-256", GitHub::sign("test-secret", &body))
    .body(body).send().await.assert_status(204);
// Stripe::sign(secret, ts, body), Paddle::sign(secret, ts, body),
// StandardWebhooks::sign(secret, id, ts, body), Hmac::sign(&source_options, Some(ts), None, body),
// Twilio::sign(token, public_url, &params), Slack::sign(secret, ts, body),
// Mailgun::sign(key, ts, token), Postmark::authorization("user:pass")
```

Pub/Sub: serve an RSA test key as Google's JWKS and sign tokens with it
(`SigningKey::from_pem(rsa_pem)?.with_kid("k")`, then `key.sign_jwt(&claims)`
with `iss`, `aud`, `email`, `email_verified`, `iat` and `exp`):

```rust,ignore
let (client, server) = HttpClient::fake();
let keys: JwkSet = key.public_jwk().into_iter().collect();
server.on(Method::GET, GooglePubSub::KEYS_URL, FakeResponse::new(200).json(&keys));
// .override_value(Arc::new(client)), then .bearer(&token) on the request
```

SNS: serve a test certificate (PEM) at the message's `SigningCertURL`
(`https://sns.<region>.amazonaws.com/....pem`) and sign
`message.string_to_sign()` with its RSA key (`key.sign(..)` gives version
2), base64 into `Signature`. Fixtures signed by OpenSSL, v1 and v2, are in
`crates/navree-webhooks/tests/fixtures/sources/`.

## Checks before you finish

- [ ] Each webhook event is registered exactly once (`for_feature` or `webhook_event`).
- [ ] A worker runs `webhooks.queue` (`navree queue:work --queue webhooks`).
- [ ] `APP_KEY` is set and `CryptoModule` is in the app (secrets encrypted).
- [ ] Routes: `AccessModule` + authenticator present, or `webhooks.routes = false`.
- [ ] Incoming routes are `#[public]`, take `VerifiedWebhook<S>` last, and call `mark_processed` in the same `Uow` as their writes.
- [ ] Every incoming secret comes from `.env`, never from code or the repository.
- [ ] `aws_sns` lists its `topics`; `google_pubsub` names its `service_account` (and `audience` when the subscription sets one).
- [ ] Twilio behind a proxy: `webhooks.incoming.twilio.url` is the URL configured in Twilio.

## Gotchas

- `serde_json::Value` sorts keys; the bytes sent keep `type`, `timestamp`,
  `data` order. Verify the raw body (`request.body()`), never a re-serialized
  one.
- `VerifiedWebhook` consumes the body: no `Json<T>` next to it.
- SNS posts JSON with `Content-Type: text/plain`; `hook.json()` reads it anyway.
- Graph wants a `202` within 3 s, Slack an answer within 3 s: queue the work.
- A workspace event never reaches app-wide endpoints, and an app-wide event
  never reaches workspace endpoints.
- A changed `APP_KEY` without the old one in `app.previous_keys` makes stored
  secrets undecryptable (deliveries fail with a crypto error).
- `webhooks:prune [--days N]` deletes deliveries (after `retain_days`) and
  receipts (after `incoming.receipt_days`); schedule it daily. Other
  commands: `webhooks:endpoints [--workspace <slug>]`,
  `webhooks:test <endpoint-id>`, `webhooks:retry <delivery-id>|failed`.
