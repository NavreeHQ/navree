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
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, ProviderKey, Resolver};
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
    list.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())).collect()
}

async fn with_cookie() -> Response {
    let mut response = Reply::redirect("/home").toast(ToastTone::Success, "signed in").into_response();
    response.headers_mut().append(http::header::SET_COOKIE, HeaderValue::from_static("app_login=abc; Path=/; HttpOnly"));
    response.headers_mut().insert("x-custom", HeaderValue::from_static("kept"));
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
            RouteDef::on(Method::GET, "!/answer", || async { Reply::view(View::new("Page").body("page")).op(Op::Reload) }).public(),
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
        m.http_layer(GlobalLayer::new(LayerSlot::Session, "test.session", |_, router| Ok(router.layer(middleware::from_fn(session_like)))));
    }
}

async fn session_like(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response.headers_mut().append(http::header::SET_COOKIE, HeaderValue::from_static("sid=rotated; Path=/; HttpOnly"));
    response
}

fn builder() -> TestAppBuilder {
    TestApp::builder().config("http.per_ip_rate_limit", serde_json::Value::Null).module(HttpModule::for_root(Default::default()))
}

async fn app() -> TestApp {
    builder().module(UiModule).module(App).build().await
}

#[tokio::test]
async fn a_reply_to_the_runtime_keeps_the_headers_set_on_it() {
    let app = app().await;
    let response = app.client().post("/cookie").header("x-navree-ui", "1").send().await;
    assert_eq!(response.header("content-type"), Some("application/vnd.navree.ops+json"));
    assert_eq!(response.set_cookie("app_login").map(|cookie| cookie.value().to_owned()).as_deref(), Some("abc"));
    assert_eq!(response.header("x-custom"), Some("kept"));
    assert_eq!(response.header("location"), None, "a runtime answer is not a redirect");
    assert_eq!(response.header("cache-control"), Some("no-store"));
}

#[tokio::test]
async fn a_toast_for_the_runtime_leaves_no_flash_for_the_next_page() {
    let app = app().await;
    let response = app.client().post("/cookie").header("x-navree-ui", "1").send().await;
    assert!(response.set_cookie("navree_ui_flash").is_none(), "the toast op already shows it");
    let plain = app.client().post("/cookie").send().await;
    assert!(plain.set_cookie("navree_ui_flash").is_some());
}

#[tokio::test]
async fn a_cookie_from_a_session_layer_survives_in_either_module_order() {
    let ui_first = builder().module(UiModule).module(SessionLike).module(App).build().await;
    let session_first = builder().module(SessionLike).module(UiModule).module(App).build().await;
    for app in [ui_first, session_first] {
        let response = app.client().post("/cookie").header("x-navree-ui", "1").send().await;
        assert_eq!(response.set_cookie("sid").map(|cookie| cookie.value().to_owned()).as_deref(), Some("rotated"));
    }
}

#[tokio::test]
async fn a_reply_varies_on_the_runtime_header() {
    let app = app().await;
    for response in [app.client().get("/answer").send().await, app.client().get("/answer").header("x-navree-ui", "1").send().await] {
        let vary: Vec<&str> = response.headers().get_all("vary").iter().filter_map(|value| value.to_str().ok()).collect();
        assert!(vary.iter().any(|value| value.to_ascii_lowercase().contains("x-navree-ui")), "{vary:?}");
    }
}
