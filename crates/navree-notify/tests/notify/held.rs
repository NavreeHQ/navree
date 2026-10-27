//! Held deliveries on a fixed clock across zones: daily digests, quiet
//! hours, send windows, urgent notifications, and `notify:release`
//! (claims, queueing, retries, preference changes).

use std::time::Duration;

use chrono::TimeDelta;
use navree_contracts::{Dispatcher, ZoneLevel, ZoneOwner};
use navree_notify::{
    Channel, DeferralReason, Delivery, Digest, Notifier, NotifyOptions, Released,
};
use navree_time::{TimeModule, TimeOptions, ZonesFake};
use serde_json::json;
use uuid::Uuid;

use crate::people::{
    DealUpdate, Mention, Person, SignInCode, TrainingDue, at, held, prefer, subjects, timed,
    timed_app,
};
use crate::support::{self, RecordingDispatcher};

#[tokio::test]
async fn daily_digests_batch_mail_at_nine_on_each_recipients_clock() {
    // Wednesday 28 October 2026, 20:00 UTC: 01:30 on Thursday in Kolkata,
    // 16:00 on Wednesday in New York.
    let (app, clock, fakes) = timed_app("2026-10-28T20:00:00Z").await;
    let notifier = app.get::<Notifier>().await;
    let kolkata = Person::in_zone(Some("Asia/Kolkata"));
    let new_york = Person::in_zone(Some("America/New_York"));

    notifier.send(&kolkata, DealUpdate::new("Acme")).await.unwrap();
    notifier.send(&kolkata, DealUpdate::new("Globex")).await.unwrap();
    notifier.send(&new_york, DealUpdate::new("Initech")).await.unwrap();

    // The database row and the text message go out at once; the mail waits.
    assert_eq!(support::rows(&app).await.len(), 3);
    assert_eq!(fakes.sms.sent().len(), 3);
    assert!(subjects(&app).await.is_empty());
    let rows = held(&app).await;
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|row| row.channel == "mail"
        && row.deferral_reason() == Some(DeferralReason::Digest)
        && row.digest.as_deref() == Some("daily")
        && row.category.as_deref() == Some("deals")));
    // 09:00 in Kolkata is 03:30 UTC; 09:00 in New York (summer time) 13:00.
    assert_eq!(rows[0].release_at, at("2026-10-29T03:30:00Z"));
    assert_eq!(rows[1].release_at, at("2026-10-29T03:30:00Z"));
    assert_eq!(rows[2].release_at, at("2026-10-29T13:00:00Z"));

    // Nothing is due yet.
    assert_eq!(notifier.release_due().await.unwrap(), Released::default());

    clock.set(at("2026-10-29T03:30:00Z"));
    let released = notifier.release_due().await.unwrap();
    assert_eq!(
        released,
        Released {
            digests: 1,
            batched: 2,
            ..Released::default()
        }
    );
    let mail = support::sent_mail(&app).await;
    assert_eq!(mail.len(), 1);
    assert_eq!(mail[0].to, [kolkata.email.clone()]);
    assert_eq!(mail[0].subject, "Your notifications");
    let text = mail[0].text.as_deref().unwrap();
    assert!(text.starts_with("2 notifications"), "{text}");
    assert!(text.contains("- Deal update: Acme\n  Acme was won."), "{text}");
    assert!(text.contains("- Deal update: Globex"), "{text}");

    clock.set(at("2026-10-29T13:00:00Z"));
    let released = notifier.release_due().await.unwrap();
    assert_eq!((released.digests, released.batched), (1, 1));
    assert_eq!(
        subjects(&app).await,
        ["Your notifications", "Your notifications"]
    );
    assert_eq!(support::sent_mail(&app).await[1].to, [new_york.email.clone()]);
    assert!(held(&app).await.is_empty());
}

