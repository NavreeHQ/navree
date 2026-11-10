//! Stored notifications as navree-orm models: the database channel writes
//! them through the ORM, and apps query (and prune) them with it.

use chrono::{TimeDelta, Utc};
use navree_db::Db;
use navree_notify::{Channel, Notifier, notifications};
use navree_orm::Model as _;
use serde_json::json;

use crate::support::{self, DealWon, TestUser};

/// `notifications`' model.
type Stored = notifications::Model;

#[tokio::test]
async fn stored_notifications_are_orm_models() {
    let app = support::app().await;
    let notifier = app.get::<Notifier>().await;
    let (alice, bob) = (TestUser::new(), TestUser::new());
    for (user, title) in [(&alice, "Agra"), (&alice, "Gulati"), (&bob, "Ilaichi")] {
        notifier
            .send(user, DealWon::via(&[Channel::Database], title))
            .await
            .unwrap();
    }

    let db = app.get::<Db>().await;
    assert_eq!(Stored::table_name(), "notifications");
    let mine = Stored::query()
        .where_(Stored::NOTIFIABLE_TYPE.eq("user"))
        .where_(Stored::NOTIFIABLE_ID.eq(alice.id.to_string()))
        .where_null(Stored::READ_AT)
        .order_by(Stored::CREATED_AT)
        .order_by(Stored::ID)
        .get(&*db)
        .await
        .unwrap();
    assert_eq!(mine.len(), 2);
    assert_eq!(mine[0].notification_type, "deal_won");
    assert_eq!(mine[0].data["title"], json!("Agra"));
    assert_eq!(mine[1].data["title"], json!("Gulati"));
    assert_eq!(
        Stored::query()
            .where_(Stored::NOTIFICATION_TYPE.eq("deal_won"))
            .count(&*db)
            .await
            .unwrap(),
        3,
        "the `type` column is the field `notification_type`"
    );
}

#[tokio::test]
async fn old_notifications_are_pruned_with_the_orm() {
    let app = support::app().await;
    let notifier = app.get::<Notifier>().await;
    let user = TestUser::new();
    notifier
        .send(&user, DealWon::via(&[Channel::Database], "Agra"))
        .await
        .unwrap();

    let db = app.get::<Db>().await;
    let tomorrow = Utc::now() + TimeDelta::days(1);
    let pruned = Stored::query()
        .where_(Stored::CREATED_AT.lt(tomorrow))
        .delete(&*db)
        .await
        .unwrap();
    assert_eq!(pruned, 1);
    assert!(support::rows(&app).await.is_empty());
}
