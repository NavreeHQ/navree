//! Pivot tables: columns and timestamps loaded with the related records,
//! reading pivots, typed pivot models, pivot conditions, values and order,
//! metadata, pivot queries and records, and polymorphic pivots.

use chrono::{DateTime, Utc};
use navree_db::Db;
use navree_orm::prelude::*;
use navree_orm::relations::Pivot;

use crate::fixtures::{seed, titles};
use crate::models::{Company, Deal, DealTag, DealWatcher, Tag, User};
use crate::support::{count_rows, sqlite};

fn tag_names(tags: &Collection<Tag>) -> Vec<String> {
    tags.iter().map(|tag| tag.name.clone()).collect()
}

/// The names of the tags `query` reads.
async fn names(db: &Db, query: BelongsToMany<Deal, Tag>) -> Vec<String> {
    tag_names(&query.get(db).await.unwrap())
}

#[tokio::test]
async fn pivot_columns_and_timestamps_load_with_the_related_records() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    crm.renewal
        .tags()
        .attach_with(
            &t.db,
            [
                (crm.vip.id, vec![col("role").set("owner")]),
                (crm.q1.id, vec![]),
            ],
        )
        .await
        .unwrap();

    let tags = crm.renewal.tags().order_by(Tag::ID).get(&t.db).await.unwrap();
    assert_eq!(tag_names(&tags), ["vip", "q1"]);
    let pivot = tags[0].pivot().expect("loaded under `pivot`");
    assert_eq!(pivot.value::<String>("role").unwrap().as_deref(), Some("owner"));
    assert_eq!(pivot.value::<i64>("deal_id").unwrap(), Some(crm.renewal.id));
    assert_eq!(pivot.value::<i64>("tag_id").unwrap(), Some(crm.vip.id));
    let added: Option<DateTime<Utc>> = pivot.value("created_at").unwrap();
    assert!(added.is_some(), "attaching set the pivot timestamps");
    assert!(pivot.value::<DateTime<Utc>>("updated_at").unwrap().is_some());
    assert!(pivot.has_timestamp_attributes());
    assert_eq!(
        tags[1].pivot_value::<Option<String>>("role").unwrap(),
        Some(None)
    );
    assert_eq!(tags[0].pivot_value::<String>("nope").unwrap(), None);
    // The pivot columns are not left among the extras.
    assert!(tags[0].extras().is_empty(), "{:?}", tags[0].extras());

    // Eager loaded the same way.
    let deals = Deal::query()
        .with("tags")
        .where_key(crm.renewal.id)
        .get(&t.db)
        .await
        .unwrap();
    let loaded = deals[0].loaded(Deal::tags).unwrap();
    assert_eq!(
        loaded[0].pivot_value::<String>("role").unwrap().as_deref(),
        Some("owner")
    );

    // Custom timestamp names; without timestamps none are written.
    let plain = crm.upgrade.belongs_to_many::<Tag>("deal_tag");
    plain.attach(&t.db, [crm.hot.id]).await.unwrap();
    let hot = crm.upgrade.tags().get(&t.db).await.unwrap();
    assert_eq!(
        hot[0].pivot_value::<Option<DateTime<Utc>>>("created_at").unwrap(),
        Some(None)
    );
    let only_updated = crm
        .platform
        .belongs_to_many::<Tag>("deal_tag")
        .with_timestamps_named(None::<&str>, Some("updated_at"));
    only_updated.attach(&t.db, [crm.hot.id]).await.unwrap();
    let hot = only_updated.get(&t.db).await.unwrap();
    let pivot = hot[0].pivot().unwrap();
    assert!(pivot.value::<DateTime<Utc>>("updated_at").unwrap().is_some());
    assert_eq!(pivot.get("created_at"), None, "not loaded");
    assert!(!pivot.has_timestamp_attributes());
}

#[tokio::test]
async fn pivots_under_another_name_know_both_records() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    crm.renewal
        .tags()
        .attach_with(&t.db, [(crm.vip.id, [col("role").set("owner")])])
        .await
        .unwrap();
    let tags = crm.renewal.assigned_tags().get(&t.db).await.unwrap();
    assert!(tags[0].pivot().is_none());
    let assignment = tags[0].relation::<Pivot>("assignment").unwrap();
    assert_eq!(
        assignment.value::<String>("role").unwrap().as_deref(),
        Some("owner")
    );
    // `chaperone_pivot`: snapshots of both records, without relations.
    assert_eq!(assignment.parent::<Deal>().unwrap().title, "Renewal");
    assert_eq!(assignment.related::<Tag>().unwrap().name, "vip");
    assert!(assignment.parent::<Company>().is_none(), "another model");
    let plain = crm.renewal.tags().get(&t.db).await.unwrap();
    assert!(plain[0].pivot().unwrap().parent::<Deal>().is_none());

    // They serialize under their name.
    let json = serde_json::to_value(&tags[0]).unwrap();
    assert_eq!(json["assignment"]["role"], "owner");
    assert_eq!(json["name"], "vip");
    let json = serde_json::to_value(&plain[0]).unwrap();
    assert_eq!(json["pivot"]["role"], "owner");
    assert_eq!(json["pivot"]["deal_id"], crm.renewal.id);
}

