---
name: navree-mail-and-notifications
description: "Sends email and notifications from a Navree app: mail transports (SMTP, SES, Mailgun, Postmark, Resend, log, array, failover), mail templates with CSS inlined and a generated text body, `TemplateMail`, `Mailable` and queued mail, and notifications over the database, mail and broadcast channels with `/api/notifications`. Use when sending an email, adding or editing a mail template, configuring SMTP, notifying users in-app, by mail or live, or asserting on sent mail in tests."
---

# Mail and notifications

`MailModule` renders mail from templates (`ViewModule`) and hands it
to a transport. `NotifyModule` sends one notification over several channels:
a database row users list at `/api/notifications`, a mail, and a broadcast
on the user's private channel. A new app has `ViewModule` and `MailModule`
(transport `log`); add `NotifyModule::from_config()` (and
`RealtimeModule` for the broadcast channel) to the root when needed.

## Steps: a mail

1. `navree make:mail posts welcome` writes `mail/welcome.rs` (a `Mailable`)
   with `welcome.html` and `welcome.txt` next to it, and registers the
   templates (`m.templates(mail::welcome::TEMPLATES)`) at `// navree:wire`.
2. Edit the templates (`{{ name }}`; HTML escapes values, text doesn't; an
   undefined variable fails the render).
3. Send it with an injected `Arc<Mailer>`: `mailer.queue(&mail).await?`
   (delivered by a queue worker after the request commits) or
   `mailer.send(&mail).await?` (now).
4. Test with `mail.transport = "array"`.

## Template mail (most mail)

```rust
use navree::mail::{MailError, Mailer, TemplateMail};
use navree::prelude::*;
use serde_json::json;

#[injectable]
pub struct Onboarding {
    mailer: Arc<Mailer>,
}

impl Onboarding {
    pub async fn welcome(&self, name: &str, email: &str) -> Result<(), MailError> {
        // renders mail/welcome.html and mail/welcome.txt (whichever exist)
        let mail = TemplateMail::new(email, "Welcome aboard", "mail/welcome")
            .context(json!({ "name": name }));
        self.mailer.queue(&mail).await
    }
}
```

Templates come from `resources/views/` files (`resources/views/mail/welcome.html`
is `mail/welcome.html`) or are embedded by a module:

```rust
pub const TEMPLATES: &[(&str, &str)] = &[
    ("mail/welcome.html", include_str!("welcome.html")),
    ("mail/welcome.txt", include_str!("welcome.txt")),
];
// wire: m.templates(TEMPLATES);   a file of the same name overrides it
```

Filters: `{{ amount_cents|money(currency) }}`, `{{ created_at|date("%d %b %Y") }}`.

## A Mailable (computed subject, several recipients)

```rust
use navree::mail::{MailError, Mailable, Message};
use navree::view::Views;
use serde_json::json;

pub struct WeeklyReport {
    pub to: String,
    pub won: u32,
}

impl Mailable for WeeklyReport {
    fn build(&self, views: &Views) -> Result<Message, MailError> {
        let context = json!({ "title": format!("{} deals", self.won) });
        Ok(Message::new()
            .to(&self.to)
            .subject("Your week")
            .html(views.render("mail/deal-won.html", &context)?)
            .text(views.render("mail/deal-won.txt", &context)?))
    }
}
```

`Message` setters: `from`, `to`, `cc`, `bcc`, `reply_to`, `subject`, `html`,
`text`; `mailer.send_message(message)` sends one without templates. Leave
`from` unset to use `mail.from`.

## Configuration

```toml
[mail]
from = "My App <hello@example.com>"
transport = "log"        # log | array | smtps://user:pass@host | ses://.. | mailgun://.. | postmark://.. | resend://.. | { type = "failover", transports = [..] }
# queue = "mail"         # the queue Mailer::queue uses
```

SMTP: `smtps://user:pass@smtp.example.com` (TLS, 465) or
`smtp://user:pass@host?tls=required` (STARTTLS, 587); percent-encode `@` in
user names (`%40`). Put the URL in the environment
(`APP_MAIL__TRANSPORT=smtps://...`), not in `config/app.toml`, and remember
a new app's TOML sets `transport`, so `.env` alone won't override it. The
`log` transport logs bodies only with `APP_LOG__LEVELS=navree::mail::body=debug`.
Point it at a local catcher (Mailpit) with `smtp://localhost:1025`.

`queue` renders and validates at once (template and address errors return
immediately), then dispatches `mail.send`; with no queue in the app it
sends at once and logs a warning.

