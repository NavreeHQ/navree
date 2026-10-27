//! Eager loading: one batched query per relation and level, nested paths,
//! constraints and per-parent limits, column selections, removing loads,
//! loading afterwards, and the relations records carry.

use navree_orm::OrmError;
use navree_orm::prelude::*;
use navree_orm::relations::Relations;

use crate::fixtures::{seed, titles};
use crate::models::{Activity, Company, Contact, Deal, Tag, User};
use crate::support::sqlite;

fn company_of(deal: &Record<Deal>) -> Option<&Record<Company>> {
    deal.loaded(Deal::company)
        .expect("company is loaded")
        .as_ref()
}

fn activities_of(deal: &Record<Deal>) -> &Collection<Activity> {
    deal.loaded(Deal::activities)
        .expect("activities are loaded")
}

#[tokio::test]
async fn eager_loads_batch_one_query_per_relation_and_level() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    t.log.clear();

    let deals = Deal::query()
        .with("company.owner")
        .with(Deal::activities)
        .order_by(Deal::ID)
        .get(&t.db)
        .await
        .unwrap();

    // Deals, their companies, the companies' owners, the activities.
    let selects = t.log.selects();
    assert_eq!(selects.len(), 4, "{selects:#?}");
    assert!(selects[1].contains("FROM \"companies\""), "{}", selects[1]);
    // Integer keys are written into the statement, each once.
    assert!(
        selects[1].contains(&format!("IN ({}, {})", crm.acme.id, crm.globex.id)),
        "{}",
        selects[1]
    );
    assert!(selects[2].contains("FROM \"users\""), "{}", selects[2]);
    assert!(selects[3].contains("FROM \"activities\""), "{}", selects[3]);

    assert_eq!(titles(&deals), ["Renewal", "Upgrade", "Platform", "Orphan"]);
    assert_eq!(company_of(&deals[0]).unwrap().name, "Acme");
    assert_eq!(company_of(&deals[2]).unwrap().name, "Globex");
    // Nothing related: loaded as `None`.
    assert!(deals[3].relation_loaded("company"));
    assert!(company_of(&deals[3]).is_none());
    let owner = company_of(&deals[1])
        .unwrap()
        .loaded(Company::owner)
        .unwrap()
        .as_ref()
        .unwrap();
    assert_eq!(owner.name, "Ada");

    let counts: Vec<usize> = deals.iter().map(|deal| activities_of(deal).len()).collect();
    assert_eq!(counts, [3, 1, 0, 0]);
    let kinds: Vec<&str> = activities_of(&deals[0])
        .iter()
        .map(|activity| activity.kind.as_str())
        .collect();
    assert_eq!(kinds, ["call", "email", "call"]);
}

#[tokio::test]
async fn no_query_runs_when_every_key_is_empty() {
    let t = sqlite().await;
    seed(&t.db).await;
    t.log.clear();
    let orphans = Deal::query()
        .where_(Deal::COMPANY_ID.is_null())
        .with("company")
        .with_all(["owner"])
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(t.log.selects().len(), 1);
    assert_eq!(titles(&orphans), ["Orphan"]);
    assert!(orphans[0].relation_loaded("company"));
    assert!(company_of(&orphans[0]).is_none());

    // No records at all: no further query either.
    let none = Deal::query()
        .where_(Deal::TITLE.eq("missing"))
        .with("company.owner")
        .get(&t.db)
        .await
        .unwrap();
    assert!(none.is_empty());
    assert_eq!(t.log.selects().len(), 1);
}

#[tokio::test]
async fn constrained_eager_loads_limit_per_parent() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    // A second meeting on Upgrade, so two deals have several activities.
    crm.upgrade
        .activities()
        .create(&t.db, Activity::new("call", 10))
        .await
        .unwrap();

    // The latest two activities of every deal: the limit applies to each.
    let deals = Deal::query()
        .with_constrained(Deal::activities, |q| q.order_by_desc(Activity::ID).limit(2))
        .order_by(Deal::ID)
        .get(&t.db)
        .await
        .unwrap();
    let minutes = |deal: &Record<Deal>| -> Vec<i64> {
        activities_of(deal).iter().map(|a| a.minutes).collect()
    };
    assert_eq!(minutes(&deals[0]), [45, 5]);
    assert_eq!(minutes(&deals[1]), [10, 60]);
    assert!(minutes(&deals[2]).is_empty());
    // No helper column is left on the records.
    assert!(
        activities_of(&deals[0])[0]
            .extras()
            .iter()
            .all(|(name, _)| !name.starts_with("navree")),
        "{:?}",
        activities_of(&deals[0])[0].extras()
    );

    // A closure on a builder of the related model, with a string path.
    let deals = Deal::query()
        .with_constrained("activities", |q: Builder<Activity>| {
            q.where_(Activity::KIND.eq("call"))
        })
        .with_constrained("company", |q: Builder<Company>| {
            q.where_(Company::NAME.ne("Acme"))
        })
        .order_by(Deal::ID)
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(activities_of(&deals[0]).len(), 2);
    assert!(company_of(&deals[0]).is_none(), "Acme is filtered out");
    assert_eq!(company_of(&deals[2]).unwrap().name, "Globex");

    // A constraint of the wrong type is an error naming both types.
    let error = Deal::query()
        .with_constrained("activities", |q: Builder<Tag>| q.limit(1))
        .get(&t.db)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("Tag"), "{error}");
}

