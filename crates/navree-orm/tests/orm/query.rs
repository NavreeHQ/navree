//! The query builder against SQLite: every where variant, selects, joins,
//! groups, orders, limits, unions, aggregates, values and plucks.

use chrono::{NaiveDate, NaiveTime, TimeZone, Utc};
use navree_orm::{Collection, Expr, Model, Value, col, col_as, table};

use crate::models::{Company, Deal, DealMeta, Stage};
use crate::support::{TestDb, sqlite};

/// Four companies and five deals.
async fn seeded() -> TestDb {
    let t = sqlite().await;
    for (name, country, active, employees) in [
        ("Acme", Some("US"), true, 120),
        ("Globex", Some("DE"), false, 40),
        ("Initech", Some("US"), true, 15),
        ("Umbrella", None, true, 300),
    ] {
        Company::create(
            &t.db,
            Company {
                name: name.into(),
                country: country.map(Into::into),
                active,
                employees,
                ..Company::default()
            },
        )
        .await
        .unwrap();
    }
    let day = |d: u32| Utc.with_ymd_and_hms(2026, 3, d, 14, 30, 0).unwrap();
    for (title, company, cents, stage, tags, closes, closed) in [
        (
            "Alpha",
            Some(1),
            5_000,
            Stage::Lead,
            vec!["vip", "q1"],
            Some(1),
            None,
        ),
        (
            "Beta",
            Some(1),
            25_000,
            Stage::Qualified,
            vec!["q1"],
            Some(15),
            None,
        ),
        (
            "Gamma",
            Some(2),
            90_000,
            Stage::ClosedWon,
            vec![],
            None,
            Some(day(20)),
        ),
        (
            "Delta",
            Some(3),
            50,
            Stage::ClosedLost,
            vec!["vip"],
            None,
            Some(day(21)),
        ),
        ("Epsilon", None, 12_000, Stage::Lead, vec![], Some(31), None),
    ] {
        Deal::create(
            &t.db,
            Deal {
                company_id: company,
                stage,
                tags: tags.into_iter().map(Into::into).collect(),
                closes_on: closes.map(|d| NaiveDate::from_ymd_opt(2026, 3, d).unwrap()),
                closed_at: closed,
                meta: DealMeta {
                    source: if cents > 20_000 {
                        "ads".into()
                    } else {
                        "web".into()
                    },
                    score: u32::try_from(cents / 1000).unwrap(),
                },
                ..Deal::new(title, cents)
            },
        )
        .await
        .unwrap();
    }
    t
}

fn titles(deals: &Collection<Deal>) -> Vec<String> {
    let mut titles = deals.map(|deal| deal.title.clone());
    titles.sort();
    titles
}

#[tokio::test]
async fn comparisons_and_boolean_groups() {
    let t = seeded().await;
    let db = &t.db;
    let big = Deal::query()
        .where_(Deal::AMOUNT_CENTS.gte(25_000))
        .get(db)
        .await
        .unwrap();
    assert_eq!(titles(&big), ["Beta", "Gamma"]);

    let either = Deal::query()
        .where_(Deal::STAGE.eq(Stage::ClosedWon))
        .or_where(Deal::TITLE.eq("Alpha"))
        .get(db)
        .await
        .unwrap();
    assert_eq!(titles(&either), ["Alpha", "Gamma"]);

    // `a AND (b OR c)`.
    let grouped = Deal::query()
        .where_(Deal::AMOUNT_CENTS.gt(1_000))
        .where_group(|g| {
            g.where_(Deal::STAGE.eq(Stage::Lead))
                .or_where(Deal::STAGE.eq(Stage::ClosedWon))
        })
        .get(db)
        .await
        .unwrap();
    assert_eq!(titles(&grouped), ["Alpha", "Epsilon", "Gamma"]);

    let not = Deal::query()
        .where_not(Deal::STAGE.eq(Stage::Lead))
        .where_(Deal::AMOUNT_CENTS.ne(50))
        .get(db)
        .await
        .unwrap();
    assert_eq!(titles(&not), ["Beta", "Gamma"]);

    let combined = Deal::query()
        .where_(
            Deal::AMOUNT_CENTS
                .lt(10_000)
                .and(Deal::STAGE.eq(Stage::Lead))
                .or(Deal::TITLE.eq("Gamma")),
        )
        .get(db)
        .await
        .unwrap();
    assert_eq!(titles(&combined), ["Alpha", "Gamma"]);
}

