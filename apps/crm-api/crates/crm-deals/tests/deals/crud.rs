//! Creating, reading, listing, updating and deleting deals.

use std::time::Duration;

use crm_testkit::Role;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::{
    board, column_positions, column_titles, company, contact, create_deal, crm, delete_company,
    id_of, new_deal,
};

#[tokio::test]
async fn create_returns_the_deal_with_its_embeds() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let agra = company(&crm, "Agra", rep.id()).await;
    let ada = contact(&crm, "Anu", "Lakhotia", Some(agra), rep.id()).await;

    let response = rep
        .post("/api/v1/deals")
        .json(&json!({
            "title": "Agra renewal",
            "company_id": agra,
            "contact_id": ada,
            "amount_cents": 125_000,
            "expected_close_date": "2026-12-01",
        }))
        .send()
        .await
        .assert_status(201);
    let body = response.json_value();
    let id = id_of(&body);
    assert_eq!(
        response.header("location"),
        Some(format!("/api/v1/deals/{id}").as_str())
    );
    let expected = json!({
        "__type": "Deal",
        "id": id,
        "title": "Agra renewal",
        "amount_cents": 125_000,
        "currency": "INR",
        "stage": "lead",
        "probability": 10,
        "position": 0,
        "expected_close_date": "2026-12-01",
        "closed_at": null,
        "lost_reason": null,
        "company_id": agra,
        "company": { "id": agra, "name": "Agra" },
        "contact_id": ada,
        "contact": { "id": ada, "full_name": "Anu Lakhotia" },
        "owner_id": rep.id(),
        "owner": { "id": rep.id(), "name": Role::Rep1.name() },
        "created_at": "2026-10-26T10:00:00Z",
        "updated_at": "2026-10-26T10:00:00Z",
    });
    assert_eq!(body, expected);

    // Anyone signed in reads it back the same way.
    let manager = crm.login_as(Role::Manager).await;
    let shown = manager
        .get(&format!("/api/v1/deals/{id}"))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(shown, expected);
    crm.shutdown().await;
}

#[tokio::test]
async fn create_validates_the_input() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let manager = crm.login_as(Role::Manager).await;
    let agra = company(&crm, "Agra", rep.id()).await;
    let gone = company(&crm, "Gone Ltd", rep.id()).await;
    delete_company(&crm, gone).await;

    // Required fields.
    rep.post("/api/v1/deals")
        .json(&json!({}))
        .send()
        .await
        .assert_validation_error("title")
        .assert_validation_error("company_id")
        .assert_validation_error("amount_cents");

    // Field rules.
    let response = rep
        .post("/api/v1/deals")
        .json(&json!({
            "title": "  ",
            "company_id": agra,
            "amount_cents": -1,
            "currency": "dollars",
            "expected_close_date": "12/01/2026",
        }))
        .send()
        .await
        .assert_status(400);
    let errors = response.json_value()["errors"].clone();
    assert_eq!(errors["title"], json!(["title should not be empty"]));
    assert_eq!(
        errors["amount_cents"],
        json!(["amount_cents must not be less than 0"])
    );
    assert_eq!(
        errors["currency"],
        json!(["currency must be a 3-letter currency code"])
    );
    assert_eq!(
        errors["expected_close_date"],
        json!(["expected_close_date must be a valid date in the format YYYY-MM-DD"])
    );
    let mut input = new_deal("Renewal", agra, 100);
    input["stage"] = json!("closed");
    rep.post("/api/v1/deals")
        .json(&input)
        .send()
        .await
        .assert_validation_error("stage");

    // Records that must exist (deleted companies and contacts do not count).
    for (field, changes) in [
        ("company_id", json!({ "company_id": Uuid::now_v7() })),
        ("company_id", json!({ "company_id": gone })),
        ("contact_id", json!({ "contact_id": Uuid::now_v7() })),
    ] {
        let mut input = new_deal("Renewal", agra, 100);
        for (key, value) in changes.as_object().expect("an object") {
            input[key] = value.clone();
        }
        let response = rep
            .post("/api/v1/deals")
            .json(&input)
            .send()
            .await
            .assert_validation_error(field);
        assert_eq!(
            response.json_value()["errors"][field],
            json!([format!("{field} does not exist")])
        );
    }
    let mut input = new_deal("Renewal", agra, 100);
    input["owner_id"] = json!(Uuid::now_v7());
    manager
        .post("/api/v1/deals")
        .json(&input)
        .send()
        .await
        .assert_validation_error("owner_id");

    // Nothing was created.
    let list = rep.get("/api/v1/deals").send().await.assert_status(200);
    assert_eq!(list.json_value()["meta"]["total"], 0);
    crm.shutdown().await;
}

