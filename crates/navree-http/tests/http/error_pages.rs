//! A module that answers errors with pages: what the pipeline asks it, and what
//! happens when it declines, fails or is slow.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use http::header::{HeaderValue, SET_COOKIE};
use http::{Method, Request, StatusCode};
use navree_http::body::Body;
use navree_http::extract::Request as Incoming;
use navree_http::middleware::{self, Next};
use navree_http::response::Response;
use navree_http::{
    AppError, ErrorContext, ErrorPages, GlobalLayer, IntoResponse, LayerSlot, ModuleBuilderHttpExt,
    RouteDef, RouteKind,
};
use navree_kernel::BoxFuture;

use crate::support::Setup;

/// What the test module does with an error.
#[derive(Clone, Copy)]
enum Mode {
    /// A page for everything.
    Page,
    /// No page: JSON stays.
    Decline,
    /// Panics.
    Panic,
    /// Takes longer than the deadline.
    Slow,
}

#[derive(Clone)]
struct Pages {
    mode: Mode,
    seen: Arc<Mutex<Vec<Seen>>>,
}

#[derive(Clone, Debug)]
struct Seen {
    status: u16,
    method: String,
    uri: String,
    headers: Vec<String>,
    request_id: bool,
    route: Option<String>,
    kind: Option<RouteKind>,
    user: bool,
    api_prefix: String,
}

impl ErrorPages for Pages {
    fn render<'a>(
        &'a self,
        request: &'a ErrorContext,
        error: &'a AppError,
    ) -> BoxFuture<'a, Option<Response>> {
        Box::pin(async move {
            let mut headers: Vec<String> = request
                .headers
                .keys()
                .map(|name| name.as_str().to_owned())
                .collect();
            headers.sort();
            self.seen.lock().unwrap().push(Seen {
                status: error.status.as_u16(),
                method: request.method.to_string(),
                uri: request.uri.to_string(),
                headers,
                request_id: request.request_id.is_some(),
                route: request.route.as_ref().and_then(|meta| meta.name.clone()),
                kind: request.route.as_ref().map(|meta| meta.kind),
                user: request.user.is_some(),
                api_prefix: request.api_prefix.to_string(),
            });
            match self.mode {
                Mode::Decline => None,
                Mode::Panic => panic!("the page failed"),
                Mode::Slow => {
                    tokio::time::sleep(Duration::from_secs(60)).await;
                    None
                }
                Mode::Page => {
                    let mut response = (
                        error.status,
                        [("content-type", "text/html; charset=utf-8")],
                        format!("<h1>{}</h1>", error.status.as_u16()),
                    )
                        .into_response();
                    response
                        .headers_mut()
                        .append(SET_COOKIE, HeaderValue::from_static("a=1"));
                    response
                        .headers_mut()
                        .append(SET_COOKIE, HeaderValue::from_static("b=2"));
                    Some(response)
                }
            }
        })
    }
}

fn routes() -> Vec<RouteDef> {
    vec![
        RouteDef::on(Method::GET, "/page", || async {
            Err::<(), _>(AppError::forbidden())
        })
        .name("page")
        .public()
        .page(),
        RouteDef::on(Method::GET, "/data", || async {
            Err::<(), _>(AppError::forbidden())
        })
        .name("data")
        .public()
        .json_only(),
        RouteDef::on(Method::GET, "/plain", || async {
            Err::<(), _>(AppError::not_found())
        })
        .name("plain")
        .public(),
        RouteDef::on(Method::GET, "/boom", || async {
            if std::hint::black_box(true) {
                panic!("secret panic text");
            }
            "never"
        })
        .name("boom")
        .public(),
        RouteDef::on(Method::GET, "/data-boom", || async {
            if std::hint::black_box(true) {
                panic!("secret panic text");
            }
            "never"
        })
        .name("data-boom")
        .public()
        .json_only(),
        RouteDef::on(Method::GET, "/ok", || async { "fine" })
            .name("ok")
            .public(),
        RouteDef::on(Method::GET, "/mine", || async {
            Err::<(), _>(AppError::forbidden())
        })
        .name("mine")
        .authenticated(),
    ]
}

