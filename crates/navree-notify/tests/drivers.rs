//! Drivers and channels a module registers under a name: SMS and WhatsApp
//! drivers that `notify.sms.driver` and `notify.whatsapp.driver` select,
//! and notification channels a module adds, which deliver and are recorded
//! like the built-in ones, while names read from requests, stored rows and
//! queued payloads never add one.

#[path = "notify/support.rs"]
#[allow(dead_code)]
mod support;

#[path = "notify/database.rs"]
#[allow(dead_code)]
mod database;

use std::sync::{Arc, Mutex};

use navree_config::DriverName;
use navree_config::__private::register_in;
use navree_contracts::Dispatcher;
use navree_db::{DatabaseModule, Db};
use navree_kernel::{
    BoxFuture, Injectable, KernelError, Module, ModuleBuilder, ProviderKey, Resolver,
};
use navree_mail::{MailModule, MailOptions, TemplateMail, TransportConfig};
use navree_notify::{
    Channel, ChannelDriver, ChannelError, ModuleBuilderNotifyChannelsExt, Notifiable, Notification,
    NotificationPreference, NotificationPreferences, Notifier, NotifyModule, NotifyOptions,
    PreferenceOptions, PreferenceOwner, SendNotificationJob, SentMessage, SmsDriver, SmsMessage,
    WhatsAppDriver, WhatsAppMessage,
};
use navree_orm::Model as _;
use navree_testing::{TestApp, TestAppBuilder, principal};
use navree_view::{ViewModule, ViewOptions};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::support::{Alert, MailTemplates, Member, RecordingDispatcher, deliveries};

/// Records the number and the text of every message it is given.
#[derive(Clone, Default)]
struct Sandesh {
    sent: Arc<Mutex<Vec<(String, String)>>>,
}

impl Sandesh {
    fn sent(&self) -> Vec<(String, String)> {
        self.sent.lock().unwrap().clone()
    }
}

impl SmsDriver for Sandesh {
    fn send<'a>(
        &'a self,
        to: &'a str,
        message: &'a SmsMessage,
    ) -> BoxFuture<'a, Result<SentMessage, ChannelError>> {
        self.sent
            .lock()
            .unwrap()
            .push((to.to_owned(), message.body.clone()));
        Box::pin(async { Ok(SentMessage::new("sandesh")) })
    }
}

impl WhatsAppDriver for Sandesh {
    fn send<'a>(
        &'a self,
        to: &'a str,
        message: &'a WhatsAppMessage,
    ) -> BoxFuture<'a, Result<SentMessage, ChannelError>> {
        let text = message.body.clone().unwrap_or_default();
        self.sent.lock().unwrap().push((to.to_owned(), text));
        Box::pin(async { Ok(SentMessage::new("sandesh")) })
    }
}

/// What the SMS driver `sandesh` reads: `[notify.sandesh]`.
#[derive(Default, Deserialize)]
#[serde(default)]
struct SandeshSettings {
    sender_id: String,
}

/// Registers the SMS driver `sandesh` and the WhatsApp driver `baat` over
/// recorders the test keeps, and records the sender id `sandesh` read.
#[derive(Default)]
struct SandeshModule {
    sms: Sandesh,
    whatsapp: Sandesh,
    sender_id: Arc<Mutex<Option<String>>>,
}

impl Module for SandeshModule {
    fn register(&self, m: &mut ModuleBuilder) {
        let (sms, sender_id) = (self.sms.clone(), self.sender_id.clone());
        m.sms_driver("sandesh", move |ctx| {
            let (sms, sender_id) = (sms.clone(), sender_id.clone());
            async move {
                let settings: SandeshSettings = ctx.settings()?;
                *sender_id.lock().unwrap() = Some(settings.sender_id);
                Ok(sms)
            }
        });
        let whatsapp = self.whatsapp.clone();
        m.whatsapp_driver("baat", move |_| {
            let whatsapp = whatsapp.clone();
            async move { Ok(whatsapp) }
        });
    }
}

