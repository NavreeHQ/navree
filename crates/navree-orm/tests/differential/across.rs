//! Statements built as syntax trees, including what the ORM does not emit
//! yet (CTEs, CASE, window functions, upserts with a condition, compounds
//! other than UNION): rendered for each dialect, they return the same rows
//! everywhere, the rows written here.

use navree_orm::Dialect;
use navree_orm::ir::JsonSegment;
use navree_orm::render::ast::{
    Assignment, BinaryOp, ConflictAction, Cte, Delete, Expr, Ident, Insert, InsertSource, Join,
    LikeCase, Lock, LockStrength, ObjectName, OnConflict, Query, Select, SelectItem, SqlType,
    Source, Statement, Truncate, Update,
};
use serde_json::{Value as Json, json};

/// A statement (or a few) and what it must return.
pub struct Portable {
    pub name: &'static str,
    /// Run in order; a lone query's rows are the result.
    pub steps: Vec<Statement>,
    /// After writes, the query whose rows are the result.
    pub check: Option<Query>,
    /// The rows, each a JSON array of its values in order.
    pub expect: Json,
    /// The dialects that refuse to render it.
    pub refused_on: &'static [Dialect],
    /// Read inside a transaction (row locks).
    pub locked: bool,
}

fn query(name: &'static str, query: Query, expect: Json) -> Portable {
    Portable {
        name,
        steps: vec![Statement::from(query)],
        check: None,
        expect,
        refused_on: &[],
        locked: false,
    }
}

fn write(name: &'static str, steps: Vec<Statement>, check: Query, expect: Json) -> Portable {
    Portable {
        name,
        steps,
        check: Some(check),
        expect,
        refused_on: &[],
        locked: false,
    }
}

fn refused_on(mut portable: Portable, dialects: &'static [Dialect]) -> Portable {
    portable.refused_on = dialects;
    portable
}

fn col(name: &'static str) -> Expr {
    Expr::col(name)
}

fn val(value: impl Into<navree_orm::Value>) -> Expr {
    Expr::value(value)
}

fn from(table: &'static str) -> Select {
    Select::from_table(table)
}

/// Deals that are not soft-deleted.
fn live() -> Expr {
    col("deals.deleted_at").is_null()
}

