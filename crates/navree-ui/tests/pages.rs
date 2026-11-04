//! The error pages in a running app: which form each asker gets, what a page shows and
//! what it never shows.

use std::sync::Arc;

use http::{Method, StatusCode};
use navree_http::{AppError, Controller, HttpModule, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{
    BoxFuture, Factory, Injectable, KernelError, Module, ModuleBuilder, ProviderKey, Resolver,
};
use navree_testing::{TestApp, TestResponse, principal};
use navree_ui::{ErrorShell, NavGroup, NavItem, Shell, UiModule, View, el, icons};
use std::time::Duration;
use uuid::Uuid;

const BROWSER: &str = "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8";
const SECRET: &str =
    "hunter2 SELECT * FROM users WHERE token='abc' /home/ubuntu/app/src/db.rs:42:7";

struct Pages;

impl Injectable for Pages {
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

fn fail(
    error: impl Fn() -> AppError + Send + Sync + Clone + 'static,
) -> impl navree_http::Handler<()> {
    move || {
        let error = error.clone();
        async move { Err::<View, _>(error()) }
    }
}

impl Controller for Pages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let status = |code: u16, path: &'static str| {
            RouteDef::on(
                Method::GET,
                format!("!/status/{path}"),
                fail(move || {
                    AppError::new(StatusCode::from_u16(code).unwrap(), "custom", "Custom")
                }),
            )
            .public()
            .page()
        };
        vec![
            RouteDef::on(Method::GET, "!/deals", || async {
                View::new("Deals").body(el("main").child("The deals"))
            })
            .public()
            .page(),
            RouteDef::on(Method::GET, "!/mine", || async {
                View::new("Mine").body(el("main").child("Mine"))
            })
            .authenticated()
            .page(),
            RouteDef::on(Method::GET, "!/secret", fail(AppError::forbidden))
                .public()
                .page(),
            RouteDef::on(Method::GET, "!/guarded", fail(AppError::unauthenticated))
                .public()
                .page(),
            RouteDef::on(
                Method::GET,
                "!/conflict",
                fail(|| AppError::conflict("Email taken")),
            )
            .public()
            .page(),
            RouteDef::on(
                Method::GET,
                "!/limited",
                fail(|| AppError::too_many_requests(Duration::from_secs(30))),
            )
            .public()
            .page(),
            RouteDef::on(Method::GET, "!/expired", fail(AppError::page_expired))
                .public()
                .page(),
            RouteDef::on(
                Method::GET,
                "!/unavailable",
                fail(|| AppError::service_unavailable("Back at noon.")),
            )
            .public()
            .page(),
            RouteDef::on(Method::GET, "!/boom", fail(|| AppError::internal(SECRET)))
                .public()
                .page(),
            RouteDef::on(
                Method::GET,
                "!/boom-detail",
                fail(|| AppError::internal(SECRET).with_detail(SECRET)),
            )
            .public()
            .page(),
            RouteDef::on(Method::GET, "!/panic", || async {
                if std::hint::black_box(true) {
                    panic!("{SECRET}");
                }
                View::new("never")
            })
            .public()
            .page(),
            RouteDef::on(
                Method::GET,
                "!/hostile",
                fail(|| AppError::conflict("<img src=x onerror=alert(1)> & \"quotes\"")),
            )
            .public()
            .page(),
            RouteDef::on(Method::POST, "!/act", fail(AppError::forbidden))
                .public()
                .page(),
            RouteDef::on(
                Method::POST,
                "!/act-conflict",
                fail(|| AppError::conflict("Email taken")),
            )
            .public()
            .page(),
            RouteDef::on(Method::POST, "!/act-guest", fail(AppError::unauthenticated))
                .public()
                .page(),
            RouteDef::on(
                Method::POST,
                "!/act-boom",
                fail(|| AppError::internal(SECRET)),
            )
            .public()
            .page(),
            RouteDef::on(
                Method::POST,
                "!/act-limited",
                fail(|| AppError::too_many_requests(Duration::from_secs(9))),
            )
            .public()
            .page(),
            RouteDef::on(Method::GET, "!/data", fail(AppError::forbidden))
                .public()
                .json_only(),
            // Not marked, and under the API prefix: data.
            RouteDef::on(Method::GET, "/v1/items", fail(AppError::forbidden)).public(),
            // Not marked, not under the prefix.
            RouteDef::on(Method::GET, "!/unmarked", fail(AppError::forbidden)).public(),
            status(410, "gone"),
            status(422, "invalid"),
            status(418, "teapot"),
            status(502, "bad-gateway"),
        ]
    }
}