#[tokio::test]
async fn eager_loads_select_columns_and_need_the_matching_keys() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    t.log.clear();
    let acme = Company::query()
        .with("contacts:id,company_id,name")
        .where_key(crm.acme.id)
        .first(&t.db)
        .await
        .unwrap()
        .unwrap();
    let selects = t.log.selects();
    assert!(
        selects[1].starts_with(
            r#"SELECT "contacts"."id", "contacts"."company_id", "contacts"."name" FROM"#
        ),
        "{}",
        selects[1]
    );
    let contacts = acme.loaded(Company::contacts).unwrap();
    assert_eq!(contacts.map(|c| c.name.clone()), ["Alice", "Carol"]);
    assert!(
        contacts.iter().all(|c| c.email.is_empty()),
        "not selected: the field's default"
    );

    // The keys matching needs must be selected; the error names them.
    let error = Company::query()
        .with("contacts:id,name")
        .get(&t.db)
        .await
        .unwrap_err();
    assert!(
        matches!(error, OrmError::InvalidQuery(_)) && error.to_string().contains("`company_id`"),
        "{error}"
    );
    let error = Deal::query()
        .with("company:name")
        .get(&t.db)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("`id`"), "{error}");
}

#[tokio::test]
async fn eager_loads_are_removed_and_replaced() {
    let query = Deal::query()
        .with("company.owner")
        .with("activities")
        .with("tags");
    assert_eq!(
        query.eager_loads(),
        ["company", "company.owner", "activities", "tags"]
    );
    assert_eq!(
        query.clone().without("company.owner").eager_loads(),
        ["company", "activities", "tags"]
    );
    assert_eq!(
        query.clone().without(Deal::company).eager_loads(),
        ["activities", "tags"]
    );
    assert_eq!(
        query
            .clone()
            .without_eager_load(["activities", "tags"])
            .eager_loads(),
        ["company", "company.owner"]
    );
    assert_eq!(query.clone().with_only(["owner"]).eager_loads(), ["owner"]);
    assert!(query.without_eager_loads().eager_loads().is_empty());

    let t = sqlite().await;
    seed(&t.db).await;
    let deal = Deal::query()
        .with("company")
        .without("company")
        .first(&t.db)
        .await
        .unwrap()
        .unwrap();
    assert!(!deal.relation_loaded("company"));
}

#[tokio::test]
async fn records_load_relations_afterwards() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    let mut deal = Deal::find_or_fail(&t.db, crm.renewal.id).await.unwrap();
    assert!(!deal.relation_loaded("company"));

    deal.load(&t.db, "company.owner").await.unwrap();
    let company = company_of(&deal).unwrap();
    assert_eq!(company.name, "Acme");
    assert!(company.relation_loaded("owner"));

    deal.load_constrained(&t.db, Deal::activities, |q| {
        q.where_(Activity::MINUTES.gt(10))
    })
    .await
    .unwrap();
    assert_eq!(activities_of(&deal).len(), 2);

    // `load_missing` keeps what is loaded and fills what is not, down nested
    // paths.
    t.log.clear();
    deal.load_missing(&t.db, ["activities", "company.owner", "owner"])
        .await
        .unwrap();
    assert_eq!(t.log.selects().len(), 1, "only `owner` was missing");
    assert_eq!(activities_of(&deal).len(), 2, "kept as loaded");
    assert_eq!(
        deal.loaded(Deal::owner).unwrap().as_ref().unwrap().name,
        "Ada"
    );
    deal.load_missing(&t.db, ["company.contacts"])
        .await
        .unwrap();
    let contacts = company_of(&deal)
        .unwrap()
        .relation::<Collection<Contact>>("contacts")
        .unwrap();
    assert_eq!(contacts.len(), 2);

    deal.load_all(&t.db, ["activities", "tags"]).await.unwrap();
    assert_eq!(activities_of(&deal).len(), 3, "loaded again, unconstrained");
    assert!(deal.relation_loaded("tags"));
}

