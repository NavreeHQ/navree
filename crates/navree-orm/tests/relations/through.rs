//! Relations through an intermediate model: has-one and has-many through,
//! composed from two relations, and soft-deleted intermediate records.

use navree_orm::prelude::*;

use crate::fixtures::{seed, titles};
use crate::models::{Contact, Deal, Team, User};
use crate::support::sqlite;

#[tokio::test]
async fn has_many_through_an_intermediate_model() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    let deals = crm
        .team
        .deals()
        .order_by(Deal::ID)
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(titles(&deals), ["Renewal", "Upgrade", "Platform"]);
    let big = crm
        .team
        .deals()
        .where_(Deal::AMOUNT_CENTS.gte(25_000))
        .order_by(Deal::ID)
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(titles(&big), ["Upgrade", "Platform"]);
    assert_eq!(crm.team.deals().count(&t.db).await.unwrap(), 3);
    // Keys are qualified by the related table, so the join is unambiguous.
    let found = crm
        .team
        .deals()
        .find(&t.db, crm.platform.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.title, "Platform");
    // The intermediate key is not left on the records.
    assert!(found.extras().is_empty(), "{:?}", found.extras());

    // Eager loaded for several parents at once.
    let other = Team::create(
        &t.db,
        Team {
            name: "Startups".into(),
            ..Team::default()
        },
    )
    .await
    .unwrap();
    t.log.clear();
    let teams = Team::query()
        .with("deals")
        .order_by(Team::ID)
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(t.log.selects().len(), 2);
    assert_eq!(teams[0].loaded(Team::deals).unwrap().len(), 3);
    assert!(teams[1].loaded(Team::deals).unwrap().is_empty());
    assert_eq!(teams[1].id, other.id);

    // Existence and counts.
    let selling = Team::query().has("deals").get(&t.db).await.unwrap();
    assert_eq!(selling.len(), 1);
    let counted = Team::query()
        .with_count("deals")
        .with_sum("deals", Deal::AMOUNT_CENTS)
        .order_by(Team::ID)
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(counted[0].extra::<i64>("deals_count").unwrap(), Some(3));
    assert_eq!(
        counted[0].extra::<i64>("deals_sum_amount_cents").unwrap(),
        Some(120_000)
    );
}

#[tokio::test]
async fn has_one_through_an_intermediate_model() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    let biggest = crm.team.biggest_deal().first(&t.db).await.unwrap().unwrap();
    assert_eq!(biggest.title, "Platform");
    let teams = Team::query().with("biggest_deal").get(&t.db).await.unwrap();
    let loaded = teams[0]
        .loaded(Team::biggest_deal)
        .unwrap()
        .as_ref()
        .unwrap();
    assert_eq!(loaded.title, "Platform");
    assert!(
        crm.team
            .biggest_deal()
            .is(&t.db, &crm.platform)
            .await
            .unwrap()
    );
    assert!(
        crm.team
            .biggest_deal()
            .is_not(&t.db, &crm.renewal)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn through_relations_composed_from_two_relations() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    // A belongs-to followed by a has-many: a deal's company's contacts.
    let contacts = crm
        .renewal
        .contacts()
        .order_by(Contact::ID)
        .get(&t.db)
        .await
        .unwrap();
    let names: Vec<&str> = contacts.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["Alice", "Carol"]);
    assert!(crm.orphan.contacts().get(&t.db).await.unwrap().is_empty());

    let deals = Deal::query()
        .with("contacts")
        .order_by(Deal::ID)
        .get(&t.db)
        .await
        .unwrap();
    let counts: Vec<usize> = deals
        .iter()
        .map(|deal| deal.loaded(Deal::contacts).unwrap().len())
        .collect();
    assert_eq!(counts, [2, 2, 1, 0]);
    let reachable = Deal::query()
        .where_relation("contacts", Contact::NAME.eq("Dave"))
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(titles(&reachable), ["Platform"]);

    // Two has-manys: a team's reps' deals.
    let rep_deals = crm
        .team
        .rep_deals()
        .order_by(Deal::ID)
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(titles(&rep_deals), ["Renewal", "Upgrade", "Platform"]);
}

#[tokio::test]
async fn soft_deleted_intermediate_records_are_left_out() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    let mut bob = User::find_or_fail(&t.db, crm.bob.id).await.unwrap();
    bob.delete(&t.db).await.unwrap();

    let deals = crm
        .team
        .deals()
        .order_by(Deal::ID)
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(
        titles(&deals),
        ["Renewal", "Platform"],
        "Bob's Upgrade is out"
    );
    let all = crm
        .team
        .all_deals()
        .order_by(Deal::ID)
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(titles(&all), ["Renewal", "Upgrade", "Platform"]);
    let counted = Team::query()
        .with_count("deals")
        .with_count("all_deals")
        .first(&t.db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(counted.extra::<i64>("deals_count").unwrap(), Some(2));
    assert_eq!(counted.extra::<i64>("all_deals_count").unwrap(), Some(3));
    let teams = Team::query().with("all_deals").get(&t.db).await.unwrap();
    assert_eq!(teams[0].loaded(Team::all_deals).unwrap().len(), 3);
}
