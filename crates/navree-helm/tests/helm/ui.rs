//! The server-rendered pages: lists, records, forms, actions, error pages,
//! assets and the security headers.

use navree_testing::{TestClient, TestResponse, principal};
use serde_json::json;
use uuid::Uuid;

use crate::support::*;

/// A browser navigation (asks for HTML).
async fn page(client: &TestClient, path: &str) -> TestResponse {
    client.get(path).header("accept", "text/html").send().await
}

/// A browser form post.
async fn submit(client: &TestClient, path: &str, form: &[(&str, &str)]) -> TestResponse {
    client
        .post(path)
        .header("accept", "text/html")
        .form(form)
        .send()
        .await
}

fn location(response: &TestResponse) -> String {
    response.header("location").unwrap_or_default().to_owned()
}

#[tokio::test]
async fn guests_are_sent_to_the_sign_in_page() {
    let app = app().await;
    let response = page(&app.client(), "/admin/resources/deals?page=2")
        .await
        .assert_status(303);
    // The next page is kept.
    assert_eq!(
        location(&response),
        "/admin/login?next=%2Fadmin%2Fresources%2Fdeals%3Fpage%3D2"
    );
    // API clients get the JSON error instead.
    app.client()
        .get("/admin/resources/deals")
        .send()
        .await
        .assert_error(401, None);
}

#[tokio::test]
async fn users_without_the_panel_get_an_error_page() {
    let app = app().await;
    let stranger = app.acting_as(principal(Uuid::now_v7()));
    let response = page(&stranger, "/admin").await.assert_status(403);
    assert!(
        response
            .header("content-type")
            .unwrap()
            .starts_with("text/html")
    );
    assert!(
        response.text().replace("&#x2f;", "/").contains("Forbidden"),
        "{}",
        response.text().replace("&#x2f;", "/")
    );
    let (_, rep) = user(&app, "rep").await;
    let response = page(&rep, "/admin/resources/invoices")
        .await
        .assert_status(404);
    assert!(
        response
            .text()
            .replace("&#x2f;", "/")
            .contains("No resource")
    );
    page(&rep, "/admin/nowhere").await.assert_status(404);
}

#[tokio::test]
async fn pages_are_strict_and_escape_values() {
    let app = app().await;
    let (_, rep) = user(&app, "rep").await;
    deal(
        &app,
        DealSeed {
            title: "<script>alert(1)</script>",
            ..DealSeed::default()
        },
    )
    .await;
    let response = page(&rep, "/admin/resources/deals")
        .await
        .assert_status(200);
    let csp = response
        .header("content-security-policy")
        .unwrap()
        .to_owned();
    assert!(csp.contains("default-src 'none'"), "{csp}");
    assert!(csp.contains("script-src 'self'"), "{csp}");
    assert!(!csp.contains("unsafe-inline"), "{csp}");
    assert!(csp.contains("frame-ancestors 'none'"), "{csp}");
    assert_eq!(response.header("x-frame-options"), Some("DENY"));
    assert_eq!(response.header("cache-control"), Some("no-store"));
    let html = response.text().replace("&#x2f;", "/");
    assert!(!html.contains("<script>alert(1)</script>"), "{html}");
    assert!(
        html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"),
        "{html}"
    );
    assert!(!html.contains("<script>"), "no inline scripts: {html}");
    assert!(!html.contains("style=\""), "no inline styles");
}

#[tokio::test]
async fn the_list_page_shows_rows_filters_actions_and_metrics() {
    let app = app().await;
    let (_, rep) = user(&app, "rep").await;
    let globex = company(&app, "Globex", None).await;
    deal(
        &app,
        DealSeed {
            title: "Globex renewal",
            amount_cents: 1_250_000,
            company: Some(globex.id),
            ..DealSeed::default()
        },
    )
    .await;
    let html = page(&rep, "/admin/resources/deals?sort=-amount_cents")
        .await
        .assert_status(200)
        .text()
        .replace("&#x2f;", "/");
    for expected in [
        "<h1>Deals</h1>",
        "Globex renewal",
        "$12,500.00",
        "aria-sort=\"descending\"",
        "name=\"filter[stage][in]\"",
        "Open deals only",
        "value=\"mark-won\"",
        "data-confirm=\"Mark the selected deals as won?\"",
        "Open pipeline",
        "New deal",
        "aria-current=\"page\"",
    ] {
        assert!(html.contains(expected), "missing {expected}");
    }
    // `fail` is only on records' pages.
    assert!(!html.contains("value=\"fail\""));
}

