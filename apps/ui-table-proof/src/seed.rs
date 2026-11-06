//! The proof's data: the same rows on every store, with the hard cases in them.

use navree::db::{Db, Executor as _, Uow};
use navree::orm::Model;

use crate::{Company, Deal};

/// How many deals the tests start from.
pub const DEALS: i64 = 137;

/// Rows that have to survive: Unicode, right-to-left, hostile, very long.
fn title(id: i64) -> String {
    match id {
        1 => "Agra renewal".to_owned(),
        2 => "مرحبا بالعالم صفقة كبيرة جدا".to_owned(),
        3 => "<img src=x onerror=alert(1)><script>alert(2)</script>".to_owned(),
        4 => "\"'&amp; --; DROP TABLE dt_deals".to_owned(),
        5 => "長い名前の取引 🚀 emoji ÉCOLE".to_owned(),
        6 => "A very long deal title that goes on and on and on and on and on and on and on, far past what any column should show on one line, and then some more".to_owned(),
        7 => "Averyveryveryveryveryveryveryveryveryveryveryveryveryveryveryveryverylongunbrokenword".to_owned(),
        8 => "=HYPERLINK(\"http://evil.example\",\"click\")".to_owned(),
        _ => format!("{} renewal {id}", ["Gulati", "Indrani", "Ujala", "Hariyali", "Sutlej"][usize::try_from(id % 5).unwrap_or(0)]),
    }
}

fn notes(id: i64) -> Option<String> {
    match id % 7 {
        0 => None,
        1 => Some("نص طويل باللغة العربية يتجاوز عرض العمود بكثير حتى يختبر الالتفاف".to_owned()),
        _ => Some(format!(
            "Notes for deal {id}. They say something about what happened last."
        )),
    }
}

fn deal(id: i64) -> Deal {
    Deal {
        id,
        title: title(id),
        stage: ["lead", "qualified", "proposal", "won", "lost"]
            [usize::try_from((id * 3) % 5).unwrap_or(0)]
        .to_owned(),
        amount: (id % 6 != 0).then_some((id * 7919) % 5_000_000),
        company_id: (id % 8 != 0).then_some(id % 5 + 1),
        closed_on: (id % 5 != 0).then(|| {
            chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap_or_default()
                + chrono::Days::new(u64::try_from(id % 90).unwrap_or(0))
        }),
        active: id % 3 != 0,
        notes: notes(id),
        version: 1,
    }
}

/// Puts the deals back as they started.
pub async fn reset_deals(uow: &Uow) -> navree::orm::Result<()> {
    Deal::query().delete(uow).await?;
    Deal::query().insert(uow, (1..=DEALS).map(deal)).await?;
    Ok(())
}

/// Creates the tables and fills the small ones.
pub async fn create(db: &Db) -> Result<(), navree::BoxError> {
    let backend = db.backend();
    let (bigint, text) = (
        if matches!(backend, navree::db::Backend::Sqlite) {
            "INTEGER"
        } else {
            "BIGINT"
        },
        "VARCHAR(500)",
    );
    for statement in [
        "DROP TABLE IF EXISTS dt_deals".to_owned(),
        "DROP TABLE IF EXISTS dt_companies".to_owned(),
        "DROP TABLE IF EXISTS dt_tasks".to_owned(),
        "DROP TABLE IF EXISTS navree_ui_table_views".to_owned(),
        format!("CREATE TABLE dt_companies (id {bigint} PRIMARY KEY, name {text} NOT NULL)"),
        format!("CREATE TABLE dt_deals (id {bigint} PRIMARY KEY, title {text} NOT NULL, stage VARCHAR(20) NOT NULL, amount {bigint}, company_id {bigint}, closed_on DATE, active BOOLEAN NOT NULL, notes TEXT, version {bigint} NOT NULL)"),
        format!("CREATE TABLE dt_tasks (id {bigint} PRIMARY KEY, title {text} NOT NULL, done BOOLEAN NOT NULL)"),
        "CREATE TABLE navree_ui_table_views (id VARCHAR(64) PRIMARY KEY, owner VARCHAR(64) NOT NULL, table_id VARCHAR(64) NOT NULL, name VARCHAR(200) NOT NULL, query TEXT NOT NULL, is_default BOOLEAN NOT NULL)".to_owned(),
    ] {
        db.pool().execute_script(&statement).await?;
    }
    let names = [
        "Agra",
        "Gulati",
        "Indrani",
        "Ujala",
        "شركة الخليج",
        "Hariyali",
        "Sutlej",
        "Warraich",
        "Tiwana",
        "Chandni",
        "Sarson",
        "Majha",
    ];
    let rows = names.iter().enumerate().map(|(at, name)| Company {
        id: at as i64 + 1,
        name: (*name).to_owned(),
    });
    Company::query().insert(db, rows).await?;
    Deal::query().insert(db, (1..=DEALS).map(deal)).await?;
    let tasks = (1..=5).map(|id| crate::Task {
        id,
        title: format!("Task {id}"),
        done: id % 2 == 0,
    });
    crate::Task::query().insert(db, tasks).await?;
    Ok(())
}

