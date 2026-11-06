//! `GET /api/v1/deals` in the list-query grammar: `filter[..]` with its
//! operators, `search`, `sort` and `per_page`, and what is refused.

use crm_testkit::{CrmClient, Role};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::{company, create_deal, crm, new_deal};

/// The titles of a page of deals, in order.
fn titles(page: &Value) -> Vec<String> {
    page["data"]
        .as_array()
        .unwrap_or_else(|| panic!("a page: {page}"))
        .iter()
        .map(|deal| deal["title"].as_str().expect("a title").to_owned())
        .collect()
}

/// `GET /api/v1/deals` with `query`, answering 200.
async fn list(client: &CrmClient, query: &[(&str, &str)]) -> Value {
    client
        .get("/api/v1/deals")
        .query(query)
        .send()
        .await
        .assert_status(200)
        .json_value()
}

/// Four deals, none of them with the same value for every field:
///
/// | Title | Stage | Amount | Closes | Company | Owner |
/// |---|---|---|---|---|---|
/// | Sutlej renewal | lead | 300 | 2026-12-01 | Sutlej | rep 1 |
/// | Beas expansion | proposal | 100 | 2026-11-15 | Beas | rep 1 |
/// | Ravi upsell | won | 200 | 2026-12-20 | Sutlej | rep 2 |
/// | Chenab pilot | lead | 400 | 2026-11-30 | Beas | rep 2 |
struct Fixture {
    sutlej: Uuid,
    beas: Uuid,
    rep2: Uuid,
}

async fn four_deals(crm: &crm_testkit::TestCrm, rep: &CrmClient, manager: &CrmClient) -> Fixture {
    let sutlej = company(crm, "Sutlej", rep.id()).await;
    let beas = company(crm, "Beas", rep.id()).await;
    let rep2 = crm.user_id(Role::Rep2);
    for (client, title, company, amount, stage, closes, owner) in [
        (
            rep,
            "Sutlej renewal",
            sutlej,
            300,
            "lead",
            "2026-12-01",
            None,
        ),
        (
            rep,
            "Beas expansion",
            beas,
            100,
            "proposal",
            "2026-11-15",
            None,
        ),
        (
            manager,
            "Ravi upsell",
            sutlej,
            200,
            "won",
            "2026-12-20",
            Some(rep2),
        ),
        (
            manager,
            "Chenab pilot",
            beas,
            400,
            "lead",
            "2026-11-30",
            Some(rep2),
        ),
    ] {
        let mut input = new_deal(title, company, amount);
        input["stage"] = json!(stage);
        input["expected_close_date"] = json!(closes);
        if let Some(owner) = owner {
            input["owner_id"] = json!(owner);
        }
        create_deal(client, input).await;
    }
    Fixture { sutlej, beas, rep2 }
}

#[tokio::test]
async fn the_stage_filter_takes_equality_and_lists() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let manager = crm.login_as(Role::Manager).await;
    four_deals(&crm, &rep, &manager).await;

    let stages = async |query: &[(&str, &str)]| {
        let page = list(&rep, query).await;
        let mut names = titles(&page);
        names.sort_unstable();
        names
    };
    assert_eq!(
        stages(&[("filter[stage]", "lead")]).await,
        ["Chenab pilot", "Sutlej renewal"]
    );
    assert_eq!(
        stages(&[("filter[stage][eq]", "won")]).await,
        ["Ravi upsell"]
    );
    assert_eq!(
        stages(&[("filter[stage][in]", "lead,won")]).await,
        ["Chenab pilot", "Ravi upsell", "Sutlej renewal"]
    );
    // The key may repeat.
    assert_eq!(
        stages(&[
            ("filter[stage][in]", "won"),
            ("filter[stage][in]", "proposal")
        ])
        .await,
        ["Beas expansion", "Ravi upsell"]
    );
    assert_eq!(
        stages(&[("filter[stage][ne]", "lead")]).await,
        ["Beas expansion", "Ravi upsell"]
    );
    assert_eq!(
        stages(&[("filter[stage][not_in]", "lead,won")]).await,
        ["Beas expansion"]
    );
    assert_eq!(stages(&[("filter[stage][null]", "true")]).await.len(), 0);
    assert_eq!(stages(&[("filter[stage][null]", "false")]).await.len(), 4);
    crm.shutdown().await;
}

