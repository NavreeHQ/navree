//! Writing through relations: saving and creating related records,
//! associating and dissociating, attaching, detaching, syncing and
//! toggling, pivot updates, and atomic pivot writes.

use navree_orm::prelude::*;
use navree_orm::OrmError;

use crate::fixtures::{seed, titles};
use crate::models::{Activity, Company, Contact, Deal, Image, Note, Tag};
use crate::support::{count_rows, sqlite};

fn ids(values: &[i64]) -> Vec<i64> {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    sorted
}

#[tokio::test]
async fn saving_related_records_sets_the_foreign_key() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    let mut call = Record::new(Activity::new("call", 12));
    assert!(crm.platform.activities().save(&t.db, &mut call).await.unwrap());
    assert!(call.exists());
    assert_eq!(call.deal_id, Some(crm.platform.id));

    // An existing record moves to the parent.
    let mut meeting = Activity::query()
        .where_(Activity::KIND.eq("meeting"))
        .first(&t.db)
        .await
        .unwrap()
        .unwrap();
    crm.platform
        .activities()
        .save_quietly(&t.db, &mut meeting)
        .await
        .unwrap();
    assert_eq!(
        Activity::find_or_fail(&t.db, meeting.id).await.unwrap().deal_id,
        Some(crm.platform.id)
    );

    let mut many = vec![
        Record::new(Activity::new("email", 1)),
        Record::new(Activity::new("email", 2)),
    ];
    assert!(crm.orphan.activities().save_many(&t.db, &mut many).await.unwrap());
    crm.orphan
        .activities()
        .save_many_quietly(&t.db, &mut [Record::new(Activity::new("sms", 1))])
        .await
        .unwrap();
    assert_eq!(crm.orphan.activities().count(&t.db).await.unwrap(), 3);

    // A has-one and a morph-one set the type too.
    let mut image = Record::new(Image {
        url: "deal.png".into(),
        ..Image::default()
    });
    crm.renewal.image().save(&t.db, &mut image).await.unwrap();
    assert_eq!(image.imageable_type.as_deref(), Some("deal"));
    assert_eq!(image.imageable_id, Some(crm.renewal.id));
    let mut note = Record::new(Note::new("call back"));
    crm.acme.notes().save(&t.db, &mut note).await.unwrap();
    assert_eq!(note.notable_type.as_deref(), Some("company"));
    assert_eq!(
        crm.acme.notes().first(&t.db).await.unwrap().unwrap().body,
        "call back"
    );
    assert!(crm.renewal.notes().first(&t.db).await.unwrap().is_none(), "same id, other type");
}

