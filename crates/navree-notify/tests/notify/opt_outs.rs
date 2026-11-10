//! STOP opt-outs and provider status callbacks on the SMS and WhatsApp
//! channels, played through the fakes.

use navree_notify::{
    Channel, ChannelCallback, DeliveryLog, DeliveryStatus, Ingested, Notifier, OptOutSource,
    OptOuts, StatusUpdate,
};

use crate::support::{self, Alert, MEMBER_PHONE, MEMBER_WHATSAPP, Member};

#[tokio::test]
async fn a_stop_reply_opts_the_number_out_until_start() {
    let (app, fakes) = support::channel_app().await;
    let notifier = app.get::<Notifier>().await;
    let member = Member::new();
    notifier
        .send(&member, Alert::via(&[Channel::Sms], "First"))
        .await
        .unwrap();

    let ingested = fakes
        .sms
        .reply(app.ctx(), MEMBER_PHONE, "stop")
        .await
        .unwrap();
    assert_eq!(
        ingested,
        Ingested {
            opted_out: 1,
            ..Ingested::default()
        }
    );

    notifier
        .send(&member, Alert::via(&[Channel::Sms], "Second"))
        .await
        .unwrap();
    assert_eq!(fakes.sms.sent().len(), 1, "an opted-out number is skipped");
    let deliveries = support::deliveries(&app).await;
    assert_eq!(
        deliveries[1].delivery_status(),
        Some(DeliveryStatus::OptedOut)
    );
    assert_eq!(deliveries[1].recipient.as_deref(), Some(MEMBER_PHONE));

    let ingested = fakes
        .sms
        .reply(app.ctx(), MEMBER_PHONE, "START")
        .await
        .unwrap();
    assert_eq!(ingested.opted_in, 1);
    notifier
        .send(&member, Alert::via(&[Channel::Sms], "Third"))
        .await
        .unwrap();
    assert_eq!(fakes.sms.sent().len(), 2);
}

#[tokio::test]
async fn an_opt_out_at_the_provider_is_recorded_and_is_not_an_error() {
    let (app, fakes) = support::channel_app().await;
    fakes.sms.opt_out_at_provider(MEMBER_PHONE);

    app.get::<Notifier>()
        .await
        .send(&Member::new(), Alert::via(&[Channel::Sms], "hi"))
        .await
        .expect("an opted-out recipient is not a failure");

    let opt_outs = app.get::<OptOuts>().await;
    assert!(
        opt_outs
            .is_opted_out(Channel::Sms, MEMBER_PHONE)
            .await
            .unwrap()
    );
    assert_eq!(
        opt_outs.list(Channel::Sms).await.unwrap()[0].source,
        "provider"
    );
    let deliveries = support::deliveries(&app).await;
    assert_eq!(
        deliveries[0].delivery_status(),
        Some(DeliveryStatus::OptedOut)
    );
}

