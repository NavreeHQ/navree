//! SQL list filters: the rules compiled into one `CASE` chain that agrees
//! with record checks (Review Focus 5, engine half).

use keel_access::engine::{
    ALL, Ability, Cond, Conditions, DELETE, MANAGE, READ, Rule, Rules, SqlColumns, UPDATE, field,
};
use sea_query::{Expr, PostgresQueryBuilder, Query, SqliteQueryBuilder};
use serde_json::{Value, json};

fn ability(define: impl FnOnce(&mut Rules)) -> Ability {
    let mut rules = Rules::new();
    define(&mut rules);
    rules.build()
}

/// `SELECT "id" FROM "posts" WHERE <filter>` with values inlined.
fn sqlite(filter: Expr) -> String {
    Query::select()
        .column("id")
        .from("posts")
        .and_where(filter)
        .to_string(SqliteQueryBuilder)
}

fn where_clause(filter: Expr) -> String {
    let sql = sqlite(filter);
    sql.split_once(" WHERE ")
        .map(|(_, w)| w.to_owned())
        .unwrap_or(sql)
}

#[test]
fn sql_filter_case_chain_snapshot() {
    let ability = ability(|r| {
        r.can(READ, "Post").when(field("published").eq(true));
        r.can(UPDATE, "Post")
            .fields(["title", "body"])
            .when(field("author_id").eq(7));
        r.can(DELETE, "Post").when(field("author_id").eq(7));
        r.cannot(DELETE, "Post").when(
            field("locked")
                .eq(true)
                .and(field("archived_at").ne(None::<&str>)),
        );
        r.cannot(DELETE, "Post")
            .when(field("status").not_in(["draft", "review"]));
        r.cannot(UPDATE, "Post").fields(["author_id"]);
    });

    insta::assert_snapshot!(
        sqlite(ability.sql_filter(DELETE, "Post")),
        @r#"SELECT "id" FROM "posts" WHERE (CASE WHEN ("status" IS NULL OR "status" NOT IN ('draft', 'review')) THEN FALSE WHEN ("archived_at" IS NOT NULL AND "locked" = TRUE) THEN FALSE WHEN ("author_id" = 7) THEN TRUE ELSE FALSE END)"#
    );
    // A field-only `cannot` does not hide records (as in `Ability::can`).
    insta::assert_snapshot!(
        where_clause(ability.sql_filter(UPDATE, "Post")),
        @r#"(CASE WHEN ("author_id" = 7) THEN TRUE ELSE FALSE END)"#
    );
    // No rule for the action/subject: FALSE.
    insta::assert_snapshot!(where_clause(ability.sql_filter(READ, "Comment")), @"FALSE");
}

#[test]
fn sql_filter_short_circuits_unconditional_rules() {
    let admin = ability(|r| {
        r.can(READ, "Post").when(field("published").eq(true));
        r.can(MANAGE, ALL);
    });
    assert_eq!(where_clause(admin.sql_filter(READ, "Post")), "TRUE");

    // Rules before an unconditional rule cannot decide; a later conditional
    // rule still can.
    let mostly = ability(|r| {
        r.can(READ, "Post").when(field("published").eq(true));
        r.can(READ, "Post");
        r.cannot(READ, "Post").when(field("hidden").eq(true));
    });
    assert_eq!(
        where_clause(mostly.sql_filter(READ, "Post")),
        r#"(CASE WHEN ("hidden" = TRUE) THEN FALSE ELSE TRUE END)"#
    );

    // Trailing WHENs that agree with ELSE are dropped.
    let denied = ability(|r| {
        r.cannot(READ, "Post").when(field("hidden").eq(true));
        r.cannot(READ, "Post");
    });
    assert_eq!(where_clause(denied.sql_filter(READ, "Post")), "FALSE");

    // Empty conditions match every record.
    let everything = ability(|r| {
        r.can(READ, "Post").when(Conditions::new());
    });
    assert_eq!(where_clause(everything.sql_filter(READ, "Post")), "TRUE");
}

