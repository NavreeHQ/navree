//! Fixtures of the preference and timing tests: people in time zones and
//! workspaces, notifications with categories, digests, urgency and send
//! windows, and an app on a fixed clock.

#![allow(dead_code)]

use std::sync::Arc;

use chrono::{DateTime, Utc};
use navree_contracts::{Clock, FixedClock};
use navree_db::Db;
use navree_mail::TemplateMail;
use navree_notify::{
    Channel, Digest, Notifiable, Notification, NotificationDeferral, NotificationPreferences,
    NotifyOptions, PreferenceOwner, SmsMessage,
};
use navree_orm::Model as _;
use navree_testing::{TestApp, TestAppBuilder};
use navree_time::SendWindow;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::{self, Fakes};

/// `text` as an instant.
pub fn at(text: &str) -> DateTime<Utc> {
    text.parse().unwrap()
}

/// A person with a mail address, a phone, a zone and a workspace.
#[derive(Clone, Debug)]
pub struct Person {
    pub id: Uuid,
    pub email: String,
    pub phone: String,
    pub zone: Option<String>,
    pub workspace: Option<Uuid>,
}

impl Person {
    /// A person in `zone` (`None`: no zone of their own).
    pub fn in_zone(zone: Option<&str>) -> Self {
        let id = Uuid::now_v7();
        Self {
            id,
            email: format!("person-{}@navree.test", id.simple()),
            phone: "+14155550100".to_owned(),
            zone: zone.map(str::to_owned),
            workspace: None,
        }
    }

    /// This person in `workspace`.
    pub fn of(mut self, workspace: Uuid) -> Self {
        self.workspace = Some(workspace);
        self
    }

    /// This person with phone `phone`.
    pub fn phone(mut self, phone: &str) -> Self {
        phone.clone_into(&mut self.phone);
        self
    }

    pub fn owner(&self) -> PreferenceOwner {
        PreferenceOwner::user(self.id)
    }
}

impl Notifiable for Person {
    fn notifiable_type(&self) -> &'static str {
        navree_notify::USER_NOTIFIABLE_TYPE
    }

    fn notifiable_id(&self) -> String {
        self.id.to_string()
    }

    fn route_mail(&self) -> Option<String> {
        Some(self.email.clone())
    }

    fn route_sms(&self) -> Option<String> {
        Some(self.phone.clone())
    }

    fn workspace_id(&self) -> Option<Uuid> {
        self.workspace
    }

    fn time_zone(&self) -> Option<String> {
        self.zone.clone()
    }
}

/// The channels and texts every test notification shares.
fn mail(subject: String, title: &str) -> TemplateMail {
    TemplateMail {
        subject,
        template: "mail/deal-won".to_owned(),
        context: json!({ "title": title }),
        ..TemplateMail::default()
    }
}

/// Category `deals`, daily digest.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DealUpdate {
    pub title: String,
    pub channels: Vec<Channel>,
}

impl DealUpdate {
    pub fn new(title: &str) -> Self {
        Self {
            title: title.to_owned(),
            channels: vec![Channel::Database, Channel::Mail, Channel::Sms],
        }
    }
}

impl Notification for DealUpdate {
    const TYPE: &'static str = "deal_update";
    const CATEGORY: &'static str = "deals";
    const DIGEST: Digest = Digest::Daily;

    fn via(&self, _n: &dyn Notifiable) -> Vec<Channel> {
        self.channels.clone()
    }

    fn to_mail(&self, _n: &dyn Notifiable) -> Option<TemplateMail> {
        Some(mail(format!("Deal update: {}", self.title), &self.title))
    }

    fn to_database(&self, _n: &dyn Notifiable) -> Option<Value> {
        Some(json!({ "title": self.title }))
    }

    fn to_sms(&self, _n: &dyn Notifiable) -> Option<SmsMessage> {
        Some(SmsMessage::new(format!("Deal update: {}", self.title)))
    }
}

/// Category `mentions` (its type), no digest.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Mention {
    pub by: String,
}

impl Notification for Mention {
    const TYPE: &'static str = "mentions";

