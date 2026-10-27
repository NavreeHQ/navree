//! The SQL the query builder generates, pinned per dialect: every snapshot
//! shows the PostgreSQL (TimescaleDB), MySQL and SQLite statement with its
//! bound values (or the error a dialect answers).

mod attributes;

use std::borrow::Cow;
use std::fmt::Write as _;

use chrono::{DateTime, NaiveDate, NaiveTime, TimeZone, Utc};
use navree_orm::ir::{
    ColumnRef, DeleteIr, Direction, Expr as IrExpr, FullText, InsertIr, Operator, TableRef,
    UpdateIr, WhereClause,
};
use navree_orm::sql::{compile_delete, compile_insert, compile_update};
use navree_orm::{Builder, Dialect, Expr, Filter, Model, Statement, Value, col, table};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Stage {
    #[default]
    Lead,
    Won,
    Lost,
}

#[derive(Model, Clone, Debug, Default, Serialize)]
#[model(timestamps, soft_deletes)]
struct Deal {
    id: Uuid,
    company_id: Option<i64>,
    title: String,
    amount_cents: i64,
    #[model(cast = "string_enum")]
    stage: Stage,
    #[model(cast = "json")]
    tags: Vec<String>,
    closed_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
}

#[derive(Model, Clone, Debug, Default, Serialize)]
struct Company {
    id: i64,
    name: String,
    country: Option<String>,
}

const DIALECTS: [Dialect; 3] = [Dialect::Postgres, Dialect::MySql, Dialect::Sqlite];

/// Each dialect's statement (or error), one block per dialect.
fn per_dialect(compile: impl Fn(Dialect) -> navree_orm::Result<Statement>) -> String {
    let mut out = String::new();
    for dialect in DIALECTS {
        match compile(dialect) {
            Ok(statement) => {
                writeln!(out, "{dialect}: {}", statement.sql).unwrap();
                if !statement.values.is_empty() {
                    writeln!(out, "  values: {:?}", statement.values).unwrap();
                }
            }
            Err(error) => writeln!(out, "{dialect}: error: {error}").unwrap(),
        }
    }
    out
}

fn query<M: Model>(builder: Builder<M>) -> String {
    per_dialect(|dialect| builder.to_sql(dialect))
}

fn rows(builder: navree_orm::TableBuilder) -> String {
    per_dialect(|dialect| builder.to_sql(dialect))
}

fn at() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 27, 8, 30, 0).unwrap()
}

#[test]
fn model_queries_select_their_columns_and_skip_trashed_rows() {
    insta::assert_snapshot!(query(Deal::query()));
    insta::assert_snapshot!(query(Deal::query().with_trashed().where_key(Uuid::nil())));
    insta::assert_snapshot!(query(Company::query().where_key_in([1_i64, 2, 3])));
}

#[test]
fn comparisons() {
    insta::assert_snapshot!(query(
        Deal::query()
            .where_(Deal::STAGE.eq(Stage::Won))
            .where_(Deal::AMOUNT_CENTS.gte(10_000))
            .where_(Deal::AMOUNT_CENTS.lt(90_000_i64))
            .where_(Deal::TITLE.ne("Draft"))
            .where_(Deal::CLOSED_AT.gt(at()))
            .where_(Deal::AMOUNT_CENTS.op("<>", 7))
    ));
}

#[test]
fn nulls_lists_and_ranges() {
    insta::assert_snapshot!(query(
        Deal::query()
            .where_null(Deal::CLOSED_AT)
            .or_where_not_null(Deal::COMPANY_ID)
            .where_in(Deal::STAGE, [Stage::Lead, Stage::Lost])
            .where_not_in(Deal::COMPANY_ID, [1_i64])
            .where_between(Deal::AMOUNT_CENTS, 100, 200)
            .where_not_between(Deal::CLOSED_AT, at(), at())
            .where_(Deal::CLOSED_AT.eq(None))
    ));
    insta::assert_snapshot!(query(
        Deal::query()
            .where_in(Deal::STAGE, Vec::<Stage>::new())
            .or_where_not_in(Deal::STAGE, Vec::<Stage>::new())
    ));
}