#[tokio::test]
async fn lists_nulls_ranges_and_columns() {
    let t = seeded().await;
    let db = &t.db;
    let listed = Deal::query()
        .where_in(Deal::STAGE, [Stage::Lead, Stage::ClosedLost])
        .get(db)
        .await
        .unwrap();
    assert_eq!(titles(&listed), ["Alpha", "Delta", "Epsilon"]);
    assert!(
        Deal::query()
            .where_in(Deal::STAGE, Vec::<Stage>::new())
            .get(db)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        Deal::query()
            .where_not_in(Deal::STAGE, Vec::<Stage>::new())
            .count(db)
            .await
            .unwrap(),
        5
    );

    let unowned = Deal::query()
        .where_null(Deal::COMPANY_ID)
        .get(db)
        .await
        .unwrap();
    assert_eq!(titles(&unowned), ["Epsilon"]);
    assert_eq!(
        Deal::query()
            .where_not_null(Deal::CLOSED_AT)
            .count(db)
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        Deal::query()
            .where_(Deal::CLOSED_AT.eq(None))
            .count(db)
            .await
            .unwrap(),
        3
    );

    let mid = Deal::query()
        .where_between(Deal::AMOUNT_CENTS, 5_000, 25_000)
        .get(db)
        .await
        .unwrap();
    assert_eq!(titles(&mid), ["Alpha", "Beta", "Epsilon"]);
    assert_eq!(
        Deal::query()
            .where_not_between(Deal::AMOUNT_CENTS, 5_000, 25_000)
            .count(db)
            .await
            .unwrap(),
        2
    );

    let touched = Deal::query()
        .where_column(Deal::UPDATED_AT, ">=", Deal::CREATED_AT)
        .count(db)
        .await
        .unwrap();
    assert_eq!(touched, 5);

    let by_untyped = Deal::query()
        .where_in("title", [Value::from("Beta"), Value::from("Nope")])
        .get(db)
        .await
        .unwrap();
    assert_eq!(titles(&by_untyped), ["Beta"]);
}

#[tokio::test]
async fn dates_and_times() {
    let t = seeded().await;
    let db = &t.db;
    let day = NaiveDate::from_ymd_opt(2026, 3, 20).unwrap();
    let closed_that_day = Deal::query()
        .where_date(Deal::CLOSED_AT, "=", day)
        .get(db)
        .await
        .unwrap();
    assert_eq!(titles(&closed_that_day), ["Gamma"]);
    assert_eq!(
        Deal::query()
            .where_date(Deal::CLOSED_AT, ">", day)
            .count(db)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        Deal::query()
            .where_month(Deal::CLOSES_ON, "=", 3)
            .count(db)
            .await
            .unwrap(),
        3
    );
    assert_eq!(
        Deal::query()
            .where_day(Deal::CLOSES_ON, "<", 15)
            .count(db)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        Deal::query()
            .where_year(Deal::CLOSED_AT, "=", 2026)
            .count(db)
            .await
            .unwrap(),
        2
    );
    let afternoon = NaiveTime::from_hms_opt(14, 30, 0).unwrap();
    assert_eq!(
        Deal::query()
            .where_time(Deal::CLOSED_AT, "=", afternoon)
            .count(db)
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        Deal::query()
            .where_(Deal::CLOSES_ON.gt(NaiveDate::from_ymd_opt(2026, 3, 10).unwrap()))
            .count(db)
            .await
            .unwrap(),
        2
    );
}

