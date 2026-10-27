//! Relations as query builders: conditions, retrieval, aggregates,
//! pagination, mass updates and touches, keys and names, and the
//! find-or-create family with the foreign key filled in.

use navree_orm::OrmError;
use navree_orm::prelude::*;

use crate::fixtures::{seed, titles};
use crate::models::{Activity, Company, Contact, Deal, Note, Tag, Team};
use crate::support::{count_rows, sqlite};

#[tokio::test]
async fn relations_take_every_builder_method() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    let calls = crm
        .renewal
        .activities()
        .where_(Activity::KIND.eq("call"))
        .order_by_desc(Activity::MINUTES)
        .get(&t.db)
        .await
        .unwrap();
    let minutes: Vec<i64> = calls.iter().map(|a| a.minutes).collect();
    assert_eq!(minutes, [45, 30]);

    // The relation's condition comes first and the caller's `OR` stays in
    // its group: other deals' activities never leak in.
    let either = crm
        .renewal
        .activities()
        .where_(Activity::KIND.eq("email"))
        .or_where(Activity::KIND.eq("meeting"))
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(either.len(), 1, "Upgrade's meeting is not Renewal's");
    assert_eq!(either[0].kind, "email");
    let sql = crm
        .renewal
        .activities()
        .where_(Activity::KIND.eq("email"))
        .or_where(Activity::KIND.eq("meeting"))
        .to_sql(Dialect::Sqlite)
        .unwrap()
        .sql;
    assert!(
        sql.ends_with(
            r#"WHERE "activities"."deal_id" = ? AND "activities"."deal_id" IS NOT NULL AND ("activities"."kind" = ? OR "activities"."kind" = ?)"#
        ),
        "{sql}"
    );

    let latest = crm
        .renewal
        .activities()
        .latest()
        .limit(1)
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(latest.len(), 1);
    let nested = crm
        .acme
        .deals()
        .with("activities")
        .has("activities")
        .order_by(Deal::ID)
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(titles(&nested), ["Renewal", "Upgrade"]);
    assert!(nested[0].relation_loaded("activities"));
    let big = crm
        .acme
        .deals()
        .where_has(Deal::activities, |q| q.where_(Activity::MINUTES.gte(60)))
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(titles(&big), ["Upgrade"]);
}

