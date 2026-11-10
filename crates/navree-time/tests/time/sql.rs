//! `ZoneSql`: local timestamps and local days in SQL, with a checked zone.

use crate::support::{day, zone};
use navree_db::{Backend, Statement};

/// The fragment with its values written in.
fn render(fragment: Statement) -> String {
    fragment.to_string()
}

#[test]
fn postgres_converts_with_at_time_zone() {
    let sql = zone("Asia/Calcutta").sql(Backend::Postgres);
    let created = sql.column("created_at");
    assert_eq!(sql.literal(), "'Asia/Kolkata'");
    assert_eq!(
        render(sql.local_timestamp(&created)),
        r#"("created_at" AT TIME ZONE 'Asia/Kolkata')"#
    );
    assert_eq!(
        render(sql.local_date(&created)),
        r#"CAST(("created_at" AT TIME ZONE 'Asia/Kolkata') AS DATE)"#
    );
    assert_eq!(
        sql.set_local().as_deref(),
        Some("SET LOCAL TIME ZONE 'Asia/Kolkata'")
    );
}

#[test]
fn mysql_converts_with_convert_tz() {
    let sql = zone("Europe/Paris").sql(Backend::MySql);
    let created = sql.column("created_at");
    assert_eq!(
        render(sql.local_timestamp(&created)),
        "CONVERT_TZ(`created_at`, '+00:00', 'Europe/Paris')"
    );
    assert_eq!(
        render(sql.local_date(&created)),
        "DATE(CONVERT_TZ(`created_at`, '+00:00', 'Europe/Paris'))"
    );
    assert_eq!(
        sql.set_local().as_deref(),
        Some("SET time_zone = 'Europe/Paris'")
    );
}

#[test]
fn a_fixed_offset_zone_needs_no_case_on_sqlite() {
    let sql = zone("Asia/Kolkata").sql(Backend::Sqlite);
    let created = sql.column("created_at");
    assert_eq!(
        render(sql.local_timestamp(&created)),
        r#"datetime("created_at", '+19800 seconds')"#
    );
    assert_eq!(
        render(sql.local_date(&created)),
        r#"date("created_at", '+19800 seconds')"#
    );
    assert_eq!(sql.set_local(), None);
}

#[test]
fn sqlite_uses_each_offset_in_force_within_the_window() {
    let london = zone("Europe/London");
    let (from, to) = navree_time::DayRange::new(day("2026-03-28"), day("2026-03-30"))
        .unwrap()
        .utc_bounds(&london);
    let sql = london.sql(Backend::Sqlite).within(from, to);
    let text = render(sql.local_date(&sql.column("created_at")));
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

#[test]
fn mysql_compares_instants_directly_within_the_window() {
    let london = zone("Europe/London");
    let (from, to) = navree_time::DayRange::new(day("2026-03-28"), day("2026-03-30"))
        .unwrap()
        .utc_bounds(&london);
    let sql = london.sql(Backend::MySql).within(from, to);
    let text = render(sql.local_date(&sql.column("created_at")));
    assert!(
        text.contains("CASE WHEN `created_at` < '2026-03-29 01:00:00'"),
        "{text}"
    );
    assert!(!text.contains("julianday"), "{text}");
    assert!(
        text.contains("DATE(CONVERT_TZ(`created_at`, '+00:00', '+01:00'))"),
        "{text}"
    );
}

#[test]
fn an_expression_with_values_carries_them_once_per_appearance() {
    let london = zone("Europe/London");
    let (from, to) = navree_time::DayRange::new(day("2026-03-28"), day("2026-03-30"))
        .unwrap()
        .utc_bounds(&london);
    let sql = london.sql(Backend::Sqlite).within(from, to);
    // `COALESCE(created_at, ?)`: a column with a fallback bound as a value.
    let expr = Statement::portable("COALESCE(created_at, ?)", ["2026-01-01".into()]);
    let day = sql.local_date(&expr);
    // Two offsets: the CASE has one WHEN and the ELSE, three appearances.
    let marks = day.sql().matches("COALESCE(created_at, ?)").count();
    assert_eq!(marks, 3, "{}", day.sql());
    assert_eq!(day.values().len(), marks);
    assert_eq!(day.sql().matches('?').count(), marks);
}
