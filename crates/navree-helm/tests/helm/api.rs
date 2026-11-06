//! The JSON API: routing, authorization, CRUD, search, sorting, filters.

use chrono::NaiveDate;
use navree_audit::AuditModule;
use navree_db::Db;
use navree_helm::FormData;
use navree_orm::{Assignment, Model};
use navree_testing::principal;
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::support::*;

#[tokio::test]
async fn reps_read_every_deal_but_edit_only_their_own() {
    // The example of the docs' Testing section.
    let app = app().await;
    let (_priya, rep) = user(&app, "rep").await;
    let (tom, _) = user(&app, "rep").await;
    let toms_deal = deal(
        &app,
        DealSeed {
            title: "Gulati renewal",
            owner: Some(tom),
            ..DealSeed::default()
        },
    )
    .await;

    rep.get("/admin/api/resources/deals")
        .query(&[("search", "Gulati")])
        .send()
        .await
        .assert_status(200)
        .assert_json_path("meta.total", 1.into());

    rep.patch(&format!("/admin/api/resources/deals/{}", toms_deal.id))
        .json(&json!({ "title": "Mine now" }))
        .send()
        .await
        .assert_error(403, None);
    assert_eq!(
        reload(&app, toms_deal.id).await.unwrap().title,
        "Gulati renewal"
    );
}

#[tokio::test]
async fn guests_get_401_and_users_without_helm_view_get_403() {
    let app = app().await;
    app.client()
        .get("/admin/api/resources")
        .send()
        .await
        .assert_error(401, None);
    let stranger = app.acting_as(principal(Uuid::now_v7()));
    stranger
        .get("/admin/api/resources")
        .send()
        .await
        .assert_error(403, None);
    stranger
        .get("/admin/api/resources/deals")
        .send()
        .await
        .assert_error(403, None);
    // A role with access to deals but not to the panel stays out.
    let id = Uuid::now_v7();
    give_role(&app, id, "outsider", &["deals.*"]).await;
    app.acting_as(principal(id))
        .get("/admin/api/resources/deals")
        .send()
        .await
        .assert_error(403, None);
}

#[tokio::test]
async fn the_metadata_lists_the_resources_the_user_may_read() {
    let app = app().await;
    let (_, rep) = user(&app, "rep").await;
    let body = rep
        .get("/admin/api/resources")
        .send()
        .await
        .assert_status(200)
        .json_value();
    let slugs: Vec<&str> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["slug"].as_str().unwrap())
        .collect();
    // Same group, so by ORDER (companies first), then by label.
    assert_eq!(slugs, ["companies", "deals"]);
    let deals = &body["data"][1];
    assert_eq!(deals["label"], "Deals");
    assert_eq!(deals["singular"], "Deal");
    assert_eq!(deals["group"], "Sales");
    assert_eq!(deals["icon"], "handshake");
    assert_eq!(deals["title"], "title");
    assert_eq!(deals["per_page"], 25);
    assert_eq!(deals["default_sort"], "-created_at");
    assert_eq!(deals["can"]["create"], true);
    let field = |name: &str| {
        deals["fields"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["name"] == name)
            .cloned()
            .unwrap()
    };
    assert_eq!(field("amount_cents")["kind"], "money");
    assert_eq!(field("amount_cents")["label"], "Amount");
    assert_eq!(field("company_id")["related"], "companies");
    assert_eq!(field("created_at")["readonly"], true);
    assert_eq!(field("probability")["on_forms"], false);
    assert_eq!(
        field("stage")["options"][2],
        json!({ "value": "won", "label": "Won" })
    );
    let actions: Vec<&str> = deals["actions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["name"].as_str().unwrap())
        .collect();
    assert_eq!(actions, ["mark-won", "mark-lost", "export", "fail"]);
    assert_eq!(deals["actions"][0]["ability"], "move");
    assert_eq!(
        deals["actions"][0]["confirm"],
        "Mark the selected deals as won?"
    );
    assert_eq!(deals["actions"][2]["queued"], true);
    assert_eq!(deals["metrics"][0]["slug"], "open-pipeline");
    assert_eq!(deals["metrics"][1]["ranged"], true);
    assert_eq!(deals["filters"][1]["related"], "companies");

    let (_, limited) = user(&app, "limited").await;
    let body = limited
        .get("/admin/api/resources")
        .send()
        .await
        .json_value();
    assert_eq!(body["data"].as_array().unwrap().len(), 1);
    assert_eq!(body["data"][0]["can"]["create"], false);
}