#[tokio::test]
async fn retrieval_on_relations() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    let activities = || crm.renewal.activities().order_by(Activity::ID);
    let all = activities().get(&t.db).await.unwrap();
    assert_eq!(all.len(), 3);
    let first = activities().first(&t.db).await.unwrap().unwrap();
    assert_eq!(first.minutes, 30);
    assert_eq!(
        activities().first_or_fail(&t.db).await.unwrap().id,
        first.id
    );
    let email = activities()
        .first_where(&t.db, Activity::KIND.eq("email"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(email.minutes, 5);
    assert!(matches!(
        activities().sole(&t.db).await.unwrap_err(),
        OrmError::MultipleRecordsFound { .. }
    ));
    let meeting = crm.upgrade.activities().sole(&t.db).await.unwrap();
    assert_eq!(meeting.kind, "meeting");

    // Keys of other parents' records are not found through the relation.
    assert_eq!(
        activities()
            .find(&t.db, first.id)
            .await
            .unwrap()
            .unwrap()
            .id,
        first.id
    );
    assert!(
        activities()
            .find(&t.db, meeting.id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        activities()
            .find_or_fail(&t.db, meeting.id)
            .await
            .unwrap_err(),
        OrmError::ModelNotFound { .. }
    ));
    let many = activities()
        .find_many(&t.db, [first.id, email.id, meeting.id])
        .await
        .unwrap();
    assert_eq!(many.len(), 2);
    assert_eq!(
        activities().find_sole(&t.db, email.id).await.unwrap().id,
        email.id
    );
    assert!(activities().exists(&t.db).await.unwrap());
    assert!(crm.platform.activities().doesnt_exist(&t.db).await.unwrap());
    assert_eq!(activities().count(&t.db).await.unwrap(), 3);
    assert_eq!(
        activities()
            .sum::<i64>(&t.db, Activity::MINUTES)
            .await
            .unwrap(),
        80
    );
    assert_eq!(
        activities()
            .max::<i64>(&t.db, Activity::MINUTES)
            .await
            .unwrap(),
        Some(45)
    );
    assert_eq!(
        activities()
            .min::<i64>(&t.db, Activity::MINUTES)
            .await
            .unwrap(),
        Some(5)
    );
    assert!(
        activities()
            .avg::<f64>(&t.db, Activity::MINUTES)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(
        activities().pluck(&t.db, Activity::KIND).await.unwrap(),
        ["call", "email", "call"]
    );
    assert_eq!(
        activities().value(&t.db, Activity::KIND).await.unwrap(),
        Some("call".to_owned())
    );

    // A belongs-to and a many-to-many: keys qualified by the related table.
    let company = crm.renewal.company().first(&t.db).await.unwrap().unwrap();
    assert_eq!(company.name, "Acme");
    assert_eq!(
        crm.renewal
            .company()
            .results(&t.db)
            .await
            .unwrap()
            .unwrap()
            .id,
        crm.acme.id
    );
    crm.renewal
        .tags()
        .attach(&t.db, [crm.vip.id, crm.q1.id])
        .await
        .unwrap();
    let vip = crm.renewal.tags().find(&t.db, crm.vip.id).await.unwrap();
    assert_eq!(vip.unwrap().name, "vip");
    assert!(
        crm.renewal
            .tags()
            .find(&t.db, crm.hot.id)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(crm.renewal.tags().count(&t.db).await.unwrap(), 2);
    let sql = crm
        .renewal
        .tags()
        .where_key(crm.vip.id)
        .to_sql(Dialect::Sqlite)
        .unwrap()
        .sql;
    assert!(sql.contains(r#""tags"."id" = ?"#), "{sql}");

    // A relation whose key is NULL relates nothing, without a query.
    t.log.clear();
    assert!(crm.orphan.company().first(&t.db).await.unwrap().is_none());
    assert!(crm.orphan.company().results(&t.db).await.unwrap().is_none());
    assert!(!crm.orphan.owner().exists(&t.db).await.unwrap());
    assert_eq!(crm.orphan.owner().count(&t.db).await.unwrap(), 0);
    assert!(t.log.selects().is_empty());
}

#[tokio::test]
async fn paginating_relations_keeps_pivot_columns() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    let page = crm
        .renewal
        .activities()
        .order_by(Activity::ID)
        .paginate(&t.db, (1, 2))
        .await
        .unwrap();
    assert_eq!(page.meta.total, 3);
    assert_eq!(page.data.len(), 2);
    let simple = crm
        .renewal
        .activities()
        .order_by(Activity::ID)
        .simple_paginate(&t.db, (2, 2))
        .await
        .unwrap();
    assert_eq!(simple.data.len(), 1);
    let cursor = crm
        .renewal
        .activities()
        .order_by(Activity::ID)
        .cursor_paginate(&t.db, CursorRequest::first().limit(2))
        .await
        .unwrap();
    assert_eq!(cursor.data.len(), 2);

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
    let page = crm
        .renewal
        .tags()
        .order_by(Tag::NAME)
        .paginate(&t.db, (1, 2))
        .await
        .unwrap();
    assert_eq!(page.meta.total, 3);
    let roles: Vec<Option<String>> = page
        .data
        .iter()
        .map(|tag| tag.pivot_value::<Option<String>>("role").unwrap().flatten())
        .collect();
    assert_eq!(roles, [None, Some("watcher".to_owned())], "hot, q1");
    // The pivot columns are not left among the extras.
    assert!(
        page.data[0].extras().is_empty(),
        "{:?}",
        page.data[0].extras()
    );
}

#[tokio::test]
async fn whether_a_record_is_the_related_one() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    // A belongs-to compares keys: no query.
    t.log.clear();
    assert!(crm.renewal.company().is(&crm.acme));
    assert!(crm.renewal.company().is_not(&crm.globex));
    assert!(!crm.orphan.company().is(&crm.acme));
    assert!(t.log.take().is_empty());
    let other_model_same_key = Company {
        id: crm.acme.id,
        ..Company::named("Copy")
    };
    assert!(crm.renewal.company().is(&Record::new(other_model_same_key)));

    // A has-one compares the foreign key; one of many asks the database.
    let latest = Activity::query()
        .where_(Activity::DEAL_ID.eq(crm.renewal.id))
        .order_by_desc(Activity::ID)
        .first(&t.db)
        .await
        .unwrap()
        .unwrap();
    let first = crm
        .renewal
        .first_activity()
        .first(&t.db)
        .await
        .unwrap()
        .unwrap();
    assert!(
        crm.renewal
            .latest_activity()
            .is(&t.db, &latest)
            .await
            .unwrap()
    );
    assert!(
        crm.renewal
            .latest_activity()
            .is_not(&t.db, &first)
            .await
            .unwrap()
    );
    assert!(
        !crm.upgrade
            .latest_activity()
            .is(&t.db, &latest)
            .await
            .unwrap()
    );
}

#[test]
fn relation_keys_and_names() {
    let deal = Deal {
        id: 7,
        company_id: Some(3),
        ..Deal::default()
    };
    let company = deal.company();
    assert_eq!(company.foreign_key_name(), "company_id");
    assert_eq!(company.qualified_foreign_key_name(), "deals.company_id");
    assert_eq!(company.owner_key_name(), "id");
    assert_eq!(company.qualified_owner_key_name(), "companies.id");
    assert_eq!(company.parent_key(), Some(Value::BigInt(Some(3))));
    assert_eq!(company.related_table(), "companies");
    assert_eq!(company.relation_name(), "company");

    let activities = deal.activities();
    assert_eq!(activities.foreign_key_name(), "deal_id");
    assert_eq!(
        activities.qualified_foreign_key_name(),
        "activities.deal_id"
    );
    assert_eq!(activities.local_key_name(), "id");
    assert_eq!(activities.parent_key(), Some(Value::BigInt(Some(7))));
    assert_eq!(activities.morph_type(), None);

    let notes = deal.notes();
    assert_eq!(notes.foreign_key_name(), "notable_id");
    assert_eq!(notes.morph_type(), Some("notable_type"));
    assert_eq!(notes.morph_name().as_deref(), Some("deal"));

    let renamed = Company::default()
        .contacts()
        .local_key("owner_id")
        .named("owner_contacts");
    assert_eq!(renamed.local_key_name(), "owner_id");
    assert_eq!(renamed.relation_name(), "owner_contacts");
    let owner = Contact::default().company().owner_key(Company::NAME);
    assert_eq!(owner.owner_key_name(), "name");

    let deals = Team::default().deals();
    assert_eq!(deals.first_key_name(), "team_id");
    assert_eq!(deals.foreign_key_name(), "owner_id");
    assert_eq!(deals.qualified_first_key_name(), "users.team_id");
    assert_eq!(deals.through_table(), "users");
}

#[tokio::test]
async fn mass_updates_and_touches_of_related_records() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    let changed = crm
        .renewal
        .activities()
        .where_(Activity::KIND.eq("call"))
        .update(&t.db, [Activity::MINUTES.set(1)])
        .await
        .unwrap();
    assert_eq!(changed, 2);
    let minutes = Activity::query()
        .order_by(Activity::ID)
        .pluck(&t.db, Activity::MINUTES)
        .await
        .unwrap();
    assert_eq!(minutes, [1, 5, 1, 60], "Upgrade's meeting is untouched");
    assert_eq!(
        crm.renewal
            .activities()
            .increment(&t.db, Activity::MINUTES, 10)
            .await
            .unwrap(),
        3
    );
    assert_eq!(crm.upgrade.activities().delete(&t.db).await.unwrap(), 1);
    assert_eq!(count_rows(&t.db, "activities").await, 3);

    // Touching sets the update timestamp of every related record.
    crm.renewal
        .tags()
        .attach(&t.db, [crm.vip.id])
        .await
        .unwrap();
    let before = Tag::find_or_fail(&t.db, crm.vip.id)
        .await
        .unwrap()
        .updated_at;
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    assert_eq!(crm.renewal.tags().touch(&t.db).await.unwrap(), 1);
    let after = Tag::find_or_fail(&t.db, crm.vip.id)
        .await
        .unwrap()
        .updated_at;
    assert!(after > before);
    let hot = Tag::find_or_fail(&t.db, crm.hot.id).await.unwrap();
    assert_eq!(hot.updated_at, crm.hot.updated_at, "not attached");
    // A relation to a model without update timestamps touches nothing.
    assert_eq!(crm.acme.contacts().touch(&t.db).await.unwrap(), 0);
}

#[tokio::test]
async fn all_related_keys() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    crm.renewal
        .tags()
        .attach(&t.db, [crm.q1.id, crm.vip.id])
        .await
        .unwrap();
    let mut ids: Vec<i64> = crm
        .renewal
        .tags()
        .all_related_ids(&t.db)
        .await
        .unwrap()
        .into_iter()
        .map(|value| i64::from_value(value).unwrap())
        .collect();
    ids.sort_unstable();
    assert_eq!(ids, [crm.vip.id, crm.q1.id]);
    let deal_ids = crm.team.deals().all_related_ids(&t.db).await.unwrap();
    assert_eq!(deal_ids.len(), 3);
}

#[tokio::test]
async fn unsaved_related_records_carry_the_foreign_key() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    let made = crm
        .renewal
        .activities()
        .make(Activity::new("call", 3))
        .unwrap();
    assert!(!made.exists());
    assert_eq!(made.deal_id, Some(crm.renewal.id));
    let many = crm
        .acme
        .contacts()
        .make_many([Contact::default(), Contact::default()])
        .unwrap();
    assert!(many.iter().all(|c| c.company_id == Some(crm.acme.id)));
    let note = crm.renewal.notes().make(Note::new("hi")).unwrap();
    assert_eq!(note.notable_type.as_deref(), Some("deal"));
    assert_eq!(note.notable_id, Some(crm.renewal.id));
    assert_eq!(count_rows(&t.db, "activities").await, 4, "nothing saved");
}