#[tokio::test]
async fn record_pages_show_fields_actions_and_relations() {
    let app = app().await;
    let (rep_id, rep) = user(&app, "rep").await;
    let globex = company(&app, "Globex", Some("Springfield")).await;
    let own = deal(
        &app,
        DealSeed {
            title: "Globex renewal",
            owner: Some(rep_id),
            company: Some(globex.id),
            ..DealSeed::default()
        },
    )
    .await;
    let html = page(&rep, &format!("/admin/resources/deals/{}", own.id))
        .await
        .assert_status(200)
        .text()
        .replace("&#x2f;", "/");
    assert!(html.contains("<h1>Globex renewal</h1>"));
    assert!(html.contains(&format!(
        "href=\"/admin/resources/companies/{}\"",
        globex.id
    )));
    assert!(
        html.contains("<strong>Secret</strong> notes"),
        "Markdown is rendered"
    );
    assert!(html.contains("/edit\">Edit</a>"));
    assert!(!html.contains("/delete\""), "reps may not delete");
    assert!(html.contains("value=\"fail\""));

    let html = page(&rep, &format!("/admin/resources/companies/{}", globex.id))
        .await
        .assert_status(200)
        .text()
        .replace("&#x2f;", "/");
    assert!(
        html.contains("Deals <span class=\"muted count\">1</span>"),
        "{html}"
    );
    assert!(html.contains("Globex renewal"));
    assert!(html.contains(&format!("via%5Bcompany_id%5D={}", globex.id)));
}

#[tokio::test]
async fn forms_create_and_update_records() {
    let app = app().await;
    let (rep_id, rep) = user(&app, "rep").await;
    let html = page(&rep, "/admin/resources/deals/new")
        .await
        .assert_status(200)
        .text()
        .replace("&#x2f;", "/");
    assert!(html.contains("name=\"title\""));
    assert!(html.contains("<span class=\"suffix\">USD</span>"));
    assert!(
        !html.contains("name=\"created_at\""),
        "read-only fields stay off create forms"
    );

    // A failed submit shows the errors under the fields, with the values.
    let html = submit(
        &rep,
        "/admin/resources/deals",
        &[("title", ""), ("amount_cents", "12.50")],
    )
    .await
    .assert_status(400)
    .text()
    .replace("&#x2f;", "/");
    assert!(html.contains("Title should not be empty"), "{html}");
    assert!(html.contains("value=\"12.50\""));

    let response = submit(
        &rep,
        "/admin/resources/deals",
        &[
            ("title", "Hooli pilot"),
            ("amount_cents", "1,250.75"),
            ("currency", "USD"),
            ("stage", "lead"),
            ("priority", "false"),
            ("priority", "true"),
            ("_tz_offset", "-120"),
        ],
    )
    .await
    .assert_status(303);
    let url = location(&response);
    assert!(url.starts_with("/admin/resources/deals/"), "{url}");
    assert!(
        response
            .header("set-cookie")
            .unwrap()
            .contains("navree_helm_flash")
    );
    let id: Uuid = url.rsplit('/').next().unwrap().parse().unwrap();
    let saved = reload(&app, id).await.unwrap();
    assert_eq!(saved.amount_cents, 125_075);
    assert!(saved.priority);
    assert_eq!(saved.owner_id, Some(rep_id));

    let html = page(&rep, &format!("{url}/edit"))
        .await
        .assert_status(200)
        .text()
        .replace("&#x2f;", "/");
    assert!(html.contains("value=\"1250.75\""), "money in major units");
    // Fields the rules keep are shown disabled.
    let (limited_id, limited) = user(&app, "limited").await;
    let theirs = deal(
        &app,
        DealSeed {
            owner: Some(limited_id),
            ..DealSeed::default()
        },
    )
    .await;
    let locked = page(
        &limited,
        &format!("/admin/resources/deals/{}/edit", theirs.id),
    )
    .await
    .assert_status(200)
    .text()
    .replace("&#x2f;", "/");
    assert!(
        locked.contains("You may not change this field."),
        "{locked}"
    );
    submit(
        &rep,
        &url,
        &[
            ("title", "Hooli pilot, phase 2"),
            ("amount_cents", "99"),
            ("priority", "false"),
        ],
    )
    .await
    .assert_status(303);
    let saved = reload(&app, id).await.unwrap();
    assert_eq!(saved.title, "Hooli pilot, phase 2");
    assert_eq!(saved.amount_cents, 9900);
    assert!(!saved.priority);

    // The edit form of someone else's deal is refused.
    let other = deal(
        &app,
        DealSeed {
            owner: Some(Uuid::now_v7()),
            ..DealSeed::default()
        },
    )
    .await;
    page(&rep, &format!("/admin/resources/deals/{}/edit", other.id))
        .await
        .assert_status(403);
}

