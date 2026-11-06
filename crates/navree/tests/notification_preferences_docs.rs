//! The examples of `docs/digging-deeper/notification-preferences`, through
//! the facade with the `notify` and `settings` features (in `full`): the
//! notifications with a category, a digest, urgency and a send window, the
//! module that registers them, the notifiable with a workspace and a zone,
//! the preferences in code and over HTTP, the workspace templates as
//! settings, and the testing example as written.
// The page's examples are written as an app writes them, without doc comments.
#![allow(missing_docs)]

use std::sync::Arc;

use navree::contracts::{Clock, FixedClock};
use navree::db::Db;
use navree::mail::{Mailer, TemplateMail};
use navree::notify::{
    Channel, Digest, Notifiable, Notification, NotificationCatalog, NotificationDeferral,
    NotificationPreferences, Notifier, PreferenceOwner, SmsDriver, SmsFake, SmsMessage,
    USER_NOTIFIABLE_TYPE, notification,
};
use navree::orm::Model as _;
use navree::prelude::*;
use navree::settings::notify::NotificationTemplates;
use navree::settings::{Scope, Settings};
use navree::testing::{TestApp, principal};
use navree::time::SendWindow;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// "Categories and digests".

/// A deal the user follows was won.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DealWon {
    pub deal_id: Uuid,
    pub title: String,
}

#[notification(category = "deals", digest = "daily")]
impl Notification for DealWon {
    const TYPE: &'static str = "deal.won";

    fn via(&self, _notifiable: &dyn Notifiable) -> Vec<Channel> {
        vec![Channel::Database, Channel::Mail, Channel::Sms]
    }

    fn to_database(&self, _notifiable: &dyn Notifiable) -> Option<Value> {
        Some(json!({
            "title": "Deal won",
            "message": self.title,
            "url": format!("/deals/{}", self.deal_id),
        }))
    }

    fn to_mail(&self, _notifiable: &dyn Notifiable) -> Option<TemplateMail> {
        Some(TemplateMail {
            subject: format!("Deal won: {}", self.title),
            template: "mail/deal-won".into(),
            context: json!({ "title": self.title }),
            ..TemplateMail::default()
        })
    }

    fn to_sms(&self, _notifiable: &dyn Notifiable) -> Option<SmsMessage> {
        Some(SmsMessage::new(format!("Deal won: {}", self.title)))
    }
}

/// A one-time sign-in code.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SignInCode {
    pub code: String,
}

#[notification(category = "security", urgent)]
impl Notification for SignInCode {
    const TYPE: &'static str = "auth.sign_in_code";

    fn via(&self, _notifiable: &dyn Notifiable) -> Vec<Channel> {
        vec![Channel::Sms]
    }

    fn to_sms(&self, _notifiable: &dyn Notifiable) -> Option<SmsMessage> {
        Some(SmsMessage::new(format!(
            "Your sign-in code is {}",
            self.code
        )))
    }
}

// ---------------------------------------------------------------------------
// "Send windows".

/// A training module is due.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrainingDue {
    pub module: String,
}

impl Notification for TrainingDue {
    const TYPE: &'static str = "training.due";
    const CATEGORY: &'static str = "training";

    fn via(&self, _notifiable: &dyn Notifiable) -> Vec<Channel> {
        vec![Channel::Database, Channel::Mail]
    }

    fn send_window(&self, _notifiable: &dyn Notifiable) -> Option<SendWindow> {
        Some(SendWindow::weekdays().between("09:00", "17:00"))
    }

    fn to_mail(&self, _notifiable: &dyn Notifiable) -> Option<TemplateMail> {
        Some(TemplateMail {
            subject: format!("{} is due", self.module),
            template: "mail/training-due".into(),
            context: json!({ "module": self.module }),
            ..TemplateMail::default()
        })
    }
}

// ---------------------------------------------------------------------------
// "Registering categories".

#[module(imports = [
    NotifyModule::for_feature::<DealWon>(),
    NotifyModule::for_feature::<SignInCode>(),
    NotifyModule::for_feature::<TrainingDue>(),
])]
pub struct DealsModule;

// ---------------------------------------------------------------------------
// "The recipient's workspace and clock".