#[tokio::test]
async fn find_or_create_on_relations_fills_the_foreign_key() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    let contacts = || crm.globex.contacts();

    let found = contacts()
        .first_or_new(&t.db, [Contact::NAME.set("Dave")], |_| {})
        .await
        .unwrap();
    assert!(found.exists());
    let new = contacts()
        .first_or_new(&t.db, [Contact::NAME.set("Erin")], |c| {
            c.email = "erin@globex.test".into();
        })
        .await
        .unwrap();
    assert!(!new.exists());
    assert_eq!(new.company_id, Some(crm.globex.id));
    assert_eq!(new.email, "erin@globex.test");

    // Alice works at Acme: not found through Globex's contacts.
    let created = contacts()
        .first_or_create(&t.db, [Contact::NAME.set("Alice")], |c| {
            c.email = "alice@globex.test".into();
        })
        .await
        .unwrap();
    assert!(created.exists() && created.was_recently_created());
    assert_eq!(created.company_id, Some(crm.globex.id));

    // A unique violation returns the record that is there.
    let first = contacts()
        .create_or_first(&t.db, [Contact::EMAIL.set("dave@globex.test")], |c| {
            c.name = "Dave again".into();
        })
        .await
        .unwrap();
    assert_eq!(first.name, "Dave");

    let updated = contacts()
        .update_or_create(&t.db, [Contact::NAME.set("Dave")], |c| {
            c.email = "dave@new.test".into();
        })
        .await
        .unwrap();
    assert_eq!(updated.email, "dave@new.test");
    assert_eq!(
        Contact::find_or_fail(&t.db, updated.id)
            .await
            .unwrap()
            .email,
        "dave@new.test"
    );

    let counted = crm
        .renewal
        .activities()
        .increment_or_create(
            &t.db,
            [Activity::KIND.set("email")],
            Activity::MINUTES,
            1,
            10,
        )
        .await
        .unwrap();
    assert_eq!(counted.minutes, 15);
    let created = crm
        .platform
        .activities()
        .increment_or_create(
            &t.db,
            [Activity::KIND.set("email")],
            Activity::MINUTES,
            1,
            10,
        )
        .await
        .unwrap();
    assert_eq!(
        (created.minutes, created.deal_id),
        (1, Some(crm.platform.id))
    );

    let blank = contacts().find_or_new(&t.db, 999_i64).await.unwrap();
    assert!(!blank.exists());
    assert_eq!(blank.company_id, Some(crm.globex.id));

    // On a many-to-many relation, the pivot row is made too.
    let tag = crm
        .renewal
        .tags()
        .first_or_create(
            &t.db,
            [Tag::NAME.set("fresh")],
            |_| {},
            [col("role").set("auto")],
        )
        .await
        .unwrap();
    assert!(tag.exists());
    let attached = crm
        .renewal
        .tags()
        .find(&t.db, tag.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        attached.pivot_value::<String>("role").unwrap().as_deref(),
        Some("auto")
    );
}

#[tokio::test]
async fn restore_or_create_on_relations_of_soft_deleting_models() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    let mut upgrade = Deal::find_or_fail(&t.db, crm.upgrade.id).await.unwrap();
    upgrade.delete(&t.db).await.unwrap();

    let restored = crm
        .acme
        .deals()
        .restore_or_create(&t.db, [Deal::TITLE.set("Upgrade")], |_| {})
        .await
        .unwrap();
    assert_eq!(restored.id, crm.upgrade.id);
    assert!(!restored.trashed());
    let new = crm
        .acme
        .deals()
        .create_or_restore(&t.db, [Deal::TITLE.set("Expansion")], |d| {
            d.stage = "lead".into();
        })
        .await
        .unwrap();
    assert_eq!(new.company_id, Some(crm.acme.id));
    assert_eq!(crm.acme.deals().count(&t.db).await.unwrap(), 3);
}
