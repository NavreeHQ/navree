//! `ZoneSql`: local timestamps and local days in SQL, with a checked zone.

use chrono::{DateTime, Utc};
use navree_time::Zone;
use sea_orm::sea_query::{Alias, Expr, ExprTrait, Order, Query, SqliteQueryBuilder};
use sea_orm::{ConnectionTrait, Database, DatabaseBackend, DbBackend, FromQueryResult, Statement};

use crate::support::{day, utc, zone};

fn render(backend: DbBackend, expr: Expr) -> String {
    let query = Query::select().expr(expr).to_owned();
    match backend {
        DbBackend::Postgres => query.to_string(sea_orm::sea_query::PostgresQueryBuilder),
        DbBackend::MySql => query.to_string(sea_orm::sea_query::MysqlQueryBuilder),
        _ => query.to_string(SqliteQueryBuilder),
    }
}

#[test]
fn postgres_converts_with_at_time_zone() {
    let sql = zone("Asia/Calcutta").sql(DbBackend::Postgres);
    assert_eq!(sql.literal(), "'Asia/Kolkata'");
    assert_eq!(
        render(
            DbBackend::Postgres,
            sql.local_timestamp(Expr::col("created_at"))
        ),
        r#"SELECT ("created_at" AT TIME ZONE 'Asia/Kolkata')"#
    );
    assert_eq!(
        render(DbBackend::Postgres, sql.local_date(Expr::col("created_at"))),
        r#"SELECT CAST(("created_at" AT TIME ZONE 'Asia/Kolkata') AS DATE)"#
    );
    assert_eq!(
        sql.set_local().as_deref(),
        Some("SET LOCAL TIME ZONE 'Asia/Kolkata'")
    );
}

#[test]
fn mysql_converts_with_convert_tz() {
    let sql = zone("Europe/Paris").sql(DbBackend::MySql);
    assert_eq!(
        render(
            DbBackend::MySql,
            sql.local_timestamp(Expr::col("created_at"))
        ),
        "SELECT CONVERT_TZ(`created_at`, '+00:00', 'Europe/Paris')"
    );
    assert_eq!(
        render(DbBackend::MySql, sql.local_date(Expr::col("created_at"))),
        "SELECT DATE(CONVERT_TZ(`created_at`, '+00:00', 'Europe/Paris'))"
    );
    assert_eq!(
        sql.set_local().as_deref(),
        Some("SET time_zone = 'Europe/Paris'")
    );
}

#[test]
fn a_fixed_offset_zone_needs_no_case_on_sqlite() {
    let sql = zone("Asia/Kolkata").sql(DbBackend::Sqlite);
    assert_eq!(
        render(
            DbBackend::Sqlite,
            sql.local_timestamp(Expr::col("created_at"))
        ),
        r#"SELECT datetime("created_at", '+19800 seconds')"#
    );
    assert_eq!(
        render(DbBackend::Sqlite, sql.local_date(Expr::col("created_at"))),
        r#"SELECT date("created_at", '+19800 seconds')"#
    );
    assert_eq!(sql.set_local(), None);
}

#[test]
fn sqlite_uses_each_offset_in_force_within_the_window() {
    let london = zone("Europe/London");
    let (from, to) = navree_time::DayRange::new(day("2026-03-28"), day("2026-03-30"))
        .unwrap()
        .utc_bounds(&london);
    let sql = london.sql(DbBackend::Sqlite).within(from, to);
    let text = render(DbBackend::Sqlite, sql.local_date(Expr::col("created_at")));
    assert!(
        text.contains("CASE WHEN julianday(\"created_at\") <"),
        "{text}"
    );
    assert!(
        text.contains("date(\"created_at\", '+0 seconds')"),
        "{text}"
    );
    assert!(
        text.contains("date(\"created_at\", '+3600 seconds')"),
        "{text}"
    );
}

#[derive(Debug, PartialEq, FromQueryResult)]
struct DayCount {
    day: String,
    count: i64,
}

async fn counts_per_local_day(
    created: &[DateTime<Utc>],
    zone: &Zone,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Vec<DayCount> {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    db.execute_unprepared("CREATE TABLE events (id INTEGER PRIMARY KEY, created_at TEXT NOT NULL)")
        .await
        .unwrap();
    for at in created {
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO events (created_at) VALUES (?)",
            [(*at).into()],
        ))
        .await
        .unwrap();
    }
    let sql = zone.sql(DbBackend::Sqlite).within(from, to);
    let local_day = sql.local_date(Expr::col("created_at"));
    let query = Query::select()
        .expr_as(local_day.clone(), Alias::new("day"))
        .expr_as(Expr::cust("COUNT(*)"), Alias::new("count"))
        .from("events")
        .and_where(Expr::col("created_at").gte(from))
        .and_where(Expr::col("created_at").lt(to))
        .add_group_by([local_day])
        .order_by_expr(Expr::cust("1"), Order::Asc)
        .to_owned();
    let (text, values) = query.build(SqliteQueryBuilder);
    DayCount::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        text,
        values,
    ))
    .all(&db)
    .await
    .unwrap()
}

#[tokio::test]
async fn sqlite_groups_rows_by_local_day_across_a_dst_change() {
    let london = zone("Europe/London");
    let range = navree_time::DayRange::new(day("2026-03-28"), day("2026-03-30")).unwrap();
    let (from, to) = range.utc_bounds(&london);
    let created = [
        utc("2026-03-28T23:30:00Z"), // 28 March, 23:30 GMT
        utc("2026-03-29T00:30:00Z"), // 29 March, 00:30 GMT
        utc("2026-03-29T22:30:00Z"), // 29 March, 23:30 BST
        utc("2026-03-29T23:30:00Z"), // 30 March, 00:30 BST
        utc("2026-03-30T22:59:00Z"), // 30 March, 23:59 BST
    ];
    let rows = counts_per_local_day(&created, &london, from, to).await;
    assert_eq!(
        rows,
        [
            DayCount {
                day: "2026-03-28".into(),
                count: 1
            },
            DayCount {
                day: "2026-03-29".into(),
                count: 2
            },
            DayCount {
                day: "2026-03-30".into(),
                count: 2
            },
        ]
    );
}

#[tokio::test]
async fn sqlite_groups_rows_by_local_day_in_a_half_hour_zone() {
    let kolkata = zone("Asia/Kolkata");
    let range = navree_time::DayRange::new(day("2026-10-26"), day("2026-10-27")).unwrap();
    let (from, to) = range.utc_bounds(&kolkata);
    let created = [
        utc("2026-10-25T18:30:00Z"), // 26 October, 00:00 IST
        utc("2026-10-26T18:29:59Z"), // 26 October, 23:59:59 IST
        utc("2026-10-26T18:30:00Z"), // 27 October, 00:00 IST
    ];
    let rows = counts_per_local_day(&created, &kolkata, from, to).await;
    assert_eq!(
        rows,
        [
            DayCount {
                day: "2026-10-26".into(),
                count: 2
            },
            DayCount {
                day: "2026-10-27".into(),
                count: 1
            },
        ]
    );
}