#[tokio::test]
async fn owner_and_company_filters_combine_with_search() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let manager = crm.login_as(Role::Manager).await;
    let fixture = four_deals(&crm, &rep, &manager).await;
    let (sutlej, beas, rep2) = (
        fixture.sutlej.to_string(),
        fixture.beas.to_string(),
        fixture.rep2.to_string(),
    );

    let found = async |query: &[(&str, &str)]| {
        let mut names = titles(&list(&rep, query).await);
        names.sort_unstable();
        names
    };
    assert_eq!(
        found(&[("filter[owner_id]", &rep2)]).await,
        ["Chenab pilot", "Ravi upsell"]
    );
    assert_eq!(
        found(&[("filter[company_id]", &sutlej)]).await,
        ["Ravi upsell", "Sutlej renewal"]
    );
    assert_eq!(
        found(&[("filter[company_id]", &sutlej), ("filter[owner_id]", &rep2)]).await,
        ["Ravi upsell"]
    );
    assert_eq!(
        found(&[("filter[company_id][in]", &format!("{sutlej},{beas}"))])
            .await
            .len(),
        4
    );
    assert_eq!(
        found(&[("filter[company_id][ne]", &sutlej)]).await,
        ["Beas expansion", "Chenab pilot"]
    );
    assert_eq!(
        found(&[
            ("filter[stage]", "lead"),
            ("filter[company_id]", &beas),
            ("search", "PILOT"),
        ])
        .await,
        ["Chenab pilot"]
    );
    // Filters narrow the total, and pages keep them.
    let page = list(
        &rep,
        &[
            ("filter[stage][in]", "lead,won"),
            ("per_page", "2"),
            ("page", "2"),
        ],
    )
    .await;
    assert_eq!(page["data"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        page["meta"],
        json!({ "current_page": 2, "per_page": 2, "total": 3, "last_page": 2 })
    );
    crm.shutdown().await;
}

#[tokio::test]
async fn sort_orders_by_the_fields_of_the_allow_list() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let manager = crm.login_as(Role::Manager).await;
    four_deals(&crm, &rep, &manager).await;

    let sorted = async |sort: &str| titles(&list(&rep, &[("sort", sort)]).await);
    assert_eq!(
        sorted("amount_cents").await,
        [
            "Beas expansion",
            "Ravi upsell",
            "Sutlej renewal",
            "Chenab pilot"
        ]
    );
    assert_eq!(
        sorted("-amount_cents").await,
        [
            "Chenab pilot",
            "Sutlej renewal",
            "Ravi upsell",
            "Beas expansion"
        ]
    );
    assert_eq!(
        sorted("title").await,
        [
            "Beas expansion",
            "Chenab pilot",
            "Ravi upsell",
            "Sutlej renewal"
        ]
    );
    assert_eq!(
        sorted("expected_close_date").await,
        [
            "Beas expansion",
            "Chenab pilot",
            "Sutlej renewal",
            "Ravi upsell"
        ]
    );
    assert_eq!(
        sorted("-expected_close_date").await,
        [
            "Ravi upsell",
            "Sutlej renewal",
            "Chenab pilot",
            "Beas expansion"
        ]
    );
    // Several fields, and a repeated `sort` adds terms: lead deals first by
    // amount, then the rest.
    let by_stage_then_amount = titles(
        &list(
            &rep,
            &[("filter[stage]", "lead"), ("sort", "-amount_cents,title")],
        )
        .await,
    );
    assert_eq!(by_stage_then_amount, ["Chenab pilot", "Sutlej renewal"]);
    let repeated = titles(&list(&rep, &[("sort", "-title"), ("sort", "amount_cents")]).await);
    assert_eq!(
        repeated,
        [
            "Sutlej renewal",
            "Ravi upsell",
            "Chenab pilot",
            "Beas expansion"
        ]
    );
    // Without `sort`, the newest deal comes first.
    let newest = titles(&list(&rep, &[]).await);
    assert_eq!(newest.first().map(String::as_str), Some("Chenab pilot"));
    crm.shutdown().await;
}