#[test]
fn groups_and_negations() {
    insta::assert_snapshot!(query(
        Deal::query()
            .where_(Deal::AMOUNT_CENTS.gt(0))
            .where_group(|g| {
                g.where_(Deal::STAGE.eq(Stage::Won))
                    .or_where_group(|inner| {
                        inner
                            .where_(Deal::STAGE.eq(Stage::Lost))
                            .where_null(Deal::CLOSED_AT)
                    })
            })
            .where_not(Deal::TITLE.eq("x").or(Deal::TITLE.eq("y")))
            .or_where_not(Deal::AMOUNT_CENTS.lt(5))
            .where_group(|g| g)
    ));
}

#[test]
fn column_comparisons_subqueries_and_exists() {
    insta::assert_snapshot!(query(
        Company::query()
            .where_exists(Deal::query().where_column(Deal::COMPANY_ID, "=", Company::ID))
            .where_not_exists(
                Deal::query()
                    .where_column(Deal::COMPANY_ID, "=", Company::ID)
                    .where_(Deal::STAGE.eq(Stage::Lost))
            )
            .where_in_sub(
                Company::ID,
                Deal::query()
                    .select([Deal::COMPANY_ID])
                    .where_(Deal::STAGE.eq(Stage::Won))
            )
            .where_sub(
                Company::ID,
                ">",
                table("deals").select([Expr::max(col("company_id"))])
            )
    ));
    insta::assert_snapshot!(query(Deal::query().where_column(
        Deal::UPDATED_AT,
        ">",
        Deal::CREATED_AT
    )));
}

#[test]
fn date_parts() {
    insta::assert_snapshot!(query(
        Deal::query()
            .where_date(
                Deal::CLOSED_AT,
                "=",
                NaiveDate::from_ymd_opt(2026, 10, 27).unwrap()
            )
            .where_time(
                Deal::CLOSED_AT,
                ">=",
                NaiveTime::from_hms_opt(9, 0, 0).unwrap()
            )
            .where_day(Deal::CLOSED_AT, "=", 7)
            .where_month(Deal::CLOSED_AT, "<", 10)
            .where_year(Deal::CLOSED_AT, ">", 2025)
    ));
}

#[test]
fn like_patterns() {
    insta::assert_snapshot!(query(
        Deal::query()
            .where_like(Deal::TITLE, "%renewal%")
            .where_not_like(Deal::TITLE, "draft%")
            .where_like_case_sensitive(Deal::TITLE, "Q_ 50%*")
            .where_(Company::COUNTRY.like("u%"))
    ));
}

#[test]
fn json_columns() {
    insta::assert_snapshot!(query(
        Deal::query()
            .where_json_contains(Deal::TAGS, "vip")
            .where_json_doesnt_contain(Deal::TAGS, "spam")
            .where_json_length(Deal::TAGS, ">=", 2)
            .where_(col("meta->owner->name").eq("ada"))
            .where_(col("meta->items[0]").eq(3))
    ));
    insta::assert_snapshot!(query(
        Deal::query().where_json_contains(Deal::TAGS, serde_json::json!({"k": 1}))
    ));
}

#[test]
fn full_text() {
    insta::assert_snapshot!(query(
        Deal::query().where_full_text([Deal::TITLE], "renewal pricing")
    ));
    insta::assert_snapshot!(query(Deal::query().where_full_text_with(
        [Deal::TITLE.expr(), Deal::STAGE.expr()],
        "\"net 30\" -draft",
        FullText::websearch().language("german")
    )));
    insta::assert_snapshot!(query(Deal::query().where_full_text_with(
        [Deal::TITLE],
        "q",
        FullText::default().language("klingon")
    )));
}

#[test]
fn raw_fragments_and_operators() {
    insta::assert_snapshot!(query(
        Deal::query()
            .where_(Deal::AMOUNT_CENTS.gt(1))
            .where_raw(
                "lower(title) = ? OR amount_cents = ?",
                [Value::from("acme"), Value::from(5_i64)]
            )
            .where_raw("meta ?? 'k' AND title <> '?'", Vec::<Value>::new())
            .where_(Deal::TITLE.null_safe_eq("x"))
            .where_(Deal::TITLE.op("regexp", "^A"))
            .where_(Deal::AMOUNT_CENTS.op("&", 4))
    ));
    insta::assert_snapshot!(query(
        Deal::query().where_(Deal::TAGS.op("@>", vec!["a".to_owned()]))
    ));
    insta::assert_snapshot!(query(Deal::query().where_raw("a = ?", [1, 2])));
    insta::assert_snapshot!(query(Deal::query().where_(Deal::TITLE.op("=>", "x"))));
}