#[tokio::test]
async fn actions_confirm_then_run() {
    let app = app().await;
    let (rep_id, rep) = user(&app, "rep").await;
    let own = deal(
        &app,
        DealSeed {
            owner: Some(rep_id),
            ..DealSeed::default()
        },
    )
    .await;
    let id = own.id.to_string();
    let html = submit(
        &rep,
        "/admin/resources/deals/actions",
        &[("action", "mark-won"), ("ids", &id)],
    )
    .await
    .assert_status(200)
    .text()
    .replace("&#x2f;", "/");
    assert!(html.contains("Mark the selected deals as won?"));
    assert_eq!(reload(&app, own.id).await.unwrap().stage, "lead");

    let response = submit(
        &rep,
        "/admin/resources/deals/actions",
        &[
            ("action", "mark-won"),
            ("ids", &id),
            ("_confirmed", "1"),
            ("_return", "/admin/resources/deals"),
        ],
    )
    .await
    .assert_status(303);
    assert_eq!(location(&response), "/admin/resources/deals");
    assert_eq!(reload(&app, own.id).await.unwrap().stage, "won");
    let flash = response.set_cookie("navree_helm_flash").unwrap();
    assert!(flash.value().contains("deals"), "{flash}");

    // An action with fields shows its form, then validates it.
    let html = submit(
        &rep,
        "/admin/resources/deals/actions",
        &[("action", "mark-lost"), ("ids", &id)],
    )
    .await
    .assert_status(200)
    .text()
    .replace("&#x2f;", "/");
    assert!(html.contains("name=\"lost_reason\""));
    let html = submit(
        &rep,
        "/admin/resources/deals/actions",
        &[
            ("action", "mark-lost"),
            ("ids", &id),
            ("_step", "run"),
            ("lost_reason", ""),
        ],
    )
    .await
    .assert_status(400)
    .text()
    .replace("&#x2f;", "/");
    assert!(html.contains("Reason should not be empty"));
    submit(
        &rep,
        "/admin/resources/deals/actions",
        &[
            ("action", "mark-lost"),
            ("ids", &id),
            ("_step", "run"),
            ("lost_reason", "Budget"),
            ("_return", "https://evil.example"),
        ],
    )
    .await
    .assert_status(303);
    assert_eq!(
        reload(&app, own.id).await.unwrap().lost_reason.as_deref(),
        Some("Budget")
    );
}

#[tokio::test]
async fn deleting_from_a_page_asks_first() {
    let app = app().await;
    let (_, manager) = user(&app, "manager").await;
    let deal = deal(
        &app,
        DealSeed {
            title: "Doomed",
            ..DealSeed::default()
        },
    )
    .await;
    let path = format!("/admin/resources/deals/{}/delete", deal.id);
    let html = submit(&manager, &path, &[])
        .await
        .assert_status(200)
        .text()
        .replace("&#x2f;", "/");
    assert!(html.contains("Delete Doomed?"));
    assert!(reload(&app, deal.id).await.is_some());
    let response = submit(&manager, &path, &[("_confirmed", "1")])
        .await
        .assert_status(303);
    assert_eq!(location(&response), "/admin/resources/deals");
    assert!(reload(&app, deal.id).await.is_none());
}

