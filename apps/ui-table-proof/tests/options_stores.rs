//! A searched list answers the same on every store `navree-orm` speaks to: the text is
//! bound and means itself (a percent sign, an underscore, a quote and a backslash are
//! characters), case and accents don't matter, the columns come from the code, a
//! workspace's rows stay in it, the page size and offset are capped, and paging
//! neither skips nor repeats a row. SQLite always runs; PostgreSQL, MySQL and MongoDB
//! run when `NAVREE_TEST_POSTGRES_URL`, `NAVREE_TEST_MYSQL_URL` and
//! `NAVREE_TEST_MONGODB_URL` are set, each in a database of its own that is dropped
//! afterwards.

use navree::db::Executor as _;
use navree::db::{Db, DbOptions};
use navree::mongo::{MongoDb, MongoOptions};
use navree::orm::{Builder, Executor, Model, Record, Value, col};
use navree::ui::{Choice, OptionQuery, OptionSource};
use uuid::Uuid;

/// What the scenario needs of a person, whichever store holds it.
trait Person: Model {
    fn make(id: i64, workspace: Uuid, name: &str, email: &str) -> Self;
    fn id(&self) -> i64;
    fn name(&self) -> &str;
}

macro_rules! person {
    ($name:ident, $($attrs:tt)*) => {
        #[derive(Model, Clone, Debug, Default, serde::Serialize)]
        #[model($($attrs)*)]
        pub struct $name {
            pub id: i64,
            pub workspace_id: Uuid,
            pub name: String,
            pub email: String,
        }
        impl Person for $name {
            fn make(id: i64, workspace: Uuid, name: &str, email: &str) -> Self {
                Self { id, workspace_id: workspace, name: name.to_owned(), email: email.to_owned() }
            }
            fn id(&self) -> i64 {
                self.id
            }
            fn name(&self) -> &str {
                &self.name
            }
        }
    };
}

person!(SqlPerson, table = "opt_people", workspace);
person!(
    MongoPerson,
    collection = "opt_people",
    driver = "mongodb",
    workspace
);

const PEOPLE: OptionSource = OptionSource::new("id", "name").search_in(&["name", "email"]);
const BY_NAME: OptionSource = OptionSource::new("id", "name");
/// A value column that isn't unique: two people can have one name.
const NAME_IS_THE_VALUE: OptionSource = OptionSource::new("name", "name");

/// The rows: 400 people alternating between workspaces A (even ids) and B (odd), and
/// the awkward ones.
fn seed<P: Person>(a: Uuid, b: Uuid) -> Vec<P> {
    let mut rows: Vec<P> = (1..=400_i64)
        .map(|n| {
            P::make(
                n,
                if n % 2 == 0 { a } else { b },
                &format!("Person {n}"),
                &format!("p{n}@example.com"),
            )
        })
        .collect();
    rows.push(P::make(1001, a, "100%_literal", "pct@example.com"));
    rows.push(P::make(1002, a, "O'Brien \\ x", "ob@example.com"));
    rows.push(P::make(1003, b, "Secret of B", "bsecret@example.com"));
    rows.push(P::make(1004, a, "Équipe Ölwechsel", "equipe@example.com"));
    // Ten people with one name, stored in the opposite order to the one they must
    // come back in: only the value column puts them in order.
    for n in (2001..=2010_i64).rev() {
        rows.push(P::make(n, a, "Same Name", &format!("same{n}@example.com")));
    }
    // Two people with one name, for a value column that repeats a value.
    rows.push(P::make(3001, a, "Twin", "twin1@example.com"));
    rows.push(P::make(3002, a, "Twin", "twin2@example.com"));
    rows
}

/// The values in an answer's markup, in order.
fn values(html: &str) -> Vec<String> {
    html.split("data-value=\"")
        .skip(1)
        .map(|part| part.split('"').next().unwrap_or_default().to_owned())
        .collect()
}

/// The offset a "Show more" link asks for, if there is one.
fn next_offset(html: &str) -> Option<u64> {
    let at = html.find("data-more=\"")?;
    let link = html[at..].split('"').nth(1)?;
    link.split("after=")
        .nth(1)?
        .split(['&', '"'])
        .next()?
        .parse()
        .ok()
}

async fn find<P: Person>(
    source: &OptionSource,
    exec: &impl Executor,
    scope: Builder<P>,
    query: &str,
) -> String {
    source
        .find(
            scope,
            &OptionQuery::parse(query),
            exec,
            "/people/options",
            |record: Record<P>| {
                let person = record.into_inner();
                Choice::new(person.id().to_string(), person.name().to_owned())
            },
        )
        .await
        .unwrap_or_else(|error| panic!("{query}: {error}"))
        .to_html()
}

