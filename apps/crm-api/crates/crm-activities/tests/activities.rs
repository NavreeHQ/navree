//! `/api/v1/activities` (spec §7 and §9): create, list with filters,
//! update, complete and delete, with the CRM's access rules.

mod support;

use crm_core::{ActivityKind, Stage, SubjectType};
use crm_testkit::Role;
use serde_json::{Value, json};
use uuid::Uuid;

use support::{NewActivity, NewDeal, activity, at, company, contact, crm, deal, deleted_company};

#[tokio::test]
async fn create_task_on_deal() {
    let crm = crm().await;
    let rep1 = crm.user_id(Role::Rep1);
    let rep2 = crm.user_id(Role::Rep2);
    let acme = company(&crm, rep1, "Acme").await;
    let renewal = deal(
        &crm,
        rep1,
        acme.id,
        NewDeal::open("Acme renewal", Stage::Proposal, 1_200_000),
    )
    .await;
    let client = crm.login_as(Role::Rep1).await;

    let response = client
        .post("/api/v1/activities")
        .json(&json!({
            "kind": "task",
            "subject": "Send the revised proposal",
            "body": "Pricing v2, three-year term",
            "due_at": "2026-10-27T17:00:00+02:00",
            "subject_type": "deal",
            "subject_id": renewal.id,
            "assignee_id": rep2,
        }))
        .send()
        .await
        .assert_status(201);
    let body = response.json_value();
    let id: Uuid = serde_json::from_value(body["id"].clone()).expect("a UUID id");
    assert_eq!(
        response.header("location"),
        Some(format!("/api/v1/activities/{id}").as_str())
    );
    assert_eq!(
        body,
        json!({
            "__type": "Activity",
            "id": id,
            "kind": "task",
            "subject": "Send the revised proposal",
            "body": "Pricing v2, three-year term",
            "due_at": "2026-10-27T15:00:00Z",
            "completed_at": null,
            "subject_type": "deal",
            "subject_id": renewal.id,
            "subject_label": "Acme renewal",
            "owner_id": rep1,
            "owner": { "id": rep1, "name": "Priya Shah" },
            "assignee_id": rep2,
            "assignee": { "id": rep2, "name": "Tom Becker" },
            "created_at": "2026-10-26T10:00:00Z",
        })
    );

    // The deal's timeline lists it.
    let page = client
        .get(&format!(
            "/api/v1/activities?subject_type=deal&subject_id={}",
            renewal.id
        ))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(page["data"], json!([body]));
    assert_eq!(
        page["meta"],
        json!({ "current_page": 1, "per_page": 15, "total": 1, "last_page": 1 })
    );
    crm.shutdown().await;
}

#[tokio::test]
async fn notes_without_assignee_or_due_date_belong_to_their_creator() {
    let crm = crm().await;
    let manager = crm.user_id(Role::Manager);
    let acme = company(&crm, manager, "Acme").await;
    let client = crm.login_as(Role::Manager).await;
    let body = client
        .post("/api/v1/activities")
        .json(&json!({
            "kind": "note",
            "subject": "  Met at the trade fair  ",
            "subject_type": "company",
            "subject_id": acme.id,
        }))
        .send()
        .await
        .assert_status(201)
        .json_value();
    assert_eq!(body["subject"], json!("Met at the trade fair"));
    assert_eq!(body["subject_label"], json!("Acme"));
    assert_eq!(body["body"], Value::Null);
    assert_eq!(body["due_at"], Value::Null);
    assert_eq!(body["assignee_id"], Value::Null);
    assert_eq!(body["assignee"], Value::Null);
    assert_eq!(
        body["owner"],
        json!({ "id": manager, "name": "Marcus Reyes" })
    );
    crm.shutdown().await;
}

