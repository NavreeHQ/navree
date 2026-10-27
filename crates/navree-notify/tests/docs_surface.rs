//! Every public item `docs/digging-deeper/notification-channels` names
//! (types, fields, methods, configuration keys and defaults, tables, routes,
//! the command and the fakes' helpers), so a rename fails the build or this
//! test. The behavior is exercised in `tests/notify`; the page's examples
//! run in the facade's `tests/notification_channels_docs.rs`.

use navree_config::{Config, ConfigSection, ConfigSources};
use navree_notify::{
    Channel, ChannelCallback, ChannelError, ConnectionTokens, DELIVERIES_TABLE, DeliveryLog,
    DeliveryStatus, InboundMessage, Ingested, Keyword, LogSms, LogWhatsApp, MetaOptions,
    MetaWhatsApp, NOTIFICATIONS_TABLE, NewPushSubscription, NotificationDelivery,
    NotificationOptOut, NotifyError, NotifyOptions, OPT_IN_KEYWORDS, OPT_OUT_KEYWORDS,
    OPT_OUTS_TABLE, OptOutSource, OptOuts, PUSH_SUBSCRIPTIONS_TABLE, PushKeys, PushSubscription,
    PushSubscriptions, PushTarget, Sent, SentMessage, SlackApi, SlackDriver, SlackFake,
    SlackMessage, SlackOptions, SlackRoute, SmsDriver, SmsDriverKind, SmsFake, SmsMessage,
    SmsOptions, SnsOptions, SnsSms, StatusUpdate, TeamsApi, TeamsDriver, TeamsFake, TeamsMessage,
    TeamsOptions, TeamsRoute, TwilioOptions, TwilioSms, TwilioWhatsApp, Urgency, VapidWebPush,
    VonageOptions, VonageSms, WebPushDriver, WebPushFake, WebPushMessage, WebPushOptions,
    WhatsAppDriver, WhatsAppDriverKind, WhatsAppFake, WhatsAppMessage, WhatsAppOptions,
    WhatsAppTemplate,
};

/// The configuration block of the page's "Configuration" section.
const DOCUMENTED: &str = r#"
[notify]
deliveries = true

[notify.slack]
bot_token = "xoxb-test"

[notify.teams]
app_id = "6f3c1f0e-2a4b-4c5d-8e9f-0a1b2c3d4e5f"
app_password = "secret"
tenant = "botframework.com"

[notify.sms]
driver = "twilio"
from = "+15550001111"
default_region = "US"
status_callback = "https://api.acme.test/api/webhooks/notify/twilio"

[notify.whatsapp]
driver = "meta"
from = "+15550001111"

[notify.web_push]
public_key = "BGp8"
private_key = "key"
subject = "mailto:ops@acme.test"
ttl_secs = 86400

[notify.twilio]
account_sid = "AC0123456789abcdef0123456789abcdef"
auth_token = "token"

[notify.meta]
phone_number_id = "105589345678901"
access_token = "token"
"#;

#[test]
fn the_documented_configuration_parses_and_validates() {
    let value: toml::Value = toml::from_str(DOCUMENTED).unwrap();
    let json = serde_json::to_value(&value["notify"]).unwrap();
    let config = Config::load(&ConfigSources::new().set("notify", json)).unwrap();
    let options = config.typed::<NotifyOptions>().unwrap();
    options.validate().unwrap();
    assert_eq!(options.sms.driver, Some(SmsDriverKind::Twilio));
    assert_eq!(options.whatsapp.driver, Some(WhatsAppDriverKind::Meta));
    assert_eq!(options.teams.tenant, "botframework.com");
}