/// Every claim, on one store.
async fn scenario<P: Person>(exec: &impl Executor, a: Uuid, store: &str) {
    let mine = || P::query().for_workspace(a);
    let ask = |query: &'static str| {
        let scope = mine();
        async move { find(&PEOPLE, exec, scope, query).await }
    };

    // The default page, then the caps.
    let first = ask("").await;
    assert_eq!(values(&first).len(), 20, "{store}: the default page");
    let big = ask("limit=100000").await;
    assert_eq!(values(&big).len(), 50, "{store}: the biggest page");
    let far = ask("after=99999999").await;
    assert!(
        values(&far).is_empty(),
        "{store}: an offset past the end: {far}"
    );
    let bad = ask("limit=-1&after=x").await;
    assert_eq!(
        values(&bad).len(),
        20,
        "{store}: a number that doesn't parse is dropped: {bad}"
    );

    // Another workspace's row is invisible: by its name, its email, and unscoped by id.
    let secret = ask("q=Secret").await;
    assert!(values(&secret).is_empty(), "{store}: {secret}");
    let by_email = ask("q=bsecret%40").await;
    assert!(values(&by_email).is_empty(), "{store}: {by_email}");
    for id in values(&big) {
        let id: i64 = id.parse().unwrap();
        assert!(
            id % 2 == 0
                || [1001, 1002, 1004].contains(&id)
                || (2001..=2010).contains(&id)
                || [3001, 3002].contains(&id),
            "{store}: {id} belongs to the other workspace"
        );
    }

    // Text is data: each of these matches itself and nothing else.
    for (query, expected, what) in [
        (
            "q=%25",
            "1001",
            "a percent sign is a character, not a wildcard",
        ),
        (
            "q=_l",
            "1001",
            "an underscore is a character, not a wildcard",
        ),
        ("q=O%27Brien", "1002", "a quote"),
        ("q=%5C", "1002", "a backslash"),
        ("q=pct%40", "1001", "the email column is searched too"),
        (
            "q=%C3%89QUIPE",
            "1004",
            "capital É, whatever the case of the data",
        ),
        ("q=%C3%A9quipe", "1004", "small é"),
        ("q=%C3%B6lwechsel", "1004", "ö finds Ö"),
        ("q=PERSON+10", "10", "case doesn't matter"),
        (
            "q=PER%00SON+10",
            "10",
            "a NUL is dropped, not sent to the store",
        ),
    ] {
        let found = ask(query).await;
        assert!(
            values(&found).contains(&expected.to_owned()),
            "{store}: {what}: {query} gave {found}"
        );
        if expected != "10" {
            assert_eq!(values(&found), [expected], "{store}: {what}: {query}");
        }
    }
    let injection = ask("q=%27%29+OR+1%3D1+--").await;
    assert!(
        values(&injection).is_empty(),
        "{store}: SQL in the text is only text: {injection}"
    );
    let quotes = ask("q=%22%7D+%7B%22%24ne%22%3A+1%7D").await;
    assert!(
        values(&quotes).is_empty(),
        "{store}: a query document in the text is only text: {quotes}"
    );

    // The builder's own limits hold: an app that narrowed the query sees only that.
    let narrowed = find(
        &PEOPLE,
        exec,
        mine().where_(navree::orm::col(P::meta().key_column()).lt(Value::I64(11))),
        "limit=50",
    )
    .await;
    assert_eq!(
        values(&narrowed),
        ["10", "2", "4", "6", "8"],
        "{store}: in label order, narrowed: {narrowed}"
    );

    // Rows with one label come back in the order of the value column, however the store
    // keeps them.
    let same = ask("q=same+name").await;
    let expected: Vec<String> = (2001..=2010).map(|n| n.to_string()).collect();
    assert_eq!(
        values(&same),
        expected,
        "{store}: one label, in the order of the value: {same}"
    );

    // The label column alone is searched when no columns are named.
    let label_only = find(&BY_NAME, exec, mine(), "q=pct").await;
    assert!(
        values(&label_only).is_empty(),
        "{store}: the email isn't searched unless named: {label_only}"
    );

    // Paging walks every row once: no gap, no repeat.
    let mut seen: Vec<String> = Vec::new();
    let mut offset = 0;
    loop {
        let page = ask_page::<P>(exec, a, offset).await;
        seen.extend(values(&page));
        match next_offset(&page) {
            Some(next) => {
                assert!(next > offset, "{store}: the offset moves forward");
                offset = next;
            }
            None => break,
        }
    }
    assert_eq!(
        seen.len(),
        215,
        "{store}: 200 people and the fifteen odd ones of workspace A"
    );
    let mut sorted = seen.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), 215, "{store}: no row twice");

    // What a handler asks of what a browser posted: is each value pickable by this caller?
    let pickable = |values: &'static [&'static str]| {
        let scope = mine();
        async move {
            PEOPLE
                .all_pickable(scope, values.iter().copied(), exec, OptionSource::as_int)
                .await
                .unwrap_or_else(|error| panic!("{store}: {values:?}: {error}"))
        }
    };
    assert!(
        pickable(&["2", "4", "1001", "2005"]).await,
        "{store}: its own rows"
    );
    assert!(
        pickable(&["2", "2", "02"]).await,
        "{store}: one row said three ways"
    );
    assert!(
        pickable(&[""]).await,
        "{store}: an empty value is no choice"
    );
    assert!(pickable(&[]).await, "{store}: nothing picked");
    assert!(
        !pickable(&["2", "3"]).await,
        "{store}: 3 belongs to the other workspace"
    );
    assert!(
        !pickable(&["1003"]).await,
        "{store}: nor does 1003, the secret one"
    );
    assert!(
        !pickable(&["999999"]).await,
        "{store}: a row that isn't there"
    );
    assert!(
        !pickable(&["2", "x"]).await,
        "{store}: text that isn't a number"
    );
    assert!(!pickable(&["2", "2; DROP"]).await, "{store}: nor is SQL");

    // A value column that repeats a value: the check is about values, not rows. One name on
    // two rows is one value, and it can't carry another workspace's name along.
    let by_value = |values: &'static [&'static str]| {
        let scope = mine();
        async move {
            NAME_IS_THE_VALUE
                .all_pickable(scope, values.iter().copied(), exec, OptionSource::as_text)
                .await
                .unwrap_or_else(|error| panic!("{store}: {values:?}: {error}"))
        }
    };
    assert!(
        by_value(&["Twin"]).await,
        "{store}: one name on two rows is one value"
    );
    assert!(
        by_value(&["Twin", "Same Name"]).await,
        "{store}: two names, three rows and more"
    );
    assert!(
        !by_value(&["Twin", "Secret of B"]).await,
        "{store}: the other workspace's name isn't carried by one that repeats"
    );
    assert!(
        !by_value(&["Twin", "Same Name", "Secret of B"]).await,
        "{store}: nor by two"
    );

    // The same twice, in the same order.
    let again = ask("limit=50").await;
    let once = ask("limit=50").await;
    assert_eq!(again, once, "{store}: a stable order");
}

