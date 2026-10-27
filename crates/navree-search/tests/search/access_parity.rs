//! Access parity: for any rule set, `accessible(scope)` returns exactly the
//! records the SQL `Accessible` list returns, on every embedded engine.
//! When the rules only name filterable, non-temporal columns, the engine
//! applies them itself and the total is exact too.
//!
//! The records cover nulls, sub-millisecond timestamp differences, floats
//! that equal integers and an enum whose serde names differ from its
//! database values; the operands include values of other JSON types than
//! their column's.

use std::collections::BTreeSet;
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use navree_access::{Ability, Accessible, Cond, Conditions, Read, Rule, Subject};
use navree_db::{DatabaseModule, Db, MigrationSet, ModuleBuilderDbExt, PageQuery};
use navree_kernel::{Module, ModuleBuilder};
use navree_search::{Search, SearchModule};
use navree_testing::TestApp;
use proptest::prelude::*;
use proptest::test_runner::{Config, TestCaseError, TestRunner};
use sea_orm::{ActiveModelTrait, ActiveValue::Set, ConnectionTrait, DbErr, EntityTrait};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};
use serde_json::{Value, json};
use uuid::Uuid;

mod item {
    use navree_search::Searchable;
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    /// Serde says `Open` / `ClosedWon`; the database holds `open` /
    /// `closed_won`, and access rules compare the database values.
    #[derive(Clone, Debug, PartialEq, Eq, EnumIter, DeriveActiveEnum, Serialize)]
    #[sea_orm(rs_type = "String", db_type = "Text")]
    pub enum Stage {
        #[sea_orm(string_value = "open")]
        Open,
        #[sea_orm(string_value = "closed_won")]
        ClosedWon,
    }

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Searchable)]
    #[sea_orm(table_name = "items")]
    #[searchable(name = "Item")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        #[searchable]
        pub title: String,
        #[searchable(filter)]
        pub owner_id: Option<Uuid>,
        #[searchable(filter)]
        pub status: Option<String>,
        #[searchable(filter)]
        pub stage: Stage,
        #[searchable(filter, sort)]
        pub amount: i64,
        #[searchable(filter)]
        pub score: f64,
        #[searchable(filter)]
        pub flag: bool,
        #[searchable(filter, sort)]
        pub created_at: DateTimeUtc,
        #[searchable(filter)]
        pub due_at: Option<DateTimeUtc>,
        pub note: Option<String>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

type Item = item::Entity;

impl Subject for item::Entity {
    const SUBJECT: &'static str = "Item";
    const PERMISSION_PREFIX: &'static str = "items";
}

struct CreateItems;

impl MigrationName for CreateItems {
    fn name(&self) -> &str {
        "m20261026_000001_create_items"
    }
}

#[async_trait]
impl MigrationTrait for CreateItems {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "CREATE TABLE items (id TEXT PRIMARY KEY NOT NULL, title TEXT NOT NULL, \
                 owner_id TEXT NULL, status TEXT NULL, stage TEXT NOT NULL, \
                 amount BIGINT NOT NULL, score DOUBLE NOT NULL, flag BOOLEAN NOT NULL, \
                 created_at TEXT NOT NULL, due_at TEXT NULL, note TEXT NULL)",
            )
            .await
            .map(|_| ())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP TABLE items")
            .await
            .map(|_| ())
    }
}

struct ItemsModule;

impl Module for ItemsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(MigrationSet::new("items").migration(CreateItems))
            .import(SearchModule::for_feature::<Item>());
    }
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

/// Columns the engine filters on.
const EXACT: [&str; 6] = ["owner_id", "status", "stage", "amount", "score", "flag"];

/// Every column a condition may name: the exact ones, timestamps, a text
/// field, a column outside the index and one that does not exist.
const FIELDS: [&str; 11] = [
    "owner_id",
    "status",
    "stage",
    "amount",
    "score",
    "flag",
    "created_at",
    "due_at",
    "title",
    "note",
    "missing",
];

