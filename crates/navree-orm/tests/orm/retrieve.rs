//! Single records: `first`, `first_or_fail`, `sole`, `find` on a query.

use navree_orm::{Model, OrmError};

use crate::models::{Company, Deal};
use crate::support::sqlite;

#[tokio::test]
async fn first_and_first_or_fail() {
    let t = sqlite().await;
    assert!(Company::query().first(&t.db).await.unwrap().is_none());
    let error = Company::query().first_or_fail(&t.db).await.unwrap_err();
    assert!(error.is_not_found());
    assert_eq!(error.to_string(), "no Company found");

    for name in ["B", "A"] {
        Company::create(&t.db, Company { name: name.into(), ..Company::default() })
            .await
            .unwrap();
    }
    let first = Company::query().order_by(Company::NAME).first_or_fail(&t.db).await.unwrap();
    assert_eq!(first.name, "A");
}

#[tokio::test]
async fn sole_needs_exactly_one_record() {
    let t = sqlite().await;
    let error = Company::query().sole(&t.db).await.unwrap_err();
    assert!(error.is_not_found());
    Company::create(&t.db, Company { name: "A".into(), ..Company::default() })
        .await
        .unwrap();
    assert_eq!(Company::query().sole(&t.db).await.unwrap().name, "A");
    Company::create(&t.db, Company { name: "B".into(), ..Company::default() })
        .await
        .unwrap();
    let error = Company::query().sole(&t.db).await.unwrap_err();
    assert!(matches!(error, OrmError::MultipleRecordsFound { .. }), "{error:?}");
    assert_eq!(error.to_string(), "expected exactly one Company, found more than one");
}

#[tokio::test]
async fn find_respects_the_query() {
    let t = sqlite().await;
    let deal = Deal::create(&t.db, Deal::new("Renewal", 100)).await.unwrap();
    assert!(Deal::query().where_(Deal::AMOUNT_CENTS.gt(500)).find(&t.db, deal.id).await.unwrap().is_none());
    assert!(Deal::query().find(&t.db, deal.id).await.unwrap().is_some());
    let error = Deal::query()
        .where_(Deal::AMOUNT_CENTS.gt(500))
        .find_or_fail(&t.db, deal.id)
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), format!("no Deal found for key {}", deal.id));
    assert!(Deal::query().find_many(&t.db, Vec::<uuid::Uuid>::new()).await.unwrap().is_empty());
}

#[tokio::test]
async fn not_found_errors_carry_a_record_not_found_source() {
    use std::error::Error as _;
    let t = sqlite().await;
    let error = Company::find_or_fail(&t.db, 9).await.unwrap_err();
    let source = error.source().and_then(|s| s.downcast_ref::<navree_db::sea_orm::DbErr>());
    assert!(matches!(source, Some(navree_db::sea_orm::DbErr::RecordNotFound(_))));
}