/// A test app with a database, views, array mail and
/// `NotifyModule::from_config()`.
fn configured() -> TestAppBuilder {
    database::database(TestApp::builder())
        .config("http.per_ip_rate_limit", Value::Null)
        .module(DatabaseModule)
        .module(ViewModule::for_root(ViewOptions {
            dirs: vec![],
            reload: false,
        }))
        .module(MailTemplates)
        .module(MailModule::for_root(MailOptions {
            from: "Meera Sharma <meera@ludhiana.example>".to_owned(),
            transport: TransportConfig::Array,
            ..MailOptions::default()
        }))
        .module(NotifyModule::from_config())
}

/// The boot error of `builder`, which must fail.
async fn boot_error(builder: TestAppBuilder) -> String {
    match builder.try_build().await {
        Ok(_) => panic!("the app booted"),
        Err(error) => error.to_string(),
    }
}

#[tokio::test]
async fn registered_sms_and_whatsapp_drivers_are_selected_by_name() {
    let module = SandeshModule::default();
    let (sms, whatsapp, sender_id) = (
        module.sms.clone(),
        module.whatsapp.clone(),
        module.sender_id.clone(),
    );
    let app = configured()
        .config(
            "notify.sms",
            json!({"driver": "sandesh", "from": "+911612345678", "default_region": "IN"}),
        )
        .config("notify.whatsapp.driver", "baat")
        .config("notify.sandesh.sender_id", "NAVREE")
        .module(module)
        .build()
        .await;
    assert_eq!(sender_id.lock().unwrap().as_deref(), Some("NAVREE"));

    let member = Member {
        phone: Some("+91 98140 12345".to_owned()),
        whatsapp: Some("+91 98140 12345".to_owned()),
        ..Member::new()
    };
    let text = "Harpreet Sandhu paid ₹1,00,000";
    app.get::<Notifier>()
        .await
        .send(
            &member,
            Alert::via(&[Channel::Sms, Channel::WhatsApp], text),
        )
        .await
        .unwrap();
    let expected = vec![("+919814012345".to_owned(), text.to_owned())];
    assert_eq!(sms.sent(), expected);
    assert_eq!(whatsapp.sent(), expected);
    let rows = deliveries(&app).await;
    assert_eq!(rows.len(), 2, "{rows:?}");
    for row in &rows {
        assert_eq!(row.provider.as_deref(), Some("sandesh"), "{row:?}");
        assert_eq!(row.recipient.as_deref(), Some("+919814012345"), "{row:?}");
        assert_eq!(row.status, "sent", "{row:?}");
    }

    // A name no module registers fails the boot, naming the method.
    let error = boot_error(
        configured()
            .config("notify.sms.driver", "doorsanchar")
            .module(SandeshModule::default()),
    )
    .await;
    assert!(
        error.contains("notify.sms.driver is `doorsanchar`, but no module registers an SMS driver under that name with m.sms_driver (known: twilio, vonage, sns, aws_sns, log, sandesh)"),
        "{error}"
    );
}

/// What the channel `doot` was given: `(to, message)`.
#[derive(Clone, Default)]
struct DootLog(Arc<Mutex<Vec<(String, Value)>>>);

impl DootLog {
    fn sent(&self) -> Vec<(String, Value)> {
        self.0.lock().unwrap().clone()
    }
}

/// The notification channel `doot`, built by the container.
struct DootChannel {
    log: Arc<DootLog>,
}

impl Injectable for DootChannel {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<DootLog>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(DootChannel {
                log: r.get::<DootLog>().await?,
            })
        })
    }
}

impl ChannelDriver for DootChannel {
    const NAME: &'static str = "doot";

    fn send<'a>(
        &'a self,
        to: &'a str,
        message: &'a Value,
    ) -> BoxFuture<'a, Result<SentMessage, ChannelError>> {
        self.log
            .0
            .lock()
            .unwrap()
            .push((to.to_owned(), message.clone()));
        Box::pin(async { Ok(SentMessage::new("doot")) })
    }
}

/// Adds the channel `doot` over a log the test keeps.
#[derive(Default)]
struct DootModule {
    log: DootLog,
}

impl Module for DootModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.value(self.log.clone());
        m.notification_channel::<DootChannel>();
    }
}

/// An invoice was paid: over the channels it names, with a message for
/// every channel a module adds.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct InvoicePaid {
    channels: Vec<Channel>,
    customer: String,
}

impl InvoicePaid {
    fn via(channels: &[Channel], customer: &str) -> Self {
        Self {
            channels: channels.to_vec(),
            customer: customer.to_owned(),
        }
    }
}

