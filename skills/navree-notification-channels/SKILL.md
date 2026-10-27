---
name: navree-notification-channels
description: "Sends Navree notifications over Slack (webhooks, bot tokens), Microsoft Teams (webhooks, proactive bot messages), SMS (Twilio, Vonage, Amazon SNS), WhatsApp (Twilio, the WhatsApp Cloud API) and browser push (VAPID): `route_*` on the notifiable, `to_*` on the notification, the `notify.*` driver tables, E.164 numbers, STOP opt-outs (`OptOuts`), push subscriptions (`notify:vapid`, `/api/notifications/push-subscriptions`), the delivery log (`notification_deliveries`, `DeliveryLog`), provider callbacks (`/api/webhooks/notify/*`), connection tokens from integrations and the channel fakes. Use when texting users, posting to Slack or Teams, sending WhatsApp messages or push notifications, handling STOP replies or delivery statuses, or testing any of these."
---

# Notification channels

The database, mail and broadcast channels are in
`navree-mail-and-notifications`. This skill covers the other five, all in the
`notify` facade feature (in `full`): `Channel::Slack`,
`Channel::MicrosoftTeams`, `Channel::Sms`, `Channel::WhatsApp`,
`Channel::WebPush`. They go out after the request commits (or as
`notify.send` jobs with `notify.queue`), like mail.

## Steps: add a channel to a notification

1. Route the notifiable: implement `route_sms`/`route_whatsapp` (a number in
   any format), `route_slack` (`SlackRoute::webhook(url)` or
   `SlackRoute::channel("C0123")`) or `route_teams`
   (`TeamsRoute::webhook(url)` or `TeamsRoute::conversation(service_url,
   conversation_id)`). Defaults are `None`, which skips the channel. Web push
   needs no route: it sends to the notifiable's saved push subscriptions.
2. Render: add the channel to `via` and implement `to_sms`
   (`SmsMessage::new(body)`), `to_whatsapp` (`WhatsAppMessage::text(body)` or
   `WhatsAppMessage::template(WhatsAppTemplate::new(name, "en_US").parameter(v))`),
   `to_slack` (`SlackMessage::new(text).blocks(json!([...]))`), `to_teams`
   (`TeamsMessage::new(text).card(json!({...}))`) or `to_web_push`
   (`WebPushMessage::new(title).body(..).url(..)`). A message's own `.to(..)`
   wins over the notifiable's route.
3. Configure the driver (secrets in the environment):
   - SMS: `[notify.sms] driver = "twilio" | "vonage" | "sns" | "log"`, `from`,
     `default_region = "US"` (numbers without a country code), plus
     `[notify.twilio] account_sid` + `APP_NOTIFY__TWILIO__AUTH_TOKEN`, or
     `[notify.vonage] api_key/api_secret`, or `[notify.sns] region/access_key_id/secret_access_key`.
   - WhatsApp: `[notify.whatsapp] driver = "twilio" | "meta" | "log"`, `from`
     (Twilio), or `[notify.meta] phone_number_id` + `APP_NOTIFY__META__ACCESS_TOKEN`.
   - Slack channel routes: `APP_NOTIFY__SLACK__BOT_TOKEN=xoxb-...`.
   - Teams conversations: `[notify.teams] app_id` + `APP_NOTIFY__TEAMS__APP_PASSWORD`
     (`tenant` defaults to `botframework.com`).
   - Web push: `navree notify:vapid`, then `[notify.web_push] public_key`,
     `subject = "mailto:..."` + `APP_NOTIFY__WEB_PUSH__PRIVATE_KEY`.
   A configured driver without its credentials fails the boot naming the keys.
4. Send as usual: `notifier.send(&user, notification).await?`.

## Web push in the browser

```ts
const { public_key } = await unwrap(api.GET("/api/notifications/push-key"));
const registration = await navigator.serviceWorker.register("/sw.js");
const subscription = await registration.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: public_key });
await unwrap(api.POST("/api/notifications/push-subscriptions", { body: subscription.toJSON() }));
```

