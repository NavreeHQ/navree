//! The fixture app the browser tests drive, and the skip rule: without
//! Chrome or Chromium the tests print a notice and pass, unless
//! `NAVREE_TEST_REQUIRE_SERVICES=1` makes them fail.

#![allow(dead_code)]

use std::sync::Arc;
use std::time::Duration;

use http::header::{COOKIE, SET_COOKIE};
use http::{HeaderMap, Method};
use navree_browser::{Browser, TestServer};
use navree_http::response::{Html, IntoResponse, Redirect};
use navree_http::{Controller, Form, Json, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_testing::TestApp;
use serde::Deserialize;
use serde_json::{Value, json};

pub const PLAYGROUND: &str = include_str!("pages/playground.html");

const LOGIN: &str = r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>Sign in · Fixture</title></head>
<body>
  <h1>Sign in</h1>
  <p id="error"></p>
  <form method="post" action="/login">
    <label for="email">Email</label><input id="email" name="email" type="email">
    <label>Password <input name="password" type="password"></label>
    <button type="submit">Sign in</button>
  </form>
  <script>
    if (location.search.includes("error")) {
      document.getElementById("error").textContent = "That email and password don't match an account.";
    }
  </script>
</body></html>"#;

const DASHBOARD: &str = r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>Dashboard · Fixture</title></head>
<body>
  <nav><a href="/playground">Playground</a> <a href="/second">Second</a></nav>
  <h1>Dashboard</h1>
  <h2>Pipeline by stage</h2>
</body></html>"#;

const SECOND: &str = r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>Second page</title></head>
<body><h1>Second page</h1><a href="/playground">Back to the playground</a></body></html>"#;

const COUNTER: &str = r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>Counter</title></head>
<body><p id="count"></p>
<script>
  const n = Number(sessionStorage.getItem("loads") || "0") + 1;
  sessionStorage.setItem("loads", String(n));
  document.getElementById("count").textContent = `Loaded ${n} times`;
</script></body></html>"#;

const MOBILE: &str = r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Responsive</title>
<style>
  #menu-button { display: none; }
  @media (max-width: 600px) { nav { display: none; } #menu-button { display: inline-block; } }
  nav.open { display: block; }
</style></head>
<body>
  <button id="menu-button" type="button" onclick="document.querySelector('nav').classList.add('open')">Open menu</button>
  <nav><a href="/companies">Companies</a> <a href="/deals">Deals</a></nav>
  <p id="device"></p>
  <script>
    const show = () => document.getElementById("device").textContent =
      `width ${innerWidth} ratio ${devicePixelRatio} touch ${navigator.maxTouchPoints > 0} agent ${navigator.userAgent}`;
    show();
    addEventListener("resize", show);
  </script>
</body></html>"#;

const BROKEN: &str = r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>Broken</title></head>
<body><p>Half a page</p>
<script>
  console.log("starting up");
  fetch("/missing.json");
  console.error("the widget failed");
  setTimeout(() => { throw new Error("kaboom"); }, 0);
</script></body></html>"#;

const TALL: &str = r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>Tall</title></head>
<body style="margin:0"><div style="height:3000px;background:linear-gradient(red,blue)">Top</div><p>Bottom</p></body></html>"#;

#[derive(Deserialize)]
struct LoginForm {
    email: String,
    password: String,
}

/// Which fixture app answered (`GET /api/label`).
pub struct Label(pub String);

/// The fixture's pages and API, at absolute paths.
struct FixtureController {
    label: Arc<Label>,
}

impl Injectable for FixtureController {
    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(FixtureController {
                label: r.get::<Label>().await?,
            })
        })
    }
}

fn page(path: &'static str, html: &'static str) -> RouteDef {
    RouteDef::on(Method::GET, path, move || async move { Html(html) }).public()
}

impl Controller for FixtureController {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let label = self.label.0.clone();
        vec![
            RouteDef::on(Method::GET, "!/api/label", move || async move {
                Json(json!({ "label": label }))
            })
            .public(),
            page("!/login", LOGIN),
            page("!/playground", PLAYGROUND),
            page("!/second", SECOND),
            page("!/counter", COUNTER),
            page("!/mobile", MOBILE),
            page("!/broken", BROKEN),
            page("!/tall", TALL),
            RouteDef::on(Method::GET, "!/", |headers: HeaderMap| async move {
                let signed_in = headers
                    .get_all(COOKIE)
                    .iter()
                    .filter_map(|v| v.to_str().ok())
                    .any(|v| v.contains("navree_at="));
                if signed_in {
                    Html(DASHBOARD).into_response()
                } else {
                    Redirect::to("/login").into_response()
                }
            })
            .public(),
            RouteDef::on(
                Method::POST,
                "!/login",
                |Form(form): Form<LoginForm>| async move {
                    if form.email == "rep1@navree.test" && form.password == "password123" {
                        (
                            [(
                                SET_COOKIE,
                                "navree_at=token-rep1; HttpOnly; Path=/; SameSite=Lax",
                            )],
                            Redirect::to("/"),
                        )
                            .into_response()
                    } else {
                        Redirect::to("/login?error=1").into_response()
                    }
                },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "!/api/deals/{id}/move",
                |navree_http::Path(id): navree_http::Path<String>| async move {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    Json(json!({ "moved": id }))
                },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "!/api/whoami",
                |headers: HeaderMap| async move {
                    let cookie = headers
                        .get(COOKIE)
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or_default()
                        .to_owned();
                    Json(json!({ "cookie": cookie }))
                },
            )
            .public(),
        ]
    }
}

struct FixtureModule {
    label: String,
}

impl Module for FixtureModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.value(Label(self.label.clone()));
        m.controller::<FixtureController>();
    }
}

/// The fixture app, booted for one test.
pub async fn app() -> TestApp {
    labeled_app("fixture").await
}

/// The fixture app answering `GET /api/label` with `label`.
pub async fn labeled_app(label: &str) -> TestApp {
    TestApp::builder()
        .config("http.security_headers", false)
        .config("http.per_ip_rate_limit", Value::Null)
        .module(FixtureModule {
            label: label.to_owned(),
        })
        .build()
        .await
}

/// The fixture app on a real port, and the app (kept alive with it).
pub struct Fixture {
    pub server: TestServer,
    pub app: TestApp,
}

impl Fixture {
    pub async fn start() -> Fixture {
        let app = app().await;
        let server = TestServer::start(app.ctx())
            .await
            .expect("the fixture serves");
        Fixture { server, app }
    }

    pub fn url(&self, path: &str) -> String {
        self.server.url(path)
    }
}

/// Whether browser tests can run here; prints the skip notice when not.
pub fn has_chrome(test: &str) -> bool {
    match Browser::find_chrome() {
        Ok(_) => true,
        Err(error) => {
            if std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() == Ok("1") {
                panic!("{test}: {error} (NAVREE_TEST_REQUIRE_SERVICES=1)");
            }
            eprintln!("skipped {test}: {error}");
            false
        }
    }
}

/// A browser and the fixture, or `None` (after a notice) without Chrome.
pub async fn start(test: &str) -> Option<(Browser, Fixture)> {
    if !has_chrome(test) {
        return None;
    }
    let fixture = Fixture::start().await;
    let browser = Browser::launch().await.expect("the browser starts");
    Some((browser, fixture))
}

/// `let (browser, site) = skip_without_chrome!("name");`
#[macro_export]
macro_rules! browser_or_skip {
    ($name:expr) => {
        match $crate::support::start($name).await {
            Some(pair) => pair,
            None => return,
        }
    };
}