#[tokio::test]
async fn lists_search_sort_and_paginate() {
    let app = app().await;
    let (_, rep) = user(&app, "rep").await;
    for (title, amount) in [
        ("Gulati renewal", 500),
        ("Indrani upgrade", 300),
        ("GULATI expansion", 900),
        ("Hariyali pilot", 100),
    ] {
        deal(
            &app,
            DealSeed {
                title,
                amount_cents: amount,
                ..DealSeed::default()
            },
        )
        .await;
    }
    let body = rep
        .get("/admin/api/resources/deals")
        .query(&[("search", "gulati"), ("sort", "-amount_cents")])
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(titles(&body), ["GULATI expansion", "Gulati renewal"]);

    let body = rep
        .get("/admin/api/resources/deals")
        .query(&[("sort", "amount_cents"), ("per_page", "2"), ("page", "2")])
        .send()
        .await
        .json_value();
    assert_eq!(titles(&body), ["Gulati renewal", "GULATI expansion"]);
    assert_eq!(
        body["meta"],
        json!({ "current_page": 2, "per_page": 2, "total": 4, "last_page": 2 })
    );

    rep.get("/admin/api/resources/deals")
        .query(&[("sort", "notes")])
        .send()
        .await
        .assert_validation_error("sort");
    rep.get("/admin/api/resources/deals")
        .query(&[("per_page", "500")])
        .send()
        .await
        .assert_validation_error("per_page");
    rep.get("/admin/api/resources/deals")
        .query(&[("page", "0")])
        .send()
        .await
        .assert_validation_error("page");

    // Helm sorts by one field: the first term of the sort.
    let body = rep
        .get("/admin/api/resources/deals")
        .query(&[("sort", "-amount_cents,title")])
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(
        titles(&body),
        [
            "GULATI expansion",
            "Gulati renewal",
            "Indrani upgrade",
            "Hariyali pilot"
        ]
    );
    // The search is `search`; `q` is not the list query's and searches nothing.
    let body = rep
        .get("/admin/api/resources/deals")
        .query(&[("q", "gulati")])
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(body["meta"]["total"], 4, "{body}");
    // Helm lists page by number, so a cursor is refused by name.
    rep.get("/admin/api/resources/deals")
        .query(&[("cursor", "abc")])
        .send()
        .await
        .assert_validation_error("cursor");
    rep.get("/admin/api/resources/deals")
        .query(&[("limit", "2")])
        .send()
        .await
        .assert_validation_error("limit");
}

