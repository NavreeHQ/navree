//! The Navree UI tools: `list_routes` marking pages, `list_components` and
//! `render_page`.

use navree_navigator::NavigatorOptions;
use navree_navigator::app::ProcessOutput;
use navree_navigator::docs::EMBEDDED_VERSION;
use serde_json::{Value, json};

use crate::support::{Fixture, call, ok, refused, write};

/// An app whose route table says which routes are pages.
fn routes_with_pages() -> Value {
    json!([
        {"method": "GET", "path": "/", "access": "public", "module": "SiteModule", "handler": "HomeController::index", "page": true},
        {"method": "GET", "path": "/deals", "access": "auth", "module": "DealsModule", "handler": "DealsController::index", "page": true},
        {"method": "POST", "path": "/deals", "access": "can:create Deal", "module": "DealsModule", "handler": "DealsController::store", "page": false},
        {"method": "GET", "path": "/api/v1/deals", "access": "can:read Deal", "module": "DealsModule", "handler": "DealsController::list", "page": false}
    ])
}

fn app(routes: Value) -> Fixture {
    let fixture = Fixture::new("");
    fixture.built();
    fixture.answers(&["routes", "--json"], routes);
    fixture
}

fn paths(routes: &Value) -> Vec<String> {
    routes
        .as_array()
        .unwrap()
        .iter()
        .map(|r| format!("{} {}", r["method"].as_str().unwrap(), r["path"].as_str().unwrap()))
        .collect()
}

#[tokio::test]
async fn routes_say_which_are_pages() {
    let fixture = app(routes_with_pages());
    let navigator = fixture.navigator();

    let all = call(&navigator, "list_routes", json!({})).await;
    assert_eq!(all.as_array().unwrap().len(), 4);
    assert_eq!(all[0]["page"], true);
    assert_eq!(all[2]["page"], false, "a route that isn't a page says so");

    let pages = call(&navigator, "list_routes", json!({"pages_only": true})).await;
    assert_eq!(paths(&pages), ["GET /", "GET /deals"]);
    assert!(pages.as_array().unwrap().iter().all(|r| r["page"] == true));

    let in_deals = call(
        &navigator,
        "list_routes",
        json!({"pages_only": true, "path": "/deals"}),
    )
    .await;
    assert_eq!(paths(&in_deals), ["GET /deals"], "the filters work together");

    let not_filtered = call(&navigator, "list_routes", json!({"pages_only": false})).await;
    assert_eq!(not_filtered.as_array().unwrap().len(), 4);
}

#[tokio::test]
async fn routes_that_do_not_say_are_unknown_and_pages_only_says_why() {
    let fixture = app(json!([
        {"method": "GET", "path": "/deals", "access": "auth", "module": "DealsModule", "handler": null},
        {"method": "GET", "path": "/health", "access": "public", "module": "ObserveModule", "handler": null}
    ]));
    let navigator = fixture.navigator();

    let all = call(&navigator, "list_routes", json!({})).await;
    assert_eq!(all.as_array().unwrap().len(), 2);
    assert!(
        all.as_array().unwrap().iter().all(|r| r.get("page").is_none()),
        "no page mark is invented: {all}"
    );
    let unfiltered = call(&navigator, "list_routes", json!({"pages_only": false})).await;
    assert_eq!(unfiltered.as_array().unwrap().len(), 2);

    let message = refused(&navigator, "list_routes", json!({"pages_only": true})).await;
    assert!(
        message.contains("doesn't say which routes are pages"),
        "{message}"
    );
    let message = refused(&navigator, "list_routes", json!({"pages_only": "yes"})).await;
    assert!(message.contains("invalid arguments"), "{message}");
}

#[tokio::test]
async fn an_app_with_no_routes_has_no_pages() {
    let fixture = app(json!([]));
    let pages = call(&fixture.navigator(), "list_routes", json!({"pages_only": true})).await;
    assert_eq!(pages, json!([]));
}

