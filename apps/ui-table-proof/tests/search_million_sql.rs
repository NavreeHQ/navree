//! The table's search box over 1,000,000 rows on MySQL and MariaDB: how long a search takes
//! as `Contains` (the default: the text anywhere in a column, so the whole table is read)
//! and as `Words` (the database's full-text index). Slow, so it is ignored unless asked for:
//!
//! ```text
//! NAVREE_TEST_MYSQL_URL=mysql://root:pw@127.0.0.1:3306/navree_test \
//! NAVREE_TEST_MARIADB_URL=mariadb://root:pw@127.0.0.1:3307/navree_test \
//! cargo test -p ui-table-proof --release --test search_million_sql -- --ignored --nocapture
//! ```
//!
//! Each server that has a URL is measured; one with none is skipped. `SEARCH_TIMINGS` names
//! a file the table is also written to (`store`, `mode`, `search`, `rows`, `median_ms`).
//! Seeding and building the index take a few minutes.
//!
//! What it holds, besides printing the numbers: the two modes find the same rows for a word
//! (and one row for a word one row has), `Words` answers a search whose matches are counted
//! in under two seconds, and for a word few rows have it is at least ten times faster than
//! reading the table.

use std::time::Instant;

use navree::db::Executor as _;
use navree::db::{Db, DbOptions};
use navree::orm::Model;
use navree::ui::table::{NoFilter, SearchMode, Total};
use navree::ui::{TableQuery, TableRow};

#[derive(Model, Clone, Debug, Default, serde::Serialize, TableRow)]
#[model(table = "dt_search_big")]
#[table(per = 25, per_pages(25))]
pub struct SearchBig {
    #[col(skip)]
    pub id: i64,
    #[col(sortable, search)]
    pub title: String,
    #[col(search)]
    pub notes: String,
}

/// What a search is called, the words typed, and how many rows hold them when that is known.
const SEARCHES: &[(&str, &str, Option<u64>)] = &[
    ("a word a tenth of the rows have", "agra", Some(100_000)),
    ("a word one row has", "ref500000", Some(1)),
    ("two words, in any order", "ludhiana renewal", None),
    ("a word no row has", "zzzzzz", Some(0)),
];

/// The most a `Words` search may take, in milliseconds: "milliseconds" with a wide margin for a slow runner.
const WORDS_LIMIT_MS: f64 = 2_000.0;

fn own_database(url: &str, name: &str) -> String {
    let (base, rest) = url.rsplit_once('/').expect("a URL with a database");
    let query = rest.split_once('?').map_or("", |(_, query)| query);
    if query.is_empty() {
        format!("{base}/{name}")
    } else {
        format!("{base}/{name}?{query}")
    }
}

fn env_url(name: &str) -> Option<String> {
    let url = std::env::var(name).ok();
    if url.is_none() {
        assert!(
            std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() != Ok("1"),
            "{name} is not set and NAVREE_TEST_REQUIRE_SERVICES=1"
        );
        eprintln!("skipped: {name} isn't set");
    }
    url
}

async fn connect(url: String) -> Db {
    Db::connect(&DbOptions {
        url,
        ..DbOptions::default()
    })
    .await
    .expect("connects")
}

/// The rows: `Renewal Agra ref1`, with a note, for every number from 1 to 1,000,000. The text is built
/// from the number so a word is found in a known share of the rows, and `ref500000` in one.
const SEED: &str = "INSERT INTO dt_search_big (id, title, notes) \
    SELECT n, \
        CONCAT(ELT(1 + n MOD 8, 'Renewal', 'Pilot', 'Rollout', 'Supply', 'Inspection', 'Training', 'Upgrade', 'Survey'), ' ', \
               ELT(1 + (n DIV 8) MOD 10, 'Agra', 'Ludhiana', 'Amritsar', 'Jalandhar', 'Patiala', 'Chandigarh', 'Jaipur', 'Lucknow', 'Kanpur', 'Bathinda'), \
               ' ref', n), \
        CONCAT(ELT(1 + (n DIV 3) MOD 6, 'Call the owner', 'Visit the site', 'Send the quote', 'Book a demo', 'Review the terms', 'Collect the cheque'), \
               ' on ', ELT(1 + n MOD 7, 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday')) \
    FROM (SELECT a.d + b.d * 10 + c.d * 100 + d.d * 1000 + e.d * 10000 + f.d * 100000 + 1 AS n \
          FROM dt_digits a, dt_digits b, dt_digits c, dt_digits d, dt_digits e, dt_digits f) AS numbers";

/// Seeds a million rows into `db`, then builds the full-text index over the searched columns, in their order.
async fn seed(db: &Db) {
    for statement in [
        "DROP TABLE IF EXISTS dt_search_big",
        "DROP TABLE IF EXISTS dt_digits",
        "CREATE TABLE dt_digits (d INT PRIMARY KEY)",
        "INSERT INTO dt_digits VALUES (0), (1), (2), (3), (4), (5), (6), (7), (8), (9)",
        "CREATE TABLE dt_search_big (id BIGINT PRIMARY KEY, title VARCHAR(160) NOT NULL, notes VARCHAR(255) NOT NULL) \
         ENGINE=InnoDB DEFAULT CHARSET=utf8mb4",
        SEED,
        "ALTER TABLE dt_search_big ADD FULLTEXT INDEX dt_search_big_words (title, notes)",
        "ANALYZE TABLE dt_search_big",
        "DROP TABLE dt_digits",
    ] {
        db.pool()
            .execute_script(statement)
            .await
            .unwrap_or_else(|error| panic!("{statement}: {error}"));
    }
}

