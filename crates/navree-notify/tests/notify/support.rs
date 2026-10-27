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
    Channel, Notifiable, Notification, NotifyModule, NotifyOptions, notifications,
};
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
