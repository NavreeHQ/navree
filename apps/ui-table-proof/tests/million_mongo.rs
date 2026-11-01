//! A million documents, timed: run by hand against a seeded database, since seeding
//! takes a while. `NAVREE_TEST_MONGODB_URL` names the server and the collection
//! `dt_big` lives in the database `ui_dt_big`.

use chrono::NaiveDate;
use navree::mongo::{MongoDb, MongoOptions};
use navree::orm::Model;
use navree::ui::table::NoFilter;
use navree::ui::{TableQuery, TableRow};

const STAGES: &[(&str, &str)] = &[("lead", "Lead"), ("qualified", "Qualified"), ("proposal", "Proposal"), ("won", "Won"), ("lost", "Lost")];

#[derive(Model, Clone, Debug, Default, serde::Serialize, TableRow)]
#[model(collection = "dt_big", driver = "mongodb")]
#[table(sort = "-amount", per = 50, per_pages(50), cursor, capped = 10000)]
pub struct Big {
    #[col(skip)]
    pub id: i64,
    #[col(sortable, search, filter = text)]
    pub title: String,
    #[col(sortable, filter = select, options = STAGES)]
    pub stage: String,
    #[col(sortable, numeric, filter = range)]
    pub amount: Option<i64>,
    #[col(sortable, filter = date)]
    pub created_on: Option<NaiveDate>,
}

#[derive(Clone, TableRow)]
#[table(sort = "-amount", per = 50, per_pages(50))]
pub struct BigRow {
    #[col(skip)]
    pub id: i64,
    #[col(sortable, search, filter = text)]
    pub title: String,
    #[col(sortable, filter = select, options = STAGES)]
    pub stage: String,
    #[col(sortable, numeric, filter = range)]
    pub amount: Option<i64>,
    #[col(sortable, filter = date)]
    pub created_on: Option<NaiveDate>,
}

fn median(mut times: Vec<u128>) -> u128 {
    times.sort_unstable();
    times[times.len() / 2]
}

#[tokio::test]
#[ignore = "needs a million seeded documents"]
async fn a_million_documents() {
    let url = std::env::var("NAVREE_TEST_MONGODB_URL").expect("the server");
    let (base, rest) = url.rsplit_once('/').expect("a database");
    let query = rest.split_once('?').map_or("", |(_, query)| query);
    let own = if query.is_empty() { format!("{base}/ui_dt_big") } else { format!("{base}/ui_dt_big?{query}") };
    let mongo = MongoDb::connect(&MongoOptions::new(own)).await.unwrap();
    for path in ["", "sort=amount", "sort=-created_on", "f.stage=won", "f.amount.min=4000000", "q=abc123", "count=1"] {
        let mut times = Vec::new();
        let mut rows = 0;
        for _ in 0..5 {
            let start = std::time::Instant::now();
            let table = TableQuery::<Big, NoFilter>::parse("/big", path);
            let page = table.load(Big::query(), &mongo).await.unwrap_or_else(|error| panic!("{path}: {error}"));
            rows = page.rows.len();
            times.push(start.elapsed().as_millis());
        }
        println!("mongodb\tcursor {path}\t{rows}\t{} ms", median(times));
    }
    for path in ["", "page=2", "page=10000", "page=20000", "f.stage=won", "f.stage=won&page=2000", "q=abc123"] {
        let mut times = Vec::new();
        let mut rows = 0;
        for _ in 0..5 {
            let start = std::time::Instant::now();
            let table = TableQuery::<BigRow, NoFilter>::parse("/big", path);
            let page = table
                .load_as(Big::query(), &mongo, |record| {
                    let big = record.into_inner();
                    BigRow { id: big.id, title: big.title, stage: big.stage, amount: big.amount, created_on: big.created_on }
                })
                .await
                .unwrap_or_else(|error| panic!("{path}: {error}"));
            rows = page.rows.len();
            times.push(start.elapsed().as_millis());
        }
        println!("mongodb\tpages {path}\t{rows}\t{} ms", median(times));
    }
}
