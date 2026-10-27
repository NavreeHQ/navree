//! `/api/v1/companies` (spec §7): CRUD, ownership (§9.3), validation,
//! search, filters, sort and soft deletes.

use crm_core::Stage;
use crm_testkit::Role;
use navree::orm::Model;
use serde_json::{Value, json};

use crate::support::{
    START, column, create_company, create_contact, crm, id_of, insert_deal, list, tick,
};

fn acme() -> Value {
    json!({
        "name": "Acme Corp",
        "domain": "acme.com",
        "industry": "Software",
        "size": "51-200",
        "phone": "+1 555-0100",
        "website": "https://acme.com",
        "city": "Berlin",
        "country": "Germany",
    })
}

#[tokio::test]
async fn companies_crud_happy_path() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let rep_id = crm.user_id(Role::Rep1);

    // Create: the creator owns it; every field comes back with `__type`
    // and the owner embed.
    let response = rep
        .post("/api/v1/companies")
        .json(&acme())
        .send()
        .await
        .assert_status(201);
    let created = response.json_value();
    let id = id_of(&created);
    assert_eq!(
        response.header("location"),
        Some(format!("/api/v1/companies/{id}").as_str())
    );
    assert_eq!(
        created,
        json!({
            "__type": "Company",
            "id": id,
            "name": "Acme Corp",
            "domain": "acme.com",
            "industry": "Software",
            "size": "51-200",
            "phone": "+1 555-0100",
            "website": "https://acme.com",
            "city": "Berlin",
            "country": "Germany",
            "owner_id": rep_id,
            "owner": { "id": rep_id, "name": Role::Rep1.name() },
            "created_at": START,
            "updated_at": START,
        })
    );

    // Show: the company plus its counts.
    let shown = rep
        .get(&format!("/api/v1/companies/{id}"))
        .send()
        .await
        .assert_status(200)
        .json_value();
    let mut expected = created.clone();
    expected["contacts_count"] = json!(0);
    expected["open_deals_count"] = json!(0);
    expected["open_deals_amount_cents"] = json!(0);
    assert_eq!(shown, expected);

    // Update: only the fields sent change; `null` clears a field.
    tick(&crm);
    let updated = rep
        .patch(&format!("/api/v1/companies/{id}"))
        .json(&json!({ "city": "Lisbon", "domain": null, "size": null }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(updated["city"], "Lisbon");
    assert_eq!(updated["domain"], Value::Null);
    assert_eq!(updated["size"], Value::Null);
    assert_eq!(updated["name"], "Acme Corp");
    assert_eq!(updated["country"], "Germany");
    assert_eq!(updated["created_at"], START);
    assert_eq!(updated["updated_at"], "2026-10-26T10:01:00Z");
    assert_eq!(updated["__type"], "Company");

    // List.
    let page = rep
        .get("/api/v1/companies")
        .send()
        .await
        .assert_status(200)
        .assert_json_path(
            "meta",
            json!({ "current_page": 1, "per_page": 15, "total": 1, "last_page": 1 }),
        )
        .json_value();
    assert_eq!(page["data"][0], updated);

    // Delete.
    rep.delete(&format!("/api/v1/companies/{id}"))
        .send()
        .await
        .assert_status(204);
    rep.get(&format!("/api/v1/companies/{id}"))
        .send()
        .await
        .assert_error(404, None);
    crm.shutdown().await;
}

#[tokio::test]
async fn company_detail_counts_live_contacts_and_open_deals() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let company = create_company(&rep, json!({ "name": "Globex" })).await;
    let id = id_of(&company);
    let other = create_company(&rep, json!({ "name": "Initech" })).await;

    create_contact(
        &rep,
        json!({ "first_name": "Ada", "last_name": "Lovelace", "company_id": id }),
    )
    .await;
    let gone = create_contact(
        &rep,
        json!({ "first_name": "Bob", "last_name": "Byte", "company_id": id }),
    )
    .await;
    create_contact(
        &rep,
        json!({ "first_name": "Cy", "last_name": "Other", "company_id": id_of(&other) }),
    )
    .await;
    rep.delete(&format!("/api/v1/contacts/{}", id_of(&gone)))
        .send()
        .await
        .assert_status(204);

    let owner = crm.user_id(Role::Rep1);
    insert_deal(&crm, &id, owner, Stage::Lead, 1_000).await;
    insert_deal(&crm, &id, owner, Stage::Proposal, 2_500).await;
    insert_deal(&crm, &id, owner, Stage::Won, 5_000).await;
    insert_deal(&crm, &id, owner, Stage::Lost, 700).await;
    insert_deal(&crm, &id_of(&other), owner, Stage::Lead, 9_999).await;

    rep.get(&format!("/api/v1/companies/{id}"))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("contacts_count", json!(1))
        .assert_json_path("open_deals_count", json!(2))
        .assert_json_path("open_deals_amount_cents", json!(3_500));
    crm.shutdown().await;
}