#[tokio::test]
async fn like_is_case_insensitive_unless_asked() {
    let t = seeded().await;
    let db = &t.db;
    assert_eq!(
        titles(
            &Deal::query()
                .where_like(Deal::TITLE, "%ALPHA%")
                .get(db)
                .await
                .unwrap()
        ),
        ["Alpha"]
    );
    assert_eq!(
        titles(
            &Deal::query()
                .where_(Deal::TITLE.like("e%"))
                .get(db)
                .await
                .unwrap()
        ),
        ["Epsilon"]
    );
    assert_eq!(
        Deal::query()
            .where_not_like(Deal::TITLE, "%a")
            .count(db)
            .await
            .unwrap(),
        1
    );
    assert!(
        Deal::query()
            .where_like_case_sensitive(Deal::TITLE, "alpha")
            .get(db)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        Deal::query()
            .where_like_case_sensitive(Deal::TITLE, "Al_ha")
            .count(db)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        Company::query()
            .where_(Company::COUNTRY.like("u%"))
            .count(db)
            .await
            .unwrap(),
        2
    );
}

#[tokio::test]
async fn json_columns_and_paths() {
    let t = seeded().await;
    let db = &t.db;
    let vip = Deal::query()
        .where_json_contains(Deal::TAGS, "vip")
        .get(db)
        .await
        .unwrap();
    assert_eq!(titles(&vip), ["Alpha", "Delta"]);
    assert_eq!(
        Deal::query()
            .where_json_doesnt_contain(Deal::TAGS, "vip")
            .count(db)
            .await
            .unwrap(),
        3
    );
    assert_eq!(
        Deal::query()
            .where_json_length(Deal::TAGS, ">", 1)
            .count(db)
            .await
            .unwrap(),
        1
    );
    let ads = Deal::query()
        .where_(col("meta->source").eq("ads"))
        .get(db)
        .await
        .unwrap();
    assert_eq!(titles(&ads), ["Beta", "Gamma"]);
    let scored = Deal::query()
        .where_(Deal::META.json("score").gte(25))
        .get(db)
        .await
        .unwrap();
    assert_eq!(titles(&scored), ["Beta", "Gamma"]);
}

#[tokio::test]
async fn subqueries_exists_and_raw() {
    let t = seeded().await;
    let db = &t.db;
    let active_ids = Company::query()
        .select([Company::ID])
        .where_(Company::ACTIVE.eq(true));
    let of_active = Deal::query()
        .where_in_sub(Deal::COMPANY_ID, active_ids)
        .get(db)
        .await
        .unwrap();
    assert_eq!(titles(&of_active), ["Alpha", "Beta", "Delta"]);

    let with_deals = Company::query()
        .where_exists(Deal::query().where_column(Deal::COMPANY_ID, "=", Company::ID))
        .get(db)
        .await
        .unwrap();
    assert_eq!(with_deals.len(), 3);
    let without = Company::query()
        .where_not_exists(Deal::query().where_column(Deal::COMPANY_ID, "=", Company::ID))
        .get(db)
        .await
        .unwrap();
    assert_eq!(without[0].name, "Umbrella");

    let above_average = Deal::query()
        .where_sub(
            Deal::AMOUNT_CENTS,
            ">",
            Deal::query().select([Expr::avg(Deal::AMOUNT_CENTS)]),
        )
        .get(db)
        .await
        .unwrap();
    assert_eq!(
        titles(&above_average),
        ["Gamma"],
        "the average is 26,410 cents"
    );

    let raw = Deal::query()
        .where_raw(
            "lower(title) = ? OR amount_cents = ?",
            [Value::from("beta"), Value::from(50_i64)],
        )
        .get(db)
        .await
        .unwrap();
    assert_eq!(titles(&raw), ["Beta", "Delta"]);
}

#[tokio::test]
async fn conditional_clauses() {
    let t = seeded().await;
    let db = &t.db;
    let only_leads = true;
    let min: Option<i64> = Some(10_000);
    let none: Option<i64> = None;
    let query = Deal::query()
        .when(only_leads, |q| q.where_(Deal::STAGE.eq(Stage::Lead)))
        .when_some(min, |q, min| q.where_(Deal::AMOUNT_CENTS.gte(min)))
        .when_some(none, |q, max| q.where_(Deal::AMOUNT_CENTS.lte(max)))
        .unless(true, |q| q.where_(Deal::TITLE.eq("never")));
    assert_eq!(titles(&query.get(db).await.unwrap()), ["Epsilon"]);
    let either = Deal::query().when_else(
        false,
        |q| q.where_(Deal::TITLE.eq("Alpha")),
        |q| q.where_(Deal::TITLE.eq("Beta")),
    );
    assert_eq!(titles(&either.get(db).await.unwrap()), ["Beta"]);
}