/// Amounts are capped (10^15 cents): sums over many deals stay within
/// 64 bits and the web app's JavaScript numbers stay exact.
#[tokio::test]
async fn amounts_are_capped() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let agra = company(&crm, "Agra", rep.id()).await;

    for amount in [json!(i64::MAX), json!(1_000_000_000_000_001_i64)] {
        let mut input = new_deal("Huge", agra, 0);
        input["amount_cents"] = amount;
        let response = rep
            .post("/api/v1/deals")
            .json(&input)
            .send()
            .await
            .assert_validation_error("amount_cents");
        assert_eq!(
            response.json_value()["errors"]["amount_cents"],
            json!(["amount_cents must not be greater than 1000000000000000"])
        );
    }
    let deal = create_deal(&rep, new_deal("Biggest", agra, 1_000_000_000_000_000)).await;
    rep.patch(&format!("/api/v1/deals/{}", id_of(&deal)))
        .json(&json!({ "amount_cents": i64::MAX }))
        .send()
        .await
        .assert_validation_error("amount_cents");
    crm.shutdown().await;
}

/// Deal search ignores case for non-ASCII letters too.
#[tokio::test]
async fn deal_search_ignores_case_beyond_ascii() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let agra = company(&crm, "Agra", rep.id()).await;
    create_deal(&rep, new_deal("Ölwechsel Rahmenvertrag", agra, 100)).await;
    create_deal(&rep, new_deal("Agra renewal", agra, 100)).await;
    for term in ["Ölwechsel", "ölwechsel", "ÖLWECHSEL", "rahmenvertrag"] {
        let page = rep
            .get("/api/v1/deals")
            .query(&json!({ "search": term }))
            .send()
            .await
            .assert_status(200)
            .json_value();
        assert_eq!(titles(&page), ["Ölwechsel Rahmenvertrag"], "{term}");
    }
    crm.shutdown().await;
}

#[tokio::test]
async fn new_deals_join_the_end_of_their_stage() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let agra = company(&crm, "Agra", rep.id()).await;

    let first = create_deal(&rep, new_deal("First", agra, 100)).await;
    let second = create_deal(&rep, new_deal("Second", agra, 200)).await;
    assert_eq!(first["position"], 0);
    assert_eq!(second["position"], 1);

    // A deal created as won is closed now, with the stage's probability.
    let mut input = new_deal("Signed", agra, 300);
    input["stage"] = json!("won");
    input["currency"] = json!("eur");
    let won = create_deal(&rep, input).await;
    assert_eq!(won["stage"], "won");
    assert_eq!(won["probability"], 100);
    assert_eq!(won["position"], 0);
    assert_eq!(won["currency"], "EUR");
    assert_eq!(won["closed_at"], "2026-10-26T10:00:00Z");

    // A deal created as lost needs a reason.
    let mut input = new_deal("Gone cold", agra, 400);
    input["stage"] = json!("lost");
    rep.post("/api/v1/deals")
        .json(&input)
        .send()
        .await
        .assert_validation_error("lost_reason");
    input["lost_reason"] = json!("No budget");
    let lost = create_deal(&rep, input).await;
    assert_eq!(lost["lost_reason"], "No budget");
    assert_eq!(lost["probability"], 0);
    assert_eq!(lost["closed_at"], "2026-10-26T10:00:00Z");
    crm.shutdown().await;
}

#[tokio::test]
async fn only_managers_create_deals_for_someone_else() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let manager = crm.login_as(Role::Manager).await;
    let agra = company(&crm, "Agra", rep.id()).await;
    let rep2 = crm.user_id(Role::Rep2);

    let mut input = new_deal("Handover", agra, 100);
    input["owner_id"] = json!(rep2);
    rep.post("/api/v1/deals")
        .json(&input)
        .send()
        .await
        .assert_error(403, None);

    // Naming themselves is fine.
    let mut own = new_deal("Mine", agra, 100);
    own["owner_id"] = json!(rep.id());
    let mine = create_deal(&rep, own).await;
    assert_eq!(mine["owner_id"], json!(rep.id()));

    let assigned = create_deal(&manager, input).await;
    assert_eq!(assigned["owner_id"], json!(rep2));
    assert_eq!(assigned["owner"]["name"], Role::Rep2.name());
    crm.shutdown().await;
}

