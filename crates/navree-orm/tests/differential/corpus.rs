//! The IR statements the renderer writes and the compiler wrote: every IR
//! feature the renderer lowers, on the fixture tables of `support.rs`. A
//! case is built per dialect (row ids are `ctid` on PostgreSQL, `rowid` on
//! SQLite).

use std::borrow::Cow;
use std::time::Duration;

use chrono::{DateTime, NaiveDate, NaiveTime, TimeZone, Utc};
use navree_orm::ir::{
    Aggregate, AggregateFunction, ColumnRef, DeleteIr, Expr as IrExpr, FullText, InValues,
    InsertIr, Join, JoinKind, OnConflict, QueryIr, SelectItem, TableRef, UpdateIr, Where,
    WhereClause,
};
use navree_orm::{Dialect, Expr, Filter, IntoQuery, LockMode, Model, Value, col, table};
use serde::Serialize;

/// What a case runs.
#[derive(Clone, Debug)]
pub enum Kind {
    /// `render_select`.
    Select(QueryIr),
    /// A locking read, inside a transaction of its own.
    Locked(QueryIr),
    /// `render_aggregate`.
    Aggregate(QueryIr, Aggregate),
    /// `render_exists`.
    Exists(QueryIr),
    /// `render_insert`.
    Insert(InsertIr),
    /// `render_update`.
    Update(UpdateIr),
    /// `render_update_from`.
    UpdateFrom {
        table: TableRef,
        sets: Vec<(Cow<'static, str>, IrExpr)>,
        joins: Vec<Join>,
        wheres: Vec<WhereClause>,
    },
    /// `render_delete`.
    Delete(DeleteIr),
    /// `render_truncate`.
    Truncate { table: &'static str, cascade: bool },
}

/// One statement, written by the renderer and compared with what the
/// compiler's SQL did.
#[derive(Clone, Debug)]
pub struct Case {
    pub name: String,
    pub kind: Kind,
    /// The rows come in an order the statement defines.
    pub ordered: bool,
    /// The dialects where the renderer deliberately differs from the
    /// compiler, and why (the compiler's SQL is wrong there).
    pub differs: &'static [(Dialect, &'static str)],
}

impl Case {
    fn differs(mut self, on: &'static [(Dialect, &'static str)]) -> Self {
        self.differs = on;
        self
    }
}

fn case(name: &str, kind: Kind) -> Case {
    Case {
        name: name.to_owned(),
        kind,
        ordered: false,
        differs: &[],
    }
}

fn read(name: &str, query: impl IntoQuery) -> Case {
    case(name, Kind::Select(query.into_query()))
}

fn ordered(name: &str, query: impl IntoQuery) -> Case {
    Case {
        ordered: true,
        ..read(name, query)
    }
}

/// The fixture's `deals` as a model: soft deletes, timestamps.
#[derive(Model, Clone, Debug, Default, Serialize)]
#[model(table = "deals", timestamps, soft_deletes)]
pub struct Deal {
    pub id: i64,
    pub company_id: Option<i64>,
    pub owner_id: Option<i64>,
    pub title: String,
    pub amount_cents: i64,
    pub stage: String,
    pub tags: Option<serde_json::Value>,
    pub meta: Option<serde_json::Value>,
    pub closed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

/// The fixture's `companies` as a model.
#[derive(Model, Clone, Debug, Default, Serialize)]
#[model(table = "companies")]
pub struct Company {
    pub id: i64,
    pub name: String,
    pub country: Option<String>,
    pub active: bool,
    pub region_id: Option<i64>,
}

fn at(day: u32, hour: u32, minute: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, day, hour, minute, 0)
        .unwrap()
}

fn count() -> Aggregate {
    Aggregate {
        function: AggregateFunction::Count,
        arg: None,
        distinct: false,
    }
}

fn aggregate(function: AggregateFunction, column: &'static str, distinct: bool) -> Aggregate {
    Aggregate {
        function,
        arg: Some(IrExpr::Column(ColumnRef::parse(column))),
        distinct,
    }
}

fn deals() -> navree_orm::TableBuilder {
    table("deals")
}

