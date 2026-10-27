//! `first_or_new`, `first_or_create`, `create_or_first` and
//! `update_or_create`.

use navree_db::sea_orm::ConnectionTrait;
use navree_orm::{Assignment, Model};

use crate::models::{Setting, Tag};
use crate::support::{count_rows, sqlite};

#[tokio::test]
async fn first_or_new_finds_or_builds_without_saving() {
    let t = sqlite().await;
    let new = Tag::first_or_new(&t.db, [Tag::NAME.set("vip")], |tag| tag.color = "gold".into())
        .await
        .unwrap();
    assert!(!new.exists());
    assert_eq!((new.name.as_str(), new.color.as_str()), ("vip", "gold"));
    assert_eq!(count_rows(&t.db, "tags").await, 0);

    Tag::create(&t.db, Tag { name: "vip".into(), color: "red".into(), ..Tag::default() })
        .await
        .unwrap();
    let found = Tag::first_or_new(&t.db, [Tag::NAME.set("vip")], |tag| tag.color = "gold".into())
        .await
        .unwrap();
    assert!(found.exists());
    assert_eq!(found.color, "red", "fill only applies to new records");
}

#[tokio::test]
async fn first_or_create_inserts_once() {
    let t = sqlite().await;
    let created = Tag::first_or_create(&t.db, [Tag::NAME.set("vip")], |tag| tag.color = "gold".into())
        .await
        .unwrap();
    assert!(created.was_recently_created());
    let again = Tag::first_or_create(&t.db, [Tag::NAME.set("vip")], |tag| tag.color = "blue".into())
        .await
        .unwrap();
    assert!(!again.was_recently_created());
    assert_eq!(again.id, created.id);
    assert_eq!(again.color, "gold");
    assert_eq!(count_rows(&t.db, "tags").await, 1);
}

#[tokio::test]
async fn create_or_first_returns_the_record_that_won_a_unique_race() {
    let t = sqlite().await;
    Tag::create(&t.db, Tag { name: "vip".into(), color: "red".into(), ..Tag::default() })
        .await
        .unwrap();
    let existing = Tag::create_or_first(&t.db, [Tag::NAME.set("vip")], |tag| tag.color = "gold".into())
        .await
        .unwrap();
    assert!(!existing.was_recently_created());
    assert_eq!(existing.color, "red");

    // Inside a unit of work, the failed insert runs in a savepoint: the
    // transaction goes on.
    let uow = t.db.uow(None, None).await;
    let inside = Tag::create_or_first(&uow, [Tag::NAME.set("vip")], |tag| tag.color = "gold".into())
        .await
        .unwrap();
    assert_eq!(inside.color, "red");
    Tag::create(&uow, Tag { name: "new".into(), color: "green".into(), ..Tag::default() })
        .await
        .unwrap();
    uow.commit().await.unwrap();
    assert_eq!(count_rows(&t.db, "tags").await, 2);
}

#[tokio::test]
async fn update_or_create_updates_the_match_or_creates_one() {
    let t = sqlite().await;
    let created = Setting::update_or_create(&t.db, [Setting::KEY.set("theme")], |s| s.value = "dark".into())
        .await
        .unwrap();
    assert!(created.was_recently_created());
    assert_eq!(created.value, "dark");

    t.db.conn()
        .execute_unprepared("UPDATE settings SET value = 'light'")
        .await
        .unwrap();
    let updated = Setting::update_or_create(&t.db, [Setting::KEY.set("theme")], |s| s.value = "solarized".into())
        .await
        .unwrap();
    assert!(!updated.was_recently_created());
    assert_eq!(updated.value, "solarized");
    assert!(updated.was_changed_on([Setting::VALUE]));
    assert_eq!(Setting::find_or_fail(&t.db, "theme").await.unwrap().value, "solarized");
    assert_eq!(count_rows(&t.db, "settings").await, 1);
}

#[tokio::test]
async fn attributes_must_be_values() {
    let t = sqlite().await;
    let error = Tag::first_or_new(
        &t.db,
        [Tag::NAME.set_expr(navree_orm::Expr::raw("upper(?)", ["x"]))],
        |_| {},
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("must be a value"), "{error}");
    let _: Assignment = Tag::COLOR.set("x");
}
