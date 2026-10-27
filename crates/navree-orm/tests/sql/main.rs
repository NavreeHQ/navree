//! The SQL the query builder generates, pinned per dialect: every snapshot
//! shows the PostgreSQL (TimescaleDB), MySQL and SQLite statement with its
//! bound values (or the error a dialect answers).

mod attributes;

use std::borrow::Cow;
use std::fmt::Write as _;

use chrono::{DateTime, NaiveDate, NaiveTime, TimeZone, Utc};
use navree_orm::ir::{
    ColumnRef, DeleteIr, Direction, Expr as IrExpr, FullText, InsertIr, OnConflict, Operator,
    TableRef, UpdateIr, WhereClause,
};
use navree_orm::sql::{compile_delete, compile_insert, compile_update};
use navree_orm::{Builder, Dialect, Expr, Filter, Model, Statement, Value, col, table};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

mod lifecycle;

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
    let insert = InsertIr::new(
        "companies",
        vec![Cow::Borrowed("name"), Cow::Borrowed("country")],
        vec![
            vec![
                IrExpr::Value(Value::from("Acme")),
                IrExpr::Value(Value::String(None)),
            ],
            vec![
                IrExpr::Value(Value::from("Globex")),
                IrExpr::Value(Value::from("DE")),
            ],
        ],
    )
    .returning(vec![Cow::Borrowed("id")]);
    insta::assert_snapshot!(per_dialect(|d| compile_insert(&insert, d)));
    let empty = InsertIr::new("counters", Vec::new(), Vec::new());
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
fn json_overlaps_keys_and_containment() {
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
    insta::assert_snapshot!(query(Deal::query().where_json_contains_key(Deal::TAGS)));
}

#[test]
fn pattern_and_null_safe_operators() {
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
        Deal::query().where_(Deal::TITLE.similar_to("(Renewal|Upsell)%"))
    ));
}

#[test]
fn operators_written_by_hand() {
    insta::assert_snapshot!(query(Deal::query().where_(Filter::op_raw(
        Deal::TAGS,
        "@>",
        Expr::raw("?::jsonb", ["[\"vip\"]"])
    ))));
    insta::assert_snapshot!(query(Deal::query().where_(Filter::op_raw(
        Deal::TITLE,
        "sounds like",
        Expr::val("renewal")
    ))));
    insta::assert_snapshot!(query(Deal::query().where_(Filter::op_raw(
        Deal::TITLE,
        "glob",
        Expr::val("Re*")
    ))));
    insta::assert_snapshot!(rows(
        table("deals").select_expr(Expr::col("title").binary("||", Expr::val("!")), "loud")
    ));
    insta::assert_snapshot!(rows(table("deals").select_expr(
        Expr::col("meta").binary("#-", Expr::raw("'{owner,email}'", [] as [i64; 0])),
        "meta"
    )));
}

#[test]
fn full_text_modes_and_vectors() {
    insta::assert_snapshot!(query(Deal::query().where_full_text_with(
        [Deal::TITLE],
        "fat & (rat | cat)",
        FullText::raw()
    )));
    insta::assert_snapshot!(rows(table("documents").where_full_text_with(
        ["search_vector"],
        "renewal pricing",
        FullText::english().websearch_mode().vectors()
    )));
    insta::assert_snapshot!(query(
        Deal::query()
            .where_(Deal::STAGE.eq(Stage::Won))
            .or_where_full_text_with([Deal::TITLE], "pricing", FullText::phrase())
    ));
}

#[test]
fn vector_similarity() {
    let embedding = [0.25_f32, -1.5, 3.0];
    insta::assert_snapshot!(rows(
        table("documents")
            .select(["id"])
            .select_vector_distance("embedding", &embedding, "distance")
            .where_vector_similar_to("embedding", &embedding, 0.75)
            .limit(5)
    ));
    insta::assert_snapshot!(rows(
        table("documents")
            .where_vector_distance_less_than("embedding", &embedding, 0.4)
            .order_by_vector_distance("embedding", &embedding)
    ));
    insta::assert_snapshot!(rows(table("documents").select_expr(
        Expr::vector_distance_by(
            Expr::col("embedding"),
            &embedding,
            navree_orm::VectorMetric::Euclidean
        ),
        "l2"
    )));
    insta::assert_snapshot!(rows(table("documents").select_expr(
        Expr::vector_distance(Expr::col("embedding"), &[f32::NAN]),
        "bad"
    )));
}

