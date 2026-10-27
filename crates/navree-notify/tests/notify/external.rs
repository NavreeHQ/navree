//! The Slack, Teams, SMS, WhatsApp and web push channels through the
//! notifier, with each driver faked: routing, the delivery log, failures
//! and queueing.

use std::sync::Arc;

use navree_contracts::Dispatcher;
use navree_notify::{
    Channel, ChannelError, Delivery, DeliveryStatus, NewPushSubscription, Notifier, NotifyError,
    NotifyOptions, PushKeys, PushSubscriptions, SendNotificationJob, SlackRoute, SmsDriver,
    TeamsRoute,
};
use navree_telemetry::TelemetryRecorder;

use crate::support::{
    self, Alert, Fakes, MEMBER_PHONE, MEMBER_WHATSAPP, Member, RecordingDispatcher,
};

const EXTERNAL: [Channel; 4] = [
    Channel::Slack,
    Channel::MicrosoftTeams,
    Channel::Sms,
    Channel::WhatsApp,
];

#[tokio::test]
async fn each_channel_goes_to_the_notifiables_route() {
    let (app, fakes) = support::channel_app().await;
    let member = Member::new();

    app.get::<Notifier>()
        .await
        .send(&member, Alert::via(&EXTERNAL, "Training due today"))
        .await
        .unwrap();

    let slack = fakes.slack.sent();
    assert_eq!(slack.len(), 1);
    assert_eq!(slack[0].to, SlackRoute::channel("C0123456789"));
    assert_eq!(slack[0].message.text, "Training due today");
    let teams = fakes.teams.sent();
    assert_eq!(
        teams[0].to,
        TeamsRoute::conversation("https://smba.trafficmanager.net/emea/", "a:1Xyz")
    );
    // Numbers are sent in E.164, read in notify.sms.default_region.
    assert_eq!(fakes.sms.sent_to(MEMBER_PHONE)[0].body, "Training due today");
    assert_eq!(
        fakes.whatsapp.sent_to(MEMBER_WHATSAPP)[0].body.as_deref(),
        Some("Training due today")
    );
    assert!(support::rows(&app).await.is_empty(), "no database channel");
}

#[tokio::test]
async fn every_delivery_is_recorded_with_its_provider_id() {
    let (app, fakes) = support::channel_app().await;
    let member = Member::new();

    let id = app
        .get::<Notifier>()
        .await
        .send(&member, Alert::via(&EXTERNAL, "Training due today"))
        .await
        .unwrap();

    let deliveries = support::deliveries(&app).await;
    let mut seen: Vec<(String, Option<String>)> = deliveries
        .iter()
        .map(|d| (d.channel.clone(), d.recipient.clone()))
        .collect();
    seen.sort();
    assert_eq!(
        seen,
        [
            ("slack".to_owned(), Some("C0123456789".to_owned())),
            ("sms".to_owned(), Some(MEMBER_PHONE.to_owned())),
            ("teams".to_owned(), Some("a:1Xyz".to_owned())),
            ("whatsapp".to_owned(), Some(MEMBER_WHATSAPP.to_owned())),
        ]
    );
    for delivery in &deliveries {
        assert_eq!(delivery.notification_id, id);
        assert_eq!(delivery.notification_type, "alert");
        assert_eq!(delivery.notifiable_id, member.id.to_string());
        assert_eq!(delivery.provider.as_deref(), Some("fake"));
        assert_eq!(delivery.delivery_status(), Some(DeliveryStatus::Sent));
        assert!(delivery.provider_message_id.is_some());
    }
    let sms = deliveries.iter().find(|d| d.channel == "sms").unwrap();
    assert_eq!(
        sms.provider_message_id.as_deref(),
        Some(fakes.sms.sent()[0].id.as_str())
    );
}