pub fn statements() -> Vec<Portable> {
    vec![
        query(
            "a common table expression",
            Query::from(
                from("won")
                    .column_as(Expr::count_all(), "n")
                    .column_as(Expr::func("SUM", vec![col("amount_cents")]), "total"),
            )
            .with([Cte::new(
                "won",
                from("deals")
                    .column(col("id"))
                    .column(col("amount_cents"))
                    .filter(col("stage").eq(val("won")).and(live())),
            )]),
            json!([[2, 370000]]),
        ),
        query(
            "a recursive common table expression",
            Query::from(from("numbers").column(col("n")))
                .with_recursive([Cte::new(
                    "numbers",
                    Query::from(Select::new().column(Expr::int(1))).union(
                        from("numbers")
                            .column(col("n").add(Expr::int(1)))
                            .filter(col("n").lt(Expr::int(5))),
                        true,
                    ),
                )
                .columns(["n"])])
                .order_by(col("n").asc()),
            json!([[1], [2], [3], [4], [5]]),
        ),
        query(
            "case",
            Query::from(
                from("deals").column(col("id")).column_as(
                    Expr::case(
                        vec![
                            (col("amount_cents").gte(val(100_000_i64)), Expr::string("big")),
                            (col("amount_cents").gt(val(0_i64)), Expr::string("small")),
                        ],
                        Some(Expr::string("none")),
                    ),
                    "size",
                ),
            )
            .order_by(col("id").asc()),
            json!([
                [1, "big"],
                [2, "small"],
                [3, "small"],
                [4, "none"],
                [5, "big"],
                [6, "small"],
                [7, "small"],
                [8, "big"]
            ]),
        ),
        query(
            "a simple case",
            Query::from(
                from("deals").column(col("id")).column_as(
                    Expr::Case {
                        operand: Some(Box::new(col("stage"))),
                        whens: vec![(val("won"), Expr::int(1)), (val("lost"), Expr::int(-1))],
                        otherwise: Some(Box::new(Expr::int(0))),
                    },
                    "score",
                ),
            )
            .order_by(col("id").asc())
            .limit(4),
            json!([[1, 1], [2, 0], [3, -1], [4, 0]]),
        ),
        query(
            "a window function",
            Query::from(
                from("deals").column(col("id")).column_as(
                    Expr::row_number()
                        .over(vec![col("stage")], vec![col("amount_cents").desc()]),
                    "rank",
                ),
            )
            .order_by(col("id").asc()),
            json!([[1, 2], [2, 2], [3, 1], [4, 3], [5, 1], [6, 2], [7, 3], [8, 1]]),
        ),
        query(
            "a window ordered with nulls last",
            Query::from(
                from("deals").column(col("id")).column_as(
                    Expr::row_number().over(Vec::new(), vec![
                        col("closed_at").desc().nulls_last(),
                        col("id").asc(),
                    ]),
                    "position",
                ),
            )
            .order_by(col("id").asc()),
            json!([[1, 2], [2, 5], [3, 3], [4, 6], [5, 1], [6, 4], [7, 7], [8, 8]]),
        ),
        query(
            "nulls last, descending",
            Query::from(from("deals").column(col("id")))
                .order_by(col("closed_at").desc().nulls_last())
                .order_by(col("id").asc()),
            json!([[5], [1], [3], [6], [2], [4], [7], [8]]),
        ),
        query(
            "nulls first, ascending",
            Query::from(from("deals").column(col("id")))
                .order_by(col("closed_at").asc().nulls_first())
                .order_by(col("id").asc()),
            json!([[2], [4], [7], [8], [6], [3], [1], [5]]),
        ),
        refused_on(
            write(
                "an upsert with a condition",
                vec![Statement::from(Insert {
                    on_conflict: Some(OnConflict {
                        target: vec![Ident::new("name")],
                        action: ConflictAction::Update {
                            sets: vec![
                                Assignment::new("color", Expr::Excluded(Ident::new("color"))),
                                Assignment::new(
                                    "hits",
                                    col("tags.hits").add(Expr::Excluded(Ident::new("hits"))),
                                ),
                            ],
                            filter: Some(col("tags.locked").eq(val(false))),
                        },
                    }),
                    ..Insert::values(
                        "tags",
                        ["name", "color", "hits", "locked"],
                        vec![
                            vec![val("vip"), val("red"), val(2_i64), val(false)],
                            vec![val("spam"), val("pink"), val(5_i64), val(false)],
                        ],
                    )
                })],
                Query::from(
                    from("tags")
                        .column(col("name"))
                        .column(col("color"))
                        .column(col("hits")),
                )
                .order_by(col("id").asc()),
                json!([["vip", "red", 5], ["spam", "grey", 0]]),
            ),
            &[Dialect::MySql],
        ),
        write(
            "an upsert",
            vec![Statement::from(Insert {
                on_conflict: Some(OnConflict {
                    target: vec![Ident::new("name")],
                    action: ConflictAction::Update {
                        sets: vec![Assignment::new(
                            "hits",
                            col("tags.hits").add(Expr::Excluded(Ident::new("hits"))),
                        )],
                        filter: None,
                    },
                }),
                ..Insert::values(
                    "tags",
                    ["name", "color", "hits", "locked"],
                    vec![
                        vec![val("vip"), val("red"), val(2_i64), val(false)],
                        vec![val("fresh"), val("blue"), val(1_i64), val(false)],
                    ],
                )
            })],
            Query::from(
                from("tags")
                    .column(col("name"))
                    .column(col("color"))
                    .column(col("hits")),
            )
            .order_by(col("name").asc()),
            json!([["fresh", "blue", 1], ["spam", "grey", 0], ["vip", "gold", 5]]),
        ),
        write(
            "an insert skipping conflicts",
            vec![Statement::from(Insert {
                on_conflict: Some(OnConflict {
                    target: Vec::new(),
                    action: ConflictAction::Ignore,
                }),
                ..Insert::values(
                    "tags",
                    ["name", "color", "hits", "locked"],
                    vec![
                        vec![val("vip"), val("red"), val(2_i64), val(false)],
                        vec![val("fresh"), val("blue"), val(1_i64), val(false)],
                    ],
                )
            })],
            Query::from(from("tags").column(col("name")).column(col("color")))
                .order_by(col("name").asc()),
            json!([["fresh", "blue"], ["spam", "grey"], ["vip", "gold"]]),
        ),
        refused_on(
            query(
                "an insert returning its rows",
                Query::from(Select::new()),
                json!([[4, "d"], [5, "e"]]),
            )
            .with_steps(vec![Statement::from(Insert {
                returning: vec![SelectItem::new(col("id")), SelectItem::new(col("name"))],
                ..Insert::values("scratch", ["name"], vec![vec![val("d")], vec![val("e")]])
            })]),
            &[Dialect::MySql],
        ),
        write(
            "an insert of default values",
            vec![Statement::from(Insert {
                table: ObjectName::parse("counters"),
                columns: Vec::new(),
                source: InsertSource::DefaultValues,
                on_conflict: None,
                returning: Vec::new(),
            })],
            Query::from(from("counters").column(col("id")).column(col("hits"))),
            json!([[1, 0]]),
        ),
        write(
            "an insert from a query",
            vec![Statement::from(Insert {
                table: ObjectName::parse("archived_deals"),
                columns: vec![Ident::new("id"), Ident::new("title")],
                source: InsertSource::Query(Box::new(Query::from(
                    from("deals")
                        .column(col("id"))
                        .column(col("title"))
                        .filter(col("stage").eq(val("won"))),
                ))),
                on_conflict: Some(OnConflict {
                    target: Vec::new(),
                    action: ConflictAction::Ignore,
                }),
                returning: Vec::new(),
            })],
            Query::from(from("archived_deals").column(col("id"))).order_by(col("id").asc()),
            json!([[1], [3], [5], [7]]),
        ),
        write(
            "an update from another table",
            vec![Statement::from(Update {
                from: vec![Source::aliased("companies", "c")],
                ..Update::new("deals", vec![Assignment::new("stage", val("frozen"))])
                    .filter(col("c.id").eq(col("deals.company_id")))
                    .filter(col("c.active").eq(val(false)))
            })],
            Query::from(
                from("deals")
                    .column(col("id"))
                    .filter(col("stage").eq(val("frozen"))),
            ),
            json!([[5]]),
        ),
        refused_on(
            write(
                "an update with an order and a limit",
                vec![Statement::from(Update {
                    order_by: vec![col("amount_cents").desc()],
                    limit: Some(1),
                    ..Update::new("deals", vec![Assignment::new("stage", val("top"))])
                })],
                Query::from(
                    from("deals")
                        .column(col("id"))
                        .filter(col("stage").eq(val("top"))),
                ),
                json!([[8]]),
            ),
            &[Dialect::Postgres, Dialect::Sqlite],
        ),
        refused_on(
            write(
                "a delete matching another table",
                vec![Statement::from(Delete {
                    alias: Some(Ident::new("d")),
                    using: vec![Source::aliased("companies", "c")],
                    ..Delete::new("deals")
                        .filter(col("c.id").eq(col("d.company_id")))
                        .filter(col("c.country").eq(val("DE")))
                })],
                Query::from(from("deals").column(col("id"))).order_by(col("id").asc()),
                json!([[1], [2], [5], [6], [7], [8]]),
            ),
            &[Dialect::Sqlite],
        ),
        refused_on(
            write(
                "a delete with an order and a limit",
                vec![Statement::from(Delete {
                    order_by: vec![col("created_at").asc()],
                    limit: Some(2),
                    ..Delete::new("deals")
                })],
                Query::from(from("deals").column(col("id"))).order_by(col("id").asc()),
                json!([[1], [2], [3], [4], [5], [8]]),
            ),
            &[Dialect::Postgres, Dialect::Sqlite],
        ),
        write(
            "emptying a table restarts its identity",
            vec![
                Statement::Truncate(Truncate {
                    table: ObjectName::parse("scratch"),
                    restart_identity: true,
                    cascade: false,
                }),
                Statement::from(Insert::values("scratch", ["name"], vec![vec![val("x")]])),
            ],
            Query::from(from("scratch").column(col("id")).column(col("name"))),
            json!([[1, "x"]]),
        ),
        query(
            "a left join counted per group",
            Query::from(
                Select::from_source(Source::aliased("companies", "c"))
                    .column(col("c.id"))
                    .column_as(Expr::func("COUNT", vec![col("d.id")]), "deals")
                    .join(Join::left(
                        Source::aliased("deals", "d"),
                        col("d.company_id")
                            .eq(col("c.id"))
                            .and(col("d.deleted_at").is_null()),
                    ))
                    .group_by(col("c.id")),
            )
            .order_by(col("c.id").asc()),
            json!([[1, 3], [2, 2], [3, 1], [4, 0]]),
        ),
        query(
            "a union of ordered and limited parts",
            Query::from(
                from("companies")
                    .column(col("name"))
                    .filter(col("country").is_not_null()),
            )
            .order_by(col("id").asc())
            .limit(2)
            .union(
                Query::from(from("users").column(col("name")))
                    .order_by(col("id").desc())
                    .limit(1),
                true,
            )
            .order_by(col("name").asc()),
            json!([["Acme"], ["Globex"], ["Linus"]]),
        ),
        query(
            "intersect",
            Query::from(
                from("deals")
                    .column(col("company_id"))
                    .filter(col("stage").eq(val("won"))),
            )
            .intersect(
                from("deals")
                    .column(col("company_id"))
                    .filter(col("stage").eq(val("lead"))),
                false,
            )
            .order_by(col("company_id").asc()),
            json!([[1]]),
        ),
        query(
            "except",
            Query::from(
                from("deals")
                    .column(col("company_id"))
                    .filter(col("stage").eq(val("won"))),
            )
            .except(
                from("deals")
                    .column(col("company_id"))
                    .filter(col("stage").eq(val("lead"))),
                false,
            )
            .order_by(col("company_id").asc()),
            json!([[3], [4]]),
        ),
        query(
            "in a subquery and not exists",
            Query::from(
                from("companies")
                    .column(col("id"))
                    .filter(
                        col("id").in_query(
                            from("deals")
                                .column(col("company_id"))
                                .filter(col("stage").eq(val("lost"))),
                        ),
                    )
                    .filter(Expr::Exists {
                        query: Box::new(Query::from(
                            from("deals")
                                .column(Expr::int(1))
                                .filter(col("deals.company_id").eq(col("companies.id")))
                                .filter(col("deals.stage").eq(val("won"))),
                        )),
                        negated: true,
                    }),
            ),
            json!([[2]]),
        ),
        query(
            "exists as a value",
            Query::from(
                from("companies").column(col("id")).column_as(
                    Expr::exists(
                        from("deals")
                            .column(Expr::int(1))
                            .filter(col("deals.company_id").eq(col("companies.id")))
                            .filter(col("deals.stage").eq(val("won"))),
                    ),
                    "has_won",
                ),
            )
            .order_by(col("id").asc()),
            json!([[1, 1], [2, 0], [3, 1], [4, 1]]),
        ),
        query(
            "a json path read as text",
            Query::from(
                from("deals")
                    .column(col("id"))
                    .column_as(
                        col("meta").json_text(vec![
                            JsonSegment::Key("owner".into()),
                            JsonSegment::Key("name".into()),
                        ]),
                        "owner",
                    )
                    .filter(
                        col("meta")
                            .json_text(vec![
                                JsonSegment::Key("owner".into()),
                                JsonSegment::Key("name".into()),
                            ])
                            .is_not_null(),
                    ),
            )
            .order_by(col("id").asc()),
            json!([[1, "ada"], [2, "grace"], [5, "linus"], [8, "ada"]]),
        ),
        query(
            "json array elements from the end",
            Query::from(
                from("deals")
                    .column(col("id"))
                    .column_as(col("tags").json_text(vec![JsonSegment::Index(-1)]), "last")
                    .filter(col("id").in_list(vec![val(1_i64), val(8_i64)])),
            )
            .order_by(col("id").asc()),
            json!([[1, "q1"], [8, "vip"]]),
        ),
        query(
            "like with an escape character",
            Query::from(from("deals").column(col("id")).filter(Expr::Like {
                expr: Box::new(col("title")),
                pattern: Box::new(val("%50!%%")),
                escape: Some('!'),
                negated: false,
                case: LikeCase::Database,
            })),
            json!([[2]]),
        ),
        query(
            "like ignoring case",
            Query::from(from("deals").column(col("id")).filter(Expr::Like {
                expr: Box::new(col("title")),
                pattern: Box::new(val("renewal%")),
                escape: None,
                negated: false,
                case: LikeCase::Ignore,
            }))
            .order_by(col("id").asc()),
            json!([[1], [3]]),
        ),
        query(
            "like matching case",
            Query::from(from("deals").column(col("id")).filter(Expr::Like {
                expr: Box::new(col("title")),
                pattern: Box::new(val("renewal%")),
                escape: None,
                negated: false,
                case: LikeCase::Match,
            })),
            json!([[3]]),
        ),
        query(
            "like matching case with wildcards that glob reads",
            Query::from(from("deals").column(col("id")).filter(Expr::Like {
                expr: Box::new(col("title")),
                pattern: Box::new(val("Orphan_d%")),
                escape: None,
                negated: false,
                case: LikeCase::Match,
            })),
            json!([[6]]),
        ),
        query(
            "null-safe equality with a null",
            Query::from(
                from("companies").column(col("id")).filter(
                    col("country").binary(BinaryOp::NullSafeEq, val(navree_orm::Value::String(None))),
                ),
            ),
            json!([[3]]),
        ),
        query(
            "null-safe inequality",
            Query::from(
                from("companies")
                    .column(col("id"))
                    .filter(col("country").binary(BinaryOp::NullSafeNotEq, val("US"))),
            )
            .order_by(col("id").asc()),
            json!([[2], [3], [4]]),
        ),
        query(
            "casts",
            Query::from(
                from("deals")
                    .column_as(col("amount_cents").cast(SqlType::Text), "text")
                    .column_as(Expr::string("42").cast(SqlType::Integer), "number")
                    .filter(col("id").eq(val(1_i64))),
            ),
            json!([["120000", 42]]),
        ),
        query(
            "row values in a list",
            Query::from(
                from("deals").column(col("id")).filter(
                    Expr::Tuple(vec![col("company_id"), col("stage")]).in_list(vec![
                        Expr::Tuple(vec![val(1_i64), val("won")]),
                        Expr::Tuple(vec![val(2_i64), val("lost")]),
                    ]),
                ),
            )
            .order_by(col("id").asc()),
            json!([[1], [3]]),
        ),
        query(
            "arithmetic keeps its precedence",
            Query::from(
                from("deals")
                    .column_as(
                        col("amount_cents").add(Expr::int(100)).mul(Expr::int(2)),
                        "a",
                    )
                    .column_as(
                        col("amount_cents").sub(Expr::int(50).sub(Expr::int(10))),
                        "b",
                    )
                    .column_as(Expr::Neg(Box::new(col("amount_cents").sub(Expr::int(500)))), "c")
                    .filter(col("id").eq(val(2_i64))),
            ),
            json!([[9200, 4460, -4000]]),
        ),
        refused_on(
            query(
                "a lateral join",
                Query::from(
                    Select::from_source(Source::aliased("companies", "c"))
                        .column(col("c.id"))
                        .column(col("l.title"))
                        .join(Join {
                            kind: navree_orm::render::ast::JoinKind::Inner,
                            source: Source::lateral(
                                Query::from(
                                    from("deals")
                                        .column(col("title"))
                                        .filter(col("deals.company_id").eq(col("c.id")))
                                        .filter(live()),
                                )
                                .order_by(col("created_at").desc())
                                .limit(1),
                                "l",
                            ),
                            on: None,
                        }),
                )
                .order_by(col("c.id").asc()),
                json!([[1, "Big renewal"], [2, "Draft deal"], [3, "Équipe expansion"]]),
            ),
            &[Dialect::Sqlite],
        ),
        query(
            "distinct",
            Query::from(from("deals").column(col("stage")).distinct()).order_by(col("stage").asc()),
            json!([["lead"], ["lost"], ["won"]]),
        ),
        query(
            "a limit and an offset",
            Query::from(from("deals").column(col("id")))
                .order_by(col("id").asc())
                .limit(2)
                .offset(3),
            json!([[4], [5]]),
        ),
        query(
            "an offset alone",
            Query::from(from("deals").column(col("id")))
                .order_by(col("id").asc())
                .offset(6),
            json!([[7], [8]]),
        ),
        query(
            "groups with a condition",
            Query::from(
                from("deals")
                    .column(col("stage"))
                    .column_as(Expr::count_all(), "n")
                    .group_by(col("stage"))
                    .having(Expr::count_all().gt(Expr::int(2))),
            )
            .order_by(col("stage").asc()),
            json!([["lead", 3], ["won", 3]]),
        ),
        query(
            "a derived table",
            Query::from(
                Select::from_source(Source::derived(
                    from("deals")
                        .column(col("company_id"))
                        .column_as(Expr::func("SUM", vec![col("amount_cents")]), "total")
                        .filter(col("company_id").is_not_null())
                        .group_by(col("company_id")),
                    "t",
                ))
                .column(col("t.company_id"))
                .column(col("t.total"))
                .filter(col("t.total").gt(val(100_000_i64))),
            )
            .order_by(col("t.company_id").asc()),
            json!([[1, 1114500], [3, 250000]]),
        ),
        Portable {
            locked: true,
            ..query(
                "a locking read",
                Query::from(from("companies").column(col("id")).filter(col("id").eq(val(1_i64))))
                    .lock(Lock::new(LockStrength::Update)),
                json!([[1]]),
            )
        },
        query(
            "a raw fragment and a template",
            Query::from(
                from("deals")
                    .column(col("id"))
                    .column_as(Expr::template("UPPER({})", vec![col("stage")]), "loud")
                    .filter(Expr::raw("amount_cents BETWEEN ? AND ?", [1_000_i64, 5_000]))
                    .filter(col("stage").eq(val("lead")).or(Expr::raw("1 = 1", Vec::<i64>::new()))),
            )
            .order_by(col("id").asc()),
            json!([[2, "LEAD"], [6, "LOST"]]),
        ),
    ]
}

impl Portable {
    fn with_steps(mut self, steps: Vec<Statement>) -> Self {
        self.steps = steps;
        self
    }
}

#[test]
fn every_portable_statement_has_expected_rows() {
    for portable in statements() {
        assert!(portable.expect.is_array(), "{}", portable.name);
        assert!(!portable.steps.is_empty(), "{}", portable.name);
    }
}