The mailer inlines `<style>` rules (`mail.render.inline_css`), generates the
text body of HTML-only mail (`mail.render.text_from_html`) and sets
`lang`/`dir` from `Message::locale` (or a `locale` in a `TemplateMail`
context). `Message` also takes `header(name, value)` and
`metadata(key, value)` (provider tags). For email APIs, named transports
(`mailer.via`), tenant transports, the delivery log, suppressions, holding
mail and tracking, see `navree-mail-delivery`.

## Notifications

```rust
use navree::mail::TemplateMail;
use navree::notify::{Channel, Notifiable, Notification, USER_NOTIFIABLE_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

/// A user as a recipient: the /api/notifications routes find users by
/// USER_NOTIFIABLE_TYPE and the id as a string.
pub struct NotifiableUser {
    pub id: Uuid,
    pub email: String,
}

impl Notifiable for NotifiableUser {
    fn notifiable_type(&self) -> &'static str {
        USER_NOTIFIABLE_TYPE
    }

    fn notifiable_id(&self) -> String {
        self.id.to_string()
    }

    fn route_mail(&self) -> Option<String> {
        Some(self.email.clone())
    }
    // broadcast_channel() defaults to `private-users.{id}`
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DealWonNotification {
    pub deal_id: Uuid,
    pub title: String,
}

impl Notification for DealWonNotification {
    const TYPE: &'static str = "deal.won"; // stored and broadcast; keep it stable

    fn via(&self, _notifiable: &dyn Notifiable) -> Vec<Channel> {
        vec![Channel::Database, Channel::Broadcast, Channel::Mail]
    }

    fn to_database(&self, _notifiable: &dyn Notifiable) -> Option<Value> {
        Some(json!({ "title": "Deal won", "message": format!("“{}” was won", self.title), "url": format!("/deals/{}", self.deal_id) }))
    }

    fn to_mail(&self, _notifiable: &dyn Notifiable) -> Option<TemplateMail> {
        Some(TemplateMail {
            to: Vec::new(), // empty: the recipient's route_mail
            subject: format!("Won: {}", self.title),
            template: "mail/deal-won".to_owned(),
            context: json!({ "title": self.title }),
        })
    }
}
```

Send with an injected `Arc<Notifier>`:
`notifier.send(&user, DealWonNotification { .. }).await?` (returns the
notification id). Inside a unit of work the row is written in the
transaction and mail/broadcast go out after the commit. `navree
make:notification posts post_liked` generates one with the database and
broadcast channels. `to_broadcast` defaults to `to_database`. With
`notify.queue = true` each channel becomes a `notify.send` job.

Routes (signed in, own notifications only; set `notify.routes = false` in
apps without an authenticator): `GET /api/notifications?unread=true`,
`GET /api/notifications/unread-count`, `POST /api/notifications/{id}/read`,
`POST /api/notifications/read-all`. The broadcast channel
`private-users.{id}` needs an authorizer (`m.channel("users.{id}", ..)`;
see `navree-events-and-broadcasting`).

## Test

```rust
let app = TestApp::builder()
    .config("mail.transport", "array")
    .module(ViewModule::from_config())
    .module(MailModule::from_config())
    .module(OnboardingModule)
    .build()
    .await;
app.get::<Onboarding>().await.welcome("Ada", "ada@example.com").await.unwrap();
let mailer = app.get::<Mailer>().await;
let sent = mailer.array_transport().expect("the array transport");
sent.assert_sent(|m| m.to == ["ada@example.com"] && m.subject == "Welcome aboard");
```

With `QueueModule` in the test app, queued mail reaches the array transport
only after `app.get::<Queue>().await.run_pending().await`; or check the
dispatch with `QueueFake` (`SendMailJob`). Count database notifications with
`notifier.unread_count(&db, &user)`; the test app then needs `DatabaseModule`
and `.temp_database()` (SQLite: the `sqlite` feature of `navree` in
`[dev-dependencies]`).

## Common mistakes

- `MailModule` without `ViewModule` (boot error), or a template name with a
  typo (render error at send/queue time).
- Calling `mailer.send` inside a request for slow SMTP: use `queue`.
- Expecting `.env` to change `mail.transport` when `config/app.toml` sets it.
- Notification routes in an app without `AuthModule`: set `notify.routes = false`.
- Slack, Teams, SMS, WhatsApp and web push channels: see
  `navree-notification-channels`.
- Channel switches per category, digests, quiet hours, send windows and
  per-workspace mail templates: see `navree-notification-preferences`.
- Putting reset links or secrets in queued mail without thinking: the
  rendered message sits in the `jobs` table until sent.

## Docs

- https://navree.com/docs/digging-deeper/mail/
- https://navree.com/docs/digging-deeper/notifications/
- https://navree.com/docs/digging-deeper/views/