/// A condition operand for `field`: values of the column's JSON type
/// (plus `null` for the equality family), drawn from small pools, and now
/// and then one of another type.
fn operand(pools: &Pools, field: &str, seed: u8, range: bool) -> Value {
    if !range && seed.is_multiple_of(7) {
        return Value::Null;
    }
    if seed % 11 == 5 {
        return match field {
            "amount" | "score" => json!("3"),
            "flag" => json!(0),
            "status" | "stage" | "title" | "note" => json!(3),
            "created_at" | "due_at" => json!(1_761_393_600_250_i64),
            _ => json!(false),
        };
    }
    let pick = |values: &[Value]| values[usize::from(seed) % values.len()].clone();
    match field {
        "owner_id" => pools.uuid(seed),
        "status" => pick(&[
            json!("draft"),
            json!("open"),
            json!("won"),
            json!("zzz"),
            json!("a"),
        ]),
        "stage" => pick(&[
            json!("open"),
            json!("closed_won"),
            json!("Open"),
            json!("ClosedWon"),
        ]),
        "amount" => pick(&[
            json!(-5),
            json!(0),
            json!(3),
            json!(4),
            json!(10),
            json!(3.5),
        ]),
        "score" => pick(&[json!(0.5), json!(1), json!(1.0), json!(2.25), json!(3)]),
        "flag" if range => json!(seed % 3),
        "flag" => json!(seed.is_multiple_of(2)),
        "created_at" | "due_at" => pools.time(seed),
        "title" => pick(&[json!("item 1"), json!("item 2"), json!("j")]),
        "note" => pick(&[json!("n1"), json!("n2")]),
        _ if range => json!(seed % 4),
        _ => pick(&[json!(1), json!("x"), json!(true)]),
    }
}

/// A condition from four random bytes: field, operator, two operand seeds.
fn condition(pools: &Pools, (f, op, a, b): (u8, u8, u8, u8)) -> (&'static str, Conditions) {
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
    (field, Conditions::new().with(field, cond))
}

type RuleSeed = (bool, u8, u8, Vec<(u8, u8, u8, u8)>);

/// A rule, and whether the engine can apply it exactly.
fn rule(pools: &Pools, (inverted, action, subject, conds): &RuleSeed) -> (Rule, bool) {
    let mut rule = Rule::allow(
        ["read", "update", "manage"][usize::from(*action) % 3],
        ["Item", "all", "Other"][usize::from(*subject) % 3],
    );
    rule.inverted = *inverted;
    let mut exact = true;
    if !conds.is_empty() {
        let mut all = Conditions::new();
        for seed in conds {
            let (field, conditions) = condition(pools, *seed);
            exact &= EXACT.contains(&field);
            all = all.and(conditions);
        }
        rule.conditions = Some(all);
    }
    (rule, exact)
}

fn rules_strategy() -> impl Strategy<Value = Vec<RuleSeed>> {
    let cond = (any::<u8>(), any::<u8>(), any::<u8>(), any::<u8>());
    let rule = (
        any::<bool>(),
        any::<u8>(),
        any::<u8>(),
        prop::collection::vec(cond, 0..3),
    );
    prop::collection::vec(rule, 0..6)
}

/// Inserts records covering every pool value, including nulls.
async fn seed(db: &Db, pools: &Pools) -> Vec<item::Model> {
    let statuses = [Some("draft"), Some("open"), Some("won"), None];
    let mut records = Vec::new();
    for i in 0..72_usize {
        let model = item::ActiveModel {
            id: Set(Uuid::now_v7()),
            title: Set(format!("item {}", i % 3)),
            owner_id: Set((i % 4 != 3).then(|| pools.owners[i % 3])),
            status: Set(statuses[(i / 2) % 4].map(str::to_owned)),
            stage: Set(if i % 5 < 2 {
                item::Stage::Open
            } else {
                item::Stage::ClosedWon
            }),
            amount: Set([-5, 0, 3, 4, 10][(i / 3) % 5]),
            score: Set([0.5, 1.0, 2.25][(i / 5) % 3]),
            flag: Set(i.is_multiple_of(2)),
            created_at: Set(pools.times[(i / 7) % 5]),
            due_at: Set((i % 3 != 0).then(|| pools.times[(i + 2) % 5])),
            note: Set((i % 2 == 0).then(|| format!("n{}", i % 3))),
        }
        .insert(db.conn())
        .await
        .expect("insert item");
        records.push(model);
    }
    records
}

