//! The cron parser against the table recorded from croner 4.0.2
//! (`tests/data/croner_next_runs.txt`, made by `examples/croner_table.rs`
//! of commit aeee6ac9, while croner was still a dependency).
//!
//! Every row must match, except the rows listed in `DIFFERENCES` and
//! [`PARSE_DIFFERENCES`], where croner is wrong or looser than the
//! documented language. Each carries its reason.

use std::str::FromStr;

use chrono::{DateTime, LocalResult, TimeDelta, TimeZone, Utc};
use chrono_tz::Tz;
use navree_schedule::CronExpression;

const TABLE: &str = include_str!("../data/croner_next_runs.txt");

/// Expressions whose acceptance differs from croner's on purpose.
const PARSE_DIFFERENCES: &[(&str, &str)] = &[
    (
        "MON * * * *",
        "croner replaces names in every field; names belong to the month and day-of-week fields",
    ),
    (
        "* MON * * *",
        "croner replaces names in every field; names belong to the month and day-of-week fields",
    ),
    (
        "* * MON * *",
        "croner replaces names in every field; names belong to the month and day-of-week fields",
    ),
    ("* * * MON *", "a weekday name is not a month"),
    (
        "JAN * * * *",
        "croner replaces names in every field; names belong to the month and day-of-week fields",
    ),
    (
        "* * JAN * *",
        "croner replaces names in every field; names belong to the month and day-of-week fields",
    ),
    (
        "1,,2 * * * *",
        "an empty list item is a typo; croner skips it",
    ),
    (
        "1, * * * *",
        "an empty list item is a typo; croner skips it",
    ),
    (
        ",1 * * * *",
        "an empty list item is a typo; croner skips it",
    ),
    (", * * * *", "a list of nothing never matches in croner"),
    (
        "* * * * L",
        "a lone L in the day of week is accepted by croner and never matches",
    ),
    (
        "* * * * 1-5#2",
        "croner accepts #n on a range, with a meaning nobody documents",
    ),
    (
        "* * * * +",
        "an empty day of week after + is accepted by croner and never matches",
    ),
    (
        "0 0 ?/5 * *",
        "croner turns every ? into *; here ? stands alone",
    ),
];

/// Rows (`zone | start | expression | kinds`) whose runs differ from croner's
/// because croner breaks the daylight saving time rule (see the module
/// documentation of `CronExpression`). The test checks the set of differing
/// rows against this file and the kinds against what actually differs; set
/// `NAVREE_UPDATE_CRON_DIFFS=1` to rewrite it after a deliberate change.
const DIFFERENCES: &str = include_str!("../data/croner_dst_differences.txt");

struct Row<'a> {
    zone: &'a str,
    start: i64,
    expr: &'a str,
    runs: Vec<i64>,
}

fn rows() -> Vec<Row<'static>> {
    TABLE
        .lines()
        .filter(|l| !l.starts_with('#') && !l.starts_with("parse |"))
        .map(|line| {
            let mut parts = line.splitn(4, " | ");
            let zone = parts.next().unwrap();
            let start = parts.next().unwrap().parse().unwrap();
            let expr = parts.next().unwrap();
            let runs = parts.next().unwrap();
            let runs = if runs == "-" {
                Vec::new()
            } else {
                runs.split(',').map(|m| m.parse().unwrap()).collect()
            };
            Row {
                zone,
                start,
                expr,
                runs,
            }
        })
        .collect()
}

fn utc(seconds: i64) -> String {
    DateTime::<Utc>::from_timestamp(seconds, 0)
        .unwrap()
        .to_rfc3339()
}

#[test]
fn parse_verdicts_match_croner() {
    let mut unexplained = Vec::new();
    let mut explained = Vec::new();
    let mut compared = 0;
    for line in TABLE.lines().filter_map(|l| l.strip_prefix("parse | ")) {
        let mut parts = line.splitn(2, " | ");
        let croner_accepts = parts.next().unwrap() == "accepted";
        let text = parts.next().unwrap_or("");
        compared += 1;
        if CronExpression::parse(text).is_ok() == croner_accepts {
            continue;
        }
        match PARSE_DIFFERENCES.iter().find(|(known, _)| *known == text) {
            Some((_, reason)) => {
                explained.push(format!(
                    "{text:?} (croner accepts: {croner_accepts}): {reason}"
                ));
            }
            None => unexplained.push(format!("{text:?}: croner accepts {croner_accepts}")),
        }
    }
    assert!(compared > 50, "{compared} verdicts");
    println!(
        "{} explained differences of {compared} verdicts:\n{}",
        explained.len(),
        explained.join("\n")
    );
    assert!(
        unexplained.is_empty(),
        "{} unexplained:\n{}",
        unexplained.len(),
        unexplained.join("\n")
    );
}