#[tokio::test]
async fn webhook_routes_are_recorded_by_host_only() {
    let (app, fakes) = support::channel_app().await;
    let mut alert = Alert::via(&[Channel::Slack], "Deploy finished");
    alert.slack_to = Some(SlackRoute::webhook(
        "https://hooks.slack.com/services/T000/B000/XXXXSECRET",
    ));

    app.get::<Notifier>()
        .await
        .send(&Member::new(), alert)
        .await
        .unwrap();

    assert!(matches!(&fakes.slack.sent()[0].to, SlackRoute::Webhook { .. }));
    let deliveries = support::deliveries(&app).await;
    assert_eq!(
        deliveries[0].recipient.as_deref(),
        Some("webhook:hooks.slack.com")
    );
}

#[tokio::test]
async fn a_message_route_wins_over_the_notifiables() {
    let (app, fakes) = support::channel_app().await;
    let mut alert = Alert::via(&[Channel::Sms], "Your code is 481 516");
    alert.sms_to = Some("+1 212 555 0199".to_owned());

    app.get::<Notifier>()
        .await
        .send(&Member::new(), alert)
        .await
        .unwrap();

    assert_eq!(fakes.sms.sent()[0].to, "+12125550199");
}

#[tokio::test]
async fn channels_without_a_route_are_skipped() {
    let (app, fakes) = support::channel_app().await;

    app.get::<Notifier>()
        .await
        .send(&Member::unreachable(), Alert::via(&EXTERNAL, "hi"))
        .await
        .unwrap();

    fakes.slack.assert_nothing_sent();
    fakes.teams.assert_nothing_sent();
    fakes.sms.assert_nothing_sent();
    fakes.whatsapp.assert_nothing_sent();
    assert!(support::deliveries(&app).await.is_empty());
}

#[tokio::test]
async fn a_refused_message_fails_its_channel_only_and_is_recorded() {
    let (app, fakes) = support::channel_app().await;
    fakes.slack.refuse("channel_not_found");

    let error = app
        .get::<Notifier>()
        .await
        .send(&Member::new(), Alert::via(&[Channel::Slack, Channel::Sms], "hi"))
        .await
        .unwrap_err();

    match error {
        NotifyError::Channel {
            channel: Channel::Slack,
            source: ChannelError::Refused { message, .. },
        } => assert_eq!(message, "channel_not_found"),
        other => panic!("unexpected error: {other:?}"),
    }
    assert_eq!(fakes.sms.sent().len(), 1, "the other channel still delivers");
    let deliveries = support::deliveries(&app).await;
    let slack = deliveries.iter().find(|d| d.channel == "slack").unwrap();
    assert_eq!(slack.delivery_status(), Some(DeliveryStatus::Failed));
    assert!(slack.error.as_deref().unwrap().contains("channel_not_found"));

    // A retry updates the same row.
    fakes.slack.accept();
    let notifier = app.get::<Notifier>().await;
    let job = SendNotificationJob {
        id: slack.notification_id,
        notification_type: "alert".to_owned(),
        notifiable_type: "user".to_owned(),
        notifiable_id: slack.notifiable_id.clone(),
        created_at: slack.created_at,
        delivery: Delivery::Slack {
            route: SlackRoute::channel("C0123456789"),
            message: navree_notify::SlackMessage::new("hi"),
        },
    };
    let envelope = navree_contracts::JobEnvelope::of(&job).unwrap();
    support::run_job(&app, &envelope).await.unwrap();
    drop(notifier);
    let deliveries = support::deliveries(&app).await;
    let slack: Vec<_> = deliveries.iter().filter(|d| d.channel == "slack").collect();
    assert_eq!(slack.len(), 1);
    assert_eq!(slack[0].delivery_status(), Some(DeliveryStatus::Sent));
    assert_eq!(slack[0].error, None);
}

