//! A value a user can't see reaches them by no route: field rules and
//! `#[model(hidden)]` hold on titles, lists, pickers, related columns, exports,
//! forms, search, sort, filters and metrics.

use navree_db::Db;
use navree_helm::{Field, ModuleBuilderHelmExt, Resource};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_orm::Model;
use navree_testing::{TestApp, TestClient, TestResponse};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::*;

async fn page(client: &TestClient, path: &str) -> TestResponse {
    client.get(path).header("accept", "text/html").send().await
}

fn plain(response: TestResponse) -> String {
    response.text().replace("&#x2f;", "/")
}

/// Every route a user reads a deal by, with the status and text of each.
async fn deal_routes(client: &TestClient, id: Uuid) -> Vec<(String, u16, String)> {
    let mut seen = Vec::new();
    for path in [
        "/admin/api/resources/deals".to_owned(),
        format!("/admin/api/resources/deals/{id}"),
        "/admin/api/resources/deals/options".to_owned(),
        "/admin/api/resources".to_owned(),
    ] {
        let response = client.get(&path).send().await;
        seen.push((path, response.status().as_u16(), response.text()));
    }
    for path in [
        "/admin/resources/deals".to_owned(),
        format!("/admin/resources/deals/{id}"),
        "/admin/resources/deals/export.csv".to_owned(),
    ] {
        let response = page(client, &path).await;
        seen.push((path, response.status().as_u16(), plain(response)));
    }
    seen
}

