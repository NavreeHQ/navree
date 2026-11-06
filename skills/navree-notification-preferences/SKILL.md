---
name: navree-notification-preferences
description: "Lets people and workspaces choose which Navree notifications reach them and when: categories (`#[notification(category = .., digest = .., urgent)]`, `NotifyModule::for_feature`), channel switches per category in `Preferences` (`NotificationPreferences`, `GET`/`PATCH /api/notifications/preferences` and `/workspace`), required categories, digests that batch mail (hourly, daily, weekly at 09:00 on the recipient's clock), quiet hours, send windows (`Notification::send_window`), held deliveries (`notification_deferrals`, `notify:release`), the recipient's zone and workspace (`Notifiable::time_zone`, `workspace_id`) and per-workspace mail templates and style (`notification-templates` settings, `TemplateOverrides`). Use when building a notification preference screen, batching mail into a digest, not texting people at night, sending reminders in business hours, letting tenants restyle notification mail, or testing any of these with a fixed clock."
---

# Notification preferences

All in the `notify` facade feature (in `full`); the workspace templates also
need `settings`. Preferences need `DatabaseModule` (tables
`notification_preferences`, `notification_deferrals`); without it everything
is delivered at once.

## Steps: give notifications categories and digests

1. Set the category, digest and urgency on the impl block:

   ```rust,ignore
   use navree::notify::{Channel, Notifiable, Notification, notification};

   #[notification(category = "deals", digest = "daily")]
   impl Notification for DealWon {
       const TYPE: &'static str = "deal.won";
       fn via(&self, _n: &dyn Notifiable) -> Vec<Channel> {
           vec![Channel::Database, Channel::Mail, Channel::Sms]
       }
       // to_database, to_mail, to_sms ...
   }

   #[notification(category = "security", urgent)] // skips quiet hours and digests
   impl Notification for SignInCode { /* ... */ }
   ```

   Or by hand: `const CATEGORY: &'static str = "deals";`,
   `const DIGEST: Digest = Digest::Daily;`, `const URGENT: bool = true;`.
   The category defaults to `TYPE`; `digest` is `off|hourly|daily|weekly`.
2. Register each type in its module so the routes list and accept its
   category: `#[module(imports = [NotifyModule::for_feature::<DealWon>()])]`
   or `m.notification::<DealWon>()` (`ModuleBuilderNotifyExt`).
3. Make categories that must always arrive required:
   `[notify.preferences] required = ["security"]`.
4. Give the notifiable its workspace and zone when you have them:
   `fn workspace_id(&self) -> Option<Uuid>` (default: the current unit of
   work's workspace) and `fn time_zone(&self) -> Option<String>` (default:
   `TimeModule`'s user zone, else the workspace's, else
   `time.default_zone`, else UTC).
5. Schedule nothing: `NotifyModule` schedules `notify:release` every
   minute; run the scheduler (`schedule:work`) in production.

Digests batch **mail only**; the other channels deliver as usual. The digest
goes out at the next `notify.digest.at` (`09:00`) on the recipient's clock
(weekly on `notify.digest.weekly_on`, hourly at the top of the hour) as one
mail rendered with `notify/digest` (`items`, `count`, `digest`, `style`;
override with `views/notify/digest.html`/`.txt`).

## Send windows and quiet hours

```rust,ignore
fn send_window(&self, _n: &dyn Notifiable) -> Option<SendWindow> {
    Some(SendWindow::weekdays().between("09:00", "17:00"))
}
```

Outside the window, or inside the recipient's `quiet_hours`, the channels in
`notify.deferred.channels` (mail, slack, teams, sms, whatsapp, web_push)
wait; database and broadcast deliver at once. `URGENT` skips quiet hours and
digests, not the window or the channel switches.

## Preferences

One JSON document per owner; what a person leaves out comes from their
workspace, then the notification:

```json
{ "channels": { "sms": false },
  "categories": { "deals": { "channels": { "mail": true }, "digest": "weekly" } },
  "digest": "daily",
  "quiet_hours": { "start": "22:00", "end": "07:00" } }
```

- Order for (category, channel): the person's category switch, their
  all-categories switch, the workspace's category switch, its
  all-categories switch, on. Required categories are always on.