/// A raw fragment keeps its own grouping: bare when it is the whole
/// condition, parenthesized beside other conditions and under `NOT`, so an
/// `OR` inside it never regroups the caller's conditions.
#[test]
fn raw_fragments_keep_their_grouping() {
    let sql = |builder: Builder<Company>| builder.to_sql(Dialect::Postgres).unwrap().sql;
    let either = || Filter::raw("name = ? OR country = ?", ["a", "b"]);
    let cases = [
        (
            sql(Company::query().where_(either())),
            "WHERE name = $1 OR country = $2",
        ),
        (
            sql(Company::query().where_not(either())),
            "WHERE NOT (name = $1 OR country = $2)",
        ),
        (
            sql(Company::query().where_(either()).where_(Company::ID.gt(1))),
            r#"WHERE (name = $1 OR country = $2) AND "companies"."id" > $3"#,
        ),
        (
            sql(Company::query()
                .where_(Company::ID.gt(1))
                .or_where_not(either())),
            r#"WHERE "companies"."id" > $1 OR (NOT (name = $2 OR country = $3))"#,
        ),
        (
            sql(Company::query()
                .group_by(["country"])
                .having_raw("count(*) > ? OR max(id) > ?", [1, 2])
                .having_raw("min(id) > ?", [0])),
            "HAVING (count(*) > $1 OR max(id) > $2) AND (min(id) > $3)",
        ),
    ];
    for (sql, ending) in cases {
        assert!(sql.ends_with(ending), "{sql}\n  should end with\n{ending}");
    }
}

#[test]
fn selects_and_distinct() {
    insta::assert_snapshot!(query(
        Deal::query()
            .add_select([(Deal::TITLE, "label")])
            .select_raw("amount_cents * ? AS doubled", [2])
            .select_sub(
                Company::query().select([Company::NAME]).where_column(
                    Company::ID,
                    "=",
                    Deal::COMPANY_ID
                ),
                "company"
            )
            .select_expr(
                Expr::coalesce([col("closed_at"), col("created_at")]),
                "settled_at"
            )
    ));
    insta::assert_snapshot!(query(
        Company::query().select([Company::COUNTRY]).distinct()
    ));
    insta::assert_snapshot!(query(
        Deal::query()
            .distinct_on([Deal::COMPANY_ID])
            .order_by(Deal::COMPANY_ID)
            .latest()
    ));
    insta::assert_snapshot!(rows(table("deals as d").select(["d.title as name", "d.*"])));
}

#[test]
fn joins() {
    insta::assert_snapshot!(query(
        Deal::query()
            .join("companies", Deal::COMPANY_ID, "=", Company::ID)
            .left_join("users as owner", "owner.id", "=", "deals.owner_id")
            .right_join_on("regions", |j| j
                .on("regions.id", "=", "companies.region_id")
                .or_on("regions.code", "=", "companies.code"))
            .cross_join("currencies")
            .where_(Company::COUNTRY.eq("US"))
    ));
    insta::assert_snapshot!(query(
        Company::query().join_sub(
            Deal::query()
                .select([Deal::COMPANY_ID])
                .select_expr(Expr::max(Deal::AMOUNT_CENTS), "top")
                .group_by([Deal::COMPANY_ID]),
            "best",
            "best.company_id",
            "=",
            Company::ID,
        )
    ));
}

#[test]
fn groups_havings_orders_and_pages() {
    insta::assert_snapshot!(rows(
        table("deals")
            .select(["stage"])
            .select_expr(Expr::count_all(), "deals")
            .select_expr(Expr::sum(col("amount_cents")), "total")
            .group_by(["stage"])
            .having(Expr::count_all().gt(2))
            .or_having_raw("SUM(amount_cents) > ?", [1000])
            .order_by_desc("total")
            .order(col("stage").asc().nulls_last())
            .order_by_raw("field(stage, ?, ?) DESC", ["won", "lost"])
            .for_page(3, 25)
    ));
    insta::assert_snapshot!(query(Deal::query().latest().skip(10)));
    insta::assert_snapshot!(query(
        Deal::query()
            .oldest()
            .in_random_order()
            .reorder_by(Deal::TITLE, Direction::Desc)
            .take(5)
    ));
}