#[tokio::test]
async fn the_components_are_listed_with_the_version_of_the_catalog() {
    let fixture = Fixture::new("");
    let navigator = fixture.navigator();
    let all = call(&navigator, "list_components", json!({})).await;
    assert_eq!(all["version"], EMBEDDED_VERSION);
    assert!(all.get("note").is_none(), "the app uses this version: {all}");
    let total = all["total"].as_u64().unwrap();
    assert!(total >= 90, "the whole catalog is built in: {total}");
    assert_eq!(all["count"].as_u64().unwrap(), total);
    assert_eq!(all["components"].as_array().unwrap().len() as u64, total);
    assert!(
        all["groups"]
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g["id"] == "inputs" && g["title"] == "Inputs")
    );

    let button = all["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "button")
        .expect("the button");
    assert_eq!(button["name"], "Button");
    assert_eq!(button["group"], "actions");
    assert_eq!(button["docs"], "components/button");
    assert!(
        button["types"].as_array().unwrap().iter().any(|t| t == "Button"),
        "{button}"
    );
    assert!(!button["summary"].as_str().unwrap().is_empty());
}

#[tokio::test]
async fn a_docs_slug_of_the_catalog_opens_with_read_doc() {
    let fixture = Fixture::new("");
    let navigator = fixture.navigator();
    let found = call(&navigator, "list_components", json!({"query": "date picker"})).await;
    let picker = &found["components"][0];
    assert_eq!(picker["id"], "date-picker", "the best match is first: {found}");
    assert_eq!(picker["docs"], "components/date-picker");
    let page = call(&navigator, "read_doc", json!({"slug": picker["docs"]})).await;
    assert!(
        page.as_str().unwrap().starts_with("# DatePicker"),
        "{}",
        page.as_str().unwrap().lines().next().unwrap_or_default()
    );
}

#[tokio::test]
async fn components_are_filtered_by_query_and_group() {
    let fixture = Fixture::new("");
    let navigator = fixture.navigator();

    let query = call(&navigator, "list_components", json!({"query": "datepicker"})).await;
    let ids: Vec<&str> = query["components"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"date-picker"), "{ids:?}");
    assert!(query["count"].as_u64().unwrap() < query["total"].as_u64().unwrap());
    assert!(query.get("hint").is_none());

    let by_type = call(&navigator, "list_components", json!({"query": "ButtonVariant"})).await;
    assert_eq!(by_type["components"][0]["id"], "button");

    let inputs = call(&navigator, "list_components", json!({"group": "inputs"})).await;
    let components = inputs["components"].as_array().unwrap();
    assert!(components.len() > 5);
    assert!(components.iter().all(|c| c["group"] == "inputs"));
    assert!(components.iter().any(|c| c["id"] == "input"));

    let both = call(
        &navigator,
        "list_components",
        json!({"group": "inputs", "query": "select"}),
    )
    .await;
    assert!(both["count"].as_u64().unwrap() > 0);
    assert!(both["count"].as_u64().unwrap() < inputs["count"].as_u64().unwrap());

    let none = call(&navigator, "list_components", json!({"query": "zzzqqq"})).await;
    assert_eq!(none["count"], 0);
    assert_eq!(none["components"], json!([]));
    assert!(none["hint"].as_str().unwrap().contains("without a query"));

    let message = refused(&navigator, "list_components", json!({"group": "nope"})).await;
    assert!(message.contains("inputs"), "lists the groups: {message}");
    let message = refused(&navigator, "list_components", json!({"filter": "x"})).await;
    assert!(message.contains("invalid arguments"), "{message}");
}

#[tokio::test]
async fn the_guides_without_a_group_are_listed_in_one() {
    let fixture = Fixture::new("");
    let guides = call(
        &fixture.navigator(),
        "list_components",
        json!({"group": "guides"}),
    )
    .await;
    let ids: Vec<&str> = guides["components"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"icons") && ids.contains(&"themes"), "{ids:?}");
    let anything = guides["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "build-anything")
        .expect("an entry with no docs page");
    assert!(anything.get("docs").is_none(), "no slug to read: {anything}");
}