#[tokio::test]
async fn typed_pivot_models_with_an_id() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    crm.renewal
        .watchers()
        .attach_with(&t.db, [(crm.ada.id, [col("level").set("mentions")])])
        .await
        .unwrap();
    crm.renewal.watchers().attach(&t.db, [&crm.bob]).await.unwrap();
    assert_eq!(count_rows(&t.db, "deal_watchers").await, 2);

    let watchers = crm
        .renewal
        .watchers()
        .order_by(User::ID)
        .get(&t.db)
        .await
        .unwrap();
    let ada = watchers[0].pivot_as::<DealWatcher>().expect("a typed pivot");
    assert_eq!(ada.level, "mentions");
    assert!(ada.id > 0, "its own id");
    assert_eq!(ada.deal_id, crm.renewal.id);
    assert!(ada.exists());
    assert_eq!(
        watchers[1].pivot_value::<String>("level").unwrap().as_deref(),
        Some(""),
        "the pivot model's own default was written"
    );
    assert!(watchers[0].pivot().is_none(), "not a plain pivot");

    // From the other side, through the same model.
    let watched = crm.ada.watched().get(&t.db).await.unwrap();
    assert_eq!(titles(&watched), ["Renewal"]);
    assert_eq!(
        watched[0].pivot_as::<DealWatcher>().unwrap().level,
        "mentions"
    );

    // Updating and detaching go through the pivot model's records.
    assert_eq!(
        crm.renewal
            .watchers()
            .update_existing_pivot(&t.db, crm.ada.id, [col("level").set("none")])
            .await
            .unwrap(),
        1
    );
    let row = DealWatcher::query()
        .where_(DealWatcher::USER_ID.eq(crm.ada.id))
        .first(&t.db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.level, "none");
    assert_eq!(
        crm.renewal.watchers().detach(&t.db, [crm.bob.id]).await.unwrap(),
        1
    );
    assert_eq!(count_rows(&t.db, "deal_watchers").await, 1);
}

#[tokio::test]
async fn typed_pivot_models_without_an_id() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    crm.renewal
        .typed_tags()
        .attach(&t.db, [crm.vip.id, crm.q1.id])
        .await
        .unwrap();
    crm.upgrade.typed_tags().attach(&t.db, [crm.vip.id]).await.unwrap();
    let tags = crm.renewal.typed_tags().order_by(Tag::ID).get(&t.db).await.unwrap();
    let mut row = tags[0].pivot_as::<DealTag>().unwrap().clone();
    assert_eq!((row.deal_id, row.tag_id), (crm.renewal.id, crm.vip.id));

    // Saved and deleted by both pivot columns: Upgrade's `vip` row is left
    // alone.
    row.role = Some("lead".into());
    row.save(&t.db).await.unwrap();
    let roles: Vec<Option<String>> = DealTag::query()
        .order_by(DealTag::DEAL_ID)
        .pluck(&t.db, DealTag::ROLE)
        .await
        .unwrap();
    assert_eq!(roles, [Some("lead".to_owned()), None, None]);
    assert!(row.delete(&t.db).await.unwrap());
    assert_eq!(count_rows(&t.db, "deal_tag").await, 2);
    assert_eq!(crm.upgrade.typed_tags().count(&t.db).await.unwrap(), 1);
}