async fn ask_page<P: Person>(exec: &impl Executor, a: Uuid, offset: u64) -> String {
    find(
        &PEOPLE,
        exec,
        P::query().for_workspace(a),
        &format!("limit=30&after={offset}"),
    )
    .await
}

fn own_database(url: &str, name: &str) -> String {
    let (base, rest) = url.rsplit_once('/').expect("a URL with a database");
    let query = rest.split_once('?').map_or("", |(_, query)| query);
    if query.is_empty() {
        format!("{base}/{name}")
    } else {
        format!("{base}/{name}?{query}")
    }
}

async fn connect(url: String) -> Db {
    Db::connect(&DbOptions {
        url,
        ..DbOptions::default()
    })
    .await
    .expect("connects")
}

fn env_url(name: &str) -> Option<String> {
    let url = std::env::var(name).ok();
    if url.is_none() {
        eprintln!("skipped: {name} isn't set");
    }
    url
}

/// A store with the table and the rows, and the scenario run on it.
async fn run_sql(db: &Db, schema: &str, a: Uuid, b: Uuid, store: &str) {
    let _ = db
        .pool()
        .execute_script("DROP TABLE IF EXISTS opt_people")
        .await;
    db.pool()
        .execute_script(schema)
        .await
        .unwrap_or_else(|error| panic!("{store}: the table: {error}"));
    SqlPerson::query()
        .without_workspace_scope()
        .insert(db, seed::<SqlPerson>(a, b))
        .await
        .unwrap_or_else(|error| panic!("{store}: the rows: {error}"));
    scenario::<SqlPerson>(db, a, store).await;
    joined(db, a, store).await;
}

