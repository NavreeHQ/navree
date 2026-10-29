//! The ORM's IR rendered through the renderer (`render_*`): the cases of
//! the `sql` tests, so the two can be read side by side. The differential
//! tests show that both run the same on real databases.

use std::borrow::Cow;
use std::fmt::Write as _;

use chrono::{DateTime, NaiveDate, NaiveTime, TimeZone, Utc};
use navree_orm::ir::{
    Aggregate, AggregateFunction, ColumnRef, DeleteIr, Direction, Expr as IrExpr, FullText,
    InsertIr, OnConflict, TableRef, UpdateIr, WhereClause,
};
use navree_orm::render::{
    render_aggregate, render_delete, render_exists, render_insert, render_select, render_truncate,
    render_update, render_update_from,
};
use navree_orm::{Builder, Dialect, Expr, Filter, Kind, LockMode, Model, Value, col, table};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{DIALECTS, per_dialect};

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

fn query<M: Model>(builder: Builder<M>) -> String {
    per_dialect(|dialect| render_select(&builder.clone().into_ir(), dialect))
}

fn rows(builder: navree_orm::TableBuilder) -> String {
    per_dialect(|dialect| render_select(builder.ir(), dialect))
}

fn at() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 27, 8, 30, 0).unwrap()
}

fn count() -> Aggregate {
    Aggregate {
        function: AggregateFunction::Count,
        arg: None,
        distinct: false,
    }
}

#[test]
fn lowered_model_queries_and_comparisons() {
    insta::assert_snapshot!(query(Deal::query()));
    insta::assert_snapshot!(query(Company::query().where_key_in([1_i64, 2, 3])));
    insta::assert_snapshot!(query(
        Deal::query()
            .where_(Deal::STAGE.eq(Stage::Won))
            .where_(Deal::AMOUNT_CENTS.gte(10_000))
            .where_(Deal::TITLE.ne("Draft"))
            .where_(Deal::CLOSED_AT.gt(at()))
            .where_(Deal::AMOUNT_CENTS.op("<>", 7))
    ));
}

#[test]
fn lowered_nulls_lists_ranges_and_groups() {
    insta::assert_snapshot!(query(
        Deal::query()
            .where_null(Deal::CLOSED_AT)
            .or_where_not_null(Deal::COMPANY_ID)
            .where_in(Deal::STAGE, [Stage::Lead, Stage::Lost])
            .where_not_in(Deal::COMPANY_ID, [1_i64])
            .where_between(Deal::AMOUNT_CENTS, 100, 200)
            .where_not_between(Deal::CLOSED_AT, at(), at())
    ));
    insta::assert_snapshot!(query(
        Deal::query()
            .where_in(Deal::STAGE, Vec::<Stage>::new())
            .or_where_not_in(Deal::STAGE, Vec::<Stage>::new())
    ));
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
    ));
}

#[test]
fn lowered_subqueries_exists_and_columns() {
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
}

#[test]
fn lowered_date_parts_and_patterns() {
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
    insta::assert_snapshot!(query(
        Deal::query()
            .where_like(Deal::TITLE, "%renewal%")
            .where_not_like(Deal::TITLE, "draft%")
            .where_like_case_sensitive(Deal::TITLE, "Q_ 50%*")
            .where_(Company::COUNTRY.like("u%"))
    ));
    insta::assert_snapshot!(query(
        Deal::query()
            .where_(Deal::TITLE.ilike("%renewal%"))
            .where_(Deal::TITLE.regex("^[A-Z]"))
            .where_(Deal::TITLE.not_regex("draft$"))
            .where_(Deal::TITLE.regex_case_insensitive("^acme"))
            .where_(Deal::TITLE.not_regex_case_insensitive("test"))
            .where_(Company::COUNTRY.is_distinct_from("US"))
    ));
    insta::assert_snapshot!(query(
        Company::query()
            .where_(Company::NAME.contains_ignore_case("Équipe 50%_"))
            .or_where(Company::COUNTRY.eq_ignore_case("Österreich"))
            .or_where(Company::NAME.contains_ignore_case("acme"))
    ));
}

#[test]
fn lowered_json_columns() {
    insta::assert_snapshot!(query(
        Deal::query()
            .where_json_contains(Deal::TAGS, "vip")
            .where_json_doesnt_contain(Deal::TAGS, "spam")
            .where_json_length(Deal::TAGS, ">=", 2)
            .where_(col("meta->owner->name").eq("ada"))
            .where_(col("meta->items[0]").eq(3))
    ));
    insta::assert_snapshot!(query(
        Deal::query()
            .where_json_overlaps(Deal::TAGS, ["vip", "q1"])
            .or_where_json_doesnt_overlap("meta->labels", "spam")
    ));
    insta::assert_snapshot!(query(
        Deal::query()
            .where_json_contains_key("meta->owner->email")
            .where_json_doesnt_contain_key("meta->tags[2]")
            .or_where_json_contains_key("meta->tags[-1]")
    ));
    insta::assert_snapshot!(query(
        Deal::query()
            .where_(Deal::TAGS.contained_by(["vip", "q1", "q2"]))
            .where_(col("meta").has_key("owner"))
            .where_(Deal::TAGS.overlaps("vip"))
    ));
}