#[tokio::test]
async fn a_persons_digest_choice_beats_the_notifications() {
    let (app, clock, _fakes) = timed_app("2026-10-28T08:00:00Z").await;
    let notifier = app.get::<Notifier>().await;
    let weekly = Person::in_zone(Some("UTC"));
    let instant = Person::in_zone(Some("UTC"));
    prefer(&app, &weekly.owner(), json!({"digest": "weekly"})).await;
    prefer(
        &app,
        &instant.owner(),
        json!({"categories": {"deals": {"digest": "off"}}}),
    )
    .await;

    notifier.send(&weekly, DealUpdate::new("Acme")).await.unwrap();
    notifier.send(&instant, DealUpdate::new("Acme")).await.unwrap();
    // A mention has no digest of its own; the weekly person's applies.
    notifier.send(&weekly, Mention { by: "Ada".into() }).await.unwrap();

    assert_eq!(subjects(&app).await, ["Deal update: Acme"]);
    let rows = held(&app).await;
    assert_eq!(rows.len(), 2);
    // Monday 2 November, 09:00 UTC.
    assert!(rows.iter().all(|row| row.release_at == at("2026-11-02T09:00:00Z")
        && row.digest.as_deref() == Some("weekly")));

    clock.set(at("2026-11-02T09:00:00Z"));
    let released = notifier.release_due().await.unwrap();
    assert_eq!((released.digests, released.batched), (1, 2));
    let mail = support::sent_mail(&app).await;
    let text = mail[1].text.as_deref().unwrap();
    assert!(text.contains("Deal update: Acme") && text.contains("Ada mentioned you"), "{text}");
}

#[tokio::test]
async fn quiet_hours_hold_what_interrupts_until_they_end_on_the_recipients_clock() {
    // 14:00 UTC: 23:00 in Tokyo, 14:00 in London.
    let (app, clock, fakes) = timed_app("2026-10-28T14:00:00Z").await;
    let notifier = app.get::<Notifier>().await;
    let tokyo = Person::in_zone(Some("Asia/Tokyo")).phone("+81 90 1234 5678");
    let london = Person::in_zone(Some("Europe/London")).phone("+44 7400 123456");
    for person in [&tokyo, &london] {
        prefer(
            &app,
            &person.owner(),
            json!({"quiet_hours": {"start": "22:00", "end": "07:00"}}),
        )
        .await;
    }

    notifier.send(&tokyo, Mention { by: "Ada".into() }).await.unwrap();
    notifier.send(&london, Mention { by: "Ada".into() }).await.unwrap();

    // The in-app notification shows at once for both.
    assert_eq!(support::rows(&app).await.len(), 2);
    // London is awake: its text and mail went out.
    assert_eq!(fakes.sms.sent_to("+447400123456").len(), 1);
    assert_eq!(subjects(&app).await, ["Ada mentioned you"]);
    // Tokyo's wait for 07:00 there (22:00 UTC).
    assert!(fakes.sms.sent_to("+819012345678").is_empty());
    let rows = held(&app).await;
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| row.release_at == at("2026-10-28T22:00:00Z")
        && row.deferral_reason() == Some(DeferralReason::QuietHours)));
    let mut channels: Vec<&str> = rows.iter().map(|row| row.channel.as_str()).collect();
    channels.sort_unstable();
    assert_eq!(channels, ["mail", "sms"]);

    clock.set(at("2026-10-28T21:59:00Z"));
    assert_eq!(notifier.release_due().await.unwrap(), Released::default());
    clock.set(at("2026-10-28T22:00:00Z"));
    let released = notifier.release_due().await.unwrap();
    assert_eq!(released.sent, 2);
    assert_eq!(fakes.sms.sent_to("+819012345678").len(), 1);
    assert_eq!(subjects(&app).await, ["Ada mentioned you", "Ada mentioned you"]);
    assert!(held(&app).await.is_empty());
}

#[tokio::test]
async fn the_workspaces_quiet_hours_apply_unless_a_person_sets_their_own() {
    let (app, _clock, fakes) = timed_app("2026-10-28T23:00:00Z").await;
    let notifier = app.get::<Notifier>().await;
    let acme = Uuid::now_v7();
    prefer(
        &app,
        &navree_notify::PreferenceOwner::workspace(acme),
        json!({"quiet_hours": {"start": "22:00", "end": "06:00"}}),
    )
    .await;
    let member = Person::in_zone(Some("UTC")).of(acme);
    let night_owl = Person::in_zone(Some("UTC")).of(acme).phone("+44 7400 123456");
    // An empty period of one's own: no quiet hours.
    prefer(
        &app,
        &night_owl.owner(),
        json!({"quiet_hours": {"start": "00:00", "end": "00:00"}}),
    )
    .await;

    notifier.send(&member, Mention { by: "Ada".into() }).await.unwrap();
    notifier.send(&night_owl, Mention { by: "Ada".into() }).await.unwrap();

    assert!(fakes.sms.sent_to("+14155550100").is_empty());
    assert_eq!(fakes.sms.sent_to("+447400123456").len(), 1);
    let rows = held(&app).await;
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| row.release_at == at("2026-10-29T06:00:00Z")));
}