#[test]
fn sources_aliases_derived_tables_and_table_functions() {
    insta::assert_snapshot!(rows(
        table("deals").from_as("d").where_(col("d.stage").eq("won"))
    ));
    insta::assert_snapshot!(rows(
        table("ignored").from_raw("generate_series(?, ?) AS n", [1, 10])
    ));
    insta::assert_snapshot!(rows(
        table("ignored").from_function(Expr::func("json_each", [Expr::val("[1,2]")]), "item")
    ));
    insta::assert_snapshot!(rows(table("ignored").from_raw("SELECT 1", [] as [i64; 0])));
    // A model's rows from a derived table named like its table.
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
fn sub_joins_and_lateral_joins() {
    let best = || {
        Deal::query()
            .select([Deal::COMPANY_ID])
            .select_expr(Expr::max(Deal::AMOUNT_CENTS), "top")
            .group_by([Deal::COMPANY_ID])
    };
    insta::assert_snapshot!(query(Company::query().right_join_sub(
        best(),
        "best",
        "best.company_id",
        "=",
        Company::ID
    )));
    insta::assert_snapshot!(query(
        Company::query().cross_join_sub(table("regions").select(["code"]), "r")
    ));
    let latest = || {
        Deal::query()
            .select([Deal::TITLE.expr()])
            .where_column(Deal::COMPANY_ID, "=", Company::ID)
            .order_by_desc(Deal::CREATED_AT)
            .limit(2)
    };
    insta::assert_snapshot!(query(
        Company::query()
            .join_lateral(latest(), "latest")
            .add_select(["latest.title"])
    ));
    insta::assert_snapshot!(query(
        Company::query()
            .left_join_lateral(latest(), "latest")
            .add_select(["latest.title"])
    ));
}

#[test]
fn limits_per_group() {
    insta::assert_snapshot!(query(
        Deal::query().latest().group_limit(2, Deal::COMPANY_ID)
    ));
    insta::assert_snapshot!(rows(
        table("activities")
            .select(["id", "deal_id"])
            .order_by_desc("due_at")
            .skip(1)
            .take(50)
            .group_limit(3, "deal_id")
    ));
    use navree_orm::ir::{Aggregate, AggregateFunction};
    use navree_orm::sql::compile_aggregate;
    let count = Aggregate {
        function: AggregateFunction::Count,
        arg: None,
        distinct: false,
    };
    let limited = Deal::query().latest().group_limit(2, Deal::COMPANY_ID);
    insta::assert_snapshot!(per_dialect(|d| compile_aggregate(
        &limited.clone().into_ir(),
        &count,
        d
    )));
}

#[test]
fn unions_with_orders_and_limits() {
    // The orders and limit of a query with unions sort and cut the union;
    // a part with its own order or limit is a derived table on SQLite.
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
}

#[test]
fn lock_modes_and_waits() {
    use navree_orm::LockMode;
    let locked = |q: Builder<Company>| query(q.where_key(1_i64));
    insta::assert_snapshot!(locked(
        Company::query().lock(LockMode::NoKeyUpdate).skip_locked()
    ));
    insta::assert_snapshot!(locked(Company::query().shared_lock().no_wait()));
    insta::assert_snapshot!(locked(Company::query().lock(LockMode::KeyShare)));
    // `skip_locked` alone locks for update; a later mode keeps the wait.
    insta::assert_snapshot!(locked(Company::query().skip_locked().lock(LockMode::Share)));
}

#[test]
fn inserts_that_skip_or_update_conflicts() {
    let base = || {
        InsertIr::new(
            "tags",
            vec![Cow::Borrowed("name"), Cow::Borrowed("color")],
            vec![vec![
                IrExpr::Value(Value::from("vip")),
                IrExpr::Value(Value::from("gold")),
            ]],
        )
    };
    insta::assert_snapshot!(per_dialect(|d| compile_insert(
        &base().on_conflict(OnConflict::Ignore { target: vec![] }),
        d
    )));
    insta::assert_snapshot!(per_dialect(|d| compile_insert(
        &base()
            .returning(vec![Cow::Borrowed("id")])
            .on_conflict(OnConflict::Ignore {
                target: vec![Cow::Borrowed("name")]
            }),
        d
    )));
    insta::assert_snapshot!(per_dialect(|d| compile_insert(
        &base().on_conflict(OnConflict::Update {
            target: vec![Cow::Borrowed("name")],
            sets: vec![
                (Cow::Borrowed("color"), Expr::excluded("color")),
                (
                    Cow::Borrowed("hits"),
                    Expr::col("tags.hits").add(Expr::excluded("hits"))
                ),
                (Cow::Borrowed("note"), Expr::val("merged")),
            ],
        }),
        d
    )));
    // PostgreSQL and SQLite need the conflict target of an update.
    insta::assert_snapshot!(per_dialect(|d| compile_insert(
        &base().on_conflict(OnConflict::Update {
            target: vec![],
            sets: vec![(Cow::Borrowed("color"), Expr::excluded("color"))],
        }),
        d
    )));
}

#[test]
fn inserts_from_queries() {
    let mut archive = InsertIr::new(
        "archived_deals",
        vec![Cow::Borrowed("id"), Cow::Borrowed("title")],
        Vec::new(),
    );
    archive.select = Some(Box::new(
        table("deals")
            .select(["id", "title"])
            .where_(col("stage").eq("lost"))
            .into_ir(),
    ));
    insta::assert_snapshot!(per_dialect(|d| compile_insert(&archive, d)));
    let mut ignore = InsertIr::new(
        "archived_deals",
        vec![Cow::Borrowed("id"), Cow::Borrowed("title")],
        Vec::new(),
    )
    .on_conflict(OnConflict::Ignore { target: vec![] });
    ignore.select = Some(Box::new(table("deals").select(["id", "title"]).into_ir()));
    insta::assert_snapshot!(per_dialect(|d| compile_insert(&ignore, d)));
    let mut mismatch = InsertIr::new("archived_deals", vec![Cow::Borrowed("id")], Vec::new());
    mismatch.select = Some(Box::new(table("deals").select(["id", "title"]).into_ir()));
    insta::assert_snapshot!(per_dialect(|d| compile_insert(&mismatch, d)));
}

#[test]
fn json_path_updates() {
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
                Cow::Borrowed("meta->tags[0]"),
                IrExpr::Value(Value::from(serde_json::json!({"k": true}))),
            ),
            (
                Cow::Borrowed("title"),
                IrExpr::Value(Value::from("Renewal")),
            ),
        ],
        wheres: vec![WhereClause::and(Deal::ID.eq(Uuid::nil()).into_where())],
    };
    insta::assert_snapshot!(per_dialect(|d| compile_update(&update, d)));
    let twice = UpdateIr {
        table: TableRef::new("deals"),
        sets: vec![
            (Cow::Borrowed("meta"), IrExpr::Value(Value::from("{}"))),
            (
                Cow::Borrowed("meta->status"),
                IrExpr::Value(Value::from("won")),
            ),
        ],
        wheres: vec![],
    };
    insta::assert_snapshot!(per_dialect(|d| compile_update(&twice, d)));
}

