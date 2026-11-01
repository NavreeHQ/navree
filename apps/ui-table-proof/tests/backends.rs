//! One definition on four stores: the same URL gives the same rows in the same
//! order on PostgreSQL, MySQL, SQLite and MongoDB. Each store's test needs its URL
//! in the environment (`NAVREE_TEST_POSTGRES_URL`, `NAVREE_TEST_MYSQL_URL`,
//! `NAVREE_TEST_MONGODB_URL`), and uses its own database, dropped afterwards.

use std::collections::BTreeMap;

use chrono::NaiveDate;
use navree::db::Executor as _;
use navree::db::{Db, DbOptions};
use navree::mongo::{MongoDb, MongoOptions};
use navree::orm::{Builder, Executor, Model, Value};
use navree::ui::table::{BulkMode, NoFilter, TableRow as _};
use navree::ui::{BulkRequest, TablePage, TableQuery, TableRow};

macro_rules! deal {
    ($name:ident, $($attrs:tt)*) => {
        #[derive(Model, Clone, Debug, Default, serde::Serialize, TableRow)]
        #[model($($attrs)*)]
        #[table(per = 10, per_pages(5, 10, 100), version = "version")]
        pub struct $name {
            #[col(skip)]
            pub id: i64,
            #[col(sortable, search, filter = text)]
            pub title: String,
            #[col(sortable, filter = multi, options = STAGES)]
            pub stage: String,
            #[col(sortable, numeric, filter = range)]
            pub amount: Option<i64>,
            #[col(sortable, filter = date)]
            pub closed_on: Option<NaiveDate>,
            #[col(sortable, filter = boolean)]
            pub active: bool,
            #[col(filter = relation, kind = int)]
            pub company_id: Option<i64>,
            #[col(skip)]
            pub version: i64,
        }
    };
}

deal!(SqlDeal, table = "dt_deals");
deal!(MongoDeal, collection = "dt_deals", driver = "mongodb");

const STAGES: &[(&str, &str)] = &[("lead", "Lead"), ("won", "Won"), ("lost", "Lost")];

macro_rules! seed {
    ($type:ident) => {
        (1..=60_i64)
            .map(|id| $type {
                id,
                title: match id {
                    7 => "%wild_card\\".to_owned(),
                    8 => "ÉCOLE Çà".to_owned(),
                    9 => "مرحبا بالعالم".to_owned(),
                    10 => "'; DROP TABLE dt_deals; --".to_owned(),
                    _ => format!("Deal {:02}", id % 40),
                },
                stage: ["lead", "won", "lost"][usize::try_from(id % 3).unwrap_or(0)].to_owned(),
                amount: (id % 5 != 0).then_some((id % 7) * 100),
                closed_on: (id % 4 != 0).then(|| NaiveDate::from_ymd_opt(2026, 1, 1).unwrap() + chrono::Days::new(u64::try_from(id % 10).unwrap_or(0))),
                active: id % 2 == 0,
                company_id: (id % 6 != 0).then_some(id % 3 + 1),
                version: 1,
            })
            .collect::<Vec<$type>>()
    };
}

const SCHEMA: &str = "CREATE TABLE dt_deals (id BIGINT PRIMARY KEY, title VARCHAR(200) NOT NULL, stage VARCHAR(20) NOT NULL, amount BIGINT, closed_on DATE, active BOOLEAN NOT NULL, company_id BIGINT, version BIGINT NOT NULL)";

const SCENARIOS: &[&str] = &[
    "",
    "per=100",
    "per=100&sort=amount",
    "per=100&sort=-amount",
    "per=100&sort=closed_on",
    "per=100&sort=-closed_on",
    "per=100&sort=stage,-amount",
    "per=100&sort=-stage,amount,title",
    "per=100&sort=title&f.title=deal",
    "per=100&q=deal",
    "per=100&q=DEAL%2001",
    "per=100&q=%25wild",
    "per=100&q=_card",
    "per=100&q=%5C",
    "per=100&q=école",
    "per=100&q=مرحبا",
    "per=100&q=DROP",
    "per=100&f.title=deal+1",
    "per=100&f.stage=won",
    "per=100&f.stage=won&f.stage=lost",
    "per=100&f.amount.min=200",
    "per=100&f.amount.max=200",
    "per=100&f.amount.min=200&f.amount.max=400",
    "per=100&f.amount.empty=1",
    "per=100&f.closed_on.from=2026-01-05",
    "per=100&f.closed_on.to=2026-01-05",
    "per=100&f.closed_on.empty=1",
    "per=100&f.active=true",
    "per=100&f.active=false&f.stage=lead",
    "per=100&f.company_id=2",
    "per=100&f.company_id=2&f.company_id=3&sort=-amount",
    "per=100&f.title=nothing-matches-this",
    "page=2",
    "page=3&sort=-amount",
    "page=5&per=10&sort=closed_on",
    "page=6&per=10&sort=-closed_on",
    "page=999",
    "per=5&page=11&sort=amount",
    "per=5&page=12&sort=amount",
];