/// The read cases.
pub fn reads(dialect: Dialect) -> Vec<Case> {
    let _ = dialect;
    let mut cases = vec![
        ordered("every deal", deals().order_by("id")),
        read("equality", deals().where_(col("stage").eq("won"))),
        read("comparisons", {
            deals()
                .where_(col("amount_cents").gte(4_500))
                .where_(col("amount_cents").lt(250_000_i64))
                .where_(col("title").ne("Draft deal"))
        }),
        read(
            "date-time comparison",
            deals().where_(col("closed_at").gt(at(1, 0, 0))),
        ),
        read(
            "operator from text",
            deals().where_(col("amount_cents").op("<>", 0)),
        ),
        read(
            "nulls with or",
            deals()
                .where_null("closed_at")
                .or_where_not_null("deleted_at"),
        ),
        read("in a list", deals().where_in("stage", ["lead", "lost"])),
        read(
            "not in a list",
            deals().where_not_in("company_id", [1_i64, 3]),
        ),
        read(
            "empty lists",
            deals()
                .where_in("stage", Vec::<String>::new())
                .or_where_not_in("stage", Vec::<String>::new()),
        ),
        read(
            "empty list alone",
            deals().where_in("stage", Vec::<String>::new()),
        ),
        read(
            "between",
            deals()
                .where_between("amount_cents", 1_000, 100_000)
                .where_not_between("created_at", at(2, 0, 0), at(12, 0, 0)),
        ),
        read("groups and negations", {
            deals()
                .where_(col("amount_cents").gt(0))
                .where_group(|g| {
                    g.where_(col("stage").eq("won")).or_where_group(|inner| {
                        inner
                            .where_(col("stage").eq("lost"))
                            .where_not_null("closed_at")
                    })
                })
                .where_not(col("title").eq("x").or(col("title").eq("Orphan_deal")))
                .or_where_not(col("amount_cents").lt(5))
        }),
        read(
            "an empty group",
            deals().where_(Filter::all(Vec::<Filter>::new())),
        ),
        read(
            "a negated empty group",
            deals().where_not(Filter::all(Vec::<Filter>::new())),
        ),
        read(
            "no alternatives",
            deals().where_(Filter::any(Vec::<Filter>::new())),
        ),
        read(
            "column against column",
            deals().where_column("updated_at", ">", "created_at"),
        ),
        read(
            "correlated exists",
            table("companies").where_exists(deals().where_column(
                "deals.company_id",
                "=",
                "companies.id",
            )),
        ),
        read(
            "correlated not exists",
            table("companies").where_not_exists(
                deals()
                    .where_column("deals.company_id", "=", "companies.id")
                    .where_(col("deals.stage").eq("lost")),
            ),
        ),
        read(
            "in a subquery",
            table("companies").where_in_sub(
                "id",
                deals()
                    .select(["company_id"])
                    .where_(col("stage").eq("won")),
            ),
        ),
        read(
            "not in a subquery",
            table("companies").where_not_in_sub(
                "id",
                deals().select(["company_id"]).where_not_null("company_id"),
            ),
        ),
        read(
            "compared with a subquery",
            table("companies").where_sub(
                "id",
                ">=",
                deals().select([Expr::max(col("company_id"))]),
            ),
        ),
        read(
            "date part: date",
            deals().where_date(
                "closed_at",
                "=",
                NaiveDate::from_ymd_opt(2026, 10, 20).unwrap(),
            ),
        ),
        read(
            "date part: date from a date-time",
            deals().where_date("created_at", "<", at(10, 12, 0)),
        ),
        read(
            "date part: time",
            deals().where_time("closed_at", ">=", NaiveTime::from_hms_opt(9, 0, 0).unwrap()),
        ),
        read("date part: day", deals().where_day("created_at", "=", 1)),
        read(
            "date part: month",
            deals().where_month("created_at", "<", 10),
        ),
        read(
            "date part: year",
            deals().where_year("closed_at", ">", 2025),
        ),
        read(
            "date part: or",
            deals()
                .where_day("created_at", "=", 5)
                .or_where_day("created_at", "=", 26),
        ),
        read(
            "like ignoring case",
            deals().where_like("title", "%renewal%"),
        ),
        read("not like", deals().where_not_like("title", "%renewal%")),
        read(
            "like matching case",
            deals().where_like_case_sensitive("title", "Renewal%"),
        ),
        read(
            "like matching case with glob characters",
            deals().where_like_case_sensitive("title", "Upsell 50%"),
        ),
        read(
            "like with one-character wildcards",
            deals().where_like("title", "Orphan_deal"),
        ),
        read(
            "operator like",
            deals().where_(col("title").op("like", "%deal%")),
        ),
        read(
            "operator not ilike",
            deals().where_(col("title").op("not ilike", "%DEAL%")),
        ),
        read("ilike", deals().where_(col("title").ilike("%RENEWAL%"))),
        read(
            "regular expression",
            deals().where_(col("title").regex("^[A-Z]")),
        ),
        read(
            "regular expression mismatch",
            deals().where_(col("title").not_regex("deal$")),
        ),
        read(
            "regular expression ignoring case",
            deals().where_(col("title").regex_case_insensitive("^renewal")),
        ),
        read(
            "distinct from",
            table("companies").where_(col("country").is_distinct_from("US")),
        ),
        read(
            "null-safe equality with a null",
            table("companies").where_(col("country").null_safe_eq(Value::String(None))),
        ),
        read(
            "null-safe equality",
            table("companies").where_(col("country").null_safe_eq("DE")),
        ),
        read(
            "similar to",
            deals().where_(col("title").similar_to("(Renewal|Big)%")),
        ),
        read(
            "bit operators",
            deals().where_(
                Expr::col("amount_cents")
                    .binary("&", Expr::val(4_i64))
                    .ne(Expr::val(0_i64)),
            ),
        ),
        read(
            "contains ignoring case beyond ascii",
            table("companies").where_(col("name").contains_ignore_case("ÉQUIPE")),
        ),
        read(
            "contains ignoring case",
            deals().where_(col("title").contains_ignore_case("RENEWAL")),
        ),
        read(
            "contains ignoring case literally",
            deals().where_(col("title").contains_ignore_case("50%")),
        ),
        read(
            "equals ignoring case",
            table("companies").where_(col("name").eq_ignore_case("acme")),
        ),
        read(
            "equals ignoring case beyond ascii",
            table("companies").where_(col("name").eq_ignore_case("équipe lyon")),
        ),
        read("json contains", deals().where_json_contains("tags", "vip")),
        read(
            "json does not contain",
            deals().where_json_doesnt_contain("tags", "spam"),
        ),
        read(
            "json contains in a path",
            deals().where_json_contains("meta->items", 3),
        ),
        read(
            "json contains an object",
            deals().where_json_contains("meta", serde_json::json!({"status": "open"})),
        ),
        read("json length", deals().where_json_length("tags", ">=", 2)),
        read(
            "json length in a path",
            deals().where_json_length("meta->items", "=", 0),
        ),
        read(
            "json path as text",
            deals().where_(col("meta->owner->name").eq("ada")),
        ),
        read(
            "json path with an index",
            deals().where_(col("meta->items[0]").eq(3)),
        ),
        read(
            "json path from the end",
            deals().where_(col("meta->items[-1]").eq(4)),
        ),
        read(
            "json path selected",
            deals()
                .select(["id", "meta->status as status"])
                .where_not_null("meta"),
        ),
        read(
            "json overlaps",
            deals()
                .where_json_overlaps("tags", ["vip", "q2"])
                .or_where_json_doesnt_overlap("meta->labels", "spam"),
        ),
        read(
            "json contains a key",
            deals()
                .where_json_contains_key("meta->owner->email")
                .or_where_json_contains_key("meta->items[1]"),
        ),
        read(
            "json does not contain a key",
            deals().where_json_doesnt_contain_key("meta->items[-2]"),
        ),
        read(
            "json contained by",
            deals().where_(col("tags").contained_by(["vip", "q1", "q2"])),
        ),
        read(
            "json has a key",
            deals().where_(col("meta").has_key("owner")),
        ),
        read(
            "json overlaps a scalar",
            deals().where_(col("tags").overlaps("q1")),
        ),
        read("full text", deals().where_full_text(["title"], "renewal")),
        read(
            "full text as a phrase",
            deals().where_full_text_with(["title"], "big renewal", FullText::phrase()),
        ),
        read(
            "full text in boolean mode",
            deals().where_full_text_with(["title", "stage"], "renewal -big", FullText::boolean()),
        ),
        read(
            "raw fragment keeps its grouping",
            deals()
                .where_raw("stage = ? OR stage = ?", ["won", "lost"])
                .where_(col("amount_cents").gt(1_000)),
        ),
        read(
            "raw fragment under not",
            deals().where_(Filter::raw("stage = ? OR stage = ?", ["won", "lost"]).not()),
        ),
        read(
            "raw fragment with a literal question mark",
            deals().where_raw("meta ?? 'owner' AND title <> '?'", Vec::<Value>::new()),
        ),
        read(
            "raw fragment with the wrong values",
            deals().where_raw("id = ?", [1, 2]),
        ),
        read(
            "raw or",
            deals()
                .where_(col("stage").eq("lead"))
                .or_where_raw("amount_cents > ?", [200_000]),
        ),
        read(
            "operator written by hand",
            deals().where_(Filter::op_raw("title", "like", Expr::val("%deal"))),
        ),
        read(
            "operator a database lacks",
            deals().where_(Filter::op_raw("title", "glob", Expr::val("*deal"))),
        ),
        read(
            "unknown operator",
            deals().where_(col("title").op("=>", "x")),
        ),
        ordered(
            "selected columns and aliases",
            deals()
                .select(["id", "title as name"])
                .select_raw("amount_cents * ? AS doubled", [2])
                .select_expr(
                    Expr::coalesce([col("closed_at"), col("created_at")]),
                    "settled",
                )
                .order_by("id"),
        ),
        ordered(
            "a correlated subquery selected",
            deals()
                .select(["id"])
                .select_sub(
                    table("companies").select(["name"]).where_column(
                        "companies.id",
                        "=",
                        "deals.company_id",
                    ),
                    "company",
                )
                .order_by("id"),
        ),
        ordered(
            "arithmetic",
            deals()
                .select(["id"])
                .select_expr(
                    Expr::col("amount_cents")
                        .div(Expr::val(100))
                        .add(Expr::val(1))
                        .mul(Expr::val(2)),
                    "scaled",
                )
                .order_by("id"),
        ),
        ordered(
            "a function and a value operator",
            deals()
                .select(["id"])
                .select_expr(Expr::lower(col("title")), "lowered")
                .select_expr(Expr::col("title").binary("||", Expr::val("!")), "loud")
                .order_by("id"),
        ),
        read("distinct", table("companies").select(["active"]).distinct()),
        ordered(
            "distinct on",
            deals()
                .select(["company_id", "title"])
                .distinct_on(["company_id"])
                .where_not_null("company_id")
                .order_by("company_id")
                .order_by_desc("amount_cents"),
        ),
        read(
            "an aliased table",
            deals().from_as("d").where_(col("d.stage").eq("won")),
        ),
        ordered(
            "a derived table",
            navree_orm::TableBuilder::from_sub(
                deals()
                    .select(["company_id"])
                    .select_expr(Expr::sum(col("amount_cents")), "total")
                    .group_by(["company_id"]),
                "totals",
            )
            .where_(col("totals.total").gt(10_000))
            .order_by("totals.company_id"),
        ),
        read(
            "a derived union",
            navree_orm::TableBuilder::from_sub(
                deals()
                    .select(["id"])
                    .where_(col("stage").eq("won"))
                    .union_all(deals().select(["id"]).where_(col("stage").eq("lost"))),
                "both",
            ),
        ),
        read(
            "a table function",
            table("ignored").from_raw("generate_series(?, ?) AS n", [1, 3]),
        ),
        read(
            "json elements as a table",
            table("ignored").from_function(Expr::func("json_each", [Expr::val("[1,2]")]), "item"),
        ),
        ordered(
            "joins",
            deals()
                .select(["deals.id", "companies.name", "owner.name as owner"])
                .join("companies", "companies.id", "=", "deals.company_id")
                .left_join("users as owner", "owner.id", "=", "deals.owner_id")
                .where_(col("companies.active").eq(true))
                .order_by("deals.id"),
        ),
        read(
            "a right join",
            deals().select(["deals.id", "companies.name"]).right_join(
                "companies",
                "companies.id",
                "=",
                "deals.company_id",
            ),
        ),
        read(
            "a cross join",
            table("regions")
                .select(["regions.code", "companies.name"])
                .cross_join("companies")
                .where_(col("companies.active").eq(false)),
        ),
        read(
            "a join on a derived table",
            table("companies")
                .select(["companies.name", "best.top"])
                .join_sub(
                    deals()
                        .select(["company_id"])
                        .select_expr(Expr::max(col("amount_cents")), "top")
                        .group_by(["company_id"]),
                    "best",
                    "best.company_id",
                    "=",
                    "companies.id",
                ),
        ),
        read(
            "a left join on a derived table",
            table("companies")
                .select(["companies.name", "lost.n"])
                .left_join_sub(
                    deals()
                        .select(["company_id"])
                        .select_expr(Expr::count_all(), "n")
                        .where_(col("stage").eq("lost"))
                        .group_by(["company_id"]),
                    "lost",
                    "lost.company_id",
                    "=",
                    "companies.id",
                ),
        ),
        read(
            "a lateral join",
            table("companies")
                .select(["companies.id", "latest.title"])
                .join_lateral(
                    deals()
                        .select(["title"])
                        .where_column("deals.company_id", "=", "companies.id")
                        .order_by_desc("created_at")
                        .limit(1),
                    "latest",
                ),
        ),
        read(
            "a left lateral join",
            table("companies")
                .select(["companies.id", "latest.title"])
                .left_join_lateral(
                    deals()
                        .select(["title"])
                        .where_column("deals.company_id", "=", "companies.id")
                        .where_(col("stage").eq("won"))
                        .order_by_desc("created_at")
                        .limit(1),
                    "latest",
                ),
        ),
        ordered(
            "groups, havings and pages",
            deals()
                .select(["stage"])
                .select_expr(Expr::count_all(), "deals")
                .select_expr(Expr::sum(col("amount_cents")), "total")
                .group_by(["stage"])
                .having(Expr::count_all().gt(1))
                .or_having_raw("SUM(amount_cents) > ?", [1_000_000])
                .order_by_desc("total")
                .for_page(1, 2),
        ),
        ordered(
            "a raw having and a raw group",
            deals()
                .select_raw("company_id AS company", Vec::<Value>::new())
                .select_expr(Expr::count_all(), "n")
                .group_by_raw("company_id", Vec::<Value>::new())
                .having_raw("COUNT(*) >= ?", [2])
                .order_by("company"),
        ),
        ordered(
            "nulls first and last",
            deals()
                .select(["id", "closed_at"])
                .order(col("closed_at").asc().nulls_first())
                .order(col("id").desc().nulls_last()),
        ),
        ordered(
            "nulls last descending",
            deals()
                .select(["id"])
                .order(col("company_id").desc().nulls_last())
                .order_by("id"),
        ),
        ordered(
            "a raw order",
            deals()
                .select(["id", "stage"])
                .order_by_raw("CASE WHEN stage = ? THEN 0 ELSE 1 END ASC", ["won"])
                .order_by("id"),
        ),
        ordered(
            "a raw order with nulls",
            deals()
                .select(["id"])
                .order_by_raw("closed_at DESC NULLS LAST", Vec::<Value>::new())
                .order_by("id"),
        ),
        read(
            "random order of one row",
            deals().where_(col("id").eq(1)).in_random_order(),
        ),
        ordered(
            "latest with an offset",
            deals().select(["id"]).latest().skip(5),
        ),
        ordered(
            "oldest with a limit",
            deals().select(["id"]).oldest().take(3),
        ),
        ordered(
            "a limit and an offset",
            deals().select(["id"]).order_by("id").limit(3).offset(2),
        ),
        ordered(
            "a union ordered and limited",
            table("companies")
                .select(["name"])
                .where_(col("country").eq("US"))
                .union(
                    table("companies")
                        .select(["name"])
                        .where_(col("country").eq("DE")),
                )
                .union_all(table("users").select(["name"]))
                .order_by_desc("name")
                .limit(4),
        ),
        ordered(
            "a union part with its own order and limit",
            table("companies")
                .select(["name"])
                .where_(col("country").eq("FR"))
                .union(
                    table("companies")
                        .select(["name"])
                        .where_not_null("country")
                        .order_by("name")
                        .limit(2),
                )
                .order_by("name"),
        ),
        read(
            "a union inside a union",
            table("companies").select(["name"]).union_all(
                table("users")
                    .select(["name"])
                    .union(table("users").select(["name"])),
            ),
        ),
        ordered(
            "a time out",
            table("companies")
                .select(["id"])
                .where_(col("country").eq("DE"))
                .timeout(Duration::from_millis(1_500))
                .order_by("id"),
        ),
        ordered(
            "a limit per group",
            deals()
                .select(["id", "company_id"])
                .where_not_null("company_id")
                .order_by_desc("amount_cents")
                .group_limit(1, "company_id"),
        ),
        ordered(
            "a limit per group with an offset",
            deals()
                .select(["id", "company_id"])
                .order_by_desc("amount_cents")
                .order_by("id")
                .skip(1)
                .take(5)
                .group_limit(2, "company_id"),
        ),
        read(
            "a limit per group of every column",
            deals().order_by("id").group_limit(1, "stage"),
        ),
        ordered(
            "time buckets",
            table("readings")
                .select_time_bucket("15 minutes", "taken_at", "bucket")
                .select_expr(Expr::avg(col("celsius")), "average")
                .group_by(["bucket"])
                .order_by("bucket"),
        ),
        ordered(
            "time buckets from an origin",
            table("readings")
                .select_expr(
                    Expr::time_bucket_from("1 hour", col("taken_at"), Expr::val(at(26, 0, 30))),
                    "hour",
                )
                .select_expr(Expr::count_all(), "n")
                .group_by(["hour"])
                .order_by("hour"),
        ),
        read(
            "calendar time buckets",
            table("readings").select_time_bucket("1 month", "taken_at", "month"),
        ),
        read(
            "first and last",
            table("readings")
                .select_first("celsius", "taken_at", "opening")
                .select_last("celsius", "taken_at", "closing"),
        ),
        read(
            "a vector distance",
            table("deals")
                .select(["id"])
                .select_vector_distance("tags", &[0.5, 1.0], "distance"),
        ),
        ordered(
            "a model query skips trashed rows",
            Deal::query().order_by(Deal::ID),
        ),
        ordered(
            "a model query with trashed rows",
            Deal::query().with_trashed().order_by(Deal::ID),
        ),
        ordered(
            "a model query of trashed rows only",
            Deal::query().only_trashed().order_by(Deal::ID),
        ),
        ordered(
            "keys",
            Company::query()
                .where_key_in([1_i64, 3, 4])
                .order_by(Company::ID),
        ),
        ordered(
            "a typed model query",
            Deal::query()
                .where_(Deal::AMOUNT_CENTS.gte(1_000))
                .where_(Deal::STAGE.ne("lost"))
                .latest(),
        ),
        ordered(
            "a model join with a qualified select",
            Deal::query()
                .join("companies", Deal::COMPANY_ID, "=", Company::ID)
                .where_(Company::COUNTRY.eq("US"))
                .order_by(Deal::ID),
        ),
    ];
    let locks = [
        ("lock for update", LockMode::Update, false, false),
        ("shared lock", LockMode::Share, false, false),
        (
            "lock no key update, skip locked",
            LockMode::NoKeyUpdate,
            true,
            false,
        ),
        ("shared lock, no wait", LockMode::Share, false, true),
        ("key share lock", LockMode::KeyShare, false, false),
    ];
    for (name, mode, skip, no_wait) in locks {
        let mut query = table("companies").where_(col("id").eq(1)).lock(mode);
        if skip {
            query = query.skip_locked();
        }
        if no_wait {
            query = query.no_wait();
        }
        cases.push(case(name, Kind::Locked(query.into_query())));
    }
    let aggregates = [
        (
            "count",
            deals().where_(col("stage").eq("won")).order_by("id"),
            count(),
        ),
        ("count of groups", deals().group_by(["stage"]), count()),
        (
            "count of groups with a having",
            deals().group_by(["stage"]).having(Expr::count_all().gt(1)),
            count(),
        ),
        (
            "sum distinct of a limited query",
            deals()
                .order_by_desc("amount_cents")
                .order_by("id")
                .limit(4),
            aggregate(AggregateFunction::Sum, "amount_cents", true),
        ),
        (
            "average",
            deals(),
            aggregate(AggregateFunction::Avg, "amount_cents", false),
        ),
        (
            "minimum",
            deals().where_not_null("closed_at"),
            aggregate(AggregateFunction::Min, "closed_at", false),
        ),
        (
            "maximum",
            deals(),
            aggregate(AggregateFunction::Max, "deals.amount_cents", false),
        ),
        (
            "count distinct",
            deals(),
            aggregate(AggregateFunction::Count, "company_id", true),
        ),
        (
            "count of distinct rows",
            table("companies").select(["active"]).distinct(),
            count(),
        ),
        (
            "count of a union",
            table("companies")
                .select(["name"])
                .union_all(table("users").select(["name"])),
            count(),
        ),
        (
            "count of a limit per group",
            deals().order_by("id").group_limit(1, "company_id"),
            count(),
        ),
        (
            "count of an offset",
            deals().order_by("id").skip(6),
            count(),
        ),
        (
            "sum of nothing",
            deals().where_(col("id").eq(-1)),
            aggregate(AggregateFunction::Sum, "amount_cents", false),
        ),
        (
            "count with a time out",
            deals().timeout(Duration::from_secs(2)),
            count(),
        ),
    ];
    for (name, query, aggregate) in aggregates {
        cases.push(case(
            &format!("aggregate: {name}"),
            Kind::Aggregate(query.into_query(), aggregate),
        ));
    }
    cases.push(case(
        "aggregate: count of a model query",
        Kind::Aggregate(Deal::query().latest().into_query(), count()),
    ));
    let exists = [
        (
            "exists",
            deals().where_(col("stage").eq("won")).order_by("id"),
        ),
        ("exists: none", deals().where_(col("stage").eq("unknown"))),
        (
            "exists: a union",
            deals()
                .select(["id"])
                .where_(col("id").eq(-1))
                .union(deals().select(["id"]).where_(col("id").eq(2))),
        ),
        (
            "exists: with a time out",
            deals().timeout(Duration::from_secs(2)),
        ),
    ];
    for (name, query) in exists {
        cases.push(case(name, Kind::Exists(query.into_query())));
    }
    cases.push(read(
        "exists as a value",
        table("companies").select(["id"]).select_expr(
            IrExpr::Exists(Box::new(
                deals()
                    .where_column("deals.company_id", "=", "companies.id")
                    .into_query(),
            )),
            "has_deals",
        ),
    ));
    // A condition in a document store's own language: refused by both.
    cases.push(read(
        "a document filter",
        table("companies").where_(Filter::from(Where::Native(
            navree_orm::ir::NativeFilter::new("mongodb", "{ status: 'open' }"),
        ))),
    ));
    // A condition built with sea-query: written by sea-query's builder and
    // embedded by the renderer as a raw fragment.
    cases.push(read(
        "a condition built with the query builder",
        table("companies").where_(Filter::from(
            sea_query::Condition::all().add(sea_query::Expr::cust("1 = 1")),
        )),
    ));
    // Where the compiler's SQL is wrong and the renderer's right.
    cases.push(
        read(
            "a json key with a double quote",
            deals()
                .select(["id"])
                .where_(col("meta->we\"ird").eq("x")),
        )
        .differs(&[(
            Dialect::MySql,
            "the compiler writes the path `$.\"we\\\"ird\"` without doubling its backslash, which MySQL's string syntax eats: an invalid JSON path",
        )]),
    );
    cases.push(
        read(
            "a schema-qualified table",
            table("main.scratch").select(["id"]),
        )
        .differs(&[(
            Dialect::Sqlite,
            "the compiler quotes `main.scratch` as one name; the renderer quotes each part",
        )]),
    );
    cases.push(
        read(
            "a union inside a union keeps its grouping",
            deals().select(["stage"]).union_all(
                deals()
                    .select(["stage"])
                    .union(deals().select(["stage"])),
            ),
        )
        .differs(&[(
            Dialect::Sqlite,
            "the compiler writes the inner union bare on SQLite, so the outer UNION ALL reads as part of a three-way union and loses its duplicates",
        )]),
    );
    cases.push(
        read(
            "a union part limited per group",
            table("users")
                .select(["id"])
                .union_all(deals().select(["id"]).group_limit(5, "stage")),
        )
        .differs(&[(
            Dialect::Sqlite,
            "the compiler leaves a part with a per-group limit (and so an ORDER BY of its own) bare on SQLite, where the ORDER BY ends the whole compound",
        )]),
    );
    cases.push(
        ordered(
            "a window ordered by raw SQL with nulls",
            deals()
                .select(["id"])
                .select_expr(
                    IrExpr::RowNumber {
                        partition: Vec::new(),
                        order: vec![
                            navree_orm::ir::Order::raw("closed_at DESC NULLS LAST", Vec::<Value>::new()),
                            col("id").asc(),
                        ],
                    },
                    "position",
                )
                .order_by("id"),
        )
        .differs(&[(
            Dialect::MySql,
            "the compiler writes NULLS LAST inside the window, which MySQL lacks; the renderer sorts the NULLs with a leading IS NULL key",
        )]),
    );
    cases.push(read(
        "a row number",
        deals().select(["id"]).select_expr(
            IrExpr::RowNumber {
                partition: vec![IrExpr::Column(ColumnRef::parse("stage"))],
                order: vec![col("amount_cents").desc().nulls_last(), col("id").asc()],
            },
            "rank",
        ),
    ));
    cases
}