#[test]
fn sql_filter_null_semantics() {
    // The single WHEN condition of `can read Post when <c>`.
    let one = |c: Conditions| {
        let ability = ability(|r| {
            r.can(READ, "Post").when(c);
        });
        let sql = where_clause(ability.sql_filter(READ, "Post"));
        sql.strip_prefix("(CASE WHEN (")
            .and_then(|s| s.strip_suffix(") THEN TRUE ELSE FALSE END)"))
            .unwrap_or(&sql)
            .to_owned()
    };
    assert_eq!(
        one(field("team_id").eq(None::<i64>)),
        r#""team_id" IS NULL"#
    );
    assert_eq!(
        one(field("team_id").ne(None::<i64>)),
        r#""team_id" IS NOT NULL"#
    );
    assert_eq!(one(field("team_id").eq(3)), r#""team_id" = 3"#);
    assert_eq!(
        one(field("team_id").ne(3)),
        r#""team_id" IS DISTINCT FROM 3"#
    );
    assert_eq!(one(field("rank").lt(2.5)), r#""rank" < 2.5"#);
    assert_eq!(one(field("rank").lte(2)), r#""rank" <= 2"#);
    assert_eq!(one(field("title").gt("m")), r#""title" > 'm'"#);
    assert_eq!(one(field("title").gte("it's")), r#""title" >= 'it''s'"#);
    assert_eq!(
        one(field("team_id").is_in([1, 2])),
        r#""team_id" IN (1, 2)"#
    );
    assert_eq!(
        one(field("team_id").is_in([Some(1), None])),
        r#""team_id" IN (1) OR "team_id" IS NULL"#
    );
    assert_eq!(
        one(field("team_id").is_in([None::<i64>])),
        r#""team_id" IS NULL"#
    );
    assert_eq!(one(field("team_id").is_in(Vec::<i64>::new())), "FALSE");
    assert_eq!(
        one(field("team_id").not_in([1, 2])),
        r#""team_id" IS NULL OR "team_id" NOT IN (1, 2)"#
    );
    assert_eq!(
        one(field("team_id").not_in([Some(1), None])),
        r#""team_id" IS NOT NULL AND "team_id" NOT IN (1)"#
    );
    assert_eq!(
        one(field("team_id").not_in([None::<i64>])),
        r#""team_id" IS NOT NULL"#
    );
    assert_eq!(one(field("team_id").not_in(Vec::<i64>::new())), "TRUE");
    assert_eq!(
        one(field("a").eq(1).and(field("b").gt(2))),
        r#""a" = 1 AND "b" > 2"#
    );
}

#[test]
fn sql_filter_with_custom_columns_and_values() {
    struct Posts;
    impl SqlColumns for Posts {
        fn column(&self, field: &str) -> Expr {
            Expr::col(("posts", field.to_owned()))
        }

        fn value(&self, field: &str, value: &Value) -> sea_query::Value {
            match (
                field,
                value.as_str().and_then(|s| uuid::Uuid::parse_str(s).ok()),
            ) {
                ("author_id", Some(id)) => id.into(),
                _ => keel_access::engine::sql_value(value),
            }
        }
    }

    let user = uuid::Uuid::from_u128(0x0190_aaaa_bbbb_7ccc_8ddd_eeee_ffff_0001);
    let ability = ability(|r| {
        r.can(UPDATE, "Post")
            .when(field("author_id").eq(user).and(field("rank").gt(1)));
    });
    let filter = ability.sql_filter_with(UPDATE, "Post", &Posts);
    let (sql, values) = Query::select()
        .column("id")
        .from("posts")
        .and_where(filter)
        .build(PostgresQueryBuilder);
    assert_eq!(
        sql,
        r#"SELECT "id" FROM "posts" WHERE (CASE WHEN ("posts"."author_id" = $1 AND "posts"."rank" > $2) THEN TRUE ELSE FALSE END)"#
    );
    assert_eq!(
        values.0,
        vec![sea_query::Value::from(user), sea_query::Value::from(1_i64)]
    );
}

#[test]
fn operands_a_column_does_not_accept_compile_like_record_checks() {
    // `flag` is a boolean column, `rank` a numeric one.
    struct Typed;
    impl SqlColumns for Typed {
        fn accepts(&self, field: &str, value: &Value) -> bool {
            match field {
                "flag" => value.is_boolean(),
                "rank" => value.is_number(),
                _ => true,
            }
        }
    }
    let filter = |conditions: Conditions| {
        let ability = ability(|r| {
            r.can(READ, "Post").when(conditions);
        });
        where_clause(ability.sql_filter_with(READ, "Post", &Typed))
    };

    // A record check never matches across types, so neither does SQL
    // (SQLite would otherwise compare `false` as `0`).
    let never = r#"(CASE WHEN (FALSE) THEN TRUE ELSE FALSE END)"#;
    let always = r#"(CASE WHEN (TRUE) THEN TRUE ELSE FALSE END)"#;
    assert_eq!(filter(field("flag").lte(0)), never);
    assert_eq!(filter(field("rank").eq("3")), never);
    assert_eq!(filter(field("rank").ne("3")), always);
    assert_eq!(filter(field("rank").not_in([json!("1")])), always);
    assert_eq!(
        filter(field("rank").is_in([json!(1), json!("1")])),
        r#"(CASE WHEN ("rank" IN (1)) THEN TRUE ELSE FALSE END)"#
    );
    assert_eq!(
        filter(field("flag").is_in([json!(1), Value::Null])),
        r#"(CASE WHEN ("flag" IS NULL) THEN TRUE ELSE FALSE END)"#
    );
    // Accepted operands compile as before.
    assert_eq!(
        filter(field("flag").eq(true)),
        r#"(CASE WHEN ("flag" = TRUE) THEN TRUE ELSE FALSE END)"#
    );
}

#[test]
fn sql_value_maps_json_scalars() {
    use keel_access::engine::sql_value;
    use serde_json::json;

    assert_eq!(sql_value(&json!(true)), sea_query::Value::from(true));
    assert_eq!(sql_value(&json!(-7)), sea_query::Value::from(-7_i64));
    assert_eq!(
        sql_value(&json!(9_007_199_254_740_991_u64)),
        sea_query::Value::from(9_007_199_254_740_991_i64)
    );
    assert_eq!(sql_value(&json!(2.5)), sea_query::Value::from(2.5_f64));
    assert_eq!(sql_value(&json!("x")), sea_query::Value::from("x"));
}

/// Property tests over random rule sets. Review Focus 5: `Ability::can` on a
/// row as JSON equals "the row is selected by `sql_filter`", executed by
/// SQLite with bound parameters (as SeaORM runs it) and with inlined values.
mod properties {
    use proptest::prelude::*;
    use proptest::test_runner::{Config, TestRunner};
    use sea_query::Order;
    use serde_json::json;
    use sqlx::{Connection, Row, SqliteConnection};

    use super::*;

    const COLUMNS: [&str; 4] = ["a", "b", "c", "d"];
    const STRINGS: [&str; 6] = ["", "a", "ab", "b", "B", "it's"];

    /// A row: `a` integer, `b` text, `c` boolean, `d` real; all nullable.
    #[derive(Clone, Debug)]
    struct Item {
        a: Option<i64>,
        b: Option<String>,
        c: Option<bool>,
        d: Option<f64>,
    }

    impl Item {
        fn json(&self, id: i64) -> Value {
            json!({ "id": id, "a": self.a, "b": self.b, "c": self.c, "d": self.d })
        }
    }

    fn item() -> impl Strategy<Value = Item> {
        (
            prop::option::of(-2_i64..=3),
            prop::option::of(prop::sample::select(STRINGS.to_vec()).prop_map(str::to_owned)),
            prop::option::of(any::<bool>()),
            prop::option::of(prop::sample::select(vec![-1.5, 0.0, 2.25, 3.0])),
        )
            .prop_map(|(a, b, c, d)| Item { a, b, c, d })
    }

    /// A non-null operand of the column's type (`d` mixes integers and floats).
    fn value_of(column: usize) -> BoxedStrategy<Value> {
        match column {
            0 => (-2_i64..=3).prop_map(Value::from).boxed(),
            1 => prop::sample::select(STRINGS.to_vec())
                .prop_map(Value::from)
                .boxed(),
            2 => any::<bool>().prop_map(Value::from).boxed(),
            _ => prop::sample::select(vec![
                json!(-1.5),
                json!(0.0),
                json!(2.25),
                json!(3.0),
                json!(3),
                json!(0),
            ])
            .boxed(),
        }
    }

    fn value_or_null(column: usize) -> BoxedStrategy<Value> {
        prop_oneof![1 => Just(Value::Null), 4 => value_of(column)].boxed()
    }

    fn cond_of(column: usize) -> BoxedStrategy<Cond> {
        let list = || prop::collection::vec(value_or_null(column), 0..4);
        let equality = prop_oneof![
            value_or_null(column).prop_map(Cond::Eq),
            value_or_null(column).prop_map(Cond::Ne),
            list().prop_map(Cond::In),
            list().prop_map(Cond::Nin),
        ];
        if column == 2 {
            // Range operators on booleans are rejected when rules are built.
            return equality.boxed();
        }
        prop_oneof![
            equality,
            value_of(column).prop_map(Cond::Lt),
            value_of(column).prop_map(Cond::Lte),
            value_of(column).prop_map(Cond::Gt),
            value_of(column).prop_map(Cond::Gte),
        ]
        .boxed()
    }

    fn conditions() -> impl Strategy<Value = Conditions> {
        let one = (0..COLUMNS.len())
            .prop_flat_map(|column| cond_of(column).prop_map(move |cond| (column, cond)));
        prop::collection::vec(one, 0..4).prop_map(|conds| {
            let mut conditions = Conditions::new();
            for (column, cond) in conds {
                let name = COLUMNS[column];
                let duplicate = conditions.get(name).is_some_and(|existing| {
                    existing.iter().any(|c| c.operator() == cond.operator())
                });
                if !duplicate {
                    conditions = conditions.with(name, cond);
                }
            }
            conditions
        })
    }

    fn rule() -> impl Strategy<Value = Rule> {
        (
            prop::sample::select(vec![READ, UPDATE, MANAGE]),
            prop::sample::select(vec!["Item", "Other", ALL]),
            any::<bool>(),
            prop::option::weighted(0.8, conditions()),
            prop::option::weighted(0.15, Just(vec!["a".to_owned()])),
        )
            .prop_map(|(action, subject, inverted, conditions, fields)| Rule {
                conditions,
                fields,
                inverted,
                ..Rule::allow(action, subject)
            })
    }

    /// Ids selected by `filter`, run with bound parameters and with literals.
    async fn selected_ids(conn: &mut SqliteConnection, filter: Expr) -> (Vec<i64>, Vec<i64>) {
        let select = Query::select()
            .column("id")
            .from("items")
            .and_where(filter)
            .order_by("id", Order::Asc)
            .to_owned();

        let (sql, values) = select.build(SqliteQueryBuilder);
        let mut bound = sqlx::query(sqlx::AssertSqlSafe(sql));
        for value in values.0 {
            bound = match value {
                sea_query::Value::Bool(v) => bound.bind(v),
                sea_query::Value::BigInt(v) => bound.bind(v),
                sea_query::Value::Double(v) => bound.bind(v),
                sea_query::Value::String(v) => bound.bind(v),
                other => panic!("unexpected SQL value {other:?}"),
            };
        }
        let with_params = bound.fetch_all(&mut *conn).await.expect("bound query runs");

        let inline = select.to_string(SqliteQueryBuilder);
        let with_literals = sqlx::query(sqlx::AssertSqlSafe(inline))
            .fetch_all(&mut *conn)
            .await
            .expect("inline query runs");

        let ids = |rows: Vec<sqlx::sqlite::SqliteRow>| {
            rows.iter().map(|row| row.get::<i64, _>(0)).collect()
        };
        (ids(with_params), ids(with_literals))
    }

    #[test]
    fn prop_record_check_equals_sql_filter() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let conn = tokio::sync::Mutex::new(runtime.block_on(async {
            let mut conn = SqliteConnection::connect("sqlite::memory:")
                .await
                .expect("sqlite");
            sqlx::query(
                "CREATE TABLE items (id INTEGER PRIMARY KEY, a INTEGER, b TEXT, c BOOLEAN, d REAL)",
            )
            .execute(&mut conn)
            .await
            .expect("create table");
            conn
        }));

        let mut runner = TestRunner::new(Config {
            cases: 768,
            failure_persistence: None,
            ..Config::default()
        });
        let strategy = (
            prop::collection::vec(rule(), 0..7),
            prop::collection::vec(item(), 1..10),
            prop::sample::select(vec![READ, UPDATE]),
        );
        let result = runner.run(&strategy, |(rules, items, action)| {
            let ability = Ability::from_rules(rules).expect("generated rules are valid");
            let expected: Vec<i64> = (1_i64..)
                .zip(&items)
                .filter(|(id, item)| ability.can(action, "Item", Some(&item.json(*id))))
                .map(|(id, _)| id)
                .collect();

            let filter = ability.sql_filter(action, "Item");
            let sql = sqlite(filter.clone());
            let (with_params, with_literals) = runtime.block_on(async {
                let mut conn = conn.lock().await;
                sqlx::query("DELETE FROM items")
                    .execute(&mut *conn)
                    .await
                    .expect("clear");
                for (id, item) in (1_i64..).zip(&items) {
                    sqlx::query("INSERT INTO items (id, a, b, c, d) VALUES (?, ?, ?, ?, ?)")
                        .bind(id)
                        .bind(item.a)
                        .bind(item.b.clone())
                        .bind(item.c)
                        .bind(item.d)
                        .execute(&mut *conn)
                        .await
                        .expect("insert");
                }
                selected_ids(&mut conn, filter).await
            });

            prop_assert_eq!(&with_params, &expected, "bound parameters; {}", sql);
            prop_assert_eq!(&with_literals, &expected, "inline literals; {}", sql);
            Ok(())
        });
        if let Err(failure) = result {
            panic!("{failure}\n{runner}");
        }
    }

    /// Token restriction is an intersection: the restricted ability allows
    /// exactly what the user's ability allows and the token covers.
    #[test]
    fn prop_restrict_to_is_user_and_token() {
        let prefixes: std::collections::BTreeMap<String, String> =
            [("items", "Item"), ("others", "Other")]
                .into_iter()
                .map(|(prefix, subject)| (prefix.to_owned(), subject.to_owned()))
                .collect();
        let tokens = vec![
            "items.read",
            "items.update",
            "items.*",
            "items.manage",
            "others.read",
            "unknown.read",
            "bad",
            "*",
        ];

        let mut runner = TestRunner::new(Config {
            cases: 1024,
            failure_persistence: None,
            ..Config::default()
        });
        let strategy = (
            prop::collection::vec(rule(), 0..7),
            prop::collection::vec(prop::sample::select(tokens), 0..4),
            prop::sample::select(vec![READ, UPDATE, MANAGE]),
            prop::sample::select(vec!["Item", "Other", ALL]),
            prop::option::of(item()),
            prop::option::of(prop::sample::select(vec!["a", "b"])),
        );
        let result = runner.run(&strategy, |(rules, token, action, subject, item, field)| {
            let user = Ability::from_rules(rules).expect("generated rules are valid");
            let token: Vec<String> = token.into_iter().map(str::to_owned).collect();
            let restricted = user.restrict_to(&token, &prefixes);

            let covered = token.iter().any(|t| t == "*")
                || token.iter().any(|t| {
                    let Some((prefix, token_action)) = t.rsplit_once('.') else {
                        return false;
                    };
                    let token_action = if token_action == "*" {
                        MANAGE
                    } else {
                        token_action
                    };
                    // An unknown prefix stands for itself.
                    prefixes.get(prefix).map_or(prefix, String::as_str) == subject
                        && (token_action == action || token_action == MANAGE)
                });
            let record = item.map(|item| item.json(1));
            let check = |ability: &Ability| match field {
                Some(field) => ability.can_field(action, subject, record.as_ref(), field),
                None => ability.can(action, subject, record.as_ref()),
            };
            prop_assert_eq!(check(&restricted), check(&user) && covered);
            Ok(())
        });
        if let Err(failure) = result {
            panic!("{failure}\n{runner}");
        }
    }
}
