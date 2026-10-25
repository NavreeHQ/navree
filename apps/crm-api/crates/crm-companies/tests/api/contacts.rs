//! `/api/v1/contacts` (spec §7): CRUD, embeds, ownership (§9.3),
//! validation, search, filters, soft deletes and the CSV export.

use crm_testkit::Role;
use sea_orm::QueryFilter;
use serde_json::{Value, json};

use crate::support::{START, column, create_company, create_contact, crm, id_of, list, tick};

#[tokio::test]
async fn contacts_crud_happy_path() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let rep_id = crm.user_id(Role::Rep1);
    let company = create_company(&rep, json!({ "name": "Acme Corp" })).await;
    let company_id = id_of(&company);

    // Create: status defaults to `lead`; the company and owner are embedded.
    let response = rep
        .post("/api/v1/contacts")
        .json(&json!({
            "first_name": "Ada",
            "last_name": "Lovelace",
            "email": "ada@acme.com",
            "phone": "+44 20 7946 0000",
            "title": "CTO",
            "company_id": company_id,
        }))
        .send()
        .await
        .assert_status(201);
    let created = response.json_value();
    let id = id_of(&created);
    assert_eq!(
        response.header("location"),
        Some(format!("/api/v1/contacts/{id}").as_str())
    );
    assert_eq!(
        created,
        json!({
            "__type": "Contact",
            "id": id,
            "first_name": "Ada",
            "last_name": "Lovelace",
            "full_name": "Ada Lovelace",
            "email": "ada@acme.com",
            "phone": "+44 20 7946 0000",
            "title": "CTO",
            "status": "lead",
            "company_id": company_id,
            "company": { "id": company_id, "name": "Acme Corp" },
            "owner_id": rep_id,
            "owner": { "id": rep_id, "name": Role::Rep1.name() },
            "created_at": START,
            "updated_at": START,
        })
    );

    let path = format!("/api/v1/contacts/{id}");
    rep.get(&path)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("full_name", json!("Ada Lovelace"))
        .assert_json_path("company", json!({ "id": company_id, "name": "Acme Corp" }));

    // Update: a new status, no company any more.
    tick(&crm);
    let updated = rep
        .patch(&path)
        .json(&json!({ "status": "customer", "company_id": null, "last_name": "King" }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(updated["status"], "customer");
    assert_eq!(updated["company_id"], Value::Null);
    assert_eq!(updated["company"], Value::Null);
    assert_eq!(updated["full_name"], "Ada King");
    assert_eq!(updated["email"], "ada@acme.com");
    assert_eq!(updated["updated_at"], "2026-10-26T10:01:00Z");

    let page = rep
        .get("/api/v1/contacts")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("meta.total", json!(1))
        .json_value();
    assert_eq!(page["data"][0], updated);

    rep.delete(&path).send().await.assert_status(204);
    rep.get(&path).send().await.assert_error(404, None);
    crm.shutdown().await;
}

#[tokio::test]
async fn contact_validation_errors_are_400() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;

    let body = rep
        .post("/api/v1/contacts")
        .json(&json!({ "email": "ada@acme.com" }))
        .send()
        .await
        .assert_validation_error("first_name")
        .assert_validation_error("last_name")
        .json_value();
    assert_eq!(
        body["errors"]["first_name"],
        json!(["first_name should not be empty"])
    );

    rep.post("/api/v1/contacts")
        .json(&json!({ "first_name": "Ada", "last_name": "Lovelace", "email": "not an email" }))
        .send()
        .await
        .assert_validation_error("email")
        .assert_error(400, "email must be a valid email address");

    rep.post("/api/v1/contacts")
        .json(&json!({ "first_name": "Ada", "last_name": "Lovelace", "status": "vip" }))
        .send()
        .await
        .assert_validation_error("status");

    // An unknown company (the `exists` rule), and a deleted one.
    rep.post("/api/v1/contacts")
        .json(&json!({ "first_name": "Ada", "last_name": "Lovelace", "company_id": uuid::Uuid::now_v7() }))
        .send()
        .await
        .assert_validation_error("company_id")
        .assert_error(400, "company_id does not exist");
    let company = create_company(&rep, json!({ "name": "Gone" })).await;
    rep.delete(&format!("/api/v1/companies/{}", id_of(&company)))
        .send()
        .await
        .assert_status(204);
    rep.post("/api/v1/contacts")
        .json(
            &json!({ "first_name": "Ada", "last_name": "Lovelace", "company_id": id_of(&company) }),
        )
        .send()
        .await
        .assert_validation_error("company_id")
        .assert_error(400, "company_id does not exist");

    // Updates validate the fields sent.
    let contact = create_contact(
        &rep,
        json!({ "first_name": "Ada", "last_name": "Lovelace" }),
    )
    .await;
    let path = format!("/api/v1/contacts/{}", id_of(&contact));
    rep.patch(&path)
        .json(&json!({ "email": "nope", "first_name": " " }))
        .send()
        .await
        .assert_validation_error("email")
        .assert_validation_error("first_name");
    rep.patch(&path)
        .json(&json!({ "company_id": uuid::Uuid::now_v7() }))
        .send()
        .await
        .assert_validation_error("company_id");
    rep.patch(&path)
        .json(&json!({ "status": null }))
        .send()
        .await
        .assert_validation_error("status");
    crm.shutdown().await;
}

