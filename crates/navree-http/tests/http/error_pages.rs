//! A module that answers errors with pages: what the pipeline asks it, and what
//! happens when it declines, fails or is slow.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use http::header::{HeaderValue, SET_COOKIE};
use http::{Method, Request, StatusCode};
use navree_http::body::Body;
use navree_http::response::Response;
use navree_http::{
    AppError, ErrorContext, ErrorPages, IntoResponse, ModuleBuilderHttpExt, RouteDef, RouteKind,
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
        })
        .boot()
        .await;
    (app, seen)
}

fn varies_on_accept(response: &crate::support::TestResponse) -> bool {
    response
        .headers
        .get_all("vary")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .any(|token| token.trim().eq_ignore_ascii_case("accept"))
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
    assert_eq!(seen[2].route, None, "405: no route handled it");
    assert_eq!(seen[2].method, "POST");
    assert_eq!(seen[0].api_prefix, "/api");
}

#[tokio::test]
async fn every_value_of_a_header_the_page_sets_arrives() {
    let (app, _) = app(Mode::Page).await;
    let response = app.get("/api/page").await;
    let cookies: Vec<_> = response.headers.get_all(SET_COOKIE).iter().collect();
    assert_eq!(cookies.len(), 2, "{:?}", response.headers);
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
        ["accept", "cookie", "sec-fetch-dest", "x-navree-ui"]
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
async fn a_panic_is_offered_without_a_route_and_leaks_nothing() {
    let (app, seen) = app(Mode::Page).await;
    let response = app.get("/api/boom").await;
    assert_eq!(response.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(response.text(), "<h1>500</h1>");
    let seen = seen.lock().unwrap();
    assert_eq!(seen[0].status, 500);
    assert_eq!(seen[0].route, None);
    assert!(!seen[0].user);
}

#[tokio::test]
async fn a_module_that_declines_leaves_json_and_marks_it_for_caches() {
    let (app, seen) = app(Mode::Decline).await;
    let response = app.get("/api/page").await;
    response.assert_error(403);
    assert!(varies_on_accept(&response), "{:?}", response.headers);
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
