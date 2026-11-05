//! `/api/v1/contacts`: CRUD, embeds, ownership,
//! validation, search, filters, soft deletes and the CSV export.

use crm_testkit::{CrmClient, Role};
use navree::audit::{AuditFilter, AuditLog};
use navree::db::PageQuery;
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

    // A rep may not hand a new contact to someone else.
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
    assert_eq!(
        names(&[("filter[company_id]", &acme)]).await,
        ["Ada Lovelace"]
    );
    assert_eq!(
        names(&[("filter[status]", "churned")]).await,
        ["alan Turing"]
    );
    assert_eq!(names(&[("filter[status]", "lead")]).await, ["Grace Hopper"]);
    let rep2_id = crm.user_id(Role::Rep2).to_string();
    assert_eq!(
        names(&[("filter[owner_id]", &rep2_id)]).await,
        ["Grace Hopper"]
    );
    assert_eq!(
        names(&[
            ("filter[owner_id]", &crm.user_id(Role::Rep1).to_string()),
            ("search", "a"),
            ("filter[company_id]", &globex)
        ])
        .await,
        Vec::<String>::new()
    );
    rep1.get("/api/v1/contacts")
        .query(&[("filter[status]", "vip")])
        .send()
        .await
        .assert_validation_error("filter[status]");
    rep1.get("/api/v1/contacts")
        .query(&[("filter[company_id]", "acme")])
        .send()
        .await
        .assert_validation_error("filter[company_id]");
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
        .query(&[("filter[status]", "customer")])
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
        .query(&[("filter[status]", "customer")])
        .send()
        .await
        .assert_status(200);

    let audit = crm.get::<AuditLog>().await;
    audit.flush().await;
    let entries = audit
        .list(
            &AuditFilter {
                action: Some("contacts.exported".to_owned()),
                ..AuditFilter::default()
            },
            PageQuery::new(1, 100),
        )
        .await
        .expect("audit entries")
        .data;
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

// MOD-013: the list query of contacts, in the list-query grammar.

/// Four contacts, by last name: Grace Hopper (Globex, lead, rep 2),
/// Katherine Johnson (Acme, prospect, rep 2), Ada Lovelace (Acme, customer,
/// rep 1) and alan Turing (no company, churned, rep 1). Returns the ids of
/// Acme and Globex.
async fn four_contacts(rep1: &CrmClient, rep2: &CrmClient) -> (String, String) {
    let acme = id_of(&create_company(rep1, json!({ "name": "Acme" })).await);
    let globex = id_of(&create_company(rep1, json!({ "name": "Globex" })).await);
    for (client, body) in [
        (
            rep1,
            json!({ "first_name": "Ada", "last_name": "Lovelace", "company_id": acme, "status": "customer" }),
        ),
        (
            rep2,
            json!({ "first_name": "Grace", "last_name": "Hopper", "company_id": globex }),
        ),
        (
            rep1,
            json!({ "first_name": "alan", "last_name": "Turing", "status": "churned" }),
        ),
        (
            rep2,
            json!({ "first_name": "Katherine", "last_name": "Johnson", "company_id": acme, "status": "prospect" }),
        ),
    ] {
        create_contact(client, body).await;
    }
    (acme, globex)
}

#[tokio::test]
async fn contact_filters_take_their_operators() {
    let crm = crm().await;
    let rep1 = crm.login_as(Role::Rep1).await;
    let rep2 = crm.login_as(Role::Rep2).await;
    let (acme, globex) = four_contacts(&rep1, &rep2).await;
    let (one, two) = (
        crm.user_id(Role::Rep1).to_string(),
        crm.user_id(Role::Rep2).to_string(),
    );
    let names =
        async |query: &[(&str, &str)]| list(&rep1, "/api/v1/contacts", query, "full_name").await;

    assert_eq!(
        names(&[("filter[status][in]", "lead,customer")]).await,
        ["Grace Hopper", "Ada Lovelace"]
    );
    assert_eq!(
        names(&[("filter[status][ne]", "lead")]).await,
        ["Katherine Johnson", "Ada Lovelace", "alan Turing"]
    );
    assert_eq!(
        names(&[("filter[status][not_in]", "lead,churned")]).await,
        ["Katherine Johnson", "Ada Lovelace"]
    );
    assert_eq!(
        names(&[("filter[company_id][null]", "true")]).await,
        ["alan Turing"]
    );
    assert_eq!(
        names(&[("filter[company_id][null]", "false")]).await,
        ["Grace Hopper", "Katherine Johnson", "Ada Lovelace"]
    );
    assert_eq!(
        names(&[("filter[company_id][in]", &format!("{acme},{globex}"))]).await,
        ["Grace Hopper", "Katherine Johnson", "Ada Lovelace"]
    );
    assert_eq!(
        names(&[("filter[company_id]", &acme), ("filter[owner_id]", &two)]).await,
        ["Katherine Johnson"]
    );
    assert_eq!(
        names(&[("filter[owner_id][ne]", &one)]).await,
        ["Grace Hopper", "Katherine Johnson"]
    );
    // A search narrows the filtered list; every word must match.
    assert_eq!(
        names(&[("search", "a"), ("filter[status][in]", "customer,churned")]).await,
        ["Ada Lovelace", "alan Turing"]
    );
    assert_eq!(
        names(&[
            ("search", "katherine johnson"),
            ("filter[company_id]", &acme)
        ])
        .await,
        ["Katherine Johnson"]
    );
    // The list has no `sort`: contacts always come by last name.
    rep1.get("/api/v1/contacts")
        .query(&[("sort", "last_name")])
        .send()
        .await
        .assert_validation_error("sort");
    crm.shutdown().await;
}

