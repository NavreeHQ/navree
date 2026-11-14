//! Drivers a module registers under a name: SMS and WhatsApp drivers that
//! `notify.sms.driver` and `notify.whatsapp.driver` select, read their own
//! settings and deliver like the built-in drivers.

#[path = "notify/support.rs"]
#[allow(dead_code)]
mod support;

#[path = "notify/database.rs"]
#[allow(dead_code)]
mod database;

use std::sync::{Arc, Mutex};

use navree_db::DatabaseModule;
use navree_kernel::{BoxFuture, Module, ModuleBuilder};
use navree_mail::{MailModule, MailOptions, TransportConfig};
use navree_notify::{
    Channel, ChannelError, ModuleBuilderNotifyChannelsExt, Notifier, NotifyModule, SentMessage,
    SmsDriver, SmsMessage, WhatsAppDriver, WhatsAppMessage,
};
use navree_testing::{TestApp, TestAppBuilder};
use navree_view::{ViewModule, ViewOptions};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::support::{Alert, Member, MailTemplates, deliveries};

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
        .send(&member, Alert::via(&[Channel::Sms, Channel::WhatsApp], text))
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