#[test]
fn table_writes_with_joins_orders_and_limits() {
    insta::assert_snapshot!(per_dialect(|d| table("deals as d")
        .join("companies", "companies.id", "=", "d.company_id")
        .where_(col("companies.active").eq(false))
        .to_update_sql(d, [col("stage").set("lost")])));
    insta::assert_snapshot!(per_dialect(|d| table("sessions")
        .where_(col("user_id").eq(7))
        .order_by("last_seen_at")
        .limit(100)
        .to_delete_sql(d)));
    insta::assert_snapshot!(per_dialect(|d| table("sessions")
        .where_(col("user_id").eq(7))
        .to_delete_sql(d)));
    insta::assert_snapshot!(per_dialect(|d| table("deals")
        .group_by(["stage"])
        .to_update_sql(d, [col("stage").set("x")])));
}

#[test]
fn updates_from_joined_tables() {
    use navree_orm::sql::compile_update_from;
    let query = table("deals")
        .join("companies as c", "c.id", "=", "deals.company_id")
        .where_(col("c.active").eq(false))
        .into_ir();
    let sets = vec![(Cow::Borrowed("stage"), IrExpr::Value(Value::from("lost")))];
    insta::assert_snapshot!(per_dialect(|d| compile_update_from(
        &query.table,
        &sets,
        &query.joins,
        &query.wheres,
        d
    )));
    let left = table("deals")
        .left_join("companies", "companies.id", "=", "deals.company_id")
        .into_ir();
    insta::assert_snapshot!(per_dialect(|d| compile_update_from(
        &left.table,
        &sets,
        &left.joins,
        &left.wheres,
        d
    )));
}

#[test]
fn truncation() {
    use navree_orm::sql::compile_truncate;
    let mut out = String::new();
    for dialect in DIALECTS {
        for statement in compile_truncate("deals", false, dialect) {
            writeln!(out, "{dialect}: {statement}").unwrap();
        }
    }
    for statement in compile_truncate("audit.events", true, Dialect::Postgres) {
        writeln!(out, "cascade: {statement}").unwrap();
    }
    insta::assert_snapshot!(out);
}

#[test]
fn operators_parse_from_their_spellings() {
    assert_eq!(Operator::from("not ilike"), Operator::NotILike);
    assert_eq!(Operator::from("is not distinct from"), Operator::NullSafeEq);
}

#[test]
fn statement_timeouts() {
    use std::time::Duration;

    use navree_orm::ir::{Aggregate, AggregateFunction};
    use navree_orm::sql::{compile_aggregate, compile_exists, compile_select};

    let timed = Company::query()
        .where_(Company::COUNTRY.eq("NZ"))
        .timeout(Duration::from_micros(1_500_100));
    let mut out = per_dialect(|d| timed.to_sql(d));
    let ir = timed.clone().into_ir();
    let count = Aggregate {
        function: AggregateFunction::Count,
        arg: None,
        distinct: false,
    };
    writeln!(
        out,
        "count: {}",
        compile_aggregate(&ir, &count, Dialect::MySql).unwrap().sql
    )
    .unwrap();
    let mut limited = ir.clone();
    limited.limit = Some(5);
    writeln!(
        out,
        "count of a limited query: {}",
        compile_aggregate(&limited, &count, Dialect::MySql)
            .unwrap()
            .sql
    )
    .unwrap();
    writeln!(
        out,
        "exists: {}",
        compile_exists(&ir, Dialect::MySql).unwrap().sql
    )
    .unwrap();
    let union = timed.clone().union(Company::query());
    writeln!(
        out,
        "union: {}",
        compile_select(&union.into_ir(), Dialect::MySql)
            .unwrap()
            .sql
    )
    .unwrap();
    insta::assert_snapshot!(out);
}
