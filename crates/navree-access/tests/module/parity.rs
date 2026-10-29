//! Review focus 5: for any rule set, `ability.can(action, record)` equals
//! "the record is in the `Accessible` list query", with the SQL running on
//! the test database (see [`crate::database`]) on real column types (UUIDs,
//! timestamps with microseconds, booleans, integers, floats, nullable
//! columns).

use std::collections::BTreeSet;
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use navree_access::{
    Ability, Accessible, ActionMarker, Cond, Conditions, Delete, Read, Rule, Subject,
    TypedColumns, Update, row_json,
};
use navree_db::navree_schema::{Dialect, Schema};
use navree_db::record::{self, Filter};
use navree_db::{Db, Executor, Kind, Record};
use proptest::prelude::*;
use proptest::test_runner::{Config, TestCaseError, TestRunner};
use serde_json::{Value, json};
use uuid::Uuid;

navree_db::record! {
    /// An item (`items`).
    #[derive(Clone, Debug, PartialEq)]
    pub struct Item in "items" {
        pub id: Uuid,
        pub owner_id: Option<Uuid>,
        pub status: Option<String>,
        pub amount: i64,
        pub score: f64,
        pub flag: bool,
        pub created_at: DateTime<Utc>,
        pub due_at: Option<DateTime<Utc>>,
    }
}

impl Subject for Item {
    const SUBJECT: &'static str = "Item";
    const PERMISSION_PREFIX: &'static str = "items";
}

/// The columns of `items` with their types.
fn columns() -> TypedColumns {
    TypedColumns::new(Item::TABLE)
        .with("id", Kind::Uuid)
        .with("owner_id", Kind::Uuid)
        .with("status", Kind::String)
        .with("amount", Kind::I64)
        .with("score", Kind::F64)
        .with("flag", Kind::Bool)
        .with("created_at", Kind::Timestamp)
        .with("due_at", Kind::Timestamp)
}

/// The values records and conditions draw from, so they often meet.
struct Pools {
    owners: [Uuid; 3],
    times: [DateTime<Utc>; 5],
}

impl Pools {
    fn new() -> Self {
        let base = DateTime::parse_from_rfc3339("2026-10-25T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut owners = [Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7()];
        owners.sort();
        Pools {
            owners,
            times: [
                base,
                base + Duration::microseconds(250_000),
                base + Duration::microseconds(250_001),
                base + Duration::milliseconds(500),
                base + Duration::seconds(1),
            ],
        }
    }

    fn uuid(&self, seed: u8) -> Value {
        json!(self.owners[usize::from(seed) % 3].hyphenated().to_string())
    }

    fn time(&self, seed: u8) -> Value {
        json!(
            self.times[usize::from(seed) % 5].to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
        )
    }
}

const FIELDS: [&str; 8] = [
    "owner_id",
    "status",
    "amount",
    "score",
    "flag",
    "created_at",
    "due_at",
    "missing",
];

/// A condition operand for `field`: values of the column's JSON type
/// (plus `null` for the equality family), drawn from small pools.
fn operand(pools: &Pools, field: &str, seed: u8, range: bool) -> Value {
    let null_allowed = !range && seed.is_multiple_of(7);
    if null_allowed {
        return Value::Null;
    }
    // Now and then an operand of another JSON type than the column's
    // (SQLite would coerce `'3'` to `3` and `false` to `0`).
    if seed % 11 == 5 {
        return match field {
            "amount" | "score" => json!("3"),
            "flag" => json!(0),
            "status" => json!(3),
            _ => json!(false),
        };
    }
    match field {
        "owner_id" => pools.uuid(seed),
        "status" => json!(["draft", "open", "won", "zzz", "a"][usize::from(seed) % 5]),
        "amount" => [
            json!(-5),
            json!(0),
            json!(3),
            json!(4),
            json!(10),
            json!(3.5),
        ][usize::from(seed) % 6]
            .clone(),
        "score" => {
            [json!(0.5), json!(1), json!(1.0), json!(2.25), json!(3)][usize::from(seed) % 5].clone()
        }
        "flag" if range => json!(seed % 3),
        "flag" => json!(seed.is_multiple_of(2)),
        "created_at" | "due_at" => pools.time(seed),
        _ if range => json!(seed % 4),
        _ => [json!(1), json!("x"), json!(true)][usize::from(seed) % 3].clone(),
    }
}

/// A condition from four random bytes: field, operator, two operand seeds.
fn condition(pools: &Pools, (f, op, a, b): (u8, u8, u8, u8)) -> Conditions {
    let field = FIELDS[usize::from(f) % FIELDS.len()];
    let cond = match op % 8 {
        0 => Cond::Eq(operand(pools, field, a, false)),
        1 => Cond::Ne(operand(pools, field, a, false)),
        2 => Cond::Lt(operand(pools, field, a, true)),
        3 => Cond::Lte(operand(pools, field, a, true)),
        4 => Cond::Gt(operand(pools, field, a, true)),
        5 => Cond::Gte(operand(pools, field, a, true)),
        6 => Cond::In(vec![
            operand(pools, field, a, false),
            operand(pools, field, b, false),
        ]),
        _ => Cond::Nin(vec![
            operand(pools, field, a, false),
            operand(pools, field, b, false),
        ]),
    };
    Conditions::new().with(field, cond)
}

type RuleSeed = (bool, u8, u8, u8, Vec<(u8, u8, u8, u8)>);

fn rule(pools: &Pools, (inverted, action, subject, fields, conds): &RuleSeed) -> Rule {
    let mut rule = Rule::allow(
        ["read", "update", "delete", "manage"][usize::from(*action) % 4],
        ["Item", "all", "Other"][usize::from(*subject) % 3],
    );
    rule.inverted = *inverted;
    if fields.is_multiple_of(6) {
        rule.fields = Some(vec!["status".to_owned()]);
    }
    if !conds.is_empty() {
        rule.conditions = Some(
            conds
                .iter()
                .map(|seed| condition(pools, *seed))
                .fold(Conditions::new(), Conditions::and),
        );
    }
    rule
}

fn rules_strategy() -> impl Strategy<Value = Vec<RuleSeed>> {
    let cond = (any::<u8>(), any::<u8>(), any::<u8>(), any::<u8>());
    let rule = (
        any::<bool>(),
        any::<u8>(),
        any::<u8>(),
        any::<u8>(),
        prop::collection::vec(cond, 0..3),
    );
    prop::collection::vec(rule, 0..6)
}

/// Creates `items` and inserts records covering every pool value, including
/// nulls; the records as stored.
async fn seed(db: &Db, pools: &Pools) -> Vec<Item> {
    let schema = Schema::new(db);
    let mysql = schema.target().await.expect("a target").dialect() == Dialect::MySql;
    schema
        .create("items", |t| {
            if mysql {
                t.binary("id").length(16).fixed().primary();
                t.binary("owner_id").length(16).fixed().nullable();
            } else {
                t.uuid("id").primary();
                t.uuid("owner_id").nullable();
            }
            t.string("status").nullable();
            t.big_integer("amount");
            t.double("score");
            t.boolean("flag");
            t.timestamp_tz("created_at");
            t.timestamp_tz("due_at").nullable();
        })
        .await
        .expect("create items");
    let statuses = [Some("draft"), Some("open"), Some("won"), None];
    let mut records = Vec::new();
    for i in 0..72_usize {
        let item = Item {
            id: Uuid::now_v7(),
            owner_id: (i % 4 != 3).then(|| pools.owners[i % 3]),
            status: statuses[(i / 2) % 4].map(str::to_owned),
            amount: [-5, 0, 3, 4, 10][(i / 3) % 5],
            score: [0.5, 1.0, 2.25][(i / 5) % 3],
            flag: i.is_multiple_of(2),
            created_at: pools.times[(i / 7) % 5],
            due_at: (i % 3 != 0).then(|| pools.times[(i + 2) % 5]),
        };
        let stored = record::insert_returning(db, &item)
            .await
            .expect("insert item");
        assert_eq!(stored, item, "the database keeps microseconds");
        records.push(stored);
    }
    records
}

async fn listed<A: ActionMarker>(db: &Db, ability: &Arc<Ability>) -> BTreeSet<Uuid> {
    let (sql, values) = Accessible::<Item, A>::new(ability.clone())
        .sql_filter(&columns())
        .to_statement(Executor::backend(db))
        .into_parts();
    Item::select()
        .filter(Filter::all().raw(sql, values))
        .all(db)
        .await
        .expect("list query")
        .into_iter()
        .map(|item| item.id)
        .collect()
}

fn allowed(ability: &Ability, action: &str, records: &[Item]) -> BTreeSet<Uuid> {
    records
        .iter()
        .filter(|item| ability.can(action, "Item", Some(&row_json(*item))))
        .map(|item| item.id)
        .collect()
}

#[test]
fn record_check_and_list_filter_agree() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let pools = Pools::new();
    let (db, records, _keep) = runtime.block_on(async {
        let (db, keep) = crate::database::connect().await;
        let records = seed(&db, &pools).await;
        (db, records, keep)
    });