#[tokio::test]
async fn urgent_notifications_ignore_quiet_hours_and_digests() {
    let (app, _clock, fakes) = timed_app("2026-10-28T14:00:00Z").await;
    let notifier = app.get::<Notifier>().await;
    let tokyo = Person::in_zone(Some("Asia/Tokyo"));
    prefer(
        &app,
        &tokyo.owner(),
        json!({"digest": "daily", "quiet_hours": {"start": "22:00", "end": "07:00"}}),
    )
    .await;

    notifier
        .send(&tokyo, SignInCode { code: "481516".into() })
        .await
        .unwrap();

    assert_eq!(subjects(&app).await, ["Your code is 481516"]);
    assert_eq!(fakes.sms.sent().len(), 1);
    assert!(held(&app).await.is_empty());
}

#[tokio::test]
async fn send_windows_wait_for_the_next_slot_in_each_zone() {
    // Friday 30 October 2026, 18:00 UTC: 23:30 in Kolkata, 11:00 in Los
    // Angeles, 18:00 in London (winter time since 25 October).
    let (app, clock, fakes) = timed_app("2026-10-30T18:00:00Z").await;
    let notifier = app.get::<Notifier>().await;
    let kolkata = Person::in_zone(Some("Asia/Kolkata"));
    let los_angeles = Person::in_zone(Some("America/Los_Angeles")).phone("+1 213 555 0100");
    let london = Person::in_zone(Some("Europe/London")).phone("+44 7400 123456");
    for person in [&kolkata, &los_angeles, &london] {
        notifier
            .send(person, TrainingDue { course: "Phishing 101".into() })
            .await
            .unwrap();
    }

    assert_eq!(support::rows(&app).await.len(), 3);
    assert_eq!(fakes.sms.sent_to("+12135550100").len(), 1);
    assert_eq!(subjects(&app).await, ["Training due: Phishing 101"]);
    let rows = held(&app).await;
    assert_eq!(rows.len(), 4);
    assert!(rows
        .iter()
        .all(|row| row.deferral_reason() == Some(DeferralReason::SendWindow)));
    // Monday 09:00 in Kolkata (03:30 UTC), then in London (09:00 UTC).
    assert_eq!(rows[0].release_at, at("2026-11-02T03:30:00Z"));
    assert_eq!(rows[1].release_at, at("2026-11-02T03:30:00Z"));
    assert_eq!(rows[2].release_at, at("2026-11-02T09:00:00Z"));
    assert_eq!(rows[3].release_at, at("2026-11-02T09:00:00Z"));

    clock.set(at("2026-11-02T03:30:00Z"));
    assert_eq!(notifier.release_due().await.unwrap().sent, 2);
    clock.set(at("2026-11-02T09:00:00Z"));
    assert_eq!(notifier.release_due().await.unwrap().sent, 2);
    assert_eq!(fakes.sms.sent().len(), 3);
    assert_eq!(subjects(&app).await.len(), 3);
}