impl Notification for InvoicePaid {
    const TYPE: &'static str = "invoice_paid";

    fn via(&self, _notifiable: &dyn Notifiable) -> Vec<Channel> {
        self.channels.clone()
    }

    fn to_mail(&self, _notifiable: &dyn Notifiable) -> Option<TemplateMail> {
        Some(TemplateMail {
            subject: format!("{} paid", self.customer),
            template: "mail/deal-won".to_owned(),
            context: json!({ "title": self.customer }),
            ..TemplateMail::default()
        })
    }

    fn to_database(&self, _notifiable: &dyn Notifiable) -> Option<Value> {
        Some(json!({ "customer": self.customer }))
    }

    fn to_channel(&self, channel: DriverName, _notifiable: &dyn Notifiable) -> Option<Value> {
        let _ = channel;
        Some(json!({ "text": format!("{} paid ₹1,00,000", self.customer) }))
    }
}

/// A customer with a mail address and an address on every channel a module
/// adds.
struct Customer {
    id: Uuid,
    to: String,
}

impl Customer {
    fn new(to: &str) -> Self {
        Self {
            id: Uuid::now_v7(),
            to: to.to_owned(),
        }
    }
}

impl Notifiable for Customer {
    fn notifiable_type(&self) -> &'static str {
        "user"
    }

    fn notifiable_id(&self) -> String {
        self.id.to_string()
    }

    fn route_mail(&self) -> Option<String> {
        Some("harpreet.sandhu@amritsar.example".to_owned())
    }

    fn route_channel(&self, channel: DriverName) -> Option<String> {
        let _ = channel;
        Some(self.to.clone())
    }
}

#[tokio::test]
async fn a_registered_channel_delivers_and_is_recorded() {
    let module = DootModule::default();
    let log = module.log.clone();
    let dispatcher = Arc::new(RecordingDispatcher::default());
    let app = support::builder(NotifyOptions {
        queue: true,
        ..NotifyOptions::default()
    })
    .module(module)
    .override_value::<dyn Dispatcher>(dispatcher.clone())
    .build()
    .await;
    let notifier = app.get::<Notifier>().await;
    let harpreet = Customer::new("@harpreet");
    let paid = || InvoicePaid::via(&[Channel::Mail, Channel::named("doot")], "Harpreet Sandhu");
    let message = json!({ "text": "Harpreet Sandhu paid ₹1,00,000" });

    // Sent now: the mail and the channel `doot`, which is recorded.
    notifier.send_now(&harpreet, paid()).await.unwrap();
    assert_eq!(log.sent(), vec![("@harpreet".to_owned(), message.clone())]);
    assert_eq!(support::sent_mail(&app).await.len(), 1);
    let rows = deliveries(&app).await;
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].channel, "doot");
    assert_eq!(rows[0].recipient.as_deref(), Some("@harpreet"));
    assert_eq!(rows[0].provider.as_deref(), Some("doot"));
    assert_eq!(rows[0].status, "sent");

    // Queued: the job carries the rendered message, reads back, and a
    // worker sends it.
    notifier.send(&harpreet, paid()).await.unwrap();
    let jobs = dispatcher.jobs();
    let envelope = jobs
        .iter()
        .find(|envelope| envelope.payload["delivery"]["via"] == "registered")
        .expect("the job of the channel doot");
    assert_eq!(envelope.payload["delivery"]["channel"], "doot");
    assert_eq!(envelope.payload["delivery"]["to"], "@harpreet");
    assert_eq!(envelope.payload["delivery"]["message"], message);
    let job: SendNotificationJob = serde_json::from_value(envelope.payload.clone()).unwrap();
    assert_eq!(job.delivery.channel(), Channel::named("doot"));
    support::run_job(&app, envelope).await.unwrap();
    assert_eq!(log.sent().len(), 2);
    assert_eq!(deliveries(&app).await.len(), 2);
}

