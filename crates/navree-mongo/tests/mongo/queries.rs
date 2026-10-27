//! Queries on MongoDB: every condition, orders, pages, values,
//! aggregates, groups, distinct values, subqueries, joins, unions, mass
//! writes, time buckets, full text, workspace scopes and refusals.

use bson::doc;
use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};
use navree_mongo::{MongoDb, MongoFilter, ObjectId, ToMongo};
use navree_orm::ir::{FullText, Operator, Order};
use navree_orm::{Decimal, Expr, Filter, Model, OrmError, Value, col, table};

use crate::models::{Company, Deal, DealMeta, Note, Reading, Stage, WORKSPACE};
use crate::support::TestMongo;

/// Companies Acme (NO, 12 people), Globex (US, 250) and Initech (US, no
/// deals); deals across stages, amounts, tags and dates.
async fn seed(mongo: &MongoDb) -> (Vec<Company>, Vec<Deal>) {
    let mut companies = Vec::new();
    for (name, country, employees) in [
        ("Acme", Some("NO"), 12),
        ("Globex", Some("US"), 250),
        ("Initech", None, 40),
    ] {
        let company = Company::create(
            mongo,
            Company {
                name: name.into(),
                country: country.map(str::to_owned),
                employees,
                ..Company::default()
            },
        )
        .await
        .unwrap();
        companies.push(company.into_inner());
    }
    let (acme, globex) = (companies[0].id, companies[1].id);
    let mut deals = Vec::new();
    for (title, company, amount, stage, tags, score, closes) in [
        ("Renewal", acme, 120_000, Stage::Won, vec!["renewal", "vip"], 9, (2026, 3, 15)),
        ("Expansion", acme, 45_000, Stage::Qualified, vec!["upsell"], 5, (2026, 6, 1)),
        ("Pilot", globex, 8_000, Stage::Lead, vec![], 2, (2026, 6, 30)),
        ("Enterprise plan", globex, 900_000, Stage::Won, vec!["vip"], 10, (2026, 12, 31)),
        ("ACME support", acme, 15_000, Stage::Lost, vec!["support"], 1, (2026, 1, 10)),
    ] {
        let deal = Deal::create(
            mongo,
            Deal {
                company_id: Some(company),
                price: Decimal::new(amount * 100 + 99, 2),
                stage,
                meta: Some(DealMeta {
                    source: if amount > 100_000 { "sales" } else { "web" }.into(),
                    score,
                    labels: Vec::new(),
                }),
                tags: tags.into_iter().map(str::to_owned).collect(),
                closes_on: NaiveDate::from_ymd_opt(closes.0, closes.1, closes.2),
                ..Deal::new(title, amount)
            },
        )
        .await
        .unwrap();
        deals.push(deal.into_inner());
    }
    (companies, deals)
}

async fn titles(mongo: &MongoDb, query: navree_orm::Builder<Deal>) -> Vec<String> {
    query.order_by(Deal::TITLE).pluck(mongo, Deal::TITLE).await.unwrap()
}