async fn ids<M, R, E>(exec: &E, query: &str, map: impl Fn(navree::orm::Record<M>) -> R) -> (Vec<String>, String, u32)
where
    M: Model,
    R: navree::ui::table::TableRow,
    E: Executor + ?Sized,
{
    let parsed = TableQuery::<R, NoFilter>::parse("/d", query);
    let page: TablePage<R> = parsed.load_as(M::query(), exec, map).await.unwrap_or_else(|error| panic!("{query}: {error}"));
    let total = match page.total {
        navree::ui::table::Total::Exact(total) => total.to_string(),
        other => format!("{other:?}"),
    };
    (page.rows.iter().map(|row| row.key().unwrap_or_default()).collect(), total, page.page)
}

type Results = BTreeMap<String, (Vec<String>, String, u32)>;

async fn all_scenarios_sql(db: &Db) -> Results {
    let mut out = BTreeMap::new();
    for query in SCENARIOS {
        out.insert((*query).to_owned(), ids::<SqlDeal, SqlDeal, _>(db, query, navree::orm::Record::into_inner).await);
    }
    out
}

async fn all_scenarios_mongo(db: &MongoDb) -> Results {
    let mut out = BTreeMap::new();
    for query in SCENARIOS {
        out.insert((*query).to_owned(), ids::<MongoDeal, MongoDeal, _>(db, query, navree::orm::Record::into_inner).await);
    }
    out
}

/// A cursor walk: every row once, forward and back, in every order asked.
async fn cursor_walk<M, E>(exec: &E, query_base: &str, map: impl Fn(navree::orm::Record<M>) -> CursorRow + Copy) -> (Vec<String>, Vec<String>)
where
    M: Model,
    E: Executor + ?Sized,
{
    let mut forward = Vec::new();
    let mut after = String::new();
    let mut last_prev = None;
    for _ in 0..100 {
        let query = TableQuery::<CursorRow, NoFilter>::parse("/d", &format!("{query_base}&after={after}"));
        let page = query.load_as(M::query(), exec, map).await.expect("a page");
        forward.extend(page.rows.iter().map(|row| row.id.to_string()));
        last_prev = page.prev.clone();
        match page.next {
            Some(next) => after = next,
            None => break,
        }
    }
    // And back from the last page.
    let mut backward = Vec::new();
    let mut before = last_prev;
    for _ in 0..100 {
        let Some(cursor) = before.clone() else { break };
        let query = TableQuery::<CursorRow, NoFilter>::parse("/d", &format!("{query_base}&before={cursor}"));
        let page = query.load_as(M::query(), exec, map).await.expect("a page");
        let mut rows: Vec<String> = page.rows.iter().map(|row| row.id.to_string()).collect();
        rows.append(&mut backward);
        backward = rows;
        before = page.prev.clone();
    }
    (forward, backward)
}

#[derive(Clone, TableRow)]
#[table(per = 7, per_pages(7), cursor, no_count)]
struct CursorRow {
    #[col(skip)]
    id: i64,
    #[col(sortable)]
    title: String,
    #[col(sortable, numeric)]
    amount: Option<i64>,
    #[col(sortable)]
    closed_on: Option<NaiveDate>,
    #[col(sortable)]
    stage: String,
}

const CURSOR_ORDERS: &[&str] = &["sort=", "sort=amount", "sort=-amount", "sort=closed_on", "sort=-closed_on", "sort=stage,-amount", "sort=-amount,closed_on", "sort=title"];

macro_rules! to_cursor_row {
    ($type:ty) => {
        |record: navree::orm::Record<$type>| {
            let deal = record.into_inner();
            CursorRow { id: deal.id, title: deal.title, amount: deal.amount, closed_on: deal.closed_on, stage: deal.stage }
        }
    };
}

