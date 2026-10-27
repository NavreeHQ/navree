//! Relation aggregates: counts, sums, averages, minima, maxima and
//! existence flags in queries (`with_*`) and afterwards (`load_*`).

use navree_orm::ir::AggregateFunction;
use navree_orm::prelude::*;

use crate::fixtures::seed;
use crate::models::{Activity, Company, Deal};
use crate::support::sqlite;

/// Extra `name` as an integer (`None` when absent or NULL).
fn extra_i64(record: &Record<Deal>, name: &str) -> Option<i64> {
    record.extra::<Option<i64>>(name).unwrap().flatten()
}

#[tokio::test]
async fn relation_counts_with_aliases_and_constraints() {
    let t = sqlite().await;
    seed(&t.db).await;
    let deals = Deal::query()
        .with_count("activities")
        .with_count("activities as touches")
        .with_count_constrained("activities as call_count", |q: Builder<Activity>| {
            q.where_(Activity::KIND.eq("call"))
        })
        .with_count_constrained(Deal::calls, |q| q.where_(Activity::MINUTES.gte(40)))
        .order_by(Deal::ID)
        .get(&t.db)
        .await
        .unwrap();
    let counts: Vec<[Option<i64>; 4]> = deals
        .iter()
        .map(|deal| {
            [
                extra_i64(deal, "activities_count"),
                extra_i64(deal, "touches"),
                extra_i64(deal, "call_count"),
                extra_i64(deal, "calls_count"),
            ]
        })
        .collect();
    assert_eq!(
        counts,
        [
            [Some(3), Some(3), Some(2), Some(1)],
            [Some(1), Some(1), Some(0), Some(0)],
            [Some(0), Some(0), Some(0), Some(0)],
            [Some(0), Some(0), Some(0), Some(0)],
        ]
    );
}

#[tokio::test]
async fn sums_averages_minima_maxima_and_existence() {
    let t = sqlite().await;
    seed(&t.db).await;
    let deals = Deal::query()
        .with_sum("activities", Activity::MINUTES)
        .with_avg(Deal::activities, Activity::MINUTES)
        .with_min("activities", Activity::MINUTES)
        .with_max("activities as longest", Activity::MINUTES)
        .with_aggregate("calls", Activity::MINUTES, AggregateFunction::Sum)
        .with_exists("activities")
        .with_exists("company")
        .order_by(Deal::ID)
        .get(&t.db)
        .await
        .unwrap();
    let renewal = &deals[0];
    assert_eq!(extra_i64(renewal, "activities_sum_minutes"), Some(80));
    let avg = renewal.extra::<f64>("activities_avg_minutes").unwrap().unwrap();
    assert!((avg - 80.0 / 3.0).abs() < 1e-9, "{avg}");
    assert_eq!(extra_i64(renewal, "activities_min_minutes"), Some(5));
    assert_eq!(extra_i64(renewal, "longest"), Some(45));
    assert_eq!(extra_i64(renewal, "calls_sum_minutes"), Some(75));
    assert_eq!(renewal.extra::<bool>("activities_exists").unwrap(), Some(true));
    assert_eq!(renewal.extra::<bool>("company_exists").unwrap(), Some(true));

    let orphan = &deals[3];
    assert_eq!(extra_i64(orphan, "activities_sum_minutes"), None);
    assert_eq!(orphan.extra::<bool>("activities_exists").unwrap(), Some(false));
    assert_eq!(orphan.extra::<bool>("company_exists").unwrap(), Some(false));

    // An aggregate takes a column.
    let error = Deal::query()
        .with_sum("activities", Expr::raw("1", Vec::<Value>::new()))
        .get(&t.db)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("column"), "{error}");
}

#[tokio::test]
async fn records_load_aggregates_afterwards() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    let mut deal = Deal::find_or_fail(&t.db, crm.renewal.id).await.unwrap();
    deal.load_count(&t.db, ["activities", "calls"]).await.unwrap();
    assert_eq!(extra_i64(&deal, "activities_count"), Some(3));
    assert_eq!(extra_i64(&deal, "calls_count"), Some(2));
    deal.load_sum(&t.db, "activities", Activity::MINUTES).await.unwrap();
    deal.load_min(&t.db, Deal::activities, Activity::MINUTES).await.unwrap();
    deal.load_max(&t.db, "activities", Activity::MINUTES).await.unwrap();
    deal.load_avg(&t.db, "calls", Activity::MINUTES).await.unwrap();
    deal.load_aggregate(&t.db, "activities", Activity::ID, AggregateFunction::Count)
        .await
        .unwrap();
    deal.load_exists(&t.db, ["company", "tags"]).await.unwrap();
    assert_eq!(extra_i64(&deal, "activities_sum_minutes"), Some(80));
    assert_eq!(extra_i64(&deal, "activities_min_minutes"), Some(5));
    assert_eq!(extra_i64(&deal, "activities_max_minutes"), Some(45));
    assert_eq!(
        deal.extra::<f64>("calls_avg_minutes").unwrap(),
        Some(37.5)
    );
    assert_eq!(extra_i64(&deal, "activities_count_id"), Some(3));
    assert_eq!(deal.extra::<bool>("company_exists").unwrap(), Some(true));
    assert_eq!(deal.extra::<bool>("tags_exists").unwrap(), Some(false));
    // The model's own columns are untouched.
    assert_eq!(deal.title, "Renewal");
}

#[tokio::test]
async fn collections_load_aggregates_in_one_query() {
    let t = sqlite().await;
    seed(&t.db).await;
    let mut deals = Deal::query().order_by(Deal::ID).get(&t.db).await.unwrap();
    t.log.clear();
    deals.load_count(&t.db, ["activities"]).await.unwrap();
    assert_eq!(t.log.selects().len(), 1);
    let counts: Vec<Option<i64>> = deals
        .iter()
        .map(|deal| extra_i64(deal, "activities_count"))
        .collect();
    assert_eq!(counts, [Some(3), Some(1), Some(0), Some(0)]);
    deals.load_sum(&t.db, "activities", Activity::MINUTES).await.unwrap();
    deals.load_avg(&t.db, "activities", Activity::MINUTES).await.unwrap();
    deals.load_min(&t.db, "activities", Activity::MINUTES).await.unwrap();
    deals.load_max(&t.db, "activities", Activity::MINUTES).await.unwrap();
    deals.load_exists(&t.db, ["company"]).await.unwrap();
    deals
        .load_aggregate(&t.db, "activities", Activity::MINUTES, AggregateFunction::Max)
        .await
        .unwrap();
    assert_eq!(extra_i64(&deals[1], "activities_sum_minutes"), Some(60));
    assert_eq!(extra_i64(&deals[1], "activities_max_minutes"), Some(60));
    assert_eq!(
        deals[1].extra::<f64>("activities_avg_minutes").unwrap(),
        Some(60.0)
    );
    assert_eq!(extra_i64(&deals[0], "activities_min_minutes"), Some(5));
    assert_eq!(deals[3].extra::<bool>("company_exists").unwrap(), Some(false));

    let mut companies = Company::query().order_by(Company::ID).get(&t.db).await.unwrap();
    companies.load_count(&t.db, ["deals", "contacts"]).await.unwrap();
    let counts: Vec<(Option<i64>, Option<i64>)> = companies
        .iter()
        .map(|company| {
            (
                company.extra::<i64>("deals_count").unwrap(),
                company.extra::<i64>("contacts_count").unwrap(),
            )
        })
        .collect();
    assert_eq!(counts, [(Some(2), Some(2)), (Some(0), Some(0)), (Some(1), Some(1))]);
}