#[test]
fn unions_and_locks() {
    insta::assert_snapshot!(rows(
        table("companies")
            .select(["name"])
            .union(table("deals").select(["title"]))
            .union_all(table("users").select(["name"]))
    ));
    insta::assert_snapshot!(query(
        Deal::query().where_key(Uuid::nil()).lock_for_update()
    ));
    insta::assert_snapshot!(query(Company::query().shared_lock()));
}

#[test]
fn timescale_helpers() {
    insta::assert_snapshot!(rows(
        table("readings")
            .select_time_bucket("15 minutes", "taken_at", "bucket")
            .select_expr(Expr::avg(col("celsius")), "average")
            .group_by(["bucket"])
            .order_by("bucket")
    ));
    insta::assert_snapshot!(rows(
        table("readings")
            .select_expr(
                Expr::time_bucket_from("1 day", col("taken_at"), Expr::val(at())),
                "day"
            )
            .select_first("celsius", "taken_at", "opening")
            .select_last("celsius", "taken_at", "closing")
            .group_by(["day"])
    ));
    insta::assert_snapshot!(rows(
        table("readings").select_time_bucket("1 month", "taken_at", "bucket")
    ));
}

#[test]
fn aggregates_count_what_the_query_returns() {
    use navree_orm::ir::{Aggregate, AggregateFunction};
    use navree_orm::sql::{compile_aggregate, compile_exists};
    let count = Aggregate {
        function: AggregateFunction::Count,
        arg: None,
        distinct: false,
    };
    let plain = Deal::query().where_(Deal::STAGE.eq(Stage::Won)).latest();
    insta::assert_snapshot!(per_dialect(|d| compile_aggregate(
        &plain.clone().into_ir(),
        &count,
        d
    )));
    let grouped = table("deals")
        .group_by(["stage"])
        .having(Expr::count_all().gt(1));
    insta::assert_snapshot!(per_dialect(|d| compile_aggregate(
        &grouped.clone().into_ir(),
        &count,
        d
    )));
    let distinct_sum = Aggregate {
        function: AggregateFunction::Sum,
        arg: Some(IrExpr::Column(ColumnRef::parse("amount_cents"))),
        distinct: true,
    };
    let limited = table("deals").order_by("title").limit(10);
    insta::assert_snapshot!(per_dialect(|d| compile_aggregate(
        &limited.clone().into_ir(),
        &distinct_sum,
        d
    )));
    insta::assert_snapshot!(per_dialect(|d| compile_exists(&plain.clone().into_ir(), d)));
}

#[test]
fn writes() {
    let insert = InsertIr {
        table: Cow::Borrowed("companies"),
        columns: vec![Cow::Borrowed("name"), Cow::Borrowed("country")],
        rows: vec![
            vec![
                IrExpr::Value(Value::from("Acme")),
                IrExpr::Value(Value::String(None)),
            ],
            vec![
                IrExpr::Value(Value::from("Globex")),
                IrExpr::Value(Value::from("DE")),
            ],
        ],
        returning: vec![Cow::Borrowed("id")],
    };
    insta::assert_snapshot!(per_dialect(|d| compile_insert(&insert, d)));
    let empty = InsertIr {
        table: Cow::Borrowed("counters"),
        columns: Vec::new(),
        rows: Vec::new(),
        returning: Vec::new(),
    };
    insta::assert_snapshot!(per_dialect(|d| compile_insert(&empty, d)));
    let update = UpdateIr {
        table: TableRef::new("deals"),
        sets: vec![
            (Cow::Borrowed("stage"), IrExpr::Value(Value::from("won"))),
            (
                Cow::Borrowed("amount_cents"),
                Expr::col("amount_cents").add(Expr::val(5_i64)),
            ),
        ],
        wheres: vec![WhereClause::and(Deal::ID.eq(Uuid::nil()).into_where())],
    };
    insta::assert_snapshot!(per_dialect(|d| compile_update(&update, d)));
    let delete = DeleteIr {
        table: TableRef::new("deals"),
        wheres: vec![WhereClause::and(Deal::AMOUNT_CENTS.lt(0).into_where())],
    };
    insta::assert_snapshot!(per_dialect(|d| compile_delete(&delete, d)));
}

#[test]
fn operators_parse_from_their_spellings() {
    assert_eq!(Operator::from("not ilike"), Operator::NotILike);
    assert_eq!(Operator::from("is not distinct from"), Operator::NullSafeEq);
}