#[tokio::test]
async fn a_registered_channel_records_a_webhook_address_by_its_host_only() {
    let module = DootModule::default();
    let log = module.log.clone();
    let app = support::builder(NotifyOptions::default())
        .module(module)
        .build()
        .await;
    let notifier = app.get::<Notifier>().await;
    let hooks = [
        "https://discord.example/api/webhooks/1234/s3cret-token-ludhiana",
        "http://chat.amritsar.example/hooks/s3cret-token-amritsar",
    ];
    for hook in hooks {
        notifier
            .send(
                &Customer::new(hook),
                InvoicePaid::via(&[Channel::named("doot")], "Meera Sharma"),
            )
            .await
            .unwrap();
    }

    // The channel gets each address whole; the log keeps its host only.
    let sent: Vec<String> = log.sent().into_iter().map(|(to, _)| to).collect();
    assert_eq!(sent, hooks);
    let rows = deliveries(&app).await;
    let mut recipients: Vec<String> = rows
        .iter()
        .filter_map(|row| row.recipient.clone())
        .collect();
    recipients.sort();
    assert_eq!(
        recipients,
        ["webhook:chat.amritsar.example", "webhook:discord.example"]
    );
    for row in &rows {
        let stored = serde_json::to_string(row).unwrap();
        assert!(!stored.contains("s3cret-token"), "{stored}");
    }
}

#[tokio::test]
async fn a_via_naming_an_unregistered_channel_fails_that_delivery() {
    let app = support::builder(NotifyOptions::default())
        .module(DootModule::default())
        .build()
        .await;
    let error = app
        .get::<Notifier>()
        .await
        .send(
            &Customer::new("@meera"),
            InvoicePaid::via(
                &[Channel::Database, Channel::named("patang")],
                "Meera Sharma",
            ),
        )
        .await
        .unwrap_err()
        .to_string();
    assert!(
        error.contains(
            "no module registers the notification channel `patang` with m.notification_channel"
        ),
        "{error}"
    );

    // The database row is stored; the other delivery is recorded as failed.
    assert_eq!(support::rows(&app).await.len(), 1);
    let rows = deliveries(&app).await;
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].channel, "patang");
    assert_eq!(rows[0].recipient.as_deref(), Some("@meera"));
    assert_eq!(rows[0].status, "failed");
    assert_eq!(
        DriverName::lookup("patang"),
        None,
        "naming a channel adds no name"
    );
}

#[tokio::test]
async fn made_up_channel_names_from_requests_leave_registered_channels_readable() {
    let options = NotifyOptions {
        preferences: PreferenceOptions {
            channels: vec![Channel::Mail, Channel::named("doot")],
            ..PreferenceOptions::default()
        },
        ..NotifyOptions::default()
    };
    let app = support::builder(options)
        .module(DootModule::default())
        .build()
        .await;
    let user = Uuid::now_v7();
    let as_user = app.acting_as(principal(user));

    // 2000 made-up channel names are refused, and none is kept.
    for i in 0..2000 {
        let mut channels = Map::new();
        channels.insert(format!("made_up_{i}"), json!(false));
        let refused = as_user
            .patch("/api/notifications/preferences")
            .json(&json!({ "channels": channels }))
            .send()
            .await
            .assert_status(400)
            .json_value();
        if i == 7 {
            let message = refused["errors"]["preferences"][0]
                .as_str()
                .unwrap_or_default();
            assert_eq!(message, "unknown notification channel `made_up_7`");
        }
    }
    assert_eq!(DriverName::lookup("made_up_7"), None);
    assert_eq!(DriverName::lookup("made_up_1999"), None);

    // The registered channel still reads: in a stored row,
    let owner = PreferenceOwner::user(user);
    let preferences = app.get::<NotificationPreferences>().await;
    preferences
        .update(&owner, &json!({"channels": {"doot": false, "mail": true}}))
        .await
        .unwrap();
    let stored = preferences.of(&owner).await.unwrap();
    assert_eq!(
        stored.channel("billing", Channel::named("doot")),
        Some(false)
    );
    assert_eq!(stored.channel("billing", Channel::Mail), Some(true));

    // and in a queued payload; a payload naming a channel the process does
    // not know does not read, and adds no name.
    let job = |channel: &str| {
        json!({
            "id": Uuid::now_v7(),
            "type": "invoice_paid",
            "notifiable_type": "user",
            "notifiable_id": user.to_string(),
            "created_at": "2026-11-15T10:30:00Z",
            "delivery": {
                "via": "registered",
                "channel": channel,
                "to": "@harpreet",
                "message": {"text": "Harpreet Sandhu paid ₹1,00,000"},
            },
        })
    };
    let read: SendNotificationJob = serde_json::from_value(job("doot")).unwrap();
    assert_eq!(read.delivery.channel(), Channel::named("doot"));
    assert!(serde_json::from_value::<SendNotificationJob>(job("made_up_job")).is_err());
    assert_eq!(DriverName::lookup("made_up_job"), None);
}

