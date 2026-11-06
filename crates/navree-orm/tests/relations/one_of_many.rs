//! One record out of many (latest, oldest, by aggregates with
//! constraints), to-many relations as to-one, and default records when
//! nothing is related.

use navree_orm::ir::AggregateFunction;
use navree_orm::prelude::*;

use crate::fixtures::seed;
use crate::models::{Activity, Company, Deal};
use crate::support::sqlite;

fn one(deal: &Record<Deal>, name: &str) -> Option<Record<Activity>> {
    deal.relation::<Option<Record<Activity>>>(name)
        .unwrap_or_else(|| panic!("{name} is loaded"))
        .clone()
}

#[tokio::test]
async fn latest_oldest_and_by_aggregate() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    let latest = crm
        .renewal
        .latest_activity()
        .first(&t.db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!((latest.kind.as_str(), latest.minutes), ("call", 45));
    let first = crm
        .renewal
        .first_activity()
        .first(&t.db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.minutes, 30);
    let longest = crm
        .renewal
        .longest_call()
        .first(&t.db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(longest.minutes, 45);
    assert!(
        crm.upgrade
            .longest_call()
            .first(&t.db)
            .await
            .unwrap()
            .is_none(),
        "no calls"
    );

    // Eager loaded: one query for every deal.
    t.log.clear();
    let deals = Deal::query()
        .with("latest_activity")
        .with("first_activity")
        .with("longest_call")
        .order_by(Deal::ID)
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(t.log.selects().len(), 4);
    let minutes = |name: &str| -> Vec<Option<i64>> {
        deals
            .iter()
            .map(|deal| one(deal, name).map(|a| a.minutes))
            .collect()
    };
    assert_eq!(minutes("latest_activity"), [Some(45), Some(60), None, None]);
    assert_eq!(minutes("first_activity"), [Some(30), Some(60), None, None]);
    assert_eq!(minutes("longest_call"), [Some(45), None, None, None]);

    // A constraint of the eager load filters the record picked: Renewal's
    // latest activity is a call, so it holds nothing.
    let deals = Deal::query()
        .with_constrained(Deal::latest_activity, |q| {
            q.where_(Activity::KIND.ne("call"))
        })
        .order_by(Deal::ID)
        .get(&t.db)
        .await
        .unwrap();
    let kinds: Vec<Option<String>> = deals
        .iter()
        .map(|deal| one(deal, "latest_activity").map(|a| a.kind.clone()))
        .collect();
    assert_eq!(kinds, [None, Some("meeting".to_owned()), None, None]);

    // Existence and counts see one record at most.
    let with_latest_call = Deal::query()
        .where_has(Deal::latest_activity, |q| {
            q.where_(Activity::KIND.eq("call"))
        })
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(with_latest_call.len(), 1);
    let counted = Deal::query()
        .with_count("latest_activity")
        .order_by(Deal::ID)
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(
        counted[0].extra::<i64>("latest_activity_count").unwrap(),
        Some(1)
    );
}

#[tokio::test]
async fn ties_and_custom_aggregates() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    // Two calls of 45 minutes: the larger key wins the tie.
    let tie = crm
        .renewal
        .activities()
        .create(&t.db, Activity::new("call", 45))
        .await
        .unwrap();
    let longest = crm
        .renewal
        .longest_call()
        .first(&t.db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(longest.id, tie.id);

    // Several columns: the longest, then the smallest key among them.
    let relation = crm.renewal.has_many(Activity::DEAL_ID).of_many(
        [
            (Activity::MINUTES, AggregateFunction::Max),
            (Activity::ID, AggregateFunction::Min),
        ],
        |q| q,
    );
    let picked = relation.first(&t.db).await.unwrap().unwrap();
    assert_eq!(picked.minutes, 45);
    assert_ne!(picked.id, tie.id, "the smaller key among the longest");

    // Only MAX and MIN pick one record.
    let error = crm
        .renewal
        .has_many(Activity::DEAL_ID)
        .of_many([(Activity::MINUTES, AggregateFunction::Sum)], |q| q)
        .first(&t.db)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("MAX or MIN"), "{error}");
    assert!(crm.renewal.latest_activity().is_one_of_many());
    assert!(!crm.renewal.activities().one().is_one_of_many());
}

#[tokio::test]
async fn to_many_relations_as_to_one() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    let any = crm.upgrade.activities().one();
    let activity = any.first(&t.db).await.unwrap().unwrap();
    assert_eq!(activity.kind, "meeting");
    let contact = crm
        .agra
        .contacts()
        .one()
        .oldest_of_many()
        .first(&t.db)
        .await
        .unwrap();
    assert_eq!(contact.unwrap().name, "Alice");
    let biggest = crm.team.biggest_deal().first(&t.db).await.unwrap().unwrap();
    assert_eq!(biggest.title, "Platform");
    let a_deal = crm.team.deals().one().first(&t.db).await.unwrap();
    assert!(a_deal.is_some());
}

#[tokio::test]
async fn defaults_when_nothing_is_related() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    // A belongs-to made from the parent.
    let placeholder = crm
        .orphan
        .company_or_placeholder()
        .results(&t.db)
        .await
        .unwrap()
        .unwrap();
    assert!(!placeholder.exists());
    assert_eq!(placeholder.name, "No company for Orphan");
    let real = crm
        .renewal
        .company_or_placeholder()
        .results(&t.db)
        .await
        .unwrap()
        .unwrap();
    assert!(real.exists());

    // Eager loaded: the default stands in for `None`.
    let deals = Deal::query()
        .with("company_or_placeholder")
        .with("first_activity_or_default")
        .order_by(Deal::ID)
        .get(&t.db)
        .await
        .unwrap();
    let company = |deal: &Record<Deal>| {
        deal.relation::<Option<Record<Company>>>("company_or_placeholder")
            .unwrap()
            .clone()
            .unwrap()
    };
    assert_eq!(company(&deals[0]).name, "Agra");
    assert_eq!(company(&deals[3]).name, "No company for Orphan");
    let activity = one(&deals[2], "first_activity_or_default").unwrap();
    assert_eq!(activity.kind, "none yet");
    // A has-one default carries the parent's key.
    assert_eq!(activity.deal_id, Some(crm.platform.id));
    assert!(!activity.exists());
    assert_eq!(
        one(&deals[0], "first_activity_or_default").unwrap().minutes,
        30
    );

    // `with_default` uses the model's `Default`.
    let blank = crm
        .orphan
        .company()
        .with_default()
        .results(&t.db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(blank.name, "");
    let valued = crm
        .platform
        .latest_activity()
        .with_default_value(Activity::new("placeholder", 0))
        .results(&t.db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(valued.kind, "placeholder");
    let through = crm
        .team
        .a_deal()
        .where_(Deal::TITLE.eq("missing"))
        .with_default_fn(|team| Deal::new(&format!("{} pipeline", team.name), 0))
        .results(&t.db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(through.title, "Enterprise pipeline");
}