#[tokio::test]
async fn conditions_become_filter_documents() {
    let Some(t) = TestMongo::connect("conditions_become_filter_documents").await else {
        return;
    };
    let mongo = &t.db;
    let (companies, _) = seed(mongo).await;
    let q = Deal::query;
    assert_eq!(titles(mongo, q().where_(Deal::STAGE.eq(Stage::Won))).await, ["Enterprise plan", "Renewal"]);
    assert_eq!(titles(mongo, q().where_(Deal::AMOUNT.gte(45_000))).await.len(), 3);
    assert_eq!(titles(mongo, q().where_(Deal::AMOUNT.lt(10_000))).await, ["Pilot"]);
    assert_eq!(
        titles(mongo, q().where_in(Deal::STAGE, [Stage::Lead, Stage::Lost])).await,
        ["ACME support", "Pilot"]
    );
    assert_eq!(titles(mongo, q().where_not_in(Deal::STAGE, [Stage::Won, Stage::Lead])).await.len(), 2);
    assert_eq!(
        titles(mongo, q().where_between(Deal::AMOUNT, 10_000, 50_000)).await,
        ["ACME support", "Expansion"]
    );
    // LIKE ignores case on every database.
    assert_eq!(titles(mongo, q().where_like(Deal::TITLE, "acme%")).await, ["ACME support"]);
    assert_eq!(titles(mongo, q().where_like(Deal::TITLE, "%PLAN")).await, ["Enterprise plan"]);
    assert!(titles(mongo, q().where_like_case_sensitive(Deal::TITLE, "acme%")).await.is_empty());
    assert_eq!(titles(mongo, q().where_not_like(Deal::TITLE, "%e%")).await, ["Pilot"]);
    // Paths into embedded documents and arrays.
    assert_eq!(titles(mongo, q().where_(col("meta->source").eq("sales"))).await, ["Enterprise plan", "Renewal"]);
    assert_eq!(titles(mongo, q().where_(col("meta->score").gt(8))).await, ["Enterprise plan", "Renewal"]);
    assert_eq!(titles(mongo, q().where_json_contains("tags", "vip")).await, ["Enterprise plan", "Renewal"]);
    assert_eq!(titles(mongo, q().where_json_contains("tags", ["renewal", "vip"])).await, ["Renewal"]);
    assert_eq!(titles(mongo, q().where_json_length("tags", Operator::Eq, 0)).await, ["Pilot"]);
    assert_eq!(titles(mongo, q().where_json_length("tags", Operator::Gt, 1)).await, ["Renewal"]);
    assert_eq!(titles(mongo, q().where_(col("meta").op("?", "labels"))).await.len(), 5);
    // Dates and their parts (UTC).
    assert_eq!(
        titles(mongo, q().where_date(Deal::CLOSES_ON, "=", NaiveDate::from_ymd_opt(2026, 6, 1).unwrap())).await,
        ["Expansion"]
    );
    assert_eq!(titles(mongo, q().where_month(Deal::CLOSES_ON, "=", 6)).await, ["Expansion", "Pilot"]);
    assert_eq!(titles(mongo, q().where_year(Deal::CLOSES_ON, ">=", 2026)).await.len(), 5);
    assert_eq!(titles(mongo, q().where_day(Deal::CLOSES_ON, "<", 2)).await, ["Expansion"]);
    // Nulls, other columns, groups, negation, ObjectIds.
    assert_eq!(
        Company::query()
            .where_null(Company::COUNTRY)
            .pluck(mongo, Company::NAME)
            .await
            .unwrap(),
        ["Initech"]
    );
    assert_eq!(
        Company::query()
            .where_column(Company::EMPLOYEES, ">", "employees")
            .count(mongo)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        titles(
            mongo,
            q().where_(Deal::STAGE.eq(Stage::Won))
                .where_group(|g| g.where_(Deal::AMOUNT.lt(200_000)).or_where(Deal::TITLE.eq("Pilot")))
        )
        .await,
        ["Renewal"]
    );
    assert_eq!(titles(mongo, q().where_not(Deal::STAGE.eq(Stage::Won))).await.len(), 3);
    assert_eq!(titles(mongo, q().where_(Deal::COMPANY_ID.eq(companies[1].id))).await, ["Enterprise plan", "Pilot"]);
    assert_eq!(
        titles(mongo, q().where_(Deal::TITLE.op("regexp", "^E"))).await,
        ["Enterprise plan", "Expansion"]
    );
    assert_eq!(
        titles(mongo, q().where_(Deal::PRICE.gt(Decimal::new(119_999_99, 2)))).await,
        ["Enterprise plan", "Renewal"]
    );
    // A deal without a date or tags has no month and no length: no
    // comparison holds for it, as in SQL.
    Deal::create(mongo, Deal { tags: Vec::new(), ..Deal::new("Undated", 1) }).await.unwrap();
    t.db.collection("deals")
        .update_one(doc! { "title": "Undated" }, doc! { "$unset": { "tags": "", "closes_on": "" } })
        .await
        .unwrap();
    assert_eq!(titles(mongo, q().where_month(Deal::CLOSES_ON, "<", 6)).await, ["ACME support", "Renewal"]);
    assert_eq!(titles(mongo, q().where_json_length("tags", Operator::Lt, 1)).await, ["Pilot"]);
    assert_eq!(
        titles(mongo, q().where_column("meta->score", "<", "amount")).await.len(),
        5,
        "the undated deal has no score, which is less than nothing"
    );
    t.cleanup().await;
}