#[tokio::test]
async fn the_contact_export_reads_the_same_list_query() {
    let crm = crm().await;
    let rep1 = crm.login_as(Role::Rep1).await;
    let rep2 = crm.login_as(Role::Rep2).await;
    let manager = crm.login_as(Role::Manager).await;
    let (acme, _) = four_contacts(&rep1, &rep2).await;

    let export = async |query: &[(&str, &str)]| {
        let csv = manager
            .get("/api/v1/contacts/export")
            .query(query)
            .send()
            .await
            .assert_status(200)
            .text();
        // The rows' last names, in list order (the header is the first line).
        csv.lines()
            .skip(1)
            .map(|line| line.split(',').nth(2).unwrap_or_default().to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        export(&[]).await,
        ["Hopper", "Johnson", "Lovelace", "Turing"]
    );
    assert_eq!(
        export(&[("filter[company_id]", &acme)]).await,
        ["Johnson", "Lovelace"]
    );
    assert_eq!(
        export(&[("filter[status][in]", "lead,churned")]).await,
        ["Hopper", "Turing"]
    );
    assert_eq!(
        export(&[("search", "ada"), ("filter[status]", "customer")]).await,
        ["Lovelace"]
    );
    // The export is not paged: a page size does not cut it short.
    assert_eq!(export(&[("per_page", "1")]).await.len(), 4);
    // The old flat names filter nothing.
    assert_eq!(export(&[("status", "customer")]).await.len(), 4);
    // What the list refuses, the export refuses.
    for (param, value) in [
        ("filter[email]", "ada@acme.com"),
        ("filter[status]", "vip"),
        ("filter[company_id]", "acme"),
        ("per_page", "101"),
    ] {
        manager
            .get("/api/v1/contacts/export")
            .query(&[(param, value)])
            .send()
            .await
            .assert_validation_error(param);
    }
    crm.shutdown().await;
}

#[tokio::test]
async fn what_the_contact_list_does_not_allow_is_a_400_naming_the_parameter() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    create_contact(
        &rep,
        json!({ "first_name": "Ada", "last_name": "Lovelace" }),
    )
    .await;

    // (the parameter sent, its value, the parameter the 400 names)
    for (sent, value, param) in [
        ("filter[email]", "ada@acme.com", "filter[email]"),
        (
            "filter[last_name][contains]",
            "love",
            "filter[last_name][contains]",
        ),
        ("filter[status]", "vip", "filter[status]"),
        ("filter[status][gt]", "lead", "filter[status][gt]"),
        ("filter[company_id]", "acme", "filter[company_id]"),
        ("filter[owner_id][in]", "nope", "filter[owner_id][in]"),
        ("sort", "password", "sort"),
        ("per_page", "101", "per_page"),
        ("page", "0", "page"),
    ] {
        rep.get("/api/v1/contacts")
            .query(&[(sent, value)])
            .send()
            .await
            .assert_validation_error(param);
    }
    rep.get("/api/v1/contacts")
        .query(&[("per_page", "100")])
        .send()
        .await
        .assert_status(200)
        .assert_json_path("meta.per_page", json!(100));
    crm.shutdown().await;
}

#[tokio::test]
async fn the_old_flat_contact_parameters_are_ignored() {
    let crm = crm().await;
    let rep1 = crm.login_as(Role::Rep1).await;
    let rep2 = crm.login_as(Role::Rep2).await;
    let (acme, _) = four_contacts(&rep1, &rep2).await;

    // No filter, and no error even for values that are no status or id.
    for query in [
        [("status", "customer"), ("company_id", acme.as_str())],
        [("status", "vip"), ("company_id", "acme")],
    ] {
        let page = rep1
            .get("/api/v1/contacts")
            .query(&query)
            .send()
            .await
            .assert_status(200)
            .json_value();
        assert_eq!(page["meta"]["total"], 4, "{query:?}");
    }
    crm.shutdown().await;
}

/// Quotes and SQL in a value are text.
#[tokio::test]
async fn hostile_contact_values_are_just_values() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    create_contact(
        &rep,
        json!({ "first_name": "Robert'); DROP TABLE contacts;--", "last_name": "Tables" }),
    )
    .await;
    create_contact(
        &rep,
        json!({ "first_name": "Ada", "last_name": "Lovelace" }),
    )
    .await;
    let names =
        async |query: &[(&str, &str)]| list(&rep, "/api/v1/contacts", query, "full_name").await;

    assert_eq!(
        names(&[("search", "'); DROP")]).await,
        ["Robert'); DROP TABLE contacts;-- Tables"]
    );
    assert_eq!(names(&[("search", "' OR 1=1")]).await, Vec::<String>::new());
    for (param, value) in [
        ("filter[status]", "' OR 1=1"),
        ("filter[status][in]", "lead') OR ('1'='1"),
        ("filter[company_id]", "' OR 1=1"),
        ("filter[owner_id][in]", "1; DROP TABLE contacts"),
    ] {
        rep.get("/api/v1/contacts")
            .query(&[(param, value)])
            .send()
            .await
            .assert_status(400);
    }
    assert_eq!(names(&[]).await.len(), 2);
    crm.shutdown().await;
}