#[tokio::test]
async fn creating_related_records() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    let created = crm
        .globex
        .contacts()
        .create(
            &t.db,
            Contact {
                name: "Erin".into(),
                email: "erin@globex.test".into(),
                ..Contact::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(created.company_id, Some(crm.globex.id));
    crm.globex
        .contacts()
        .create_quietly(
            &t.db,
            Contact {
                name: "Fay".into(),
                email: "fay@globex.test".into(),
                ..Contact::default()
            },
        )
        .await
        .unwrap();
    let many = crm
        .platform
        .activities()
        .create_many(&t.db, [Activity::new("call", 1), Activity::new("call", 2)])
        .await
        .unwrap();
    assert!(many.iter().all(|a| a.deal_id == Some(crm.platform.id) && a.exists()));
    crm.platform
        .activities()
        .create_many_quietly(&t.db, [Activity::new("email", 1)])
        .await
        .unwrap();
    assert_eq!(crm.platform.activities().count(&t.db).await.unwrap(), 3);
    assert_eq!(crm.globex.contacts().count(&t.db).await.unwrap(), 3);

    // From untyped attributes.
    let forced = crm
        .orphan
        .activities()
        .force_create(&t.db, [Activity::KIND.set("visit"), Activity::MINUTES.set(90)])
        .await
        .unwrap();
    assert_eq!((forced.kind.as_str(), forced.deal_id), ("visit", Some(crm.orphan.id)));
    crm.orphan
        .activities()
        .force_create_quietly(&t.db, [col("kind").set("visit")])
        .await
        .unwrap();
    let rows = crm
        .orphan
        .activities()
        .force_create_many(
            &t.db,
            [
                vec![Activity::KIND.set("a")],
                vec![Activity::KIND.set("b")],
            ],
        )
        .await
        .unwrap();
    assert_eq!(rows.len(), 2);
    crm.orphan
        .activities()
        .force_create_many_quietly(&t.db, [vec![Activity::KIND.set("c")]])
        .await
        .unwrap();
    assert_eq!(crm.orphan.activities().count(&t.db).await.unwrap(), 5);
    let error = crm
        .orphan
        .activities()
        .force_create(&t.db, [col("nope").set(1)])
        .await
        .unwrap_err();
    assert!(error.to_string().contains("nope"), "{error}");
}

#[tokio::test]
async fn associating_and_dissociating() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    let mut deal = Deal::find_or_fail(&t.db, crm.orphan.id).await.unwrap();
    deal.associate(Deal::company, &crm.globex).unwrap();
    assert_eq!(deal.company_id, Some(crm.globex.id));
    // The loaded relation is set: no query to read it.
    assert_eq!(
        deal.loaded(Deal::company).unwrap().as_ref().unwrap().name,
        "Globex"
    );
    // Saving is separate.
    assert_eq!(
        Deal::find_or_fail(&t.db, deal.id).await.unwrap().company_id,
        None
    );
    deal.save(&t.db).await.unwrap();
    assert_eq!(
        Deal::find_or_fail(&t.db, deal.id).await.unwrap().company_id,
        Some(crm.globex.id)
    );

    deal.dissociate(Deal::company).unwrap().save(&t.db).await.unwrap();
    assert_eq!(deal.company_id, None);
    assert!(deal.loaded(Deal::company).unwrap().is_none());
    assert_eq!(
        Deal::find_or_fail(&t.db, deal.id).await.unwrap().company_id,
        None
    );

    // A morph-to sets the type and the key.
    let mut note = Record::new(Note::new("hello"));
    note.associate(Note::notable, &crm.renewal).unwrap();
    assert_eq!(note.notable_type.as_deref(), Some("deal"));
    assert_eq!(note.notable_id, Some(crm.renewal.id));
    note.save(&t.db).await.unwrap();
    note.dissociate(Note::notable).unwrap();
    assert_eq!((note.notable_type.as_deref(), note.notable_id), (None, None));
    assert!(note.loaded(Note::notable).unwrap().is_none());
}

#[tokio::test]
async fn attaching_and_detaching() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    // By key, by record, with pivot values.
    crm.renewal.tags().attach(&t.db, [crm.vip.id]).await.unwrap();
    crm.renewal.tags().attach(&t.db, [&crm.q1]).await.unwrap();
    crm.renewal
        .tags()
        .attach_with(&t.db, [(&crm.hot, [col("role").set("owner")])])
        .await
        .unwrap();
    assert_eq!(count_rows(&t.db, "deal_tag").await, 3);
    let attached: Vec<i64> = crm
        .renewal
        .tags()
        .pluck(&t.db, Tag::ID)
        .await
        .unwrap();
    assert_eq!(ids(&attached), ids(&[crm.vip.id, crm.q1.id, crm.hot.id]));

    // Attaching twice is the database's error (the pivot's primary key).
    let error = crm.renewal.tags().attach(&t.db, [crm.vip.id]).await.unwrap_err();
    assert!(error.is_unique_violation(), "{error:?}");

    assert_eq!(
        crm.renewal.tags().detach(&t.db, [crm.vip.id, crm.vip.id]).await.unwrap(),
        1
    );
    assert_eq!(crm.renewal.tags().detach(&t.db, Vec::<i64>::new()).await.unwrap(), 0);
    assert_eq!(crm.renewal.tags().detach_all(&t.db).await.unwrap(), 2);
    assert_eq!(count_rows(&t.db, "deal_tag").await, 0);
}