/// What the app's `ErrorShell` does.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Builder {
    /// The app has none.
    #[default]
    Absent,
    /// It builds a shell.
    Works,
    /// It panics when called.
    PanicsAtOnce,
    /// It returns a future that panics when polled.
    PanicsLater,
    /// Its future never finishes.
    Hangs,
}

#[derive(Default)]
struct App {
    shell: bool,
    builder: Builder,
}

impl Module for App {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<Pages>();
        let shell = || {
            Shell::new("Acme").group(
                NavGroup::plain().item(NavItem::new("Deals", "/deals").icon(icons::HANDSHAKE)),
            )
        };
        match self.builder {
            Builder::Absent => {}
            Builder::Works => {
                m.contribute(ErrorShell::new(move |_| async move { Some(shell()) }));
            }
            Builder::PanicsAtOnce => {
                m.contribute(ErrorShell::new(|_| -> std::future::Ready<Option<Shell>> {
                    panic!("the shell builder failed at once")
                }));
            }
            Builder::PanicsLater => {
                m.contribute(ErrorShell::new(|_| async move {
                    if std::hint::black_box(true) {
                        panic!("the shell builder failed later");
                    }
                    None::<Shell>
                }));
            }
            Builder::Hangs => {
                m.contribute(ErrorShell::new(|_| async move {
                    std::future::pending::<Option<Shell>>().await
                }));
            }
        }
        if self.shell {
            m.factory(Factory::<Shell>::new(|_| async {
                Ok(Arc::new(Shell::new("Acme").group(NavGroup::plain().item(
                    NavItem::new("Deals", "/deals").icon(icons::HANDSHAKE),
                ))))
            }))
            .exports::<Shell>();
        }
    }
}

async fn app_with(shell: bool, debug: bool) -> TestApp {
    TestApp::builder()
        .fake_auth()
        .config("ui.debug", debug)
        .module(HttpModule::for_root(Default::default()))
        .module(UiModule)
        .module(App {
            shell,
            ..App::default()
        })
        .build()
        .await
}

async fn app() -> TestApp {
    app_with(true, false).await
}

async fn get(app: &TestApp, path: &str, accept: &str) -> TestResponse {
    app.client().get(path).header("accept", accept).send().await
}

async fn get_as_user(app: &TestApp, path: &str) -> TestResponse {
    app.acting_as(principal(Uuid::from_u128(7)))
        .get(path)
        .header("accept", BROWSER)
        .send()
        .await
}

fn count(text: &str, needle: &str) -> usize {
    text.matches(needle).count()
}

