//! What a person may use of a table, and how much of it a request may take, on SQLite:
//! a column taken away from them is not filtered on, sorted by, searched or exported,
//! an export stops at its limit and says so, and a person keeps a limited number of
//! saved views. Each test works in a database file of its own.

use navree::db::Executor as _;
use navree::db::{Db, DbOptions};
use navree::orm::{Model, Record};
use navree::ui::table::{NoFilter, Total};
use navree::ui::{TableQuery, TableRow};
use uuid::Uuid;

#[derive(Model, Clone, Debug, Default, serde::Serialize, TableRow)]
#[model(table = "cs_staff")]
#[table(sort = "name", per = 10, per_pages(10, 100))]
pub struct Staff {
    #[col(skip)]
    pub id: i64,
    #[col(sortable, search, filter = text, label = "Name")]
    pub name: String,
    #[col(sortable, search, filter = text, label = "City")]
    pub city: String,
    #[col(sortable, numeric, hidden, filter = range, label = "Pay")]
    pub pay: i64,
}

const SCHEMA: &str =
    "CREATE TABLE cs_staff (id INTEGER PRIMARY KEY, name TEXT NOT NULL, city TEXT NOT NULL, pay INTEGER NOT NULL)";

/// A database file of its own, with the staff table and six people in it.
async fn staff() -> (Db, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("ui-cs-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let db = Db::connect(&DbOptions {
        url: format!("sqlite://{}?mode=rwc", dir.join("c.db").display()),
        ..DbOptions::default()
    })
    .await
    .expect("connects");
    db.pool().execute_script(SCHEMA).await.expect("the table");
    let people = [
        (1, "Gurpreet Sandhu", "Amritsar", 82_000),
        (2, "Simran Grewal", "Jalandhar", 64_000),
        (3, "Harpreet Dhillon", "Ludhiana", 91_000),
        (4, "Manjot Kaur", "Amritsar", 58_000),
        (5, "Jaspreet Bajwa", "Patiala", 73_000),
        (6, "Arshdeep Brar", "Ludhiana", 67_000),
    ];
    Staff::query()
        .insert(
            &db,
            people.map(|(id, name, city, pay)| Staff {
                id,
                name: name.to_owned(),
                city: city.to_owned(),
                pay,
            }),
        )
        .await
        .expect("the people");
    (db, dir)
}

async fn done(db: Db, dir: std::path::PathBuf) {
    db.close().await.unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

fn query(url: &str) -> TableQuery<Staff, NoFilter> {
    TableQuery::parse("/staff", url)
}

async fn names(query: &TableQuery<Staff, NoFilter>, db: &Db) -> Vec<String> {
    let page = query.load(Staff::query(), db).await.expect("loads");
    page.rows.into_iter().map(|row| row.name).collect()
}

async fn csv(query: &TableQuery<Staff, NoFilter>, db: &Db) -> String {
    let response = query
        .export_csv(Staff::query(), db, "staff.csv", Record::into_inner)
        .await
        .expect("exports");
    let bytes = navree::http::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("the file");
    String::from_utf8(bytes.to_vec()).expect("text")
}

#[tokio::test]
async fn a_column_taken_away_is_not_filtered_sorted_or_searched_by_the_database() {
    let (db, dir) = staff().await;
    let url = "filter[pay][gte]=80000&sort=-pay&per_page=100";

    // Hidden is only how the column starts: the URL still filters and sorts by it.
    let open = query(url).load(Staff::query(), &db).await.unwrap();
    assert_eq!(open.total, Total::Exact(2));
    let open: Vec<String> = open.rows.into_iter().map(|row| row.name).collect();
    assert_eq!(open, ["Harpreet Dhillon", "Gurpreet Sandhu"]);

    // Taken away, the filter and the sort are ignored, as for a column that isn't there.
    let closed = query(url).without_columns(["pay"]);
    let page = closed.load(Staff::query(), &db).await.unwrap();
    assert_eq!(page.total, Total::Exact(6));
    let rows: Vec<String> = page.rows.into_iter().map(|row| row.name).collect();
    assert_eq!(
        rows,
        [
            "Gurpreet Sandhu",
            "Simran Grewal",
            "Harpreet Dhillon",
            "Manjot Kaur",
            "Jaspreet Bajwa",
            "Arshdeep Brar"
        ],
        "in the order of the key, not of the pay"
    );

    // The search reads the columns that are left.
    assert_eq!(
        names(&query("search=Gurpreet"), &db).await,
        ["Gurpreet Sandhu"]
    );
    assert!(
        names(&query("search=Gurpreet").without_columns(["name"]), &db)
            .await
            .is_empty(),
        "the name is no longer searched"
    );
    assert_eq!(
        names(&query("search=Amritsar").without_columns(["name"]), &db)
            .await
            .len(),
        2,
        "the city still is"
    );
    // With none left to search, there is no search.
    assert_eq!(
        names(
            &query("search=Gurpreet&per_page=100").without_columns(["name", "city"]),
            &db
        )
        .await
        .len(),
        6
    );
    done(db, dir).await;
}

#[tokio::test]
async fn an_export_holds_back_the_columns_taken_away() {
    let (db, dir) = staff().await;
    let url = "cols=name,city,pay&filter[pay][gte]=80000&sort=-pay";
    let open = csv(&query(url), &db).await;
    assert!(open.contains("Pay") && open.contains("82000"), "{open}");
    assert_eq!(open.matches("\r\n").count(), 3, "a header and two rows");

    let closed = csv(&query(url).without_columns(["pay"]), &db).await;
    let lines: Vec<&str> = closed.trim_start_matches('\u{feff}').lines().collect();
    assert_eq!(lines[0], "Name,City");
    assert_eq!(lines.len(), 7, "the filter on the pay is ignored: {closed}");
    assert!(
        !closed.contains("82000") && !closed.contains("Pay"),
        "{closed}"
    );
    done(db, dir).await;
}
