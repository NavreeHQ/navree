//! PostgreSQL (with TimescaleDB) and MySQL: the channel tables on a real
//! server, so their migrations and every statement of the
//! `NotificationDelivery`, `NotificationOptOut` and `PushSubscription`
//! models run on each: recording and updating deliveries, the provider
//! lookup of status callbacks, the opt-out unique key, the push
//! subscription upsert, and UUIDs and timestamps that come back as they
//! were written.
//!
//! Opt-in: build with `--features postgres` and set
//! `NAVREE_TEST_POSTGRES_URL`, or `--features mysql` and
//! `NAVREE_TEST_MYSQL_URL`, to a URL of a throwaway server whose user may
//! create databases. Each test creates its own database and drops it at
//! the end.

#![allow(dead_code)]

use navree_db::{DatabaseModule, Db};
use navree_notify::{
    Channel, DeliveryLog, DeliveryStatus, NotificationDelivery, NotificationOptOut, Notifier,
    NotifyModule, NotifyOptions, OptOutSource, OptOuts, PushSubscription, PushSubscriptions,
};
use navree_orm::Model as _;
use navree_testing::TestApp;
use sea_orm::{ConnectionTrait, Database, DbBackend};
use uuid::Uuid;

use crate::external::subscription;
use crate::support::{self, Alert, Fakes, MEMBER_PHONE, MEMBER_WHATSAPP, Member};

/// A database of its own on the test server.
struct Scratch {
    admin: String,
    name: String,
    url: String,
    mysql: bool,
}

impl Scratch {
    async fn new(var: &str, test: &str, mysql: bool) -> Option<Scratch> {
        let admin = match std::env::var(var) {
            Ok(url) if !url.is_empty() => url,
            _ => {
                println!("SKIPPED {test}: set {var}");
                return None;
            }
        };
        let name = format!("navree_notify_{}", Uuid::now_v7().simple());
        let conn = Database::connect(&admin).await.unwrap();
        let create = if mysql {
            format!("CREATE DATABASE `{name}` CHARACTER SET utf8mb4")
        } else {
            format!("CREATE DATABASE {name}")
        };
        conn.execute_unprepared(&create).await.unwrap();
        conn.close().await.unwrap();
        let (base, rest) = admin.rsplit_once('/').unwrap();
        let options = rest.find('?').map_or("", |at| &rest[at..]);
        let url = format!("{base}/{name}{options}");
        Some(Scratch {
            admin,
            name,
            url,
            mysql,
        })
    }

    async fn drop(self) {
        let conn = Database::connect(&self.admin).await.unwrap();
        let drop = if self.mysql {
            format!("DROP DATABASE IF EXISTS `{}`", self.name)
        } else {
            format!("DROP DATABASE IF EXISTS {} WITH (FORCE)", self.name)
        };
        conn.execute_unprepared(&drop).await.unwrap();
    }
}