/// What the metadata says of deals.
fn deals_in(index: &Value) -> &Value {
    index["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|resource| resource["slug"] == "deals")
        .unwrap()
}

fn short(id: Uuid) -> String {
    let text = id.to_string();
    text[text.len() - 8..].to_owned()
}

#[tokio::test]
async fn a_title_the_rules_hide_reaches_no_route() {
    let app = app().await;
    let d = deal(
        &app,
        DealSeed {
            title: "TITLESECRET",
            ..DealSeed::default()
        },
    )
    .await;
    let (_, hides) = user(&app, "no-title").await;
    for (path, status, text) in deal_routes(&hides, d.id).await {
        assert_eq!(status, 200, "{path}");
        assert!(!text.contains("TITLESECRET"), "{path}: {text}");
    }
    // The record is named by its label and short key instead.
    let fallback = format!("Deal #{}", short(d.id));
    let list = hides
        .get("/admin/api/resources/deals")
        .send()
        .await
        .json_value();
    assert_eq!(list["data"][0]["title"], fallback.as_str());
    let shown = hides
        .get(&format!("/admin/api/resources/deals/{}", d.id))
        .send()
        .await
        .json_value();
    assert_eq!(shown["data"]["title"], fallback.as_str());
    let options = hides
        .get("/admin/api/resources/deals/options")
        .send()
        .await
        .json_value();
    assert_eq!(options["data"][0]["title"], fallback.as_str());
    let detail = plain(page(&hides, &format!("/admin/resources/deals/{}", d.id)).await);
    assert!(
        detail.contains(&format!("<title>{fallback}")),
        "the page's title: {detail}"
    );
    // Someone the rule doesn't bind reads it everywhere.
    let (_, manager) = user(&app, "manager").await;
    let list = manager
        .get("/admin/api/resources/deals")
        .send()
        .await
        .json_value();
    assert_eq!(list["data"][0]["title"], "TITLESECRET");
    let csv = page(&manager, "/admin/resources/deals/export.csv")
        .await
        .text();
    assert!(csv.contains("TITLESECRET"), "{csv}");
}

#[tokio::test]
async fn a_title_a_rule_hides_on_some_records_shows_on_the_others() {
    let app = app().await;
    let (me, client) = user(&app, "title-if-owner").await;
    let mine = deal(
        &app,
        DealSeed {
            title: "MINE",
            owner: Some(me),
            ..DealSeed::default()
        },
    )
    .await;
    let other = deal(
        &app,
        DealSeed {
            title: "OTHERSECRET",
            ..DealSeed::default()
        },
    )
    .await;
    for (path, _, text) in deal_routes(&client, other.id).await {
        assert!(!text.contains("OTHERSECRET"), "{path}: {text}");
    }
    let shown = client
        .get(&format!("/admin/api/resources/deals/{}", mine.id))
        .send()
        .await
        .json_value();
    assert_eq!(shown["data"]["title"], "MINE");
    // The column can't be asked about: an order or a search would tell.
    client
        .get("/admin/api/resources/deals?sort=title")
        .send()
        .await
        .assert_status(400);
    client
        .get("/admin/api/resources/deals?search=OTHER")
        .send()
        .await
        .assert_status(400);
}

#[tokio::test]
async fn the_title_of_a_related_record_the_rules_hide_reaches_no_route() {
    let app = app().await;
    let agra = company(&app, "COMPANYSECRET", Some("Town")).await;
    let d = deal(
        &app,
        DealSeed {
            title: "Open deal",
            company: Some(agra.id),
            ..DealSeed::default()
        },
    )
    .await;
    let (_, hides) = user(&app, "no-company-name").await;
    for (path, status, text) in deal_routes(&hides, d.id).await {
        assert_eq!(status, 200, "{path}");
        assert!(!text.contains("COMPANYSECRET"), "{path}: {text}");
    }
    for path in [
        "/admin/api/resources/companies".to_owned(),
        format!("/admin/api/resources/companies/{}", agra.id),
        "/admin/api/resources/companies/options".to_owned(),
    ] {
        let text = hides.get(&path).send().await.assert_status(200).text();
        assert!(!text.contains("COMPANYSECRET"), "{path}: {text}");
    }
    for path in [
        "/admin/resources/companies".to_owned(),
        format!("/admin/resources/companies/{}", agra.id),
        "/admin/resources/companies/export.csv".to_owned(),
    ] {
        let text = plain(page(&hides, &path).await);
        assert!(!text.contains("COMPANYSECRET"), "{path}: {text}");
    }
    let csv = page(&hides, "/admin/resources/deals/export.csv")
        .await
        .text();
    assert!(
        csv.contains(&format!("Company #{}", short(agra.id))),
        "named by its label and short key: {csv}"
    );
}

/// A company whose API token, `#[model(hidden)]`, is set.
async fn company_with_token(app: &TestApp) -> Company {
    let db = app.get::<Db>().await;
    let company = Company {
        id: Uuid::now_v7(),
        name: "TokenCo".into(),
        api_token: Some("TOKENSECRET".into()),
        created_at: now(),
        ..Company::default()
    };
    Company::insert(&*db, [company.clone()]).await.unwrap();
    company
}

#[tokio::test]
async fn a_hidden_column_reaches_no_route_for_anyone() {
    let app = app().await;
    let c = company_with_token(&app).await;
    for role in ["rep", "manager", ADMIN] {
        let (_, client) = user(&app, role).await;
        for path in [
            "/admin/api/resources/companies".to_owned(),
            format!("/admin/api/resources/companies/{}", c.id),
            "/admin/api/resources/companies/options".to_owned(),
        ] {
            let text = client.get(&path).send().await.assert_status(200).text();
            assert!(!text.contains("TOKENSECRET"), "{role} {path}: {text}");
        }
        for path in [
            "/admin/resources/companies".to_owned(),
            format!("/admin/resources/companies/{}", c.id),
            "/admin/resources/companies/export.csv".to_owned(),
        ] {
            let text = plain(page(&client, &path).await);
            assert!(!text.contains("TOKENSECRET"), "{role} {path}: {text}");
        }
    }
    // Not a column of the file or the page at all, for anyone.
    let (_, manager) = user(&app, "manager").await;
    let csv = page(&manager, "/admin/resources/companies/export.csv")
        .await
        .text();
    assert!(!csv.contains("API token"), "{csv}");
    let detail = plain(page(&manager, &format!("/admin/resources/companies/{}", c.id)).await);
    assert!(!detail.contains("API token"), "{detail}");
    // The edit form doesn't fill it in.
    let form = plain(
        page(
            &manager,
            &format!("/admin/resources/companies/{}/edit", c.id),
        )
        .await
        .assert_status(200),
    );
    assert!(!form.contains("TOKENSECRET"), "{form}");
    assert!(
        form.contains("name=\"api_token\""),
        "still set here: {form}"
    );
    // What the metadata says.
    let index = manager
        .get("/admin/api/resources")
        .send()
        .await
        .json_value();
    let companies = index["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["slug"] == "companies")
        .unwrap();
    let token = companies["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "api_token")
        .unwrap();
    assert_eq!(token["on_index"], false);
    assert_eq!(token["on_detail"], false);
    assert_eq!(token["sortable"], false);
}

#[tokio::test]
async fn saving_a_form_leaves_a_hidden_column_it_was_not_told_about() {
    let app = app().await;
    let c = company_with_token(&app).await;
    let (_, manager) = user(&app, "manager").await;
    let edit = |client: &TestClient, form: Vec<(&'static str, &'static str)>| {
        let path = format!("/admin/resources/companies/{}", c.id);
        let client = client.clone();
        async move {
            client
                .post(&path)
                .header("accept", "text/html")
                .form(&form)
                .send()
                .await
                .status()
                .as_u16()
        }
    };
    // Blank is what the form shows: it isn't a new value.
    assert_eq!(
        edit(&manager, vec![("name", "Renamed"), ("api_token", "")]).await,
        303
    );
    let saved = reload_company(&app, c.id).await.unwrap();
    assert_eq!(saved.name, "Renamed");
    assert_eq!(saved.api_token.as_deref(), Some("TOKENSECRET"));
    // A new one replaces it.
    assert_eq!(
        edit(
            &manager,
            vec![("name", "Renamed"), ("api_token", "NEWTOKEN")]
        )
        .await,
        303
    );
    let saved = reload_company(&app, c.id).await.unwrap();
    assert_eq!(saved.api_token.as_deref(), Some("NEWTOKEN"));
    // And the API's answer to a write doesn't echo it.
    let answer = manager
        .patch(&format!("/admin/api/resources/companies/{}", c.id))
        .json(&json!({ "api_token": "THIRDTOKEN" }))
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(!answer.contains("THIRDTOKEN"), "{answer}");
}

#[tokio::test]
async fn search_tells_nothing_about_a_column_the_rules_hide() {
    let app = app().await;
    deal(
        &app,
        DealSeed {
            title: "TITLESECRET",
            ..DealSeed::default()
        },
    )
    .await;
    let (_, hides) = user(&app, "no-title").await;
    // The only searched column is hidden: there is nothing to search by.
    for term in ["TITLES", "NOPE"] {
        hides
            .get(&format!("/admin/api/resources/deals?search={term}"))
            .send()
            .await
            .assert_status(400);
        page(
            &hides,
            &format!("/admin/resources/deals/export.csv?search={term}"),
        )
        .await
        .assert_status(400);
        let options = hides
            .get(&format!("/admin/api/resources/deals/options?search={term}"))
            .send()
            .await
            .assert_status(200)
            .json_value();
        assert_eq!(options["data"], json!([]), "{term}");
    }
    let html = plain(
        page(&hides, "/admin/resources/deals")
            .await
            .assert_status(200),
    );
    assert!(!html.contains("name=\"search\""), "no search box: {html}");
    let index = hides.get("/admin/api/resources").send().await.json_value();
    assert_eq!(deals_in(&index)["search"], false, "{index}");

    // Of several, the column it may read is still searched.
    let agra = company(&app, "COMPANYSECRET", Some("Town")).await;
    let (_, no_name) = user(&app, "no-company-name").await;
    let found = |query: &'static str| {
        let no_name = no_name.clone();
        async move {
            no_name
                .get(&format!("/admin/api/resources/companies?search={query}"))
                .send()
                .await
                .assert_status(200)
                .json_value()["data"]
                .as_array()
                .unwrap()
                .len()
        }
    };
    assert_eq!(found("Town").await, 1);
    assert_eq!(found("COMPANYSECRET").await, 0, "{}", agra.id);
}

#[tokio::test]
async fn a_list_is_not_sorted_by_a_column_the_rules_hide() {
    let app = app().await;
    for (title, amount_cents) in [("Small", 5), ("Big", 5000), ("Middle", 500)] {
        deal(
            &app,
            DealSeed {
                title,
                amount_cents,
                ..DealSeed::default()
            },
        )
        .await;
    }
    let (_, hides) = user(&app, "no-amount").await;
    hides
        .get("/admin/api/resources/deals?sort=amount_cents")
        .send()
        .await
        .assert_status(400);
    hides
        .get("/admin/api/resources/deals?sort=-amount_cents")
        .send()
        .await
        .assert_status(400);
    page(&hides, "/admin/resources/deals?sort=amount_cents")
        .await
        .assert_status(400);
    page(
        &hides,
        "/admin/resources/deals/export.csv?sort=amount_cents",
    )
    .await
    .assert_status(400);
    // Nothing offers it, and the metadata says so.
    let html = plain(
        page(&hides, "/admin/resources/deals")
            .await
            .assert_status(200),
    );
    assert!(!html.contains("sort=amount_cents"), "{html}");
    assert!(html.contains("sort=title"), "{html}");
    let index = hides.get("/admin/api/resources").send().await.json_value();
    let amount = deals_in(&index)["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "amount_cents")
        .unwrap();
    assert_eq!(amount["sortable"], false, "{index}");
}

#[tokio::test]
async fn a_default_order_on_a_hidden_column_gives_way_to_the_key() {
    let app = app().await;
    // By name these come in the other order: the name is the default sort.
    for name in ["Zeta", "Alpha", "Middle"] {
        company(&app, name, None).await;
    }
    let (_, hides) = user(&app, "no-company-name").await;
    let list = hides
        .get("/admin/api/resources/companies")
        .send()
        .await
        .assert_status(200)
        .json_value();
    let ids: Vec<String> = ids(&list);
    let mut by_key = ids.clone();
    by_key.sort();
    by_key.reverse();
    assert_eq!(ids, by_key, "newest key first: {list}");
    page(&hides, "/admin/resources/companies/export.csv")
        .await
        .assert_status(200);
    // The picker doesn't sort by it either.
    let options = hides
        .get("/admin/api/resources/companies/options")
        .send()
        .await
        .json_value();
    let mut picked: Vec<String> = options["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["id"].as_str().unwrap().to_owned())
        .collect();
    let sorted = {
        let mut sorted = picked.clone();
        sorted.sort();
        sorted
    };
    assert_eq!(picked, sorted, "{options}");
    picked.clear();
}

#[tokio::test]
async fn filters_on_a_column_the_rules_hide_are_refused() {
    let app = app().await;
    let agra = company(&app, "Agra", None).await;
    deal(
        &app,
        DealSeed {
            title: "Listed",
            stage: "won",
            amount_cents: 5000,
            company: Some(agra.id),
            expected_close_date: chrono::NaiveDate::from_ymd_opt(2026, 11, 1),
            ..DealSeed::default()
        },
    )
    .await;
    let cases = [
        ("no-stage", "filter%5Bstage%5D=won"),
        ("no-stage", "filter%5Bstage%5D%5Bin%5D=won%2Clost"),
        ("no-company", "filter%5Bcompany_id%5D=ID"),
        ("no-company", "filter%5Bcompany_id%5D%5Bin%5D=ID"),
        ("no-company", "via%5Bcompany_id%5D=ID"),
        (
            "no-close-date",
            "filter%5Bexpected_close_date%5D=2026-11-01..2026-11-02",
        ),
        // A filter of the app's own names no column: it can't be vouched for.
        ("no-amount", "filter%5Bmin_amount%5D=1000"),
    ];
    for (role, query) in cases {
        let (_, client) = user(&app, role).await;
        let query = query.replace("ID", &agra.id.to_string());
        client
            .get(&format!("/admin/api/resources/deals?{query}"))
            .send()
            .await
            .assert_status(400);
        page(
            &client,
            &format!("/admin/resources/deals/export.csv?{query}"),
        )
        .await
        .assert_status(400);
        // Without it the same user gets the list.
        client
            .get("/admin/api/resources/deals")
            .send()
            .await
            .assert_status(200);
    }
    // The toolbar and the metadata don't offer it.
    let (_, no_stage) = user(&app, "no-stage").await;
    let html = plain(
        page(&no_stage, "/admin/resources/deals")
            .await
            .assert_status(200),
    );
    assert!(!html.contains("filter[stage]"), "{html}");
    let index = no_stage
        .get("/admin/api/resources")
        .send()
        .await
        .json_value();
    let filters: Vec<&str> = deals_in(&index)["filters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    assert!(!filters.contains(&"stage"), "{filters:?}");
}

#[tokio::test]
async fn a_relationship_panel_on_a_key_the_rules_hide_is_left_out() {
    let app = app().await;
    let agra = company(&app, "Agra", None).await;
    deal(
        &app,
        DealSeed {
            title: "PANELDEAL",
            company: Some(agra.id),
            ..DealSeed::default()
        },
    )
    .await;
    let (_, hides) = user(&app, "no-company").await;
    let html = plain(
        page(&hides, &format!("/admin/resources/companies/{}", agra.id))
            .await
            .assert_status(200),
    );
    assert!(!html.contains("PANELDEAL"), "{html}");
}

#[tokio::test]
async fn a_metric_of_a_column_the_rules_hide_is_not_given() {
    let app = app().await;
    deal(
        &app,
        DealSeed {
            title: "Counted",
            amount_cents: 77_700,
            ..DealSeed::default()
        },
    )
    .await;
    let (_, hides) = user(&app, "no-amount").await;
    let metric = |slug: &'static str| {
        let hides = hides.clone();
        async move {
            hides
                .get(&format!("/admin/api/resources/deals/metrics/{slug}"))
                .send()
                .await
                .status()
                .as_u16()
        }
    };
    assert_eq!(metric("open-pipeline").await, 403);
    assert_eq!(metric("pipeline-by-company").await, 403);
    assert_eq!(metric("new-deals").await, 200);
    assert_eq!(metric("deals-by-stage").await, 200);
    for path in ["/admin/resources/deals", "/admin"] {
        let html = plain(page(&hides, path).await.assert_status(200));
        assert!(!html.contains("Open pipeline"), "{path}: {html}");
        assert!(!html.contains("$777.00"), "{path}: {html}");
    }
    let index = hides.get("/admin/api/resources").send().await.json_value();
    let slugs: Vec<&str> = deals_in(&index)["metrics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["slug"].as_str().unwrap())
        .collect();
    assert!(!slugs.contains(&"open-pipeline"), "{slugs:?}");
    assert!(slugs.contains(&"new-deals"), "{slugs:?}");
    // By the group as well.
    let (_, no_stage) = user(&app, "no-stage").await;
    no_stage
        .get("/admin/api/resources/deals/metrics/deals-by-stage")
        .send()
        .await
        .assert_status(403);
}

#[tokio::test]
async fn a_computed_field_a_rule_names_is_hidden_and_money_needs_its_currency() {
    let app = app().await;
    let d = deal(
        &app,
        DealSeed {
            stage: "won",
            amount_cents: 12_345,
            ..DealSeed::default()
        },
    )
    .await;
    let (_, no_probability) = user(&app, "no-probability").await;
    let shown = no_probability
        .get(&format!("/admin/api/resources/deals/{}", d.id))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert!(shown["data"]["values"].get("probability").is_none());
    assert!(shown["data"]["display"].get("probability").is_none());
    let (_, no_currency) = user(&app, "no-currency").await;
    let shown: Value = no_currency
        .get(&format!("/admin/api/resources/deals/{}", d.id))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert!(shown["data"]["display"].get("currency").is_none());
    assert!(
        shown["data"]["display"].get("amount_cents").is_none(),
        "an amount says its currency: {shown}"
    );
}

#[tokio::test]
async fn a_filter_of_the_apps_own_runs_for_who_may_read_what_it_says_it_reads() {
    let app = app().await;
    deal(
        &app,
        DealSeed {
            title: "Big",
            amount_cents: 5000,
            ..DealSeed::default()
        },
    )
    .await;
    // It says it reads the amount, which this user may read.
    let (_, no_title) = user(&app, "no-title").await;
    no_title
        .get("/admin/api/resources/deals?filter%5Bmin_amount%5D=1000")
        .send()
        .await
        .assert_status(200);
    // This one doesn't say: for someone with a column hidden, it could read that.
    no_title
        .get("/admin/api/resources/deals?filter%5Bopen%5D=1")
        .send()
        .await
        .assert_status(400);
    // With nothing hidden it runs.
    let (_, rep) = user(&app, "rep").await;
    rep.get("/admin/api/resources/deals?filter%5Bopen%5D=1")
        .send()
        .await
        .assert_status(200);
}

/// Deals with a field a user reads on its page and doesn't take in a file.
struct NoExportNotes;

impl Injectable for NoExportNotes {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(NoExportNotes) })
    }
}

impl Resource for NoExportNotes {
    type Model = Deal;

    const LABEL: &'static str = "Quiet";
    const SLUG: &'static str = "no-export";
    const TITLE: &'static str = "title";
    const SIDEBAR_COUNT: bool = false;

    fn fields(&self) -> Vec<Field> {
        vec![
            Field::text("title"),
            Field::textarea("notes").hide_on_index().hide_on_export(),
        ]
    }
}

struct NoExportModule;

impl Module for NoExportModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.resource::<NoExportNotes>();
    }
}

#[tokio::test]
async fn a_field_can_stay_out_of_the_file_and_on_the_page() {
    let app = builder().module(NoExportModule).build().await;
    let d = deal(&app, DealSeed::default()).await;
    let (_, manager) = user(&app, "manager").await;
    let csv = page(&manager, "/admin/resources/no-export/export.csv")
        .await
        .assert_status(200)
        .text();
    assert!(!csv.contains("Secret"), "{csv}");
    assert!(!csv.contains("Notes"), "{csv}");
    let detail = plain(page(&manager, &format!("/admin/resources/no-export/{}", d.id)).await);
    assert!(detail.contains("Secret"), "{detail}");
}