#[tokio::test]
async fn orders_values_and_aggregates() {
    let Some(t) = TestMongo::connect("orders_values_and_aggregates").await else {
        return;
    };
    let mongo = &t.db;
    seed(mongo).await;
    let q = Deal::query;
    let by_amount: Vec<i64> = q().order_by_desc(Deal::AMOUNT).limit(3).pluck(mongo, Deal::AMOUNT).await.unwrap();
    assert_eq!(by_amount, [900_000, 120_000, 45_000]);
    let page: Vec<String> = q().order_by(Deal::AMOUNT).for_page(2, 2).pluck(mongo, Deal::TITLE).await.unwrap();
    assert_eq!(page, ["Expansion", "Renewal"]);
    let newest = q().latest().first(mongo).await.unwrap().unwrap();
    assert_eq!(newest.title, "ACME support", "created last");
    assert_eq!(
        q().where_(Deal::TITLE.eq("Pilot")).value(mongo, Deal::AMOUNT).await.unwrap(),
        Some(8_000)
    );
    let keyed = q().pluck_keyed(mongo, Deal::AMOUNT, Deal::TITLE).await.unwrap();
    assert_eq!(keyed["Renewal"], 120_000);
    let error = q().where_(Deal::STAGE.eq(Stage::Won)).sole(mongo).await.unwrap_err();
    assert!(matches!(error, OrmError::MultipleRecordsFound { .. }), "{error:?}");
    assert!(q().where_(Deal::AMOUNT.gt(1_000_000)).doesnt_exist(mongo).await.unwrap());
    assert!(q().where_(Deal::STAGE.eq(Stage::Lost)).exists(mongo).await.unwrap());
    assert_eq!(q().limit(0).count(mongo).await.unwrap(), 0);
    assert!(q().limit(0).get(mongo).await.unwrap().is_empty());

    assert_eq!(q().count(mongo).await.unwrap(), 5);
    assert_eq!(q().where_(Deal::STAGE.eq(Stage::Won)).count(mongo).await.unwrap(), 2);
    assert_eq!(q().limit(2).count(mongo).await.unwrap(), 2, "a limited query counts what it returns");
    assert_eq!(q().count_column(mongo, Deal::CLOSES_ON).await.unwrap(), 5);
    assert_eq!(q().count_distinct(mongo, Deal::STAGE).await.unwrap(), 4);
    assert_eq!(q().sum::<i64>(mongo, Deal::AMOUNT).await.unwrap(), 1_088_000);
    assert_eq!(q().where_(Deal::AMOUNT.gt(1_000_000)).sum::<i64>(mongo, Deal::AMOUNT).await.unwrap(), 0);
    assert_eq!(q().avg::<f64>(mongo, Deal::AMOUNT).await.unwrap(), Some(217_600.0));
    assert_eq!(q().min::<i64>(mongo, Deal::AMOUNT).await.unwrap(), Some(8_000));
    assert_eq!(q().max::<Decimal>(mongo, Deal::PRICE).await.unwrap(), Some(Decimal::new(90_000_099, 2)));
    assert_eq!(q().where_(Deal::AMOUNT.gt(1_000_000)).max::<i64>(mongo, Deal::AMOUNT).await.unwrap(), None);
    let shuffled = q().in_random_order().pluck(mongo, Deal::TITLE).await.unwrap();
    assert_eq!(shuffled.len(), 5);
    // NULLs last ascending, which MongoDB does not do on its own.
    let countries: Vec<Option<String>> = Company::query()
        .order(Order::asc(Company::COUNTRY.expr()).nulls_last())
        .pluck(mongo, Company::COUNTRY)
        .await
        .unwrap();
    assert_eq!(countries, [Some("NO".into()), Some("US".into()), None]);
    t.cleanup().await;
}