The service worker gets `{title, body?, icon?, url?, tag?, data?}` in
`event.data.json()`. `DELETE /api/notifications/push-subscriptions`
`{endpoint}` unsubscribes. A push service's 404/410 deletes the subscription.
Routes follow `notify.routes` (authenticated). In code: `PushSubscriptions`
(`subscribe`, `unsubscribe`, `of`, `delete`).

## Opt-outs and statuses

- Replies STOP/STOPALL/UNSUBSCRIBE/CANCEL/END/QUIT/OPTOUT/REVOKE opt a
  number out of that channel; START/YES/UNSTOP/OPTIN opt it back in. Twilio
  21610 and Cloud API 131050 refusals opt out too. Opted-out sends are
  skipped and recorded `opted_out`; `send` still succeeds.
- `Arc<OptOuts>`: `opt_out(Channel::Sms, number, OptOutSource::Manual)`,
  `opt_in`, `is_opted_out`, `list`. Numbers are normalized to E.164.
- Each delivery is a `NotificationDelivery` row (`notification_deliveries`:
  channel, provider, recipient, `provider_message_id`, status
  `sent|delivered|read|failed|opted_out`, error). Query it with the ORM;
  `Arc<DeliveryLog>` has `for_notification`, `find_by_message`, `ingest`.
- Provider callbacks: with the `webhooks` feature, configure
  `[webhooks.incoming.twilio|vonage|meta] secret` (Meta also `verify_token`)
  and point the provider at `/api/webhooks/notify/twilio|vonage|meta`
  (`notify.sms.status_callback` for Twilio/Vonage statuses). Or write your own
  route: `VerifiedWebhook<Twilio>` then
  `log.ingest(ChannelCallback::from_twilio(hook.body())).await?`.

## Connections (multi-tenant bots)

`SlackRoute::channel(c).connection(id)` / `TeamsRoute::conversation(u, c).connection(id)`
send with an integrations connection's token: `IntegrationsModule` registers
`dyn ConnectionTokens` when `integrations` and `notify` are both on.

## Test

```rust
let sms = SmsFake::new();
let app = TestApp::builder()
    .temp_database()
    .config("notify.routes", false)
    .config("notify.sms.default_region", "US")
    .module(DatabaseModule::from_config())
    .module(NotifyModule::from_config())
    .override_value::<dyn SmsDriver>(sms.driver())
    .build()
    .await;
// ... send ...
assert_eq!(sms.sent_to("+14155550100")[0].body, "...");
sms.deliver(app.ctx(), "+14155550100").await?;          // status callback
sms.reply(app.ctx(), "+14155550100", "STOP").await?;    // inbound STOP
```

Fakes: `SlackFake`, `TeamsFake`, `SmsFake`, `WhatsAppFake`, `WebPushFake`
(`driver()`, `sent()`, `assert_sent`, `refuse`; SMS/WhatsApp `sent_to`,
`opt_out_at_provider`, `deliver`, `fail`, `reply`, WhatsApp `read`; push
`expire(endpoint)`). Install every channel `via` lists, or the missing ones
fail with `NotifyError::Unavailable`. Real drivers test against wiremock by
setting the table's `endpoint`.

## Common mistakes

- A national number without `notify.sms.default_region`: `InvalidRecipient`.
- Free-text WhatsApp outside the 24-hour window: send a template.
- A new VAPID pair invalidates every browser subscription.
- Behind a proxy, Twilio signs its public URL: set
  `webhooks.incoming.twilio.url` or forward `X-Forwarded-Proto`/`-Host`.
- Webhook URLs (Slack, Teams) are secrets: the log records only their host.

## Docs

- https://navree.com/docs/digging-deeper/notification-channels/
- https://navree.com/docs/digging-deeper/notifications/
- https://navree.com/docs/packages/webhooks/#notification-callbacks