#[tokio::test]
async fn the_zone_comes_from_time_module_when_the_person_has_none() {
    let (builder, _clock, _fakes) = timed(NotifyOptions::default(), "2026-10-28T20:00:00Z");
    let acme = Uuid::now_v7();
    let ada = Person::in_zone(None);
    let bob = Person::in_zone(None).of(acme);
    let carol = Person::in_zone(None);
    let users = ZonesFake::new(ZoneLevel::User).owner(ZoneOwner::new(ada.id).zone("Asia/Kolkata"));
    let workspaces =
        ZonesFake::new(ZoneLevel::Workspace).owner(ZoneOwner::new(acme).country("AE"));
    let app = builder
        .module(TimeModule::for_root(TimeOptions {
            default_zone: "America/New_York".to_owned(),
            ..TimeOptions::default()
        }))
        .module(users.module())
        .module(workspaces.module())
        .build()
        .await;
    let notifier = app.get::<Notifier>().await;
    for person in [&ada, &bob, &carol] {
        notifier.send(person, DealUpdate::new("Acme")).await.unwrap();
    }
    let rows = held(&app).await;
    let release = |person: &Person| {
        rows.iter()
            .find(|row| row.notifiable_id == person.id.to_string())
            .unwrap()
            .release_at
    };
    // The user's zone; the workspace's country (Dubai, +04:00); the default.
    assert_eq!(release(&ada), at("2026-10-29T03:30:00Z"));
    assert_eq!(release(&bob), at("2026-10-29T05:00:00Z"));
    assert_eq!(release(&carol), at("2026-10-29T13:00:00Z"));
}

#[tokio::test]
async fn turning_a_channel_off_drops_what_is_held() {
    let (app, clock, _fakes) = timed_app("2026-10-28T08:00:00Z").await;
    let notifier = app.get::<Notifier>().await;
    let ada = Person::in_zone(Some("UTC"));
    notifier.send(&ada, DealUpdate::new("Acme")).await.unwrap();
    assert_eq!(held(&app).await.len(), 1);

    prefer(
        &app,
        &ada.owner(),
        json!({"categories": {"deals": {"channels": {"mail": false}}}}),
    )
    .await;
    clock.set(at("2026-10-28T09:00:00Z"));
    let released = notifier.release_due().await.unwrap();
    assert_eq!(
        released,
        Released {
            dropped: 1,
            ..Released::default()
        }
    );
    assert!(subjects(&app).await.is_empty());
    assert!(held(&app).await.is_empty());
}

#[tokio::test]
async fn releases_are_queued_with_notify_queue() {
    let dispatcher = std::sync::Arc::new(RecordingDispatcher::default());
    let (builder, clock, _fakes) = timed(
        NotifyOptions {
            queue: true,
            ..NotifyOptions::default()
        },
        "2026-10-28T08:00:00Z",
    );
    let app = builder
        .override_value::<dyn Dispatcher>(dispatcher.clone())
        .build()
        .await;
    let notifier = app.get::<Notifier>().await;
    let ada = Person::in_zone(Some("UTC"));
    notifier.send(&ada, DealUpdate::new("Acme")).await.unwrap();
    notifier.send(&ada, DealUpdate::new("Globex")).await.unwrap();
    // The database and SMS channels were queued; the mail is held.
    let queued_now = dispatcher.jobs().len();
    assert_eq!(queued_now, 4);

    clock.set(at("2026-10-28T09:00:00Z"));
    let released = notifier.release_due().await.unwrap();
    assert_eq!((released.digests, released.batched), (1, 1 + 1));
    let jobs = dispatcher.jobs();
    assert_eq!(jobs.len(), queued_now + 1);
    let digest = jobs.last().unwrap();
    assert_eq!(digest.job_type, "notify.send");
    let job: navree_notify::SendNotificationJob =
        serde_json::from_value(digest.payload.clone()).unwrap();
    assert_eq!(job.notification_type, navree_notify::DIGEST_TYPE);
    match &job.delivery {
        Delivery::Digest { digest, to, items } => {
            assert_eq!(*digest, Digest::Daily);
            assert_eq!(to, &[ada.email.clone()]);
            assert_eq!(items.len(), 2);
        }
        other => panic!("not a digest: {other:?}"),
    }
    assert!(held(&app).await.is_empty());

    // The worker sends it.
    support::run_job(&app, digest).await.unwrap();
    assert_eq!(subjects(&app).await, ["Your notifications"]);
}