    fn via(&self, _n: &dyn Notifiable) -> Vec<Channel> {
        vec![Channel::Database, Channel::Mail, Channel::Sms]
    }

    fn to_mail(&self, _n: &dyn Notifiable) -> Option<TemplateMail> {
        Some(mail(format!("{} mentioned you", self.by), &self.by))
    }

    fn to_database(&self, _n: &dyn Notifiable) -> Option<Value> {
        Some(json!({ "by": self.by }))
    }

    fn to_sms(&self, _n: &dyn Notifiable) -> Option<SmsMessage> {
        Some(SmsMessage::new(format!("{} mentioned you", self.by)))
    }
}

/// Category `security`, urgent.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SignInCode {
    pub code: String,
}

impl Notification for SignInCode {
    const TYPE: &'static str = "sign_in_code";
    const CATEGORY: &'static str = "security";
    const URGENT: bool = true;

    fn via(&self, _n: &dyn Notifiable) -> Vec<Channel> {
        vec![Channel::Mail, Channel::Sms]
    }

    fn to_mail(&self, _n: &dyn Notifiable) -> Option<TemplateMail> {
        Some(mail(format!("Your code is {}", self.code), &self.code))
    }

    fn to_sms(&self, _n: &dyn Notifiable) -> Option<SmsMessage> {
        Some(SmsMessage::new(format!("Your code is {}", self.code)))
    }
}

/// Category `training`, weekdays 09:00 to 17:00 on the recipient's clock.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrainingDue {
    pub course: String,
}

impl Notification for TrainingDue {
    const TYPE: &'static str = "training_due";
    const CATEGORY: &'static str = "training";

    fn via(&self, _n: &dyn Notifiable) -> Vec<Channel> {
        vec![Channel::Database, Channel::Mail, Channel::Sms]
    }

    fn send_window(&self, _n: &dyn Notifiable) -> Option<SendWindow> {
        Some(SendWindow::weekdays().between("09:00", "17:00"))
    }

    fn to_mail(&self, _n: &dyn Notifiable) -> Option<TemplateMail> {
        Some(mail(format!("Training due: {}", self.course), &self.course))
    }

    fn to_database(&self, _n: &dyn Notifiable) -> Option<Value> {
        Some(json!({ "course": self.course }))
    }

    fn to_sms(&self, _n: &dyn Notifiable) -> Option<SmsMessage> {
        Some(SmsMessage::new(format!("Training due: {}", self.course)))
    }
}

/// An app with every channel faked, on a clock showing `now`.
pub fn timed(options: NotifyOptions, now: &str) -> (TestAppBuilder, Arc<FixedClock>, Fakes) {
    let clock = Arc::new(FixedClock::new(at(now)));
    let fakes = Fakes::default();
    let builder = fakes
        .install(support::builder(NotifyOptions {
            sms: support::channel_options().sms,
            ..options
        }))
        .override_value::<dyn Clock>(clock.clone());
    (builder, clock, fakes)
}

/// [`timed`] with the default options, booted.
pub async fn timed_app(now: &str) -> (TestApp, Arc<FixedClock>, Fakes) {
    let (builder, clock, fakes) = timed(NotifyOptions::default(), now);
    (builder.build().await, clock, fakes)
}

/// Stores `preferences` (a JSON merge patch) for `owner`.
pub async fn prefer(app: &TestApp, owner: &PreferenceOwner, preferences: Value) {
    app.get::<NotificationPreferences>()
        .await
        .update(owner, &preferences)
        .await
        .expect("store preferences");
}

/// Every held delivery, by release time.
pub async fn held(app: &TestApp) -> Vec<NotificationDeferral> {
    let db = app.get::<Db>().await;
    NotificationDeferral::query()
        .oldest_by(NotificationDeferral::RELEASE_AT)
        .get(&*db)
        .await
        .expect("read deferrals")
        .into_iter()
        .map(navree_orm::Record::into_inner)
        .collect()
}

/// The subjects of every mail sent, in order.
pub async fn subjects(app: &TestApp) -> Vec<String> {
    support::sent_mail(app)
        .await
        .into_iter()
        .map(|message| message.subject)
        .collect()
}