/// Sends over every external channel, plays the providers and checks the
/// tables on `scratch`.
async fn flow(scratch: &Scratch) {
    let fakes = Fakes::default();
    let app = fakes
        .install(
            TestApp::builder()
                .config("database.url", scratch.url.as_str())
                .config("database.migrate_on_boot", true)
                .module(DatabaseModule)
                .module(NotifyModule::for_root(NotifyOptions {
                    routes: false,
                    ..support::channel_options()
                })),
        )
        .build()
        .await;
    let db = app.get::<Db>().await;
    let expected = if scratch.mysql {
        DbBackend::MySql
    } else {
        DbBackend::Postgres
    };
    assert_eq!(db.backend(), expected);
    let member = Member::new();
    let push = app.get::<PushSubscriptions>().await;
    let browser = subscription("https://fcm.googleapis.com/fcm/send/server-test");

    // Subscribing twice keeps one row; the keys survive the round trip.
    push.subscribe(&member, browser.clone(), Some("Firefox")).await.unwrap();
    let saved = push.subscribe(&member, browser.clone(), Some("Firefox")).await.unwrap();
    assert_eq!(PushSubscription::query().count(&*db).await.unwrap(), 1);
    let found = push.of("user", &member.id.to_string()).await.unwrap();
    assert_eq!(found[0].id, saved.id);
    assert_eq!(found[0].auth, browser.keys.auth);
    assert_eq!(found[0].created_at, saved.created_at);

    // One notification over every recorded channel.
    let id = app
        .get::<Notifier>()
        .await
        .send(
            &member,
            Alert::via(
                &[
                    Channel::Slack,
                    Channel::MicrosoftTeams,
                    Channel::Sms,
                    Channel::WhatsApp,
                    Channel::WebPush,
                ],
                "Training due today",
            ),
        )
        .await
        .unwrap();
    let log = app.get::<DeliveryLog>().await;
    let deliveries = log.for_notification(id).await.unwrap();
    assert_eq!(deliveries.len(), 5);
    assert!(deliveries.iter().all(|d| d.notification_id == id));
    let push_delivery = deliveries.iter().find(|d| d.channel == "web_push").unwrap();
    assert_eq!(push_delivery.recipient.as_deref(), Some(saved.id.to_string().as_str()));
    let pushed = push.of("user", &member.id.to_string()).await.unwrap();
    assert!(pushed[0].last_used_at.is_some());

    // Provider callbacks move them forward.
    fakes.whatsapp.read(app.ctx(), MEMBER_WHATSAPP).await.unwrap();
    fakes.sms.deliver(app.ctx(), MEMBER_PHONE).await.unwrap();
    let whatsapp = deliveries.iter().find(|d| d.channel == "whatsapp").unwrap();
    let whatsapp = log
        .find_by_message("fake", whatsapp.provider_message_id.as_deref().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(whatsapp.delivery_status(), Some(DeliveryStatus::Read));
    assert_eq!(
        NotificationDelivery::query()
            .where_(NotificationDelivery::STATUS.eq("delivered"))
            .count(&*db)
            .await
            .unwrap(),
        1
    );

    // STOP, the unique key, and START.
    let opt_outs = app.get::<OptOuts>().await;
    assert_eq!(fakes.sms.reply(app.ctx(), MEMBER_PHONE, "STOP").await.unwrap().opted_out, 1);
    assert!(
        !opt_outs
            .opt_out(Channel::Sms, "(415) 555-0100", OptOutSource::Manual)
            .await
            .unwrap(),
        "the unique key keeps one row"
    );
    assert_eq!(NotificationOptOut::query().count(&*db).await.unwrap(), 1);
    app.get::<Notifier>()
        .await
        .send(&member, Alert::via(&[Channel::Sms], "Skipped"))
        .await
        .unwrap();
    assert_eq!(fakes.sms.sent().len(), 1, "the opted-out number is skipped");
    assert!(opt_outs.opt_in(Channel::Sms, MEMBER_PHONE).await.unwrap());
    assert!(!opt_outs.is_opted_out(Channel::Sms, MEMBER_PHONE).await.unwrap());

    // A gone subscription is deleted.
    fakes.web_push.expire(&browser.endpoint);
    app.get::<Notifier>()
        .await
        .send(&member, Alert::via(&[Channel::WebPush], "Gone"))
        .await
        .unwrap();
    assert_eq!(PushSubscription::query().count(&*db).await.unwrap(), 0);

    app.shutdown().await.unwrap();
}

#[cfg(feature = "postgres")]
#[tokio::test]
async fn channel_tables_work_on_postgres() {
    let Some(scratch) = Scratch::new("NAVREE_TEST_POSTGRES_URL", "postgres", false).await else {
        return;
    };
    flow(&scratch).await;
    scratch.drop().await;
}

#[cfg(feature = "mysql")]
#[tokio::test]
async fn channel_tables_work_on_mysql() {
    let Some(scratch) = Scratch::new("NAVREE_TEST_MYSQL_URL", "mysql", true).await else {
        return;
    };
    flow(&scratch).await;
    scratch.drop().await;
}
