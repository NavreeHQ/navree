//! Review focus 5: for any rule set, `ability.can(action, record)` equals
//! "the record is in the `Accessible` list query", with the SQL running
//! against SQLite on real column types (UUID blobs, timestamps, booleans,
//! integers, floats, nullable columns).

use std::collections::BTreeSet;
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use navree_access::{
    Ability, Accessible, ActionMarker, Cond, Conditions, Delete, Read, Rule, Subject, Update,
    record_json,
};
use navree_db::{Db, DbOptions};
use proptest::prelude::*;
use proptest::test_runner::{Config, TestCaseError, TestRunner};
use sea_orm::sea_query::{ColumnDef, Table};
use sea_orm::{ActiveModelTrait, EntityTrait, Set};
use sea_orm_migration::SchemaManager;
use serde_json::{Value, json};
use uuid::Uuid;

mod item {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "items")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        pub owner_id: Option<Uuid>,
        pub status: Option<String>,
        pub amount: i64,
        pub score: f64,
        pub flag: bool,
        pub created_at: DateTimeUtc,
        pub due_at: Option<DateTimeUtc>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

impl Subject for item::Entity {
    const SUBJECT: &'static str = "Item";
    const PERMISSION_PREFIX: &'static str = "items";
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

/// Inserts records covering every pool value, including nulls.
async fn seed(db: &Db, pools: &Pools) -> Vec<item::Model> {
    let manager = SchemaManager::new(db.conn());
    manager
        .create_table(
            Table::create()
                .table("items")
                .col(ColumnDef::new("id").uuid().not_null().primary_key())
                .col(ColumnDef::new("owner_id").uuid().null())
                .col(ColumnDef::new("status").string().null())
                .col(ColumnDef::new("amount").big_integer().not_null())
                .col(ColumnDef::new("score").double().not_null())
                .col(ColumnDef::new("flag").boolean().not_null())
                .col(
                    ColumnDef::new("created_at")
                        .timestamp_with_time_zone()
                        .not_null(),
                )
                .col(ColumnDef::new("due_at").timestamp_with_time_zone().null())
                .to_owned(),
        )
        .await
        .expect("create items");
    let statuses = [Some("draft"), Some("open"), Some("won"), None];
    let mut records = Vec::new();
    for i in 0..72_usize {
        let model = item::ActiveModel {
            id: Set(Uuid::now_v7()),
            owner_id: Set((i % 4 != 3).then(|| pools.owners[i % 3])),
            status: Set(statuses[(i / 2) % 4].map(str::to_owned)),
            amount: Set([-5, 0, 3, 4, 10][(i / 3) % 5]),
            score: Set([0.5, 1.0, 2.25][(i / 5) % 3]),
            flag: Set(i.is_multiple_of(2)),
            created_at: Set(pools.times[(i / 7) % 5]),
            due_at: Set((i % 3 != 0).then(|| pools.times[(i + 2) % 5])),
        }
        .insert(db.conn())
        .await
        .expect("insert item");
        records.push(model);
    }
    records
}

async fn listed<A: ActionMarker>(db: &Db, ability: &Arc<Ability>) -> BTreeSet<Uuid> {
    Accessible::<item::Entity, A>::new(ability.clone())
        .apply(item::Entity::find())
        .all(db.conn())
        .await
        .expect("list query")
        .into_iter()
        .map(|item| item.id)
        .collect()
}

fn allowed(ability: &Ability, action: &str, records: &[item::Model]) -> BTreeSet<Uuid> {
    records
        .iter()
        .filter(|item| ability.can(action, "Item", Some(&record_json(*item))))
        .map(|item| item.id)
        .collect()
}

#[test]
fn record_check_and_list_filter_agree() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (url, _guard) = navree_testing::temp_sqlite_url();
    let pools = Pools::new();
    let (db, records) = runtime.block_on(async {
        let db = Db::connect(&DbOptions {
            url,
            ..DbOptions::default()
        })
        .await
        .expect("connect");
        let records = seed(&db, &pools).await;
        (db, records)
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
                            "`{action}` disagrees for rules {}:\n  only in SQL: {:?}\n  only in record checks: {:?}",
                            ability.to_casl_json(),
                            sql.difference(&expected)
                                .map(|id| record_json(records.iter().find(|r| r.id == *id).unwrap()))
                                .collect::<Vec<_>>(),
                            expected
                                .difference(&sql)
                                .map(|id| record_json(records.iter().find(|r| r.id == *id).unwrap()))
                                .collect::<Vec<_>>(),
                        )));
                    }
                }
                Ok(())
            })
        })
        .unwrap();
}

#[test]
fn record_json_uses_column_names_and_condition_forms() {
    let pools = Pools::new();
    let id = Uuid::now_v7();
    let model = item::Model {
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
        record_json(&model),
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