/// The edit form sends `company_id` back unchanged: a contact whose company
/// was deleted since stays editable. Moving it to another deleted company
/// is still refused.
#[tokio::test]
async fn contact_of_a_deleted_company_stays_editable() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let company = create_company(&rep, json!({ "name": "Soon Gone Ltd" })).await;
    let company_id = id_of(&company);
    let contact = create_contact(
        &rep,
        json!({ "first_name": "Ada", "last_name": "Byte", "company_id": company_id }),
    )
    .await;
    let other = create_company(&rep, json!({ "name": "Also Gone" })).await;
    for gone in [&company_id, &id_of(&other)] {
        rep.delete(&format!("/api/v1/companies/{gone}"))
            .send()
            .await
            .assert_status(204);
    }
    let path = format!("/api/v1/contacts/{}", id_of(&contact));

    // Exactly what the web form sends, with only the phone changed.
    rep.patch(&path)
        .json(&json!({
            "first_name": "Ada",
            "last_name": "Byte",
            "email": null,
            "phone": "+1 555 0100",
            "title": null,
            "status": "lead",
            "company_id": company_id,
        }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("phone", json!("+1 555 0100"))
        .assert_json_path("company_id", json!(company_id));
    rep.patch(&path)
        .json(&json!({ "company_id": id_of(&other) }))
        .send()
        .await
        .assert_error(400, "company_id does not exist");
    crm.shutdown().await;
}

/// Contact search ignores case for non-ASCII letters too.
#[tokio::test]
async fn contact_search_ignores_case_beyond_ascii() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    create_contact(
        &rep,
        json!({ "first_name": "Élodie", "last_name": "Åberg" }),
    )
    .await;
    create_contact(
        &rep,
        json!({ "first_name": "Ada", "last_name": "Lovelace" }),
    )
    .await;
    for term in ["Élodie", "élodie", "Åberg", "åberg", "ÉLODIE ÅBERG"] {
        assert_eq!(
            list(&rep, "/api/v1/contacts", &[("search", term)], "full_name").await,
            ["Élodie Åberg"],
            "{term}"
        );
    }
    crm.shutdown().await;
}