#[tokio::test]
async fn opt_outs_are_per_channel_and_read_numbers_in_any_format() {
    let (app, fakes) = support::channel_app().await;
    let opt_outs = app.get::<OptOuts>().await;

    assert!(
        opt_outs
            .opt_out(Channel::WhatsApp, "+44 7400 123456", OptOutSource::Manual)
            .await
            .unwrap()
    );
    assert!(
        !opt_outs
            .opt_out(Channel::WhatsApp, "+447400123456", OptOutSource::Keyword)
            .await
            .unwrap(),
        "already opted out"
    );
    assert!(
        opt_outs
            .is_opted_out(Channel::WhatsApp, MEMBER_WHATSAPP)
            .await
            .unwrap()
    );
    assert!(
        !opt_outs
            .is_opted_out(Channel::Sms, MEMBER_WHATSAPP)
            .await
            .unwrap()
    );
    assert!(
        !opt_outs
            .is_opted_out(Channel::Sms, "(415) 555-0100")
            .await
            .unwrap()
    );
    assert!(
        opt_outs
            .opt_out(Channel::Sms, "555", OptOutSource::Manual)
            .await
            .is_err()
    );

    app.get::<Notifier>()
        .await
        .send(
            &Member::new(),
            Alert::via(&[Channel::WhatsApp, Channel::Sms], "hi"),
        )
        .await
        .unwrap();
    fakes.whatsapp.assert_nothing_sent();
    assert_eq!(fakes.sms.sent().len(), 1);

    assert!(
        opt_outs
            .opt_in(Channel::WhatsApp, MEMBER_WHATSAPP)
            .await
            .unwrap()
    );
    assert!(
        !opt_outs
            .opt_in(Channel::WhatsApp, MEMBER_WHATSAPP)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn provider_statuses_move_deliveries_forward_only() {
    let (app, fakes) = support::channel_app().await;
    app.get::<Notifier>()
        .await
        .send(&Member::new(), Alert::via(&[Channel::WhatsApp], "hi"))
        .await
        .unwrap();

    fakes
        .whatsapp
        .read(app.ctx(), MEMBER_WHATSAPP)
        .await
        .unwrap();
    let ingested = fakes
        .whatsapp
        .deliver(app.ctx(), MEMBER_WHATSAPP)
        .await
        .unwrap();

    assert_eq!(
        ingested.updated, 0,
        "a late `delivered` does not undo `read`"
    );
    let deliveries = support::deliveries(&app).await;
    assert_eq!(deliveries[0].delivery_status(), Some(DeliveryStatus::Read));
}

#[tokio::test]
async fn a_failed_status_keeps_the_providers_error() {
    let (app, _fakes) = support::channel_app().await;
    app.get::<Notifier>()
        .await
        .send(&Member::new(), Alert::via(&[Channel::Sms], "hi"))
        .await
        .unwrap();
    let delivery = support::deliveries(&app).await.remove(0);
    let message_id = delivery.provider_message_id.clone().unwrap();
    let log = app.get::<DeliveryLog>().await;

    let updated = log
        .apply(
            &StatusUpdate::new("fake", &message_id, DeliveryStatus::Failed).error(
                Some("30003".into()),
                Some("Unreachable destination handset".into()),
            ),
        )
        .await
        .unwrap()
        .expect("the delivery exists");

    assert_eq!(updated.delivery_status(), Some(DeliveryStatus::Failed));
    assert_eq!(
        updated.error.as_deref(),
        Some("Unreachable destination handset")
    );
    assert_eq!(
        log.find_by_message("fake", &message_id)
            .await
            .unwrap()
            .unwrap()
            .id,
        delivery.id
    );
    assert_eq!(
        log.for_notification(delivery.notification_id)
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(
        log.apply(&StatusUpdate::new(
            "fake",
            "unknown",
            DeliveryStatus::Delivered
        ))
        .await
        .unwrap()
        .is_none()
    );
}

#[tokio::test]
async fn statuses_are_matched_by_provider_and_message_id() {
    let (app, _fakes) = support::channel_app().await;
    app.get::<Notifier>()
        .await
        .send(&Member::new(), Alert::via(&[Channel::Sms], "hi"))
        .await
        .unwrap();
    let delivery = support::deliveries(&app).await.remove(0);
    let message_id = delivery.provider_message_id.unwrap();
    let log = app.get::<DeliveryLog>().await;

    // Another provider's message with the same id is not this delivery.
    let twilio = StatusUpdate::new("twilio", &message_id, DeliveryStatus::Failed)
        .error(Some("21610".into()), None);
    assert_eq!(
        log.ingest([ChannelCallback::Status(twilio)]).await.unwrap(),
        Ingested::default()
    );

    // Error 21610 means "opted out" from Twilio only.
    let fake = StatusUpdate::new("fake", &message_id, DeliveryStatus::Failed)
        .error(Some("21610".into()), None);
    let ingested = log.ingest([ChannelCallback::Status(fake)]).await.unwrap();
    assert_eq!(ingested.updated, 1);
    assert_eq!(ingested.opted_out, 0);
}

#[tokio::test]
async fn twilio_callbacks_update_twilio_deliveries_and_opt_outs() {
    let (app, _fakes) = support::channel_app().await;
    let log = app.get::<DeliveryLog>().await;
    // Record a delivery as the Twilio driver would.
    let notifier = app.get::<Notifier>().await;
    notifier
        .send(&Member::new(), Alert::via(&[Channel::Sms], "hi"))
        .await
        .unwrap();
    let delivery = support::deliveries(&app).await.remove(0);
    let db = app.get::<navree_db::Db>().await;
    use navree_orm::Model as _;
    navree_notify::NotificationDelivery::query()
        .where_(navree_notify::NotificationDelivery::ID.eq(delivery.id))
        .update(
            &*db,
            [
                navree_notify::NotificationDelivery::PROVIDER.set(Some("twilio".to_owned())),
                navree_notify::NotificationDelivery::PROVIDER_MESSAGE_ID
                    .set(Some("SM0123".to_owned())),
            ],
        )
        .await
        .unwrap();

    let callbacks = ChannelCallback::from_twilio(
        b"MessageSid=SM0123&MessageStatus=failed&ErrorCode=21610&To=%2B14155550100",
    );
    let ingested = log.ingest(callbacks).await.unwrap();

    assert_eq!(
        ingested,
        Ingested {
            updated: 1,
            opted_out: 1,
            opted_in: 0
        }
    );
    let opt_outs = app.get::<OptOuts>().await;
    assert!(
        opt_outs
            .is_opted_out(Channel::Sms, MEMBER_PHONE)
            .await
            .unwrap()
    );

    let inbound = ChannelCallback::from_twilio(
        b"MessageSid=SM9&From=%2B14155550100&To=%2B15550001111&Body=UNSTOP",
    );
    assert_eq!(log.ingest(inbound).await.unwrap().opted_in, 1);
    assert!(
        !opt_outs
            .is_opted_out(Channel::Sms, MEMBER_PHONE)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn the_fakes_name_a_missing_message() {
    let (app, fakes) = support::channel_app().await;
    assert!(fakes.sms.deliver(app.ctx(), MEMBER_PHONE).await.is_err());
}
