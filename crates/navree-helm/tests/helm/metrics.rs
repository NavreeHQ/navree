//! Metrics: values with ranges, trends, partitions, access and caching.

use chrono::{TimeZone, Utc};
use navree_db::QueryLog;
use navree_helm::{Field, Metric, ModuleBuilderHelmExt, Resource};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_testing::{TestApp, principal};
use serde_json::json;
use uuid::Uuid;

use crate::support::*;

/// Four deals around the app's clock (Monday 26 October 2026, noon UTC).
async fn seed(app: &TestApp, owner: Uuid) {
    let at = |m, d, h| Utc.with_ymd_and_hms(2026, m, d, h, 0, 0).unwrap();
    let globex = company(app, "Globex", None).await;
    let initech = company(app, "Initech", None).await;
    deal(
        app,
        DealSeed {
            title: "One",
            amount_cents: 1000,
            stage: "lead",
            owner: Some(owner),
            company: Some(globex.id),
            created_at: at(10, 26, 9),
            ..DealSeed::default()
        },
    )
    .await;
    deal(
        app,
        DealSeed {
            title: "Two",
            amount_cents: 2000,
            stage: "won",
            closed: true,
            company: Some(globex.id),
            created_at: at(10, 25, 15),
            ..DealSeed::default()
        },
    )
    .await;
    deal(
        app,
        DealSeed {
            title: "Three",
            amount_cents: 3000,
            stage: "lead",
            company: Some(initech.id),
            created_at: at(10, 20, 10),
            ..DealSeed::default()
        },
    )
    .await;
    deal(
        app,
        DealSeed {
            title: "Four",
            amount_cents: 4000,
            stage: "lost",
            closed: true,
            created_at: at(9, 10, 8),
            ..DealSeed::default()
        },
    )
    .await;
}

#[tokio::test]
async fn value_metrics_aggregate_the_readable_rows() {
    let app = app().await;
    let (rep_id, rep) = user(&app, "rep").await;
    seed(&app, rep_id).await;
    rep.get("/admin/api/resources/deals/metrics/open-pipeline")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("kind", json!("value"))
        .assert_json_path("label", json!("Open pipeline"))
        .assert_json_path("value", json!(4000.0))
        .assert_json_path("display", json!("$40.00"))
        .assert_json_path("currency", json!("USD"))
        .assert_json_path("range", json!(null));

    // Rows the user may not read are left out.
    let mine = Uuid::now_v7();
    give_role(&app, mine, "own-only", &["helm.view"]).await;
    let own_only = app.acting_as(principal(mine));
    own_only
        .get("/admin/api/resources/deals/metrics/open-pipeline")
        .send()
        .await
        .assert_json_path("value", json!(0.0))
        .assert_json_path("display", json!("$0.00"));
}

#[tokio::test]
async fn ranged_values_compare_with_the_previous_period() {
    let app = app().await;
    let (rep_id, rep) = user(&app, "rep").await;
    seed(&app, rep_id).await;
    rep.get("/admin/api/resources/deals/metrics/new-deals")
        .send()
        .await
        .assert_json_path("range", json!("30"))
        .assert_json_path("value", json!(3.0))
        .assert_json_path("previous", json!(1.0))
        .assert_json_path("change", json!(2.0))
        .assert_json_path("display", json!("3"))
        .assert_json_path(
            "ranges.3",
            json!({ "value": "month", "label": "This month" }),
        );
    rep.get("/admin/api/resources/deals/metrics/new-deals")
        .query(&[("range", "7")])
        .send()
        .await
        .assert_json_path("value", json!(3.0))
        .assert_json_path("previous", json!(0.0))
        .assert_json_path("change", json!(null));
    rep.get("/admin/api/resources/deals/metrics/new-deals")
        .query(&[("range", "year")])
        .send()
        .await
        .assert_json_path("value", json!(4.0));
    rep.get("/admin/api/resources/deals/metrics/new-deals")
        .query(&[("range", "week")])
        .send()
        .await
        .assert_validation_error("range");
}