#[tokio::test]
async fn filters_narrow_the_list() {
    let app = app().await;
    let (_, rep) = user(&app, "rep").await;
    let gulati = company(&app, "Gulati", Some("Springfield")).await;
    let indrani = company(&app, "Indrani", None).await;
    let day = |d| NaiveDate::from_ymd_opt(2026, 10, d);
    deal(
        &app,
        DealSeed {
            title: "A",
            stage: "lead",
            company: Some(gulati.id),
            expected_close_date: day(3),
            amount_cents: 100,
            ..DealSeed::default()
        },
    )
    .await;
    deal(
        &app,
        DealSeed {
            title: "B",
            stage: "won",
            company: Some(gulati.id),
            expected_close_date: day(20),
            amount_cents: 5000,
            closed: true,
            ..DealSeed::default()
        },
    )
    .await;
    deal(
        &app,
        DealSeed {
            title: "C",
            stage: "lost",
            company: Some(indrani.id),
            expected_close_date: day(27),
            amount_cents: 900,
            closed: true,
            ..DealSeed::default()
        },
    )
    .await;
    deal(
        &app,
        DealSeed {
            title: "D",
            stage: "qualified",
            ..DealSeed::default()
        },
    )
    .await;

    let list = |query: Vec<(&'static str, String)>| {
        let rep = rep.clone();
        async move {
            let mut query = query;
            query.push(("sort", "title".to_owned()));
            let response = rep
                .get("/admin/api/resources/deals")
                .query(&query)
                .send()
                .await;
            titles(&response.assert_status(200).json_value())
        }
    };
    // `filter[stage]=won` is equal, `filter[stage][in]=a,b` is one of a list, and
    // the key of a list may repeat.
    assert_eq!(list(vec![("filter[stage]", "won".into())]).await, ["B"]);
    assert_eq!(list(vec![("filter[stage][eq]", "won".into())]).await, ["B"]);
    assert_eq!(
        list(vec![
            ("filter[stage][in]", "lead".into()),
            ("filter[stage][in]", "won".into())
        ])
        .await,
        ["A", "B"]
    );
    assert_eq!(
        list(vec![("filter[stage][in]", "lead,lost".into())]).await,
        ["A", "C"]
    );
    assert_eq!(
        list(vec![("filter[company_id]", gulati.id.to_string())]).await,
        ["A", "B"]
    );
    assert_eq!(
        list(vec![(
            "filter[company_id][in]",
            format!("{},{}", gulati.id, indrani.id)
        )])
        .await,
        ["A", "B", "C"]
    );
    assert_eq!(list(vec![("filter[open]", "1".into())]).await, ["A", "D"]);
    assert_eq!(
        list(vec![("filter[open]", "0".into())]).await,
        ["A", "B", "C", "D"]
    );
    assert_eq!(
        list(vec![(
            "filter[expected_close_date]",
            "2026-10-10..2026-10-27".into()
        )])
        .await,
        ["B", "C"]
    );
    assert_eq!(
        list(vec![("filter[expected_close_date]", "..2026-10-20".into())]).await,
        ["A", "B"]
    );
    // `week` is the week of the app's clock (Monday 26 October 2026).
    assert_eq!(
        list(vec![("filter[expected_close_date]", "week".into())]).await,
        ["C"]
    );
    assert_eq!(
        list(vec![("filter[min_amount]", "900".into())]).await,
        ["B", "C", "D"]
    );
    assert_eq!(
        list(vec![
            ("filter[stage]", "won".into()),
            ("filter[company_id]", gulati.id.to_string())
        ])
        .await,
        ["B"]
    );
    // `filters[..]` is not the list query's: it is left alone and nothing is
    // filtered.
    assert_eq!(
        list(vec![("filters[stage]", "won".into())]).await,
        ["A", "B", "C", "D"]
    );
    assert_eq!(
        list(vec![("filters[stage]", "lead,lost".into())]).await,
        ["A", "B", "C", "D"]
    );

    rep.get("/admin/api/resources/deals")
        .query(&[("filter[stage]", "archived")])
        .send()
        .await
        .assert_validation_error("filter[stage]");
    rep.get("/admin/api/resources/deals")
        .query(&[("filter[stage][in]", "won,archived")])
        .send()
        .await
        .assert_validation_error("filter[stage]");
    // A comma makes a list only with `in`: this is one value, and no stage.
    rep.get("/admin/api/resources/deals")
        .query(&[("filter[stage]", "lead,lost")])
        .send()
        .await
        .assert_validation_error("filter[stage]");
    rep.get("/admin/api/resources/deals")
        .query(&[("filter[color]", "red")])
        .send()
        .await
        .assert_validation_error("filter[color]");
    rep.get("/admin/api/resources/deals")
        .query(&[("filter[expected_close_date]", "soon")])
        .send()
        .await
        .assert_validation_error("filter[expected_close_date]");
    // A Helm filter is equal or one of a list. Any other operator is refused by
    // name: some the list query has and one it does not.
    for param in [
        "filter[stage][regex]",
        "filter[stage][gte]",
        "filter[stage][ne]",
        "filter[stage][not_in]",
        "filter[stage][contains]",
        "filter[stage][null]",
    ] {
        rep.get("/admin/api/resources/deals")
            .query(&[(param, "won")])
            .send()
            .await
            .assert_validation_error(param);
    }
    // A second equal filter on one field is a problem the API names.
    rep.get("/admin/api/resources/deals")
        .query(&[("filter[stage]", "lead"), ("filter[stage]", "won")])
        .send()
        .await
        .assert_validation_error("filter[stage]");
}