#[tokio::test]
async fn selects_and_extras() {
    let t = seeded().await;
    let db = &t.db;
    let deals = Deal::query()
        .select_raw("amount_cents * ? AS doubled", [2_i64])
        .add_select([(Deal::TITLE, "label")])
        .where_(Deal::TITLE.eq("Beta"))
        .get(db)
        .await
        .unwrap();
    let beta = &deals[0];
    assert_eq!(beta.title, "Beta", "the model's columns are still selected");
    assert_eq!(beta.extra::<i64>("doubled").unwrap(), Some(50_000));
    assert_eq!(
        beta.extra::<String>("label").unwrap().as_deref(),
        Some("Beta")
    );
    assert_eq!(beta.extra::<i64>("absent").unwrap(), None);
    let json = serde_json::to_value(beta).unwrap();
    assert_eq!(json["doubled"], 50_000);
    assert_eq!(json["label"], "Beta");

    let with_company = Deal::query()
        .select_sub(
            Company::query().select([Company::NAME]).where_column(
                Company::ID,
                "=",
                Deal::COMPANY_ID,
            ),
            "company_name",
        )
        .where_(Deal::TITLE.eq("Gamma"))
        .first(db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        with_company
            .extra::<String>("company_name")
            .unwrap()
            .as_deref(),
        Some("Globex")
    );

    // Selecting fewer columns than the model has fails with a precise error.
    let error = Deal::query().select([Deal::ID]).get(db).await.unwrap_err();
    assert!(
        error
            .to_string()
            .starts_with("cannot read column `company_id` of Deal"),
        "{error}"
    );
}

#[tokio::test]
async fn joins_groups_and_havings() {
    let t = seeded().await;
    let db = &t.db;
    let from_us = Deal::query()
        .join("companies", Deal::COMPANY_ID, "=", Company::ID)
        .where_(Company::COUNTRY.eq("US"))
        .get(db)
        .await
        .unwrap();
    assert_eq!(
        titles(&from_us),
        ["Alpha", "Beta", "Delta"],
        "joined columns stay out of the model"
    );

    let rows = table("companies as c")
        .left_join_on("deals as d", |j| {
            j.on("d.company_id", "=", "c.id")
                .where_(col("d.amount_cents").gt(1_000))
        })
        .select(["c.name"])
        .select_expr(Expr::count(col("d.id")), "deals")
        .group_by(["c.name"])
        .having(Expr::count(col("d.id")).gte(1))
        .order_by("c.name")
        .get(db)
        .await
        .unwrap();
    let summary: Vec<(String, i64)> = rows
        .iter()
        .map(|row| {
            (
                row.get::<String>("name").unwrap(),
                row.get::<i64>("deals").unwrap(),
            )
        })
        .collect();
    assert_eq!(summary, [("Acme".to_owned(), 2), ("Globex".to_owned(), 1)]);

    let stages = Deal::query()
        .select([Deal::STAGE])
        .distinct()
        .pluck(db, Deal::STAGE)
        .await
        .unwrap();
    assert_eq!(stages.len(), 4);
}

