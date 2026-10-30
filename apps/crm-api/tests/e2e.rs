//! The whole CRM API (`crm_api::app`) end to end, across the feature
//! modules: a rep builds a pipeline (company, contact, deal, task), a
//! manager reassigns the deal and the new owner is notified once the queue
//! runs, ownership holds across modules, the dashboard reflects the deal
//! and every change is in the audit trail.

use chrono::Duration;
use crm_testkit::{CrmClient, Role, TestCrm};
use navree::audit::AuditLog;
use navree::contracts::Clock;
use navree::mail::{Mailer, Message};
use serde_json::{Value, json};

async fn crm() -> TestCrm {
    TestCrm::builder().app(crm_api::app).build().await
}

/// `POST path` as `client`, expecting 201 with a `Location` header naming
/// the new record; returns the body.
async fn create(client: &CrmClient, path: &str, body: Value) -> Value {
    let response = client
        .post(path)
        .json(&body)
        .send()
        .await
        .assert_status(201);
    let created = response.json_value();
    let id = created["id"].as_str().expect("an id").to_owned();
    assert_eq!(
        response.header("location"),
        Some(format!("{path}/{id}").as_str()),
        "{created}"
    );
    created
}

fn id(record: &Value) -> String {
    record["id"].as_str().expect("an id").to_owned()
}

async fn notifications(client: &CrmClient) -> Vec<Value> {
    client
        .get("/api/notifications")
        .send()
        .await
        .assert_status(200)
        .json_value()["data"]
        .as_array()
        .expect("data")
        .clone()
}

async fn mails(crm: &TestCrm) -> Vec<Message> {
    crm.get::<Mailer>()
        .await
        .array_transport()
        .expect("the array transport")
        .sent()
}

