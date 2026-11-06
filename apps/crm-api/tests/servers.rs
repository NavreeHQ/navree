//! The CRM on the databases it runs on: SQLite, PostgreSQL with
//! TimescaleDB (where the activity timeline is a hypertable) and MySQL
//! (where its UUID keys are 16-byte binaries, the users table's included).
//! Each seeds the demo data and goes through a day of CRM work over the API:
//! sorting and searching beyond ASCII, a company's counts and sums, the
//! board and its moves, tasks by due date on the timeline, the dashboard,
//! the contacts export, soft deletes, users and their roles. PostgreSQL and
//! MySQL must answer every question exactly as SQLite does.
//!
//! The server tests need the driver and a server whose user may create
//! databases: `NAVREE_TEST_POSTGRES_URL` and `NAVREE_TEST_MYSQL_URL`, or the
//! test servers of `pwsh -File scripts/dev-databases.ps1 start`. Without
//! them they skip with a notice. Each works in a database of its own,
//! dropped afterwards:
//!
//! ```text
//! cargo test -p crm-api --features postgres,mysql --test servers
//! ```

use crm_testkit::{CrmClient, Role, TestCrm};
use navree::db::Db;
use navree::orm::testing::database::TestDb;
use navree::schema::Schema;
use serde_json::{Value, json};