fn median(mut times: Vec<f64>) -> f64 {
    times.sort_by(f64::total_cmp);
    times[times.len() / 2]
}

/// How long the page of a search takes (the median of `rounds`), and how many rows match.
async fn timed(db: &Db, text: &str, mode: SearchMode, rounds: usize) -> (f64, u64, usize) {
    let mut times = Vec::new();
    let mut matching = 0;
    let mut shown = 0;
    for _ in 0..rounds {
        let query = TableQuery::<SearchBig, NoFilter>::parse(
            "/search",
            &format!("search={}", text.replace(' ', "+")),
        )
        .search_mode(mode);
        let start = Instant::now();
        let page = query
            .load(SearchBig::query(), db)
            .await
            .unwrap_or_else(|error| panic!("{mode:?} {text}: {error}"));
        times.push(start.elapsed().as_secs_f64() * 1000.0);
        matching = match page.total {
            Total::Exact(total) | Total::AtLeast(total) => total,
            Total::Unknown => 0,
        };
        shown = page.rows.len();
        for row in &page.rows {
            let found = format!("{} {}", row.title, row.notes).to_lowercase();
            if mode == SearchMode::Contains {
                assert!(found.contains(text), "{mode:?} {text}: {found}");
            }
        }
    }
    (median(times), matching, shown)
}

/// Seeds one server, measures every search in both modes, and checks what the numbers must show.
async fn measure(store: &str, url: &str, table: &mut String) {
    let admin = connect(url.to_owned()).await;
    let _ = admin
        .pool()
        .execute_script("DROP DATABASE IF EXISTS ui_dt_search_million")
        .await;
    admin
        .pool()
        .execute_script("CREATE DATABASE ui_dt_search_million CHARACTER SET utf8mb4")
        .await
        .unwrap();
    let db = connect(own_database(url, "ui_dt_search_million")).await;
    let started = Instant::now();
    seed(&db).await;
    eprintln!(
        "{store}: seeded and indexed in {:.0} s",
        started.elapsed().as_secs_f64()
    );

    for (label, text, known) in SEARCHES {
        // Reading the whole table takes seconds: once is enough to measure it.
        let (contains_ms, contains_total, _) = timed(&db, text, SearchMode::Contains, 1).await;
        let (words_ms, words_total, words_shown) = timed(&db, text, SearchMode::Words, 5).await;
        for (mode, ms, total) in [
            ("contains", contains_ms, contains_total),
            ("words", words_ms, words_total),
        ] {
            table.push_str(&format!("{store}\t{mode}\t{label}\t{total}\t{ms:.1}\n"));
        }
        eprintln!(
            "{store}: {label}: contains {contains_ms:.0} ms ({contains_total} rows), words {words_ms:.1} ms ({words_total} rows)"
        );
        if let Some(rows) = known {
            assert_eq!(
                contains_total, *rows,
                "{store}: contains finds {rows} for {text}"
            );
        }
        // The words of a text are found whole: where the substring is a whole word, the two agree.
        if *text == "agra" || *text == "ref500000" || *text == "zzzzzz" {
            assert_eq!(
                words_total, contains_total,
                "{store}: words and contains find the same rows for {text}"
            );
        }
        assert!(words_shown <= 25);
        // Two words match the rows that hold either, a fifth of the table: that one is printed, not held.
        if known.is_some() {
            assert!(
                words_ms < WORDS_LIMIT_MS,
                "{store}: a words search for {text} took {words_ms:.0} ms"
            );
        }
        if *text == "ref500000" {
            assert!(
                words_ms * 10.0 < contains_ms,
                "{store}: words ({words_ms:.1} ms) is not ten times faster than reading the table ({contains_ms:.0} ms)"
            );
        }
    }
    db.close().await.unwrap();
    admin
        .pool()
        .execute_script("DROP DATABASE ui_dt_search_million")
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "seeds 1,000,000 rows and builds a full-text index, a few minutes a server"]
async fn a_million_rows_answer_a_words_search_in_milliseconds() {
    let mut table = String::from("store\tmode\tsearch\trows\tmedian_ms\n");
    let mut ran = false;
    for (store, name) in [
        ("mysql", "NAVREE_TEST_MYSQL_URL"),
        ("mariadb", "NAVREE_TEST_MARIADB_URL"),
    ] {
        if let Some(url) = env_url(name) {
            ran = true;
            measure(store, &url, &mut table).await;
        }
    }
    if ran {
        println!("{table}");
        if let Ok(path) = std::env::var("SEARCH_TIMINGS") {
            std::fs::write(path, &table).unwrap();
        }
    }
}