#[tokio::test]
async fn syncing_reports_what_changed() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    crm.renewal
        .tags()
        .attach(&t.db, [crm.vip.id, crm.q1.id])
        .await
        .unwrap();

    let changes = crm
        .renewal
        .tags()
        .sync(&t.db, [crm.q1.id, crm.hot.id])
        .await
        .unwrap();
    assert_eq!(changes.attached_as::<i64>().unwrap(), [crm.hot.id]);
    assert_eq!(changes.detached_as::<i64>().unwrap(), [crm.vip.id]);
    assert!(changes.updated_as::<i64>().unwrap().is_empty());
    assert!(!changes.is_empty());
    let again = crm
        .renewal
        .tags()
        .sync(&t.db, [crm.q1.id, crm.hot.id])
        .await
        .unwrap();
    assert!(again.is_empty(), "nothing to do");

    // With pivot values: changed values are updated.
    let changes = crm
        .renewal
        .tags()
        .sync_with(
            &t.db,
            [
                (crm.q1.id, vec![col("role").set("owner")]),
                (crm.hot.id, vec![]),
            ],
        )
        .await
        .unwrap();
    assert_eq!(changes.updated_as::<i64>().unwrap(), [crm.q1.id]);
    assert!(changes.attached.is_empty() && changes.detached.is_empty());

    let changes = crm
        .renewal
        .tags()
        .sync_without_detaching(&t.db, [crm.vip.id])
        .await
        .unwrap();
    assert_eq!(changes.attached_as::<i64>().unwrap(), [crm.vip.id]);
    assert!(changes.detached.is_empty());
    assert_eq!(crm.renewal.tags().count(&t.db).await.unwrap(), 3);

    let changes = crm
        .renewal
        .tags()
        .sync_with_pivot_values(&t.db, [crm.vip.id, crm.hot.id], [col("role").set("x")])
        .await
        .unwrap();
    assert_eq!(changes.detached_as::<i64>().unwrap(), [crm.q1.id]);
    assert_eq!(ids(&changes.updated_as::<i64>().unwrap()), ids(&[crm.vip.id, crm.hot.id]));
    let roles: Vec<Option<String>> = crm
        .renewal
        .tags()
        .get(&t.db)
        .await
        .unwrap()
        .iter()
        .map(|tag| tag.pivot_value::<String>("role").unwrap())
        .collect();
    assert_eq!(roles, [Some("x".to_owned()), Some("x".to_owned())]);

    // Syncing to nothing detaches everything.
    let changes = crm.renewal.tags().sync(&t.db, Vec::<i64>::new()).await.unwrap();
    assert_eq!(changes.detached.len(), 2);
}

#[tokio::test]
async fn toggling_and_updating_pivot_rows() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    crm.renewal.tags().attach(&t.db, [crm.vip.id]).await.unwrap();
    let changes = crm
        .renewal
        .tags()
        .toggle(&t.db, [crm.vip.id, crm.q1.id])
        .await
        .unwrap();
    assert_eq!(changes.attached_as::<i64>().unwrap(), [crm.q1.id]);
    assert_eq!(changes.detached_as::<i64>().unwrap(), [crm.vip.id]);
    let attached: Vec<i64> = crm.renewal.tags().pluck(&t.db, Tag::ID).await.unwrap();
    assert_eq!(attached, [crm.q1.id]);

    let before = crm.renewal.tags().get(&t.db).await.unwrap()[0]
        .pivot()
        .unwrap()
        .clone();
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    assert_eq!(
        crm.renewal
            .tags()
            .update_existing_pivot(&t.db, crm.q1.id, [col("role").set("lead")])
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        crm.renewal
            .tags()
            .update_existing_pivot(&t.db, crm.hot.id, [col("role").set("lead")])
            .await
            .unwrap(),
        0,
        "not attached"
    );
    let after = crm.renewal.tags().get(&t.db).await.unwrap()[0]
        .pivot()
        .unwrap()
        .clone();
    assert_eq!(after.value::<String>("role").unwrap().as_deref(), Some("lead"));
    let stamp = |pivot: &navree_orm::relations::Pivot| {
        pivot
            .value::<chrono::DateTime<chrono::Utc>>("updated_at")
            .unwrap()
            .unwrap()
    };
    assert!(stamp(&after) > stamp(&before), "the pivot's update timestamp");
}

