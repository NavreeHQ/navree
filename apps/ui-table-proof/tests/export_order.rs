//! An export longer than one read, on SQLite, writes every row once: sorted by a column of
//! a joined table (which walks the key, named with its table so the joined table's own `id`
//! is not taken for it), and sorted by a column with NULL in it where one read ends.
//! Each test works in a database file of its own.

use navree::db::Executor as _;
use navree::db::{Db, DbOptions};
use navree::orm::{Model, Record, col};
use navree::ui::table::NoFilter;
use navree::ui::{TableQuery, TableRow};
use uuid::Uuid;

/// More rows than an export reads at a time (2,000), so every test crosses a boundary.
const ROWS: i64 = 5_500;

#[derive(Model, Clone, Debug, Default, serde::Serialize)]
#[model(table = "eo_deals")]
pub struct Deal {
    pub id: i64,
    pub name: String,
    pub company_id: i64,
}

#[derive(Model, Clone, Debug, Default, serde::Serialize)]
#[model(table = "eo_companies")]
pub struct Company {
    pub id: i64,
    pub name: String,
}

/// A deal with its company's name, sortable by it.
#[derive(Clone, TableRow)]
pub struct DealRow {
    #[col(skip)]
    pub id: i64,
    #[col(sortable, label = "Name")]
    pub name: String,
    #[col(sortable, source = "eo_companies.name", label = "Company")]
    pub company: String,
}

#[derive(Model, Clone, Debug, Default, serde::Serialize, TableRow)]
#[model(table = "eo_people")]
pub struct Person {
    #[col(skip)]
    pub id: i64,
    #[col(sortable, label = "Name")]
    pub name: String,
    /// NULL for many people: the table knows it can be.
    #[col(sortable, label = "City")]
    pub city: Option<String>,
}

/// The same people, drawn by a row that says the city is always there.
#[derive(Clone, TableRow)]
pub struct PersonRow {
    #[col(skip)]
    pub id: i64,
    #[col(sortable, label = "Name")]
    pub name: String,
    #[col(sortable, label = "City")]
    pub city: String,
}

const SCHEMA: [&str; 3] = [
    "CREATE TABLE eo_companies (id INTEGER PRIMARY KEY, name TEXT NOT NULL)",
    "CREATE TABLE eo_deals (id INTEGER PRIMARY KEY, name TEXT NOT NULL, company_id INTEGER NOT NULL)",
    "CREATE TABLE eo_people (id INTEGER PRIMARY KEY, name TEXT NOT NULL, city TEXT)",
];

/// A database file of its own, with the tables.
async fn database() -> (Db, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("ui-eo-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let db = Db::connect(&DbOptions {
        url: format!("sqlite://{}?mode=rwc", dir.join("e.db").display()),
        ..DbOptions::default()
    })
    .await
    .expect("connects");
    for statement in SCHEMA {
        db.pool()
            .execute_script(statement)
            .await
            .expect("the tables");
    }
    (db, dir)
}

async fn done(db: Db, dir: std::path::PathBuf) {
    db.close().await.unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

/// The first field of each line of an export, the header left out.
fn first_fields(file: &str) -> Vec<String> {
    file.trim_start_matches('\u{feff}')
        .lines()
        .skip(1)
        .map(|line| line.split(',').next().unwrap_or_default().to_owned())
        .collect()
}

/// The file an export answers with.
async fn file_of(response: navree::http::response::Response) -> String {
    let bytes = navree::http::body::to_bytes(response.into_body(), 64 << 20)
        .await
        .expect("the file");
    String::from_utf8(bytes.to_vec()).expect("text")
}

/// Each of `names` once, and all of them: no row skipped, none written twice.
fn every_row_once(mut names: Vec<String>, expected: usize, what: &str) {
    assert_eq!(names.len(), expected, "{what}: one line per row");
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), expected, "{what}: no row twice");
}