/// Our runs by brute force: every UTC minute after `start` at which the
/// expression is due under the daylight saving time rule, written out
/// without the skipping search of `next_after`.
fn brute_force(cron: &CronExpression, zone: Tz, start: DateTime<Utc>, count: usize) -> Vec<i64> {
    let local = |t: DateTime<Utc>| t.with_timezone(&zone).naive_local();
    let mut minute =
        DateTime::from_timestamp(start.timestamp().div_euclid(60) * 60 + 60, 0).unwrap();
    let mut runs = Vec::new();
    while runs.len() < count {
        let now = local(minute);
        let due = if cron.matches_local(now) {
            match zone.from_local_datetime(&now) {
                LocalResult::Ambiguous(first, _) => first.to_utc() == minute,
                _ => true,
            }
        } else {
            // The clock jumped over local minutes: they run now, once.
            let mut skipped = local(minute - TimeDelta::minutes(1)) + TimeDelta::minutes(1);
            let mut hit = false;
            while skipped < now {
                hit |= cron.matches_local(skipped);
                skipped += TimeDelta::minutes(1);
            }
            hit
        };
        if due {
            runs.push(minute.timestamp() / 60);
        }
        minute += TimeDelta::minutes(1);
    }
    runs
}

/// What croner does differently at `minute` (unix minutes), when only one
/// side has a run there.
fn kind(cron: &CronExpression, zone: Tz, minute: i64, croner_has_it: bool) -> &'static str {
    let at = DateTime::<Utc>::from_timestamp(minute * 60, 0).unwrap();
    let local = |t: DateTime<Utc>| t.with_timezone(&zone).naive_local();
    let now = local(at);
    let second_pass = matches!(
        zone.from_local_datetime(&now),
        LocalResult::Ambiguous(first, _) if first.to_utc() != at
    );
    let after_gap = local(at - TimeDelta::minutes(1)) + TimeDelta::minutes(1) < now;
    match (
        croner_has_it,
        second_pass,
        after_gap,
        cron.matches_local(now),
    ) {
        (true, true, _, _) => "croner runs a repeated local time a second time",
        (true, false, true, false) => "croner runs a time before the gap again when the gap ends",
        (false, false, true, _) => "croner skips a skipped local time",
        _ => "UNEXPLAINED",
    }
}

#[test]
fn next_runs_match_croner() {
    let rows = rows();
    assert!(rows.len() > 1500, "{} rows", rows.len());
    let mut differing = Vec::new();
    let mut unexplained = Vec::new();
    for row in &rows {
        let Ok(cron) = CronExpression::from_str(row.expr) else {
            if PARSE_DIFFERENCES
                .iter()
                .all(|(known, _)| *known != row.expr)
            {
                unexplained.push(format!("{} does not parse", row.expr));
            }
            continue;
        };
        let zone: Tz = row.zone.parse().unwrap();
        let start = DateTime::from_timestamp(row.start, 0).unwrap();
        let mut cursor = start;
        let mut ours = Vec::new();
        while ours.len() < 10 {
            let Some(next) = cron.next_after(cursor, &zone) else {
                break;
            };
            assert_eq!(next.timestamp() % 60, 0);
            ours.push(next.timestamp() / 60);
            cursor = next;
        }
        if ours == row.runs {
            continue;
        }
        // Our runs are the daylight saving time rule's, whatever croner did.
        assert_eq!(
            ours,
            brute_force(&cron, zone, start, ours.len()),
            "{} | {} | {}: next_after disagrees with the brute-force rule",
            row.zone,
            row.expr,
            utc(row.start)
        );
        // Compare the common window, then say what croner did differently.
        let window = ours
            .last()
            .copied()
            .min(row.runs.last().copied())
            .unwrap_or(i64::MAX);
        let mut kinds = Vec::new();
        for &m in row
            .runs
            .iter()
            .filter(|m| **m <= window && !ours.contains(m))
        {
            kinds.push(kind(&cron, zone, m, true));
        }
        for &m in ours
            .iter()
            .filter(|m| **m <= window && !row.runs.contains(m))
        {
            kinds.push(kind(&cron, zone, m, false));
        }
        kinds.sort_unstable();
        kinds.dedup();
        if kinds.is_empty() {
            // Only the ends of the two lists differ (they stop at 10 runs).
            continue;
        }
        let kinds = kinds.join("; ");
        if kinds.contains("UNEXPLAINED") {
            unexplained.push(format!(
                "{} | {} | start {} | {kinds}",
                row.zone,
                row.expr,
                utc(row.start)
            ));
        }
        differing.push(format!(
            "{} | {} | {} | {kinds}",
            row.zone, row.start, row.expr
        ));
    }
    println!(
        "{} rows compared, {} differ from croner",
        rows.len(),
        differing.len()
    );
    assert!(
        unexplained.is_empty(),
        "{} unexplained:\n{}",
        unexplained.len(),
        unexplained.join("\n")
    );

    let actual = differing.join("\n");
    if std::env::var_os("NAVREE_UPDATE_CRON_DIFFS").is_some() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/data/croner_dst_differences.txt"
        );
        std::fs::write(path, format!("{actual}\n")).unwrap();
        return;
    }
    assert_eq!(
        actual.trim(),
        DIFFERENCES.replace("\r\n", "\n").trim(),
        "rows that differ from croner changed"
    );
}
