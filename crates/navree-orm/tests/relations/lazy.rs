//! Loading relations one record at a time: the lazy-loading rules
//! (prevented, handled, automatic), saving records with their loaded
//! relations (`push`), and refreshing loaded relations.

use std::sync::{Arc, Mutex};

use navree_orm::OrmError;
use navree_orm::prelude::*;
use navree_orm::relations::LazyLoading;

use crate::fixtures::seed;
use crate::models::{Activity, Company, Deal, Tag, User};
use crate::support::sqlite;

#[tokio::test]
async fn lazy_loads_of_records_from_a_result_of_several_can_be_prevented() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    LazyLoading::new()
        .prevented()
        .scope(async {
            assert!(LazyLoading::current().is_prevented());
            let mut deals = Deal::query()
                .order_by(Deal::ID)
                .get(&t.db)
                .await
                .unwrap()
                .into_vec();
            let error = deals[0]
                .relation_or_load::<Option<Record<Company>>, _>(&t.db, "company")
                .await
                .unwrap_err();
            let OrmError::LazyLoading { model, relation } = &error else {
                panic!("{error:?}");
            };
            assert_eq!((model.as_ref(), relation.as_str()), ("Deal", "company"));
            assert!(error.to_string().contains("company"), "{error}");
            assert!(deals[0].related(&t.db, Deal::activities).await.is_err());

            // A record fetched alone may still load lazily.
            let mut alone = Deal::find_or_fail(&t.db, crm.renewal.id).await.unwrap();
            let company = alone
                .relation_or_load::<Option<Record<Company>>, _>(&t.db, "company")
                .await
                .unwrap();
            assert_eq!(company.as_ref().unwrap().name, "Acme");
            // An eager-loaded relation is no lazy load.
            let mut loaded = Deal::query().with("company").get(&t.db).await.unwrap().into_vec();
            assert!(
                loaded[1]
                    .relation_or_load::<Option<Record<Company>>, _>(&t.db, "company")
                    .await
                    .is_ok()
            );
        })
        .await;

    // Outside the scope, the installed rules apply (none by default).
    let mut deals = Deal::query().get(&t.db).await.unwrap().into_vec();
    let activities = deals[0].related(&t.db, Deal::activities).await.unwrap();
    assert_eq!(activities.len(), 3);
}

#[tokio::test]
async fn lazy_loading_violations_can_be_handled_another_way() {
    let t = sqlite().await;
    seed(&t.db).await;
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let log = seen.clone();
    LazyLoading::new()
        .prevented()
        .on_violation(move |violation| {
            log.lock().unwrap().push(format!(
                "{}.{}: {violation}",
                violation.model(),
                violation.relation()
            ));
            Ok(())
        })
        .scope(async {
            let mut deals = Deal::query().get(&t.db).await.unwrap().into_vec();
            let owner = deals[0]
                .relation_or_load::<Option<Record<User>>, _>(&t.db, "owner")
                .await
                .unwrap();
            assert_eq!(owner.as_ref().unwrap().name, "Ada", "the load went ahead");
        })
        .await;
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert!(seen[0].starts_with("Deal.owner: "), "{}", seen[0]);
    assert!(seen[0].contains("multi-record result"), "{}", seen[0]);
}

#[tokio::test]
async fn automatic_eager_loading_loads_for_the_whole_result() {
    let t = sqlite().await;
    seed(&t.db).await;
    LazyLoading::new()
        .automatic()
        .scope(async {
            assert!(LazyLoading::current().is_automatic());
            let mut deals = Deal::query().order_by(Deal::ID).get(&t.db).await.unwrap().into_vec();
            t.log.clear();
            let company = deals[0]
                .relation_or_load::<Option<Record<Company>>, _>(&t.db, "company")
                .await
                .unwrap();
            assert_eq!(company.as_ref().unwrap().name, "Acme");
            assert_eq!(t.log.selects().len(), 1);
            // The other records of the result find it loaded: no query.
            for deal in deals.iter_mut().skip(1) {
                deal.relation_or_load::<Option<Record<Company>>, _>(&t.db, "company")
                    .await
                    .unwrap();
            }
            assert!(t.log.selects().is_empty());
            let names: Vec<Option<String>> = deals
                .iter()
                .map(|deal| {
                    deal.loaded(Deal::company)
                        .unwrap()
                        .as_ref()
                        .map(|company| company.name.clone())
                })
                .collect();
            assert_eq!(
                names,
                [
                    Some("Acme".to_owned()),
                    Some("Acme".to_owned()),
                    Some("Globex".to_owned()),
                    None
                ]
            );
        })
        .await;
}