/// The audit actions recorded for one record, oldest first (after the
/// background writer has caught up: access denials are written there).
async fn audit_actions(
    crm: &TestCrm,
    admin: &CrmClient,
    subject_type: &str,
    subject_id: &str,
) -> Vec<String> {
    crm.get::<AuditLog>().await.flush().await;
    let page = admin
        .get("/api/audit")
        .query(&json!({ "subject_type": subject_type, "subject_id": subject_id }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    let mut actions: Vec<String> = page["data"]
        .as_array()
        .expect("data")
        .iter()
        .map(|entry| entry["action"].as_str().expect("an action").to_owned())
        .collect();
    actions.reverse();
    actions
}

#[tokio::test]
async fn a_deal_from_first_contact_to_reassignment() {
    let crm = crm().await;
    let rep1 = crm.login_as(Role::Rep1).await;
    let rep2 = crm.login_as(Role::Rep2).await;
    let manager = crm.login_as(Role::Manager).await;
    let admin = crm.login_as(Role::Admin).await;

    // Rep 1 builds a pipeline: company -> contact -> deal -> task.
    let company = create(
        &rep1,
        "/api/v1/companies",
        json!({ "name": "Globex", "domain": "globex.example", "industry": "Energy" }),
    )
    .await;
    assert_eq!(company["__type"], "Company");
    assert_eq!(
        company["owner"],
        json!({ "id": rep1.id(), "name": Role::Rep1.name() })
    );

    let contact = create(
        &rep1,
        "/api/v1/contacts",
        json!({
            "first_name": "Hank",
            "last_name": "Scorpio",
            "email": "hank@globex.example",
            "company_id": id(&company),
        }),
    )
    .await;
    assert_eq!(contact["__type"], "Contact");
    assert_eq!(contact["status"], "lead");
    assert_eq!(
        contact["company"],
        json!({ "id": id(&company), "name": "Globex" })
    );

    let deal = create(
        &rep1,
        "/api/v1/deals",
        json!({
            "title": "Globex doomsday device",
            "company_id": id(&company),
            "contact_id": id(&contact),
            "amount_cents": 1_250_000,
            "stage": "proposal",
            "expected_close_date": "2026-12-01",
        }),
    )
    .await;
    let deal_id = id(&deal);
    assert_eq!(deal["__type"], "Deal");
    assert_eq!(deal["owner_id"], json!(rep1.id()));
    assert_eq!(deal["probability"], 50);
    assert_eq!(deal["currency"], "USD");
    assert_eq!(
        deal["company"],
        json!({ "id": id(&company), "name": "Globex" })
    );
    assert_eq!(
        deal["contact"],
        json!({ "id": id(&contact), "full_name": "Hank Scorpio" })
    );

    let due_at = crm.clock().now() + Duration::hours(3);
    let task = create(
        &rep1,
        "/api/v1/activities",
        json!({
            "kind": "task",
            "subject": "Send the proposal",
            "due_at": due_at,
            "subject_type": "deal",
            "subject_id": deal_id,
            "assignee_id": rep1.id(),
        }),
    )
    .await;
    assert_eq!(task["__type"], "Activity");
    assert_eq!(task["subject_label"], "Globex doomsday device");
    assert_eq!(
        task["assignee"],
        json!({ "id": rep1.id(), "name": Role::Rep1.name() })
    );

    // The company page counts what hangs off it.
    rep1.get(&format!("/api/v1/companies/{}", id(&company)))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("contacts_count", json!(1))
        .assert_json_path("open_deals_count", json!(1))
        .assert_json_path("open_deals_amount_cents", json!(1_250_000));

    // The dashboard reflects the deal and rep 1's task.
    let dashboard = rep1
        .get("/api/v1/dashboard")
        .send()
        .await
        .assert_status(200)
        .json_value();
    let proposal = dashboard["pipeline"]
        .as_array()
        .expect("pipeline")
        .iter()
        .find(|stage| stage["stage"] == "proposal")
        .cloned()
        .expect("the proposal stage");
    assert_eq!(
        proposal,
        json!({ "stage": "proposal", "count": 1, "amount_cents": 1_250_000 })
    );
    assert_eq!(
        dashboard["open_deals"],
        json!({ "count": 1, "amount_cents": 1_250_000, "weighted_amount_cents": 625_000 })
    );
    assert_eq!(
        dashboard["tasks"],
        json!({ "overdue": 0, "due_today": 1, "upcoming": 0 })
    );
    assert_eq!(dashboard["recent_activities"][0]["id"], json!(id(&task)));

    // Rep 2 reads rep 1's deal, but may not move it.
    let deal_path = format!("/api/v1/deals/{deal_id}");
    rep2.get(&deal_path).send().await.assert_status(200);
    rep2.post(&format!("{deal_path}/move"))
        .json(&json!({ "stage": "negotiation" }))
        .send()
        .await
        .assert_error(403, None);
    // A rep cannot hand their deal to someone else.
    rep1.patch(&deal_path)
        .json(&json!({ "owner_id": rep2.id() }))
        .send()
        .await
        .assert_error(403, None);

    // The manager reassigns the deal to rep 2.
    manager
        .patch(&deal_path)
        .json(&json!({ "owner_id": rep2.id() }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("owner_id", json!(rep2.id()))
        .assert_json_path("owner.name", json!(Role::Rep2.name()));

    // The notification is queued: nothing arrives until a worker runs.
    assert!(notifications(&rep2).await.is_empty());
    assert!(mails(&crm).await.is_empty());
    let stats = crm.run_queue().await;
    assert_eq!(stats.failed, 0, "{stats:?}");
    let received = notifications(&rep2).await;
    assert_eq!(received.len(), 1, "{received:?}");
    assert_eq!(received[0]["type"], "deal.assigned");
    assert_eq!(received[0]["data"]["deal_id"], json!(deal_id));
    assert_eq!(
        received[0]["data"]["url"],
        json!(format!("/deals/{deal_id}"))
    );
    rep2.get("/api/notifications/unread-count")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("count", json!(1));
    let sent = mails(&crm).await;
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0].to, [Role::Rep2.email()]);
    assert!(notifications(&rep1).await.is_empty());

    // Now it is rep 2's: rep 1 may no longer move it, rep 2 may.
    rep1.post(&format!("{deal_path}/move"))
        .json(&json!({ "stage": "negotiation" }))
        .send()
        .await
        .assert_error(403, None);
    rep2.post(&format!("{deal_path}/move"))
        .json(&json!({ "stage": "negotiation", "position": 0 }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("stage", json!("negotiation"))
        .assert_json_path("probability", json!(75));

    // Rep 1 still owns the task on it and completes it.
    rep1.post(&format!("/api/v1/activities/{}/complete", id(&task)))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("completed_at", json!(crm.clock().now()));
    rep1.get("/api/v1/dashboard")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("tasks.due_today", json!(0))
        .assert_json_path("open_deals.weighted_amount_cents", json!(937_500));

    // Every change is in the audit trail, attributed to who made it.
    assert_eq!(
        audit_actions(&crm, &admin, "Company", &id(&company)).await,
        ["company.created"]
    );
    assert_eq!(
        audit_actions(&crm, &admin, "Contact", &id(&contact)).await,
        ["contact.created"]
    );
    // Data changes, and the refused attempts (rep 2's move, rep 1's move
    // after the reassignment) as access denials.
    let deal_actions = audit_actions(&crm, &admin, "Deal", &deal_id).await;
    let (denied, changes): (Vec<&str>, Vec<&str>) = deal_actions
        .iter()
        .map(String::as_str)
        .partition(|action| *action == "access.denied");
    assert_eq!(changes, ["deal.created", "deal.updated", "deal.updated"]);
    assert_eq!(denied.len(), 2, "{deal_actions:?}");
    assert_eq!(
        audit_actions(&crm, &admin, "Activity", &id(&task)).await,
        ["activity.created", "activity.updated"]
    );
    let reassignment = admin
        .get("/api/audit")
        .query(&json!({
            "subject_type": "Deal",
            "subject_id": deal_id,
            "actor_id": manager.id(),
        }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(reassignment["data"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        reassignment["data"][0]["old_values"]["owner_id"],
        json!(rep1.id())
    );
    assert_eq!(
        reassignment["data"][0]["new_values"]["owner_id"],
        json!(rep2.id())
    );
    // Reps do not read the audit log.
    rep1.get("/api/audit").send().await.assert_error(403, None);
    crm.shutdown().await;
}

#[tokio::test]
async fn deleted_companies_drop_out_of_every_module() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let company = create(&rep, "/api/v1/companies", json!({ "name": "Initech" })).await;
    let path = format!("/api/v1/companies/{}", id(&company));
    rep.delete(&path).send().await.assert_status(204);

    rep.get(&path).send().await.assert_error(404, None);
    rep.get("/api/v1/companies")
        .query(&[("search", "Initech")])
        .send()
        .await
        .assert_status(200)
        .assert_json_path("meta.total", json!(0));
    // Nothing new can be attached to it.
    rep.post("/api/v1/contacts")
        .json(&json!({
            "first_name": "Peter",
            "last_name": "Gibbons",
            "company_id": id(&company),
        }))
        .send()
        .await
        .assert_validation_error("company_id");
    rep.post("/api/v1/deals")
        .json(&json!({
            "title": "TPS reports",
            "company_id": id(&company),
            "amount_cents": 100,
        }))
        .send()
        .await
        .assert_validation_error("company_id");
    rep.post("/api/v1/activities")
        .json(&json!({
            "kind": "note",
            "subject": "Hello",
            "subject_type": "company",
            "subject_id": id(&company),
        }))
        .send()
        .await
        .assert_validation_error("subject_id");
    crm.shutdown().await;
}

#[tokio::test]
async fn deleting_a_deal_deletes_its_activities() {
    let crm = crm().await;
    let rep1 = crm.login_as(Role::Rep1).await;
    let rep2 = crm.login_as(Role::Rep2).await;
    let admin = crm.login_as(Role::Admin).await;
    let company = create(&rep1, "/api/v1/companies", json!({ "name": "Umbrella" })).await;
    let deal =
        |title: &str| json!({ "title": title, "company_id": id(&company), "amount_cents": 500 });
    let doomed = id(&create(&rep1, "/api/v1/deals", deal("Doomed")).await);
    let kept = id(&create(&rep1, "/api/v1/deals", deal("Kept")).await);
    let activity = |deal: &str, subject: &str| {
        json!({
            "kind": "task",
            "subject": subject,
            "due_at": crm.clock().now(),
            "subject_type": "deal",
            "subject_id": deal,
        })
    };
    // Notes and tasks by the owner and by a colleague.
    let own = id(&create(&rep1, "/api/v1/activities", activity(&doomed, "Mine")).await);
    let other = id(&create(&rep2, "/api/v1/activities", activity(&doomed, "Theirs")).await);
    let survivor = id(&create(&rep1, "/api/v1/activities", activity(&kept, "Stays")).await);
    let on_company = id(&create(
        &rep1,
        "/api/v1/activities",
        json!({
            "kind": "note",
            "subject": "About Umbrella",
            "subject_type": "company",
            "subject_id": id(&company),
        }),
    )
    .await);

    rep1.delete(&format!("/api/v1/deals/{doomed}"))
        .send()
        .await
        .assert_status(204);

    let listed: Vec<String> = rep1
        .get("/api/v1/activities")
        .send()
        .await
        .assert_status(200)
        .json_value()["data"]
        .as_array()
        .expect("data")
        .iter()
        .map(id)
        .collect();
    let mut listed = listed;
    listed.sort();
    let mut expected = vec![survivor, on_company];
    expected.sort();
    assert_eq!(listed, expected);
    rep1.get("/api/v1/dashboard")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("tasks.due_today", json!(1));
    // The deletions are audited as the deal's deleter's.
    for activity in [&own, &other] {
        assert_eq!(
            audit_actions(&crm, &admin, "Activity", activity).await,
            ["activity.created", "activity.deleted"]
        );
    }
    crm.shutdown().await;
}

#[tokio::test]
async fn errors_share_one_json_shape_across_modules() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;

    // 401 without a token, on every module's routes.
    for path in [
        "/api/v1/me",
        "/api/v1/users",
        "/api/v1/companies",
        "/api/v1/contacts",
        "/api/v1/deals",
        "/api/v1/deals/board",
        "/api/v1/activities",
        "/api/v1/dashboard",
        "/api/me/abilities",
        "/api/notifications",
    ] {
        crm.client()
            .get(path)
            .send()
            .await
            .assert_error(401, "Unauthorized");
    }

    // 400: `message` lists every problem, `errors` maps them to fields.
    let body = rep
        .post("/api/v1/companies")
        .json(&json!({ "name": " ", "website": "x".repeat(300) }))
        .send()
        .await
        .assert_validation_error("name")
        .assert_validation_error("website")
        .assert_error(400, "name should not be empty")
        .json_value();
    assert_eq!(body["statusCode"], 400);
    assert_eq!(body["error"], "Bad Request");
    assert_eq!(body["message"].as_array().map(Vec::len), Some(2), "{body}");
    rep.post("/api/v1/contacts")
        .json(&json!({ "first_name": "Ada", "last_name": "Lovelace", "status": "vip" }))
        .send()
        .await
        .assert_validation_error("status");
    rep.post("/api/v1/deals")
        .json(&json!({ "title": "Lost cause" }))
        .send()
        .await
        .assert_validation_error("company_id");

    // 404 for unknown records, 403 for what the rules refuse.
    rep.post(&format!("/api/v1/deals/{}/move", uuid::Uuid::now_v7()))
        .json(&json!({ "stage": "won" }))
        .send()
        .await
        .assert_error(404, None);
    rep.get("/api/v1/contacts/export")
        .send()
        .await
        .assert_error(403, None);
    rep.post("/api/v1/users")
        .json(&json!({
            "name": "Eve",
            "email": "eve@navree.test",
            "password": "a long password",
            "role": "admin",
        }))
        .send()
        .await
        .assert_error(403, None);
    crm.shutdown().await;
}