#[tokio::test]
async fn orders_limits_and_pages() {
    let t = seeded().await;
    let db = &t.db;
    let by_amount = Deal::query()
        .order_by_desc(Deal::AMOUNT_CENTS)
        .limit(2)
        .pluck(db, Deal::TITLE)
        .await
        .unwrap();
    assert_eq!(by_amount, ["Gamma", "Beta"]);
    let second_page = Deal::query()
        .order_by(Deal::TITLE)
        .for_page(2, 2)
        .pluck(db, Deal::TITLE)
        .await
        .unwrap();
    assert_eq!(second_page, ["Delta", "Epsilon"]);
    let skipped = Deal::query()
        .order_by(Deal::TITLE)
        .skip(4)
        .pluck(db, Deal::TITLE)
        .await
        .unwrap();
    assert_eq!(skipped, ["Gamma"]);
    let reordered = Deal::query()
        .order_by(Deal::TITLE)
        .reorder_by(Deal::AMOUNT_CENTS, navree_orm::ir::Direction::Asc)
        .take(1)
        .pluck(db, Deal::TITLE)
        .await
        .unwrap();
    assert_eq!(reordered, ["Delta"]);
    let latest = Deal::query().latest().first(db).await.unwrap().unwrap();
    assert_eq!(latest.title, "Epsilon");
    assert_eq!(
        Deal::query()
            .oldest()
            .first(db)
            .await
            .unwrap()
            .unwrap()
            .title,
        "Alpha"
    );
    assert_eq!(Deal::query().in_random_order().count(db).await.unwrap(), 5);
    let raw = Deal::query()
        .order_by_raw("amount_cents % ? DESC", [7_i64])
        .pluck(db, Deal::TITLE)
        .await
        .unwrap();
    assert_eq!(raw.len(), 5);
}

#[tokio::test]
async fn unions_combine_queries() {
    let t = seeded().await;
    let db = &t.db;
    let names = table("companies")
        .select(["name"])
        .where_(col("country").eq("DE"))
        .union(
            table("deals")
                .select(["title"])
                .where_(col("amount_cents").lt(100)),
        )
        .pluck(db, col_as::<String>("name"))
        .await
        .unwrap();
    let mut names = names;
    names.sort();
    assert_eq!(names, ["Delta", "Globex"]);
}