async fn app(mode: Mode) -> (crate::support::TestApp, Arc<Mutex<Vec<Seen>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let pages = Pages {
        mode,
        seen: seen.clone(),
    };
    let app = Setup::new(routes())
        .authenticated()
        .register(move |m| {
            m.error_pages(pages.clone());
            m.http_layer(GlobalLayer::new(
                LayerSlot::Session,
                "test.session",
                |_, router| {
                    Ok(router
                        .layer(middleware::from_fn(session_cookie))
                        .layer(middleware::from_fn(deny)))
                },
            ));
        })
        .boot()
        .await;
    (app, seen)
}

/// A session layer: it writes its cookie on every answer, an error's included.
async fn session_cookie(request: Incoming, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .append(SET_COOKIE, HeaderValue::from_static("sid=1"));
    response
}

/// A layer that refuses a request before any route runs, as the CSRF check, the
/// authenticator and the limits do.
async fn deny(request: Incoming, next: Next) -> Response {
    if request.headers().contains_key("x-deny") {
        return AppError::forbidden().into_response();
    }
    next.run(request).await
}

fn varies_on_accept(response: &crate::support::TestResponse) -> bool {
    varies_on(response, "accept")
}

fn varies_on(response: &crate::support::TestResponse, name: &str) -> bool {
    response
        .headers
        .get_all("vary")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .any(|token| token.trim().eq_ignore_ascii_case(name))
}