#[tokio::test]
async fn subject_must_exist() {
    let crm = crm().await;
    let rep1 = crm.user_id(Role::Rep1);
    let acme = company(&crm, rep1, "Acme").await;
    let gone = deleted_company(&crm, rep1, "Gone Ltd").await;
    let client = crm.login_as(Role::Rep1).await;
    let create = |subject_type: &str, subject_id: Uuid| {
        client.post("/api/v1/activities").json(&json!({
            "kind": "call",
            "subject": "Intro call",
            "subject_type": subject_type,
            "subject_id": subject_id,
        }))
    };

    for (subject_type, subject_id) in [
        // Nothing has this id.
        ("deal", Uuid::now_v7()),
        // A company's id under another type.
        ("contact", acme.id),
        ("deal", acme.id),
        // Soft-deleted.
        ("company", gone.id),
    ] {
        let body = create(subject_type, subject_id)
            .send()
            .await
            .assert_validation_error("subject_id")
            .json_value();
        assert_eq!(
            body["errors"],
            json!({ "subject_id": ["subject_id does not exist"] }),
            "{subject_type} {subject_id}"
        );
    }
    create("company", acme.id).send().await.assert_status(201);

    // The assignee must be a user, too.
    client
        .post("/api/v1/activities")
        .json(&json!({
            "kind": "task",
            "subject": "Follow up",
            "subject_type": "company",
            "subject_id": acme.id,
            "assignee_id": Uuid::now_v7(),
        }))
        .send()
        .await
        .assert_validation_error("assignee_id")
        .assert_error(400, "assignee_id does not exist");
    crm.shutdown().await;
}

#[tokio::test]
async fn invalid_input_is_a_400_listing_every_field() {
    let crm = crm().await;
    let client = crm.login_as(Role::Rep1).await;
    let body = client
        .post("/api/v1/activities")
        .json(&json!({
            "kind": "lunch",
            "subject": "   ",
            "due_at": "tomorrow",
            "subject_type": "account",
            "subject_id": "not-a-uuid",
        }))
        .send()
        .await
        .assert_status(400)
        .json_value();
    let errors = body["errors"].as_object().expect("field errors");
    let mut fields: Vec<&str> = errors.keys().map(String::as_str).collect();
    fields.sort_unstable();
    assert!(
        !fields.is_empty()
            && fields.iter().all(|field| [
                "kind",
                "subject",
                "due_at",
                "subject_type",
                "subject_id"
            ]
            .contains(field)),
        "{body}"
    );

    // Missing required fields (serde reports the first one it meets).
    client
        .post("/api/v1/activities")
        .json(&json!({}))
        .send()
        .await
        .assert_error(400, "kind should not be empty");

    // A blank subject (after trimming).
    let acme = company(&crm, crm.user_id(Role::Rep1), "Acme").await;
    client
        .post("/api/v1/activities")
        .json(&json!({
            "kind": "note",
            "subject": "   ",
            "subject_type": "company",
            "subject_id": acme.id,
        }))
        .send()
        .await
        .assert_validation_error("subject");

    // Guests are turned away before validation.
    crm.client()
        .post("/api/v1/activities")
        .json(&json!({}))
        .send()
        .await
        .assert_error(401, None);
    crm.shutdown().await;
}

/// The `subject`s of a page of activities, in order.
fn subjects(page: &Value) -> Vec<String> {
    page["data"]
        .as_array()
        .unwrap_or_else(|| panic!("a page: {page}"))
        .iter()
        .map(|activity| activity["subject"].as_str().unwrap_or_default().to_owned())
        .collect()
}

async fn list(client: &crm_testkit::CrmClient, query: &str) -> Value {
    client
        .get(&format!("/api/v1/activities?{query}"))
        .send()
        .await
        .assert_status(200)
        .json_value()
}