#[tokio::test]
async fn unknown_deals_are_not_found() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    for path in [
        format!("/api/v1/deals/{}", Uuid::now_v7()),
        "/api/v1/deals/not-a-uuid".to_owned(),
    ] {
        rep.get(&path).send().await.assert_error(404, None);
    }
    crm.client()
        .get("/api/v1/deals")
        .send()
        .await
        .assert_error(401, None);
    crm.shutdown().await;
}

/// The titles of a page of deals, in order.
fn titles(page: &Value) -> Vec<String> {
    page["data"]
        .as_array()
        .expect("data")
        .iter()
        .map(|deal| deal["title"].as_str().expect("a title").to_owned())
        .collect()
}

#[tokio::test]
async fn list_filters_searches_and_paginates() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let manager = crm.login_as(Role::Manager).await;
    let agra = company(&crm, "Agra", rep.id()).await;
    let gulati = company(&crm, "Gulati", rep.id()).await;
    let rep2 = crm.user_id(Role::Rep2);

    create_deal(&rep, new_deal("Agra renewal", agra, 100)).await;
    let mut input = new_deal("Gulati expansion", gulati, 200);
    input["stage"] = json!("proposal");
    create_deal(&rep, input).await;
    let mut input = new_deal("Agra upsell", agra, 300);
    input["owner_id"] = json!(rep2);
    create_deal(&manager, input).await;

    let list = async |query: Value| {
        rep.get("/api/v1/deals")
            .query(&query)
            .send()
            .await
            .assert_status(200)
            .json_value()
    };

    // Newest first, every deal a full resource.
    let all = list(json!({})).await;
    assert_eq!(
        titles(&all),
        ["Agra upsell", "Gulati expansion", "Agra renewal"]
    );
    assert_eq!(all["data"][0]["__type"], "Deal");
    assert_eq!(all["data"][0]["owner"]["name"], Role::Rep2.name());
    assert_eq!(
        all["meta"],
        json!({ "current_page": 1, "per_page": 15, "total": 3, "last_page": 1 })
    );

    let proposals = list(json!({ "filter[stage]": "proposal" })).await;
    assert_eq!(titles(&proposals), ["Gulati expansion"]);
    let owned = list(json!({ "filter[owner_id]": rep2 })).await;
    assert_eq!(titles(&owned), ["Agra upsell"]);
    let at_agra = list(json!({ "filter[company_id]": agra })).await;
    assert_eq!(titles(&at_agra), ["Agra upsell", "Agra renewal"]);
    let found = list(json!({ "search": "AGRA" })).await;
    assert_eq!(titles(&found), ["Agra upsell", "Agra renewal"]);
    let narrowed = list(json!({ "search": "expan", "filter[company_id]": gulati })).await;
    assert_eq!(titles(&narrowed), ["Gulati expansion"]);

    let page = list(json!({ "per_page": 2, "page": 2 })).await;
    assert_eq!(titles(&page), ["Agra renewal"]);
    assert_eq!(
        page["meta"],
        json!({ "current_page": 2, "per_page": 2, "total": 3, "last_page": 2 })
    );

    rep.get("/api/v1/deals")
        .query(&json!({ "filter[stage]": "closed" }))
        .send()
        .await
        .assert_validation_error("filter[stage]");
    crm.shutdown().await;
}