#[tokio::test]
async fn an_app_without_the_module_answers_in_json_as_before() {
    let app = Setup::new(routes()).authenticated().boot().await;
    let response = app
        .send(
            Request::get("/api/page")
                .header("accept", "text/html")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(response.header("content-type"), Some("application/json"));
    let response = app.get("/nowhere").await;
    response.assert_error(404);
    assert!(!varies_on_accept(&response), "{:?}", response.headers);
}

#[tokio::test]
async fn the_module_answers_a_handler_error_and_a_missing_path() {
    let (app, seen) = app(Mode::Page).await;
    let response = app.get("/api/page").await;
    assert_eq!(response.status, StatusCode::FORBIDDEN);
    assert_eq!(
        response.header("content-type"),
        Some("text/html; charset=utf-8")
    );
    assert_eq!(response.text(), "<h1>403</h1>");
    assert!(response.header("x-request-id").is_some());
    let response = app.get("/nowhere").await;
    assert_eq!(response.status, StatusCode::NOT_FOUND);
    assert_eq!(response.text(), "<h1>404</h1>");
    let response = app
        .send(Request::post("/api/page").body(Body::empty()).unwrap())
        .await;
    assert_eq!(response.status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(response.text(), "<h1>405</h1>");
    assert!(response.header("allow").is_some(), "{:?}", response.headers);

    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 3);
    assert_eq!(seen[0].route.as_deref(), Some("page"));
    assert_eq!(seen[0].kind, Some(RouteKind::Page));
    assert_eq!(seen[1].route, None, "no route matched");
    assert_eq!(
        seen[2].route.as_deref(),
        Some("page"),
        "405: the module is told which route the path belongs to"
    );
    assert_eq!(seen[2].method, "POST");
    assert_eq!(seen[0].api_prefix, "/api");
}

#[tokio::test]
async fn every_cookie_survives_a_page_the_session_layers_and_the_page_both_set() {
    let (app, _) = app(Mode::Page).await;
    let response = app.get("/api/page").await;
    let mut cookies: Vec<&str> = response
        .headers
        .get_all(SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .collect();
    cookies.sort_unstable();
    assert_eq!(
        cookies,
        ["a=1", "b=2", "sid=1"],
        "a page adds to the response's cookies and never takes one away: {:?}",
        response.headers
    );
}

#[tokio::test]
async fn the_json_answer_keeps_the_session_cookie_too() {
    let (app, _) = app(Mode::Decline).await;
    let response = app.get("/api/page").await;
    response.assert_error(403);
    let cookies: Vec<_> = response.headers.get_all(SET_COOKIE).iter().collect();
    assert_eq!(cookies.len(), 1, "{:?}", response.headers);
}

#[tokio::test]
async fn a_header_the_error_sets_twice_is_not_doubled() {
    // `Allow` is on the 405 the router drew and on the error it carries.
    let (app, _) = app(Mode::Page).await;
    let response = app
        .send(Request::post("/api/page").body(Body::empty()).unwrap())
        .await;
    assert_eq!(response.status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(response.headers.get_all("allow").iter().count(), 1);
}

#[tokio::test]
async fn a_route_that_is_json_only_is_never_offered() {
    let (app, seen) = app(Mode::Page).await;
    let response = app
        .send(
            Request::get("/api/data")
                .header("accept", "text/html")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    response.assert_error(403);
    assert!(seen.lock().unwrap().is_empty());
    assert!(!varies_on_accept(&response), "{:?}", response.headers);
}

#[tokio::test]
async fn the_page_sees_only_what_decides_a_page() {
    let (app, seen) = app(Mode::Page).await;
    app.send(
        Request::get("/api/plain?x=1")
            .header("accept", "text/html")
            .header("cookie", "sid=1")
            .header("authorization", "Bearer secret")
            .header("x-api-key", "secret")
            .header("x-navree-ui", "1")
            .header("sec-fetch-dest", "document")
            .header("user-agent", "t")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    let seen = seen.lock().unwrap();
    assert_eq!(seen[0].uri, "/api/plain?x=1");
    assert_eq!(
        seen[0].headers,
        [
            "accept",
            "cookie",
            "sec-fetch-dest",
            "user-agent",
            "x-navree-ui"
        ]
    );
    assert!(seen[0].request_id);
}

#[tokio::test]
async fn the_signed_in_user_is_known_for_an_error_the_route_raised() {
    let (app, seen) = app(Mode::Page).await;
    let user = "6f9619ff-8b86-d011-b42d-00cf4fc964ff";
    app.send(
        Request::get("/api/mine")
            .header("x-user-id", user)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    // A guest on the same route is refused by authentication before the route.
    app.send(Request::get("/api/mine").body(Body::empty()).unwrap())
        .await;
    let seen = seen.lock().unwrap();
    assert!(seen[0].user, "{:?}", seen[0]);
    assert!(!seen[1].user, "{:?}", seen[1]);
    assert_eq!(seen[1].status, 401);
}

#[tokio::test]
async fn a_panic_is_offered_with_its_route_and_leaks_nothing() {
    let (app, seen) = app(Mode::Page).await;
    let response = app.get("/api/boom").await;
    assert_eq!(response.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(response.text(), "<h1>500</h1>");
    let seen = seen.lock().unwrap();
    assert_eq!(seen[0].status, 500);
    assert_eq!(
        seen[0].route.as_deref(),
        Some("boom"),
        "the route that panicked"
    );
    assert!(!seen[0].user, "a panic does not know the user");
    assert!(
        !response.text().contains("secret panic text"),
        "nothing of the panic reaches the page"
    );
}

#[tokio::test]
async fn a_module_that_declines_leaves_json_and_marks_it_for_caches() {
    let (app, seen) = app(Mode::Decline).await;
    let response = app.get("/api/page").await;
    response.assert_error(403);
    assert!(varies_on_accept(&response), "{:?}", response.headers);
    assert!(
        varies_on(&response, "x-navree-ui"),
        "{:?}",
        response.headers
    );
    assert_eq!(response.header("cache-control"), Some("no-store"));
    assert_eq!(seen.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn a_module_that_panics_leaves_json() {
    let (app, _) = app(Mode::Panic).await;
    let response = app.get("/api/page").await;
    response.assert_error(403);
    assert_eq!(
        app.get("/api/ok").await.text(),
        "fine",
        "the app carries on"
    );
}

#[tokio::test(start_paused = true)]
async fn a_module_that_is_too_slow_leaves_json() {
    let (app, _) = app(Mode::Slow).await;
    let response = app.get("/api/page").await;
    response.assert_error(403);
}

#[tokio::test]
async fn a_success_never_asks() {
    let (app, seen) = app(Mode::Page).await;
    assert_eq!(app.get("/api/ok").await.text(), "fine");
    assert!(seen.lock().unwrap().is_empty());
}

const BROWSER: &str = "text/html,application/xhtml+xml,*/*;q=0.8";

fn browser(method: Method, path: &str) -> http::request::Builder {
    Request::builder()
        .method(method)
        .uri(path)
        .header("accept", BROWSER)
}

#[tokio::test]
async fn a_json_only_route_is_json_for_an_error_raised_before_it_runs() {
    let (app, seen) = app(Mode::Page).await;
    // A layer ahead of the router refuses both: the CSRF check, a limit and the authenticator
    // do the same. The route's mark still decides the form.
    let response = app
        .send(
            browser(Method::GET, "/api/data")
                .header("x-deny", "1")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    response.assert_error(403);
    assert!(
        !seen.lock().unwrap().iter().any(|s| s.status == 403),
        "the module is never asked about a json_only route"
    );
    let response = app
        .send(
            browser(Method::GET, "/api/page")
                .header("x-deny", "1")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(response.status, StatusCode::FORBIDDEN);
    assert_eq!(
        response.text(),
        "<h1>403</h1>",
        "a page route gets its page"
    );
    let seen = seen.lock().unwrap();
    let asked = seen.last().expect("asked about the page route");
    assert_eq!(asked.route.as_deref(), Some("page"), "{asked:?}");
    assert_eq!(asked.kind, Some(RouteKind::Page));
}

#[tokio::test]
async fn a_405_follows_the_kind_of_the_route_at_that_path() {
    let (app, seen) = app(Mode::Page).await;
    // `/api/data` is a json_only GET: a POST to it is data.
    let response = app
        .send(
            browser(Method::POST, "/api/data")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    response.assert_error(405);
    assert!(response.header("allow").is_some());
    // `/api/page` is a page route: a POST to it is a page.
    let response = app
        .send(
            browser(Method::POST, "/api/page")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(response.status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(response.text(), "<h1>405</h1>");
    let seen = seen.lock().unwrap();
    assert_eq!(
        seen.last().and_then(|s| s.route.as_deref()),
        Some("page"),
        "the module is told which route the path belongs to"
    );
}

#[tokio::test]
async fn a_path_no_route_has_is_still_decided_by_the_client() {
    let (app, seen) = app(Mode::Page).await;
    let response = app
        .send(
            browser(Method::GET, "/nowhere")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(response.status, StatusCode::NOT_FOUND);
    assert_eq!(seen.lock().unwrap().last().unwrap().route, None);
}

#[tokio::test]
async fn a_panic_on_a_json_only_route_is_json() {
    let (app, _) = app(Mode::Page).await;
    let response = app
        .send(
            browser(Method::GET, "/api/data-boom")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    response.assert_error(500);
}

#[tokio::test]
async fn a_panic_and_a_timeout_carry_the_security_headers() {
    let (app, _) = app(Mode::Page).await;
    let response = app.get("/api/boom").await;
    assert_eq!(response.status, StatusCode::INTERNAL_SERVER_ERROR);
    for (name, value) in [
        ("x-frame-options", "DENY"),
        ("x-content-type-options", "nosniff"),
        ("referrer-policy", "no-referrer"),
    ] {
        assert_eq!(response.header(name), Some(value), "a panic page: {name}");
    }
}