#[tokio::test]
async fn due_filters_overdue_today_upcoming() {
    // Monday 2026-10-26, 10:00 UTC: "today" is [10-26 00:00, 10-27 00:00).
    let crm = crm().await;
    let rep1 = crm.user_id(Role::Rep1);
    let acme = company(&crm, rep1, "Acme").await;
    let renewal = deal(
        &crm,
        rep1,
        acme.id,
        NewDeal::open("Renewal", Stage::Lead, 100),
    )
    .await;
    let task = |subject: &str| NewActivity::task(subject, renewal.id, rep1);
    for new in [
        task("Overdue").due(at("2026-10-25T23:59:59Z")),
        task("Overdue but done")
            .due(at("2026-10-24T09:00:00Z"))
            .completed(at("2026-10-24T10:00:00Z")),
        task("Today early").due(at("2026-10-26T00:00:00Z")),
        task("Today late").due(at("2026-10-26T23:59:59Z")),
        task("Today done")
            .due(at("2026-10-26T12:00:00Z"))
            .completed(at("2026-10-26T09:00:00Z")),
        task("Tomorrow").due(at("2026-10-27T00:00:00Z")),
        task("Next week").due(at("2026-11-02T09:00:00Z")),
        task("Someday"),
        task("Meeting yesterday")
            .kind(ActivityKind::Meeting)
            .due(at("2026-10-25T15:00:00Z")),
        task("Meeting today")
            .kind(ActivityKind::Meeting)
            .due(at("2026-10-26T15:00:00Z")),
    ] {
        activity(&crm, new).await;
    }
    let client = crm.login_as(Role::Rep1).await;

    // Overdue: open tasks due before today (meetings are never overdue).
    assert_eq!(subjects(&list(&client, "due=overdue").await), ["Overdue"]);
    // Today and upcoming go by the due date, earliest first.
    assert_eq!(
        subjects(&list(&client, "due=today").await),
        ["Today early", "Today done", "Meeting today", "Today late"]
    );
    assert_eq!(
        subjects(&list(&client, "due=today&kind=task&completed=false").await),
        ["Today early", "Today late"]
    );
    assert_eq!(
        subjects(&list(&client, "due=upcoming").await),
        ["Tomorrow", "Next week"]
    );
    client
        .get("/api/v1/activities?due=later")
        .send()
        .await
        .assert_validation_error("due");

    // A day later, by the app's clock (log in again: tokens follow it too).
    crm.clock().set(at("2026-10-27T10:00:00Z"));
    let client = crm.login_as(Role::Rep1).await;
    assert_eq!(
        subjects(&list(&client, "due=overdue").await),
        ["Overdue", "Today early", "Today late"]
    );
    assert_eq!(subjects(&list(&client, "due=today").await), ["Tomorrow"]);
    crm.shutdown().await;
}

#[tokio::test]
async fn list_filters_by_kind_assignee_completion_and_subject() {
    let crm = crm().await;
    let rep1 = crm.user_id(Role::Rep1);
    let rep2 = crm.user_id(Role::Rep2);
    let acme = company(&crm, rep1, "Acme").await;
    let ada = contact(&crm, rep1, "Ada", "Lovelace").await;
    let renewal = deal(
        &crm,
        rep1,
        acme.id,
        NewDeal::open("Renewal", Stage::Lead, 100),
    )
    .await;
    let start = crm_testkit::TestCrm::start();
    let minutes = |n: i64| start + chrono::Duration::minutes(n);
    for new in [
        NewActivity::task("Call Ada", renewal.id, rep1)
            .kind(ActivityKind::Call)
            .on(SubjectType::Contact, ada.id)
            .created(minutes(1)),
        NewActivity::task("Send deck", renewal.id, rep1)
            .assigned(rep2)
            .created(minutes(2)),
        NewActivity::task("Book demo", renewal.id, rep1)
            .assigned(rep2)
            .completed(minutes(3))
            .created(minutes(3)),
        NewActivity::task("Old win", renewal.id, rep1)
            .completed(minutes(4))
            .created(minutes(-60)),
        NewActivity::task("Note on Acme", renewal.id, rep2)
            .kind(ActivityKind::Note)
            .on(SubjectType::Company, acme.id)
            .created(minutes(5)),
    ] {
        activity(&crm, new).await;
    }
    let client = crm.login_as(Role::Rep2).await;

    // Newest first without filters.
    assert_eq!(
        subjects(&list(&client, "").await),
        [
            "Note on Acme",
            "Book demo",
            "Send deck",
            "Call Ada",
            "Old win"
        ]
    );
    assert_eq!(subjects(&list(&client, "kind=call").await), ["Call Ada"]);
    assert_eq!(
        subjects(&list(&client, &format!("assignee_id={rep2}")).await),
        ["Book demo", "Send deck"]
    );
    // Completed ones, most recently completed first.
    assert_eq!(
        subjects(&list(&client, "completed=true").await),
        ["Old win", "Book demo"]
    );
    assert_eq!(
        subjects(&list(&client, "completed=false").await),
        ["Note on Acme", "Send deck", "Call Ada"]
    );
    assert_eq!(
        subjects(&list(&client, "subject_type=company").await),
        ["Note on Acme"]
    );
    assert_eq!(
        subjects(
            &list(
                &client,
                &format!("subject_type=contact&subject_id={}", ada.id)
            )
            .await
        ),
        ["Call Ada"]
    );
    // Pagination keeps the filters.
    let page = list(&client, "completed=false&per_page=2&page=2").await;
    assert_eq!(subjects(&page), ["Call Ada"]);
    assert_eq!(
        page["meta"],
        json!({ "current_page": 2, "per_page": 2, "total": 3, "last_page": 2 })
    );
    for bad in ["completed=maybe", "kind=lunch", "assignee_id=nobody"] {
        client
            .get(&format!("/api/v1/activities?{bad}"))
            .send()
            .await
            .assert_status(400);
    }
    crm.shutdown().await;
}