#[test]
fn the_configuration_keys_have_their_documented_defaults() {
    let NotifyOptions {
        queue,
        routes,
        deliveries,
        slack,
        teams,
        sms,
        whatsapp,
        web_push,
        twilio,
        vonage,
        sns,
        meta,
    } = NotifyOptions::default();
    assert!(!queue && routes && deliveries);

    let SlackOptions {
        bot_token,
        endpoint,
    } = slack;
    assert!(bot_token.expose().is_empty() && endpoint.is_none());

    let TeamsOptions {
        app_id,
        app_password,
        tenant,
        token_url,
    } = teams;
    assert_eq!(tenant, "botframework.com");
    assert!(app_id.is_none() && app_password.expose().is_empty() && token_url.is_none());

    let SmsOptions {
        driver,
        from,
        default_region,
        status_callback,
    } = sms;
    assert!(driver.is_none() && from.is_none() && default_region.is_none());
    assert!(status_callback.is_none());

    let WhatsAppOptions {
        driver,
        from,
        status_callback,
    } = whatsapp;
    assert!(driver.is_none() && from.is_none() && status_callback.is_none());

    let WebPushOptions {
        public_key,
        private_key,
        subject,
        ttl_secs,
    } = web_push;
    assert_eq!(ttl_secs, 86_400);
    assert!(public_key.is_none() && private_key.expose().is_empty() && subject.is_empty());

    let TwilioOptions {
        account_sid,
        auth_token,
        api_key,
        endpoint,
    } = twilio;
    assert!(account_sid.is_empty() && auth_token.expose().is_empty());
    assert!(api_key.is_none() && endpoint.is_none());

    let VonageOptions {
        api_key,
        api_secret,
        endpoint,
    } = vonage;
    assert!(api_key.is_empty() && api_secret.expose().is_empty() && endpoint.is_none());

    let SnsOptions {
        region,
        access_key_id,
        secret_access_key,
        session_token,
        sms_type,
        endpoint,
    } = sns;
    assert_eq!(sms_type, "Transactional");
    assert!(region.is_empty() && access_key_id.is_empty());
    assert!(secret_access_key.expose().is_empty() && session_token.is_none());
    assert!(endpoint.is_none());

    let MetaOptions {
        phone_number_id,
        access_token,
        api_version,
        endpoint,
    } = meta;
    assert_eq!(api_version, "v21.0");
    assert!(phone_number_id.is_empty() && access_token.expose().is_empty());
    assert!(endpoint.is_none());

    for (kind, name) in [
        (SmsDriverKind::Twilio, "twilio"),
        (SmsDriverKind::Vonage, "vonage"),
        (SmsDriverKind::Sns, "sns"),
        (SmsDriverKind::Log, "log"),
    ] {
        assert_eq!(serde_json::to_value(kind).unwrap(), name);
    }
    for (kind, name) in [
        (WhatsAppDriverKind::Twilio, "twilio"),
        (WhatsAppDriverKind::Meta, "meta"),
        (WhatsAppDriverKind::Log, "log"),
    ] {
        assert_eq!(serde_json::to_value(kind).unwrap(), name);
    }
}

#[test]
fn channels_tables_and_keywords_have_their_documented_names() {
    let names: Vec<&str> = [
        Channel::Slack,
        Channel::MicrosoftTeams,
        Channel::Sms,
        Channel::WhatsApp,
        Channel::WebPush,
    ]
    .map(Channel::as_str)
    .to_vec();
    assert_eq!(names, ["slack", "teams", "sms", "whatsapp", "web_push"]);
    assert_eq!(NOTIFICATIONS_TABLE, "notifications");
    assert_eq!(DELIVERIES_TABLE, "notification_deliveries");
    assert_eq!(OPT_OUTS_TABLE, "notification_opt_outs");
    assert_eq!(PUSH_SUBSCRIPTIONS_TABLE, "push_subscriptions");
    assert_eq!(
        OPT_OUT_KEYWORDS,
        [
            "STOP",
            "STOPALL",
            "UNSUBSCRIBE",
            "CANCEL",
            "END",
            "QUIT",
            "OPTOUT",
            "REVOKE"
        ]
    );
    assert_eq!(OPT_IN_KEYWORDS, ["START", "YES", "UNSTOP", "OPTIN"]);
    let statuses = [
        DeliveryStatus::Sent,
        DeliveryStatus::Delivered,
        DeliveryStatus::Read,
        DeliveryStatus::Failed,
        DeliveryStatus::OptedOut,
    ]
    .map(DeliveryStatus::as_str);
    assert_eq!(
        statuses,
        ["sent", "delivered", "read", "failed", "opted_out"]
    );
    assert_eq!(
        [
            OptOutSource::Keyword,
            OptOutSource::Provider,
            OptOutSource::Manual
        ]
        .map(OptOutSource::as_str),
        ["keyword", "provider", "manual"]
    );
    assert_eq!(
        [
            Urgency::VeryLow,
            Urgency::Low,
            Urgency::Normal,
            Urgency::High
        ]
        .map(Urgency::as_str),
        ["very-low", "low", "normal", "high"]
    );
    let _: Option<Keyword> = navree_notify::keyword("STOP");
}