#[tokio::test]
async fn the_query_and_the_access_rules_hide_rows() {
    let app = app().await;
    let (_, rep) = user(&app, "rep").await;
    let visible = company(&app, "Visible", None).await;
    let gone = company(&app, "Gone", None).await;
    let db = app.get::<Db>().await;
    Company::query()
        .where_key(gone.id)
        .update(&*db, [Assignment::value("deleted_at", now())])
        .await
        .unwrap();

    let body = rep
        .get("/admin/api/resources/companies")
        .send()
        .await
        .json_value();
    assert_eq!(titles(&body), ["Visible"]);
    rep.get(&format!("/admin/api/resources/companies/{}", gone.id))
        .send()
        .await
        .assert_error(404, None);
    rep.get(&format!("/admin/api/resources/companies/{}", visible.id))
        .send()
        .await
        .assert_status(200);

    // A role that reads only its own deals.
    let mine = Uuid::now_v7();
    give_role(&app, mine, "own-only", &["helm.view"]).await;
    deal(
        &app,
        DealSeed {
            title: "Mine",
            owner: Some(mine),
            ..DealSeed::default()
        },
    )
    .await;
    let other = deal(
        &app,
        DealSeed {
            title: "Other",
            owner: Some(Uuid::now_v7()),
            ..DealSeed::default()
        },
    )
    .await;
    let own_only = app.acting_as(principal(mine));
    let body = own_only
        .get("/admin/api/resources/deals")
        .send()
        .await
        .json_value();
    assert_eq!(titles(&body), ["Mine"]);
    own_only
        .get(&format!("/admin/api/resources/deals/{}", other.id))
        .send()
        .await
        .assert_error(404, None);
    // A role with no rule on deals at all may not open the list.
    let none = Uuid::now_v7();
    give_role(&app, none, "nothing", &["helm.view"]).await;
    app.acting_as(principal(none))
        .get("/admin/api/resources/deals")
        .send()
        .await
        .assert_error(403, None);
}