#[tokio::test]
async fn pages_come_in_navrees_shapes() {
    let Some(t) = TestMongo::connect("pages_come_in_navrees_shapes").await else {
        return;
    };
    let mongo = &t.db;
    seed(mongo).await;
    let page = Deal::query().order_by(Deal::AMOUNT).paginate(mongo, (2, 2)).await.unwrap();
    assert_eq!((page.meta.total, page.meta.last_page, page.meta.current_page), (5, 3, 2));
    assert_eq!(page.data.iter().map(|d| d.title.as_str()).collect::<Vec<_>>(), ["Expansion", "Renewal"]);
    let simple = Deal::query().order_by(Deal::AMOUNT).simple_paginate(mongo, (3, 2)).await.unwrap();
    assert!(!simple.meta.has_more);
    assert_eq!(simple.data.len(), 1);

    // Cursors over ObjectId keys (the default order) keep their type.
    let first = Deal::query().cursor_paginate(mongo, navree_orm::CursorRequest::first().limit(2)).await.unwrap();
    assert_eq!(first.data.len(), 2);
    let next = first.next_cursor.clone().expect("a next page");
    let second = Deal::query()
        .cursor_paginate(mongo, navree_orm::CursorRequest::after(next).limit(2))
        .await
        .unwrap();
    assert_eq!(second.data.len(), 2);
    assert!(second.data[0].id > first.data[1].id);
    let back = Deal::query()
        .cursor_paginate(mongo, navree_orm::CursorRequest::after(second.prev_cursor.clone().unwrap()).limit(2))
        .await
        .unwrap();
    assert_eq!(
        back.data.iter().map(|d| d.id).collect::<Vec<_>>(),
        first.data.iter().map(|d| d.id).collect::<Vec<_>>()
    );
    // And over dates, newest first.
    let newest = Deal::query().latest().cursor_paginate(mongo, navree_orm::CursorRequest::first().limit(3)).await.unwrap();
    let older = Deal::query()
        .latest()
        .cursor_paginate(mongo, navree_orm::CursorRequest::after(newest.next_cursor.unwrap()).limit(3))
        .await
        .unwrap();
    assert_eq!(newest.data.len() + older.data.len(), 5);
    assert!(older.next_cursor.is_none());
    t.cleanup().await;
}

#[tokio::test]
async fn groups_having_and_distinct_values() {
    let Some(t) = TestMongo::connect("groups_having_and_distinct_values").await else {
        return;
    };
    let mongo = &t.db;
    seed(mongo).await;
    let rows = table("deals")
        .select(["stage"])
        .select_expr(Expr::count_all(), "deals")
        .select_expr(Expr::sum(Expr::col("amount")), "total")
        .group_by(["stage"])
        .having(Expr::count_all().gt(1))
        .order_by_desc("total")
        .get(mongo)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get::<String>("stage").unwrap(), "won");
    assert_eq!(rows[0].get::<i64>("deals").unwrap(), 2);
    assert_eq!(rows[0].get::<i64>("total").unwrap(), 1_020_000);
    assert_eq!(rows[0].columns(), ["stage", "deals", "total"]);

    // Grouped counts count the groups.
    assert_eq!(table("deals").group_by(["company_id"]).count(mongo).await.unwrap(), 2);

    let stages: Vec<Value> = table("deals").distinct().order_by("stage").pluck(mongo, "stage").await.unwrap();
    assert_eq!(stages.len(), 4);
    // DISTINCT ON: the first deal of each company by amount.
    let biggest = Deal::query()
        .distinct_on([Deal::COMPANY_ID])
        .order_by(Deal::COMPANY_ID)
        .order_by_desc(Deal::AMOUNT)
        .pluck(mongo, Deal::TITLE)
        .await
        .unwrap();
    assert_eq!(biggest.len(), 2);
    assert!(biggest.contains(&"Renewal".to_owned()) && biggest.contains(&"Enterprise plan".to_owned()));
    t.cleanup().await;
}