/// A deal of Acme (owned by rep1) to hang activities on.
async fn renewal(crm: &crm_testkit::TestCrm) -> crm_core::entities::deal::Model {
    let rep1 = crm.user_id(Role::Rep1);
    let acme = company(crm, rep1, "Acme").await;
    deal(
        crm,
        rep1,
        acme.id,
        NewDeal::open("Acme renewal", Stage::Negotiation, 500_000),
    )
    .await
}

#[tokio::test]
async fn complete_sets_completed_at() {
    let crm = crm().await;
    let rep1 = crm.user_id(Role::Rep1);
    let rep2 = crm.user_id(Role::Rep2);
    let deal = renewal(&crm).await;
    let task = activity(
        &crm,
        NewActivity::task("Send contract", deal.id, rep1)
            .assigned(rep2)
            .due(at("2026-10-26T17:00:00Z")),
    )
    .await;
    let path = format!("/api/v1/activities/{}/complete", task.id);

    // The assignee completes it at the app's "now".
    crm.clock().set(at("2026-10-26T11:30:00Z"));
    let assignee = crm.login_as(Role::Rep2).await;
    let body = assignee
        .post(&path)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("__type", json!("Activity"))
        .assert_json_path("completed_at", json!("2026-10-26T11:30:00Z"))
        .json_value();
    assert_eq!(body["id"], json!(task.id));
    assert_eq!(body["subject_label"], json!("Acme renewal"));

    // Completing again keeps the first completion time (within the access
    // token's 15 minutes).
    crm.clock().set(at("2026-10-26T11:40:00Z"));
    assignee
        .post(&path)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("completed_at", json!("2026-10-26T11:30:00Z"));

    // It is listed as completed now.
    let page = assignee
        .get("/api/v1/activities?completed=true")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(page["data"][0]["id"], json!(task.id));

    // Only tasks are completed.
    let note = activity(
        &crm,
        NewActivity::task("Kick-off notes", deal.id, rep1).kind(ActivityKind::Note),
    )
    .await;
    crm.login_as(Role::Rep1)
        .await
        .post(&format!("/api/v1/activities/{}/complete", note.id))
        .send()
        .await
        .assert_error(400, "Only tasks can be completed");
    assignee
        .post(&format!("/api/v1/activities/{}/complete", Uuid::now_v7()))
        .send()
        .await
        .assert_error(404, None);
    crm.shutdown().await;
}