#[test]
fn lowered_full_text_raw_and_operators() {
    insta::assert_snapshot!(query(Deal::query().where_full_text_with(
        [Deal::TITLE.expr(), Deal::STAGE.expr()],
        "\"net 30\" -draft",
        FullText::websearch().language("german")
    )));
    insta::assert_snapshot!(rows(table("documents").where_full_text_with(
        ["search_vector"],
        "renewal pricing",
        FullText::english().websearch_mode().vectors()
    )));
    insta::assert_snapshot!(query(
        Deal::query()
            .where_(Deal::AMOUNT_CENTS.gt(1))
            .where_raw(
                "lower(title) = ? OR amount_cents = ?",
                [Value::from("acme"), Value::from(5_i64)]
            )
            .where_raw("meta ?? 'k' AND title <> '?'", Vec::<Value>::new())
            .where_(Deal::TITLE.null_safe_eq("x"))
            .where_(Deal::AMOUNT_CENTS.op("&", 4))
    ));
    insta::assert_snapshot!(query(Deal::query().where_raw("a = ?", [1, 2])));
    insta::assert_snapshot!(query(Deal::query().where_(Deal::TITLE.op("=>", "x"))));
    insta::assert_snapshot!(query(Deal::query().where_(Filter::op_raw(
        Deal::TITLE,
        "sounds like",
        Expr::val("renewal")
    ))));
    insta::assert_snapshot!(rows(
        table("deals").select_expr(Expr::col("title").binary("||", Expr::val("!")), "loud")
    ));
}

#[test]
fn lowered_selects_joins_and_sources() {
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
        Deal::query()
            .distinct_on([Deal::COMPANY_ID])
            .order_by(Deal::COMPANY_ID)
            .latest()
    ));
    insta::assert_snapshot!(query(
        Deal::query()
            .join("companies", Deal::COMPANY_ID, "=", Company::ID)
            .left_join("users as owner", "owner.id", "=", "deals.owner_id")
            .cross_join("currencies")
            .where_(Company::COUNTRY.eq("US"))
    ));
    insta::assert_snapshot!(query(
        Company::query()
            .join_lateral(
                Deal::query()
                    .select([Deal::TITLE.expr()])
                    .where_column(Deal::COMPANY_ID, "=", Company::ID)
                    .order_by_desc(Deal::CREATED_AT)
                    .limit(2),
                "latest"
            )
            .add_select(["latest.title"])
    ));
    insta::assert_snapshot!(rows(
        table("ignored").from_raw("generate_series(?, ?) AS n", [1, 10])
    ));
    insta::assert_snapshot!(query(
        Deal::query()
            .from_sub(
                Deal::query()
                    .where_(Deal::STAGE.eq(Stage::Won))
                    .union_all(Deal::query().where_(Deal::AMOUNT_CENTS.gt(100)))
            )
            .order_by(Deal::TITLE)
    ));
}

#[test]
fn lowered_groups_orders_unions_and_locks() {
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
    insta::assert_snapshot!(query(
        Company::query()
            .where_(Company::COUNTRY.eq("DE"))
            .union(
                Company::query()
                    .where_(Company::COUNTRY.eq("US"))
                    .order_by(Company::NAME)
                    .limit(5)
            )
            .order_by_desc(Company::NAME)
            .limit(10)
    ));
    insta::assert_snapshot!(query(
        Company::query()
            .where_key(1_i64)
            .lock(LockMode::NoKeyUpdate)
            .skip_locked()
    ));
    insta::assert_snapshot!(query(Company::query().where_key(1_i64).shared_lock()));
    insta::assert_snapshot!(query(
        Company::query().where_key(1_i64).shared_lock().no_wait()
    ));
}

#[test]
fn lowered_group_limits_time_buckets_and_vectors() {
    insta::assert_snapshot!(rows(
        table("activities")
            .select(["id", "deal_id"])
            .order_by_desc("due_at")
            .skip(1)
            .take(50)
            .group_limit(3, "deal_id")
    ));
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
            .group_by(["day"])
    ));
    let embedding = [0.25_f32, -1.5, 3.0];
    insta::assert_snapshot!(rows(
        table("documents")
            .select(["id"])
            .select_vector_distance("embedding", &embedding, "distance")
            .where_vector_similar_to("embedding", &embedding, 0.75)
            .limit(5)
    ));
}