#[tokio::test]
async fn collections_load_relations_afterwards() {
    let t = sqlite().await;
    seed(&t.db).await;
    let mut deals = Deal::query().order_by(Deal::ID).get(&t.db).await.unwrap();
    t.log.clear();
    deals.load(&t.db, "company").await.unwrap();
    assert_eq!(t.log.selects().len(), 1, "one query for every record");
    assert_eq!(company_of(&deals[1]).unwrap().name, "Acme");

    deals
        .load_constrained(&t.db, "activities", |q: Builder<Activity>| {
            q.where_(Activity::KIND.eq("meeting"))
        })
        .await
        .unwrap();
    assert_eq!(activities_of(&deals[0]).len(), 0);
    assert_eq!(activities_of(&deals[1]).len(), 1);

    t.log.clear();
    deals
        .load_missing(&t.db, ["company", "owner", "activities"])
        .await
        .unwrap();
    assert_eq!(t.log.selects().len(), 1, "only owners were missing");
    deals
        .load_all(&t.db, ["company.owner", "tags"])
        .await
        .unwrap();
    assert!(deals.iter().all(|deal| deal.relation_loaded("tags")));
}

#[tokio::test]
async fn loaded_relations_live_on_the_record() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    let mut deal = Deal::query()
        .with("company")
        .with("activities")
        .where_key(crm.renewal.id)
        .first(&t.db)
        .await
        .unwrap()
        .unwrap();

    // Typed by name, or by the relation method.
    assert!(
        deal.relation::<Option<Record<Company>>>("company")
            .unwrap()
            .is_some()
    );
    assert!(
        deal.relation::<Collection<Company>>("company").is_none(),
        "the wrong type reads as nothing"
    );
    assert_eq!(deal.loaded(Deal::activities).unwrap().len(), 3);
    assert_eq!(
        deal.relations().names().collect::<Vec<_>>(),
        ["company", "activities"]
    );

    deal.relation_mut::<Collection<Activity>>("activities")
        .unwrap()
        .push(Record::new(Activity::new("note", 1)));
    assert_eq!(activities_of(&deal).len(), 4);

    let without = deal.without_relation("activities");
    assert!(without.relation_loaded("company") && !without.relation_loaded("activities"));
    let bare = deal.without_relations();
    assert!(bare.relations().is_empty());
    assert!(
        deal.relation_loaded("activities"),
        "the original keeps them"
    );

    deal.unset_relation("company");
    assert!(!deal.relation_loaded("company"));
    deal.set_relation("company", Some(Record::new(Company::named("Hand made"))));
    assert_eq!(company_of(&deal).unwrap().name, "Hand made");

    let mut relations = Relations::new();
    relations.set_relation("owner", None::<Record<User>>);
    deal.set_relations(relations);
    assert_eq!(deal.relations().len(), 1);
    assert!(deal.relation_loaded("owner"));
    deal.unset_relations();
    assert!(deal.relations().is_empty());
}

#[tokio::test]
async fn eager_loads_respect_the_related_models_global_scopes() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    let mut upgrade = Deal::find_or_fail(&t.db, crm.upgrade.id).await.unwrap();
    assert!(upgrade.delete(&t.db).await.unwrap());

    // A soft-deleted deal is not loaded...
    let acme = Company::query()
        .with("deals")
        .where_key(crm.acme.id)
        .first(&t.db)
        .await
        .unwrap()
        .unwrap();
    let deals = acme.loaded(Company::deals).unwrap();
    assert_eq!(titles(deals), ["Renewal"]);
    // ...unless the constraint asks for trashed records.
    let acme = Company::query()
        .with_constrained(Company::deals, |q| q.with_trashed())
        .where_key(crm.acme.id)
        .first(&t.db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        titles(acme.loaded(Company::deals).unwrap()),
        ["Renewal", "Upgrade"]
    );

    // The same for a belongs-to.
    let activity = Activity::query()
        .where_(Activity::KIND.eq("meeting"))
        .with("deal")
        .first(&t.db)
        .await
        .unwrap()
        .unwrap();
    assert!(activity.loaded(Activity::deal).unwrap().is_none());
}

#[tokio::test]
async fn children_get_a_snapshot_of_their_parent() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    t.log.clear();
    // `Company::deals` names its inverse, `company`.
    let acme = Company::query()
        .with("deals")
        .where_key(crm.acme.id)
        .first(&t.db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(t.log.selects().len(), 2);
    for deal in acme.loaded(Company::deals).unwrap().iter() {
        let parent = company_of(deal).expect("set from the parent");
        assert_eq!(parent.id, crm.acme.id);
        // A snapshot without relations: no reference cycle.
        assert!(parent.relations().is_empty());
    }
    // Querying the relation of a record sets it too.
    let deals = crm.acme.deals().get(&t.db).await.unwrap();
    assert!(deals.iter().all(|deal| company_of(deal).is_some()));
    // Turned off for one query.
    let deals = crm.acme.deals().without_inverse().get(&t.db).await.unwrap();
    assert!(deals.iter().all(|deal| !deal.relation_loaded("company")));
    // A related record made through the relation gets it as well.
    let made = crm.acme.deals().make(Deal::new("Draft", 1)).unwrap();
    assert_eq!(made.company_id, Some(crm.acme.id));
    assert_eq!(company_of(&made).unwrap().name, "Acme");
}