#[tokio::test]
async fn what_the_list_does_not_allow_is_a_400_naming_the_parameter() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let manager = crm.login_as(Role::Manager).await;
    four_deals(&crm, &rep, &manager).await;

    // (the parameter sent, its value, the parameter the 400 names)
    for (sent, value, param) in [
        // A field the list does not declare, or a column that is not a filter.
        ("filter[x]", "1", "filter[x]"),
        (
            "filter[password][contains]",
            "1",
            "filter[password][contains]",
        ),
        ("filter[title]", "1", "filter[title]"),
        (
            "filter[amount_cents][gte]",
            "1",
            "filter[amount_cents][gte]",
        ),
        // An operator the field does not take.
        ("filter[stage][gt]", "lead", "filter[stage][gt]"),
        (
            "filter[owner_id][contains]",
            "a",
            "filter[owner_id][contains]",
        ),
        // A value of the wrong type.
        ("filter[stage]", "closed", "filter[stage]"),
        ("filter[owner_id]", "nope", "filter[owner_id]"),
        ("filter[company_id][in]", "nope", "filter[company_id][in]"),
        ("filter[stage][null]", "maybe", "filter[stage][null]"),
        // Sorting and paging.
        ("sort", "password", "sort"),
        ("sort", "title,title", "sort"),
        ("per_page", "101", "per_page"),
        ("page", "0", "page"),
    ] {
        rep.get("/api/v1/deals")
            .query(&[(sent, value)])
            .send()
            .await
            .assert_validation_error(param);
    }
    // Every problem is reported at once.
    let errors = rep
        .get("/api/v1/deals")
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
    let agra = company(&crm, "Agra", rep.id()).await;
    create_deal(&rep, new_deal("Agra renewal", agra, 100)).await;

    let page = list(&rep, &[("per_page", "100")]).await;
    assert_eq!(page["meta"]["per_page"], 100);
    // It used to be clamped to 100; now it is a 400.
    rep.get("/api/v1/deals")
        .query(&[("per_page", "101")])
        .send()
        .await
        .assert_validation_error("per_page");
    rep.get("/api/v1/deals")
        .query(&[("per_page", "0")])
        .send()
        .await
        .assert_validation_error("per_page");
    // Numbered and cursor pages do not mix.
    rep.get("/api/v1/deals")
        .query(&[("page", "2"), ("limit", "10")])
        .send()
        .await
        .assert_validation_error("limit");
    crm.shutdown().await;
}

#[tokio::test]
async fn the_old_flat_parameters_are_ignored() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let manager = crm.login_as(Role::Manager).await;
    let fixture = four_deals(&crm, &rep, &manager).await;

    // `stage=won` and friends are not the list query's: no error, no filter.
    let page = list(
        &rep,
        &[
            ("stage", "won"),
            ("owner_id", &fixture.rep2.to_string()),
            ("company_id", &fixture.beas.to_string()),
        ],
    )
    .await;
    assert_eq!(page["meta"]["total"], 4);
    // A flat name that is no stage is not validated either.
    let page = list(&rep, &[("stage", "closed"), ("utm_source", "mail")]).await;
    assert_eq!(page["meta"]["total"], 4);
    // The same names inside `filter[..]` do filter.
    let page = list(&rep, &[("filter[stage]", "won")]).await;
    assert_eq!(page["meta"]["total"], 1);
    crm.shutdown().await;
}

/// Quotes and SQL in a value are text: the search finds a title that has
/// them, and a list value that is no stage or id is a 400, not a query.
#[tokio::test]
async fn hostile_values_are_just_values() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let agra = company(&crm, "Agra", rep.id()).await;
    create_deal(&rep, new_deal("Agra renewal", agra, 100)).await;
    create_deal(&rep, new_deal("Robert'); DROP TABLE deals;--", agra, 200)).await;

    let none = list(&rep, &[("search", "' OR 1=1")]).await;
    assert_eq!(none["meta"]["total"], 0);
    let found = list(&rep, &[("search", "'); DROP TABLE")]).await;
    assert_eq!(titles(&found), ["Robert'); DROP TABLE deals;--"]);
    let wildcards = list(&rep, &[("search", "%")]).await;
    assert_eq!(wildcards["meta"]["total"], 0);
    for (param, value) in [
        ("filter[stage]", "' OR 1=1"),
        ("filter[stage][in]", "lead') OR ('1'='1"),
        ("filter[owner_id]", "' OR 1=1"),
        ("filter[company_id][in]", "1; DROP TABLE deals"),
        ("sort", "title; DROP TABLE deals"),
    ] {
        rep.get("/api/v1/deals")
            .query(&[(param, value)])
            .send()
            .await
            .assert_status(400);
    }
    // Nothing happened to the table.
    let all = list(&rep, &[]).await;
    assert_eq!(all["meta"]["total"], 2);
    crm.shutdown().await;
}

#[tokio::test]
async fn a_filtered_list_still_needs_a_signed_in_reader() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let agra = company(&crm, "Agra", rep.id()).await;
    create_deal(&rep, new_deal("Agra renewal", agra, 100)).await;

    // Everyone with a CRM role reads deals; a guest reads none, whatever the
    // filters say.
    for role in [Role::Rep2, Role::Manager, Role::Admin] {
        let client = crm.login_as(role).await;
        let page = list(&client, &[("filter[stage]", "lead")]).await;
        assert_eq!(page["meta"]["total"], 1, "{role:?}");
    }
    crm.client()
        .get("/api/v1/deals")
        .query(&[("filter[stage]", "lead")])
        .send()
        .await
        .assert_error(401, None);
    crm.shutdown().await;
}
