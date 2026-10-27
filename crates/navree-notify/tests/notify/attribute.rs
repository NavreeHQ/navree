//! `#[notification]`: the category, digest and urgency of a notification
//! set by the attribute, read back through the trait's constants and the
//! catalog.

use navree_notify::{
    Channel, Digest, Notifiable, Notification, NotificationCatalog, NotificationKind, NotifyModule,
    NotifyOptions, notification,
};
use serde::{Deserialize, Serialize};

use crate::support;

#[derive(Serialize, Deserialize)]
struct DealWon {
    title: String,
}

#[notification(category = "deals", digest = "daily")]
impl Notification for DealWon {
    const TYPE: &'static str = "deal_won";

    fn via(&self, _n: &dyn Notifiable) -> Vec<Channel> {
        vec![Channel::Database, Channel::Mail]
    }
}

#[derive(Serialize, Deserialize)]
struct SignInCode {
    code: String,
}

#[notification(type = "sign_in_code", category = "security", urgent)]
impl Notification for SignInCode {
    fn via(&self, _n: &dyn Notifiable) -> Vec<Channel> {
        vec![Channel::Sms]
    }
}

#[derive(Serialize, Deserialize)]
struct Plain;

#[notification]
impl Notification for Plain {
    const TYPE: &'static str = "plain";

    fn via(&self, _n: &dyn Notifiable) -> Vec<Channel> {
        vec![Channel::Database]
    }
}

#[test]
fn the_attribute_sets_the_constants() {
    assert_eq!(DealWon::CATEGORY, "deals");
    assert_eq!(DealWon::DIGEST, Digest::Daily);
    const { assert!(!DealWon::URGENT) };
    assert_eq!(SignInCode::TYPE, "sign_in_code");
    assert_eq!(SignInCode::CATEGORY, "security");
    assert_eq!(SignInCode::DIGEST, Digest::Off);
    const { assert!(SignInCode::URGENT) };
    // Without arguments, the trait's defaults: the category is the type.
    assert_eq!(Plain::CATEGORY, "plain");
    assert_eq!(Plain::DIGEST, Digest::Off);
}

#[tokio::test]
async fn registered_types_fill_the_catalog() {
    let app = support::builder(NotifyOptions::default())
        .module(NotifyModule::for_feature::<DealWon>())
        .module(NotifyModule::for_feature::<SignInCode>())
        .build()
        .await;
    let catalog = app.get::<NotificationCatalog>().await;
    assert_eq!(
        catalog.kinds(),
        [
            NotificationKind {
                notification_type: "deal_won",
                category: "deals",
                digest: Digest::Daily,
                urgent: false,
            },
            NotificationKind {
                notification_type: "sign_in_code",
                category: "security",
                digest: Digest::Off,
                urgent: true,
            },
        ]
    );
}
