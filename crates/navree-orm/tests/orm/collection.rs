//! `Collection`: finding, filtering, grouping, plucking and refreshing
//! records.

use navree_db::Executor as _;
use navree_orm::{Collection, Model};

use crate::models::{Company, Deal, Stage};
use crate::support::{TestDb, sqlite};

async fn seeded() -> (TestDb, Collection<Company>) {
    let t = sqlite().await;
    for (name, country, employees) in [
        ("Acme", Some("US"), 10),
        ("Globex", Some("DE"), 20),
        ("Initech", Some("US"), 30),
    ] {
        Company::create(
            &t.db,
            Company {
                name: name.into(),
                country: country.map(Into::into),
                employees,
                ..Company::default()
            },
        )
        .await
        .unwrap();
    }
    let all = Company::query()
        .order_by(Company::ID)
        .get(&t.db)
        .await
        .unwrap();
    (t, all)
}

#[tokio::test]
async fn keys_finding_and_containment() {
    let (_t, all) = seeded().await;
    assert_eq!(all.len(), 3);
    assert_eq!(all.model_keys(), [1, 2, 3]);
    assert_eq!(all.find(2).unwrap().name, "Globex");
    assert!(all.contains(3));
    assert!(!all.contains(9));
    assert_eq!(all[0].name, "Acme", "indexes like a slice");
    assert_eq!(all.first().unwrap().id, 1);
    assert_eq!(all.iter().map(|c| c.employees).sum::<i32>(), 60);
}

#[tokio::test]
async fn transforming_and_grouping() {
    let (_t, all) = seeded().await;
    assert_eq!(all.map(|c| c.name.clone()), ["Acme", "Globex", "Initech"]);
    assert_eq!(
        all.pluck(Company::NAME).unwrap(),
        ["Acme", "Globex", "Initech"]
    );
    assert_eq!(
        all.pluck(Company::COUNTRY).unwrap(),
        [
            Some("US".to_owned()),
            Some("DE".to_owned()),
            Some("US".to_owned())
        ]
    );
    let big = all.clone().filter(|c| c.employees > 15);
    assert_eq!(big.model_keys(), [2, 3]);
    let (us, other) = all
        .clone()
        .partition(|c| c.country.as_deref() == Some("US"));
    assert_eq!((us.len(), other.len()), (2, 1));

    let by_country = all
        .clone()
        .group_by(|c| c.country.clone().unwrap_or_default());
    assert_eq!(by_country.keys().collect::<Vec<_>>(), ["US", "DE"]);
    assert_eq!(by_country["US"].model_keys(), [1, 3]);
    let by_name = all.clone().key_by(|c| c.name.clone());
    assert_eq!(by_name["Initech"].id, 3);

    let sorted = all.clone().sort_by_key(|c| std::cmp::Reverse(c.employees));
    assert_eq!(sorted.model_keys(), [3, 2, 1]);
    assert_eq!(all.clone().reverse().model_keys(), [3, 2, 1]);
}

#[tokio::test]
async fn set_operations_by_key() {
    let (_t, all) = seeded().await;
    let some: Collection<Company> = all.clone().only([1_i64, 3]);
    assert_eq!(all.clone().diff(&some).model_keys(), [2]);
    assert_eq!(all.clone().intersect(&some).model_keys(), [1, 3]);
    assert_eq!(all.clone().except([1_i64]).model_keys(), [2, 3]);
    let doubled: Collection<Company> = all.iter().chain(all.iter()).cloned().collect();
    assert_eq!(doubled.len(), 6);
    assert_eq!(doubled.unique().model_keys(), [1, 2, 3]);
}

#[tokio::test]
async fn queries_and_refreshes() {
    let (t, all) = seeded().await;
    let updated = all
        .clone()
        .only([1_i64, 2])
        .to_query()
        .update(&t.db, [Company::EMPLOYEES.set(99)])
        .await
        .unwrap();
    assert_eq!(updated, 2);
    t.db.pool()
        .execute_script("DELETE FROM companies WHERE id = 3")
        .await
        .unwrap();
    let fresh = all.fresh(&t.db).await.unwrap();
    assert_eq!(fresh.model_keys(), [1, 2]);
    assert_eq!(fresh[0].employees, 99);
    assert_eq!(all[0].employees, 10, "the old collection is unchanged");
}

#[tokio::test]
async fn serializes_as_an_array_of_records() {
    let t = sqlite().await;
    Deal::create(
        &t.db,
        Deal {
            stage: Stage::Qualified,
            ..Deal::new("A", 5)
        },
    )
    .await
    .unwrap();
    let deals = Deal::all(&t.db).await.unwrap();
    let json = serde_json::to_value(&deals).unwrap();
    assert_eq!(json.as_array().unwrap().len(), 1);
    assert_eq!(json[0]["title"], "A");
    assert_eq!(json[0]["stage"], "qualified");
    assert!(json[0].get("scratch").is_none());
    assert_eq!(deals.clone().into_models()[0].title, "A");
}