#[tokio::test]
async fn trends_bucket_by_day() {
    let app = app().await;
    let (rep_id, rep) = user(&app, "rep").await;
    seed(&app, rep_id).await;
    let body = rep
        .get("/admin/api/resources/deals/metrics/deals-by-day")
        .send()
        .await
        .assert_status(200)
        .json_value();
    let points: Vec<(String, f64)> = body["points"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p["key"].as_str().unwrap().to_owned(),
                p["value"].as_f64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        points,
        [
            ("2026-10-20".to_owned(), 1.0),
            ("2026-10-21".to_owned(), 0.0),
            ("2026-10-22".to_owned(), 0.0),
            ("2026-10-23".to_owned(), 0.0),
            ("2026-10-24".to_owned(), 0.0),
            ("2026-10-25".to_owned(), 1.0),
            ("2026-10-26".to_owned(), 1.0),
        ]
    );
    assert_eq!(body["value"], 3.0);
}

#[tokio::test]
async fn partitions_group_with_labels() {
    let app = app().await;
    let (rep_id, rep) = user(&app, "rep").await;
    seed(&app, rep_id).await;
    let body = rep
        .get("/admin/api/resources/deals/metrics/deals-by-stage")
        .send()
        .await
        .assert_status(200)
        .json_value();
    let groups: Vec<(&str, &str, f64)> = body["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| {
            (
                g["key"].as_str().unwrap(),
                g["label"].as_str().unwrap(),
                g["value"].as_f64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        groups,
        [
            ("lead", "Lead", 2.0),
            ("lost", "Lost", 1.0),
            ("won", "Won", 1.0)
        ]
    );

    let body = rep
        .get("/admin/api/resources/deals/metrics/pipeline-by-company")
        .send()
        .await
        .json_value();
    let labels: Vec<(&str, &str)> = body["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| (g["label"].as_str().unwrap(), g["display"].as_str().unwrap()))
        .collect();
    assert_eq!(
        labels,
        [
            ("None", "$40.00"),
            ("Globex", "$30.00"),
            ("Initech", "$30.00")
        ]
    );
}

#[tokio::test]
async fn results_are_cached_per_user_and_range() {
    let app = app().await;
    let (rep_id, rep) = user(&app, "rep").await;
    seed(&app, rep_id).await;
    let open = || async {
        rep.get("/admin/api/resources/deals/metrics/open-pipeline")
            .send()
            .await
            .json_value()["value"]
            .as_f64()
            .unwrap()
    };
    assert_eq!(open().await, 4000.0);
    deal(
        &app,
        DealSeed {
            amount_cents: 500,
            ..DealSeed::default()
        },
    )
    .await;
    assert_eq!(open().await, 4000.0, "cached for helm.metrics_cache_secs");
    // Another user computes their own.
    let (_, manager) = user(&app, "manager").await;
    manager
        .get("/admin/api/resources/deals/metrics/open-pipeline")
        .send()
        .await
        .assert_json_path("value", json!(4500.0));

    let uncached = builder().config("helm.metrics_cache_secs", 0).build().await;
    let (_, rep) = user(&uncached, "rep").await;
    let first = rep
        .get("/admin/api/resources/deals/metrics/open-pipeline")
        .send()
        .await
        .json_value()["value"]
        .clone();
    deal(
        &uncached,
        DealSeed {
            amount_cents: 500,
            ..DealSeed::default()
        },
    )
    .await;
    let second = rep
        .get("/admin/api/resources/deals/metrics/open-pipeline")
        .send()
        .await
        .json_value()["value"]
        .clone();
    assert_eq!((first, second), (json!(0.0), json!(500.0)));
}

#[tokio::test]
async fn unknown_metrics_are_404_and_unreadable_ones_403() {
    let app = app().await;
    let (_, rep) = user(&app, "rep").await;
    rep.get("/admin/api/resources/deals/metrics/revenue")
        .send()
        .await
        .assert_error(404, None);
    let none = Uuid::now_v7();
    give_role(&app, none, "nothing", &["helm.view"]).await;
    app.acting_as(principal(none))
        .get("/admin/api/resources/deals/metrics/open-pipeline")
        .send()
        .await
        .assert_error(403, None);
}

#[tokio::test]
async fn a_trend_of_counts_reads_the_column_it_buckets_by_and_no_records() {
    // SQLite takes any text in a UUID column, so a record that cannot be read
    // whole is easy to make there; the trend must not try to read it.
    if crate::servers::Server::current() != crate::servers::Server::Sqlite {
        return;
    }
    let app = app().await;
    let (rep_id, rep) = user(&app, "rep").await;
    seed(&app, rep_id).await;
    {
        use navree_db::Executor as _;
        let db = app.get::<navree_db::Db>().await;
        db.execute(&navree_db::Statement::text(
            "UPDATE deals SET company_id = 'not a uuid'".to_owned(),
        ))
        .await
        .unwrap();
    }
    // The records cannot be read whole now...
    assert_ne!(
        rep.get("/admin/api/resources/deals")
            .send()
            .await
            .status()
            .as_u16(),
        200
    );
    // ...and the line of counts is the same as with ones that can.
    let body = rep
        .get("/admin/api/resources/deals/metrics/deals-by-day")
        .send()
        .await
        .assert_status(200)
        .json_value();
    let total: f64 = body["points"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["value"].as_f64().unwrap())
        .sum();
    assert!(total > 0.0, "{body}");
    assert_eq!(body["value"], json!(total));
}

/// What a trend asks the database: one grouped statement, whatever the
/// aggregate and however many rows are in its window.
#[tokio::test]
async fn a_trend_is_grouped_by_the_database_in_one_statement() {
    let app = builder().module(GroupedTrendModule).build().await;
    let at = |day: u32| Utc.with_ymd_and_hms(2026, 10, day, 6, 0, 0).unwrap();
    for (day, cents) in [(26, 100), (26, 200), (25, 50), (20, 7), (1, 999)] {
        deal(
            &app,
            DealSeed {
                amount_cents: cents,
                created_at: at(day),
                ..DealSeed::default()
            },
        )
        .await;
    }
    let (_, manager) = user(&app, "manager").await;
    for (slug, expected) in [
        ("count-by-day", [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 2.0]),
        ("sum-by-day", [7.0, 0.0, 0.0, 0.0, 0.0, 50.0, 300.0]),
        ("average-by-day", [7.0, 0.0, 0.0, 0.0, 0.0, 50.0, 150.0]),
        ("largest-by-day", [7.0, 0.0, 0.0, 0.0, 0.0, 50.0, 200.0]),
    ] {
        let path = format!("/admin/api/resources/grouped-trend/metrics/{slug}");
        let (response, queries) = QueryLog::capture(manager.get(&path).send()).await;
        let body = response.assert_status(200).json_value();
        let points: Vec<f64> = body["points"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["value"].as_f64().unwrap())
            .collect();
        assert_eq!(points, expected, "{slug}: {body}");
        let grouped: Vec<String> = queries
            .iter()
            .map(|q| q.sql.clone())
            .filter(|sql| sql.contains("deals"))
            .collect();
        assert_eq!(grouped.len(), 1, "{slug}: {grouped:?}");
        assert!(grouped[0].contains("GROUP BY"), "{slug}: {grouped:?}");
    }
}

struct GroupedTrend;

impl Injectable for GroupedTrend {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(GroupedTrend) })
    }
}

impl Resource for GroupedTrend {
    type Model = Deal;

    const LABEL: &'static str = "Grouped";
    const SLUG: &'static str = "grouped-trend";
    const TITLE: &'static str = "title";
    const SIDEBAR_COUNT: bool = false;

    fn fields(&self) -> Vec<Field> {
        vec![Field::text("title")]
    }

    fn metrics(&self) -> Vec<Metric<Deal>> {
        vec![
            Metric::trend("Count by day").count().by_day(Deal::CREATED_AT, 7),
            Metric::trend("Sum by day")
                .sum(Deal::AMOUNT_CENTS)
                .by_day(Deal::CREATED_AT, 7),
            Metric::trend("Average by day")
                .average(Deal::AMOUNT_CENTS)
                .by_day(Deal::CREATED_AT, 7),
            Metric::trend("Largest by day")
                .max(Deal::AMOUNT_CENTS)
                .by_day(Deal::CREATED_AT, 7),
        ]
    }
}

struct GroupedTrendModule;

impl Module for GroupedTrendModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.resource::<GroupedTrend>();
    }
}