#[tokio::test]
async fn subqueries_joins_and_unions_use_lookups() {
    let Some(t) = TestMongo::connect("subqueries_joins_and_unions_use_lookups").await else {
        return;
    };
    let mongo = &t.db;
    seed(mongo).await;
    // Correlated EXISTS: companies with a won deal.
    let with_won: Vec<String> = Company::query()
        .where_exists(
            Deal::query()
                .where_column(Deal::COMPANY_ID, "=", Company::ID)
                .where_(Deal::STAGE.eq(Stage::Won)),
        )
        .order_by(Company::NAME)
        .pluck(mongo, Company::NAME)
        .await
        .unwrap();
    assert_eq!(with_won, ["Acme", "Globex"]);
    let without: Vec<String> = Company::query()
        .where_not_exists(Deal::query().where_column(Deal::COMPANY_ID, "=", Company::ID))
        .pluck(mongo, Company::NAME)
        .await
        .unwrap();
    assert_eq!(without, ["Initech"]);
    // IN (subquery): deals of Norwegian companies.
    let norwegian = titles(
        mongo,
        Deal::query().where_in_sub(
            Deal::COMPANY_ID,
            Company::query().select([Company::ID]).where_(Company::COUNTRY.eq("NO")),
        ),
    )
    .await;
    assert_eq!(norwegian, ["ACME support", "Expansion", "Renewal"]);
    // A subquery per row: each company's deal count.
    let counts = Company::query()
        .select_sub(
            Deal::query()
                .select([(Expr::count_all(), "n")])
                .where_column(Deal::COMPANY_ID, "=", Company::ID),
            "deals_count",
        )
        .order_by(Company::NAME)
        .get(mongo)
        .await
        .unwrap();
    let counted: Vec<(String, i64)> = counts
        .iter()
        .map(|c| (c.name.clone(), c.extra::<i64>("deals_count").unwrap().unwrap()))
        .collect();
    assert_eq!(counted, [("Acme".into(), 3), ("Globex".into(), 2), ("Initech".into(), 0)]);
    // Joins.
    let joined = table("deals")
        .join("companies", "companies._id", "=", "deals.company_id")
        .select(["deals.title", "companies.name"])
        .where_(col("companies.country").eq("US"))
        .order_by("deals.title")
        .get(mongo)
        .await
        .unwrap();
    let pairs: Vec<(String, String)> = joined
        .iter()
        .map(|row| (row.get("title").unwrap(), row.get("name").unwrap()))
        .collect();
    assert_eq!(
        pairs,
        [("Enterprise plan".into(), "Globex".into()), ("Pilot".into(), "Globex".into())]
    );
    let left = table("companies")
        .left_join("deals", "deals.company_id", "=", "companies._id")
        .where_null("deals._id")
        .pluck(mongo, "companies.name")
        .await
        .unwrap();
    assert_eq!(left, [Value::from("Initech")]);
    // NULL keys join nothing, as in SQL (MongoDB would match Initech's
    // null country with itself).
    let unmatched = table("companies")
        .left_join("companies as twins", "twins.country", "=", "companies.country")
        .where_null("twins._id")
        .pluck(mongo, "companies.name")
        .await
        .unwrap();
    assert_eq!(unmatched, [Value::from("Initech")]);
    // Unions: the columns are named by the first query's, by position.
    let names: Vec<String> = table("companies")
        .select(["name"])
        .where_(col("country").eq("NO"))
        .union_all(table("deals").select(["title"]).where_(col("amount").lt(10_000)))
        .union(table("companies").select(["name"]).where_(col("country").eq("NO")))
        .order_by_desc("name")
        .get(mongo)
        .await
        .unwrap()
        .iter()
        .map(|row| row.get("name").unwrap())
        .collect();
    assert_eq!(names, ["Pilot", "Acme"], "UNION removes the repeated Acme");
    t.cleanup().await;
}