#[tokio::test]
async fn another_navree_version_in_the_lockfile_is_noted() {
    let fixture = Fixture::new("");
    write(
        &fixture.root().join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"navree\"\nversion = \"0.0.7\"\n",
    );
    let answer = call(&fixture.navigator(), "list_components", json!({})).await;
    assert_eq!(answer["version"], EMBEDDED_VERSION);
    assert_eq!(answer["app_version"], "0.0.7");
    let note = answer["note"].as_str().unwrap();
    assert!(
        note.contains("0.0.7") && note.contains(EMBEDDED_VERSION),
        "{note}"
    );
}

#[tokio::test]
async fn the_components_answer_outside_an_app_too() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let fixture = Fixture::new("");
    let navigator = fixture.navigator_with(NavigatorOptions {
        path: dir.path().to_path_buf(),
        docs_offline: true,
        ..NavigatorOptions::default()
    });
    let answer = call(&navigator, "list_components", json!({"query": "card"})).await;
    assert!(answer["count"].as_u64().unwrap() > 0, "{answer}");
    assert!(answer.get("note").is_none() && answer.get("app_version").is_none());
}

const DEALS_PAGE: &str = r#"<!doctype html><html lang="en"><head><title>Deals</title></head><body>
<nav data-slot="navigation-menu"><a href="/deals">Deals</a><a href="/companies">Companies</a></nav>
<div data-slot="row"><h1 data-slot="typography">Deals</h1></div>
<div data-slot="card"><h2 data-slot="card-title">Sutlej Supply renewal</h2>
<a data-slot="button" href="/deals/42">Open</a></div>
<form data-slot="form" method="post" action="/deals">
<label for="title">Deal name</label><input data-slot="input" type="text" name="title" id="title" required>
<button data-slot="button" type="submit">Create deal</button></form>
</body></html>"#;

fn printed(path: &str, status: u64, content_type: Option<&str>, location: Option<&str>, html: Option<&str>) -> Value {
    json!({
        "path": path,
        "status": status,
        "content_type": content_type,
        "location": location,
        "html": html,
    })
}