/// A scoped query that repeats a row, as a join does: person 2 is in two of the
/// caller's projects, 4 in none of them, and 3 (of the other workspace) in one.
async fn joined(db: &Db, a: Uuid, store: &str) {
    let _ = db
        .pool()
        .execute_script("DROP TABLE IF EXISTS opt_members")
        .await;
    for statement in [
        "CREATE TABLE opt_members (person_id BIGINT NOT NULL, project VARCHAR(50) NOT NULL)",
        "INSERT INTO opt_members VALUES (2, 'p1'), (2, 'p2'), (4, 'other'), (3, 'p1')",
    ] {
        db.pool()
            .execute_script(statement)
            .await
            .unwrap_or_else(|error| panic!("{store}: {statement}: {error}"));
    }
    let people_in_my_projects = || {
        SqlPerson::query()
            .for_workspace(a)
            .join(
                "opt_members",
                col("opt_people.id"),
                "=",
                col("opt_members.person_id"),
            )
            .where_(col("opt_members.project").is_in(vec![Value::from("p1"), Value::from("p2")]))
    };
    let source = OptionSource::new("opt_people.id", "opt_people.name");
    let pickable = |values: &'static [&'static str]| {
        let scope = people_in_my_projects();
        let source = &source;
        async move {
            source
                .all_pickable(scope, values.iter().copied(), db, OptionSource::as_int)
                .await
                .unwrap_or_else(|error| panic!("{store}: {values:?}: {error}"))
        }
    };
    assert!(
        pickable(&["2"]).await,
        "{store}: a person on two rows is one value"
    );
    assert!(
        !pickable(&["4"]).await,
        "{store}: 4 is in none of the caller's projects"
    );
    assert!(
        !pickable(&["2", "4"]).await,
        "{store}: a repeated row doesn't make up for a missing one"
    );
    assert!(
        !pickable(&["2", "3"]).await,
        "{store}: nor for a row of the other workspace"
    );
}

#[tokio::test]
async fn a_searched_list_answers_the_same_on_sqlite_postgres_mysql_and_mongodb() {
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    let dir = std::env::temp_dir().join(format!("ui-opt-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let sqlite = connect(format!("sqlite://{}?mode=rwc", dir.join("o.db").display())).await;
    run_sql(
        &sqlite,
        "CREATE TABLE opt_people (id INTEGER PRIMARY KEY, workspace_id BLOB NOT NULL, name TEXT NOT NULL, email TEXT NOT NULL)",
        a,
        b,
        "sqlite",
    )
    .await;
    sqlite.close().await.unwrap();

    if let Some(url) = env_url("NAVREE_TEST_POSTGRES_URL") {
        let admin = connect(url.clone()).await;
        let _ = admin
            .pool()
            .execute_script("DROP DATABASE IF EXISTS ui_opt_stores WITH (FORCE)")
            .await;
        admin
            .pool()
            .execute_script("CREATE DATABASE ui_opt_stores")
            .await
            .unwrap();
        {
            let db = connect(own_database(&url, "ui_opt_stores")).await;
            run_sql(
                &db,
                "CREATE TABLE opt_people (id BIGINT PRIMARY KEY, workspace_id UUID NOT NULL, name TEXT NOT NULL, email TEXT NOT NULL)",
                a,
                b,
                "postgres",
            )
            .await;
            db.close().await.unwrap();
        }
        admin
            .pool()
            .execute_script("DROP DATABASE ui_opt_stores WITH (FORCE)")
            .await
            .unwrap();
    }
    if let Some(url) = env_url("NAVREE_TEST_MYSQL_URL") {
        let admin = connect(url.clone()).await;
        let _ = admin
            .pool()
            .execute_script("DROP DATABASE IF EXISTS ui_opt_stores")
            .await;
        admin
            .pool()
            .execute_script("CREATE DATABASE ui_opt_stores CHARACTER SET utf8mb4")
            .await
            .unwrap();
        {
            let db = connect(own_database(&url, "ui_opt_stores")).await;
            run_sql(
                &db,
                "CREATE TABLE opt_people (id BIGINT PRIMARY KEY, workspace_id BINARY(16) NOT NULL, name VARCHAR(200) NOT NULL, email VARCHAR(200) NOT NULL)",
                a,
                b,
                "mysql",
            )
            .await;
            db.close().await.unwrap();
        }
        admin
            .pool()
            .execute_script("DROP DATABASE ui_opt_stores")
            .await
            .unwrap();
    }
    if let Some(url) = env_url("NAVREE_TEST_MONGODB_URL") {
        let mongo = MongoDb::connect(&MongoOptions::new(own_database(&url, "ui_opt_stores")))
            .await
            .unwrap();
        MongoPerson::query()
            .without_workspace_scope()
            .delete(&mongo)
            .await
            .unwrap();
        MongoPerson::query()
            .without_workspace_scope()
            .insert(&mongo, seed::<MongoPerson>(a, b))
            .await
            .unwrap();
        scenario::<MongoPerson>(&mongo, a, "mongodb").await;
        mongo.database().drop().await.unwrap();
    }
    let _ = std::fs::remove_dir_all(dir);
}