#[tokio::test]
async fn mass_writes_update_and_delete_in_one_command() {
    let Some(t) = TestMongo::connect("mass_writes_update_and_delete_in_one_command").await else {
        return;
    };
    let mongo = &t.db;
    seed(mongo).await;
    let changed = Deal::query()
        .where_(Deal::STAGE.eq(Stage::Won))
        .update(mongo, [Deal::TAGS.set(vec!["closed".to_owned()])])
        .await
        .unwrap();
    assert_eq!(changed, 2);
    // A limited write touches the first rows only (their keys found first).
    let capped = Deal::query()
        .order_by(Deal::AMOUNT)
        .limit(2)
        .update(mongo, [Deal::STAGE.set(Stage::Lost)])
        .await
        .unwrap();
    assert_eq!(capped, 2);
    assert_eq!(
        titles(mongo, Deal::query().where_(Deal::STAGE.eq(Stage::Lost))).await,
        ["ACME support", "Pilot"]
    );
    let bumped = Deal::query()
        .where_(Deal::AMOUNT.lt(20_000))
        .increment(mongo, Deal::AMOUNT, 1_000)
        .await
        .unwrap();
    assert_eq!(bumped, 2);
    assert_eq!(
        Deal::query().where_(Deal::TITLE.eq("Pilot")).value(mongo, Deal::AMOUNT).await.unwrap(),
        Some(9_000)
    );
    // Soft-deleting models mark rows; table writes remove them.
    let trashed = Deal::query().where_(Deal::STAGE.eq(Stage::Lost)).delete(mongo).await.unwrap();
    assert_eq!(trashed, 2);
    assert_eq!(Deal::query().count(mongo).await.unwrap(), 3);
    assert_eq!(Deal::query().with_trashed().count(mongo).await.unwrap(), 5);
    let removed = table("deals").where_null("meta").delete(mongo).await.unwrap();
    assert_eq!(removed, 0);
    let inserted = table("audit")
        .insert(
            mongo,
            [
                [("action", Value::from("login")), ("at", Value::from(Utc::now()))],
                [("action", Value::from("logout")), ("at", Value::from(Utc::now()))],
            ],
        )
        .await
        .unwrap();
    assert_eq!(inserted, 2);
    assert_eq!(table("audit").count(mongo).await.unwrap(), 2);
    t.cleanup().await;
}

#[tokio::test]
async fn time_buckets_and_first_and_last() {
    let Some(t) = TestMongo::connect("time_buckets_and_first_and_last").await else {
        return;
    };
    let mongo = &t.db;
    let start: DateTime<Utc> = Utc.with_ymd_and_hms(2026, 10, 27, 8, 0, 0).unwrap();
    for (minutes, celsius) in [(5, 10.0), (20, 12.0), (55, 14.0), (65, 20.0), (110, 22.0)] {
        Reading::create(
            mongo,
            Reading {
                sensor: "roof".into(),
                celsius,
                taken_at: start + Duration::minutes(minutes),
                ..Reading::default()
            },
        )
        .await
        .unwrap();
    }
    let hours = table("readings")
        .select_expr(Expr::time_bucket("1 hour", Expr::col("taken_at")), "bucket")
        .select_expr(Expr::avg(Expr::col("celsius")), "average")
        .select_expr(Expr::first(Expr::col("celsius"), Expr::col("taken_at")), "opening")
        .select_expr(Expr::last(Expr::col("celsius"), Expr::col("taken_at")), "closing")
        .group_by(["bucket"])
        .order_by("bucket")
        .get(mongo)
        .await
        .unwrap();
    assert_eq!(hours.len(), 2);
    assert_eq!(hours[0].get::<DateTime<Utc>>("bucket").unwrap(), start);
    assert_eq!(hours[0].get::<f64>("average").unwrap(), 12.0);
    assert_eq!(hours[0].get::<f64>("opening").unwrap(), 10.0);
    assert_eq!(hours[0].get::<f64>("closing").unwrap(), 14.0);
    assert_eq!(hours[1].get::<DateTime<Utc>>("bucket").unwrap(), start + Duration::hours(1));
    assert_eq!(hours[1].get::<f64>("closing").unwrap(), 22.0);
    // Weekly buckets start on Mondays, as on TimescaleDB.
    let weeks = table("readings")
        .select_time_bucket("1 week", "taken_at", "week")
        .distinct()
        .get(mongo)
        .await
        .unwrap();
    assert_eq!(weeks.len(), 1);
    assert_eq!(
        weeks[0].get::<DateTime<Utc>>("week").unwrap(),
        Utc.with_ymd_and_hms(2026, 10, 26, 0, 0, 0).unwrap()
    );
    t.cleanup().await;
}

#[tokio::test]
async fn full_text_runs_on_text_indexes() {
    let Some(t) = TestMongo::connect("full_text_runs_on_text_indexes").await else {
        return;
    };
    let mongo = &t.db;
    mongo
        .schema()
        .create("deals", |t| {
            t.object_id_primary();
            t.string("title");
            t.fulltext(["title"]);
            t.without_validator();
        })
        .await
        .unwrap();
    seed(mongo).await;
    let found = titles(mongo, Deal::query().where_full_text([Deal::TITLE.expr()], "enterprise")).await;
    assert_eq!(found, ["Enterprise plan"]);
    let phrase = titles(
        mongo,
        Deal::query().where_full_text_with([Deal::TITLE.expr()], "ACME support", FullText::phrase()),
    )
    .await;
    assert_eq!(phrase, ["ACME support"]);
    let error = Deal::query()
        .where_(Deal::STAGE.eq(Stage::Won))
        .or_where_full_text([Deal::TITLE.expr()], "plan")
        .get(mongo)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("joined with AND"), "{error}");
    t.cleanup().await;
}