#[tokio::test]
async fn an_export_sorted_by_a_column_of_a_joined_table_writes_every_row_once() {
    let (db, dir) = database().await;
    // Companies with the deals' own ids, so each table has an `id` in every row.
    let companies: Vec<Company> = [
        "Sandhu Traders",
        "Grewal Exports",
        "Dhillon Agro",
        "Bajwa Textiles",
    ]
    .iter()
    .enumerate()
    .map(|(at, name)| Company {
        id: i64::try_from(at).unwrap() + 1,
        name: (*name).to_owned(),
    })
    .collect();
    Company::query()
        .insert(&db, companies)
        .await
        .expect("the companies");
    let deals: Vec<Deal> = (1..=ROWS)
        .map(|n| Deal {
            id: n,
            name: format!("Deal {n:05}"),
            company_id: n % 4 + 1,
        })
        .collect();
    for chunk in deals.chunks(1_000) {
        Deal::query()
            .insert(&db, chunk.to_vec())
            .await
            .expect("the deals");
    }
    let joined = || {
        Deal::query().join(
            "eo_companies",
            col("eo_companies.id"),
            "=",
            col("eo_deals.company_id"),
        )
    };
    let to_row = |record: Record<Deal>| {
        let deal = record.into_inner();
        DealRow {
            id: deal.id,
            name: deal.name,
            company: String::new(),
        }
    };
    for sort in ["company", "-company"] {
        let query =
            TableQuery::<DealRow, NoFilter>::parse("/deals", &format!("sort={sort}&cols=name"));
        let response = query
            .export_csv(joined(), &db, "deals.csv", to_row)
            .await
            .unwrap_or_else(|error| panic!("{sort}: {error}"));
        let names = first_fields(&file_of(response).await);
        let expected = usize::try_from(ROWS).unwrap();
        every_row_once(names.clone(), expected, sort);
        // In the order of the key, the deals' own.
        assert!(
            names.windows(2).all(|pair| pair[0] < pair[1]),
            "{sort}: in the deals' key order"
        );
    }
    // The page reads the same join, sorted by the joined column.
    let page = TableQuery::<DealRow, NoFilter>::parse("/deals", "sort=company")
        .load_as(joined(), &db, to_row)
        .await
        .expect("a page of the join");
    assert!(!page.rows.is_empty());
    done(db, dir).await;
}

#[tokio::test]
async fn an_export_sorted_by_a_column_with_null_where_a_read_ends_writes_every_row_once() {
    let (db, dir) = database().await;
    // 3,000 people with a city (many alike), 2,500 with none. Going up, NULL comes first, so
    // the first read ends among the NULLs; going down, it comes last, so the first read ends
    // among the cities and the second among the NULLs.
    let people: Vec<Person> = (1..=ROWS)
        .map(|n| Person {
            id: n,
            name: format!("Person {n:05}"),
            city: (n % 11 < 6).then(|| format!("City {:03}", n % 97)),
        })
        .collect();
    let with_city = people.iter().filter(|person| person.city.is_some()).count();
    assert!(with_city > 2_000 && people.len() - with_city > 2_000);
    for chunk in people.chunks(1_000) {
        Person::query()
            .insert(&db, chunk.to_vec())
            .await
            .expect("the people");
    }
    let expected = usize::try_from(ROWS).unwrap();
    for sort in ["city", "-city", "city,-name", "-city,name"] {
        let url = format!("sort={sort}&cols=name");
        // A row that says the city can be missing.
        let query = TableQuery::<Person, NoFilter>::parse("/people", &url);
        let response = query
            .export_csv(Person::query(), &db, "people.csv", Record::into_inner)
            .await
            .unwrap_or_else(|error| panic!("{sort}: {error}"));
        every_row_once(
            first_fields(&file_of(response).await),
            expected,
            &format!("{sort}, a column said to hold NULL"),
        );
        // A row that says it is always there: the database still has NULL in it.
        let query = TableQuery::<PersonRow, NoFilter>::parse("/people", &url);
        let response = query
            .export_csv(
                Person::query(),
                &db,
                "people.csv",
                |record: Record<Person>| {
                    let person = record.into_inner();
                    PersonRow {
                        id: person.id,
                        name: person.name,
                        city: person.city.unwrap_or_default(),
                    }
                },
            )
            .await
            .unwrap_or_else(|error| panic!("{sort}: {error}"));
        every_row_once(
            first_fields(&file_of(response).await),
            expected,
            &format!("{sort}, a column said to hold none"),
        );
    }
    done(db, dir).await;
}