async fn listed(db: &Db, scope: &Accessible<Item, Read>) -> BTreeSet<String> {
    scope
        .clone()
        .apply(item::Entity::find())
        .all(db.conn())
        .await
        .expect("list query")
        .into_iter()
        .map(|item| item.id.to_string())
        .collect()
}

/// Runs the property on `engine` (`dir`: the Tantivy directory).
fn parity(engine: &str, dir: Option<&std::path::Path>, cases: u32) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let pools = Pools::new();
    let (app, db, search) = runtime.block_on(async {
        let mut builder = TestApp::builder()
            .temp_database()
            .config("search.engine", engine)
            .config("search.sync", "off");
        if let Some(dir) = dir {
            builder = builder.config("search.tantivy.path", dir.to_string_lossy().to_string());
        }
        let app = builder
            .module(DatabaseModule::from_config())
            .module(SearchModule::from_config())
            .module(ItemsModule)
            .build()
            .await;
        let db = app.get::<Db>().await;
        seed(&db, &pools).await;
        let search = app.get::<Search>().await;
        let report = search.import::<Item>().await.expect("import");
        assert_eq!(report.documents, 72);
        (app, db, search)
    });

    let mut runner = TestRunner::new(Config::with_cases(cases));
    runner
        .run(&rules_strategy(), |seeds| {
            let (rules, exact): (Vec<Rule>, Vec<bool>) =
                seeds.iter().map(|seed| rule(&pools, seed)).unzip();
            let exact = exact.into_iter().all(|e| e);
            let Ok(ability) = Ability::from_rules(rules) else {
                return Ok(()); // e.g. two `$in` on one field: rejected when built
            };
            let scope = Accessible::<Item, Read>::new(Arc::new(ability));
            runtime.block_on(async {
                let sql = listed(&db, &scope).await;
                let page = search
                    .query::<Item>("")
                    .accessible(scope.clone())
                    .paginate(PageQuery::new(1, 100))
                    .await
                    .map_err(|e| TestCaseError::fail(e.to_string()))?;
                let found: BTreeSet<String> = page.data.iter().map(|i| i.id.to_string()).collect();
                let ids: BTreeSet<String> = search
                    .query::<Item>("")
                    .accessible(scope.clone())
                    .ids(100)
                    .await
                    .map_err(|e| TestCaseError::fail(e.to_string()))?
                    .into_iter()
                    .collect();
                let rules = scope.ability().to_casl_json();
                if found != sql || ids != sql {
                    return Err(TestCaseError::fail(format!(
                        "{engine}: search and SQL disagree for rules {rules}:\n  \
                         only in search: {:?}\n  only in SQL: {:?}",
                        found.difference(&sql).collect::<Vec<_>>(),
                        sql.difference(&found).collect::<Vec<_>>(),
                    )));
                }
                if page.meta.total < sql.len() as u64 {
                    return Err(TestCaseError::fail(format!(
                        "{engine}: total {} below the {} readable records for rules {rules}",
                        page.meta.total,
                        sql.len()
                    )));
                }
                if exact && page.meta.total != sql.len() as u64 {
                    return Err(TestCaseError::fail(format!(
                        "{engine}: total {} for {} readable records, rules on exact fields {rules}",
                        page.meta.total,
                        sql.len()
                    )));
                }
                Ok(())
            })
        })
        .unwrap();
    drop(app);
}

#[test]
fn the_memory_engine_returns_what_sql_lists() {
    parity("memory", None, 192);
}

#[test]
fn the_database_engine_returns_what_sql_lists() {
    parity("postgres", None, 128);
}

#[cfg(feature = "tantivy")]
#[test]
fn the_tantivy_engine_returns_what_sql_lists() {
    let dir = tempfile::tempdir().unwrap();
    parity("tantivy", Some(dir.path()), 128);
}