#[tokio::test]
async fn a_driver_name_of_another_kind_is_an_unknown_channel() {
    // A cache store a module of the process registered, a built-in name of
    // another kind, and a name nobody registered: a request gets the same
    // text for each.
    register_in("cache.store", "sanduk_cache").unwrap();
    DriverName::register("memory").unwrap();
    let options = NotifyOptions {
        preferences: PreferenceOptions {
            channels: vec![Channel::Mail, Channel::named("doot")],
            ..PreferenceOptions::default()
        },
        ..NotifyOptions::default()
    };
    let app = support::builder(options)
        .module(DootModule::default())
        .build()
        .await;
    let as_user = app.acting_as(principal(Uuid::now_v7()));
    for name in ["sanduk_cache", "memory", "pune_kabutar"] {
        let mut channels = Map::new();
        channels.insert(name.to_owned(), json!(false));
        let refused = as_user
            .patch("/api/notifications/preferences")
            .json(&json!({ "channels": channels }))
            .send()
            .await
            .assert_status(400)
            .json_value();
        assert_eq!(
            refused["errors"]["preferences"][0],
            format!("unknown notification channel `{name}`"),
            "{refused}"
        );
        assert_eq!(Channel::from_name(name), None, "{name}");
    }

    // A channel a module adds is still found, and switched.
    as_user
        .patch("/api/notifications/preferences")
        .json(&json!({ "channels": {"doot": false} }))
        .send()
        .await
        .assert_status(200);
}

#[tokio::test]
async fn a_stored_preference_naming_a_channel_no_module_registers_keeps_the_others() {
    let app = support::app().await;
    let owner = PreferenceOwner::user(Uuid::now_v7());
    let row = NotificationPreference {
        id: Uuid::now_v7(),
        owner_type: owner.owner_type.clone(),
        owner_id: owner.owner_id.clone(),
        preferences: json!({
            "channels": {"mail": false, "kabutar_old": true},
            "categories": {"deals": {"channels": {"kabutar_old": false, "sms": false}}},
        }),
        ..NotificationPreference::default()
    };
    let db = app.get::<Db>().await;
    NotificationPreference::insert_or_ignore(&*db, [row])
        .await
        .unwrap();

    let read = app
        .get::<NotificationPreferences>()
        .await
        .of(&owner)
        .await
        .unwrap();
    assert_eq!(read.channel("billing", Channel::Mail), Some(false));
    assert_eq!(read.channel("deals", Channel::Sms), Some(false));
    assert_eq!(
        serde_json::to_value(&read).unwrap(),
        json!({
            "channels": {"mail": false},
            "categories": {"deals": {"channels": {"sms": false}}},
        })
    );
    assert_eq!(DriverName::lookup("kabutar_old"), None);
}

/// Registers the SMS driver `push`, the name of the table `notify.push`.
struct PushSmsModule;

impl Module for PushSmsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.sms_driver("push", |_| async {
            Ok::<_, navree_kernel::BoxError>(Sandesh::default())
        });
    }
}

/// Registers the WhatsApp driver `push`.
struct PushWhatsAppModule;

impl Module for PushWhatsAppModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.whatsapp_driver("push", |_| async {
            Ok::<_, navree_kernel::BoxError>(Sandesh::default())
        });
    }
}

#[tokio::test]
async fn an_sms_or_whatsapp_driver_cannot_take_the_name_of_the_push_table() {
    let sms = "m.sms_driver(\"push\") in `PushSmsModule`: `notify.push` is a setting of the notify section; register the SMS driver under another name";
    let error = boot_error(configured().module(PushSmsModule)).await;
    assert!(error.contains(sms), "{error}");

    let whatsapp = "m.whatsapp_driver(\"push\") in `PushWhatsAppModule`: `notify.push` is a setting of the notify section; register the WhatsApp driver under another name";
    let error = boot_error(configured().module(PushWhatsAppModule)).await;
    assert!(error.contains(whatsapp), "{error}");
}