#[tokio::test]
async fn a_page_is_drawn_by_the_app_and_answered_as_an_outline() {
    let fixture = Fixture::new("");
    fixture.built();
    fixture.answers(
        &["ui:render", "/deals", "--json"],
        printed("/deals", 200, Some("text/html; charset=utf-8"), None, Some(DEALS_PAGE)),
    );
    let page = call(&fixture.navigator(), "render_page", json!({"path": "/deals"})).await;

    assert_eq!(page["path"], "/deals");
    assert_eq!(page["status"], 200);
    assert_eq!(page["title"], "Deals");
    assert_eq!(
        page["headings"],
        json!([{"level": 1, "text": "Deals"}, {"level": 2, "text": "Sutlej Supply renewal"}])
    );
    assert_eq!(
        page["links"],
        json!([
            {"text": "Deals", "href": "/deals"},
            {"text": "Companies", "href": "/companies"},
            {"text": "Open", "href": "/deals/42"},
        ])
    );
    let form = &page["forms"][0];
    assert_eq!(form["action"], "/deals");
    assert_eq!(form["method"], "post");
    assert_eq!(
        form["fields"],
        json!([{"type": "text", "name": "title", "label": "Deal name", "required": true}])
    );
    assert_eq!(form["buttons"], json!(["Create deal"]));

    let components: Vec<(&str, u64)> = page["components"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| (c["id"].as_str().unwrap(), c["count"].as_u64().unwrap()))
        .collect();
    assert!(components.contains(&("button", 2)), "{components:?}");
    assert!(components.contains(&("card", 1)), "{components:?}");
    assert!(components.contains(&("input", 1)), "{components:?}");
    let card = page["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "card")
        .unwrap();
    assert_eq!(card["docs"], "components/card", "a slug for read_doc");
    assert!(
        page["other_slots"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s == "row"),
        "a layout slot is no catalog component"
    );
    assert_eq!(page["truncated"], false);
    assert!(page.get("redirect").is_none() && page.get("note").is_none());

    let render: Vec<_> = fixture
        .runner
        .calls()
        .into_iter()
        .filter(|c| c.args.first().map(String::as_str) == Some("ui:render"))
        .collect();
    assert_eq!(render.len(), 1);
    assert_eq!(render[0].args, ["ui:render", "/deals", "--json"]);
}

#[tokio::test]
async fn a_page_is_asked_for_every_time() {
    let fixture = Fixture::new("");
    fixture.built();
    fixture.answers(
        &["ui:render", "/deals", "--json"],
        printed("/deals", 200, Some("text/html"), None, Some(DEALS_PAGE)),
    );
    let navigator = fixture.navigator();
    call(&navigator, "render_page", json!({"path": "/deals"})).await;
    call(&navigator, "render_page", json!({"path": "/deals"})).await;
    let asked = fixture
        .runner
        .calls()
        .iter()
        .filter(|c| c.args.first().map(String::as_str) == Some("ui:render"))
        .count();
    assert_eq!(asked, 2, "the data behind a page changes, so the answer isn't kept");
}

#[tokio::test]
async fn a_query_string_goes_to_the_app_and_a_fragment_does_not() {
    let fixture = Fixture::new("");
    fixture.built();
    fixture.answers(
        &["ui:render", "/deals?page=2", "--json"],
        printed("/deals?page=2", 200, Some("text/html"), None, Some("<title>Page 2</title>")),
    );
    let page = call(
        &fixture.navigator(),
        "render_page",
        json!({"path": "/deals?page=2#top"}),
    )
    .await;
    assert_eq!(page["path"], "/deals?page=2");
    assert_eq!(page["title"], "Page 2");
}

#[tokio::test]
async fn a_redirect_is_answered_with_where_it_leads() {
    let fixture = Fixture::new("");
    fixture.built();
    fixture.answers(
        &["ui:render", "/deals", "--json"],
        printed("/deals", 302, None, Some("/sign-in?next=%2Fdeals"), None),
    );
    let answer = call(&fixture.navigator(), "render_page", json!({"path": "/deals"})).await;
    assert_eq!(answer["status"], 302);
    assert_eq!(answer["redirect"], "/sign-in?next=%2Fdeals");
    assert!(answer["note"].as_str().unwrap().contains("/sign-in"));
    assert!(answer.get("title").is_none(), "there is no page to outline");
}

#[tokio::test]
async fn an_answer_that_is_not_a_page_says_so() {
    let fixture = Fixture::new("");
    fixture.built();
    fixture.answers(
        &["ui:render", "/api/v1/deals", "--json"],
        printed("/api/v1/deals", 200, Some("application/json"), None, None),
    );
    let answer = call(
        &fixture.navigator(),
        "render_page",
        json!({"path": "/api/v1/deals"}),
    )
    .await;
    assert_eq!(answer["status"], 200);
    assert!(
        answer["note"].as_str().unwrap().contains("application/json"),
        "{answer}"
    );
}

#[tokio::test]
async fn an_error_page_is_outlined_with_its_status() {
    let fixture = Fixture::new("");
    fixture.built();
    fixture.answers(
        &["ui:render", "/nope", "--json"],
        printed(
            "/nope",
            404,
            Some("text/html"),
            None,
            Some("<title>Not found</title><h1>Page not found</h1><a href=\"/\">Go to home</a>"),
        ),
    );
    let answer = call(&fixture.navigator(), "render_page", json!({"path": "/nope"})).await;
    assert_eq!(answer["status"], 404);
    assert_eq!(answer["headings"][0]["text"], "Page not found");
    assert_eq!(answer["links"][0]["href"], "/");
}

#[tokio::test]
async fn an_app_without_the_render_command_is_told_what_to_do() {
    let fixture = Fixture::new("");
    fixture.built();
    fixture.runner.on(
        "crm",
        &["ui:render"],
        ProcessOutput {
            success: false,
            stdout: String::new(),
            stderr: "Error: unknown command `ui:render`; run `help` to list the available commands\n"
                .to_owned(),
        },
    );
    let message = refused(&fixture.navigator(), "render_page", json!({"path": "/"})).await;
    assert!(message.contains("no `ui:render` command"), "{message}");
    assert!(message.contains("UiModule"), "{message}");
    assert!(
        !message.contains("failed:"),
        "plain words, not the raw failure: {message}"
    );
}

#[tokio::test]
async fn another_failure_of_the_command_is_shown_as_it_was() {
    let fixture = Fixture::new("");
    fixture.built();
    fixture.runner.on(
        "crm",
        &["ui:render"],
        ProcessOutput {
            success: false,
            stdout: String::new(),
            stderr: "Error: cannot connect to the database".to_owned(),
        },
    );
    let message = refused(&fixture.navigator(), "render_page", json!({"path": "/"})).await;
    assert!(message.contains("cannot connect to the database"), "{message}");
}

#[tokio::test]
async fn output_that_is_not_a_rendered_page_is_refused() {
    let fixture = Fixture::new("");
    fixture.built();
    fixture.runner.on("crm", &["ui:render"], ok("{\"level\":\"ERROR\",\"status\":500}\nnot json\n"));
    let message = refused(&fixture.navigator(), "render_page", json!({"path": "/"})).await;
    assert!(message.contains("did not print the expected JSON"), "{message}");
}

#[tokio::test]
async fn only_a_path_of_the_app_is_asked_for() {
    let fixture = Fixture::new("");
    fixture.built();
    let navigator = fixture.navigator();
    for bad in ["deals", "//evil.example/deals", "http://127.0.0.1:9/deals", "/a b", ""] {
        let message = refused(&navigator, "render_page", json!({"path": bad})).await;
        assert!(message.contains("`path`"), "{bad:?}: {message}");
    }
    let message = refused(&navigator, "render_page", json!({})).await;
    assert!(message.contains("invalid arguments"), "{message}");
    assert!(
        fixture
            .runner
            .calls()
            .iter()
            .all(|c| c.args.first().map(String::as_str) != Some("ui:render")),
        "the app was never asked"
    );
}

#[tokio::test]
async fn a_build_error_and_no_build_reach_render_page() {
    let fixture = Fixture::new("");
    fixture.runner.on(
        "cargo",
        &["metadata"],
        ok(
            &json!({"packages": [{"name": "crm", "targets": [{"name": "crm", "kind": ["bin"]}]}], "target_directory": fixture.root().join("target")})
                .to_string(),
        ),
    );
    let no_build = fixture.navigator_with(NavigatorOptions {
        path: fixture.root().to_path_buf(),
        no_build: true,
        docs_offline: true,
        ..NavigatorOptions::default()
    });
    let message = refused(&no_build, "render_page", json!({"path": "/"})).await;
    assert!(message.contains("--no-build"), "{message}");

    let error = json!({"reason": "compiler-message", "message": {"level": "error", "rendered": "error[E0425]: cannot find value `deal`"}});
    fixture.runner.on(
        "cargo",
        &["build"],
        ProcessOutput {
            success: false,
            stdout: format!("{error}\n{}", json!({"reason": "build-finished", "success": false})),
            stderr: String::new(),
        },
    );
    let message = refused(&fixture.navigator(), "render_page", json!({"path": "/"})).await;
    assert!(message.contains("cannot find value `deal`"), "{message}");
}