/// The documented columns of `notification_deliveries`.
#[test]
fn the_delivery_log_has_its_documented_columns() {
    let NotificationDelivery {
        id: _,
        notification_id: _,
        notification_type: _,
        notifiable_type: _,
        notifiable_id: _,
        channel: _,
        provider: _,
        recipient: _,
        provider_message_id: _,
        status: _,
        error: _,
        created_at: _,
        updated_at: _,
    } = NotificationDelivery::default();
    let NotificationOptOut {
        id: _,
        channel: _,
        address: _,
        source: _,
        created_at: _,
    } = NotificationOptOut::default();
    let PushSubscription {
        id: _,
        notifiable_type: _,
        notifiable_id: _,
        endpoint: _,
        endpoint_hash: _,
        p256dh: _,
        auth: _,
        user_agent: _,
        created_at: _,
        last_used_at: _,
    } = PushSubscription::default();
    let Ingested {
        updated: _,
        opted_out: _,
        opted_in: _,
    } = Ingested::default();
}

/// The builders and fake helpers the page names.
#[allow(clippy::unused_async)]
async fn _documented_api(
    log: &DeliveryLog,
    opt_outs: &OptOuts,
    push: &PushSubscriptions,
    app: &navree_kernel::AppContext,
) -> Result<(), NotifyError> {
    let _ = SlackMessage::new("x")
        .blocks(serde_json::json!([]))
        .thread("1")
        .to(SlackRoute::channel("C1").connection(uuid::Uuid::nil()));
    let _ = SlackRoute::webhook("https://hooks.slack.com/x");
    let _ = TeamsMessage::new("x")
        .card(serde_json::json!({}))
        .summary("s")
        .to(TeamsRoute::conversation("https://smba", "a:1"));
    let _ = TeamsRoute::webhook("https://x");
    let _ = SmsMessage::new("x").from("Acme").to("+1");
    let _ = WhatsAppMessage::text("x").to("+1").from("+1");
    let _ = WhatsAppMessage::template(WhatsAppTemplate::new("t", "en_US").parameter("p"));
    let push_message = WebPushMessage::new("t")
        .body("b")
        .icon("i")
        .url("/u")
        .tag("t")
        .data(serde_json::json!({}))
        .ttl_secs(1)
        .urgency(Urgency::High)
        .topic("x");
    let _ = push_message.payload();

    let _ = log.for_notification(uuid::Uuid::nil()).await?;
    let _ = log.find_by_message("twilio", "SM1").await?;
    let _: Ingested = log.ingest(ChannelCallback::from_twilio(b"")).await?;
    let _ = ChannelCallback::from_vonage(b"{}");
    let _ = ChannelCallback::from_meta(b"{}");
    let _ = log
        .apply(&StatusUpdate::new(
            "twilio",
            "SM1",
            DeliveryStatus::Delivered,
        ))
        .await?;
    let _: Option<&InboundMessage> = None;

    opt_outs
        .opt_out(Channel::Sms, "+1", OptOutSource::Manual)
        .await?;
    opt_outs.opt_in(Channel::Sms, "+1").await?;
    opt_outs.is_opted_out(Channel::Sms, "+1").await?;
    opt_outs.list(Channel::Sms).await?;

    let user = TestUser;
    let subscription = NewPushSubscription {
        endpoint: "https://x".into(),
        expiration_time: None,
        keys: PushKeys {
            p256dh: String::new(),
            auth: String::new(),
        },
    };
    let saved = push.subscribe(&user, subscription, None).await?;
    push.unsubscribe(&user, "https://x").await?;
    push.of("user", "1").await?;
    push.delete(saved.id).await?;

    let sms = SmsFake::new();
    let _: std::sync::Arc<dyn SmsDriver> = sms.driver();
    let _: Vec<Sent<String, SmsMessage>> = sms.sent();
    let _ = sms.sent_to("+1");
    sms.refuse("x");
    sms.accept();
    sms.opt_out_at_provider("+1");
    sms.deliver(app, "+1").await?;
    sms.fail(app, "+1").await?;
    sms.reply(app, "+1", "STOP").await?;
    sms.assert_sent(|_| true);
    sms.assert_nothing_sent();
    let whatsapp = WhatsAppFake::new();
    let _: std::sync::Arc<dyn WhatsAppDriver> = whatsapp.driver();
    whatsapp.read(app, "+1").await?;
    let _: std::sync::Arc<dyn SlackDriver> = SlackFake::new().driver();
    let _: std::sync::Arc<dyn TeamsDriver> = TeamsFake::new().driver();
    let web_push = WebPushFake::new();
    web_push.expire("https://x");
    let _: std::sync::Arc<dyn WebPushDriver> = web_push.driver();
    let _ = web_push.sent_to("https://x");
    Ok(())
}