#[test]
fn lowered_aggregates_exists_and_timeouts() {
    let plain = Deal::query().where_(Deal::STAGE.eq(Stage::Won)).latest();
    insta::assert_snapshot!(per_dialect(|d| render_aggregate(
        &plain.clone().into_ir(),
        &count(),
        d
    )));
    let grouped = table("deals")
        .group_by(["stage"])
        .having(Expr::count_all().gt(1));
    insta::assert_snapshot!(per_dialect(|d| render_aggregate(grouped.ir(), &count(), d)));
    let distinct_sum = Aggregate {
        function: AggregateFunction::Sum,
        arg: Some(IrExpr::Column(ColumnRef::parse("amount_cents"))),
        distinct: true,
    };
    let limited = table("deals").order_by("title").limit(10);
    insta::assert_snapshot!(per_dialect(|d| render_aggregate(
        limited.ir(),
        &distinct_sum,
        d
    )));
    insta::assert_snapshot!(per_dialect(|d| render_exists(&plain.clone().into_ir(), d)));
    let timed = Company::query()
        .where_(Company::COUNTRY.eq("NZ"))
        .timeout(std::time::Duration::from_micros(1_500_100))
        .into_ir();
    let mut out = per_dialect(|d| render_select(&timed, d));
    writeln!(
        out,
        "count: {}",
        render_aggregate(&timed, &count(), Dialect::MySql)
            .unwrap()
            .sql
    )
    .unwrap();
    writeln!(
        out,
        "exists: {}",
        render_exists(&timed, Dialect::MySql).unwrap().sql
    )
    .unwrap();
    insta::assert_snapshot!(out);
}

#[test]
fn lowered_writes() {
    let insert = InsertIr::new(
        "companies",
        vec![Cow::Borrowed("name"), Cow::Borrowed("country")],
        vec![
            vec![
                IrExpr::Value(Value::from("Acme")),
                IrExpr::Value(Value::Null(Kind::String)),
            ],
            vec![
                IrExpr::Value(Value::from("Globex")),
                IrExpr::Value(Value::from("DE")),
            ],
        ],
    )
    .returning(vec![Cow::Borrowed("id")]);
    insta::assert_snapshot!(per_dialect(|d| render_insert(&insert, d)));
    let empty = InsertIr::new("counters", Vec::new(), Vec::new());
    insta::assert_snapshot!(per_dialect(|d| render_insert(&empty, d)));
    let upsert = InsertIr::new(
        "tags",
        vec![Cow::Borrowed("name"), Cow::Borrowed("color")],
        vec![vec![
            IrExpr::Value(Value::from("vip")),
            IrExpr::Value(Value::from("gold")),
        ]],
    )
    .on_conflict(OnConflict::Update {
        target: vec![Cow::Borrowed("name")],
        sets: vec![
            (Cow::Borrowed("color"), Expr::excluded("color")),
            (
                Cow::Borrowed("hits"),
                Expr::col("tags.hits").add(Expr::excluded("hits")),
            ),
        ],
    });
    insta::assert_snapshot!(per_dialect(|d| render_insert(&upsert, d)));
    let mut archive = InsertIr::new(
        "archived_deals",
        vec![Cow::Borrowed("id"), Cow::Borrowed("title")],
        Vec::new(),
    )
    .on_conflict(OnConflict::Ignore { target: vec![] });
    archive.select = Some(Box::new(table("deals").select(["id", "title"]).into_ir()));
    insta::assert_snapshot!(per_dialect(|d| render_insert(&archive, d)));
    let update = UpdateIr {
        table: TableRef::new("deals"),
        sets: vec![
            (
                Cow::Borrowed("meta->status"),
                IrExpr::Value(Value::from("won")),
            ),
            (
                Cow::Borrowed("meta->owner->name"),
                IrExpr::Value(Value::from("ada")),
            ),
            (
                Cow::Borrowed("title"),
                IrExpr::Value(Value::from("Renewal")),
            ),
            (
                Cow::Borrowed("amount_cents"),
                Expr::col("amount_cents").add(Expr::val(5_i64)),
            ),
        ],
        wheres: vec![WhereClause::and(Deal::ID.eq(Uuid::nil()).into_where())],
    };
    insta::assert_snapshot!(per_dialect(|d| render_update(&update, d)));
    let delete = DeleteIr {
        table: TableRef::new("deals"),
        wheres: vec![WhereClause::and(Deal::AMOUNT_CENTS.lt(0).into_where())],
    };
    insta::assert_snapshot!(per_dialect(|d| render_delete(&delete, d)));
    let query = table("deals")
        .join("companies as c", "c.id", "=", "deals.company_id")
        .where_(col("c.active").eq(false))
        .into_ir();
    let sets = vec![(Cow::Borrowed("stage"), IrExpr::Value(Value::from("lost")))];
    insta::assert_snapshot!(per_dialect(|d| render_update_from(
        &query.table,
        &sets,
        &query.joins,
        &query.wheres,
        d
    )));
    let mut out = String::new();
    for dialect in DIALECTS {
        for statement in render_truncate("deals", false, dialect) {
            writeln!(out, "{dialect}: {statement}").unwrap();
        }
    }
    for statement in render_truncate("audit.events", true, Dialect::Postgres) {
        writeln!(out, "cascade: {statement}").unwrap();
    }
    insta::assert_snapshot!(out);
}
