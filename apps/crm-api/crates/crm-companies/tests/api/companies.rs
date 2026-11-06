//! `/api/v1/companies`: CRUD, ownership, validation,
//! search, filters, sort and soft deletes.

use crm_core::Stage;
use crm_testkit::{CrmClient, Role, TestCrm};
use navree::orm::Model;
use serde_json::{Value, json};

use crate::support::{
    START, column, create_company, create_contact, crm, id_of, insert_deal, list, tick,
};

fn agra() -> Value {
    json!({
        "name": "Agra Corp",
        "domain": "agra.com",
        "industry": "Software",
        "size": "51-200",
        "phone": "+91 98765 43210",
        "website": "https://agra.com",
        "city": "Ludhiana",
        "country": "India",
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
        .json(&agra())
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
            "name": "Agra Corp",
            "domain": "agra.com",
            "industry": "Software",
            "size": "51-200",
            "phone": "+91 98765 43210",
            "website": "https://agra.com",
            "city": "Ludhiana",
            "country": "India",
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
        .json(&json!({ "city": "Amritsar", "domain": null, "size": null }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(updated["city"], "Amritsar");
    assert_eq!(updated["domain"], Value::Null);
    assert_eq!(updated["size"], Value::Null);
    assert_eq!(updated["name"], "Agra Corp");
    assert_eq!(updated["country"], "India");
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
    let company = create_company(&rep, json!({ "name": "Gulati" })).await;
    let id = id_of(&company);
    let other = create_company(&rep, json!({ "name": "Ilaichi" })).await;

    create_contact(
        &rep,
        json!({ "first_name": "Anu", "last_name": "Lakhotia", "company_id": id }),
    )
    .await;
    let gone = create_contact(
        &rep,
        json!({ "first_name": "Baljit", "last_name": "Bains", "company_id": id }),
    )
    .await;
    create_contact(
        &rep,
        json!({ "first_name": "Chetan", "last_name": "Other", "company_id": id_of(&other) }),
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
    let company = create_company(&rep2, agra()).await;
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
        .assert_json_path("name", json!("Agra Corp"));
    crm.client().get(&path).send().await.assert_error(401, None);
    crm.shutdown().await;
}

#[tokio::test]
async fn rep_can_update_own() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let company = create_company(&rep, agra()).await;
    let path = format!("/api/v1/companies/{}", id_of(&company));

    rep.patch(&path)
        .json(&json!({ "name": "Agra Inc", "owner_id": crm.user_id(Role::Rep1) }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("name", json!("Agra Inc"));

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

    // Reps sending another owner_id get a 403, and nothing is
    // created.
    rep1.post("/api/v1/companies")
        .json(&json!({ "name": "Hariyali", "owner_id": rep2_id }))
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
        json!({ "name": "Hariyali", "owner_id": crm.user_id(Role::Rep1) }),
    )
    .await;

    // Managers assign on create and reassign on update.
    let assigned = create_company(
        &manager,
        json!({ "name": "Pind Pantry", "owner_id": rep2_id }),
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
        .json(&json!({ "domain": "agra.com" }))
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
        .json(&json!({ "name": "Agra", "size": "huge" }))
        .send()
        .await
        .assert_validation_error("size")
        .assert_error(
            400,
            "size must be one of the following values: 1-10, 11-50, 51-200, 201-1000, 1000+",
        );
    rep.post("/api/v1/companies")
        .json(&json!({ "name": "Agra", "owner_id": "not-a-uuid" }))
        .send()
        .await
        .assert_validation_error("owner_id");

    let company = create_company(&rep, agra()).await;
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
        .assert_json_path("name", json!("Agra Corp"));
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
            names(&[("filter[industry]", industry)]).await,
            ["Équipe Lyon"],
            "{industry}"
        );
    }
    assert_eq!(
        names(&[("filter[industry]", "ENERGIE")]).await,
        ["Plain Ltd"]
    );
    crm.shutdown().await;
}

#[tokio::test]
async fn search_and_filters() {
    let crm = crm().await;
    let rep1 = crm.login_as(Role::Rep1).await;
    let rep2 = crm.login_as(Role::Rep2).await;
    create_company(
        &rep1,
        json!({ "name": "Ilaichi", "domain": "ilaichi.com", "industry": "Software" }),
    )
    .await;
    tick(&crm);
    create_company(
        &rep2,
        json!({ "name": "Gulati", "domain": "gulati.io", "industry": "Manufacturing" }),
    )
    .await;
    tick(&crm);
    create_company(
        &rep1,
        json!({ "name": "agra", "domain": "agra.com", "industry": "Software" }),
    )
    .await;
    tick(&crm);
    create_company(&rep2, json!({ "name": "100% Juice", "industry": "Food" })).await;

    let names =
        async |query: &[(&str, &str)]| list(&rep1, "/api/v1/companies", query, "name").await;

    // Default order: by name, case-insensitively.
    assert_eq!(
        names(&[]).await,
        ["100% Juice", "agra", "Gulati", "Ilaichi"]
    );
    // Search: name or domain, case-insensitive, `%` and `_` are literal.
    assert_eq!(names(&[("search", "AGR")]).await, ["agra"]);
    assert_eq!(names(&[("search", ".io")]).await, ["Gulati"]);
    assert_eq!(names(&[("search", "chi")]).await, ["Ilaichi"]);
    assert_eq!(names(&[("search", "%")]).await, ["100% Juice"]);
    assert_eq!(names(&[("search", "_")]).await, Vec::<String>::new());
    assert_eq!(names(&[("search", "  ")]).await.len(), 4);
    // Filters.
    let rep2_id = crm.user_id(Role::Rep2).to_string();
    assert_eq!(
        names(&[("filter[owner_id]", rep2_id.as_str())]).await,
        ["100% Juice", "Gulati"]
    );
    assert_eq!(
        names(&[("filter[industry]", "Software")]).await,
        ["agra", "Ilaichi"]
    );
    assert_eq!(
        names(&[("filter[industry]", "software"), ("search", "ilai")]).await,
        ["Ilaichi"]
    );
    // Sort.
    assert_eq!(
        names(&[("sort", "-name")]).await,
        ["Ilaichi", "Gulati", "agra", "100% Juice"]
    );
    assert_eq!(
        names(&[("sort", "created_at")]).await,
        ["Ilaichi", "Gulati", "agra", "100% Juice"]
    );
    assert_eq!(
        names(&[("sort", "-created_at")]).await,
        ["100% Juice", "agra", "Gulati", "Ilaichi"]
    );
    let refused = rep1
        .get("/api/v1/companies")
        .query(&[("sort", "domain")])
        .send()
        .await
        .assert_validation_error("sort")
        .json_value();
    let message = refused["errors"]["sort"][0].as_str().unwrap_or_default();
    assert!(
        message.contains("domain") && message.contains("name, created_at"),
        "{message}"
    );
    rep1.get("/api/v1/companies")
        .query(&[("filter[owner_id]", "nope")])
        .send()
        .await
        .assert_validation_error("filter[owner_id]");
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
        .assert_json_path("data.0.name", json!("Ilaichi"));
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

// The list query of companies, in the list-query grammar.

/// Four companies, created a minute apart in this order: Sutlej Supply
/// (rep 1, Software), Beas Traders (rep 2, Logistics), Ravi Foods (rep 2,
/// Food) and Chenab Textiles (rep 1, no industry).
async fn four_companies(crm: &TestCrm, rep1: &CrmClient, rep2: &CrmClient) {
    create_company(
        rep1,
        json!({ "name": "Sutlej Supply", "domain": "sutlej.in", "industry": "Software" }),
    )
    .await;
    tick(crm);
    create_company(
        rep2,
        json!({ "name": "Beas Traders", "industry": "Logistics" }),
    )
    .await;
    tick(crm);
    create_company(rep2, json!({ "name": "Ravi Foods", "industry": "Food" })).await;
    tick(crm);
    create_company(rep1, json!({ "name": "Chenab Textiles" })).await;
}

#[tokio::test]
async fn owner_and_industry_filters_take_their_operators() {
    let crm = crm().await;
    let rep1 = crm.login_as(Role::Rep1).await;
    let rep2 = crm.login_as(Role::Rep2).await;
    four_companies(&crm, &rep1, &rep2).await;
    let (one, two) = (
        crm.user_id(Role::Rep1).to_string(),
        crm.user_id(Role::Rep2).to_string(),
    );
    let names =
        async |query: &[(&str, &str)]| list(&rep1, "/api/v1/companies", query, "name").await;

    assert_eq!(
        names(&[("filter[owner_id]", &one)]).await,
        ["Chenab Textiles", "Sutlej Supply"]
    );
    assert_eq!(
        names(&[("filter[owner_id][eq]", &two)]).await,
        ["Beas Traders", "Ravi Foods"]
    );
    assert_eq!(
        names(&[("filter[owner_id][in]", &format!("{one},{two}"))]).await,
        [
            "Beas Traders",
            "Chenab Textiles",
            "Ravi Foods",
            "Sutlej Supply"
        ]
    );
    assert_eq!(
        names(&[("filter[owner_id][ne]", &one)]).await,
        ["Beas Traders", "Ravi Foods"]
    );
    assert_eq!(
        names(&[("filter[owner_id][not_in]", &two)]).await,
        ["Chenab Textiles", "Sutlej Supply"]
    );
    assert_eq!(
        names(&[("filter[owner_id][null]", "true")]).await,
        Vec::<String>::new()
    );
    assert_eq!(names(&[("filter[owner_id][null]", "false")]).await.len(), 4);
    // The industry matches ignoring case; a blank one is left out.
    assert_eq!(
        names(&[("filter[industry]", "SOFTWARE")]).await,
        ["Sutlej Supply"]
    );
    assert_eq!(
        names(&[("filter[industry]", "  food ")]).await,
        ["Ravi Foods"]
    );
    assert_eq!(names(&[("filter[industry]", "  ")]).await.len(), 4);
    assert_eq!(
        names(&[("filter[industry]", "Software"), ("filter[owner_id]", &two)]).await,
        Vec::<String>::new()
    );
    // The industry is matched whole, by the service: other operators would
    // be ignored, so they are refused.
    for operator in ["contains", "starts_with", "in", "ne"] {
        let param = format!("filter[industry][{operator}]");
        rep1.get("/api/v1/companies")
            .query(&[(param.as_str(), "soft")])
            .send()
            .await
            .assert_validation_error(&param);
    }
    crm.shutdown().await;
}

#[tokio::test]
async fn sort_takes_the_names_of_the_allow_list_and_several_fields() {
    let crm = crm().await;
    let rep1 = crm.login_as(Role::Rep1).await;
    let rep2 = crm.login_as(Role::Rep2).await;
    four_companies(&crm, &rep1, &rep2).await;
    let two = crm.user_id(Role::Rep2).to_string();
    let names =
        async |query: &[(&str, &str)]| list(&rep1, "/api/v1/companies", query, "name").await;

    assert_eq!(
        names(&[("sort", "name,-created_at")]).await,
        [
            "Beas Traders",
            "Chenab Textiles",
            "Ravi Foods",
            "Sutlej Supply"
        ]
    );
    assert_eq!(
        names(&[("sort", "-created_at")]).await,
        [
            "Chenab Textiles",
            "Ravi Foods",
            "Beas Traders",
            "Sutlej Supply"
        ]
    );
    // A repeated `sort` adds terms; with a filter.
    assert_eq!(
        names(&[
            ("filter[owner_id]", &two),
            ("sort", "created_at"),
            ("sort", "name")
        ])
        .await,
        ["Beas Traders", "Ravi Foods"]
    );
    assert_eq!(
        names(&[("filter[owner_id]", &two), ("sort", "-name")]).await,
        ["Ravi Foods", "Beas Traders"]
    );
    for sort in ["domain", "password", "owner_id", "name;drop", "-"] {
        rep1.get("/api/v1/companies")
            .query(&[("sort", sort)])
            .send()
            .await
            .assert_validation_error("sort");
    }
    crm.shutdown().await;
}

#[tokio::test]
async fn what_the_list_does_not_allow_is_a_400_naming_the_parameter() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    create_company(&rep, agra()).await;

    // (the parameter sent, its value, the parameter the 400 names)
    for (sent, value, param) in [
        ("filter[name]", "Agra", "filter[name]"),
        ("filter[x]", "1", "filter[x]"),
        (
            "filter[password][contains]",
            "x",
            "filter[password][contains]",
        ),
        ("filter[owner_id]", "nope", "filter[owner_id]"),
        ("filter[owner_id][gt]", "a", "filter[owner_id][gt]"),
        ("filter[owner_id][null]", "maybe", "filter[owner_id][null]"),
        ("per_page", "101", "per_page"),
        ("per_page", "0", "per_page"),
        ("page", "x", "page"),
    ] {
        rep.get("/api/v1/companies")
            .query(&[(sent, value)])
            .send()
            .await
            .assert_validation_error(param);
    }
    // Every problem is reported at once.
    let errors = rep
        .get("/api/v1/companies")
        .query(&[("filter[x]", "1"), ("sort", "y"), ("per_page", "1000")])
        .send()
        .await
        .assert_status(400)
        .json_value()["errors"]
        .clone();
    for param in ["filter[x]", "sort", "per_page"] {
        assert!(errors[param].is_array(), "{param}: {errors}");
    }
    crm.shutdown().await;
}

#[tokio::test]
async fn the_page_size_is_at_most_100_and_refused_beyond() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    create_company(&rep, agra()).await;

    rep.get("/api/v1/companies")
        .query(&[("per_page", "100")])
        .send()
        .await
        .assert_status(200)
        .assert_json_path("meta.per_page", json!(100));
    // It used to be clamped to 100; now it is a 400.
    rep.get("/api/v1/companies")
        .query(&[("per_page", "101")])
        .send()
        .await
        .assert_validation_error("per_page");
    crm.shutdown().await;
}

#[tokio::test]
async fn the_old_flat_parameters_are_ignored() {
    let crm = crm().await;
    let rep1 = crm.login_as(Role::Rep1).await;
    let rep2 = crm.login_as(Role::Rep2).await;
    four_companies(&crm, &rep1, &rep2).await;
    let two = crm.user_id(Role::Rep2).to_string();

    // `owner_id=` and `industry=` are not the list query's: no filter, and
    // no error even for a value that is no id.
    for query in [
        [("owner_id", two.as_str()), ("industry", "Software")],
        [("owner_id", "nope"), ("industry", "")],
    ] {
        let page = rep1
            .get("/api/v1/companies")
            .query(&query)
            .send()
            .await
            .assert_status(200)
            .json_value();
        assert_eq!(page["meta"]["total"], 4, "{query:?}");
    }
    // The same name inside `filter[..]` does filter.
    let page = rep1
        .get("/api/v1/companies")
        .query(&[("filter[owner_id]", two.as_str())])
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(page["meta"]["total"], 2);
    crm.shutdown().await;
}

/// Quotes and SQL in a value are text.
#[tokio::test]
async fn hostile_values_are_just_values() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    create_company(
        &rep,
        json!({ "name": "Robert'); DROP TABLE companies;--", "industry": "' OR 1=1" }),
    )
    .await;
    create_company(&rep, agra()).await;
    let names = async |query: &[(&str, &str)]| list(&rep, "/api/v1/companies", query, "name").await;

    // The industry and the search are plain values, found or not.
    assert_eq!(
        names(&[("filter[industry]", "' or 1=1")]).await,
        ["Robert'); DROP TABLE companies;--"]
    );
    assert_eq!(
        names(&[("filter[industry]", "x' OR '1'='1")]).await,
        Vec::<String>::new()
    );
    assert_eq!(
        names(&[("search", "'); DROP TABLE")]).await,
        ["Robert'); DROP TABLE companies;--"]
    );
    assert_eq!(names(&[("search", "' OR 1=1")]).await, Vec::<String>::new());
    // An id or a sort is not text, so it is refused.
    for (param, value) in [
        ("filter[owner_id]", "' OR 1=1"),
        ("filter[owner_id][in]", "1; DROP TABLE companies"),
        ("sort", "name; DROP TABLE companies"),
    ] {
        rep.get("/api/v1/companies")
            .query(&[(param, value)])
            .send()
            .await
            .assert_status(400);
    }
    assert_eq!(names(&[]).await.len(), 2);
    crm.shutdown().await;
}