#[tokio::test]
async fn native_filters_and_pipelines() {
    let Some(t) = TestMongo::connect("native_filters_and_pipelines").await else {
        return;
    };
    let mongo = &t.db;
    seed(mongo).await;
    let vip = titles(
        mongo,
        Deal::query().where_(Filter::document(doc! { "tags": { "$elemMatch": { "$eq": "vip" } } })),
    )
    .await;
    assert_eq!(vip, ["Enterprise plan", "Renewal"]);
    // Stages after the query's own: each deal with its company.
    use navree_mongo::MongoQuery;
    let deals = Deal::query()
        .where_(Deal::STAGE.eq(Stage::Won))
        .order_by(Deal::TITLE)
        .pipeline([
            doc! { "$lookup": { "from": "companies", "localField": "company_id", "foreignField": "_id", "as": "company" } },
            doc! { "$set": { "company_name": { "$first": "$company.name" } } },
            doc! { "$unset": "company" },
        ])
        .get(mongo)
        .await
        .unwrap();
    let names: Vec<(String, String)> = deals
        .iter()
        .map(|d| (d.title.clone(), d.extra::<String>("company_name").unwrap().unwrap()))
        .collect();
    assert_eq!(names, [("Enterprise plan".into(), "Globex".into()), ("Renewal".into(), "Acme".into())]);
    // Shown without running.
    let command = Deal::query().where_(Deal::AMOUNT.gt(10)).limit(3).to_mongo().unwrap();
    assert!(command.to_string().starts_with("db.deals.find({"), "{command}");
    t.cleanup().await;
}

#[tokio::test]
async fn global_scopes_keep_workspaces_apart() {
    let Some(t) = TestMongo::connect("global_scopes_keep_workspaces_apart").await else {
        return;
    };
    let mongo = &t.db;
    let (one, two) = (ObjectId::new(), ObjectId::new());
    for (workspace, body) in [(one, "a"), (one, "b"), (two, "c")] {
        Note::create(mongo, Note { workspace_id: workspace, body: body.into(), ..Note::default() })
            .await
            .unwrap();
    }
    let in_one = WORKSPACE
        .scope(one, async { Note::query().order_by(Note::BODY).pluck(mongo, Note::BODY).await.unwrap() })
        .await;
    assert_eq!(in_one, ["a", "b"]);
    let in_two = WORKSPACE.scope(two, async { Note::query().count(mongo).await.unwrap() }).await;
    assert_eq!(in_two, 1);
    let everywhere = WORKSPACE
        .scope(two, async { Note::query().without_global_scopes().count(mongo).await.unwrap() })
        .await;
    assert_eq!(everywhere, 3);
    t.cleanup().await;
}

#[tokio::test]
async fn what_mongodb_lacks_is_refused_by_name() {
    let Some(t) = TestMongo::connect("what_mongodb_lacks_is_refused_by_name").await else {
        return;
    };
    let mongo = &t.db;
    let refused = |error: OrmError, needle: &str| {
        assert!(matches!(error, OrmError::Unsupported { .. }), "{error:?}");
        assert!(error.to_string().contains(needle), "{error}");
    };
    refused(Deal::query().where_raw("amount > ?", [5]).get(mongo).await.unwrap_err(), "where_raw");
    refused(Deal::query().lock_for_update().get(mongo).await.unwrap_err(), "row locks");
    refused(
        table("deals").right_join("companies", "companies._id", "=", "deals.company_id").get(mongo).await.unwrap_err(),
        "RIGHT JOIN",
    );
    refused(Deal::query().order_by_raw("amount DESC", Vec::<Value>::new()).get(mongo).await.unwrap_err(), "raw SQL orders");
    t.cleanup().await;
}
