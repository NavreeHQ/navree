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
    push.subscribe(&member, browser.clone(), Some("Firefox"))
        .await
        .unwrap();
    let saved = push
        .subscribe(&member, browser.clone(), Some("Firefox"))
        .await
        .unwrap();
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
    assert_eq!(
        push_delivery.recipient.as_deref(),
        Some(saved.id.to_string().as_str())
    );
    let pushed = push.of("user", &member.id.to_string()).await.unwrap();
    assert!(pushed[0].last_used_at.is_some());

    // Provider callbacks move them forward.
    fakes
        .whatsapp
        .read(app.ctx(), MEMBER_WHATSAPP)
        .await
        .unwrap();
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
    assert_eq!(
        fakes
            .sms
            .reply(app.ctx(), MEMBER_PHONE, "STOP")
            .await
            .unwrap()
            .opted_out,
        1
    );
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
    assert!(
        !opt_outs
            .is_opted_out(Channel::Sms, MEMBER_PHONE)
            .await
            .unwrap()
    );

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

/// Preferences and held deliveries on `scratch`: the owner upsert, the
/// one-query resolution, JSON documents and jobs that come back as they
/// were written, release times compared on the server, and two releases
/// at once claiming each row once.
async fn preference_flow(scratch: &Scratch) {
    use std::sync::Arc;

    use navree_contracts::{Clock, FixedClock};
    use navree_notify::{
        DeferralReason, NotificationDeferral, NotificationPreference, NotificationPreferences,
        PreferenceOwner,
    };
    use serde_json::json;

    use crate::people::{DealUpdate, Mention, Person, at, held};

    let fakes = Fakes::default();
    let clock = Arc::new(FixedClock::new(at("2026-10-28T20:00:00Z")));
    let app = fakes
        .install(
            TestApp::builder()
                .config("database.url", scratch.url.as_str())
                .config("database.migrate_on_boot", true)
                .module(DatabaseModule)
                .module(navree_view::ViewModule::for_root(
                    navree_view::ViewOptions {
                        dirs: vec![],
                        reload: false,
                    },
                ))
                .module(support::MailTemplates)
                .module(navree_mail::MailModule::for_root(
                    navree_mail::MailOptions {
                        from: "Navree <noreply@navree.test>".to_owned(),
                        transport: navree_mail::TransportConfig::Array,
                        ..navree_mail::MailOptions::default()
                    },
                ))
                .module(NotifyModule::for_root(NotifyOptions {
                    routes: false,
                    ..support::channel_options()
                })),
        )
        .override_value::<dyn Clock>(clock.clone())
        .build()
        .await;
    let db = app.get::<Db>().await;
    let preferences = app.get::<NotificationPreferences>().await;
    let notifier = app.get::<Notifier>().await;
    let acme = Uuid::now_v7();
    let kolkata = Person::in_zone(Some("Asia/Kolkata")).of(acme);

    // Two changes keep one row per owner; the document round-trips.
    preferences
        .update(&kolkata.owner(), &json!({"channels": {"whatsapp": false}}))
        .await
        .unwrap();
    let own = preferences
        .update(
            &kolkata.owner(),
            &json!({"quiet_hours": {"start": "22:00", "end": "07:00"}}),
        )
        .await
        .unwrap();
    preferences
        .update(
            &PreferenceOwner::workspace(acme),
            &json!({"channels": {"slack": false}}),
        )
        .await
        .unwrap();
    assert_eq!(
        NotificationPreference::query().count(&*db).await.unwrap(),
        2
    );
    let resolved = preferences.resolve(&kolkata, Some(acme)).await.unwrap();
    assert_eq!(resolved.own, own);
    assert!(!resolved.allows("deals", Channel::Slack));
    assert!(!resolved.allows("deals", Channel::WhatsApp));

    // 01:30 in Kolkata: the digest mail waits for 09:00, the text for 07:00.
    notifier
        .send(&kolkata, DealUpdate::new("Acme"))
        .await
        .unwrap();
    notifier
        .send(&kolkata, Mention { by: "Ada".into() })
        .await
        .unwrap();
    // Both texts and the mention's mail wait for the quiet hours, the deal
    // update's mail for the digest.
    let rows = held(&app).await;
    assert_eq!(rows.len(), 4);
    let reasons: Vec<Option<DeferralReason>> = rows.iter().map(|r| r.deferral_reason()).collect();
    assert_eq!(
        reasons,
        [
            Some(DeferralReason::QuietHours),
            Some(DeferralReason::QuietHours),
            Some(DeferralReason::QuietHours),
            Some(DeferralReason::Digest),
        ]
    );
    assert_eq!(rows[0].release_at, at("2026-10-29T01:30:00Z"));
    assert_eq!(rows[3].release_at, at("2026-10-29T03:30:00Z"));
    let job = rows[3].send_job().unwrap();
    assert_eq!(job.workspace_id, Some(acme));
    assert_eq!(job.category.as_deref(), Some("deals"));
    assert_eq!(
        NotificationDeferral::query()
            .where_(NotificationDeferral::RELEASE_AT.lte(at("2026-10-29T02:00:00Z")))
            .count(&*db)
            .await
            .unwrap(),
        3
    );

    // Two releases at once: each row is claimed and sent once.
    clock.set(at("2026-10-29T03:30:00Z"));
    let (first, second) = tokio::join!(notifier.release_due(), notifier.release_due());
    let (first, second) = (first.unwrap(), second.unwrap());
    assert_eq!(first.sent + second.sent, 3);
    assert_eq!(first.digests + second.digests, 1);
    assert_eq!(first.batched + second.batched, 1);
    assert_eq!(fakes.sms.sent().len(), 2);
    assert_eq!(support::sent_mail(&app).await.len(), 2);
    assert!(held(&app).await.is_empty());

    app.shutdown().await.unwrap();
}

#[cfg(feature = "postgres")]
#[tokio::test]
async fn preferences_and_held_deliveries_work_on_postgres() {
    let Some(scratch) =
        Scratch::new("NAVREE_TEST_POSTGRES_URL", "postgres preferences", false).await
    else {
        return;
    };
    preference_flow(&scratch).await;
    scratch.drop().await;
}

#[cfg(feature = "mysql")]
#[tokio::test]
async fn preferences_and_held_deliveries_work_on_mysql() {
    let Some(scratch) = Scratch::new("NAVREE_TEST_MYSQL_URL", "mysql preferences", true).await
    else {
        return;
    };
    preference_flow(&scratch).await;
    scratch.drop().await;
}