#[tokio::test]
async fn conditions_values_and_order_on_pivot_columns() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    crm.renewal
        .tags()
        .attach_with(
            &t.db,
            [
                (crm.vip.id, vec![col("role").set("owner")]),
                (crm.q1.id, vec![col("role").set("watcher")]),
                (crm.hot.id, vec![]),
            ],
        )
        .await
        .unwrap();
    let db = &t.db;
    let tags = || crm.renewal.tags().order_by(Tag::ID);
    assert_eq!(names(db, tags().where_pivot("role", "=", "owner")).await, ["vip"]);
    assert_eq!(
        names(
            db,
            tags()
                .where_pivot("role", "=", "owner")
                .or_where_pivot("role", "=", "watcher")
        )
        .await,
        ["vip", "q1"]
    );
    assert_eq!(
        names(db, tags().where_pivot_in("role", ["owner", "watcher"])).await,
        ["vip", "q1"]
    );
    assert_eq!(
        names(db, tags().where_pivot_not_in("role", ["owner"])).await,
        ["q1"],
        "NULL is neither in nor out"
    );
    assert_eq!(names(db, tags().where_pivot_null("role")).await, ["hot"]);
    assert_eq!(
        names(db, tags().where_pivot_not_null("role")).await,
        ["vip", "q1"]
    );
    assert_eq!(
        names(
            db,
            tags()
                .where_pivot_null("role")
                .or_where_pivot_in("role", ["owner"])
        )
        .await,
        ["vip", "hot"]
    );
    assert_eq!(
        names(db, tags().where_pivot_between("tag_id", crm.vip.id, crm.q1.id)).await,
        ["vip", "q1"]
    );
    assert_eq!(
        names(
            db,
            tags().where_pivot_not_between("tag_id", crm.vip.id, crm.q1.id)
        )
        .await,
        ["hot"]
    );
    assert_eq!(
        names(db, crm.renewal.tags().order_by_pivot_desc("role")).await,
        ["q1", "vip", "hot"]
    );
    assert_eq!(
        names(db, crm.renewal.tags().order_by_pivot("tag_id")).await,
        ["vip", "q1", "hot"]
    );

    // A pivot condition that also fills new rows.
    crm.platform.owner_tags().attach(db, [crm.hot.id]).await.unwrap();
    let owners = crm.platform.owner_tags().get(db).await.unwrap();
    assert_eq!(tag_names(&owners), ["hot"]);
    assert_eq!(
        owners[0].pivot_value::<String>("role").unwrap().as_deref(),
        Some("owner")
    );
    assert_eq!(names(db, crm.renewal.owner_tags()).await, ["vip"]);
    // Detaching through it only touches its rows.
    assert_eq!(crm.renewal.owner_tags().detach_all(&t.db).await.unwrap(), 1);
    assert_eq!(crm.renewal.tags().count(&t.db).await.unwrap(), 2);
}

#[test]
fn pivot_metadata() {
    let deal = Deal {
        id: 4,
        ..Deal::default()
    };
    let tags = deal.tags();
    assert_eq!(tags.pivot_table(), "deal_tag");
    assert_eq!(tags.foreign_pivot_key_name(), "deal_id");
    assert_eq!(tags.qualified_foreign_pivot_key_name(), "deal_tag.deal_id");
    assert_eq!(tags.related_pivot_key_name(), "tag_id");
    assert_eq!(tags.qualified_related_pivot_key_name(), "deal_tag.tag_id");
    assert_eq!(tags.parent_key_name(), "id");
    assert_eq!(tags.related_key_name(), "id");
    assert_eq!(tags.pivot_columns(), ["role", "created_at", "updated_at"]);
    assert!(tags.has_pivot_column("role"));
    assert!(!tags.has_pivot_column("color"));
    assert_eq!(tags.qualify_pivot_column("role"), "deal_tag.role");
    assert_eq!(tags.qualify_pivot_column("other.role"), "other.role");
    assert_eq!(tags.pivot_accessor(), "pivot");
    assert_eq!(deal.assigned_tags().pivot_accessor(), "assignment");
    assert_eq!(tags.morph_type(), None);
    let labels = deal.labels();
    assert_eq!(labels.morph_type(), Some("taggable_type"));
    assert_eq!(labels.morph_name(), Some("deal"));
    assert_eq!(labels.foreign_pivot_key_name(), "taggable_id");
    let inverse = Tag::default().labeled_deals();
    assert_eq!(inverse.related_pivot_key_name(), "taggable_id");
    assert_eq!(inverse.morph_name(), Some("deal"));
    assert_eq!(navree_orm::relations::joining_table::<Tag, Deal>(), "deal_tag");
    let renamed = deal
        .belongs_to_many::<Tag>("labels_of_deals")
        .foreign_pivot_key("opportunity_id")
        .related_pivot_key("label_id")
        .parent_key(Deal::TITLE)
        .related_key(Tag::NAME);
    assert_eq!(renamed.foreign_pivot_key_name(), "opportunity_id");
    assert_eq!(renamed.related_pivot_key_name(), "label_id");
    assert_eq!(renamed.parent_key_name(), "title");
    assert_eq!(renamed.related_key_name(), "name");
}

