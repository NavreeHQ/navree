//! A searched list can't reach past the scope it's given: the text is bound, the
//! columns come from the code, the page size and offset are capped, paging neither
//! skips nor repeats, and a workspace-scoped model with row-level security on
//! PostgreSQL answers only its own rows, for a role that isn't a superuser.

use navree::db::Executor as _;
use navree::db::{Db, DbOptions};
use navree::orm::Model;
use navree::ui::{Choice, OptionQuery, OptionSource};
use uuid::Uuid;

#[derive(Model, Clone, Debug, Default, serde::Serialize)]
#[model(table = "opt_people", workspace)]
pub struct OptPerson {
    pub id: i64,
    pub workspace_id: Uuid,
    pub name: String,
    pub email: String,
}

const PEOPLE: OptionSource = OptionSource::new("id", "name").search_in(&["name", "email"]);

fn own_database(url: &str, name: &str) -> String {
    let (base, _) = url.rsplit_once('/').expect("a URL with a database");
    format!("{base}/{name}")
}

fn as_role(url: &str, role: &str) -> String {
    let rest = url.split_once("://").expect("a scheme").1;
    let host = rest.split_once('@').expect("a user").1;
    format!("postgres://{role}:opt_app_pw@{host}")
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

async fn find(
    db: &impl navree::orm::Executor,
    scope: navree::orm::Builder<OptPerson>,
    query: &str,
) -> String {
    PEOPLE
        .find(
            scope,
            &OptionQuery::parse(query),
            db,
            "/people/options",
            |record| {
                let person = record.into_inner();
                Choice::new(person.id.to_string(), person.name)
            },
        )
        .await
        .unwrap()
        .to_html()
}

#[tokio::test]
async fn a_searched_list_stays_in_its_workspace_binds_its_text_and_pages_without_gaps() {
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
        .execute_script("DROP DATABASE IF EXISTS ui_opt_scoping WITH (FORCE)")
        .await;
    admin
        .pool()
        .execute_script("CREATE DATABASE ui_opt_scoping")
        .await
        .unwrap();
    let own = own_database(&url, "ui_opt_scoping");
    let owner = Db::connect(&DbOptions {
        url: own.clone(),
        ..DbOptions::default()
    })
    .await
    .unwrap();
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    for statement in [
        "DO $$ BEGIN IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'opt_app') THEN CREATE ROLE opt_app LOGIN PASSWORD 'opt_app_pw' NOSUPERUSER NOBYPASSRLS; END IF; END $$".to_owned(),
        "CREATE TABLE opt_people (id BIGINT PRIMARY KEY, workspace_id UUID NOT NULL, name TEXT NOT NULL, email TEXT NOT NULL)".to_owned(),
        format!("INSERT INTO opt_people SELECT n, CASE WHEN n % 2 = 0 THEN '{a}'::uuid ELSE '{b}'::uuid END, 'Person ' || n, 'p' || n || '@example.com' FROM generate_series(1, 400) n"),
        format!("INSERT INTO opt_people VALUES (1001, '{a}', '100%_literal', 'pct@example.com'), (1002, '{a}', 'O''Brien \\ x', 'ob@example.com'), (1003, '{b}', 'Secret of B', 'bsecret@example.com')"),
        "ALTER TABLE opt_people ENABLE ROW LEVEL SECURITY".to_owned(),
        "ALTER TABLE opt_people FORCE ROW LEVEL SECURITY".to_owned(),
        "CREATE POLICY opt_people_scope ON opt_people USING (workspace_id = NULLIF(current_setting('navree.workspace', true), '')::uuid)".to_owned(),
        "GRANT SELECT ON opt_people TO opt_app".to_owned(),
    ] {
        owner
            .pool()
            .execute_script(&statement)
            .await
            .unwrap_or_else(|error| panic!("{statement}: {error}"));
    }
    let db = Db::connect(&DbOptions {
        url: as_role(&own, "opt_app"),
        ..DbOptions::default()
    })
    .await
    .unwrap();

    // No workspace entered and row-level security on: nothing, not everything.
    let none = find(&db, OptPerson::query().without_workspace_scope(), "").await;
    assert!(values(&none).is_empty(), "no setting, no options: {none}");

    // Workspace A, by the ORM's scope and by the policy.
    let uow = db.uow(None, None).await;
    uow.set_local("navree.workspace", &a.to_string())
        .await
        .unwrap();
    let first = find(&uow, OptPerson::query().for_workspace(a), "").await;
    assert_eq!(values(&first).len(), 20, "the default page");
    // The policy holds even if the ORM's scope is opted out of.
    let policy = find(
        &uow,
        OptPerson::query().without_workspace_scope(),
        "limit=50",
    )
    .await;
    assert_eq!(values(&policy).len(), 50);
    for id in values(&policy) {
        assert!(
            id.parse::<i64>().unwrap() % 2 == 0 || id == "1001" || id == "1002",
            "{id}"
        );
    }
    // B's row is invisible by name, and by its id.
    let secret = find(&uow, OptPerson::query().for_workspace(a), "q=Secret").await;
    assert!(values(&secret).is_empty(), "{secret}");
    let by_email = find(&uow, OptPerson::query().for_workspace(a), "q=bsecret%40").await;
    assert!(values(&by_email).is_empty(), "{by_email}");

    // What a handler asks of a posted value: pickable by this caller, by the ORM's scope
    // and by the policy alike, and not by a caller who entered no workspace.
    let ask = |scope: navree::orm::Builder<OptPerson>, values: &'static [&'static str]| {
        let uow = &uow;
        async move {
            PEOPLE
                .all_pickable(scope, values.iter().copied(), uow, OptionSource::as_int)
                .await
                .unwrap()
        }
    };
    assert!(ask(OptPerson::query().for_workspace(a), &["2", "4", "1001"]).await);
    assert!(!ask(OptPerson::query().for_workspace(a), &["2", "3"]).await);
    assert!(!ask(OptPerson::query().for_workspace(a), &["1003"]).await);
    assert!(
        !ask(OptPerson::query().without_workspace_scope(), &["2", "3"]).await,
        "the policy holds when the ORM's scope is left out"
    );
    assert!(
        !PEOPLE
            .all_pickable(
                OptPerson::query().without_workspace_scope(),
                ["2"],
                &db,
                OptionSource::as_int,
            )
            .await
            .unwrap(),
        "with no workspace entered nothing is pickable"
    );

    // Text is data: a percent sign, an underscore, a quote and a backslash match themselves.
    let percent = find(&uow, OptPerson::query().for_workspace(a), "q=%25").await;
    assert_eq!(
        values(&percent),
        ["1001"],
        "a % is a character, not a wildcard: {percent}"
    );
    let underscore = find(&uow, OptPerson::query().for_workspace(a), "q=_l").await;
    assert_eq!(values(&underscore), ["1001"], "{underscore}");
    let quote = find(&uow, OptPerson::query().for_workspace(a), "q=O%27Brien").await;
    assert_eq!(values(&quote), ["1002"], "{quote}");
    let slash = find(&uow, OptPerson::query().for_workspace(a), "q=%5C").await;
    assert_eq!(values(&slash), ["1002"], "{slash}");
    let injection = find(
        &uow,
        OptPerson::query().for_workspace(a),
        "q=%27%29+OR+1%3D1+--",
    )
    .await;
    assert!(values(&injection).is_empty(), "{injection}");
    // Search runs over every column the code names: the email finds the row too.
    let email = find(&uow, OptPerson::query().for_workspace(a), "q=pct%40").await;
    assert_eq!(values(&email), ["1001"], "{email}");
    // Case doesn't matter.
    let loud = find(&uow, OptPerson::query().for_workspace(a), "q=PERSON+10").await;
    assert!(values(&loud).contains(&"10".to_owned()), "{loud}");

    // The caps: a limit past 50, an offset past the end, a bad number.
    let big = find(&uow, OptPerson::query().for_workspace(a), "limit=100000").await;
    assert_eq!(values(&big).len(), 50);
    let far = find(&uow, OptPerson::query().for_workspace(a), "after=99999999").await;
    assert!(values(&far).is_empty(), "{far}");
    let bad = find(
        &uow,
        OptPerson::query().for_workspace(a),
        "limit=-1&after=x",
    )
    .await;
    assert_eq!(values(&bad).len(), 20, "{bad}");

    // The builder's own limits hold: an app that narrowed the query sees only that.
    let narrowed = find(
        &uow,
        OptPerson::query()
            .for_workspace(a)
            .where_(navree::orm::col("id").lt(navree::orm::Value::I64(11))),
        "limit=50",
    )
    .await;
    // In label order: "Person 10" comes before "Person 2".
    assert_eq!(values(&narrowed), ["10", "2", "4", "6", "8"], "{narrowed}");

    // Paging walks every row once: no gap, no repeat.
    let mut seen: Vec<String> = Vec::new();
    let mut offset = 0;
    loop {
        let page = find(
            &uow,
            OptPerson::query().for_workspace(a),
            &format!("limit=30&after={offset}"),
        )
        .await;
        seen.extend(values(&page));
        match next_offset(&page) {
            Some(next) => {
                assert!(next > offset, "the offset moves forward");
                offset = next;
            }
            None => break,
        }
    }
    assert_eq!(
        seen.len(),
        202,
        "200 people and the two odd ones of workspace A"
    );
    let mut sorted = seen.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), 202, "no row twice");

    // The answer is ordered by the label, then the value: the same twice.
    let again = find(&uow, OptPerson::query().for_workspace(a), "limit=50").await;
    let once = find(&uow, OptPerson::query().for_workspace(a), "limit=50").await;
    assert_eq!(again, once);
}
