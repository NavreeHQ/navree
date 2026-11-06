//! A request never needs two pool connections at once. On SQLite a unit of
//! work holds the write lock on its connection from its first query; a
//! request that then waits for a second connection (to load the
//! `CurrentUser`, the user's roles, ...) starves the pool as soon as about
//! `max_connections` requests run together: the others hold connections
//! while they wait for the lock, and fail with "database is locked".
//!
//! With a pool of one connection (and a short acquire timeout) every such
//! request fails deterministically, so this suite drives the routes that
//! combine a record check, `CurrentUser` and the user resource.

use std::time::{Duration, Instant};

use crm_testkit::{CrmClient, Role, TestCrm};
use serde_json::{Value, json};

async fn crm() -> TestCrm {
    TestCrm::builder()
        .app(crm_api::app)
        .config("database.max_connections", 1)
        .config("database.min_connections", 1)
        .config("database.connect_timeout_secs", 1)
        .build()
        .await
}

async fn create(client: &CrmClient, path: &str, body: Value) -> String {
    client
        .post(path)
        .json(&body)
        .send()
        .await
        .assert_status(201)
        .json_value()["id"]
        .as_str()
        .expect("an id")
        .to_owned()
}

#[tokio::test]
async fn record_checks_with_current_user_need_one_connection() {
    let started = Instant::now();
    let crm = crm().await;
    let manager = crm.login_as(Role::Manager).await;
    let rep = crm.login_as(Role::Rep1).await;
    let admin = crm.login_as(Role::Admin).await;

    let company = create(&rep, "/api/v1/companies", json!({ "name": "Agra" })).await;
    let contact = create(
        &rep,
        "/api/v1/contacts",
        json!({ "first_name": "Anu", "last_name": "Bains", "company_id": company }),
    )
    .await;
    let deal = create(
        &rep,
        "/api/v1/deals",
        json!({ "title": "Renewal", "company_id": company, "amount_cents": 100 }),
    )
    .await;

    rep.patch(&format!("/api/v1/companies/{company}"))
        .json(&json!({ "city": "Lyon" }))
        .send()
        .await
        .assert_status(200);
    rep.patch(&format!("/api/v1/contacts/{contact}"))
        .json(&json!({ "phone": "+91 98765 43210" }))
        .send()
        .await
        .assert_status(200);
    rep.patch(&format!("/api/v1/deals/{deal}"))
        .json(&json!({ "title": "Renewal 2027" }))
        .send()
        .await
        .assert_status(200);
    manager
        .post(&format!("/api/v1/deals/{deal}/move"))
        .json(&json!({ "stage": "proposal", "position": 0 }))
        .send()
        .await
        .assert_status(200);
    manager
        .patch(&format!("/api/v1/deals/{deal}"))
        .json(&json!({ "owner_id": crm.user_id(Role::Rep2) }))
        .send()
        .await
        .assert_status(200);
    admin
        .patch(&format!("/api/v1/users/{}", crm.user_id(Role::Rep2)))
        .json(&json!({ "name": "Tarun B." }))
        .send()
        .await
        .assert_status(200);
    for path in ["/api/v1/deals/board", "/api/v1/dashboard", "/api/auth/me"] {
        rep.get(path).send().await.assert_status(200);
    }
    // A pool wait would take the acquire timeout (4 s here) per request.
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "took {:?}",
        started.elapsed()
    );
    crm.shutdown().await;
}

/// What every open board does on each `deal.updated`: many board reads
/// while a deal moves. With the default pool (10), more requests than
/// connections must queue, not fail with "database is locked".
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn board_reads_and_a_move_under_load() {
    let crm = TestCrm::builder().app(crm_api::app).build().await;
    let manager = crm.login_as(Role::Manager).await;
    let company = create(&manager, "/api/v1/companies", json!({ "name": "Agra" })).await;
    let mut deals = Vec::new();
    for title in ["A", "B", "C"] {
        deals.push(
            create(
                &manager,
                "/api/v1/deals",
                json!({ "title": title, "company_id": company, "amount_cents": 100 }),
            )
            .await,
        );
    }

    let started = Instant::now();
    let mut requests = tokio::task::JoinSet::new();
    for n in 0..24 {
        let client = manager.clone();
        let deal = deals[n % deals.len()].clone();
        requests.spawn(async move {
            let response = match n % 8 {
                0 => {
                    client
                        .post(&format!("/api/v1/deals/{deal}/move"))
                        .json(&json!({ "stage": "proposal", "position": 0 }))
                        .send()
                        .await
                }
                1 => {
                    client
                        .patch(&format!("/api/v1/deals/{deal}"))
                        .json(&json!({ "title": format!("Deal {n}") }))
                        .send()
                        .await
                }
                _ => client.get("/api/v1/deals/board").send().await,
            };
            response.status().as_u16()
        });
    }
    let mut statuses = Vec::new();
    while let Some(status) = requests.join_next().await {
        statuses.push(status.expect("the request task"));
    }
    assert!(statuses.iter().all(|s| *s == 200), "{statuses:?}");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "took {:?}",
        started.elapsed()
    );
    crm.shutdown().await;
}