#[tokio::test]
async fn a_record_shows_its_values_display_and_permissions() {
    let app = app().await;
    let (rep_id, rep) = user(&app, "rep").await;
    let gulati = company(&app, "Gulati", None).await;
    let deal = deal(
        &app,
        DealSeed {
            title: "Gulati renewal",
            amount_cents: 1_250_000,
            stage: "qualified",
            owner: Some(rep_id),
            company: Some(gulati.id),
            expected_close_date: NaiveDate::from_ymd_opt(2026, 11, 30),
            ..DealSeed::default()
        },
    )
    .await;
    let body = rep
        .get(&format!("/admin/api/resources/deals/{}", deal.id))
        .send()
        .await
        .assert_status(200)
        .json_value();
    let data = &body["data"];
    assert_eq!(data["id"], deal.id.to_string());
    assert_eq!(data["title"], "Gulati renewal");
    assert_eq!(data["values"]["amount_cents"], 1_250_000);
    assert_eq!(data["values"]["company_id"], gulati.id.to_string());
    assert_eq!(data["values"]["probability"], "25%");
    assert_eq!(data["display"]["amount_cents"], "$12,500.00");
    assert_eq!(data["display"]["stage"], "Qualified");
    assert_eq!(data["display"]["company_id"], "Gulati");
    assert_eq!(data["display"]["expected_close_date"], "30 Nov 2026");
    assert_eq!(data["display"]["notes"], "Secret notes");
    assert_eq!(data["display"]["priority"], "No");
    assert_eq!(data["can"], json!({ "update": true, "delete": false }));
    // Read-only fields and the field the rules keep (owner_id).
    let readonly: Vec<&str> = data["readonly"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(readonly, ["owner_id", "closed_at", "created_at"]);

    rep.get(&format!("/admin/api/resources/deals/{}", Uuid::now_v7()))
        .send()
        .await
        .assert_error(404, None);
    rep.get("/admin/api/resources/deals/not-a-uuid")
        .send()
        .await
        .assert_error(404, None);
    rep.get("/admin/api/resources/invoices")
        .send()
        .await
        .assert_error(404, None);
}

#[tokio::test]
async fn field_rules_hide_values_the_user_may_not_read() {
    let app = app().await;
    let (_, reader) = user(&app, "no-secrets").await;
    let deal = deal(&app, DealSeed::default()).await;
    let body = reader
        .get(&format!("/admin/api/resources/deals/{}", deal.id))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert!(body["data"]["values"].get("notes").is_none(), "{body}");
    assert!(body["data"]["display"].get("notes").is_none(), "{body}");
    assert_eq!(body["data"]["values"]["title"], "Deal");
}

#[tokio::test]
async fn creating_validates_and_saves_through_the_resource() {
    let app = app().await;
    let (rep_id, rep) = user(&app, "rep").await;
    let gulati = company(&app, "Gulati", None).await;

    rep.post("/admin/api/resources/deals")
        .json(&json!({ "amount_cents": -5 }))
        .send()
        .await
        .assert_status(400)
        .assert_json_path("errors.title", json!(["Title should not be empty"]))
        .assert_json_path(
            "errors.amount_cents",
            json!(["Amount must not be less than 0"]),
        );
    rep.post("/admin/api/resources/deals")
        .json(&json!({ "title": "X", "amount_cents": 1, "created_at": "2020-01-01T00:00:00Z", "secret": 1 }))
        .send()
        .await
        .assert_status(400)
        .assert_json_path("errors.created_at", json!(["property created_at should not exist"]))
        .assert_json_path("errors.secret", json!(["property secret should not exist"]));
    rep.post("/admin/api/resources/deals")
        .json(&json!({ "title": "X", "amount_cents": 1, "company_id": Uuid::now_v7() }))
        .send()
        .await
        .assert_json_path("errors.company_id", json!(["Company does not exist"]));
    rep.post("/admin/api/resources/deals")
        .json(&json!({ "title": "X", "amount_cents": 1, "stage": "archived" }))
        .send()
        .await
        .assert_json_path(
            "errors.stage",
            json!(["Stage must be one of the following values: lead, qualified, won, lost"]),
        );
    rep.post("/admin/api/resources/deals")
        .json(&json!(["not", "an", "object"]))
        .send()
        .await
        .assert_error(400, "The body must be a JSON object.");

    let body = rep
        .post("/admin/api/resources/deals")
        .json(&json!({
            "title": "Gulati renewal",
            "amount_cents": "125050",
            "company_id": gulati.id,
            "expected_close_date": "2026-12-01",
            "priority": "on",
        }))
        .send()
        .await
        .assert_status(201)
        .json_value();
    let id: Uuid = body["data"]["id"].as_str().unwrap().parse().unwrap();
    let saved = reload(&app, id).await.unwrap();
    assert_eq!(saved.title, "Gulati renewal");
    assert_eq!(saved.amount_cents, 125_050);
    // Defaults fill what the form did not send; the resource sets the owner.
    assert_eq!(saved.currency, "USD");
    assert_eq!(saved.stage, "lead");
    assert_eq!(saved.owner_id, Some(rep_id));
    assert!(saved.priority);
    assert_eq!(
        saved.expected_close_date,
        NaiveDate::from_ymd_opt(2026, 12, 1)
    );
    assert_eq!(body["data"]["display"]["amount_cents"], "$1,250.50");

    // Without `create` on deals, nothing is created.
    let (_, limited) = user(&app, "limited").await;
    limited
        .post("/admin/api/resources/deals")
        .json(&json!({ "title": "Nope", "amount_cents": 1 }))
        .send()
        .await
        .assert_error(403, None);
}

#[tokio::test]
async fn the_default_create_writes_the_form_to_the_columns() {
    let app = app().await;
    let (_, manager) = user(&app, "manager").await;
    let body = manager
        .post("/admin/api/resources/companies")
        .json(&json!({ "name": "Indrani", "city": "", "website": "https://indrani.example" }))
        .send()
        .await
        .assert_status(201)
        .json_value();
    let id: Uuid = body["data"]["id"].as_str().unwrap().parse().unwrap();
    let saved = reload_company(&app, id).await.unwrap();
    assert_eq!(saved.name, "Indrani");
    // A blank nullable column is stored as NULL; created_at is set.
    assert_eq!(saved.city, None);
    assert_eq!(saved.website.as_deref(), Some("https://indrani.example"));
    assert_eq!(saved.created_at, now());
    manager
        .post("/admin/api/resources/companies")
        .json(&json!({ "name": "Bad", "website": "ftp://nope" }))
        .send()
        .await
        .assert_json_path("errors.website", json!(["Website must be a URL address"]));
}

#[tokio::test]
async fn updating_checks_the_record_and_its_fields() {
    let app = app().await;
    let (rep_id, rep) = user(&app, "rep").await;
    let own = deal(
        &app,
        DealSeed {
            title: "Own",
            owner: Some(rep_id),
            ..DealSeed::default()
        },
    )
    .await;

    let body = rep
        .patch(&format!("/admin/api/resources/deals/{}", own.id))
        .json(&json!({ "title": "Own, renamed", "stage": "qualified" }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(body["data"]["title"], "Own, renamed");
    let saved = reload(&app, own.id).await.unwrap();
    assert_eq!(saved.stage, "qualified");
    assert_eq!(saved.updated_at, now());

    // A field the rules keep (owner_id is also read-only here, but the
    // rule answers first).
    rep.patch(&format!("/admin/api/resources/deals/{}", own.id))
        .json(&json!({ "owner_id": Uuid::now_v7() }))
        .send()
        .await
        .assert_error(403, "You may not change these fields: owner_id.");
    rep.patch(&format!("/admin/api/resources/deals/{}", own.id))
        .json(&json!({ "title": "" }))
        .send()
        .await
        .assert_validation_error("title");
    rep.patch(&format!("/admin/api/resources/deals/{}", own.id))
        .json(&json!({ "created_at": "2020-01-01T00:00:00Z" }))
        .send()
        .await
        .assert_validation_error("created_at");

    // `limited` may change only the title of their own deals.
    let (limited_id, limited) = user(&app, "limited").await;
    let theirs = deal(
        &app,
        DealSeed {
            title: "Theirs",
            owner: Some(limited_id),
            ..DealSeed::default()
        },
    )
    .await;
    limited
        .patch(&format!("/admin/api/resources/deals/{}", theirs.id))
        .json(&json!({ "amount_cents": 1 }))
        .send()
        .await
        .assert_error(403, "You may not change these fields: amount_cents.");
    limited
        .patch(&format!("/admin/api/resources/deals/{}", theirs.id))
        .json(&json!({ "title": "Renamed" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path(
            "data.readonly",
            json!([
                "amount_cents",
                "currency",
                "stage",
                "company_id",
                "owner_id",
                "expected_close_date",
                "notes",
                "priority",
                "lost_reason",
                "closed_at",
                "created_at"
            ]),
        );
    limited
        .patch(&format!("/admin/api/resources/deals/{}", Uuid::now_v7()))
        .json(&json!({ "title": "Ghost" }))
        .send()
        .await
        .assert_error(404, None);
}

#[tokio::test]
async fn deleting_needs_the_delete_ability() {
    let app = app().await;
    let (rep_id, rep) = user(&app, "rep").await;
    let (_, manager) = user(&app, "manager").await;
    let own = deal(
        &app,
        DealSeed {
            owner: Some(rep_id),
            ..DealSeed::default()
        },
    )
    .await;
    rep.delete(&format!("/admin/api/resources/deals/{}", own.id))
        .send()
        .await
        .assert_error(403, None);
    assert!(reload(&app, own.id).await.is_some());
    manager
        .delete(&format!("/admin/api/resources/deals/{}", own.id))
        .send()
        .await
        .assert_status(204);
    assert!(reload(&app, own.id).await.is_none());
    manager
        .delete(&format!("/admin/api/resources/deals/{}", own.id))
        .send()
        .await
        .assert_error(404, None);
}

#[tokio::test]
async fn pickers_list_readable_records_by_title() {
    let app = app().await;
    let (_, rep) = user(&app, "rep").await;
    company(&app, "Indrani", Some("Austin")).await;
    company(&app, "Gulati", Some("Springfield")).await;
    company(&app, "Hariyali", Some("Palo Alto")).await;
    let body = rep
        .get("/admin/api/resources/companies/options")
        .send()
        .await
        .assert_status(200)
        .json_value();
    let names: Vec<&str> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["title"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Gulati", "Hariyali", "Indrani"]);
    let body = rep
        .get("/admin/api/resources/companies/options")
        .query(&[("search", "spring")])
        .send()
        .await
        .json_value();
    assert_eq!(body["data"].as_array().unwrap().len(), 1);
    assert_eq!(body["data"][0]["title"], "Gulati");

    let (_, limited) = user(&app, "limited").await;
    limited
        .get("/admin/api/resources/companies/options")
        .send()
        .await
        .assert_error(403, None);
}

#[tokio::test]
async fn changes_are_audited_with_the_user() {
    let app = builder()
        .config("audit.routes", false)
        .module(AuditModule::from_config())
        .build()
        .await;
    let (rep_id, rep) = user(&app, "rep").await;
    let own = deal(
        &app,
        DealSeed {
            owner: Some(rep_id),
            ..DealSeed::default()
        },
    )
    .await;
    rep.patch(&format!("/admin/api/resources/deals/{}", own.id))
        .json(&json!({ "title": "Audited" }))
        .send()
        .await
        .assert_status(200);
    let log = app.get::<navree_audit::AuditLog>().await;
    log.flush().await;
    let rows = navree_audit::AuditLog::list(
        &*app.get::<navree_audit::AuditLog>().await,
        &navree_audit::AuditFilter::default(),
        navree_db::PageQuery {
            page: 1,
            per_page: 100,
        },
    )
    .await
    .unwrap()
    .data;
    let updated = rows
        .iter()
        .find(|r| r.action == "deal.updated")
        .expect("deal.updated");
    assert_eq!(updated.actor_id, Some(rep_id));
    assert_eq!(
        updated.subject_id.as_deref(),
        Some(own.id.to_string().as_str())
    );
    assert_eq!(updated.new_values.as_ref().unwrap()["title"], "Audited");
}

/// The default writes record their change events with the record's
/// serialized form: a hidden field (`Company::api_token`) is stored, but
/// never reaches an observer or the audit log, on create, update or
/// delete.
#[tokio::test]
async fn hidden_fields_stay_out_of_change_events_and_audit_entries() {
    let app = builder()
        .config("audit.routes", false)
        .module(AuditModule::from_config())
        .build()
        .await;
    let (manager_id, manager) = user(&app, "manager").await;
    let body = manager
        .post("/admin/api/resources/companies")
        .json(&json!({ "name": "Indrani", "api_token": "tok-created-secret" }))
        .send()
        .await
        .assert_status(201)
        .json_value();
    let id: Uuid = body["data"]["id"].as_str().unwrap().parse().unwrap();
    let saved = reload_company(&app, id).await.unwrap();
    assert_eq!(saved.api_token.as_deref(), Some("tok-created-secret"));
    assert_eq!(saved.created_at, now());

    manager
        .patch(&format!("/admin/api/resources/companies/{id}"))
        .json(&json!({ "api_token": "tok-updated-secret", "city": "Austin" }))
        .send()
        .await
        .assert_status(200);
    let saved = reload_company(&app, id).await.unwrap();
    assert_eq!(saved.api_token.as_deref(), Some("tok-updated-secret"));
    assert_eq!(saved.city.as_deref(), Some("Austin"));
    // Only the hidden field changes: nothing is recorded for it.
    manager
        .patch(&format!("/admin/api/resources/companies/{id}"))
        .json(&json!({ "api_token": "tok-rotated-secret" }))
        .send()
        .await
        .assert_status(200);
    manager
        .delete(&format!("/admin/api/resources/companies/{id}"))
        .send()
        .await
        .assert_status(204);
    assert!(reload_company(&app, id).await.is_none());

    let log = app.get::<navree_audit::AuditLog>().await;
    log.flush().await;
    let rows: Vec<_> = navree_audit::AuditLog::list(
        &log,
        &navree_audit::AuditFilter::default(),
        navree_db::PageQuery {
            page: 1,
            per_page: 100,
        },
    )
    .await
    .unwrap()
    .data
    .into_iter()
    .filter(|r| r.subject_id.as_deref() == Some(id.to_string().as_str()))
    .collect();
    let mut actions: Vec<&str> = rows.iter().map(|r| r.action.as_str()).collect();
    actions.sort_unstable();
    assert_eq!(
        actions,
        ["company.created", "company.deleted", "company.updated"],
        "{rows:?}"
    );
    assert!(rows.iter().all(|r| r.actor_id == Some(manager_id)));
    let updated = rows.iter().find(|r| r.action == "company.updated").unwrap();
    assert_eq!(
        updated.new_values.as_ref().unwrap(),
        &json!({ "city": "Austin" })
    );
    let text = serde_json::to_string(&rows).unwrap();
    assert!(text.contains("Indrani"), "{text}");
    assert!(
        !text.contains("tok-"),
        "a secret reached the audit log: {text}"
    );
    assert!(!text.contains("api_token"), "{text}");
}

#[tokio::test]
async fn form_data_parses_into_a_validated_dto() {
    let mut values = Map::new();
    values.insert("title".into(), json!(""));
    values.insert("amount_cents".into(), json!(-1));
    let error = FormData::new(values).parse::<NewDeal>().await.unwrap_err();
    assert_eq!(error.status.as_u16(), 400);
    let errors = error.errors.unwrap();
    assert!(errors.contains_key("title"), "{errors:?}");
    assert!(errors.contains_key("amount_cents"), "{errors:?}");

    let mut values = Map::new();
    values.insert("title".into(), json!("Fine"));
    values.insert("amount_cents".into(), json!(5));
    values.insert("company_id".into(), Value::Null);
    let form = FormData::new(values);
    assert!(form.contains("title") && form.get("amount_cents") == Some(&json!(5)));
    let dto: NewDeal = form.parse().await.unwrap();
    assert_eq!(dto.title, "Fine");
    assert_eq!(dto.company_id, None);
}
