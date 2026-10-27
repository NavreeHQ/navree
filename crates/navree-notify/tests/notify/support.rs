//! Test fixtures: a notifiable user, a configurable notification, a
//! recording broadcaster and dispatcher, and app builders.

#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use navree_contracts::{
    BoxError, BoxFuture, Broadcaster, Dispatcher, JobContext, JobEnvelope, JobRegistration,
};
use navree_db::{DatabaseModule, Db};
use navree_kernel::{Module, ModuleBuilder};
use navree_mail::{MailModule, MailOptions, Mailer, Message, TemplateMail, TransportConfig};
use navree_notify::{
    Channel, Notifiable, Notification, NotificationDelivery, NotifyModule, NotifyOptions,
    SlackDriver, SlackFake, SlackMessage, SlackRoute, SmsDriver, SmsFake, SmsMessage, TeamsDriver,
    TeamsFake, TeamsMessage, TeamsRoute, WebPushDriver, WebPushFake, WebPushMessage,
    WhatsAppDriver, WhatsAppFake, WhatsAppMessage, notifications,
};
use navree_orm::Model as _;
use navree_testing::{TestApp, TestAppBuilder};
use navree_view::{ModuleBuilderViewExt, ViewModule, ViewOptions};
use sea_orm::{EntityTrait, QueryOrder};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

/// A user that receives notifications.
#[derive(Clone, Debug)]
pub struct TestUser {
    pub id: Uuid,
    pub email: Option<String>,
}

impl TestUser {
    /// A user with an email address.
    pub fn new() -> Self {
        let id = Uuid::now_v7();
        Self {
            id,
            email: Some(format!("user-{}@navree.test", id.simple())),
        }
    }

    /// A user without an email address.
    pub fn without_email() -> Self {
        Self {
            email: None,
            ..Self::new()
        }
    }
}

impl Notifiable for TestUser {
    fn notifiable_type(&self) -> &'static str {
        "user"
    }

    fn notifiable_id(&self) -> String {
        self.id.to_string()
    }

    fn route_mail(&self) -> Option<String> {
        self.email.clone()
    }
}

/// A notification sent over the channels it names, with the CRM's data
/// shape `{title, message, url}`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DealWon {
    pub channels: Vec<Channel>,
    pub title: String,
}

impl DealWon {
    pub fn via(channels: &[Channel], title: &str) -> Self {
        Self {
            channels: channels.to_vec(),
            title: title.to_owned(),
        }
    }

    pub fn data(&self) -> Value {
        json!({
            "title": self.title,
            "message": format!("{} was won", self.title),
            "url": "/deals/1",
        })
    }
}

impl Notification for DealWon {
    const TYPE: &'static str = "deal_won";

    fn via(&self, _notifiable: &dyn Notifiable) -> Vec<Channel> {
        self.channels.clone()
    }

    fn to_mail(&self, _notifiable: &dyn Notifiable) -> Option<TemplateMail> {
        Some(TemplateMail {
            subject: format!("Deal won: {}", self.title),
            template: "mail/deal-won".to_owned(),
            context: json!({ "title": self.title }),
            ..TemplateMail::default()
        })
    }

    fn to_database(&self, _notifiable: &dyn Notifiable) -> Option<Value> {
        Some(self.data())
    }
}

/// The mail templates `DealWon` renders.
pub struct MailTemplates;

impl Module for MailTemplates {
    fn register(&self, m: &mut ModuleBuilder) {
        m.templates(&[
            ("mail/deal-won.html", "<p>{{ title }} was won.</p>"),
            ("mail/deal-won.txt", "{{ title }} was won."),
        ]);
    }
}

/// Records every broadcast.
#[derive(Default)]
pub struct RecordingBroadcaster {
    pub sent: Mutex<Vec<(Vec<String>, String, Value)>>,
}

impl RecordingBroadcaster {
    pub fn sent(&self) -> Vec<(Vec<String>, String, Value)> {
        self.sent.lock().unwrap().clone()
    }
}

impl Broadcaster for RecordingBroadcaster {
    fn broadcast(
        &self,
        channels: Vec<String>,
        event: String,
        data: Value,
    ) -> BoxFuture<'_, Result<(), BoxError>> {
        self.sent.lock().unwrap().push((channels, event, data));
        Box::pin(async { Ok(()) })
    }
}