#[tokio::test]
async fn an_invalid_number_fails_the_sms_channel() {
    let (app, fakes) = support::channel_app().await;
    let mut member = Member::new();
    member.phone = Some("555".to_owned());

    let error = app
        .get::<Notifier>()
        .await
        .send(&member, Alert::via(&[Channel::Sms], "hi"))
        .await
        .unwrap_err();

    assert!(matches!(error, NotifyError::InvalidRecipient(_)), "{error:?}");
    fakes.sms.assert_nothing_sent();
    let deliveries = support::deliveries(&app).await;
    assert_eq!(deliveries[0].recipient.as_deref(), Some("555"));
    assert_eq!(deliveries[0].delivery_status(), Some(DeliveryStatus::Failed));
}

#[tokio::test]
async fn sms_without_a_driver_is_unavailable() {
    let app = support::builder(support::channel_options()).build().await;

    let error = app
        .get::<Notifier>()
        .await
        .send(&Member::new(), Alert::via(&[Channel::Sms], "hi"))
        .await
        .unwrap_err();

    match error {
        NotifyError::Unavailable {
            channel: Some(Channel::Sms),
            reason,
        } => assert!(reason.contains("notify.sms.driver"), "{reason}"),
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn the_log_driver_takes_messages_in_development() {
    let mut options = support::channel_options();
    options.sms.driver = Some(navree_notify::SmsDriverKind::Log);
    options.whatsapp.driver = Some(navree_notify::WhatsAppDriverKind::Log);
    let app = support::builder(options).build().await;

    app.get::<Notifier>()
        .await
        .send(&Member::new(), Alert::via(&[Channel::Sms, Channel::WhatsApp], "hi"))
        .await
        .unwrap();

    let deliveries = support::deliveries(&app).await;
    assert_eq!(deliveries.len(), 2);
    assert!(deliveries.iter().all(|d| d.provider.as_deref() == Some("log")));
}

#[tokio::test]
async fn recording_can_be_turned_off() {
    let fakes = Fakes::default();
    let app = fakes
        .install(support::builder(NotifyOptions {
            deliveries: false,
            ..support::channel_options()
        }))
        .build()
        .await;

    app.get::<Notifier>()
        .await
        .send(&Member::new(), Alert::via(&[Channel::Sms], "hi"))
        .await
        .unwrap();

    assert_eq!(fakes.sms.sent().len(), 1);
    assert!(support::deliveries(&app).await.is_empty());
}

#[tokio::test]
async fn channels_work_without_a_database_and_record_nothing() {
    let fakes = Fakes::default();
    let app = fakes
        .install(
            navree_testing::TestApp::builder()
                .config("http.per_ip_rate_limit", serde_json::Value::Null)
                .module(navree_notify::NotifyModule::for_root(NotifyOptions {
                    routes: false,
                    ..support::channel_options()
                })),
        )
        .build()
        .await;

    app.get::<Notifier>()
        .await
        .send_now(&Member::new(), Alert::via(&[Channel::Slack, Channel::Sms], "hi"))
        .await
        .unwrap();

    assert_eq!(fakes.slack.sent().len(), 1);
    assert_eq!(fakes.sms.sent().len(), 1);
}

#[tokio::test]
async fn queued_channels_carry_the_rendered_message() {
    let dispatcher = Arc::new(RecordingDispatcher::default());
    let fakes = Fakes::default();
    let app = fakes
        .install(support::builder(NotifyOptions {
            queue: true,
            ..support::channel_options()
        }))
        .override_value::<dyn Dispatcher>(dispatcher.clone())
        .build()
        .await;

    app.get::<Notifier>()
        .await
        .send(&Member::new(), Alert::via(&EXTERNAL, "Queued"))
        .await
        .unwrap();

    fakes.sms.assert_nothing_sent();
    let jobs = dispatcher.jobs();
    assert_eq!(jobs.len(), 4);
    let vias: Vec<&str> = jobs
        .iter()
        .map(|job| job.payload["delivery"]["via"].as_str().unwrap())
        .collect();
    assert_eq!(vias, ["slack", "teams", "sms", "whatsapp"]);
    for job in &jobs {
        support::run_job(&app, job).await.unwrap();
    }
    assert_eq!(fakes.sms.sent_to(MEMBER_PHONE)[0].body, "Queued");
    assert_eq!(fakes.teams.sent().len(), 1);
    assert_eq!(support::deliveries(&app).await.len(), 4);
}

#[tokio::test]
async fn delivered_channels_are_reported_to_telemetry() {
    let recorder = TelemetryRecorder::new();
    let fakes = Fakes::default();
    let app = fakes
        .install(support::builder(support::channel_options()))
        .module(recorder.module())
        .build()
        .await;

    app.get::<Notifier>()
        .await
        .send_now(&Member::new(), Alert::via(&EXTERNAL, "hi"))
        .await
        .unwrap();

    let mut channels: Vec<String> = recorder
        .notifications()
        .into_iter()
        .map(|n| n.channel)
        .collect();
    channels.sort();
    assert_eq!(channels, ["slack", "sms", "teams", "whatsapp"]);
}

#[tokio::test]
async fn web_push_goes_to_every_subscription_and_drops_gone_ones() {
    let (app, fakes) = support::channel_app().await;
    let member = Member::new();
    let push = app.get::<PushSubscriptions>().await;
    let phone = subscription("https://fcm.googleapis.com/fcm/send/phone");
    let laptop = subscription("https://updates.push.services.mozilla.com/wpush/v2/laptop");
    push.subscribe(&member, phone.clone(), Some("Android")).await.unwrap();
    push.subscribe(&member, laptop.clone(), Some("Firefox")).await.unwrap();
    fakes.web_push.expire(&laptop.endpoint);

    app.get::<Notifier>()
        .await
        .send(&member, Alert::via(&[Channel::WebPush], "Deal won"))
        .await
        .unwrap();

    let pushed = fakes.web_push.sent_to(&phone.endpoint);
    assert_eq!(pushed[0].title, "Deal won");
    assert_eq!(pushed[0].url.as_deref(), Some("/alerts"));
    let left = push.of("user", &member.id.to_string()).await.unwrap();
    assert_eq!(left.len(), 1, "the gone subscription is deleted");
    assert_eq!(left[0].endpoint, phone.endpoint);
    assert!(left[0].last_used_at.is_some());
    let deliveries = support::deliveries(&app).await;
    assert_eq!(deliveries.len(), 2);
    assert_eq!(
        deliveries
            .iter()
            .filter(|d| d.delivery_status() == Some(DeliveryStatus::Failed))
            .count(),
        1
    );
}

#[tokio::test]
async fn web_push_without_subscriptions_sends_nothing() {
    let recorder = TelemetryRecorder::new();
    let fakes = Fakes::default();
    let app = fakes
        .install(support::builder(support::channel_options()))
        .module(recorder.module())
        .build()
        .await;

    app.get::<Notifier>()
        .await
        .send(&Member::new(), Alert::via(&[Channel::WebPush], "hi"))
        .await
        .unwrap();

    fakes.web_push.assert_nothing_sent();
    assert!(recorder.notifications().is_empty(), "nothing was delivered");
}

#[tokio::test]
async fn an_app_driver_replaces_the_configured_one() {
    let fakes = Fakes::default();
    let mut options = support::channel_options();
    options.sms.driver = Some(navree_notify::SmsDriverKind::Log);
    let app = support::builder(options)
        .override_value::<dyn SmsDriver>(fakes.sms.driver())
        .build()
        .await;

    app.get::<Notifier>()
        .await
        .send(&Member::new(), Alert::via(&[Channel::Sms], "hi"))
        .await
        .unwrap();

    assert_eq!(fakes.sms.sent().len(), 1);
}

/// A browser subscription at `endpoint` (the RFC 8291 example keys).
pub fn subscription(endpoint: &str) -> NewPushSubscription {
    NewPushSubscription {
        endpoint: endpoint.to_owned(),
        expiration_time: None,
        keys: PushKeys {
            p256dh: "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4"
                .to_owned(),
            auth: "BTBZMqHH6r4Tts7J_aSIgg".to_owned(),
        },
    }
}