/// A member of a workspace, as the recipient of notifications.
#[derive(Clone, Debug)]
pub struct Member {
    pub user_id: Uuid,
    pub workspace_id: Uuid,
    pub email: String,
    pub mobile: Option<String>,
    /// The zone the member picked in their profile, if any.
    pub time_zone: Option<String>,
}

impl Notifiable for Member {
    fn notifiable_type(&self) -> &'static str {
        USER_NOTIFIABLE_TYPE
    }

    fn notifiable_id(&self) -> String {
        self.user_id.to_string()
    }

    fn route_mail(&self) -> Option<String> {
        Some(self.email.clone())
    }

    fn route_sms(&self) -> Option<String> {
        self.mobile.clone()
    }

    fn workspace_id(&self) -> Option<Uuid> {
        Some(self.workspace_id)
    }

    fn time_zone(&self) -> Option<String> {
        self.time_zone.clone()
    }
}

/// The mail templates of the page's notifications.
struct Templates;

impl Module for Templates {
    fn register(&self, m: &mut ModuleBuilder) {
        m.templates(&[
            ("mail/deal-won.txt", "{{ title }} was won."),
            ("mail/training-due.txt", "{{ module }} is due."),
        ]);
    }
}

fn member(zone: &str) -> Member {
    let user_id = Uuid::now_v7();
    Member {
        user_id,
        workspace_id: Uuid::now_v7(),
        email: format!("member-{}@acme.test", user_id.simple()),
        mobile: Some("+919812345678".into()),
        time_zone: Some(zone.into()),
    }
}

// ---------------------------------------------------------------------------
// "Testing", as written on the page.

#[tokio::test]
async fn deal_mail_waits_for_the_morning_digest() {
    // Wednesday 20:00 UTC is 01:30 on Thursday in Kolkata.
    let clock = Arc::new(FixedClock::new("2026-10-28T20:00:00Z".parse().unwrap()));
    let sms = SmsFake::new();
    let app = TestApp::builder()
        .temp_database()
        .config("mail.transport", "array")
        .config("notify.routes", false)
        .module(DatabaseModule::from_config())
        .module(ViewModule::from_config())
        .module(Templates)
        .module(MailModule::from_config())
        .module(NotifyModule::from_config())
        .override_value::<dyn Clock>(clock.clone())
        .override_value::<dyn SmsDriver>(sms.driver())
        .build()
        .await;
    let notifier = app.get::<Notifier>().await;
    let priya = member("Asia/Kolkata");

    for title in ["Acme renewal", "Globex expansion"] {
        let won = DealWon {
            deal_id: Uuid::now_v7(),
            title: title.into(),
        };
        notifier.send(&priya, won).await.unwrap();
    }
    // The texts go out at once; the mail waits for the digest.
    assert_eq!(sms.sent().len(), 2);
    let mailer = app.get::<Mailer>().await;
    let mail = mailer.array_transport().unwrap();
    assert!(mail.sent().is_empty());

    // 09:00 in Kolkata: one mail lists both deals.
    clock.set("2026-10-29T03:30:00Z".parse().unwrap());
    let released = notifier.release_due().await.unwrap();
    assert_eq!((released.digests, released.batched), (1, 2));
    let digest = &mail.sent()[0];
    assert_eq!(digest.to, [priya.email.as_str()]);
    assert!(
        digest
            .text
            .as_deref()
            .unwrap()
            .contains("Deal won: Acme renewal")
    );
}

// ---------------------------------------------------------------------------
// The rest of the page: urgency, send windows, the catalog, preferences in
// code and over HTTP, and the workspace templates.

async fn app(clock: Arc<FixedClock>) -> TestApp {
    TestApp::builder()
        .temp_database()
        .config("mail.transport", "array")
        .config("notify.preferences.required", "security")
        .config("notify.sms.driver", "log")
        .config("settings.store", "memory")
        .module(DatabaseModule::from_config())
        .module(ViewModule::from_config())
        .module(Templates)
        .module(MailModule::from_config())
        .module(NotifyModule::from_config())
        .module(SettingsModule::from_config())
        .module(DealsModule)
        .override_value::<dyn Clock>(clock)
        .build()
        .await
}