#[tokio::test]
async fn the_dashboard_shows_metrics_or_a_hint() {
    let app = app().await;
    let (_, rep) = user(&app, "rep").await;
    deal(
        &app,
        DealSeed {
            amount_cents: 1000,
            ..DealSeed::default()
        },
    )
    .await;
    let html = page(&rep, "/admin")
        .await
        .assert_status(200)
        .text()
        .replace("&#x2f;", "/");
    assert!(html.contains("Open pipeline"));
    assert!(html.contains("$10.00"));
    assert!(html.contains("Companies") && html.contains("Deals"), "menu");

    let empty = crate::servers::database(navree_testing::TestApp::builder())
        .module(navree_db::DatabaseModule)
        .module(navree_access::AccessModule::for_root(options()))
        .module(navree_helm::HelmModule::from_config())
        .build()
        .await;
    let id = Uuid::now_v7();
    give_role(&empty, id, "rep", &["helm.view"]).await;
    let html = page(&empty.acting_as(principal(id)), "/admin")
        .await
        .assert_status(200)
        .text()
        .replace("&#x2f;", "/");
    assert!(html.contains("navree make:resource deals Deal"));
}

#[tokio::test]
async fn assets_are_public_and_cached_for_good() {
    let app = builder()
        .config("helm.accent", "#ff8800")
        .config("helm.title", "Acme CRM")
        .build()
        .await;
    let response = app
        .client()
        .get("/admin/assets/helm.css")
        .send()
        .await
        .assert_status(200);
    assert_eq!(
        response.header("content-type"),
        Some("text/css; charset=utf-8")
    );
    assert_eq!(
        response.header("cache-control"),
        Some("public, max-age=31536000, immutable")
    );
    let theme = app
        .client()
        .get("/admin/assets/theme.css")
        .send()
        .await
        .text()
        .replace("&#x2f;", "/");
    assert!(theme.contains("--accent:#ff8800"), "{theme}");
    app.client()
        .get("/admin/assets/helm.js")
        .send()
        .await
        .assert_status(200);
    app.client()
        .get("/admin/assets/secrets.txt")
        .send()
        .await
        .assert_status(404);
    let (_, rep) = user(&app, "rep").await;
    let html = page(&rep, "/admin").await.text().replace("&#x2f;", "/");
    assert!(html.contains("<title>Dashboard · Acme CRM</title>"));
}

#[tokio::test]
async fn the_panel_moves_with_helm_path() {
    let app = builder().config("helm.path", "/back-office").build().await;
    let (_, rep) = user(&app, "rep").await;
    rep.get("/back-office/api/resources")
        .send()
        .await
        .assert_status(200);
    rep.get("/admin/api/resources")
        .send()
        .await
        .assert_status(404);
    let html = page(&rep, "/back-office")
        .await
        .assert_status(200)
        .text()
        .replace("&#x2f;", "/");
    assert!(html.contains("href=\"/back-office/resources/deals\""));
    let _ = json!(null);
}

#[tokio::test]
async fn the_shell_shows_icons_and_remembers_a_collapsed_sidebar() {
    let app = app().await;
    let (_, rep) = user(&app, "rep").await;
    let html = page(&rep, "/admin/resources/deals")
        .await
        .assert_status(200)
        .text()
        .replace("&#x2f;", "/");
    // A Lucide icon per resource (`handshake` for deals), the current one
    // marked; no letter tiles.
    assert!(
        html.contains(
            "<a class=\"menu-item\" href=\"/admin/resources/deals\" title=\"Deals\" aria-current=\"page\"><svg class=\"icon\""
        ),
        "{html}"
    );
    assert!(html.contains("<path d=\"m11 17 2 2a1 1 0 1 0 3-3\"/>"));
    assert!(html.contains("<span class=\"menu-label\">Companies</span>"));
    // The account menu, the top bar's trigger and the command menu.
    assert!(html.contains("<span class=\"user-icon\" aria-hidden=\"true\"><svg"));
    assert!(!html.contains("class=\"avatar\""));
    assert!(html.contains("data-sidebar-trigger"));
    assert!(html.contains("<dialog class=\"command\""));
    assert!(
        html.contains("<div class=\"shell\" data-shell>"),
        "expanded by default"
    );
    // The script remembers a collapsed sidebar in a cookie.
    let html = rep
        .get("/admin")
        .header("accept", "text/html")
        .header("cookie", "navree_helm_sidebar=collapsed")
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(html.contains("<div class=\"shell is-collapsed\" data-shell>"));
}
