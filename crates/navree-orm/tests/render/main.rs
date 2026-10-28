//! The SQL navree-orm's own renderer writes, pinned per dialect: every
//! snapshot shows the PostgreSQL (TimescaleDB), MySQL and SQLite statement
//! with its bound values (or the error a dialect answers). `main.rs` pins
//! the syntax tree's grammar; `lowering.rs` the ORM's IR rendered through
//! it (the cases of the `sql` tests).

mod lowering;

use std::fmt::Write as _;

use navree_orm::ir::JsonSegment;
use navree_orm::render::ast::{
    Assignment, BinaryOp, ConflictAction, Cte, Delete, Distinct, Expr, Ident, Insert, InsertSource,
    Join, JoinKind, JsonMode, LikeCase, Lock, LockStrength, LockWait, ObjectName, OnConflict,
    Query, Select, SelectItem, SetExpr, SetOperator, Source, SqlType, Statement, Truncate, Update,
};
use navree_orm::render::{render, render_all, render_expr, render_query};
use navree_orm::{Dialect, Value};

const DIALECTS: [Dialect; 3] = [Dialect::Postgres, Dialect::MySql, Dialect::Sqlite];

/// Each dialect's statement (or error), one block per dialect.
fn per_dialect(render: impl Fn(Dialect) -> navree_orm::Result<navree_orm::Statement>) -> String {
    let mut out = String::new();
    for dialect in DIALECTS {
        match render(dialect) {
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

fn query(query: Query) -> String {
    per_dialect(|dialect| render_query(&query, dialect))
}

fn statement(statement: impl Into<Statement>) -> String {
    let statement = statement.into();
    per_dialect(|dialect| render(&statement, dialect))
}

fn expr(expr: Expr) -> String {
    per_dialect(|dialect| render_expr(&expr, dialect))
}

fn col(name: &'static str) -> Expr {
    Expr::col(name)
}

fn val(value: impl Into<Value>) -> Expr {
    Expr::value(value)
}

fn deals() -> Select {
    Select::from_table("deals")
}

#[test]
fn selects_lists_aliases_and_distinct() {
    insta::assert_snapshot!(query(Query::from(deals())));
    insta::assert_snapshot!(query(Query::from(
        Select::from_source(Source::aliased("deals", "d"))
            .column(Expr::Wildcard(Some(ObjectName::parse("d"))))
            .column_as(col("d.title"), "name")
            .column_as(col("amount_cents").mul(val(2_i64)), "doubled")
            .distinct()
    )));
    insta::assert_snapshot!(query(
        Query::from(Select {
            distinct: Distinct::On(vec![col("company_id")]),
            ..deals().column(col("company_id")).column(col("title"))
        })
        .order_by(col("company_id").asc())
        .order_by(col("created_at").desc())
    ));
    // A SELECT without a table.
    insta::assert_snapshot!(query(Query::from(
        Select::new()
            .column_as(Expr::int(1), "one")
            .column_as(Expr::string("it's"), "text")
    )));
}

#[test]
fn sources_derived_tables_and_table_functions() {
    let best = Query::from(
        deals()
            .column(col("company_id"))
            .column_as(Expr::func("MAX", vec![col("amount_cents")]), "top")
            .group_by(col("company_id")),
    );
    insta::assert_snapshot!(query(Query::from(
        Select::from_source(Source::derived(best, "best")).filter(col("best.top").gt(val(100_i64)))
    )));
    insta::assert_snapshot!(query(Query::from(
        Select::from_source(Source::function(
            Expr::func("generate_series", vec![val(1_i64), val(3_i64)]),
            "n"
        ))
        .column(col("n"))
    )));
    insta::assert_snapshot!(query(Query::from(
        Select::from_table(ObjectName::parse("audit.events"))
            .column(Expr::qualified(ObjectName::parse("audit.events"), "id"))
    )));
    insta::assert_snapshot!(query(Query::from(Select::from_source(Source::Function {
        call: Box::new(col("not_a_call")),
        alias: None
    }))));
}

#[test]
fn joins_of_every_kind() {
    insta::assert_snapshot!(query(Query::from(
        deals()
            .column(Expr::Wildcard(Some(ObjectName::parse("deals"))))
            .join(Join::inner(
                Source::table("companies"),
                col("companies.id").eq(col("deals.company_id"))
            ))
            .join(Join::left(
                Source::aliased("users", "owner"),
                col("owner.id").eq(col("deals.owner_id"))
            ))
            .join(Join {
                kind: JoinKind::Right,
                source: Source::table("regions"),
                on: Some(
                    col("regions.id")
                        .eq(col("companies.region_id"))
                        .or(col("regions.code").eq(col("companies.code")))
                ),
            })
            .join(Join::cross(Source::table("currencies")))
            .join(Join {
                kind: JoinKind::Inner,
                source: Source::table("tags"),
                on: None,
            })
    )));
    insta::assert_snapshot!(query(Query::from(deals().join(Join {
        kind: JoinKind::Full,
        source: Source::table("companies"),
        on: Some(col("companies.id").eq(col("deals.company_id"))),
    }))));
    insta::assert_snapshot!(query(Query::from(deals().join(Join {
        kind: JoinKind::Cross,
        source: Source::table("companies"),
        on: Some(col("companies.id").eq(col("deals.company_id"))),
    }))));
}

#[test]
fn lateral_joins() {
    let latest = || {
        Query::from(
            deals()
                .column(col("deals.title"))
                .filter(col("deals.company_id").eq(col("companies.id"))),
        )
        .order_by(col("deals.created_at").desc())
        .limit(2)
    };
    insta::assert_snapshot!(query(Query::from(
        Select::from_table("companies")
            .column(col("companies.name"))
            .column(col("latest.title"))
            .join(Join {
                kind: JoinKind::Inner,
                source: Source::lateral(latest(), "latest"),
                on: None,
            })
    )));
    insta::assert_snapshot!(query(Query::from(Select::from_table("companies").join(
        Join {
            kind: JoinKind::Left,
            source: Source::lateral(latest(), "latest"),
            on: Some(Expr::bool(true)),
        }
    ))));
}

#[test]
fn conditions_group_with_the_right_parentheses() {
    // (a AND b) OR c, NOT, nested ORs inside ANDs, raw fragments wrapped
    // beside others but bare alone.
    insta::assert_snapshot!(query(Query::from(
        deals().filter(
            col("amount_cents")
                .gt(val(0_i64))
                .and(
                    col("stage")
                        .eq(val("won"))
                        .or(col("stage").eq(val("lost")).and(col("closed_at").is_null()))
                )
                .and(
                    col("title")
                        .eq(val("x"))
                        .or(col("title").eq(val("y")))
                        .not()
                )
                .or(col("amount_cents").lt(val(5_i64)).not())
        )
    )));
    insta::assert_snapshot!(query(Query::from(deals().filter(Expr::raw(
        "lower(title) = ? OR amount_cents = ?",
        [Value::from("acme"), Value::from(5_i64)]
    )))));
    insta::assert_snapshot!(query(Query::from(
        deals()
            .filter(Expr::raw(
                "lower(title) = ? OR amount_cents = ?",
                ["acme", "5"]
            ))
            .filter(col("id").gt(val(1_i64)))
            .filter(Expr::raw("a = ? OR b = ?", [1_i64, 2]).not())
    )));
    // Empty junctions: TRUE and FALSE.
    insta::assert_snapshot!(query(Query::from(
        deals().filter(Expr::And(Vec::new()).and(Expr::Or(Vec::new()).not()))
    )));
}

#[test]
fn groups_havings_orders_and_pages() {
    insta::assert_snapshot!(query(
        Query::from(
            deals()
                .column(col("stage"))
                .column_as(Expr::count_all(), "deals")
                .column_as(Expr::func("SUM", vec![col("amount_cents")]), "total")
                .group_by(col("stage"))
                .having(Expr::count_all().gt(val(2_i64)))
                .having(Expr::raw(
                    "SUM(amount_cents) > ? OR MAX(amount_cents) > ?",
                    [1000_i64, 50]
                ))
        )
        .order_by(col("total").desc())
        .order_by(col("stage").asc().nulls_last())
        .order_by(col("closed_at").desc().nulls_first())
        .order_by(Expr::raw("field(stage, ?, ?)", ["won", "lost"]).desc())
        .order_by(Expr::Random.asc())
        .limit(25)
        .offset(50)
    ));
    insta::assert_snapshot!(query(Query::from(deals()).offset(10)));
    insta::assert_snapshot!(query(Query::from(deals()).limit(u64::MAX)));
}

#[test]
fn unions_with_orders_and_limits() {
    let part = |country: &'static str| {
        Query::from(
            Select::from_table("companies")
                .column(col("name"))
                .filter(col("country").eq(val(country))),
        )
    };
    insta::assert_snapshot!(query(
        part("DE")
            .union(part("US"), false)
            .union(part("FR"), true)
            .order_by(col("name").desc())
            .limit(10)
    ));
    // A part with its own order and limit keeps them: parenthesized, or a
    // derived table on SQLite.
    insta::assert_snapshot!(query(
        part("DE")
            .union(part("US").order_by(col("name").asc()).limit(5), false)
            .order_by(col("name").desc())
    ));
    // Grouping that needs parentheses: a compound on the right, and
    // another operator on the left.
    insta::assert_snapshot!(query(Query::new(SetExpr::Operation {
        op: SetOperator::Union,
        all: false,
        left: Box::new(part("DE").into_set_expr()),
        right: Box::new(part("US").union(part("FR"), true).into_set_expr()),
    })));
    insta::assert_snapshot!(query(
        part("DE")
            .union(part("US"), false)
            .intersect(part("FR"), false)
            .except(part("NZ"), true)
    ));
}

#[test]
fn common_table_expressions() {
    let won = Cte::new(
        "won",
        deals()
            .column(col("id"))
            .column(col("amount_cents"))
            .filter(col("stage").eq(val("won"))),
    );
    insta::assert_snapshot!(query(
        Query::from(
            Select::from_table("won")
                .column_as(Expr::func("SUM", vec![col("amount_cents")]), "total")
        )
        .with([won])
    ));
    // Counting to five, with the seed and the step bound.
    let numbers = Cte::new(
        "numbers",
        Query::from(Select::new().column(val(1_i64))).union(
            Select::from_table("numbers")
                .column(col("n").add(val(1_i64)))
                .filter(col("n").lt(val(5_i64))),
            true,
        ),
    )
    .columns(["n"]);
    insta::assert_snapshot!(query(
        Query::from(Select::from_table("numbers").column(col("n")))
            .with_recursive([numbers])
            .order_by(col("n").asc())
    ));
}

#[test]
fn window_functions() {
    insta::assert_snapshot!(query(Query::from(deals().column(col("id")).column_as(
        Expr::row_number().over(
            vec![col("company_id")],
            vec![col("amount_cents").desc().nulls_last(), col("id").asc()]
        ),
        "rank"
    ))));
    insta::assert_snapshot!(query(Query::from(deals().column_as(
        Expr::func("SUM", vec![col("amount_cents")]).over(Vec::new(), Vec::new()),
        "running"
    ))));
}

#[test]
fn row_locks() {
    let locked = |lock: Lock| {
        query(
            Query::from(deals().filter(col("id").eq(val(1_i64))))
                .limit(1)
                .lock(lock),
        )
    };
    insta::assert_snapshot!(locked(Lock::new(LockStrength::Update)));
    insta::assert_snapshot!(locked(Lock {
        strength: LockStrength::NoKeyUpdate,
        of: Vec::new(),
        wait: LockWait::SkipLocked,
    }));
    insta::assert_snapshot!(locked(Lock::new(LockStrength::Share)));
    insta::assert_snapshot!(locked(Lock {
        strength: LockStrength::KeyShare,
        of: vec![ObjectName::parse("deals")],
        wait: LockWait::NoWait,
    }));
}

#[test]
fn execution_time_hints() {
    let mut timed = Query::from(deals().column(col("id"))).union(
        Select::from_table("archived_deals").column(col("id")),
        false,
    );
    timed.max_execution_time = Some(1500);
    insta::assert_snapshot!(query(timed));
}

#[test]
fn values_raw_fragments_and_templates() {
    insta::assert_snapshot!(expr(
        col("a")
            .eq(val(1_i64))
            .and(Expr::raw(
                "data ?? 'k' AND note <> '?' AND price > ?",
                [5_i64]
            ))
            .and(Expr::template(
                "COALESCE({}, '{}', {})",
                vec![col("meta"), val("x")]
            ))
    ));
    insta::assert_snapshot!(expr(Expr::raw("a = ?", [1_i64, 2])));
    insta::assert_snapshot!(expr(Expr::template("f({}, {})", vec![col("a")])));
    insta::assert_snapshot!(expr(Expr::template("{} ? 'k'", vec![col("meta")])));
}

#[test]
fn functions_and_aggregates() {
    insta::assert_snapshot!(expr(Expr::func(
        "coalesce",
        vec![
            col("closed_at"),
            Expr::func_distinct("COUNT", vec![col("owner_id")]),
            Expr::count_all(),
            Expr::func("public.score", vec![]),
        ]
    )));
    insta::assert_snapshot!(expr(Expr::func("drop table x; --", vec![])));
}

#[test]
fn arithmetic_keeps_its_precedence() {
    // (a + b) * c, a - (b - c), a / (b * c), -(a + b), a - -5.
    insta::assert_snapshot!(expr(Expr::Tuple(vec![
        col("a").add(col("b")).mul(col("c")),
        col("a").sub(col("b").sub(col("c"))),
        col("a").sub(col("b")).sub(col("c")),
        col("a").div(col("b").mul(col("c"))),
        Expr::Neg(Box::new(col("a").add(col("b")))),
        col("a").sub(Expr::int(-5)),
        col("a").add(col("b")).eq(col("c").mul(Expr::int(2))),
        col("a").eq(col("b")).eq(Expr::bool(false)),
    ])));
    insta::assert_snapshot!(expr(Expr::Tuple(vec![
        col("flags").binary(BinaryOp::BitAnd, Expr::int(4)),
        col("flags").binary(BinaryOp::BitOr, col("a").add(col("b"))),
        col("a").binary(BinaryOp::BitXor, col("b")),
        col("a").binary(BinaryOp::ShiftLeft, Expr::int(2)),
        col("a").binary(BinaryOp::Mod, Expr::int(7)),
        col("first").binary(BinaryOp::Concat, col("last")),
    ])));
}

#[test]
fn comparisons_and_predicates() {
    insta::assert_snapshot!(expr(Expr::And(vec![
        col("a").ne(val(1_i64)),
        col("a").lte(val(2_i64)),
        col("a").gte(val(3_i64)),
        col("b").is_null(),
        col("c").is_not_null(),
        col("d").in_list(vec![val("x"), val("y")]),
        col("e").not_in_list(vec![val(1_i64)]),
        col("f").between(val(1_i64), val(9_i64)),
        Expr::Between {
            expr: Box::new(col("g")),
            low: Box::new(col("lo").add(Expr::int(1))),
            high: Box::new(col("hi")),
            negated: true,
        },
        col("h").binary(BinaryOp::NullSafeEq, val("x")),
        col("i").binary(BinaryOp::NullSafeNotEq, val("x")),
    ])));
    // An empty list is still SQL.
    insta::assert_snapshot!(expr(
        col("a")
            .in_list(Vec::new())
            .or(col("b").not_in_list(Vec::new()))
    ));
    insta::assert_snapshot!(expr(
        Expr::Tuple(vec![col("a"), col("b")])
            .in_list(vec![Expr::Tuple(vec![val(1_i64), val(2_i64)])])
    ));
}

#[test]
fn subqueries_and_exists() {
    let owners = Query::from(
        Select::from_table("users")
            .column(col("id"))
            .filter(col("active").eq(val(true))),
    );
    insta::assert_snapshot!(query(Query::from(
        deals()
            .column_as(
                Expr::Subquery(Box::new(Query::from(
                    Select::from_table("companies")
                        .column(col("companies.name"))
                        .filter(col("companies.id").eq(col("deals.company_id")))
                ))),
                "company"
            )
            .filter(col("owner_id").in_query(owners.clone()))
            .filter(Expr::InQuery {
                expr: Box::new(col("owner_id")),
                query: Box::new(owners.clone()),
                negated: true,
            })
            .filter(Expr::exists(owners.clone()))
            .filter(Expr::Exists {
                query: Box::new(owners),
                negated: true,
            })
    )));
}

#[test]
fn like_patterns() {
    let like = |case: LikeCase, negated: bool, escape: Option<char>| Expr::Like {
        expr: Box::new(col("title")),
        pattern: Box::new(val("50\\%%")),
        escape,
        negated,
        case,
    };
    insta::assert_snapshot!(expr(Expr::And(vec![
        like(LikeCase::Database, false, None),
        like(LikeCase::Database, true, Some('\\')),
        like(LikeCase::Ignore, false, Some('!')),
        like(LikeCase::Ignore, true, None),
    ])));
    insta::assert_snapshot!(expr(Expr::Like {
        expr: Box::new(col("title")),
        pattern: Box::new(val("Q_ 50%*")),
        escape: None,
        negated: false,
        case: LikeCase::Match,
    }));
    insta::assert_snapshot!(expr(like(LikeCase::Match, true, Some('\\'))));
}

#[test]
fn casts() {
    insta::assert_snapshot!(expr(Expr::Tuple(vec![
        col("a").cast(SqlType::Text),
        col("a").cast(SqlType::Integer),
        col("a").cast(SqlType::Double),
        col("a").cast(SqlType::Decimal {
            precision: 12,
            scale: 2
        }),
        col("a").cast(SqlType::Date),
        col("a").cast(SqlType::Time),
        col("a").cast(SqlType::DateTime),
        col("a").cast(SqlType::Json),
        col("a").cast(SqlType::Custom("vector".into())),
    ])));
    insta::assert_snapshot!(expr(
        col("a").cast(SqlType::Custom("int); DROP TABLE x".into()))
    ));
}

#[test]
fn case_expressions() {
    insta::assert_snapshot!(expr(Expr::case(
        vec![
            (col("amount_cents").gt(val(1000_i64)), Expr::string("big")),
            (col("amount_cents").gt(val(0_i64)), Expr::string("small")),
        ],
        Some(Expr::string("none"))
    )));
    insta::assert_snapshot!(expr(Expr::Case {
        operand: Some(Box::new(col("stage"))),
        whens: vec![(val("won"), Expr::int(1))],
        otherwise: None,
    }));
    insta::assert_snapshot!(expr(Expr::case(Vec::new(), None)));
}

#[test]
fn json_paths() {
    let path = vec![
        JsonSegment::Key("owner".into()),
        JsonSegment::Key("it's \"odd\"".into()),
        JsonSegment::Index(0),
        JsonSegment::Index(-2),
    ];
    insta::assert_snapshot!(expr(Expr::Tuple(vec![
        col("meta").json_text(path.clone()),
        col("meta").json(path),
        col("meta").json_text(vec![JsonSegment::Key("a\\b".into())]),
        col("meta")
            .json(vec![JsonSegment::Key("tags".into())])
            .eq(val("[]")),
        col("meta").json_text(Vec::new()),
    ])));
}

#[test]
fn upsert_references_and_operators_written_as_is() {
    insta::assert_snapshot!(expr(Expr::Tuple(vec![
        Expr::Excluded(Ident::new("color")),
        col("tags").binary(BinaryOp::Custom("@>".into()), val("[\"vip\"]")),
        col("title").binary(BinaryOp::Custom("SOUNDS LIKE".into()), val("x")),
    ])));
    insta::assert_snapshot!(expr(
        col("meta").binary(BinaryOp::Custom("?|".into()), val("k"))
    ));
    insta::assert_snapshot!(expr(
        col("a").binary(BinaryOp::Custom("= 1; DROP TABLE x".into()), val(1_i64))
    ));
}

#[test]
fn inserts() {
    let rows = Insert::values(
        "companies",
        ["name", "country"],
        vec![
            vec![val("Acme"), val(Value::String(None))],
            vec![val("Globex"), Expr::Default],
        ],
    );
    insta::assert_snapshot!(statement(rows.clone()));
    insta::assert_snapshot!(statement(Insert {
        returning: vec![
            SelectItem::new(col("id")),
            SelectItem::aliased(col("name"), "label")
        ],
        ..Insert::values("companies", ["name"], vec![vec![val("Acme")]])
    }));
    insta::assert_snapshot!(statement(Insert {
        table: ObjectName::parse("counters"),
        columns: Vec::new(),
        source: InsertSource::DefaultValues,
        on_conflict: None,
        returning: Vec::new(),
    }));
    insta::assert_snapshot!(statement(Insert::values(
        "companies",
        ["name", "country"],
        vec![vec![val("Acme")]]
    )));
    insta::assert_snapshot!(statement(Insert::values("companies", ["name"], Vec::new())));
}

#[test]
fn inserts_from_queries() {
    let archive = |conflict: Option<OnConflict>, filter: bool| {
        let mut select = deals().column(col("id")).column(col("title"));
        if filter {
            select = select.filter(col("stage").eq(val("lost")));
        }
        Insert {
            table: ObjectName::parse("archived_deals"),
            columns: vec![Ident::new("id"), Ident::new("title")],
            source: InsertSource::Query(Box::new(Query::from(select))),
            on_conflict: conflict,
            returning: Vec::new(),
        }
    };
    let ignore = || {
        Some(OnConflict {
            target: Vec::new(),
            action: ConflictAction::Ignore,
        })
    };
    insta::assert_snapshot!(statement(archive(None, true)));
    // SQLite needs a WHERE before the conflict clause.
    insta::assert_snapshot!(statement(archive(ignore(), false)));
    insta::assert_snapshot!(statement(archive(ignore(), true)));
    let mut union = archive(ignore(), false);
    union.source = InsertSource::Query(Box::new(
        Query::from(deals().column(col("id")).column(col("title"))).union(
            Select::from_table("old_deals")
                .column(col("id"))
                .column(col("title")),
            true,
        ),
    ));
    insta::assert_snapshot!(statement(union));
}

#[test]
fn upserts() {
    let tag = || {
        Insert::values(
            "tags",
            ["name", "color"],
            vec![vec![val("vip"), val("gold")]],
        )
    };
    insta::assert_snapshot!(statement(Insert {
        on_conflict: Some(OnConflict {
            target: vec![Ident::new("name")],
            action: ConflictAction::Ignore,
        }),
        returning: vec![SelectItem::new(col("id"))],
        ..tag()
    }));
    insta::assert_snapshot!(statement(Insert {
        on_conflict: Some(OnConflict {
            target: vec![Ident::new("name")],
            action: ConflictAction::Update {
                sets: vec![
                    Assignment::new("color", Expr::Excluded(Ident::new("color"))),
                    Assignment::new(
                        "hits",
                        col("tags.hits").add(Expr::Excluded(Ident::new("hits")))
                    ),
                    Assignment::new("note", val("merged")),
                ],
                filter: None,
            },
        }),
        ..tag()
    }));
    // A condition on the update: PostgreSQL and SQLite only.
    insta::assert_snapshot!(statement(Insert {
        on_conflict: Some(OnConflict {
            target: vec![Ident::new("name")],
            action: ConflictAction::Update {
                sets: vec![Assignment::new(
                    "color",
                    Expr::Excluded(Ident::new("color"))
                )],
                filter: Some(col("tags.locked").eq(val(false))),
            },
        }),
        ..tag()
    }));
    // Without a target: PostgreSQL refuses an update.
    insta::assert_snapshot!(statement(Insert {
        on_conflict: Some(OnConflict {
            target: Vec::new(),
            action: ConflictAction::Update {
                sets: vec![Assignment::new(
                    "color",
                    Expr::Excluded(Ident::new("color"))
                )],
                filter: None,
            },
        }),
        ..tag()
    }));
}

#[test]
fn updates() {
    insta::assert_snapshot!(statement(
        Update::new(
            "deals",
            vec![
                Assignment::new("stage", val("won")),
                Assignment::new("amount_cents", col("amount_cents").add(val(5_i64))),
            ]
        )
        .filter(col("id").eq(val(7_i64)))
    ));
    insta::assert_snapshot!(statement(Update {
        alias: Some(Ident::new("d")),
        from: vec![Source::aliased("companies", "c")],
        returning: vec![SelectItem::new(col("d.id"))],
        ..Update::new("deals", vec![Assignment::new("stage", val("lost"))])
            .filter(col("c.id").eq(col("d.company_id")))
            .filter(col("c.active").eq(val(false)))
    }));
    insta::assert_snapshot!(statement(Update {
        order_by: vec![col("created_at").asc()],
        limit: Some(100),
        ..Update::new("sessions", vec![Assignment::new("expired", val(true))])
            .filter(col("user_id").eq(val(7_i64)))
    }));
    insta::assert_snapshot!(statement(Update::new("deals", Vec::new())));
}

#[test]
fn deletes() {
    insta::assert_snapshot!(statement(
        Delete::new("deals").filter(col("amount_cents").lt(val(0_i64)))
    ));
    insta::assert_snapshot!(statement(Delete {
        alias: Some(Ident::new("d")),
        using: vec![Source::aliased("companies", "c")],
        ..Delete::new("deals")
            .filter(col("c.id").eq(col("d.company_id")))
            .filter(col("c.active").eq(val(false)))
    }));
    insta::assert_snapshot!(statement(Delete {
        order_by: vec![col("last_seen_at").asc()],
        limit: Some(100),
        ..Delete::new("sessions").filter(col("user_id").eq(val(7_i64)))
    }));
    insta::assert_snapshot!(statement(Delete {
        returning: vec![SelectItem::new(col("id"))],
        ..Delete::new("sessions")
    }));
}

#[test]
fn truncation() {
    let mut out = String::new();
    for (table, cascade) in [("deals", false), ("audit.events", true)] {
        let truncate = Statement::Truncate(Truncate {
            table: ObjectName::parse(table),
            restart_identity: true,
            cascade,
        });
        for dialect in DIALECTS {
            for statement in render_all(&truncate, dialect).unwrap() {
                writeln!(out, "{dialect}: {statement}").unwrap();
            }
            if let Err(error) = render(&truncate, dialect) {
                writeln!(out, "{dialect}: render alone: {error}").unwrap();
            }
        }
    }
    insta::assert_snapshot!(out);
}

#[test]
fn odd_identifiers_and_literals_are_quoted() {
    insta::assert_snapshot!(query(Query::from(
        Select::from_source(Source::aliased(
            ObjectName(vec![Ident::new("my\"schema"), Ident::new("we`ird table")]),
            "a.b"
        ))
        .column_as(
            Expr::qualified(ObjectName::single("a.b"), "col\"umn"),
            "it's"
        )
        .filter(col("x").eq(Expr::string("it's a \\ backslash")))
    )));
}

#[test]
fn placeholders_number_across_nested_queries() {
    let inner = |n: i64| {
        Query::from(
            Select::from_table("t")
                .column(col("id"))
                .filter(col("n").eq(val(n))),
        )
    };
    let cte = Cte::new("c", inner(1));
    let body = Query::from(
        Select::from_table("c")
            .column(col("id"))
            .column_as(Expr::raw("? + ?", [2_i64, 3]), "sum")
            .filter(col("id").in_query(inner(4)))
            .filter(Expr::exists(inner(5))),
    )
    .union(inner(6).limit(7), false)
    .with([cte])
    .limit(8);
    insta::assert_snapshot!(query(body));
}

/// Every snapshot above renders without panicking; this one checks the
/// AST's own conveniences.
#[test]
fn builders_flatten_junctions() {
    let flat = col("a")
        .eq(val(1_i64))
        .and(col("b").eq(val(2_i64)))
        .and(col("c").eq(val(3_i64)));
    assert!(matches!(&flat, Expr::And(items) if items.len() == 3));
    let mode = col("meta").json(vec![JsonSegment::Key("a".into())]);
    assert!(matches!(
        mode,
        Expr::JsonPath {
            mode: JsonMode::Json,
            ..
        }
    ));
}