fn check_expectations(results: &Results) {
    let get = |query: &str| &results[query];
    assert_eq!(get("").0.len(), 10);
    assert_eq!(get("").1, "60");
    // NULL is the smallest: first going up, last going down.
    let up = &get("per=100&sort=amount").0;
    assert!(up[..12].iter().all(|id| id.parse::<i64>().unwrap() % 5 == 0), "NULLs first ascending: {up:?}");
    assert!(up[12..].iter().all(|id| id.parse::<i64>().unwrap() % 5 != 0), "{up:?}");
    let down = &get("per=100&sort=-amount").0;
    assert!(down[48..].iter().all(|id| id.parse::<i64>().unwrap() % 5 == 0), "NULLs last descending: {down:?}");
    // Ties are in key order, in the direction of the last term.
    assert_eq!(&up[..3], ["5", "10", "15"], "ascending ties go by key up");
    assert_eq!(&down[48..51], ["60", "55", "50"], "descending ties go by key down");
    // `%`, `_` and `\` are text.
    assert_eq!(get("per=100&q=%25wild").0, ["7"]);
    assert_eq!(get("per=100&q=_card").0, ["7"]);
    assert_eq!(get("per=100&q=%5C").0, ["7"]);
    assert_eq!(get("per=100&q=école").0, ["8"], "case ignored past ASCII");
    assert_eq!(get("per=100&q=مرحبا").0, ["9"]);
    assert_eq!(get("per=100&q=DROP").0, ["10"]);
    assert_eq!(get("per=100&f.title=nothing-matches-this").1, "0");
    assert_eq!(get("per=100&f.amount.empty=1").1, "12");
    // A page past the end is the last page.
    assert_eq!(get("page=999").2, 6);
    assert_eq!(get("page=999").0.len(), 10);
    // Reading from the far end gives the same rows as from the near end.
    let whole = &get("per=100&sort=amount").0;
    let page_11 = &get("per=5&page=11&sort=amount").0;
    let page_12 = &get("per=5&page=12&sort=amount").0;
    assert_eq!(page_11.as_slice(), &whole[50..55], "a far page");
    assert_eq!(page_12.as_slice(), &whole[55..60], "the last page");
    let whole = &get("per=100&sort=-closed_on").0;
    assert_eq!(get("page=6&per=10&sort=-closed_on").0.as_slice(), &whole[50..60]);
}

fn own_database(url: &str, name: &str) -> String {
    let (base, rest) = url.rsplit_once('/').expect("a URL with a database");
    let query = rest.split_once('?').map_or("", |(_, query)| query);
    if query.is_empty() { format!("{base}/{name}") } else { format!("{base}/{name}?{query}") }
}

async fn sql_store(url: String) -> Db {
    let db = Db::connect(&DbOptions { url, ..DbOptions::default() }).await.expect("connects");
    let _ = db.pool().execute_script("DROP TABLE IF EXISTS dt_deals").await;
    db.pool().execute_script(SCHEMA).await.expect("the table");
    SqlDeal::query().insert(&db, seed!(SqlDeal)).await.expect("the rows");
    db
}

async fn admin_connect(url: &str) -> Db {
    Db::connect(&DbOptions { url: url.to_owned(), ..DbOptions::default() }).await.expect("connects")
}

async fn run_sql(db: &Db, label: &str) -> Results {
    let results = all_scenarios_sql(db).await;
    check_expectations(&results);
    for order in CURSOR_ORDERS {
        let (forward, backward) = cursor_walk::<SqlDeal, _>(db, order, to_cursor_row!(SqlDeal)).await;
        let mut sorted = forward.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), 60, "{label} {order}: each row once: {forward:?}");
        assert_eq!(forward.len(), 60, "{label} {order}: {forward:?}");
        // Backward stops one page short of the end: the walk started from the last page.
        let last = forward.len() - (forward.len() - 1) % 7 - 1;
        assert_eq!(backward.as_slice(), &forward[..last], "{label} {order}: back the same way");
    }
    bulk_sql(db, label).await;
    results
}

async fn bulk_sql(db: &Db, label: &str) {
    let post = |body: String| BulkRequest::<SqlDeal, NoFilter>::parse(body.as_bytes());
    // All that match, except two.
    let matching = SqlDeal::query().where_(navree::orm::col("stage").eq(Value::String("won".into()))).count(db).await.unwrap();
    let request = post(format!("action=a&_mode=all&_expected={}&ex=1&ex=4&_q=f.stage%3Dwon", matching - 2)).unwrap();
    assert_eq!(request.mode(), &BulkMode::All);
    let target = request.resolve(SqlDeal::query(), db).await.unwrap().expect("what was shown");
    let keys: Vec<i64> = target.clone().pluck(db, navree::orm::col_as::<i64>("id")).await.unwrap();
    assert!(!keys.contains(&1) && !keys.contains(&4), "{label}: {keys:?}");
    assert_eq!(keys.len() as u64, matching - 2, "{label}");
    // The set changed: refused, whatever is asked.
    let request = post(format!("action=a&_mode=all&_expected={}&_q=f.stage%3Dwon", matching + 1)).unwrap();
    let refused = request.resolve(SqlDeal::query(), db).await.unwrap();
    assert!(refused.is_err(), "{label}: a different count is refused");
    // The keys named, scoped by the query given.
    let request = post("action=a&ids=1&ids=2&ids=999".to_owned()).unwrap();
    let scoped = SqlDeal::query().where_(navree::orm::col("stage").ne(Value::String("lead".into())));
    let target = request.resolve(scoped, db).await.unwrap().unwrap();
    let keys: Vec<i64> = target.pluck(db, navree::orm::col_as::<i64>("id")).await.unwrap();
    assert_eq!(keys.len(), 2, "{label}: id 999 doesn't exist: {keys:?}");
    // Hostile posts.
    assert!(post("ids=1;DROP".to_owned()).is_err(), "not a key");
    assert!(post("_mode=all".to_owned()).is_err(), "all with no count");
}

