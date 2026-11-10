//! Touching parents: a child's save or delete sets its parents' update
//! timestamps (only when it changed), suspended inside `without_touching`;
//! many-to-many writes touch the parent when the related model touches the
//! relation.

use chrono::Duration;
use navree_contracts::FixedClock;
use navree_db::Db;
use navree_orm::prelude::*;
use navree_orm::relations::{is_ignoring_touch, without_touching, without_touching_on};

use crate::fixtures::seed;
use crate::models::{Activity, Company, Deal, Tag};
use crate::support::{app, epoch};

fn advance(clock: &FixedClock, minutes: i64) {
    clock.advance(Duration::minutes(minutes).to_std().expect("forward"));
}

async fn updated_at(db: &Db, deal: i64) -> chrono::DateTime<chrono::Utc> {
    Deal::query()
        .with_trashed()
        .find_or_fail(db, deal)
        .await
        .unwrap()
        .updated_at
}

#[tokio::test]
async fn saving_or_deleting_a_child_touches_its_parents() {
    let (_app, db, clock) = app(Vec::new()).await;
    let db = &*db;
    let crm = seed(db).await;
    let deal = crm.renewal.id;
    assert_eq!(updated_at(db, deal).await, epoch());

    advance(&clock, 5);
    let mut activity = crm.renewal.activities().first(db).await.unwrap().unwrap();
    activity.minutes = 31;
    activity.save(db).await.unwrap();
    assert_eq!(updated_at(db, deal).await, epoch() + Duration::minutes(5));

    // A save that changes nothing touches nothing.
    advance(&clock, 5);
    activity.save(db).await.unwrap();
    assert_eq!(updated_at(db, deal).await, epoch() + Duration::minutes(5));

    // Creating through the relation, and deleting, touch.
    advance(&clock, 5);
    let mut created = crm
        .renewal
        .activities()
        .create(db, Activity::new("note", 1))
        .await
        .unwrap();
    assert_eq!(updated_at(db, deal).await, epoch() + Duration::minutes(15));
    advance(&clock, 5);
    created.delete(db).await.unwrap();
    assert_eq!(updated_at(db, deal).await, epoch() + Duration::minutes(20));
    assert_eq!(Activity::touched_relations(), ["deal"]);
}

#[tokio::test]
async fn touching_is_suspended_inside_a_scope() {
    let (_app, db, clock) = app(Vec::new()).await;
    let db = &*db;
    let crm = seed(db).await;
    let deal = crm.renewal.id;
    let mut activity = crm.renewal.activities().first(db).await.unwrap().unwrap();

    advance(&clock, 5);
    assert!(!is_ignoring_touch::<Deal>());
    without_touching(async {
        assert!(is_ignoring_touch::<Deal>());
        assert!(is_ignoring_touch::<Company>());
        activity.minutes = 1;
        activity.save(db).await.unwrap();
    })
    .await;
    assert_eq!(updated_at(db, deal).await, epoch());
    assert!(!is_ignoring_touch::<Deal>(), "only inside the scope");

    // Only the listed models.
    without_touching_on::<(Company,), _>(async {
        assert!(is_ignoring_touch::<Company>());
        assert!(!is_ignoring_touch::<Deal>());
        activity.minutes = 2;
        activity.save(db).await.unwrap();
    })
    .await;
    assert_eq!(updated_at(db, deal).await, epoch() + Duration::minutes(5));
    advance(&clock, 5);
    without_touching_on::<(Deal, Tag), _>(async {
        activity.minutes = 3;
        activity.save(db).await.unwrap();
    })
    .await;
    assert_eq!(updated_at(db, deal).await, epoch() + Duration::minutes(5));
}

#[tokio::test]
async fn many_to_many_writes_touch_the_parent_when_the_relation_is_touched() {
    let (_app, db, clock) = app(Vec::new()).await;
    let db = &*db;
    let crm = seed(db).await;
    let deal = crm.platform.id;
    let tags = || crm.platform.tags();

    // `Tag` touches `deals`: attaching tags to a deal touches the deal.
    advance(&clock, 1);
    tags().attach(db, [crm.vip.id]).await.unwrap();
    assert_eq!(updated_at(db, deal).await, epoch() + Duration::minutes(1));
    advance(&clock, 1);
    tags()
        .update_existing_pivot(db, crm.vip.id, [col("role").set("owner")])
        .await
        .unwrap();
    assert_eq!(updated_at(db, deal).await, epoch() + Duration::minutes(2));
    advance(&clock, 1);
    tags().sync(db, [crm.vip.id, crm.q1.id]).await.unwrap();
    assert_eq!(updated_at(db, deal).await, epoch() + Duration::minutes(3));
    // A sync that changes nothing touches nothing.
    advance(&clock, 1);
    tags().sync(db, [crm.vip.id, crm.q1.id]).await.unwrap();
    assert_eq!(updated_at(db, deal).await, epoch() + Duration::minutes(3));
    advance(&clock, 1);
    tags().toggle(db, [crm.q1.id]).await.unwrap();
    assert_eq!(updated_at(db, deal).await, epoch() + Duration::minutes(5));
    advance(&clock, 1);
    tags().detach(db, [crm.vip.id]).await.unwrap();
    assert_eq!(updated_at(db, deal).await, epoch() + Duration::minutes(6));

    // And a tag's own save touches the deals it is attached to.
    tags().attach(db, [crm.hot.id]).await.unwrap();
    advance(&clock, 10);
    let mut hot = Tag::find_or_fail(db, crm.hot.id).await.unwrap();
    hot.name = "HOT".into();
    hot.save(db).await.unwrap();
    assert_eq!(updated_at(db, deal).await, epoch() + Duration::minutes(16));
    assert_eq!(
        updated_at(db, crm.renewal.id).await,
        epoch(),
        "not attached to the tag"
    );
}