#[tokio::test]
async fn urgent_codes_skip_quiet_hours_and_training_waits_for_the_window() {
    // Friday 30 October 2026, 18:00 UTC: 23:30 in Kolkata.
    let clock = Arc::new(FixedClock::new("2026-10-30T18:00:00Z".parse().unwrap()));
    let app = app(clock.clone()).await;
    let notifier = app.get::<Notifier>().await;
    let preferences = app.get::<NotificationPreferences>().await;
    let priya = member("Asia/Kolkata");
    preferences
        .update(
            &PreferenceOwner::of(&priya),
            &json!({ "quiet_hours": { "start": "22:00", "end": "07:00" } }),
        )
        .await
        .unwrap();

    notifier
        .send(
            &priya,
            SignInCode {
                code: "481516".into(),
            },
        )
        .await
        .unwrap();
    notifier
        .send(
            &priya,
            TrainingDue {
                module: "Phishing basics".into(),
            },
        )
        .await
        .unwrap();

    let db = app.get::<Db>().await;
    let held = NotificationDeferral::query().get(&*db).await.unwrap();
    // The code went out; the training mail waits for Monday 09:00 there.
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].release_at.to_rfc3339(), "2026-11-02T03:30:00+00:00");
    assert_eq!(held[0].reason, "send_window");
}

#[tokio::test]
async fn preferences_in_code_and_over_http() {
    let clock = Arc::new(FixedClock::new("2026-10-28T12:00:00Z".parse().unwrap()));
    let app = app(clock).await;
    let catalog = app.get::<NotificationCatalog>().await;
    assert_eq!(catalog.kind("deal.won").unwrap().category, "deals");

    // "In code".
    let preferences = app.get::<NotificationPreferences>().await;
    let priya = member("Asia/Kolkata");
    let owner = PreferenceOwner::user(priya.user_id);
    preferences
        .update(
            &owner,
            &json!({
                "categories": { "deals": { "channels": { "sms": false }, "digest": "weekly" } },
                "quiet_hours": { "start": "22:00", "end": "07:00" },
            }),
        )
        .await
        .unwrap();
    let resolved = preferences
        .resolve(&priya, Some(priya.workspace_id))
        .await
        .unwrap();
    assert!(!resolved.allows("deals", Channel::Sms));
    assert_eq!(resolved.digest("deals", Digest::Daily), Digest::Weekly);

    // "The preference routes".
    let as_priya = app.acting_as(principal(priya.user_id));
    let body = as_priya
        .patch("/api/notifications/preferences")
        .json(&json!({ "categories": { "deals": { "digest": null } } }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(
        body["data"],
        json!({
            "categories": { "deals": { "channels": { "sms": false } } },
            "quiet_hours": { "start": "22:00", "end": "07:00" }
        })
    );
    let deals = &body["categories"][0];
    assert_eq!(deals["category"], "deals");
    assert_eq!(deals["types"], json!(["deal.won"]));
    assert_eq!(deals["digest"], "daily");
    assert_eq!(deals["channels"]["sms"], false);
    assert_eq!(body["categories"][1]["required"], true);

    let refused = as_priya
        .patch("/api/notifications/preferences")
        .json(&json!({ "categories": { "security": { "channels": { "sms": false } } } }))
        .send()
        .await
        .assert_status(400)
        .json_value();
    assert_eq!(
        refused["errors"]["categories.security.channels.sms"],
        json!(["security notifications cannot be turned off"])
    );
}

#[tokio::test]
async fn workspace_templates_restyle_the_mail() {
    let clock = Arc::new(FixedClock::new("2026-10-28T12:00:00Z".parse().unwrap()));
    let app = app(clock).await;
    let settings = app.get::<Settings>().await;
    let db = app.get::<Db>().await;
    let priya = member("UTC");
    let acme = priya.workspace_id;

    let uow = db.uow(None, None).await;
    settings
        .update::<NotificationTemplates>(
            &uow,
            Scope::workspace(acme),
            json!({
                "brand_color": "#0F766E",
                "templates": { "training.due": { "subject": "Acme Academy: {{ module }}" } },
            }),
        )
        .await
        .unwrap();
    uow.commit().await.unwrap();

    // Wednesday 12:00 UTC is inside the window.
    app.get::<Notifier>()
        .await
        .send(
            &priya,
            TrainingDue {
                module: "Phishing basics".into(),
            },
        )
        .await
        .unwrap();
    let mailer = app.get::<Mailer>().await;
    let sent = mailer.array_transport().unwrap().sent();
    assert_eq!(sent[0].subject, "Acme Academy: Phishing basics");
}