fn assert_same(what: &str, left: &Results, right: &Results) {
    for query in SCENARIOS {
        assert_eq!(left[*query], right[*query], "{what} differ on `{query}`");
    }
}

fn env_url(name: &str) -> Option<String> {
    let url = std::env::var(name).ok();
    if url.is_none() {
        eprintln!("skipped: {name} isn't set");
    }
    url
}

#[tokio::test]
async fn sqlite_mysql_postgres_and_mongodb_agree() {
    let dir = std::env::temp_dir().join(format!("ui-table-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let sqlite = sql_store(format!("sqlite://{}?mode=rwc", dir.join("t.db").display())).await;
    let baseline = run_sql(&sqlite, "sqlite").await;

    if let Some(url) = env_url("NAVREE_TEST_POSTGRES_URL") {
        let admin = admin_connect(&url).await;
        let _ = admin.pool().execute_script("DROP DATABASE IF EXISTS ui_dt_backends WITH (FORCE)").await;
        admin.pool().execute_script("CREATE DATABASE ui_dt_backends").await.unwrap();
        {
            let db = sql_store(own_database(&url, "ui_dt_backends")).await;
            let results = run_sql(&db, "postgres").await;
            assert_same("sqlite and postgres", &baseline, &results);
            db.close().await;
        }
        admin.pool().execute_script("DROP DATABASE ui_dt_backends WITH (FORCE)").await.unwrap();
    }
    if let Some(url) = env_url("NAVREE_TEST_MYSQL_URL") {
        let admin = admin_connect(&url).await;
        let _ = admin.pool().execute_script("DROP DATABASE IF EXISTS ui_dt_backends").await;
        admin.pool().execute_script("CREATE DATABASE ui_dt_backends CHARACTER SET utf8mb4").await.unwrap();
        {
            let db = sql_store(own_database(&url, "ui_dt_backends")).await;
            let results = run_sql(&db, "mysql").await;
            assert_same("sqlite and mysql", &baseline, &results);
            db.close().await;
        }
        admin.pool().execute_script("DROP DATABASE ui_dt_backends").await.unwrap();
    }
    if let Some(url) = env_url("NAVREE_TEST_MONGODB_URL") {
        let mongo = MongoDb::connect(&MongoOptions::new(own_database(&url, "ui_dt_backends"))).await.unwrap();
        MongoDeal::query().delete(&mongo).await.unwrap();
        MongoDeal::query().insert(&mongo, seed!(MongoDeal)).await.unwrap();
        let results = all_scenarios_mongo(&mongo).await;
        check_expectations(&results);
        assert_same("sqlite and mongodb", &baseline, &results);
        for order in CURSOR_ORDERS {
            let (forward, backward) = cursor_walk::<MongoDeal, _>(&mongo, order, to_cursor_row!(MongoDeal)).await;
            let mut sorted = forward.clone();
            sorted.sort();
            sorted.dedup();
            assert_eq!(sorted.len(), 60, "mongodb {order}: {forward:?}");
            let last = forward.len() - (forward.len() - 1) % 7 - 1;
            assert_eq!(backward.as_slice(), &forward[..last], "mongodb {order}");
        }
        // All that match, except two, on documents too.
        let matching = MongoDeal::query().where_(navree::orm::col("stage").eq(Value::String("won".into()))).count(&mongo).await.unwrap();
        let body = format!("action=a&_mode=all&_expected={}&ex=1&ex=4&_q=f.stage%3Dwon", matching - 2);
        let request = BulkRequest::<MongoDeal, NoFilter>::parse(body.as_bytes()).unwrap();
        let target: Builder<MongoDeal> = request.resolve(MongoDeal::query(), &mongo).await.unwrap().expect("what was shown");
        assert_eq!(target.count(&mongo).await.unwrap(), matching - 2);
        mongo.database().drop().await.unwrap();
    }
    sqlite.close().await;
    let _ = std::fs::remove_dir_all(dir);
}