struct TestUser;

impl navree_notify::Notifiable for TestUser {
    fn notifiable_type(&self) -> &'static str {
        "user"
    }

    fn notifiable_id(&self) -> String {
        "1".into()
    }

    fn route_mail(&self) -> Option<String> {
        None
    }
}

/// The builders, services and fake helpers above type-check (the function
/// is never called: it needs a booted app).
#[test]
fn the_documented_api_type_checks() {
    let _ = _documented_api;
}

/// The drivers and errors the page names exist with their documented shapes.
#[test]
fn drivers_and_errors_have_their_documented_names() {
    let _ = (
        std::any::type_name::<SlackApi>(),
        std::any::type_name::<TeamsApi>(),
        std::any::type_name::<TwilioSms>(),
        std::any::type_name::<TwilioWhatsApp>(),
        std::any::type_name::<VonageSms>(),
        std::any::type_name::<SnsSms>(),
        std::any::type_name::<MetaWhatsApp>(),
        std::any::type_name::<VapidWebPush>(),
        std::any::type_name::<LogSms>(),
        std::any::type_name::<LogWhatsApp>(),
        std::any::type_name::<dyn ConnectionTokens>(),
        std::any::type_name::<PushTarget>(),
    );
    let refused = ChannelError::Refused {
        provider: "twilio".into(),
        status: 400,
        code: Some("21211".into()),
        message: "Invalid 'To' Phone Number".into(),
    };
    for error in [
        refused,
        ChannelError::OptedOut {
            provider: "twilio".into(),
        },
        ChannelError::InvalidRecipient("x".into()),
        ChannelError::Gone,
        ChannelError::Config("x".into()),
    ] {
        let error = NotifyError::Channel {
            channel: Channel::Sms,
            source: error,
        };
        assert!(error.to_string().starts_with("the sms channel failed"));
    }
    let _ = NotifyError::InvalidRecipient("555".into());
    let _ = SentMessage::new("twilio").id("SM1");
}