/// Records every dispatched job instead of queueing it.
#[derive(Default)]
pub struct RecordingDispatcher {
    pub jobs: Mutex<Vec<JobEnvelope>>,
}

impl RecordingDispatcher {
    pub fn jobs(&self) -> Vec<JobEnvelope> {
        self.jobs.lock().unwrap().clone()
    }
}

impl Dispatcher for RecordingDispatcher {
    fn dispatch_envelope(&self, env: JobEnvelope) -> BoxFuture<'_, Result<String, BoxError>> {
        let mut jobs = self.jobs.lock().unwrap();
        jobs.push(env);
        let id = format!("job-{}", jobs.len());
        Box::pin(async move { Ok(id) })
    }
}

/// A test app with a temporary database, views, array mail and
/// `NotifyModule::for_root(options)`.
pub fn builder(options: NotifyOptions) -> TestAppBuilder {
    TestApp::builder()
        .temp_database()
        .config("http.per_ip_rate_limit", Value::Null)
        .module(DatabaseModule)
        .module(ViewModule::for_root(ViewOptions {
            dirs: vec![],
            reload: false,
        }))
        .module(MailTemplates)
        .module(MailModule::for_root(MailOptions {
            from: "Navree <noreply@navree.test>".to_owned(),
            transport: TransportConfig::Array,
            ..MailOptions::default()
        }))
        .module(NotifyModule::for_root(options))
}

/// [`builder`] with the default options, booted.
pub async fn app() -> TestApp {
    builder(NotifyOptions::default()).build().await
}

/// [`builder`] with a recording broadcaster, booted.
pub async fn app_with_broadcaster() -> (TestApp, Arc<RecordingBroadcaster>) {
    let broadcaster = Arc::new(RecordingBroadcaster::default());
    let app = builder(NotifyOptions::default())
        .override_value::<dyn Broadcaster>(broadcaster.clone())
        .build()
        .await;
    (app, broadcaster)
}

/// Every stored notification, oldest first.
pub async fn rows(app: &TestApp) -> Vec<notifications::Model> {
    let db = app.get::<Db>().await;
    notifications::Entity::find()
        .order_by_asc(notifications::Column::CreatedAt)
        .order_by_asc(notifications::Column::Id)
        .all(db.conn())
        .await
        .expect("read notifications")
}

/// Every message the array transport recorded.
pub async fn sent_mail(app: &TestApp) -> Vec<Message> {
    app.get::<Mailer>()
        .await
        .array_transport()
        .expect("array transport")
        .sent()
}

/// Runs a dispatched job the way a queue worker would: through the
/// `JobRegistration` the app's modules contributed.
pub async fn run_job(app: &TestApp, envelope: &JobEnvelope) -> Result<(), BoxError> {
    let registration = app
        .ctx()
        .contributions::<JobRegistration>()
        .into_iter()
        .find(|c| c.value.name == envelope.job_type)
        .unwrap_or_else(|| panic!("no job registration for `{}`", envelope.job_type));
    (registration.value.run)(
        envelope.payload.clone(),
        JobContext {
            app: app.ctx().clone(),
            job_id: "job-1".to_owned(),
            attempt: 1,
        },
    )
    .await
}

/// A recipient with an address on every external channel.
#[derive(Clone, Debug)]
pub struct Member {
    pub id: Uuid,
    pub phone: Option<String>,
    pub whatsapp: Option<String>,
    pub slack: Option<SlackRoute>,
    pub teams: Option<TeamsRoute>,
}

impl Member {
    /// A member with a US phone (written nationally), a UK WhatsApp
    /// number, a Slack channel and a Teams conversation.
    pub fn new() -> Self {
        Self {
            id: Uuid::now_v7(),
            phone: Some("(415) 555-0100".to_owned()),
            whatsapp: Some("+44 7400 123456".to_owned()),
            slack: Some(SlackRoute::channel("C0123456789")),
            teams: Some(TeamsRoute::conversation(
                "https://smba.trafficmanager.net/emea/",
                "a:1Xyz",
            )),
        }
    }

    /// A member with no external addresses.
    pub fn unreachable() -> Self {
        Self {
            phone: None,
            whatsapp: None,
            slack: None,
            teams: None,
            ..Self::new()
        }
    }
}