/// Creates the big table with `count` rows, and the indexes its sorts use. One
/// index per sort column serves both directions; on PostgreSQL NULLs are named so
/// the index holds them where the table's order wants them.
pub async fn big(db: &Db, count: u64) -> Result<(), navree::BoxError> {
    let backend = db.backend();
    let pool = db.pool();
    pool.execute_script("DROP TABLE IF EXISTS dt_big").await?;
    let postgres = matches!(backend, navree::db::Backend::Postgres);
    let bigint = if matches!(backend, navree::db::Backend::Sqlite) {
        "INTEGER"
    } else {
        "BIGINT"
    };
    pool.execute_script(&format!("CREATE TABLE dt_big (id {bigint} PRIMARY KEY, title VARCHAR(200) NOT NULL, stage VARCHAR(20) NOT NULL, amount {bigint}, created_on DATE)")).await?;
    if count > 0 {
        let stage = "CASE n % 5 WHEN 0 THEN 'lead' WHEN 1 THEN 'qualified' WHEN 2 THEN 'proposal' WHEN 3 THEN 'won' ELSE 'lost' END";
        match backend {
            navree::db::Backend::Postgres => {
                pool.execute_script(&format!(
                    "INSERT INTO dt_big SELECT n, 'Deal ' || n || ' ' || md5(n::text), {stage}, CASE WHEN n % 20 = 0 THEN NULL ELSE (n * 7919) % 5000000 END, CASE WHEN n % 10 = 0 THEN NULL ELSE DATE '2020-01-01' + (n % 2000)::int END FROM generate_series(1::bigint, {count}::bigint) n"
                ))
                .await?;
            }
            navree::db::Backend::MySql => {
                let digits = "(SELECT 0 d UNION ALL SELECT 1 UNION ALL SELECT 2 UNION ALL SELECT 3 UNION ALL SELECT 4 UNION ALL SELECT 5 UNION ALL SELECT 6 UNION ALL SELECT 7 UNION ALL SELECT 8 UNION ALL SELECT 9)";
                pool.execute_script(&format!(
                    "INSERT INTO dt_big WITH s AS (SELECT a.d + b.d * 10 + c.d * 100 + e.d * 1000 + f.d * 10000 + g.d * 100000 + 1 AS n FROM {digits} a, {digits} b, {digits} c, {digits} e, {digits} f, {digits} g) SELECT n, CONCAT('Deal ', n, ' ', MD5(n)), {stage}, IF(n % 20 = 0, NULL, (n * 7919) % 5000000), IF(n % 10 = 0, NULL, DATE_ADD('2020-01-01', INTERVAL (n % 2000) DAY)) FROM s WHERE n <= {count}"
                ))
                .await?;
            }
            _ => {
                pool.execute_script(&format!(
                    "INSERT INTO dt_big WITH RECURSIVE s (n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM s WHERE n < {count}) SELECT n, 'Deal ' || n || ' ' || hex(n * 2654435761), {stage}, CASE WHEN n % 20 = 0 THEN NULL ELSE (n * 7919) % 5000000 END, CASE WHEN n % 10 = 0 THEN NULL ELSE date('2020-01-01', '+' || (n % 2000) || ' days') END FROM s"
                ))
                .await?;
            }
        }
    }
    let nulls = if postgres { " NULLS FIRST" } else { "" };
    for statement in [
        format!("CREATE INDEX dt_big_amount ON dt_big (amount{nulls}, id)"),
        format!("CREATE INDEX dt_big_created ON dt_big (created_on{nulls}, id)"),
        "CREATE INDEX dt_big_title ON dt_big (title, id)".to_owned(),
        "CREATE INDEX dt_big_stage ON dt_big (stage, id)".to_owned(),
    ] {
        pool.execute_script(&statement).await?;
    }
    if postgres {
        pool.execute_script("VACUUM ANALYZE dt_big").await?;
    }
    Ok(())
}
