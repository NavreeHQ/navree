//! The examples of `content/docs/tools/fleet.mdx`, written as an app writes
//! them (the facade, the prelude and the macros), compiled and run.
#![cfg(feature = "sqlite")]

use std::sync::Mutex;
use std::time::Duration;

use chrono::{TimeZone, Utc};
use navree::BoxFuture;
use navree::auth::AuthPages;
use navree::contracts::{Clock, Job, JobContext, JobOptions};
use navree::events::Listener;
use navree::fleet::{Fleet, LongWaitDetected};
use navree::prelude::*;
use navree::queue::Queue;
use navree::testing::{FixedClock, TestApp};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// "Installation": the app root, with the sign-in the dashboard uses.
#[allow(dead_code)]
fn app(sources: ConfigSources) -> AppBuilder {
    Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(HttpModule::from_config())
        .module(DatabaseModule::from_config())
        .module(QueueModule::from_config())
        .module(FleetModule::from_config())
        .module(SessionModule::from_config())
        .module(AuthModule::from_config())
        .module(UiModule)
        .module(AuthPages::new().app_name("Acme Operations"))
}

/// The chat client of the long-wait example.
#[derive(Default)]
pub struct ChatClient {
    posted: Mutex<Vec<(String, String)>>,
}

impl ChatClient {
    async fn post(&self, channel: &str, text: &str) -> Result<(), BoxError> {
        self.posted
            .lock()
            .unwrap()
            .push((channel.to_owned(), text.to_owned()));
        Ok(())
    }
}

/// Posts long waits to the ops channel.
#[injectable]
pub struct PostToOpsChannel {
    chat: Arc<ChatClient>,
}

impl Listener<LongWaitDetected> for PostToOpsChannel {
    fn handle<'a>(
        &'a self,
        event: &'a LongWaitDetected,
        _ctx: &'a AppContext,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            let text = format!(
                "Queue `{}` is behind: its oldest job has waited {} s (threshold {} s).",
                event.queue, event.wait_secs, event.threshold_secs
            );
            self.chat.post("#ops", &text).await?;
            Ok(())
        })
    }
}

/// Subscribes the listener.
#[module(register = Self::wire)]
pub struct OpsModule;

impl OpsModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.listen::<LongWaitDetected, PostToOpsChannel>();
    }
}

/// The digest mail of the testing example, on queue `mail`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SendDigest {
    user_id: Uuid,
}

impl Job for SendDigest {
    const NAME: &'static str = "mail.send_digest";

    fn options() -> JobOptions {
        JobOptions {
            queue: "mail".into(),
            ..JobOptions::default()
        }
    }

    async fn handle(self, _ctx: JobContext) -> Result<(), BoxError> {
        Ok(())
    }
}

/// Registers the digest job.
#[module(register = Self::wire)]
pub struct DigestModule;

impl DigestModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.job::<SendDigest>();
    }
}

/// "Testing": the example, as written on the page.
#[tokio::test]
async fn a_mail_waiting_two_minutes_is_a_long_wait() {
    let clock = Arc::new(FixedClock::new(
        Utc.with_ymd_and_hms(2026, 10, 26, 9, 0, 0).unwrap(),
    ));
    let app = TestApp::builder()
        .temp_database()
        .config("fleet.long_wait.mail", 60)
        .config("fleet.dashboard", false) // no AccessModule in this test app
        .module(DatabaseModule::from_config())
        .module(QueueModule::from_config())
        .module(FleetModule::from_config())
        .module(DigestModule)
        .override_value::<dyn Clock>(clock.clone())
        .build()
        .await;

    // SendDigest's options put it on the `mail` queue.
    app.get::<Queue>()
        .await
        .dispatch(SendDigest {
            user_id: Uuid::now_v7(),
        })
        .await
        .unwrap();
    clock.advance(Duration::from_secs(120));

    let late = app.get::<Fleet>().await.long_waits().await.unwrap();
    assert_eq!(late.len(), 1);
    assert_eq!(late[0].queue, "mail");
    assert_eq!(late[0].wait_secs, 120);
    app.shutdown().await.unwrap();
}

/// "Pausing and continuing" and the long-wait listener.
#[tokio::test]
async fn pausing_and_the_listener_work_through_the_facade() {
    let chat = Arc::new(ChatClient::default());
    let app = TestApp::builder()
        .temp_database()
        .config("fleet.dashboard", false)
        .config(
            "fleet.supervisors.exports",
            serde_json::json!({ "queues": ["exports"], "balance": "off", "max_processes": 2 }),
        )
        .module(DatabaseModule::from_config())
        .module(QueueModule::from_config())
        .module(FleetModule::from_config())
        .module(EventsModule)
        .module(OpsModule)
        .override_value(chat.clone())
        .build()
        .await;
    let fleet = app.get::<Fleet>().await;
    fleet.pause(Some("exports")).await.unwrap();
    assert!(fleet.paused().await.unwrap().contains("exports"));
    fleet.resume(Some("exports")).await.unwrap();
    assert!(fleet.paused().await.unwrap().is_empty());

    let bus = app.get::<navree::events::EventBus>().await;
    bus.emit(LongWaitDetected {
        queue: "mail".into(),
        wait_secs: 95,
        threshold_secs: 30,
    })
    .await
    .unwrap();
    assert_eq!(
        chat.posted.lock().unwrap().clone(),
        [(
            "#ops".to_owned(),
            "Queue `mail` is behind: its oldest job has waited 95 s (threshold 30 s).".to_owned()
        )]
    );
    app.shutdown().await.unwrap();
}