#[tokio::test]
async fn rep_cannot_update_other_reps_company() {
    let crm = crm().await;
    let rep1 = crm.login_as(Role::Rep1).await;
    let rep2 = crm.login_as(Role::Rep2).await;
    let company = create_company(&rep2, acme()).await;
    let path = format!("/api/v1/companies/{}", id_of(&company));

    rep1.patch(&path)
        .json(&json!({ "name": "Mine now" }))
        .send()
        .await
        .assert_error(403, "Forbidden");
    rep1.delete(&path)
        .send()
        .await
        .assert_error(403, "Forbidden");

    // Reps still read it, unchanged.
    rep1.get(&path)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("name", json!("Acme Corp"));
    crm.client().get(&path).send().await.assert_error(401, None);
    crm.shutdown().await;
}

#[tokio::test]
async fn rep_can_update_own() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let company = create_company(&rep, acme()).await;
    let path = format!("/api/v1/companies/{}", id_of(&company));

    rep.patch(&path)
        .json(&json!({ "name": "Acme Inc", "owner_id": crm.user_id(Role::Rep1) }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("name", json!("Acme Inc"));

    // Managers manage every company, a rep's included.
    let manager = crm.login_as(Role::Manager).await;
    manager
        .patch(&path)
        .json(&json!({ "industry": "Robotics" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("industry", json!("Robotics"))
        .assert_json_path("owner_id", json!(crm.user_id(Role::Rep1)));

    rep.delete(&path).send().await.assert_status(204);
    crm.shutdown().await;
}

#[tokio::test]
async fn only_managers_assign_companies_to_someone_else() {
    let crm = crm().await;
    let rep1 = crm.login_as(Role::Rep1).await;
    let manager = crm.login_as(Role::Manager).await;
    let rep2_id = crm.user_id(Role::Rep2);

    // Spec §9.3: reps sending another owner_id get a 403, and nothing is
    // created.
    rep1.post("/api/v1/companies")
        .json(&json!({ "name": "Hooli", "owner_id": rep2_id }))
        .send()
        .await
        .assert_error(403, None);
    rep1.get("/api/v1/companies")
        .send()
        .await
        .assert_json_path("meta.total", json!(0));

    // Naming themselves is fine.
    create_company(
        &rep1,
        json!({ "name": "Hooli", "owner_id": crm.user_id(Role::Rep1) }),
    )
    .await;

    // Managers assign on create and reassign on update.
    let assigned = create_company(
        &manager,
        json!({ "name": "Pied Piper", "owner_id": rep2_id }),
    )
    .await;
    assert_eq!(assigned["owner_id"], json!(rep2_id));
    assert_eq!(
        assigned["owner"],
        json!({ "id": rep2_id, "name": Role::Rep2.name() })
    );
    let path = format!("/api/v1/companies/{}", id_of(&assigned));

    // The new owner (a rep) cannot hand it on.
    let rep2 = crm.login_as(Role::Rep2).await;
    rep2.patch(&path)
        .json(&json!({ "owner_id": crm.user_id(Role::Rep1) }))
        .send()
        .await
        .assert_error(403, None);

    let rep1_id = crm.user_id(Role::Rep1);
    manager
        .patch(&path)
        .json(&json!({ "owner_id": rep1_id }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("owner", json!({ "id": rep1_id, "name": Role::Rep1.name() }));

    // An owner who does not exist is a validation error.
    manager
        .post("/api/v1/companies")
        .json(&json!({ "name": "Nobody's", "owner_id": uuid::Uuid::now_v7() }))
        .send()
        .await
        .assert_validation_error("owner_id")
        .assert_error(400, "owner_id does not exist");
    crm.shutdown().await;
}

#[tokio::test]
async fn company_validation_errors_are_400() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;

    rep.post("/api/v1/companies")
        .json(&json!({ "domain": "acme.com" }))
        .send()
        .await
        .assert_validation_error("name")
        .assert_error(400, "name should not be empty");
    rep.post("/api/v1/companies")
        .json(&json!({ "name": "   " }))
        .send()
        .await
        .assert_validation_error("name")
        .assert_error(400, "name should not be empty");
    rep.post("/api/v1/companies")
        .json(&json!({ "name": "x".repeat(256) }))
        .send()
        .await
        .assert_validation_error("name");
    rep.post("/api/v1/companies")
        .json(&json!({ "name": "Acme", "size": "huge" }))
        .send()
        .await
        .assert_validation_error("size")
        .assert_error(
            400,
            "size must be one of the following values: 1-10, 11-50, 51-200, 201-1000, 1000+",
        );
    rep.post("/api/v1/companies")
        .json(&json!({ "name": "Acme", "owner_id": "not-a-uuid" }))
        .send()
        .await
        .assert_validation_error("owner_id");

    let company = create_company(&rep, acme()).await;
    let path = format!("/api/v1/companies/{}", id_of(&company));
    rep.patch(&path)
        .json(&json!({ "name": "" }))
        .send()
        .await
        .assert_validation_error("name");
    rep.patch(&path)
        .json(&json!({ "name": null }))
        .send()
        .await
        .assert_validation_error("name");
    rep.get(&path)
        .send()
        .await
        .assert_json_path("name", json!("Acme Corp"));
    crm.shutdown().await;
}

/// Search and the industry filter ignore case for non-ASCII letters too
/// (SQLite's own `LOWER` and `LIKE` fold ASCII only).
#[tokio::test]
async fn search_and_industry_ignore_case_beyond_ascii() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    create_company(
        &rep,
        json!({ "name": "Équipe Lyon", "industry": "Énergie" }),
    )
    .await;
    create_company(
        &rep,
        json!({ "name": "Øresund Ünited", "industry": "Shipping" }),
    )
    .await;
    create_company(&rep, json!({ "name": "Plain Ltd", "industry": "energie" })).await;

    let names = async |query: &[(&str, &str)]| list(&rep, "/api/v1/companies", query, "name").await;
    for term in ["Équipe", "équipe", "ÉQUIPE", "lyon"] {
        assert_eq!(names(&[("search", term)]).await, ["Équipe Lyon"], "{term}");
    }
    for term in ["Øresund", "øresund", "ünited"] {
        assert_eq!(
            names(&[("search", term)]).await,
            ["Øresund Ünited"],
            "{term}"
        );
    }
    for industry in ["Énergie", "énergie", "ÉNERGIE"] {
        assert_eq!(
            names(&[("industry", industry)]).await,
            ["Équipe Lyon"],
            "{industry}"
        );
    }
    assert_eq!(names(&[("industry", "ENERGIE")]).await, ["Plain Ltd"]);
    crm.shutdown().await;
}

#[tokio::test]
async fn search_and_filters() {
    let crm = crm().await;
    let rep1 = crm.login_as(Role::Rep1).await;
    let rep2 = crm.login_as(Role::Rep2).await;
    create_company(
        &rep1,
        json!({ "name": "Initech", "domain": "initech.com", "industry": "Software" }),
    )
    .await;
    tick(&crm);
    create_company(
        &rep2,
        json!({ "name": "Globex", "domain": "globex.io", "industry": "Manufacturing" }),
    )
    .await;
    tick(&crm);
    create_company(
        &rep1,
        json!({ "name": "acme", "domain": "acme.com", "industry": "Software" }),
    )
    .await;
    tick(&crm);
    create_company(&rep2, json!({ "name": "100% Juice", "industry": "Food" })).await;

    let names =
        async |query: &[(&str, &str)]| list(&rep1, "/api/v1/companies", query, "name").await;

    // Default order: by name, case-insensitively.
    assert_eq!(
        names(&[]).await,
        ["100% Juice", "acme", "Globex", "Initech"]
    );
    // Search: name or domain, case-insensitive, `%` and `_` are literal.
    assert_eq!(names(&[("search", "ACM")]).await, ["acme"]);
    assert_eq!(names(&[("search", ".io")]).await, ["Globex"]);
    assert_eq!(names(&[("search", "te")]).await, ["Initech"]);
    assert_eq!(names(&[("search", "%")]).await, ["100% Juice"]);
    assert_eq!(names(&[("search", "_")]).await, Vec::<String>::new());
    assert_eq!(names(&[("search", "  ")]).await.len(), 4);
    // Filters.
    let rep2_id = crm.user_id(Role::Rep2).to_string();
    assert_eq!(
        names(&[("owner_id", rep2_id.as_str())]).await,
        ["100% Juice", "Globex"]
    );
    assert_eq!(
        names(&[("industry", "Software")]).await,
        ["acme", "Initech"]
    );
    assert_eq!(
        names(&[("industry", "software"), ("search", "init")]).await,
        ["Initech"]
    );
    // Sort.
    assert_eq!(
        names(&[("sort", "-name")]).await,
        ["Initech", "Globex", "acme", "100% Juice"]
    );
    assert_eq!(
        names(&[("sort", "created_at")]).await,
        ["Initech", "Globex", "acme", "100% Juice"]
    );
    assert_eq!(
        names(&[("sort", "-created_at")]).await,
        ["100% Juice", "acme", "Globex", "Initech"]
    );
    rep1.get("/api/v1/companies")
        .query(&[("sort", "domain")])
        .send()
        .await
        .assert_validation_error("sort")
        .assert_error(
            400,
            "sort must be one of the following values: name, -name, created_at, -created_at",
        );
    rep1.get("/api/v1/companies")
        .query(&[("owner_id", "nope")])
        .send()
        .await
        .assert_validation_error("owner_id");
    // Pages.
    rep1.get("/api/v1/companies")
        .query(&[("per_page", "3"), ("page", "2")])
        .send()
        .await
        .assert_status(200)
        .assert_json_path(
            "meta",
            json!({ "current_page": 2, "per_page": 3, "total": 4, "last_page": 2 }),
        )
        .assert_json_path("data.0.name", json!("Initech"));
    crm.shutdown().await;
}

#[tokio::test]
async fn soft_deleted_hidden() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let admin = crm.login_as(Role::Admin).await;
    let kept = create_company(&rep, json!({ "name": "Kept" })).await;
    let gone = create_company(&rep, json!({ "name": "Gone" })).await;
    let path = format!("/api/v1/companies/{}", id_of(&gone));

    tick(&crm);
    rep.delete(&path).send().await.assert_status(204);

    let page = admin.get("/api/v1/companies").send().await.json_value();
    assert_eq!(column(&page, "name"), ["Kept"]);
    let page = admin
        .get("/api/v1/companies")
        .query(&[("search", "Gone")])
        .send()
        .await
        .json_value();
    assert_eq!(page["meta"]["total"], 0);
    admin.get(&path).send().await.assert_error(404, None);
    admin
        .patch(&path)
        .json(&json!({ "name": "Back" }))
        .send()
        .await
        .assert_error(404, None);
    admin.delete(&path).send().await.assert_error(404, None);

    // The row stays, marked deleted.
    let db = crm.get::<navree::db::Db>().await;
    let row = crm_core::Company::query()
        .with_trashed()
        .find(&*db, id_of(&gone).parse::<uuid::Uuid>().expect("uuid"))
        .await
        .expect("query")
        .expect("the row is kept");
    assert!(row.trashed());
    assert_eq!(
        row.deleted_at
            .map(|at| at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
        Some("2026-10-26T10:01:00Z".to_owned())
    );
    admin
        .get(&format!("/api/v1/companies/{}", id_of(&kept)))
        .send()
        .await
        .assert_status(200);
    crm.shutdown().await;
}