- `broadcast` follows `database`; it is not switchable.
- A start equal to the end is "no quiet hours" (overrides the workspace's).

Routes (signed in): `GET /api/notifications/preferences` →
`{data, workspace, categories: [{category, types, required, digest,
channels}], channels, time_zone}`; `PATCH` takes a JSON merge patch (`null`
removes a choice) and answers the same; refusals are `400
validation_failed` with `errors["categories.security.channels.sms"]`.
`GET`/`PATCH /api/notifications/preferences/workspace` act on the request's
workspace (404 outside one; `PATCH` needs
`notify.preferences.workspace_permission`, `workspace.update`).

In code (`Arc<NotificationPreferences>`):

```rust,ignore
let owner = PreferenceOwner::user(user_id); // or ::workspace(id), ::of(&notifiable)
preferences.update(&owner, &json!({"channels": {"sms": false}})).await?;
let resolved = preferences.resolve(&member, Some(workspace_id)).await?;
resolved.allows("deals", Channel::Sms); resolved.digest("deals", Digest::Off);
```

`of`, `update`, `set`, `forget`, `resolve`; errors are
`NotifyError::InvalidPreferences(fields)`.

## Held deliveries

Rows of `notification_deferrals` (`NotificationDeferral`: `reason`
`digest|quiet_hours|send_window`, `release_at`, `attempts`). They join the
unit of work. `notify:release` / `Notifier::release_due()` sends what is due
(digest rows per recipient and frequency as one mail; as `notify.send` jobs
with `notify.queue`), drops channels turned off since, retries after
`notify.deferred.retry_secs` and drops after `max_attempts`. Claims are per
row, so several schedulers are safe.

## Workspace mail templates

With `settings` on, `PATCH /api/settings/notification-templates?scope=workspace`
(or `organization`, or global):

```json
{ "logo_url": "https://cdn.acme.test/logo.png", "brand_color": "#0F766E",
  "footer": "Agra Corp, Pune", "style": { "accent": "#F59E0B" },
  "templates": { "deal.won": { "subject": "We won {{ title }}!", "html": "{% extends \"layouts/mail.html\" %}..." } } }
```

Every notification mail gets `style` (`{{ style.brand_color }}`); an override
replaces the subject or a body of one type (`notify.digest` for the digest),
renders with the mail's variables, keeps the parts it leaves out and is
skipped with a warning if it fails. Mail of a workspace notification goes
through the workspace's mail transport. Elsewhere, register your own
`Arc<dyn TemplateOverrides>` returning `NotificationOverrides`.

## Test

```rust,ignore
let clock = Arc::new(FixedClock::new("2026-10-28T20:00:00Z".parse()?)); // 01:30 in Kolkata
let app = TestApp::builder()
    .temp_database()
    .config("mail.transport", "array")
    .config("notify.routes", false)
    .module(DatabaseModule::from_config())
    .module(ViewModule::from_config())
    .module(MailModule::from_config())
    .module(NotifyModule::from_config())
    .override_value::<dyn Clock>(clock.clone())
    .override_value::<dyn SmsDriver>(sms.driver())
    .build()
    .await;
notifier.send(&priya, deal_won).await?;              // the mail waits
clock.set("2026-10-29T03:30:00Z".parse()?);           // 09:00 in Kolkata
let released = notifier.release_due().await?;        // one digest mail
```

Read held rows with `NotificationDeferral::query().get(&db)`; store
preferences with `NotificationPreferences::update`. `temp_database()` is
SQLite: the app's `[dev-dependencies]` turn on the `sqlite` feature of
`navree`.

## Common mistakes

- Expecting a digest for SMS or Slack: only mail is batched.
- `urgent` with a digest: a compile error.
- Unregistered categories are accepted only while no type is registered;
  once one is, `PATCH` refuses unknown categories.
- No scheduler running in production: held deliveries never go out. Run
  `schedule:work`, or call `notify:release` from your own scheduler with
  `notify.deferred.schedule = false`.
- Writing `broadcast` in `channels`: refused; switch `database` instead.

## Docs

- https://navree.com/docs/digging-deeper/notification-preferences/
- https://navree.com/docs/digging-deeper/notifications/
- https://navree.com/docs/digging-deeper/time-zones/#send-windows
- https://navree.com/docs/packages/settings/