impl Notifiable for Member {
    fn notifiable_type(&self) -> &'static str {
        "user"
    }

    fn notifiable_id(&self) -> String {
        self.id.to_string()
    }

    fn route_mail(&self) -> Option<String> {
        None
    }

    fn route_slack(&self) -> Option<SlackRoute> {
        self.slack.clone()
    }

    fn route_teams(&self) -> Option<TeamsRoute> {
        self.teams.clone()
    }

    fn route_sms(&self) -> Option<String> {
        self.phone.clone()
    }

    fn route_whatsapp(&self) -> Option<String> {
        self.whatsapp.clone()
    }
}

/// The E.164 form of [`Member::new`]'s phone.
pub const MEMBER_PHONE: &str = "+14155550100";

/// The E.164 form of [`Member::new`]'s WhatsApp number.
pub const MEMBER_WHATSAPP: &str = "+447400123456";

/// A notification over the channels it names, with one text for all.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Alert {
    pub channels: Vec<Channel>,
    pub text: String,
    /// Overrides the notifiable's number.
    pub sms_to: Option<String>,
    /// Overrides the notifiable's Slack route.
    pub slack_to: Option<SlackRoute>,
}

impl Alert {
    pub fn via(channels: &[Channel], text: &str) -> Self {
        Self {
            channels: channels.to_vec(),
            text: text.to_owned(),
            ..Self::default()
        }
    }
}

impl Notification for Alert {
    const TYPE: &'static str = "alert";

    fn via(&self, _notifiable: &dyn Notifiable) -> Vec<Channel> {
        self.channels.clone()
    }

    fn to_slack(&self, _notifiable: &dyn Notifiable) -> Option<SlackMessage> {
        let message = SlackMessage::new(&self.text);
        Some(match &self.slack_to {
            Some(route) => message.to(route.clone()),
            None => message,
        })
    }

    fn to_teams(&self, _notifiable: &dyn Notifiable) -> Option<TeamsMessage> {
        Some(TeamsMessage::new(&self.text))
    }

    fn to_sms(&self, _notifiable: &dyn Notifiable) -> Option<SmsMessage> {
        let message = SmsMessage::new(&self.text);
        Some(match &self.sms_to {
            Some(to) => message.to(to),
            None => message,
        })
    }

    fn to_whatsapp(&self, _notifiable: &dyn Notifiable) -> Option<WhatsAppMessage> {
        Some(WhatsAppMessage::text(&self.text))
    }

    fn to_web_push(&self, _notifiable: &dyn Notifiable) -> Option<WebPushMessage> {
        Some(WebPushMessage::new(&self.text).url("/alerts"))
    }
}

/// A fake for every external channel.
#[derive(Clone, Debug, Default)]
pub struct Fakes {
    pub slack: SlackFake,
    pub teams: TeamsFake,
    pub sms: SmsFake,
    pub whatsapp: WhatsAppFake,
    pub web_push: WebPushFake,
}

impl Fakes {
    /// `builder` with every fake installed.
    pub fn install(&self, builder: TestAppBuilder) -> TestAppBuilder {
        builder
            .override_value::<dyn SlackDriver>(self.slack.driver())
            .override_value::<dyn TeamsDriver>(self.teams.driver())
            .override_value::<dyn SmsDriver>(self.sms.driver())
            .override_value::<dyn WhatsAppDriver>(self.whatsapp.driver())
            .override_value::<dyn WebPushDriver>(self.web_push.driver())
    }
}

/// The options of the channel tests: numbers without a country code are
/// read in the US.
pub fn channel_options() -> NotifyOptions {
    let mut options = NotifyOptions::default();
    options.sms.default_region = Some("US".to_owned());
    options
}

/// A test app with every channel faked, booted.
pub async fn channel_app() -> (TestApp, Fakes) {
    let fakes = Fakes::default();
    let app = fakes.install(builder(channel_options())).build().await;
    (app, fakes)
}

/// Every delivery recorded, oldest first.
pub async fn deliveries(app: &TestApp) -> Vec<NotificationDelivery> {
    let db = app.get::<Db>().await;
    NotificationDelivery::query()
        .oldest_by(NotificationDelivery::CREATED_AT)
        .get(&*db)
        .await
        .expect("read deliveries")
        .into_iter()
        .map(navree_orm::Record::into_inner)
        .collect()
}