#[tokio::test]
async fn every_status_on_a_page_route_is_a_page_with_that_status() {
    let app = app().await;
    let cases: &[(&str, u16, &str)] = &[
        ("/guarded", 401, "Sign in required"),
        ("/secret", 403, "No access"),
        ("/missing/page", 404, "Page not found"),
        ("/status/gone", 410, "Gone"),
        ("/expired", 419, "Page expired"),
        ("/status/invalid", 422, "Couldn&#39;t use that"),
        ("/conflict", 409, "Conflict"),
        ("/limited", 429, "Too many requests"),
        ("/boom", 500, "Something went wrong"),
        ("/unavailable", 503, "Unavailable"),
        ("/status/teapot", 418, "Request problem"),
        ("/status/bad-gateway", 502, "Something went wrong"),
    ];
    for (path, status, title) in cases {
        let response = get(&app, path, BROWSER).await;
        let text = response.text();
        assert_eq!(response.status().as_u16(), *status, "{path}: {text}");
        assert!(
            response
                .header("content-type")
                .unwrap_or("")
                .starts_with("text/html"),
            "{path}"
        );
        assert!(
            text.contains(&format!("<title>{title}</title>")),
            "{path}: {text}"
        );
        assert_eq!(count(&text, "<h1"), 1, "{path}: one h1");
        assert!(
            !text.contains("<script"),
            "{path}: a page of an error runs nothing"
        );
        let id = response.header("x-request-id").expect("a request id");
        assert!(text.contains(id), "{path}: the reference is the request id");
        assert_eq!(response.header("cache-control"), Some("no-store"), "{path}");
        assert!(
            response
                .header("content-security-policy")
                .unwrap_or("")
                .contains("script-src 'self'"),
            "{path}"
        );
        assert!(
            text.contains(&format!(r#"data-status="{status}""#)),
            "{path}: {text}"
        );
    }
}

#[tokio::test]
async fn a_method_the_page_does_not_take_is_a_page() {
    let app = app().await;
    let response = app
        .client()
        .post("/deals")
        .header("accept", BROWSER)
        .send()
        .await;
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert!(
        response.text().contains("Not allowed"),
        "{}",
        response.text()
    );
    assert!(response.header("allow").is_some(), "the Allow header stays");
}

#[tokio::test]
async fn a_signed_in_user_gets_the_error_inside_the_shell() {
    let app = app().await;
    for (path, status) in [
        ("/secret", 403),
        ("/nowhere", 404),
        ("/conflict", 409),
        ("/boom", 500),
    ] {
        let response = get_as_user(&app, path).await;
        let text = response.text();
        assert_eq!(response.status().as_u16(), status, "{path}");
        assert!(
            text.contains(r#"data-slot="sidebar""#),
            "{path}: in the shell: {text}"
        );
        assert!(text.contains(r#"href="/deals""#), "{path}: the app's links");
        assert_eq!(count(&text, "<h1"), 1, "{path}");
        assert!(text.contains(r#"<h1 data-slot="page-title">"#), "{path}");
        assert!(
            text.contains("data-ui-click"),
            "{path}: the shell page can copy the reference"
        );
    }
    // The ones that must work when much does not, and a guest, are on their own.
    for (path, user) in [
        ("/limited", true),
        ("/unavailable", true),
        ("/guarded", true),
        ("/secret", false),
    ] {
        let response = if user {
            get_as_user(&app, path).await
        } else {
            get(&app, path, BROWSER).await
        };
        let text = response.text();
        assert!(!text.contains(r#"data-slot="sidebar""#), "{path}: {text}");
        assert!(
            text.contains(r#"<main data-slot="state-page" id="main">"#),
            "{path}: {text}"
        );
    }
}

#[tokio::test]
async fn without_a_shell_the_page_stands_alone() {
    let app = app_with(false, false).await;
    let response = get_as_user(&app, "/secret").await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(response.text().contains(r#"data-slot="state-page""#));
    assert!(!response.text().contains("sidebar"));
}

#[tokio::test]
async fn the_api_and_clients_that_want_data_keep_the_json_error() {
    let app = app().await;
    // A route marked as data, whatever the client asks.
    let response = get(&app, "/data", BROWSER).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(response.header("content-type"), Some("application/json"));
    assert_eq!(response.json_value()["code"], "forbidden");
    // A page route asked for JSON alone.
    for accept in ["application/json", "application/json, text/plain, */*"] {
        let response = get(&app, "/secret", accept).await;
        assert_eq!(
            response.header("content-type"),
            Some("application/json"),
            "{accept}"
        );
        assert_eq!(response.json_value()["statusCode"], 403);
    }
    // An unmarked route under the prefix stays data for a browser too.
    let response = get(&app, "/api/v1/items", BROWSER).await;
    assert_eq!(response.header("content-type"), Some("application/json"));
    let response = get(&app, "/api/v1/nowhere", BROWSER).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(response.header("content-type"), Some("application/json"));
    // An unmarked route outside the prefix follows the client.
    assert_eq!(
        get(&app, "/unmarked", BROWSER)
            .await
            .header("content-type")
            .map(|v| v.starts_with("text/html")),
        Some(true)
    );
    for accept in ["application/json", "*/*"] {
        let response = get(&app, "/unmarked", accept).await;
        assert_eq!(
            response.header("content-type"),
            Some("application/json"),
            "{accept}"
        );
    }
    // A missing path: a page for a browser, data for a script.
    let response = get(&app, "/nowhere", "application/json").await;
    assert_eq!(response.header("content-type"), Some("application/json"));
    assert_eq!(response.json_value()["code"], "not_found");
    // Whatever was answered, a cache is told the answer depends on the asker.
    let varies = |response: &TestResponse| {
        response
            .headers()
            .get_all("vary")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(','))
            .any(|token| token.trim().eq_ignore_ascii_case("accept"))
    };
    assert!(varies(&get(&app, "/nowhere", "application/json").await));
    assert!(varies(&get(&app, "/nowhere", BROWSER).await));
}

#[tokio::test]
async fn a_head_request_gets_the_headers_of_the_page_and_no_body() {
    let app = app().await;
    let response = app
        .client()
        .request(Method::HEAD, "/secret")
        .header("accept", BROWSER)
        .send()
        .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(
        response
            .header("content-type")
            .unwrap_or("")
            .starts_with("text/html")
    );
}

#[tokio::test]
async fn nothing_inside_the_app_reaches_the_page_in_production() {
    let app = app().await;
    for path in ["/boom", "/boom-detail", "/panic"] {
        for user in [false, true] {
            let response = if user {
                get_as_user(&app, path).await
            } else {
                get(&app, path, BROWSER).await
            };
            assert_eq!(
                response.status(),
                StatusCode::INTERNAL_SERVER_ERROR,
                "{path}"
            );
            let text = response.text();
            let headers = format!("{:?}", response.headers());
            for secret in [
                "hunter2",
                "SELECT",
                "token=",
                "/home/ubuntu",
                "db.rs",
                "42:7",
                "panicked",
            ] {
                assert!(
                    !text.contains(secret),
                    "{path}: {secret} in the page: {text}"
                );
                assert!(!headers.contains(secret), "{path}: {secret} in the headers");
            }
            assert!(!text.contains("Development only"), "{path}");
            assert!(text.contains("Something went wrong on our side"), "{path}");
        }
    }
}

#[tokio::test]
async fn a_development_build_shows_more_and_says_so() {
    let app = app_with(true, true).await;
    let response = get(&app, "/boom", BROWSER).await;
    let text = response.text();
    assert!(
        text.contains(r#"<details data-slot="state-debug"><summary>Development only</summary>"#),
        "{text}"
    );
    assert!(text.contains("hunter2"), "{text}");
    assert!(text.contains("internal_error"), "{text}");
}

#[tokio::test]
async fn an_errors_own_words_are_shown_escaped() {
    let app = app().await;
    let text = get(&app, "/hostile", BROWSER).await.text();
    assert!(!text.contains("<img"), "{text}");
    assert!(
        text.contains("&lt;img src=x onerror=alert(1)&gt; &amp; &quot;quotes&quot;"),
        "{text}"
    );
    let text = get(&app, "/conflict", BROWSER).await.text();
    assert!(text.contains("Email taken"), "{text}");
    let text = get(&app, "/limited", BROWSER).await.text();
    assert!(text.contains("Try again in 30 seconds."), "{text}");
    let response = get(&app, "/limited", BROWSER).await;
    assert_eq!(response.header("retry-after"), Some("30"));
    let text = get(&app, "/unavailable", BROWSER).await.text();
    assert!(text.contains("Back at noon."), "{text}");
}

#[tokio::test]
async fn a_request_id_from_the_client_cannot_inject_markup() {
    let app = app().await;
    let response = app
        .client()
        .get("/secret")
        .header("accept", BROWSER)
        .header("x-request-id", "<script>alert(1)</script>")
        .send()
        .await;
    let text = response.text();
    assert!(!text.contains("<script>alert(1)"), "{text}");
}

#[tokio::test]
async fn a_guest_is_sent_to_sign_in_and_back() {
    let app = app().await;
    let text = get(&app, "/guarded?tab=notes", BROWSER).await.text();
    assert!(
        text.contains(r#"href="/sign-in?next=%2Fguarded%3Ftab%3Dnotes""#),
        "{text}"
    );
    // A guest on a page that needs a user.
    let text = get(&app, "/mine", BROWSER).await.text();
    assert!(text.contains(r#"href="/sign-in?next=%2Fmine""#), "{text}");
}

#[tokio::test]
async fn a_guest_can_be_redirected_instead() {
    let app = TestApp::builder()
        .fake_auth()
        .config("ui.sign_in_redirect", true)
        .config("ui.sign_in", "/login")
        .module(HttpModule::for_root(Default::default()))
        .module(UiModule)
        .module(App {
            shell: true,
            ..App::default()
        })
        .build()
        .await;
    let response = get(&app, "/mine?x=1", BROWSER).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        response.header("location"),
        Some("/login?next=%2Fmine%3Fx%3D1")
    );
    // An API client is not redirected.
    let response = get(&app, "/mine", "application/json").await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_sign_in_page_is_not_a_place_to_come_back_to() {
    let app = app().await;
    let response = app
        .client()
        .get("/guarded")
        .header("accept", BROWSER)
        .header("x-navree-ui", "1")
        .header("x-navree-from", "/sign-in?next=%2Fdeals")
        .send()
        .await;
    let text = response.text();
    assert!(text.contains(r#"href="/sign-in""#), "{text}");
    assert!(!text.contains("next="), "{text}");
}

#[tokio::test]
async fn a_hostile_return_address_is_dropped() {
    let app = app().await;
    for from in [
        "//evil.example",
        "/\\evil.example",
        "https://evil.example",
        "/%2f/evil.example",
        "javascript:alert(1)",
    ] {
        let response = app
            .client()
            .get("/guarded")
            .header("accept", BROWSER)
            .header("x-navree-ui", "1")
            .header("x-navree-region", "kpis")
            .header("x-navree-from", from)
            .send()
            .await;
        let text = response.text();
        assert!(text.contains(r#"href="/sign-in""#), "{from}: {text}");
        assert!(!text.contains("evil"), "{from}: {text}");
    }
}

#[tokio::test]
async fn a_region_gets_the_state_alone() {
    let app = app().await;
    let response = app
        .client()
        .get("/secret")
        .header("accept", "*/*")
        .header("x-navree-ui", "1")
        .header("x-navree-region", "kpis")
        .send()
        .await;
    let text = response.text();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(response.header("x-navree-state"), Some("403"));
    assert!(
        response
            .header("content-type")
            .unwrap_or("")
            .starts_with("text/html")
    );
    assert!(text.contains(r#"data-slot="state""#), "{text}");
    assert!(!text.contains("<script"), "{text}");
    assert!(
        !text.contains("data-build"),
        "a fragment, not a page of the runtime: {text}"
    );
    assert!(!text.contains("sidebar"), "{text}");
    assert!(!text.contains("<h1"), "{text}");
    assert!(
        !text.contains("data-ui-click"),
        "no copy button, no script: {text}"
    );
    assert!(
        !text.contains(r#" id=""#) && !text.contains("aria-labelledby"),
        "no id that a second region would repeat: {text}"
    );
}

#[tokio::test]
async fn a_boosted_navigation_gets_the_page() {
    let app = app().await;
    let response = app
        .acting_as(principal(Uuid::from_u128(7)))
        .get("/secret")
        .header("accept", "*/*")
        .header("x-navree-ui", "1")
        .send()
        .await;
    let text = response.text();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(
        text.contains("data-build"),
        "the runtime can morph it: {text}"
    );
    assert!(text.contains(r#"data-slot="sidebar""#), "{text}");
    assert!(response.header("x-navree-state").is_none());
}

#[tokio::test]
async fn an_action_gets_a_toast_and_the_page_stays() {
    let app = app().await;
    let post = |path: &'static str| {
        let client = app.client();
        async move {
            client
                .post(path)
                .header("accept", "*/*")
                .header("x-navree-ui", "1")
                .header("x-navree-from", "/deals?per=50")
                .send()
                .await
        }
    };
    let response = post("/act").await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        response.header("content-type"),
        Some("application/vnd.navree.ops+json")
    );
    let body = response.json_value();
    assert_eq!(body["ops"][0]["op"], "toast");
    assert_eq!(body["ops"][0]["tone"], "danger");
    assert_eq!(
        body["ops"][0]["text"],
        "You don't have permission to do that."
    );
    assert_eq!(body["ops"].as_array().unwrap().len(), 1, "no navigation");

    let body = post("/act-conflict").await.json_value();
    assert_eq!(body["ops"][0]["tone"], "warning");
    assert_eq!(body["ops"][0]["text"], "Email taken");

    let response = post("/act-limited").await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(
        response.json_value()["ops"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Try again in 9 seconds.")
    );

    let response = post("/act-boom").await;
    let text = response.text();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(text.contains("Reference"), "{text}");
    assert!(
        text.contains(response.header("x-request-id").unwrap()),
        "{text}"
    );
    for secret in ["hunter2", "SELECT", "/home/ubuntu"] {
        assert!(!text.contains(secret), "{secret}: {text}");
    }
}

#[tokio::test]
async fn a_guests_action_leads_to_sign_in_and_back() {
    let app = app().await;
    let response = app
        .client()
        .post("/act-guest")
        .header("accept", "*/*")
        .header("x-navree-ui", "1")
        .header("x-navree-from", "/deals?per=50")
        .send()
        .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let body = response.json_value();
    assert_eq!(body["ops"][0]["op"], "toast");
    assert_eq!(body["ops"][1]["op"], "navigate");
    assert_eq!(body["ops"][1]["href"], "/sign-in?next=%2Fdeals%3Fper%3D50");
    // And from a hostile place, nowhere in particular.
    let response = app
        .client()
        .post("/act-guest")
        .header("accept", "*/*")
        .header("x-navree-ui", "1")
        .header("x-navree-from", "//evil.example")
        .send()
        .await;
    let body = response.json_value();
    assert_eq!(body["ops"][1]["href"], "/sign-in");
}

#[tokio::test]
async fn a_form_posted_without_script_gets_a_page() {
    let app = app().await;
    let response = app
        .client()
        .post("/act")
        .header("accept", BROWSER)
        .send()
        .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(response.text().contains(r#"data-slot="state""#));
    assert!(
        response
            .header("content-type")
            .unwrap_or("")
            .starts_with("text/html")
    );
}

#[tokio::test]
async fn an_app_that_borrows_the_toolkit_gets_no_pages() {
    use navree_ui::UiOnDemand;
    let app = TestApp::builder()
        .module(HttpModule::for_root(Default::default()))
        .module(UiOnDemand)
        .module(App {
            shell: true,
            ..App::default()
        })
        .build()
        .await;
    let response = get(&app, "/secret", BROWSER).await;
    assert_eq!(response.header("content-type"), Some("application/json"));
}

#[tokio::test]
async fn the_words_can_be_the_apps_own() {
    use navree_ui::pages::KindWords;
    use navree_ui::{PagesOptions, StateKind, StateLabels};
    let labels = StateLabels::default().with_words(
        StateKind::NotFound,
        KindWords {
            short: "Introuvable".into(),
            title: "Page introuvable".into(),
            description: "Cette page n'existe pas.".into(),
            toast: "Introuvable".into(),
        },
    );
    let app = TestApp::builder()
        .module(HttpModule::for_root(Default::default()))
        .module(UiModule::with_pages(
            PagesOptions::default()
                .labels(labels)
                .home("/accueil")
                .debug(false),
        ))
        .module(App::default())
        .build()
        .await;
    let text = get(&app, "/nowhere", BROWSER).await.text();
    assert!(text.contains("<title>Introuvable</title>"), "{text}");
    assert!(text.contains("Page introuvable"), "{text}");
    assert!(text.contains(r#"href="/accueil""#), "{text}");
}

#[test]
fn a_region_can_say_what_to_show_when_it_gets_no_answer() {
    use navree_ui::{Region, State, StateKind, render};
    let plain = render(&Region::new("kpis").src("/f").lazy().into());
    assert!(
        !plain.contains("region-failed"),
        "nothing unless asked: {plain}"
    );

    let html = render(&Region::new("kpis").src("/f").lazy().when_offline().into());
    assert!(
        html.contains(r#"<div data-slot="region-failed" role="alert" data-when="offline">"#),
        "{html}"
    );
    assert!(html.contains("You&#39;re offline"), "{html}");
    assert!(
        html.contains("data-ui-refresh"),
        "Try again fetches the region: {html}"
    );
    assert!(
        !html.contains("data-status"),
        "no answer, no status: {html}"
    );
    // It comes after the content, so what the region had is read first.
    assert!(
        html.find("skeleton").unwrap() < html.find("region-failed").unwrap(),
        "{html}"
    );

    let mine = render(
        &Region::new("kpis")
            .src("/f")
            .children("Old numbers")
            .when_failed(State::of(StateKind::Unavailable))
            .into(),
    );
    // Not tied to being offline: any failure the server did not draw shows it.
    assert!(!mine.contains("data-when"), "{mine}");
    assert!(
        mine.contains("Old numbers") && mine.contains("We&#39;ll be right back"),
        "{mine}"
    );
    assert!(mine.find("Old numbers").unwrap() < mine.find("region-failed").unwrap());
}

// ---- Review fixes -----------------------------------------------------------------------

async fn configured(config: &[(&str, serde_json::Value)], shell: bool) -> TestApp {
    let mut builder = TestApp::builder().fake_auth();
    for (key, value) in config {
        builder = builder.config(key, value.clone());
    }
    builder
        .module(HttpModule::for_root(Default::default()))
        .module(UiModule)
        .module(App {
            shell,
            ..App::default()
        })
        .build()
        .await
}

fn links(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(at) = rest.find(" href=\"") {
        rest = &rest[at + 7..];
        let end = rest.find('"').unwrap_or(rest.len());
        out.push(rest[..end].to_owned());
        rest = &rest[end..];
    }
    out
}

#[tokio::test]
async fn production_is_final_the_details_never_show() {
    use serde_json::json;
    // A debug build, so the default would show them; the environment says production.
    for config in [
        vec![("env", json!("production"))],
        vec![("app.env", json!("production"))],
        vec![("env", json!(" Production "))],
        // Asked for in code or configuration: production still wins.
        vec![("env", json!("production")), ("ui.debug", json!(true))],
        vec![("app.env", json!("PRODUCTION")), ("ui.debug", json!(true))],
    ] {
        let app = configured(&config, true).await;
        let response = get(&app, "/boom", BROWSER).await;
        let text = response.text();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(
            !text.contains("Development only") && !text.contains("hunter2"),
            "{config:?}: {text}"
        );
    }
    // Any other environment keeps the rule: on when asked, and by default in a debug build.
    let app = configured(
        &[("env", json!("staging")), ("ui.debug", json!(true))],
        true,
    )
    .await;
    assert!(
        get(&app, "/boom", BROWSER)
            .await
            .text()
            .contains("Development only")
    );
    let app = configured(&[("env", json!("development"))], true).await;
    assert_eq!(
        get(&app, "/boom", BROWSER)
            .await
            .text()
            .contains("Development only"),
        cfg!(debug_assertions)
    );
    // Production wins over code, too.
    use navree_ui::PagesOptions;
    let app = TestApp::builder()
        .config("env", "production")
        .module(HttpModule::for_root(Default::default()))
        .module(UiModule::with_pages(PagesOptions::default().debug(true)))
        .module(App::default())
        .build()
        .await;
    assert!(!get(&app, "/boom", BROWSER).await.text().contains("hunter2"));
}

#[tokio::test]
async fn after_a_post_without_script_the_links_never_point_at_the_post() {
    let app = app().await;
    let post = |path: &'static str| {
        let client = app.client();
        async move { client.post(path).header("accept", BROWSER).send().await }
    };
    // Reload and Try again would be a GET of an address that only takes a POST.
    for (path, status) in [
        ("/act", 403),
        ("/act-conflict", 409),
        ("/act-boom", 500),
        ("/act-limited", 429),
    ] {
        let response = post(path).await;
        let text = response.text();
        assert_eq!(response.status().as_u16(), status, "{path}");
        let found = links(&text);
        assert!(
            !found
                .iter()
                .any(|href| href.contains("/act") || href.is_empty()),
            "{path}: {found:?}"
        );
        assert!(
            found.contains(&"/".to_owned()),
            "{path} offers Home: {found:?}"
        );
        let actions = text
            .split(r#"data-slot="state-actions""#)
            .nth(1)
            .and_then(|rest| rest.split("</div>").next())
            .unwrap_or_default();
        assert!(
            !actions.contains("Try again") && !actions.contains("Reload"),
            "{path}: {actions}"
        );
    }
    // A guest's sign-in has no way back to a post address.
    let text = post("/act-guest").await.text();
    let found = links(&text);
    assert!(found.contains(&"/sign-in".to_owned()), "{found:?}");
    assert!(!text.contains("next="), "{text}");
    // A GET keeps all of it.
    let text = get(&app, "/conflict", BROWSER).await.text();
    assert!(links(&text).contains(&"/conflict".to_owned()), "{text}");
    let text = get(&app, "/guarded?a=1", BROWSER).await.text();
    assert!(text.contains("next=%2Fguarded%3Fa%3D1"), "{text}");
}

#[tokio::test]
async fn a_failed_post_through_the_runtime_still_names_the_page_it_was_on() {
    let app = app().await;
    let response = app
        .client()
        .post("/act-guest")
        .header("accept", "*/*")
        .header("x-navree-ui", "1")
        .header("x-navree-from", "/deals?per=50")
        .send()
        .await;
    let body = response.json_value();
    assert_eq!(body["ops"][1]["href"], "/sign-in?next=%2Fdeals%3Fper%3D50");
}

#[tokio::test(start_paused = true)]
async fn a_shell_builder_that_fails_leaves_the_page_on_its_own_not_json() {
    for builder in [Builder::PanicsAtOnce, Builder::PanicsLater, Builder::Hangs] {
        let app = TestApp::builder()
            .fake_auth()
            .config("ui.debug", false)
            .module(HttpModule::for_root(Default::default()))
            .module(UiModule)
            .module(App {
                shell: true,
                builder,
            })
            .build()
            .await;
        let response = get_as_user(&app, "/secret").await;
        let text = response.text();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert!(
            response
                .header("content-type")
                .unwrap_or("")
                .starts_with("text/html"),
            "a page, not JSON"
        );
        assert!(
            text.contains(r#"data-slot="state-page""#),
            "on its own: {text}"
        );
        assert!(!text.contains(r#"data-slot="sidebar""#), "{text}");
    }
    // And one that works is used.
    let app = TestApp::builder()
        .fake_auth()
        .module(HttpModule::for_root(Default::default()))
        .module(UiModule)
        .module(App {
            shell: false,
            builder: Builder::Works,
        })
        .build()
        .await;
    let text = get_as_user(&app, "/secret").await.text();
    assert!(text.contains(r#"data-slot="sidebar""#), "{text}");
}

#[tokio::test]
async fn a_guest_is_not_redirected_in_a_circle_when_sign_in_itself_says_401() {
    let app = TestApp::builder()
        .fake_auth()
        .config("ui.sign_in_redirect", true)
        // The sign-in address is a page that needs a user: it answers a guest with a 401.
        .config("ui.sign_in", "/mine")
        .module(HttpModule::for_root(Default::default()))
        .module(UiModule)
        .module(App::default())
        .build()
        .await;
    let response = get(&app, "/mine?x=1", BROWSER).await;
    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "no 303 to itself"
    );
    assert!(response.text().contains(r#"data-slot="state""#));
    // Another page still redirects.
    let response = get(&app, "/guarded", BROWSER).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.header("location"), Some("/mine?next=%2Fguarded"));
}

#[tokio::test]
async fn an_error_page_on_its_own_carries_the_build_so_a_boosted_link_is_one_request() {
    let app = app().await;
    for path in ["/secret", "/guarded", "/boom", "/nowhere"] {
        let text = get(&app, path, BROWSER).await.text();
        assert!(
            text.contains(&format!(r#"data-build="{}""#, navree_ui::assets::BUILD)),
            "{path}: {text}"
        );
        assert!(!text.contains("<script"), "{path}");
    }
}

#[tokio::test]
async fn json_the_pages_left_alone_is_marked_for_caches() {
    let app = app().await;
    // A browser asks the API, and a script asks a page: both get JSON, and the same address
    // answers a page to the runtime, so a cache keeps them apart and keeps none.
    let response = get(&app, "/api/v1/nowhere", BROWSER).await;
    assert_eq!(response.header("content-type"), Some("application/json"));
    let varies: Vec<String> = response
        .headers()
        .get_all("vary")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(|v| v.trim().to_ascii_lowercase())
        .collect();
    assert!(
        varies.contains(&"accept".to_owned()) && varies.contains(&"x-navree-ui".to_owned()),
        "{varies:?}"
    );
    assert_eq!(response.header("cache-control"), Some("no-store"));
    let response = get(&app, "/secret", "application/json").await;
    assert_eq!(response.header("content-type"), Some("application/json"));
    assert_eq!(response.header("cache-control"), Some("no-store"));
}

#[tokio::test]
async fn the_runtime_header_wins_over_an_accept_that_asks_for_json() {
    // The runtime is the one asking, whatever it sends in Accept: it gets a page, and
    // a route marked `json_only()` is the exception.
    let app = app().await;
    let ask = |path: &'static str| {
        let client = app.client();
        async move {
            client
                .get(path)
                .header("accept", "application/json")
                .header("x-navree-ui", "1")
                .send()
                .await
        }
    };
    assert!(
        ask("/secret")
            .await
            .header("content-type")
            .unwrap_or("")
            .starts_with("text/html")
    );
    assert_eq!(
        ask("/data").await.header("content-type"),
        Some("application/json")
    );
}

#[tokio::test]
async fn an_error_raised_before_its_route_ran_follows_the_route() {
    // No token on a post to a json_only route: the CSRF check answers before the route does.
    // (The session-backed case is in apps/ui-proof/tests/pages_session.rs.) Here a route that
    // is json_only answers a 405 as data, and a page route as a page.
    let app = app().await;
    let response = app
        .client()
        .post("/data")
        .header("accept", BROWSER)
        .send()
        .await;
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(response.header("content-type"), Some("application/json"));
    let response = app
        .client()
        .request(Method::DELETE, "/deals")
        .header("accept", BROWSER)
        .send()
        .await;
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert!(
        response
            .header("content-type")
            .unwrap_or("")
            .starts_with("text/html")
    );
}
