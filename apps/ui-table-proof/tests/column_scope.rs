//! What a person may use of a table, and how much of it a request may take, on SQLite:
//! a column taken away from them is not filtered on, sorted by, searched or exported,
//! an export stops at its limit and says so, and a person keeps a limited number of
//! saved views. Each test works in a database file of its own.

use navree::db::Executor as _;
use navree::db::{Db, DbOptions};
use navree::orm::{Model, Record};
use navree::ui::table::{
    MAX_SAVED_VIEWS, NoFilter, Total, TooManyViews, delete_view, list_views, save_view,
};
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

/// An export: the file, and whether the response says the result was cut.
async fn export(query: &TableQuery<Staff, NoFilter>, db: &Db) -> (String, bool) {
    let response = query
        .export_csv(Staff::query(), db, "staff.csv", Record::into_inner)
        .await
        .expect("exports");
    let cut = response.headers().contains_key("x-navree-truncated");
    let bytes = navree::http::body::to_bytes(response.into_body(), 64 << 20)
        .await
        .expect("the file");
    (String::from_utf8(bytes.to_vec()).expect("text"), cut)
}

async fn csv(query: &TableQuery<Staff, NoFilter>, db: &Db) -> String {
    export(query, db).await.0
}

/// The names in an export, in the order of its lines.
fn exported_names(file: &str) -> Vec<&str> {
    file.trim_start_matches('\u{feff}')
        .lines()
        .skip(1)
        .map(|line| line.split(',').next().unwrap_or_default())
        .collect()
}

/// `count` more people, named `Person 00001` on, who all live in Ludhiana.
async fn more_people(db: &Db, count: i64) {
    let people: Vec<Staff> = (1..=count)
        .map(|n| Staff {
            id: 100 + n,
            name: format!("Person {n:05}"),
            city: "Ludhiana".to_owned(),
            pay: 50_000 + n,
        })
        .collect();
    for chunk in people.chunks(1_000) {
        Staff::query()
            .insert(db, chunk.to_vec())
            .await
            .expect("more people");
    }
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
async fn an_export_stops_at_its_limit_and_says_so_only_when_it_cut_the_result() {
    let (db, dir) = staff().await;
    let (all, cut) = export(&query(""), &db).await;
    assert_eq!(exported_names(&all).len(), 6);
    assert!(!cut, "the default limit is far above six rows");

    let (three, cut) = export(&query("").export_limit(3), &db).await;
    assert_eq!(
        exported_names(&three),
        ["Arshdeep Brar", "Gurpreet Sandhu", "Harpreet Dhillon"],
        "the first three of the page's order"
    );
    assert!(cut, "three of six is a cut result");

    let (five, cut) = export(&query("").export_limit(5), &db).await;
    assert_eq!(exported_names(&five).len(), 5);
    assert!(cut);

    // A limit as large as the result is not a cut, nor is one past the ceiling.
    for limit in [6, 7, u64::MAX] {
        let (file, cut) = export(&query("").export_limit(limit), &db).await;
        assert_eq!(exported_names(&file).len(), 6, "limit {limit}");
        assert!(!cut, "limit {limit}: nothing was left out");
    }
    done(db, dir).await;
}

#[tokio::test]
async fn a_long_export_goes_on_from_its_position_and_stops_exactly_at_its_limit() {
    let (db, dir) = staff().await;
    more_people(&db, 4_500).await;

    // More rows than one read takes: no row twice, none left out, in the page's order.
    let (all, cut) = export(&query(""), &db).await;
    let names = exported_names(&all);
    assert_eq!(names.len(), 4_506);
    assert!(!cut);
    assert!(
        names.windows(2).all(|pair| pair[0] < pair[1]),
        "in order, without a repeat"
    );

    let (some, cut) = export(&query("").export_limit(4_321), &db).await;
    let names = exported_names(&some);
    assert_eq!(names.len(), 4_321);
    assert!(cut);
    assert!(names.windows(2).all(|pair| pair[0] < pair[1]));

    // By a column that isn't the key, and downwards.
    let (some, cut) = export(&query("sort=-pay").export_limit(2_500), &db).await;
    assert_eq!(exported_names(&some).len(), 2_500);
    assert!(cut);
    done(db, dir).await;
}

const VIEWS: &str = "CREATE TABLE navree_ui_table_views (id VARCHAR(64) PRIMARY KEY, owner VARCHAR(64) NOT NULL, table_id VARCHAR(64) NOT NULL, name VARCHAR(200) NOT NULL, query TEXT NOT NULL, is_default BOOLEAN NOT NULL)";

#[tokio::test]
async fn a_person_keeps_a_limited_number_of_saved_views_of_a_table() {
    let (db, dir) = staff().await;
    db.pool()
        .execute_script(VIEWS)
        .await
        .expect("the views table");
    let view = query("filter[city][contains]=Amritsar");
    // The owner names the workspace and the person, so each workspace's people have their own.
    let owner = "ws1:gurpreet";
    for n in 1..=MAX_SAVED_VIEWS {
        save_view(&db, owner, "staff", &format!("View {n}"), &view, n == 1)
            .await
            .expect("saved");
    }

    let full = save_view(&db, owner, "staff", "One more", &view, true)
        .await
        .expect_err("the limit");
    let refusal = TooManyViews::of(&full).expect("the error says why");
    assert_eq!(refusal.limit, MAX_SAVED_VIEWS);
    assert!(refusal.to_string().contains("50"), "{refusal}");
    // Nothing was written, and the default is the one it was.
    let kept = list_views(&db, owner, "staff").await.unwrap();
    assert_eq!(u64::try_from(kept.len()).unwrap(), MAX_SAVED_VIEWS);
    assert_eq!(kept.iter().filter(|view| view.is_default).count(), 1);
    assert!(
        kept.iter()
            .any(|view| view.name == "View 1" && view.is_default)
    );

    // Another owner and another table have views of their own.
    save_view(&db, "ws2:gurpreet", "staff", "Mine", &view, false)
        .await
        .expect("another owner");
    save_view(&db, owner, "payroll", "Mine", &view, false)
        .await
        .expect("another table");

    // Deleting one makes room.
    delete_view(&db, owner, &kept[0].id).await.unwrap();
    save_view(&db, owner, "staff", "One more", &view, false)
        .await
        .expect("there is room again");
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