#[tokio::test]
async fn update_changes_only_the_fields_sent() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let agra = company(&crm, "Agra", rep.id()).await;
    let gulati = company(&crm, "Gulati", rep.id()).await;
    let ada = contact(&crm, "Anu", "Lakhotia", Some(agra), rep.id()).await;
    let mut input = new_deal("Agra renewal", agra, 100);
    input["contact_id"] = json!(ada);
    input["expected_close_date"] = json!("2026-12-01");
    let id = id_of(&create_deal(&rep, input).await);

    crm.clock().advance(Duration::from_secs(5 * 60));
    let updated = rep
        .patch(&format!("/api/v1/deals/{id}"))
        .json(&json!({
            "amount_cents": 250_000,
            "company_id": gulati,
            "contact_id": null,
            "expected_close_date": null,
        }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(updated["title"], "Agra renewal");
    assert_eq!(updated["amount_cents"], 250_000);
    assert_eq!(
        updated["company"],
        json!({ "id": gulati, "name": "Gulati" })
    );
    assert_eq!(updated["contact_id"], json!(null));
    assert_eq!(updated["contact"], json!(null));
    assert_eq!(updated["expected_close_date"], json!(null));
    assert_eq!(updated["created_at"], "2026-10-26T10:00:00Z");
    assert_eq!(updated["updated_at"], "2026-10-26T10:05:00Z");

    // Validation applies to the fields sent.
    rep.patch(&format!("/api/v1/deals/{id}"))
        .json(&json!({ "title": "", "amount_cents": -5 }))
        .send()
        .await
        .assert_validation_error("title")
        .assert_validation_error("amount_cents");
    rep.patch(&format!("/api/v1/deals/{id}"))
        .json(&json!({ "contact_id": Uuid::now_v7() }))
        .send()
        .await
        .assert_validation_error("contact_id");
    let shown = rep
        .get(&format!("/api/v1/deals/{id}"))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(shown, updated);
    crm.shutdown().await;
}

#[tokio::test]
async fn reps_change_only_their_own_deals() {
    let crm = crm().await;
    let rep1 = crm.login_as(Role::Rep1).await;
    let rep2 = crm.login_as(Role::Rep2).await;
    let manager = crm.login_as(Role::Manager).await;
    let agra = company(&crm, "Agra", rep1.id()).await;
    let id = id_of(&create_deal(&rep1, new_deal("Agra renewal", agra, 100)).await);
    let path = format!("/api/v1/deals/{id}");

    rep2.patch(&path)
        .json(&json!({ "title": "Mine now" }))
        .send()
        .await
        .assert_error(403, None);
    rep2.delete(&path).send().await.assert_error(403, None);
    // Their own deal, but not theirs to hand over.
    rep1.patch(&path)
        .json(&json!({ "owner_id": rep2.id() }))
        .send()
        .await
        .assert_error(403, None);

    manager
        .patch(&path)
        .json(&json!({ "title": "Agra renewal 2027" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("title", json!("Agra renewal 2027"));
    crm.shutdown().await;
}

#[tokio::test]
async fn delete_removes_the_deal_and_closes_the_gap() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let agra = company(&crm, "Agra", rep.id()).await;
    let first = id_of(&create_deal(&rep, new_deal("First", agra, 100)).await);
    create_deal(&rep, new_deal("Second", agra, 100)).await;
    create_deal(&rep, new_deal("Third", agra, 100)).await;

    rep.delete(&format!("/api/v1/deals/{first}"))
        .send()
        .await
        .assert_status(204);
    rep.get(&format!("/api/v1/deals/{first}"))
        .send()
        .await
        .assert_error(404, None);
    let board = board(&rep).await;
    assert_eq!(column_titles(&board, "lead"), ["Second", "Third"]);
    assert_eq!(column_positions(&board, "lead"), [0, 1]);
    crm.shutdown().await;
}

#[tokio::test]
async fn resending_unchanged_references_is_fine() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let agra = company(&crm, "Agra", rep.id()).await;
    let ada = contact(&crm, "Anu", "Lakhotia", Some(agra), rep.id()).await;
    let mut input = new_deal("Agra renewal", agra, 100);
    input["contact_id"] = json!(ada);
    let id = id_of(&create_deal(&rep, input).await);
    // The company goes away after the deal was made; an edit form sends
    // every field back.
    delete_company(&crm, agra).await;

    let updated = rep
        .patch(&format!("/api/v1/deals/{id}"))
        .json(&json!({
            "title": "Agra renewal (final)",
            "company_id": agra,
            "contact_id": ada,
            "owner_id": rep.id(),
        }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(updated["title"], "Agra renewal (final)");
    assert_eq!(updated["company"], json!({ "id": agra, "name": "Agra" }));
    crm.shutdown().await;
}