#[tokio::test]
async fn a_failed_release_is_tried_again_then_dropped() {
    let (builder, clock, fakes) = timed(
        NotifyOptions {
            deferred: navree_notify::DeferredOptions {
                max_attempts: 2,
                retry_secs: 600,
                ..navree_notify::DeferredOptions::default()
            },
            ..NotifyOptions::default()
        },
        "2026-10-28T14:00:00Z",
    );
    let app = builder.build().await;
    let notifier = app.get::<Notifier>().await;
    let tokyo = Person::in_zone(Some("Asia/Tokyo"));
    prefer(
        &app,
        &tokyo.owner(),
        json!({"quiet_hours": {"start": "22:00", "end": "07:00"}, "channels": {"mail": false}}),
    )
    .await;
    notifier.send(&tokyo, Mention { by: "Ada".into() }).await.unwrap();
    assert_eq!(held(&app).await.len(), 1);

    fakes.sms.refuse("carrier down");
    clock.set(at("2026-10-28T22:00:00Z"));
    let released = notifier.release_due().await.unwrap();
    assert_eq!(
        released,
        Released {
            failed: 1,
            ..Released::default()
        }
    );
    let rows = held(&app).await;
    assert_eq!(rows[0].attempts, 1);
    assert_eq!(rows[0].release_at, at("2026-10-28T22:10:00Z"));
    // Not again before the retry time.
    clock.advance(Duration::from_secs(300));
    assert_eq!(notifier.release_due().await.unwrap(), Released::default());

    clock.advance(Duration::from_secs(300));
    let released = notifier.release_due().await.unwrap();
    assert_eq!(released.dropped, 1);
    assert!(held(&app).await.is_empty());
}

#[tokio::test]
async fn held_deliveries_join_the_unit_of_work() {
    let (app, _clock, _fakes) = timed_app("2026-10-28T08:00:00Z").await;
    let notifier = app.get::<Notifier>().await;
    let db = app.get::<navree_db::Db>().await;
    let ada = Person::in_zone(Some("UTC"));

    let uow = db.uow(None, None).await;
    notifier
        .send_in(&uow, &ada, DealUpdate::new("Acme"))
        .await
        .unwrap();
    uow.rollback().await.unwrap();
    assert!(held(&app).await.is_empty());
    assert!(support::rows(&app).await.is_empty());

    let uow = db.uow(None, None).await;
    notifier
        .send_in(&uow, &ada, DealUpdate::new("Acme"))
        .await
        .unwrap();
    uow.commit().await.unwrap();
    assert_eq!(held(&app).await.len(), 1);
    assert_eq!(support::rows(&app).await.len(), 1);
}

#[tokio::test]
async fn the_release_command_runs_and_the_schedule_is_registered() {
    let (app, clock, _fakes) = timed_app("2026-10-28T08:00:00Z").await;
    let notifier = app.get::<Notifier>().await;
    let ada = Person::in_zone(Some("UTC"));
    notifier.send(&ada, DealUpdate::new("Acme")).await.unwrap();
    clock.set(at("2026-10-28T09:00:00Z") + TimeDelta::seconds(5));

    let command = app
        .ctx()
        .commands()
        .into_iter()
        .find(|c| c.name == "notify:release")
        .expect("the module registers `notify:release`");
    assert!(command.about.contains("held"), "{}", command.about);
    app.run_command("notify:release", &[]).await.unwrap();
    assert_eq!(subjects(&app).await, ["Your notifications"]);
    assert!(held(&app).await.is_empty());

    let schedules = app.ctx().contributions::<navree_schedule::Schedule>();
    assert!(!schedules.is_empty(), "NotifyModule contributes a schedule");
}

#[tokio::test]
async fn without_a_database_nothing_is_held() {
    let app = navree_testing::TestApp::builder()
        .module(navree_view::ViewModule::for_root(navree_view::ViewOptions {
            dirs: vec![],
            reload: false,
        }))
        .module(support::MailTemplates)
        .module(navree_mail::MailModule::for_root(navree_mail::MailOptions {
            from: "Navree <noreply@navree.test>".to_owned(),
            transport: navree_mail::TransportConfig::Array,
            ..navree_mail::MailOptions::default()
        }))
        .module(navree_notify::NotifyModule::for_root(NotifyOptions::default()))
        .build()
        .await;
    let notifier = app.get::<Notifier>().await;
    let ada = Person::in_zone(Some("UTC"));
    let mut update = DealUpdate::new("Acme");
    update.channels = vec![Channel::Mail];
    notifier.send(&ada, update).await.unwrap();
    // The digest needs the database: the mail went out at once.
    assert_eq!(subjects(&app).await, ["Deal update: Acme"]);
}
