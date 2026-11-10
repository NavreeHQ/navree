//! A searched list over 1,000,000 rows on PostgreSQL: how long a first answer, a text
//! search, a deep page and a search that finds nothing take, with and without a trigram
//! index. Slow, so it is ignored unless asked for:
//!
//! ```text
//! cargo test -p ui-table-proof --release --test options_million -- --ignored --nocapture
//! ```
//!
//! `OPTIONS_TIMINGS` names a file the table is also written to.

use std::time::Instant;

use navree::db::Executor as _;
use navree::db::{Db, DbOptions};
use navree::orm::Model;
use navree::ui::{Choice, OptionQuery, OptionSource};
use uuid::Uuid;

#[derive(Model, Clone, Debug, Default, serde::Serialize)]
#[model(table = "opt_big", workspace)]
pub struct OptBig {
    pub id: i64,
    pub workspace_id: Uuid,
    pub name: String,
    pub email: String,
}

const BIG: OptionSource = OptionSource::new("id", "name").search_in(&["name", "email"]);

fn own_database(url: &str, name: &str) -> String {
    let (base, _) = url.rsplit_once('/').expect("a URL with a database");
    format!("{base}/{name}")
}

async fn timed(
    db: &impl navree::orm::Executor,
    workspace: Uuid,
    query: &str,
    rounds: usize,
) -> (f64, usize) {
    let mut times = Vec::new();
    let mut found = 0;
    for _ in 0..rounds {
        let start = Instant::now();
        let list = BIG
            .find(
                OptBig::query().for_workspace(workspace),
                &OptionQuery::parse(query),
                db,
                "/big/options",
                |record| {
                    let row = record.into_inner();
                    Choice::new(row.id.to_string(), row.name)
                },
            )
            .await
            .unwrap();
        found = list.to_html().matches("data-value=").count();
        times.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    times.sort_by(f64::total_cmp);
    (times[times.len() / 2], found)
}

#[tokio::test]
#[ignore = "seeds 1,000,000 rows"]
async fn a_million_rows_answer_a_search_in_time() {
    let Ok(url) = std::env::var("NAVREE_TEST_POSTGRES_URL") else {
        assert!(
            std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() != Ok("1"),
            "NAVREE_TEST_POSTGRES_URL is not set and NAVREE_TEST_REQUIRE_SERVICES=1"
        );
        eprintln!("skipped: NAVREE_TEST_POSTGRES_URL isn't set");
        return;
    };
    let admin = Db::connect(&DbOptions {
        url: url.clone(),
        ..DbOptions::default()
    })
    .await
    .unwrap();
    let _ = admin
        .pool()
        .execute_script("DROP DATABASE IF EXISTS ui_opt_million WITH (FORCE)")
        .await;
    admin
        .pool()
        .execute_script("CREATE DATABASE ui_opt_million")
        .await
        .unwrap();
    let db = Db::connect(&DbOptions {
        url: own_database(&url, "ui_opt_million"),
        ..DbOptions::default()
    })
    .await
    .unwrap();
    let workspace = Uuid::new_v4();
    for statement in [
        "CREATE EXTENSION IF NOT EXISTS pg_trgm".to_owned(),
        "CREATE TABLE opt_big (id BIGINT PRIMARY KEY, workspace_id UUID NOT NULL, name TEXT NOT NULL, email TEXT NOT NULL)".to_owned(),
        format!("INSERT INTO opt_big SELECT n, '{workspace}'::uuid, initcap(substr(md5(n::text), 1, 6)) || ' ' || initcap(substr(md5((n * 7)::text), 1, 8)), substr(md5((n * 13)::text), 1, 10) || '@example.com' FROM generate_series(1, 1000000) n"),
        "CREATE INDEX opt_big_order ON opt_big (workspace_id, name, id)".to_owned(),
        "ANALYZE opt_big".to_owned(),
    ] {
        db.pool()
            .execute_script(&statement)
            .await
            .unwrap_or_else(|error| panic!("{statement}: {error}"));
    }
    let cases = [
        ("the first answer, no text", "limit=20"),
        ("a text that matches rows", "q=ab1&limit=20"),
        ("a text that matches few rows", "q=ab12cd&limit=20"),
        ("a text that matches none", "q=zzzzzz&limit=20"),
        ("a deep page", "after=900000&limit=20"),
        ("the biggest page", "q=a&limit=50"),
    ];
    let mut table = String::from("case\tmedian_ms\trows\tindex\n");
    for index in ["none", "trigram"] {
        if index == "trigram" {
            for statement in [
                "CREATE INDEX opt_big_trgm_name ON opt_big USING gin (lower(name) gin_trgm_ops)",
                "CREATE INDEX opt_big_trgm_email ON opt_big USING gin (lower(email) gin_trgm_ops)",
                "ANALYZE opt_big",
            ] {
                db.pool().execute_script(statement).await.unwrap();
            }
        }
        for (label, query) in cases {
            let (median, found) = timed(&db, workspace, query, 5).await;
            table.push_str(&format!("{label}\t{median:.1}\t{found}\t{index}\n"));
        }
    }
    println!("{table}");
    if let Ok(path) = std::env::var("OPTIONS_TIMINGS") {
        std::fs::write(path, &table).unwrap();
    }
}