    let mut runner = TestRunner::new(Config::with_cases(400));
    runner
        .run(&rules_strategy(), |seeds| {
            let rules: Vec<Rule> = seeds.iter().map(|seed| rule(&pools, seed)).collect();
            let Ok(ability) = Ability::from_rules(rules) else {
                return Ok(()); // e.g. two `$in` on one field: rejected when built
            };
            let ability = Arc::new(ability);
            runtime.block_on(async {
                for (action, sql) in [
                    ("read", listed::<Read>(&db, &ability).await),
                    ("update", listed::<Update>(&db, &ability).await),
                    ("delete", listed::<Delete>(&db, &ability).await),
                ] {
                    let expected = allowed(&ability, action, &records);
                    if sql != expected {
                        return Err(TestCaseError::fail(format!(
                            "`{action}` disagrees for rules {}:\n  SQL: {}\n  only in SQL: {:?}\n  only in record checks: {:?}",
                            ability.to_casl_json(),
                            ability
                                .sql_filter_with(action, "Item", &columns())
                                .to_statement(Executor::backend(&db)),
                            sql.difference(&expected)
                                .map(|id| row_json(records.iter().find(|r| r.id == *id).unwrap()))
                                .collect::<Vec<_>>(),
                            expected
                                .difference(&sql)
                                .map(|id| row_json(records.iter().find(|r| r.id == *id).unwrap()))
                                .collect::<Vec<_>>(),
                        )));
                    }
                }
                Ok(())
            })
        })
        .unwrap();
    runtime.block_on(async {
        let _ = db.close().await;
    });
}

#[test]
fn record_json_uses_column_names_and_condition_forms() {
    let pools = Pools::new();
    let id = Uuid::now_v7();
    let item = Item {
        id,
        owner_id: None,
        status: Some("open".into()),
        amount: 3,
        score: 2.25,
        flag: true,
        created_at: pools.times[1],
        due_at: None,
    };
    assert_eq!(
        row_json(&item),
        json!({
            "id": id.hyphenated().to_string(),
            "owner_id": null,
            "status": "open",
            "amount": 3,
            "score": 2.25,
            "flag": true,
            "created_at": "2026-10-25T12:00:00.250000Z",
            "due_at": null,
        })
    );
}