#[tokio::test]
async fn aggregates_values_and_plucks() {
    let t = seeded().await;
    let db = &t.db;
    assert_eq!(Deal::query().count(db).await.unwrap(), 5);
    assert_eq!(
        Deal::query()
            .sum::<i64>(db, Deal::AMOUNT_CENTS)
            .await
            .unwrap(),
        132_050
    );
    assert_eq!(
        Deal::query()
            .where_(Deal::TITLE.eq("none"))
            .sum::<i64>(db, Deal::AMOUNT_CENTS)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        Deal::query()
            .min::<i64>(db, Deal::AMOUNT_CENTS)
            .await
            .unwrap(),
        Some(50)
    );
    assert_eq!(
        Deal::query()
            .max::<i64>(db, Deal::AMOUNT_CENTS)
            .await
            .unwrap(),
        Some(90_000)
    );
    assert_eq!(
        Deal::query()
            .avg::<f64>(db, Deal::AMOUNT_CENTS)
            .await
            .unwrap(),
        Some(26_410.0)
    );
    assert_eq!(
        Deal::query()
            .where_(Deal::TITLE.eq("none"))
            .max::<i64>(db, Deal::AMOUNT_CENTS)
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        Deal::query()
            .count_column(db, Deal::CLOSED_AT)
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        Deal::query()
            .count_distinct(db, Deal::COMPANY_ID)
            .await
            .unwrap(),
        3
    );
    // A grouped query counts its groups.
    assert_eq!(
        Deal::query()
            .group_by([Deal::STAGE])
            .count(db)
            .await
            .unwrap(),
        4
    );

    let title: Option<String> = Deal::query()
        .where_(Deal::AMOUNT_CENTS.eq(50))
        .value(db, Deal::TITLE)
        .await
        .unwrap();
    assert_eq!(title.as_deref(), Some("Delta"));
    let missing: Option<String> = Deal::query()
        .where_(Deal::AMOUNT_CENTS.eq(1))
        .value(db, Deal::TITLE)
        .await
        .unwrap();
    assert!(missing.is_none());
    let stage = Deal::query()
        .where_(Deal::TITLE.eq("Gamma"))
        .value(db, Deal::STAGE)
        .await
        .unwrap();
    assert_eq!(
        stage,
        Some(Stage::ClosedWon),
        "values decode through the field's cast"
    );
    let closed = Deal::query()
        .order_by(Deal::TITLE)
        .pluck(db, Deal::CLOSED_AT)
        .await
        .unwrap();
    assert_eq!(
        closed.iter().filter(|at| at.is_some()).count(),
        2,
        "nullable columns pluck as options"
    );
    let keyed = Company::query()
        .order_by(Company::NAME)
        .pluck_keyed(db, Company::EMPLOYEES, Company::NAME)
        .await
        .unwrap();
    assert_eq!(keyed.get("Umbrella"), Some(&300));
    assert_eq!(keyed.keys().next().map(String::as_str), Some("Acme"));
    let untyped = Company::query()
        .where_key(1_i64)
        .value(db, "name")
        .await
        .unwrap();
    assert_eq!(untyped, Some(Value::from("Acme")));

    assert!(
        Deal::query()
            .where_(Deal::STAGE.eq(Stage::ClosedWon))
            .exists(db)
            .await
            .unwrap()
    );
    assert!(
        Deal::query()
            .where_(Deal::TITLE.eq("Zeta"))
            .doesnt_exist(db)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn keys_select_and_exclude_records() {
    let t = seeded().await;
    let db = &t.db;
    assert_eq!(
        Company::query()
            .where_key(2_i64)
            .first(db)
            .await
            .unwrap()
            .unwrap()
            .name,
        "Globex"
    );
    assert_eq!(
        Company::query()
            .where_key_not(2_i64)
            .count(db)
            .await
            .unwrap(),
        3
    );
    assert_eq!(
        Company::query()
            .where_key_in([1_i64, 4])
            .count(db)
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        Company::query()
            .where_key_not_in([1_i64, 4])
            .count(db)
            .await
            .unwrap(),
        2
    );
    let unknown = Company::query()
        .where_(Company::NAME.op("=>", "x"))
        .get(db)
        .await
        .unwrap_err();
    assert!(
        unknown.to_string().contains("`=>` is not a known operator"),
        "{unknown}"
    );
}

#[tokio::test]
async fn time_buckets_run_on_sqlite() {
    let t = sqlite().await;
    let base = Utc.with_ymd_and_hms(2026, 10, 27, 8, 0, 0).unwrap();
    for minutes in [0, 10, 20, 70, 75, 130] {
        crate::models::Event::create(
            &t.db,
            crate::models::Event {
                name: "tick".into(),
                occurred_at: base + chrono::Duration::minutes(minutes),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }
    let rows = table("events")
        .select_time_bucket("1 hour", "occurred_at", "bucket")
        .select_expr(Expr::count_all(), "ticks")
        .group_by(["bucket"])
        .order_by("bucket")
        .get(&t.db)
        .await
        .unwrap();
    let buckets: Vec<(String, i64)> = rows
        .iter()
        .map(|row| {
            (
                row.get::<String>("bucket").unwrap(),
                row.get::<i64>("ticks").unwrap(),
            )
        })
        .collect();
    assert_eq!(
        buckets,
        [
            ("2026-10-27 08:00:00".to_owned(), 3),
            ("2026-10-27 09:00:00".to_owned(), 2),
            ("2026-10-27 10:00:00".to_owned(), 1)
        ]
    );
    let error = table("events")
        .select_first("name", "occurred_at", "first")
        .get(&t.db)
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "first(value, time) (a TimescaleDB aggregate) is not supported on SQLite"
    );
}

#[tokio::test]
async fn statement_timeouts_are_not_supported_on_sqlite() {
    let t = seeded().await;
    let timed = || Company::query().timeout(std::time::Duration::from_secs(1));
    let error = timed().get(&t.db).await.unwrap_err();
    assert_eq!(
        error.to_string(),
        "a statement timeout is not supported on SQLite"
    );
    assert!(timed().count(&t.db).await.is_err());
    assert!(timed().exists(&t.db).await.is_err());
    // Without a timeout nothing changes.
    assert_eq!(Company::query().count(&t.db).await.unwrap(), 4);
}