#[tokio::test]
async fn collections_and_records_opt_into_automatic_loading() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    let mut deals = Deal::query()
        .order_by(Deal::ID)
        .get(&t.db)
        .await
        .unwrap()
        .with_relationship_autoloading()
        .into_vec();
    t.log.clear();
    for deal in &mut deals {
        deal.related(&t.db, Deal::activities).await.unwrap();
    }
    assert_eq!(t.log.selects().len(), 1, "one query for the four deals");
    assert_eq!(deals[0].loaded(Deal::activities).unwrap().len(), 3);

    // Nested: the loaded records load together too.
    let mut acme = Company::find_or_fail(&t.db, crm.acme.id)
        .await
        .unwrap()
        .with_relationship_autoloading();
    acme.related(&t.db, Company::deals).await.unwrap();
    let mut deals = acme
        .relation_mut::<Collection<Deal>>("deals")
        .unwrap()
        .clone()
        .into_vec();
    t.log.clear();
    for deal in &mut deals {
        deal.related(&t.db, Deal::owner).await.unwrap();
    }
    assert_eq!(t.log.selects().len(), 1, "both deals' owners in one query");
}

#[tokio::test]
async fn push_saves_the_record_and_its_loaded_relations() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    crm.renewal.tags().attach(&t.db, [crm.vip.id]).await.unwrap();
    let mut deal = Deal::query()
        .with("company")
        .with("activities")
        .with("tags")
        .where_key(crm.renewal.id)
        .first(&t.db)
        .await
        .unwrap()
        .unwrap();

    deal.title = "Renewal 2027".into();
    deal.relation_mut::<Option<Record<Company>>>("company")
        .unwrap()
        .as_mut()
        .unwrap()
        .name = "Acme Inc".into();
    for activity in deal
        .relation_mut::<Collection<Activity>>("activities")
        .unwrap()
        .iter_mut()
    {
        activity.minutes += 1;
    }
    // A new record in a loaded relation is inserted.
    deal.relation_mut::<Collection<Activity>>("activities")
        .unwrap()
        .push(crm.renewal.activities().make(Activity::new("sms", 1)).unwrap());
    // Pivots are saved too.
    let tags = deal.relation_mut::<Collection<Tag>>("tags").unwrap();
    tags[0]
        .relation_mut::<navree_orm::relations::Pivot>("pivot")
        .unwrap()
        .set("role", "pushed");
    tags[0].name = "VIP".into();

    assert!(deal.push(&t.db).await.unwrap());
    assert_eq!(
        Deal::find_or_fail(&t.db, crm.renewal.id).await.unwrap().title,
        "Renewal 2027"
    );
    assert_eq!(
        Company::find_or_fail(&t.db, crm.acme.id).await.unwrap().name,
        "Acme Inc"
    );
    let minutes = crm
        .renewal
        .activities()
        .order_by(Activity::ID)
        .pluck(&t.db, Activity::MINUTES)
        .await
        .unwrap();
    assert_eq!(minutes, [31, 6, 46, 1]);
    let tag = crm.renewal.tags().first(&t.db).await.unwrap().unwrap();
    assert_eq!(tag.name, "VIP");
    assert_eq!(
        tag.pivot_value::<String>("role").unwrap().as_deref(),
        Some("pushed")
    );

    // Quietly: the same, without hooks.
    deal.title = "Renewal 2028".into();
    assert!(deal.push_quietly(&t.db).await.unwrap());
    assert_eq!(
        Deal::find_or_fail(&t.db, crm.renewal.id).await.unwrap().title,
        "Renewal 2028"
    );
}

#[tokio::test]
async fn refresh_reloads_loaded_relations() {
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
    deal.set_relation("custom", Some(Record::new(Company::named("By hand"))));

    // Changes made elsewhere.
    let mut acme = Company::find_or_fail(&t.db, crm.acme.id).await.unwrap();
    acme.name = "Acme Renamed".into();
    acme.save(&t.db).await.unwrap();
    crm.renewal
        .activities()
        .create(&t.db, Activity::new("call", 1))
        .await
        .unwrap();

    deal.refresh(&t.db).await.unwrap();
    assert_eq!(
        deal.loaded(Deal::company).unwrap().as_ref().unwrap().name,
        "Acme Renamed"
    );
    assert_eq!(deal.loaded(Deal::activities).unwrap().len(), 4);
    // What is not a relation of the model is left as it is.
    assert_eq!(
        deal.relation::<Option<Record<Company>>>("custom")
            .unwrap()
            .as_ref()
            .unwrap()
            .name,
        "By hand"
    );
}