fn val(value: impl Into<Value>) -> IrExpr {
    IrExpr::Value(value.into())
}

fn names(names: &[&'static str]) -> Vec<Cow<'static, str>> {
    names.iter().map(|name| Cow::Borrowed(*name)).collect()
}

fn and(filter: impl Into<Filter>) -> WhereClause {
    WhereClause::and(filter.into().into_where())
}

/// The write cases.
pub fn writes(dialect: Dialect) -> Vec<Case> {
    let mut cases = Vec::new();
    let companies = InsertIr::new(
        "companies",
        names(&["name", "country", "active"]),
        vec![
            vec![val("Hooli"), val(Value::String(None)), val(true)],
            vec![val("Pied Piper"), val("US"), val(false)],
        ],
    );
    cases.push(case("insert rows", Kind::Insert(companies.clone())));
    cases.push(case(
        "insert returning keys",
        Kind::Insert(companies.clone().returning(names(&["id", "name"]))),
    ));
    cases.push(case(
        "insert default values",
        Kind::Insert(InsertIr::new("counters", Vec::new(), Vec::new())),
    ));
    cases.push(case(
        "insert columns without rows",
        Kind::Insert(InsertIr::new("scratch", names(&["name"]), Vec::new())),
    ));
    cases.push(case(
        "insert a row of the wrong length",
        Kind::Insert(InsertIr::new(
            "companies",
            names(&["name", "country"]),
            vec![vec![val("Hooli")]],
        )),
    ));
    let tags = || {
        InsertIr::new(
            "tags",
            names(&["name", "color", "hits", "locked"]),
            vec![
                vec![val("vip"), val("red"), val(2_i64), val(false)],
                vec![val("new"), val("blue"), val(1_i64), val(false)],
            ],
        )
    };
    cases.push(case(
        "insert or ignore",
        Kind::Insert(tags().on_conflict(OnConflict::Ignore { target: vec![] })),
    ));
    cases.push(case(
        "insert or ignore on a target, returning",
        Kind::Insert(
            tags()
                .returning(names(&["name"]))
                .on_conflict(OnConflict::Ignore {
                    target: names(&["name"]),
                }),
        ),
    ));
    cases.push(case(
        "upsert",
        Kind::Insert(tags().on_conflict(OnConflict::Update {
            target: names(&["name"]),
            sets: vec![
                (Cow::Borrowed("color"), Expr::excluded("color")),
                (
                    Cow::Borrowed("hits"),
                    Expr::col("tags.hits").add(Expr::excluded("hits")),
                ),
                (Cow::Borrowed("note"), val("merged")),
            ],
        })),
    ));
    cases.push(case(
        "upsert without a target",
        Kind::Insert(tags().on_conflict(OnConflict::Update {
            target: vec![],
            sets: vec![(Cow::Borrowed("color"), Expr::excluded("color"))],
        })),
    ));
    cases.push(case(
        "upsert without assignments",
        Kind::Insert(tags().on_conflict(OnConflict::Update {
            target: names(&["name"]),
            sets: vec![],
        })),
    ));
    let mut archive = InsertIr::new("archived_deals", names(&["id", "title"]), Vec::new());
    archive.select = Some(Box::new(
        deals()
            .select(["id", "title"])
            .where_(col("stage").eq("won"))
            .into_query(),
    ));
    cases.push(case("insert from a query", Kind::Insert(archive)));
    let mut everything = InsertIr::new("archived_deals", names(&["id", "title"]), Vec::new())
        .on_conflict(OnConflict::Ignore { target: vec![] });
    everything.select = Some(Box::new(deals().select(["id", "title"]).into_query()));
    cases.push(case(
        "insert or ignore from a query",
        Kind::Insert(everything),
    ));
    let mut mismatch = InsertIr::new("archived_deals", names(&["id"]), Vec::new());
    mismatch.select = Some(Box::new(deals().select(["id", "title"]).into_query()));
    cases.push(case(
        "insert from a query of the wrong width",
        Kind::Insert(mismatch),
    ));
    let mut nameless = InsertIr::new("archived_deals", Vec::new(), Vec::new());
    nameless.select = Some(Box::new(deals().select(["id", "title"]).into_query()));
    cases.push(case(
        "insert from a query naming no columns",
        Kind::Insert(nameless),
    ));

    let update = |sets: Vec<(&'static str, IrExpr)>, wheres: Vec<WhereClause>| UpdateIr {
        table: TableRef::new("deals"),
        sets: sets
            .into_iter()
            .map(|(column, value)| (Cow::Borrowed(column), value))
            .collect(),
        wheres,
    };
    cases.push(case(
        "update a column",
        Kind::Update(update(
            vec![("stage", val("won"))],
            vec![and(col("id").eq(2))],
        )),
    ));
    cases.push(case(
        "update with arithmetic",
        Kind::Update(update(
            vec![
                (
                    "amount_cents",
                    Expr::col("amount_cents").add(Expr::val(5_i64)),
                ),
                ("updated_at", val(at(28, 9, 0))),
            ],
            vec![and(col("stage").eq("lead"))],
        )),
    ));
    cases.push(case(
        "update every row",
        Kind::Update(update(vec![("owner_id", val(Value::BigInt(None)))], vec![])),
    ));
    cases.push(case(
        "update json paths",
        Kind::Update(update(
            vec![
                ("meta->status", val("won")),
                ("meta->owner->name", val("ada2")),
                ("title", val("Renamed")),
            ],
            vec![and(col("id").is_in([1_i64, 4]))],
        )),
    ));
    cases.push(case(
        "update a json array element",
        Kind::Update(update(
            vec![("meta->items[0]", val(9_i64))],
            vec![and(col("id").eq(1))],
        )),
    ));
    cases.push(case(
        "update a json path to an object and an expression",
        Kind::Update(update(
            vec![
                ("meta->extra", val(serde_json::json!({"k": [1, true]}))),
                ("meta->title", Expr::col("title")),
            ],
            vec![and(col("id").eq(5))],
        )),
    ));
    cases.push(case(
        "update a column twice",
        Kind::Update(update(
            vec![("meta", val("{}")), ("meta->status", val("won"))],
            vec![],
        )),
    ));
    cases.push(case("update nothing", Kind::Update(update(vec![], vec![]))));
    // A model's mass update with joins: the keys in a materialized derived
    // table (MySQL refuses a subquery on the written table otherwise).
    let mut inner = deals()
        .join("companies", "companies.id", "=", "deals.company_id")
        .where_(col("companies.country").eq("US"))
        .into_query();
    inner.columns = vec![SelectItem {
        expr: IrExpr::Column(ColumnRef::new(Some("deals"), "id")),
        alias: None,
    }];
    inner.distinct = navree_orm::ir::Distinct::Yes;
    let mut keys = QueryIr::new("navree_keys");
    keys.table = TableRef::subquery(inner, "navree_keys");
    keys.columns = vec![SelectItem {
        expr: IrExpr::Column(ColumnRef::new(Some("navree_keys"), "id")),
        alias: None,
    }];
    let by_keys = || {
        vec![WhereClause::and(Where::In {
            expr: IrExpr::Column(ColumnRef::new(Some("deals"), "id")),
            values: InValues::Subquery(Box::new(keys.clone())),
            not: false,
        })]
    };
    cases.push(case(
        "update by keys of a joined query",
        Kind::Update(update(vec![("stage", val("archived"))], by_keys())),
    ));
    cases.push(case(
        "delete by keys of a joined query",
        Kind::Delete(DeleteIr {
            table: TableRef::new("deals"),
            wheres: by_keys(),
        }),
    ));
    // A table's write with an order and a limit: by row id.
    let row_id = match dialect {
        Dialect::Postgres => Some("ctid"),
        Dialect::Sqlite => Some("rowid"),
        Dialect::MySql => None,
    };
    if let Some(row_id) = row_id {
        let mut rows = deals()
            .where_(col("stage").eq("lead"))
            .order_by("created_at")
            .limit(2)
            .into_query();
        rows.columns = vec![SelectItem {
            expr: IrExpr::Column(ColumnRef::new(Some("deals"), row_id)),
            alias: None,
        }];
        let wheres = vec![WhereClause::and(Where::In {
            expr: IrExpr::Column(ColumnRef::new(None::<&str>, row_id)),
            values: InValues::Subquery(Box::new(rows)),
            not: false,
        })];
        cases.push(case(
            "update by row ids, ordered and limited",
            Kind::Update(update(vec![("stage", val("stale"))], wheres.clone())),
        ));
        cases.push(case(
            "delete by row ids, ordered and limited",
            Kind::Delete(DeleteIr {
                table: TableRef::new("deals"),
                wheres,
            }),
        ));
    }

    let joined = |kind: JoinKind, table: TableRef, on: Vec<WhereClause>| Join { kind, table, on };
    cases.push(case(
        "update from a joined table",
        Kind::UpdateFrom {
            table: TableRef::new("deals"),
            sets: vec![(Cow::Borrowed("stage"), val("lost"))],
            joins: vec![joined(
                JoinKind::Inner,
                TableRef::aliased("companies", "c"),
                vec![and(col("c.id").eq_column(&col("deals.company_id")))],
            )],
            wheres: vec![and(col("c.active").eq(false))],
        },
    ));
    cases.push(case(
        "update from a cross join",
        Kind::UpdateFrom {
            table: TableRef::new("deals"),
            sets: vec![(Cow::Borrowed("title"), Expr::col("regions.code"))],
            joins: vec![joined(JoinKind::Cross, TableRef::new("regions"), vec![])],
            wheres: vec![
                and(col("regions.code").eq("EU")),
                and(col("deals.company_id").eq(2)),
            ],
        },
    ));
    cases.push(case(
        "update from two joined tables",
        Kind::UpdateFrom {
            table: TableRef::new("deals"),
            sets: vec![(Cow::Borrowed("stage"), val("checked"))],
            joins: vec![
                joined(
                    JoinKind::Inner,
                    TableRef::new("companies"),
                    vec![and(col("companies.id").eq_column(&col("deals.company_id")))],
                ),
                joined(
                    JoinKind::Inner,
                    TableRef::new("users"),
                    vec![and(col("users.id").eq_column(&col("deals.owner_id")))],
                ),
            ],
            wheres: vec![and(col("users.active").eq(false))],
        },
    ));
    cases.push(case(
        "update from a left join",
        Kind::UpdateFrom {
            table: TableRef::new("deals"),
            sets: vec![(Cow::Borrowed("stage"), val("lost"))],
            joins: vec![joined(
                JoinKind::Left,
                TableRef::new("companies"),
                vec![and(col("companies.id").eq_column(&col("deals.company_id")))],
            )],
            wheres: vec![],
        },
    ));
    cases.push(case(
        "update from a joined table with json paths",
        Kind::UpdateFrom {
            table: TableRef::new("deals"),
            sets: vec![(
                Cow::Borrowed("meta->country"),
                Expr::col("companies.country"),
            )],
            joins: vec![joined(
                JoinKind::Inner,
                TableRef::new("companies"),
                vec![and(col("companies.id").eq_column(&col("deals.company_id")))],
            )],
            wheres: vec![and(col("companies.country").eq("DE"))],
        },
    ));

    cases.push(case(
        "delete matching rows",
        Kind::Delete(DeleteIr {
            table: TableRef::new("deals"),
            wheres: vec![and(col("amount_cents").lt(5_000))],
        }),
    ));
    cases.push(case(
        "delete with a raw condition",
        Kind::Delete(DeleteIr {
            table: TableRef::new("deals"),
            wheres: vec![WhereClause::and(Where::Raw {
                sql: Cow::Borrowed("stage = ? OR title LIKE ?"),
                values: vec![Value::from("lost"), Value::from("%Q1")],
            })],
        }),
    ));
    cases.push(case(
        "delete every row",
        Kind::Delete(DeleteIr {
            table: TableRef::new("scratch"),
            wheres: vec![],
        }),
    ));
    cases.push(case(
        "truncate",
        Kind::Truncate {
            table: "scratch",
            cascade: false,
        },
    ));
    cases.push(case(
        "truncate with cascade",
        Kind::Truncate {
            table: "scratch",
            cascade: true,
        },
    ));
    cases
}