/// What a day of CRM work answers, question by question, in a form every
/// database must agree on (no ids; names sorted wherever the database's
/// collation would decide their order).
type Answers = Vec<(&'static str, Value)>;

/// The whole CRM API on `database_url`, or on a temporary SQLite database.
async fn crm(database_url: Option<&str>) -> TestCrm {
    let builder = TestCrm::builder().app(crm_api::app);
    match database_url {
        Some(url) => builder.config("database.url", url),
        None => builder,
    }
    .build()
    .await
}

#[tokio::test]
async fn sqlite_gets_through_a_day_of_crm_work() {
    let crm = crm(None).await;
    let answers = day_of_work(&crm).await;
    crm.shutdown().await;

    let answer = |question: &str| -> Value {
        answers
            .iter()
            .find(|(asked, _)| *asked == question)
            .map(|(_, answer)| answer.clone())
            .unwrap_or_else(|| panic!("no answer to {question}"))
    };
    assert_eq!(
        answer("seeded"),
        json!({ "companies": 20, "contacts": 50, "deals": 30, "activities": 60 })
    );
    let accented = json!(["Électricité Ångström"]);
    for question in [
        "search ÉLECTRICITÉ",
        "search ångström",
        "search electricite.example",
        "industry énergie",
    ] {
        assert_eq!(answer(question), accented, "{question}");
    }
    assert_eq!(
        answer("industry SOFTWARE"),
        json!(["Beas Analytics", "Chandni Cloudworks", "Pipal Software"])
    );
    let found = |query: Value| -> Value {
        answers
            .iter()
            .find(|(asked, answer)| *asked == "contacts found" && answer[0] == query)
            .map(|(_, answer)| answer[1].clone())
            .unwrap_or_else(|| panic!("no contacts found for {query}"))
    };
    assert_eq!(
        found(json!({ "search": "DHILLON" })),
        json!(["Harleen Dhillon", "Manish Dhillon"])
    );
    assert_eq!(
        found(json!({ "search": "simran verma" })),
        json!(["Simran Verma"])
    );
    assert_eq!(
        answer("Doaba Logistics"),
        json!({ "contacts_count": 3, "open_deals_count": 2, "open_deals_amount_cents": 760_000_000 })
    );
    assert_eq!(
        answer("moved to the top of qualified")[0],
        json!(["Grid modernisation", 0])
    );
    assert_eq!(answer("lost without a reason"), json!(400));
    assert_eq!(answer("tasks on the deal"), json!(["Called the grid team"]));
    assert_eq!(answer("task deleted"), json!(204));
    assert_eq!(answer("deleted company"), json!([204, 404]));
    assert_eq!(
        answer("the deal names its deleted company"),
        json!("Électricité Ångström")
    );
    // Every stage keeps its deals at positions 0..n.
    for stage in answer("board at the end of the day").as_array().unwrap() {
        let positions: Vec<i64> = stage["deals"]
            .as_array()
            .unwrap()
            .iter()
            .map(|deal| deal[1].as_i64().unwrap())
            .collect();
        let dense: Vec<i64> = (0..positions.len() as i64).collect();
        assert_eq!(positions, dense, "{stage}");
    }
}

#[tokio::test]
async fn timescaledb_answers_as_sqlite_does() {
    let Some(database) = TestDb::postgres()
        .await
        .expect("a PostgreSQL test database")
    else {
        return;
    };
    let crm = crm(Some(database.url())).await;
    let db = crm.get::<Db>().await;
    assert!(
        Schema::new(&db).is_hypertable("activities").await.unwrap(),
        "the activity timeline is a hypertable"
    );
    let answers = day_of_work(&crm).await;
    drop(db);
    crm.shutdown().await;
    database
        .close()
        .await
        .expect("the test database is dropped");
    assert_same(&answers, &on_sqlite().await);
}

#[tokio::test]
async fn mysql_answers_as_sqlite_does() {
    let Some(database) = TestDb::mysql().await.expect("a MySQL test database") else {
        return;
    };
    let crm = crm(Some(database.url())).await;
    let answers = day_of_work(&crm).await;
    crm.shutdown().await;
    database
        .close()
        .await
        .expect("the test database is dropped");
    assert_same(&answers, &on_sqlite().await);
}

/// The answers on a temporary SQLite database.
async fn on_sqlite() -> Answers {
    let crm = crm(None).await;
    let answers = day_of_work(&crm).await;
    crm.shutdown().await;
    answers
}

fn assert_same(on_server: &Answers, on_sqlite: &Answers) {
    let questions = |answers: &Answers| answers.iter().map(|(q, _)| *q).collect::<Vec<_>>();
    assert_eq!(questions(on_server), questions(on_sqlite));
    for ((question, server), (_, sqlite)) in on_server.iter().zip(on_sqlite) {
        assert_eq!(server, sqlite, "{question}");
    }
}

/// Seeds the demo data and works through the API as a rep, the manager
/// and the admin.
async fn day_of_work(crm: &TestCrm) -> Answers {
    crm.app()
        .run_command("db:seed", &[])
        .await
        .expect("db:seed");
    let rep = crm.login_as(Role::Rep1).await;
    let manager = crm.login_as(Role::Manager).await;
    let admin = crm.login_as(Role::Admin).await;
    let mut answers = Answers::new();

    answers.push((
        "seeded",
        json!({
            "companies": total(&admin, "/api/v1/companies").await,
            "contacts": total(&admin, "/api/v1/contacts").await,
            "deals": total(&admin, "/api/v1/deals").await,
            "activities": total(&admin, "/api/v1/activities").await,
        }),
    ));
    for sort in ["name", "-name", "-created_at"] {
        let page = list(
            &rep,
            "/api/v1/companies",
            json!({ "sort": sort, "per_page": 5 }),
        )
        .await;
        answers.push(("companies sorted", json!([sort, names(&page, "name")])));
    }
    let page = list(&rep, "/api/v1/contacts", json!({ "per_page": 10 })).await;
    answers.push(("contacts in order", names(&page, "full_name")));

    // Searching and filtering ignore case, letters beyond ASCII included.
    let electricite = create(
        &rep,
        "/api/v1/companies",
        json!({ "name": "Électricité Ångström", "domain": "electricite.example", "industry": "Énergie" }),
    )
    .await;
    for (question, query) in [
        ("search ÉLECTRICITÉ", json!({ "search": "ÉLECTRICITÉ" })),
        ("search ångström", json!({ "search": "ångström" })),
        (
            "search electricite.example",
            json!({ "search": "electricite.example" }),
        ),
        ("industry énergie", json!({ "filter[industry]": "énergie" })),
        (
            "industry SOFTWARE",
            json!({ "filter[industry]": "SOFTWARE" }),
        ),
    ] {
        let page = list(&rep, "/api/v1/companies", query).await;
        answers.push((question, sorted(names(&page, "name"))));
    }
    for query in [
        json!({ "search": "DHILLON" }),
        json!({ "search": "simran verma" }),
        json!({ "filter[status]": "customer", "per_page": 100 }),
    ] {
        let page = list(&rep, "/api/v1/contacts", query.clone()).await;
        answers.push((
            "contacts found",
            json!([query, sorted(names(&page, "full_name"))]),
        ));
    }
    let page = list(&rep, "/api/v1/deals", json!({ "search": "RENEWAL" })).await;
    answers.push(("deals found", sorted(names(&page, "title"))));

    // A company's live contacts and open deals, counted and summed.
    let doaba = list(&rep, "/api/v1/companies", json!({ "search": "doaba" })).await;
    let detail = rep
        .get(&format!("/api/v1/companies/{}", id(&doaba["data"][0])))
        .send()
        .await
        .assert_status(200)
        .json_value();
    answers.push((
        "Doaba Logistics",
        json!({
            "contacts_count": detail["contacts_count"],
            "open_deals_count": detail["open_deals_count"],
            "open_deals_amount_cents": detail["open_deals_amount_cents"],
        }),
    ));

    // The board, and a deal moving through it.
    answers.push(("board", board(&rep).await));
    let deal = create(
        &rep,
        "/api/v1/deals",
        json!({ "title": "Grid modernisation", "company_id": id(&electricite), "amount_cents": 250_000_000 }),
    )
    .await;
    let deal_path = format!("/api/v1/deals/{}", id(&deal));
    let moved = |body: Value| rep.post(&format!("{deal_path}/move")).json(&body).send();
    moved(json!({ "stage": "qualified", "position": 0 }))
        .await
        .assert_status(200);
    answers.push((
        "moved to the top of qualified",
        column(&board(&rep).await, "qualified"),
    ));
    answers.push((
        "lost without a reason",
        json!(moved(json!({ "stage": "lost" })).await.status().as_u16()),
    ));
    let lost = moved(json!({ "stage": "lost", "lost_reason": "Budget moved to 2027" }))
        .await
        .assert_status(200)
        .json_value();
    answers.push((
        "lost",
        json!([
            lost["stage"],
            lost["closed_at"],
            lost["lost_reason"],
            lost["probability"]
        ]),
    ));
    answers.push((
        "qualified closes the gap",
        column(&board(&rep).await, "qualified"),
    ));

    // A task on the deal: on the timeline, by due date, done, renamed and
    // deleted.
    let task = create(
        &rep,
        "/api/v1/activities",
        json!({
            "kind": "task",
            "subject": "Call the grid team",
            "due_at": "2026-10-26T15:00:00Z",
            "subject_type": "deal",
            "subject_id": id(&deal),
        }),
    )
    .await;
    let task_path = format!("/api/v1/activities/{}", id(&task));
    for due in ["overdue", "today", "upcoming"] {
        let page = list(
            &rep,
            "/api/v1/activities",
            json!({ "filter[due]": due, "per_page": 100 }),
        )
        .await;
        answers.push(("tasks due", json!([due, sorted(names(&page, "subject"))])));
    }
    let done = rep
        .post(&format!("{task_path}/complete"))
        .send()
        .await
        .assert_status(200)
        .json_value();
    answers.push(("task completed", done["completed_at"].clone()));
    rep.patch(&task_path)
        .json(&json!({ "subject": "Called the grid team" }))
        .send()
        .await
        .assert_status(200);
    let on_deal = list(
        &rep,
        "/api/v1/activities",
        json!({ "filter[subject_type]": "deal", "filter[subject_id]": id(&deal) }),
    )
    .await;
    answers.push(("tasks on the deal", names(&on_deal, "subject")));

    // The dashboard, while the task is on the timeline.
    let dashboard = rep
        .get("/api/v1/dashboard")
        .send()
        .await
        .assert_status(200)
        .json_value();
    for part in ["pipeline", "won_this_month", "open_deals", "tasks"] {
        answers.push(("dashboard", json!([part, dashboard[part]])));
    }
    answers.push((
        "dashboard recent activities",
        names(
            &json!({ "data": dashboard["recent_activities"] }),
            "subject",
        ),
    ));
    answers.push((
        "task deleted",
        json!(rep.delete(&task_path).send().await.status().as_u16()),
    ));

    // The contacts export.
    let csv = manager
        .get("/api/v1/contacts/export")
        .send()
        .await
        .assert_status(200)
        .text();
    // Each line without its first cell, the contact's id.
    let mut lines: Vec<&str> = csv
        .lines()
        .map(|line| line.split_once(',').map_or(line, |(_, rest)| rest))
        .collect();
    let header = lines.remove(0);
    lines.sort_unstable();
    answers.push(("export", json!([header, lines])));

    // A deleted company is gone, and the deal still names it.
    let company_path = format!("/api/v1/companies/{}", id(&electricite));
    let deleted = manager.delete(&company_path).send().await.status().as_u16();
    let after = manager.get(&company_path).send().await.status().as_u16();
    answers.push(("deleted company", json!([deleted, after])));
    let deal = rep
        .get(&deal_path)
        .send()
        .await
        .assert_status(200)
        .json_value();
    answers.push((
        "the deal names its deleted company",
        deal["company"]["name"].clone(),
    ));

    // Users and their roles.
    let users = list(&admin, "/api/v1/users", json!({ "per_page": 100 })).await;
    let users: Vec<Value> = users["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|user| json!([user["name"], user["roles"]]))
        .collect();
    answers.push(("users", json!(users)));

    answers.push(("board at the end of the day", board(&rep).await));
    answers
}

/// `GET path` with `query`, answering 200.
async fn list(client: &CrmClient, path: &str, query: Value) -> Value {
    client
        .get(path)
        .query(&query)
        .send()
        .await
        .assert_status(200)
        .json_value()
}

/// How many records `GET path` lists in all.
async fn total(client: &CrmClient, path: &str) -> Value {
    list(client, path, json!({ "per_page": 1 })).await["meta"]["total"].clone()
}

/// `POST path` with `body`, answering 201; the record created.
async fn create(client: &CrmClient, path: &str, body: Value) -> Value {
    client
        .post(path)
        .json(&body)
        .send()
        .await
        .assert_status(201)
        .json_value()
}

fn id(record: &Value) -> String {
    record["id"].as_str().expect("an id").to_owned()
}

/// The `field` of each record of `page`, in order.
fn names(page: &Value, field: &str) -> Value {
    page["data"]
        .as_array()
        .expect("a page")
        .iter()
        .map(|record| record[field].clone())
        .collect()
}

/// `names` sorted, for results whose order the database's collation
/// decides.
fn sorted(names: Value) -> Value {
    let mut names: Vec<String> = names
        .as_array()
        .expect("names")
        .iter()
        .map(|name| name.as_str().expect("a name").to_owned())
        .collect();
    names.sort();
    json!(names)
}

/// The board: each stage with its count, amount and `[title, position]`
/// of its deals, in board order.
async fn board(client: &CrmClient) -> Value {
    let board = client
        .get("/api/v1/deals/board")
        .send()
        .await
        .assert_status(200)
        .json_value();
    board["stages"]
        .as_array()
        .expect("stages")
        .iter()
        .map(|stage| {
            json!({
                "stage": stage["stage"],
                "count": stage["count"],
                "amount_cents": stage["amount_cents"],
                "deals": stage["deals"]
                    .as_array()
                    .expect("deals")
                    .iter()
                    .map(|deal| json!([deal["title"], deal["position"]]))
                    .collect::<Vec<_>>(),
            })
        })
        .collect()
}

/// The `[title, position]` of the deals in `stage` of `board`.
fn column(board: &Value, stage: &str) -> Value {
    board
        .as_array()
        .expect("stages")
        .iter()
        .find(|column| column["stage"] == stage)
        .map(|column| column["deals"].clone())
        .unwrap_or_else(|| panic!("no {stage} column"))
}