#[tokio::test]
async fn complete_is_for_the_assignee_the_owner_and_managers() {
    let crm = crm().await;
    let rep1 = crm.user_id(Role::Rep1);
    let rep2 = crm.user_id(Role::Rep2);
    let deal = renewal(&crm).await;
    let task = |subject: &str| NewActivity::task(subject, deal.id, rep2);
    let owned = activity(&crm, task("Owned by rep2")).await;
    let assigned = activity(&crm, task("Assigned to rep1").assigned(rep1)).await;
    let others = activity(&crm, task("Only for rep2")).await;
    let for_manager = activity(&crm, task("Anyone's, for the manager")).await;
    let for_admin = activity(&crm, task("Anyone's, for the admin")).await;
    let complete = |id: Uuid| format!("/api/v1/activities/{id}/complete");

    let rep1_client = crm.login_as(Role::Rep1).await;
    rep1_client
        .post(&complete(assigned.id))
        .send()
        .await
        .assert_status(200);
    rep1_client
        .post(&complete(others.id))
        .send()
        .await
        .assert_error(403, None);
    crm.login_as(Role::Rep2)
        .await
        .post(&complete(owned.id))
        .send()
        .await
        .assert_status(200);
    crm.login_as(Role::Manager)
        .await
        .post(&complete(for_manager.id))
        .send()
        .await
        .assert_status(200);
    crm.login_as(Role::Admin)
        .await
        .post(&complete(for_admin.id))
        .send()
        .await
        .assert_status(200);
    crm.client()
        .post(&complete(others.id))
        .send()
        .await
        .assert_error(401, None);
    crm.shutdown().await;
}

