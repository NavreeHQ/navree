//! Compound queries on SQLite: unions with their own orders and limits,
//! derived tables, table functions, joins to subqueries, lateral joins and
//! limits per group, and writes through them.

use navree_orm::{Expr, Model, OrmError, col, col_as, table};

use crate::models::{Company, Deal, Stage};
use crate::support::sqlite;

/// Companies A..E (ids 1..5) with 1, 2, 3, 0 and 2 deals of growing value.
async fn seed(db: &navree_db::Db) {
    let mut amount = 0;
    for (name, deals) in [("A", 1), ("B", 2), ("C", 3), ("D", 0), ("E", 2)] {
        let company = Company::create(
            db,
            Company {
                name: name.into(),
                country: Some(if name < "C" { "DE" } else { "US" }.into()),
                ..Company::default()
            },
        )
        .await
        .unwrap();
        for n in 0..deals {
            amount += 100;
            Deal::create(
                db,
                Deal {
                    company_id: Some(company.id),
                    stage: if n == 0 { Stage::ClosedWon } else { Stage::Lead },
                    ..Deal::new(&format!("{name}{n}"), amount)
                },
            )
            .await
            .unwrap();
        }
    }
}

fn names(companies: &navree_orm::Collection<Company>) -> Vec<String> {
    companies.map(|c| c.name.clone())
}

#[tokio::test]
async fn unions_sort_and_cut_as_a_whole() {
    let t = sqlite().await;
    seed(&t.db).await;
    // The second part keeps its own order and limit (a derived table on
    // SQLite); the outer order and limit apply to the union.
    let companies = Company::query()
        .where_(Company::COUNTRY.eq("DE"))
        .union(
            Company::query()
                .where_(Company::COUNTRY.eq("US"))
                .order_by_desc(Company::NAME)
                .limit(2),
        )
        .order_by(Company::NAME)
        .limit(3)
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(names(&companies), ["A", "B", "D"]);
    // Counts and pages count the union's rows.
    let union = || {
        Company::query()
            .where_(Company::COUNTRY.eq("DE"))
            .union_all(Company::query().where_(Company::NAME.eq("A")))
    };
    assert_eq!(union().count(&t.db).await.unwrap(), 3);
    let page = union().order_by(Company::NAME).paginate(&t.db, (2, 2)).await.unwrap();
    assert_eq!(page.meta.total, 3);
    assert_eq!(page.data.len(), 1);
    assert!(union().exists(&t.db).await.unwrap());
    // Values come from the union as a whole, in its order.
    let plucked: Vec<String> = union()
        .order_by_desc(Company::NAME)
        .pluck(&t.db, Company::NAME)
        .await
        .unwrap();
    assert_eq!(plucked, ["B", "A", "A"]);
    let first = union()
        .order_by(Company::NAME)
        .value(&t.db, Company::NAME)
        .await
        .unwrap();
    assert_eq!(first.as_deref(), Some("A"));
}

#[tokio::test]
async fn models_read_from_derived_tables() {
    let t = sqlite().await;
    seed(&t.db).await;
    let big_or_won = Deal::query()
        .from_sub(
            Deal::query()
                .where_(Deal::STAGE.eq(Stage::ClosedWon))
                .union(Deal::query().where_(Deal::AMOUNT_CENTS.gte(700))),
        )
        .order_by(Deal::AMOUNT_CENTS);
    let titles: Vec<String> = big_or_won
        .clone()
        .pluck(&t.db, Deal::TITLE)
        .await
        .unwrap();
    assert_eq!(titles, ["A0", "B0", "C0", "E0", "E1"]);
    // A write through a derived table narrows by key: only those rows.
    let changed = big_or_won
        .update(&t.db, [Deal::PROBABILITY.set(0.9)])
        .await
        .unwrap();
    assert_eq!(changed, 5);
    assert_eq!(
        Deal::query()
            .where_(Deal::PROBABILITY.eq(0.9))
            .count(&t.db)
            .await
            .unwrap(),
        5
    );
}

#[tokio::test]
async fn limits_per_group_keep_the_top_rows_of_each() {
    let t = sqlite().await;
    seed(&t.db).await;
    let top = Deal::query()
        .order_by_desc(Deal::AMOUNT_CENTS)
        .group_limit(2, Deal::COMPANY_ID)
        .get(&t.db)
        .await
        .unwrap();
    let mut titles: Vec<String> = top.map(|deal| deal.title.clone());
    titles.sort();
    assert_eq!(titles, ["A0", "B0", "B1", "C1", "C2", "E0", "E1"]);
    assert!(top.iter().all(|deal| deal.extras().is_empty()), "no rank column");
    // An offset skips per group; the rank orders the rows.
    let second: Vec<String> = Deal::query()
        .order_by_desc(Deal::AMOUNT_CENTS)
        .skip(1)
        .group_limit(1, Deal::COMPANY_ID)
        .pluck(&t.db, Deal::TITLE)
        .await
        .unwrap();
    let mut second = second;
    second.sort();
    assert_eq!(second, ["B0", "C1", "E0"]);
    assert_eq!(
        Deal::query()
            .group_limit(1, Deal::COMPANY_ID)
            .count(&t.db)
            .await
            .unwrap(),
        4
    );
}

#[tokio::test]
async fn joins_to_subqueries_and_table_functions() {
    let t = sqlite().await;
    seed(&t.db).await;
    let best = Deal::query()
        .select([Deal::COMPANY_ID])
        .select_expr(Expr::max(Deal::AMOUNT_CENTS), "top")
        .group_by([Deal::COMPANY_ID]);
    let companies = Company::query()
        .right_join_sub(best, "best", "best.company_id", "=", Company::ID)
        .add_select(["best.top"])
        .order_by(Company::ID)
        .get(&t.db)
        .await
        .unwrap();
    let tops: Vec<(String, i64)> = companies
        .iter()
        .map(|c| (c.name.clone(), c.extra::<i64>("top").unwrap().unwrap()))
        .collect();
    assert_eq!(
        tops,
        [
            ("A".to_owned(), 100),
            ("B".to_owned(), 300),
            ("C".to_owned(), 600),
            ("E".to_owned(), 800)
        ]
    );
    let pairs = table("companies")
        .cross_join_sub(table("deals").select(["title"]).where_(col("amount_cents").eq(100)), "first")
        .count(&t.db)
        .await
        .unwrap();
    assert_eq!(pairs, 5);
    // SQLite's json_each as a table function.
    let numbers: Vec<i64> = table("ignored")
        .from_raw("json_each(?) AS item", ["[3, 1, 2]"])
        .order_by("item.value")
        .pluck(&t.db, col_as::<i64>("item.value"))
        .await
        .unwrap();
    assert_eq!(numbers, [1, 2, 3]);
    let error = Company::query()
        .join_lateral(Deal::query().where_column(Deal::COMPANY_ID, "=", Company::ID), "d")
        .get(&t.db)
        .await
        .unwrap_err();
    assert!(matches!(error, OrmError::Unsupported { .. }), "{error}");
    assert_eq!(Company::meta().table(), "companies");
}