#[tokio::test]
async fn rep_cannot_update_other_reps_contact_or_assign_contacts() {
    let crm = crm().await;
    let rep1 = crm.login_as(Role::Rep1).await;
    let rep2 = crm.login_as(Role::Rep2).await;
    let contact = create_contact(
        &rep2,
        json!({ "first_name": "Grace", "last_name": "Hopper" }),
    )
    .await;
    let path = format!("/api/v1/contacts/{}", id_of(&contact));

    rep1.patch(&path)
        .json(&json!({ "title": "Admiral" }))
        .send()
        .await
        .assert_error(403, "Forbidden");
    rep1.delete(&path)
        .send()
        .await
        .assert_error(403, "Forbidden");

    // Spec §9.3.
    rep1.post("/api/v1/contacts")
        .json(&json!({ "first_name": "Al", "last_name": "Turing", "owner_id": crm.user_id(Role::Rep2) }))
        .send()
        .await
        .assert_error(403, None);
    rep2.patch(&path)
        .json(&json!({ "owner_id": crm.user_id(Role::Rep1) }))
        .send()
        .await
        .assert_error(403, None);

    let manager = crm.login_as(Role::Manager).await;
    let rep1_id = crm.user_id(Role::Rep1);
    manager
        .patch(&path)
        .json(&json!({ "owner_id": rep1_id, "title": "Rear Admiral" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("owner", json!({ "id": rep1_id, "name": Role::Rep1.name() }));
    rep1.patch(&path)
        .json(&json!({ "title": "Admiral" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("title", json!("Admiral"));
    crm.shutdown().await;
}

#[tokio::test]
async fn search_and_filters() {
    let crm = crm().await;
    let rep1 = crm.login_as(Role::Rep1).await;
    let rep2 = crm.login_as(Role::Rep2).await;
    let acme = id_of(&create_company(&rep1, json!({ "name": "Acme" })).await);
    let globex = id_of(&create_company(&rep1, json!({ "name": "Globex" })).await);
    create_contact(
        &rep1,
        json!({ "first_name": "Ada", "last_name": "Lovelace", "email": "ada@acme.com", "company_id": acme, "status": "customer" }),
    )
    .await;
    create_contact(
        &rep2,
        json!({ "first_name": "Grace", "last_name": "Hopper", "email": "grace@navy.mil", "company_id": globex }),
    )
    .await;
    create_contact(
        &rep1,
        json!({ "first_name": "alan", "last_name": "Turing", "email": "alan@bletchley.uk", "status": "churned" }),
    )
    .await;

    let names =
        async |query: &[(&str, &str)]| list(&rep1, "/api/v1/contacts", query, "full_name").await;

    // By last name, then first name.
    assert_eq!(
        names(&[]).await,
        ["Grace Hopper", "Ada Lovelace", "alan Turing"]
    );
    // Search: first name, last name, email; every word must match.
    assert_eq!(names(&[("search", "ADA")]).await, ["Ada Lovelace"]);
    assert_eq!(names(&[("search", "hop")]).await, ["Grace Hopper"]);
    assert_eq!(names(&[("search", "bletchley")]).await, ["alan Turing"]);
    assert_eq!(names(&[("search", "a")]).await.len(), 3);
    assert_eq!(names(&[("search", "ada love")]).await, ["Ada Lovelace"]);
    assert_eq!(
        names(&[("search", "ada hopper")]).await,
        Vec::<String>::new()
    );
    // Filters.
    assert_eq!(names(&[("company_id", &acme)]).await, ["Ada Lovelace"]);
    assert_eq!(names(&[("status", "churned")]).await, ["alan Turing"]);
    assert_eq!(names(&[("status", "lead")]).await, ["Grace Hopper"]);
    let rep2_id = crm.user_id(Role::Rep2).to_string();
    assert_eq!(names(&[("owner_id", &rep2_id)]).await, ["Grace Hopper"]);
    assert_eq!(
        names(&[
            ("owner_id", &crm.user_id(Role::Rep1).to_string()),
            ("search", "a"),
            ("company_id", &globex)
        ])
        .await,
        Vec::<String>::new()
    );
    rep1.get("/api/v1/contacts")
        .query(&[("status", "vip")])
        .send()
        .await
        .assert_validation_error("status");
    rep1.get("/api/v1/contacts")
        .query(&[("company_id", "acme")])
        .send()
        .await
        .assert_validation_error("company_id");
    crm.shutdown().await;
}

#[tokio::test]
async fn soft_deleted_hidden() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let manager = crm.login_as(Role::Manager).await;
    create_contact(&rep, json!({ "first_name": "Kept", "last_name": "One" })).await;
    let gone = create_contact(&rep, json!({ "first_name": "Gone", "last_name": "Two" })).await;
    let path = format!("/api/v1/contacts/{}", id_of(&gone));
    rep.delete(&path).send().await.assert_status(204);

    let page = manager.get("/api/v1/contacts").send().await.json_value();
    assert_eq!(column(&page, "full_name"), ["Kept One"]);
    manager.get(&path).send().await.assert_error(404, None);
    manager
        .patch(&path)
        .json(&json!({ "title": "Back" }))
        .send()
        .await
        .assert_error(404, None);
    manager.delete(&path).send().await.assert_error(404, None);
    let csv = manager
        .get("/api/v1/contacts/export")
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(!csv.contains("Gone"), "{csv}");
    crm.shutdown().await;
}

#[tokio::test]
async fn export_csv_header_and_rows() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let manager = crm.login_as(Role::Manager).await;
    let company = create_company(&rep, json!({ "name": "Acme, Inc." })).await;
    let ada = create_contact(
        &rep,
        json!({
            "first_name": "Ada",
            "last_name": "Lovelace",
            "email": "ada@acme.com",
            "phone": "+44 20 7946 0000",
            "title": "Chief \"Analytical\" Engineer",
            "status": "customer",
            "company_id": id_of(&company),
        }),
    )
    .await;
    let grace = create_contact(
        &rep,
        json!({ "first_name": "Grace", "last_name": "Hopper", "title": "=HYPERLINK(\"http://evil\")" }),
    )
    .await;

    let response = manager
        .get("/api/v1/contacts/export")
        .send()
        .await
        .assert_status(200);
    assert_eq!(
        response.header("content-type"),
        Some("text/csv; charset=utf-8")
    );
    assert_eq!(
        response.header("content-disposition"),
        Some("attachment; filename=\"contacts.csv\"")
    );
    let rep_name = Role::Rep1.name();
    let expected = format!(
        "id,first_name,last_name,email,phone,title,status,company,owner,created_at\r\n\
         {grace},Grace,Hopper,,,\"'=HYPERLINK(\"\"http://evil\"\")\",lead,,{rep_name},{START}\r\n\
         {ada},Ada,Lovelace,ada@acme.com,+44 20 7946 0000,\"Chief \"\"Analytical\"\" Engineer\",customer,\"Acme, Inc.\",{rep_name},{START}\r\n",
        grace = id_of(&grace),
        ada = id_of(&ada),
    );
    assert_eq!(response.text(), expected);

    // The list's filters apply.
    let csv = manager
        .get("/api/v1/contacts/export")
        .query(&[("status", "customer")])
        .send()
        .await
        .assert_status(200)
        .text();
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines.len(), 2, "{csv}");
    assert!(lines[1].starts_with(&id_of(&ada)), "{csv}");

    // Admins export too.
    crm.login_as(Role::Admin)
        .await
        .get("/api/v1/contacts/export")
        .send()
        .await
        .assert_status(200);
    crm.shutdown().await;
}

#[tokio::test]
async fn exports_are_audited() {
    let crm = crm().await;
    let manager = crm.login_as(Role::Manager).await;
    manager
        .get("/api/v1/contacts/export")
        .query(&[("status", "customer")])
        .send()
        .await
        .assert_status(200);

    let audit = crm.get::<keel::audit::AuditLog>().await;
    audit.flush().await;
    let db = crm.get::<keel::db::Db>().await;
    let entries = <keel::audit::audit_log::Entity as sea_orm::EntityTrait>::find()
        .filter(sea_orm::ColumnTrait::eq(
            &keel::audit::audit_log::Column::Action,
            "contacts.exported",
        ))
        .all(db.conn())
        .await
        .expect("audit entries");
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(entries[0].actor_id, Some(crm.user_id(Role::Manager)));
    crm.shutdown().await;
}

#[tokio::test]
async fn rep_cannot_export() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    create_contact(
        &rep,
        json!({ "first_name": "Ada", "last_name": "Lovelace" }),
    )
    .await;
    rep.get("/api/v1/contacts/export")
        .send()
        .await
        .assert_error(403, "Forbidden");
    crm.client()
        .get("/api/v1/contacts/export")
        .send()
        .await
        .assert_error(401, None);
    crm.shutdown().await;
}
