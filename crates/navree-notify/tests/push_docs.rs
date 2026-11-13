//! What `docs/digging-deeper/notification-channels` says of native app
//! push, checked against the crate: the configuration block parses and
//! validates, the example notification compiles and renders what the page
//! shows, and the names the tables use exist. The channel itself is
//! exercised in `tests/notify/devices.rs` and `tests/notify/native_drivers.rs`.
// The page's example is written as an app writes it, without doc comments.
#![allow(missing_docs)]

use navree_config::{Config, ConfigSection, ConfigSources};
use navree_notify::{
    ApnsOptions, Channel, ChannelError, DeviceTarget, FcmOptions, LiveSessions, NewPushDevice,
    Notifiable, Notification, PUSH_DEVICES_TABLE, PushDevice, PushDevices, PushDriver,
    PushEnvironment, PushFake, PushMessage, PushOptions, PushPlatform, Urgency,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// The configuration block of the page's "Configure the services" step,
/// with the secrets filled in as the environment would.
const DOCUMENTED: &str = r#"
[notify.push]
ttl_secs = 86400

[notify.push.apns]
team_id = "DEF123GHIJ"
key_id = "ABC123DEFG"
key = "from the environment"
bundle_ids = ["com.acme.crm"]

[notify.push.fcm]
service_account = "from the environment"
packages = ["com.acme.crm"]
"#;

#[test]
fn the_documented_configuration_parses_and_validates() {
    let value: toml::Value = toml::from_str(DOCUMENTED).unwrap();
    let json = serde_json::to_value(&value["notify"]).unwrap();
    let config = Config::load(&ConfigSources::new().set("notify", json)).unwrap();
    let options = config.typed::<PushOptions>().unwrap();
    options.validate().unwrap();
    assert_eq!(options.ttl_secs, 86_400);
    assert!(options.apns.is_configured() && options.fcm.is_configured());
    assert_eq!(options.apns.bundle_ids, ["com.acme.crm"]);
    assert_eq!(options.fcm.packages, ["com.acme.crm"]);
}

#[test]
fn the_keys_have_their_documented_defaults() {
    let PushOptions {
        ttl_secs,
        apns,
        fcm,
    } = PushOptions::default();
    assert_eq!(ttl_secs, 86_400);
    let ApnsOptions {
        team_id,
        key_id,
        key,
        sandbox_key_id,
        sandbox_key,
        bundle_ids,
        endpoint,
        sandbox_endpoint,
    } = apns;
    assert!(team_id.is_empty() && key_id.is_empty() && key.expose().is_empty());
    assert!(sandbox_key_id.is_none() && sandbox_key.expose().is_empty());
    assert!(bundle_ids.is_empty() && endpoint.is_none() && sandbox_endpoint.is_none());
    let FcmOptions {
        project_id,
        service_account,
        packages,
        endpoint,
        token_url,
    } = fcm;
    assert!(project_id.is_none() && service_account.expose().is_empty());
    assert!(packages.is_empty() && endpoint.is_none() && token_url.is_none());
    assert_eq!(PushOptions::KEY, "notify.push");
}

// The page's example, as written there (with `navree_notify` for
// `navree::notify`).

/// An order left the warehouse.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrderShipped {
    pub order: String,
    pub city: String,
}

impl Notification for OrderShipped {
    const TYPE: &'static str = "order_shipped";
    const CATEGORY: &'static str = "orders";

    fn via(&self, _notifiable: &dyn Notifiable) -> Vec<Channel> {
        vec![Channel::Database, Channel::Push]
    }

    fn to_push(&self, _notifiable: &dyn Notifiable) -> Option<PushMessage> {
        Some(
            PushMessage::new("Your order has shipped")
                .body(format!(
                    "Order {} is on its way to {}",
                    self.order, self.city
                ))
                .href(format!("/orders/{}", self.order))
                .collapse(format!("order-{}", self.order))
                .thread("orders"),
        )
    }
}

struct Customer;

impl Notifiable for Customer {
    fn notifiable_type(&self) -> &'static str {
        navree_notify::USER_NOTIFIABLE_TYPE
    }

    fn notifiable_id(&self) -> String {
        "0193f7a2-6c1e-7d4a-9b2e-5f8c3a1d0e42".to_owned()
    }

    fn route_mail(&self) -> Option<String> {
        None
    }
}

#[test]
fn the_example_renders_a_checked_message() {
    let shipped = OrderShipped {
        order: "A-1042".into(),
        city: "Amritsar".into(),
    };
    assert_eq!(shipped.via(&Customer), [Channel::Database, Channel::Push]);
    let push = shipped.to_push(&Customer).unwrap();
    assert!(push.check().is_ok());
    assert_eq!(push.href.as_deref(), Some("/orders/A-1042"));
    assert_eq!(
        push.body.as_deref(),
        Some("Order A-1042 is on its way to Amritsar")
    );
}

#[test]
fn the_documented_request_bodies_parse() {
    let device: NewPushDevice = serde_json::from_value(json!({
        "platform": "ios",
        "token": "740f4707bebcf74f9b7c25d48e3358945f6aa01da5ddb387462c7eaf61bb78ad",
        "app_id": "com.acme.crm",
        "environment": "sandbox",
    }))
    .unwrap();
    assert!(device.check().is_ok());
    assert_eq!(device.environment, PushEnvironment::Sandbox);
    let android: NewPushDevice = serde_json::from_value(json!({
        "platform": "android",
        "token": "dQw4w9WgXcQ:APA91bHun4MxP5egoKMwt2KZFBaFUH-1RYqx",
        "app_id": "com.acme.crm",
    }))
    .unwrap();
    assert_eq!(android.platform, PushPlatform::Android);
    assert_eq!(android.environment, PushEnvironment::Production);
}

#[allow(clippy::unused_async)]
async fn _documented_api(
    devices: &PushDevices,
    fake: &PushFake,
) -> Result<(), navree_notify::NotifyError> {
    let user = Customer;
    let device = NewPushDevice {
        platform: PushPlatform::Android,
        token: "token".into(),
        app_id: "com.acme.crm".into(),
        environment: PushEnvironment::Production,
    };
    let saved: PushDevice = devices.register(&user, "session", device).await?;
    let _: bool = devices.unregister(&user, &saved.token).await?;
    let _: Vec<PushDevice> = devices.of("user", "id").await?;
    let _: bool = devices.delete(saved.id).await?;
    let _: u64 = devices.end_session("session").await?;
    let _: u64 = devices.end_all("user", &["id".to_owned()]).await?;
    let _ = fake.sent_to("token");
    fake.expire("token");
    let _ = fake.driver();
    Ok(())
}

#[test]
fn the_documented_names_exist() {
    assert_eq!(PUSH_DEVICES_TABLE, "push_devices");
    assert_eq!(Channel::Push.as_str(), "push");
    let _ = (
        std::any::type_name::<dyn PushDriver>(),
        std::any::type_name::<dyn LiveSessions>(),
        std::any::type_name::<DeviceTarget>(),
    );
    let message = PushMessage::silent()
        .data("refresh", "inbox")
        .urgency(Urgency::High)
        .ttl_secs(60)
        .badge(1)
        .sound("default")
        .mutable_content();
    assert!(!message.is_silent());
    let error = ChannelError::InvalidMessage("href: x".into());
    assert!(error.to_string().starts_with("the message cannot be sent"));
}