#[tokio::test]
async fn pivot_writes_are_atomic() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    // A bare connection: each call runs in a transaction of its own, so a
    // failing attach leaves nothing behind.
    crm.renewal.tags().attach(&t.db, [crm.q1.id]).await.unwrap();
    let error = crm
        .renewal
        .tags()
        .attach(&t.db, [crm.vip.id, crm.q1.id])
        .await
        .unwrap_err();
    assert!(error.is_unique_violation(), "{error:?}");
    assert_eq!(count_rows(&t.db, "deal_tag").await, 1, "vip was rolled back");

    // In a unit of work, the writes are part of its transaction.
    let uow = t.db.uow(None, None).await;
    crm.renewal.tags().sync(&uow, [crm.vip.id, crm.hot.id]).await.unwrap();
    crm.upgrade.tags().attach(&uow, [crm.vip.id]).await.unwrap();
    uow.rollback().await.unwrap();
    assert_eq!(count_rows(&t.db, "deal_tag").await, 1);
    let uow = t.db.uow(None, None).await;
    crm.renewal.tags().sync(&uow, [crm.vip.id, crm.hot.id]).await.unwrap();
    uow.commit().await.unwrap();
    assert_eq!(count_rows(&t.db, "deal_tag").await, 2);
}

#[tokio::test]
async fn saving_and_creating_many_to_many_records() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    let mut cold = Record::new(Tag::named("cold"));
    assert!(
        crm.renewal
            .tags()
            .save(&t.db, &mut cold, [col("role").set("owner")])
            .await
            .unwrap()
    );
    assert!(cold.exists());
    let warm = crm
        .renewal
        .tags()
        .create(&t.db, Tag::named("warm"), [])
        .await
        .unwrap();
    crm.renewal
        .tags()
        .create_quietly(&t.db, Tag::named("quiet"), [])
        .await
        .unwrap();
    let mut pair = vec![Record::new(Tag::named("a")), Record::new(Tag::named("b"))];
    crm.renewal.tags().save_many(&t.db, &mut pair).await.unwrap();
    crm.renewal
        .tags()
        .create_many(&t.db, [Tag::named("c")])
        .await
        .unwrap();
    crm.renewal
        .tags()
        .force_create(&t.db, [Tag::NAME.set("forced")], [col("role").set("forced")])
        .await
        .unwrap();
    assert_eq!(crm.renewal.tags().count(&t.db).await.unwrap(), 7);
    let owner = crm
        .renewal
        .tags()
        .where_pivot("role", "=", "owner")
        .first(&t.db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(owner.id, cold.id);
    assert!(crm.renewal.tags().find(&t.db, warm.id).await.unwrap().is_some());

    // A failing save attaches nothing.
    let mut duplicate = Record::new(Tag::named("warm"));
    let error = crm
        .renewal
        .tags()
        .save(&t.db, &mut duplicate, [])
        .await
        .unwrap_err();
    assert!(matches!(error, OrmError::Db(_)), "{error:?}");
    assert_eq!(crm.renewal.tags().count(&t.db).await.unwrap(), 7);
}

#[tokio::test]
async fn relations_mass_assign_through_the_relation_query() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    let updated = crm
        .acme
        .deals()
        .update(&t.db, [Deal::STAGE.set("qualified")])
        .await
        .unwrap();
    assert_eq!(updated, 2);
    let stages = Deal::query()
        .order_by(Deal::ID)
        .pluck(&t.db, Deal::STAGE)
        .await
        .unwrap();
    assert_eq!(stages, ["qualified", "qualified", "lead", "lead"]);
    let qualified = Company::query()
        .where_has(Company::deals, |q| q.where_(Deal::STAGE.eq("qualified")))
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(qualified.len(), 1);
    let deals = Deal::query()
        .where_(Deal::STAGE.eq("lead"))
        .order_by(Deal::ID)
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(titles(&deals), ["Platform", "Orphan"]);
}