#[tokio::test]
async fn querying_the_pivot_table_and_pivot_records() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    crm.renewal
        .tags()
        .attach(&t.db, [crm.vip.id, crm.q1.id])
        .await
        .unwrap();
    crm.upgrade.tags().attach(&t.db, [crm.vip.id]).await.unwrap();

    let rows = crm.renewal.tags().pivot_query().unwrap();
    assert_eq!(rows.clone().count(&t.db).await.unwrap(), 2);
    let sql = rows.to_sql(Dialect::Sqlite).unwrap().sql;
    assert!(sql.contains(r#"FROM "deal_tag" WHERE "deal_tag"."deal_id" = ?"#), "{sql}");
    let vip = crm
        .renewal
        .tags()
        .pivot_query_for([crm.vip.id])
        .unwrap();
    assert_eq!(vip.count(&t.db).await.unwrap(), 1);

    // A new pivot row, saved; then one marked stored, deleted by both keys.
    let mut row = crm
        .platform
        .tags()
        .new_pivot([col("tag_id").set(crm.hot.id), col("role").set("new")])
        .unwrap();
    assert!(!row.exists());
    assert_eq!(row.foreign_key(), "deal_id");
    assert_eq!(row.related_key(), "tag_id");
    assert_eq!(row.table(), "deal_tag");
    assert!(row.save(&t.db).await.unwrap());
    assert!(row.exists());
    assert!(row.has_timestamp_attributes(), "the relation's timestamps were set");
    assert_eq!(crm.platform.tags().count(&t.db).await.unwrap(), 1);
    row.set("role", "changed");
    assert!(row.is_dirty());
    assert!(row.save(&t.db).await.unwrap());
    assert!(!row.save(&t.db).await.unwrap(), "nothing changed");
    let stored = crm.platform.tags().get(&t.db).await.unwrap();
    assert_eq!(
        stored[0].pivot_value::<String>("role").unwrap().as_deref(),
        Some("changed")
    );

    let mut existing = crm
        .renewal
        .tags()
        .new_existing_pivot([col("tag_id").set(crm.vip.id)])
        .unwrap();
    assert!(existing.exists());
    assert_eq!(existing.delete(&t.db).await.unwrap(), 1);
    assert_eq!(crm.renewal.tags().count(&t.db).await.unwrap(), 1);
    assert_eq!(crm.upgrade.tags().count(&t.db).await.unwrap(), 1, "kept");

    // A loaded pivot deletes its own row.
    let left = crm.renewal.tags().get(&t.db).await.unwrap();
    let mut pivot: Pivot = left[0].pivot().unwrap().clone();
    assert_eq!(pivot.delete(&t.db).await.unwrap(), 1);
    assert!(!pivot.exists());
    assert!(crm.renewal.tags().get(&t.db).await.unwrap().is_empty());
}

#[tokio::test]
async fn polymorphic_pivots_carry_the_morph_type() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    crm.renewal
        .labels()
        .attach(&t.db, [crm.vip.id, crm.hot.id])
        .await
        .unwrap();
    // The same key under another type: kept apart by the type column.
    assert_eq!(crm.acme.id, crm.renewal.id);
    crm.acme
        .labels()
        .attach(&t.db, [crm.vip.id, crm.q1.id])
        .await
        .unwrap();

    let labels = crm.renewal.labels().order_by(Tag::ID).get(&t.db).await.unwrap();
    assert_eq!(tag_names(&labels), ["vip", "hot"]);
    let pivot = labels[0].pivot().unwrap();
    assert_eq!(pivot.morph_type(), Some("taggable_type"));
    assert_eq!(pivot.morph_name(), Some("deal"));
    assert_eq!(
        pivot.value::<String>("taggable_type").unwrap().as_deref(),
        Some("deal")
    );

    let deals = crm.vip.labeled_deals().get(&t.db).await.unwrap();
    assert_eq!(titles(&deals), ["Renewal"]);
    let companies = crm.vip.labeled_companies().get(&t.db).await.unwrap();
    assert_eq!(companies.len(), 1);
    assert_eq!(companies[0].name, "Acme");
    let tagged = Tag::query()
        .has("labeled_companies")
        .order_by(Tag::ID)
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(tag_names(&tagged), ["vip", "q1"]);

    // Detaching only removes rows of the parent's type.
    assert_eq!(
        crm.acme.labels().detach(&t.db, [crm.vip.id]).await.unwrap(),
        1
    );
    assert_eq!(crm.renewal.labels().count(&t.db).await.unwrap(), 2);
    let deals = Deal::query()
        .with("labels")
        .where_key(crm.renewal.id)
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(deals[0].loaded(Deal::labels).unwrap().len(), 2);
}