#[tokio::test]
async fn rep_cannot_edit_others_activity() {
    let crm = crm().await;
    let rep1 = crm.user_id(Role::Rep1);
    let rep2 = crm.user_id(Role::Rep2);
    let deal = renewal(&crm).await;
    // Rep2's task, assigned to rep1: rep1 may complete it, not change it.
    let theirs = activity(
        &crm,
        NewActivity::task("Task of rep2", deal.id, rep2).assigned(rep1),
    )
    .await;
    let path = format!("/api/v1/activities/{}", theirs.id);
    let rep1_client = crm.login_as(Role::Rep1).await;

    rep1_client
        .patch(&path)
        .json(&json!({ "subject": "Hijacked" }))
        .send()
        .await
        .assert_error(403, None);
    rep1_client
        .delete(&path)
        .send()
        .await
        .assert_error(403, None);
    rep1_client
        .post(&format!("{path}/complete"))
        .send()
        .await
        .assert_status(200);

    // The owner and managers may.
    crm.login_as(Role::Rep2)
        .await
        .patch(&path)
        .json(&json!({ "subject": "Renamed by rep2" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("subject", json!("Renamed by rep2"));
    crm.login_as(Role::Manager)
        .await
        .patch(&path)
        .json(&json!({ "subject": "Renamed by the manager" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("subject", json!("Renamed by the manager"));
    crm.shutdown().await;
}

#[tokio::test]
async fn update_changes_only_the_fields_sent() {
    let crm = crm().await;
    let rep1 = crm.user_id(Role::Rep1);
    let rep2 = crm.user_id(Role::Rep2);
    let deal = renewal(&crm).await;
    let acme_id = deal.company_id;
    let client = crm.login_as(Role::Rep1).await;
    let created = client
        .post("/api/v1/activities")
        .json(&json!({
            "kind": "task",
            "subject": "Draft the SOW",
            "body": "Two phases",
            "due_at": "2026-10-28T12:00:00Z",
            "subject_type": "deal",
            "subject_id": deal.id,
            "assignee_id": rep2,
        }))
        .send()
        .await
        .assert_status(201)
        .json_value();
    let path = format!("/api/v1/activities/{}", created["id"].as_str().unwrap());

    // One field.
    let body = client
        .patch(&path)
        .json(&json!({ "subject": " Draft the SOW v2 " }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    let mut expected = created.clone();
    expected["subject"] = json!("Draft the SOW v2");
    assert_eq!(body, expected);

    // Nulls clear the optional fields; absent ones stay.
    let body = client
        .patch(&path)
        .json(&json!({ "body": null, "due_at": null, "assignee_id": null }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    expected["body"] = Value::Null;
    expected["due_at"] = Value::Null;
    expected["assignee_id"] = Value::Null;
    expected["assignee"] = Value::Null;
    assert_eq!(body, expected);

    // Moving it to another record, with a new kind, due date and assignee.
    let body = client
        .patch(&path)
        .json(&json!({
            "kind": "meeting",
            "subject_type": "company",
            "subject_id": acme_id,
            "due_at": "2026-10-29T09:00:00-04:00",
            "assignee_id": rep1,
        }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(body["kind"], json!("meeting"));
    assert_eq!(body["subject_type"], json!("company"));
    assert_eq!(body["subject_id"], json!(acme_id));
    assert_eq!(body["subject_label"], json!("Acme"));
    assert_eq!(body["due_at"], json!("2026-10-29T13:00:00Z"));
    assert_eq!(
        body["assignee"],
        json!({ "id": rep1, "name": "Priya Shah" })
    );

    // The new values are validated like on create, and nothing changes.
    for (input, field) in [
        (json!({ "subject": "  " }), "subject"),
        (json!({ "subject_type": "contact" }), "subject_id"),
        (json!({ "subject_id": Uuid::now_v7() }), "subject_id"),
        (json!({ "assignee_id": Uuid::now_v7() }), "assignee_id"),
        (json!({ "kind": "lunch" }), "kind"),
        (json!({ "subject": null }), "subject"),
    ] {
        client
            .patch(&path)
            .json(&input)
            .send()
            .await
            .assert_validation_error(field);
    }
    let unchanged = client
        .get("/api/v1/activities?subject_type=company")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(unchanged["data"][0], body);

    client
        .patch(&format!("/api/v1/activities/{}", Uuid::now_v7()))
        .json(&json!({ "subject": "Nobody's" }))
        .send()
        .await
        .assert_error(404, None);
    crm.shutdown().await;
}

#[tokio::test]
async fn owner_deletes_activity() {
    let crm = crm().await;
    let rep1 = crm.user_id(Role::Rep1);
    let deal = renewal(&crm).await;
    let mine = activity(&crm, NewActivity::task("Mine", deal.id, rep1)).await;
    let client = crm.login_as(Role::Rep1).await;
    let path = format!("/api/v1/activities/{}", mine.id);
    client.delete(&path).send().await.assert_status(204);
    client.delete(&path).send().await.assert_error(404, None);
    let page = client
        .get("/api/v1/activities")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(page["meta"]["total"], json!(0));
    crm.shutdown().await;
}

#[tokio::test]
async fn activity_changes_are_audited() {
    let crm = crm().await;
    let deal = renewal(&crm).await;
    let rep1 = crm.login_as(Role::Rep1).await;
    let id = rep1
        .post("/api/v1/activities")
        .json(&json!({
            "kind": "task",
            "subject": "Call the CFO",
            "subject_type": "deal",
            "subject_id": deal.id,
        }))
        .send()
        .await
        .assert_status(201)
        .json_value()["id"]
        .clone();
    let id = id.as_str().expect("an id").to_owned();
    rep1.post(&format!("/api/v1/activities/{id}/complete"))
        .send()
        .await
        .assert_status(200);
    crm.get::<keel::audit::AuditLog>().await.flush().await;

    let page = crm
        .login_as(Role::Admin)
        .await
        .get(&format!("/api/audit?subject_type=Activity&subject_id={id}"))
        .send()
        .await
        .assert_status(200)
        .json_value();
    let entries = page["data"].as_array().expect("entries");
    let actions: Vec<&str> = entries
        .iter()
        .map(|entry| entry["action"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(actions, ["activity.updated", "activity.created"], "{page}");
    assert_eq!(entries[0]["actor_id"], json!(crm.user_id(Role::Rep1)));
    assert_eq!(
        entries[0]["new_values"]["completed_at"],
        json!("2026-10-26T10:00:00Z")
    );
    crm.shutdown().await;
}
