//! A reply and a real session, in both module orders: signing in through the
//! runtime keeps the user, and the rotated id reaches the browser.

use std::sync::Arc;

use http::Method;
use navree::crypto::{CryptoModule, generate_key};
use navree::http::{Controller, HttpModule, RouteDef};
use navree::kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree::prelude::*;
use navree::session::{Session, SessionModule};
use navree::testing::TestApp;
use navree::ui::{Reply, UiModule};

async fn login(session: Session) -> Reply {
    let _ = session.insert("user", &"anu");
    session.regenerate();
    Reply::redirect("/home")
}

async fn logout(session: Session) -> Reply {
    session.invalidate();
    Reply::redirect("/home")
}

async fn whoami(session: Session) -> String {
    format!("{:?}", session.get::<String>("user"))
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
            RouteDef::on(Method::POST, "!/login", login).public(),
            RouteDef::on(Method::POST, "!/logout", logout).public(),
            RouteDef::on(Method::GET, "!/who", whoami).public(),
        ]
    }
}

struct App;

impl Module for App {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<Pages>();
    }
}

async fn app(ui_first: bool) -> TestApp {
    let builder = TestApp::builder()
        .config("app.key", generate_key())
        .config("http.per_ip_rate_limit", serde_json::Value::Null)
        .module(HttpModule::for_root(Default::default()))
        .module(CryptoModule);
    let builder = if ui_first {
        builder
            .module(UiModule)
            .module(SessionModule::for_root(Default::default()))
    } else {
        builder
            .module(SessionModule::for_root(Default::default()))
            .module(UiModule)
    };
    builder.module(App).build().await
}

#[tokio::test]
async fn signing_in_and_out_through_the_runtime_works_in_either_order() {
    for ui_first in [true, false] {
        let app = app(ui_first).await;
        let client = app.client();
        client.csrf_cookie().await;
        let token = client.cookie("XSRF-TOKEN");

        let response = client
            .post("/login")
            .xsrf()
            .header("x-navree-ui", "1")
            .send()
            .await;
        assert_eq!(response.status(), 200, "ui_first={ui_first}");
        assert_eq!(
            response.header("content-type"),
            Some("application/vnd.navree.ops+json"),
            "ui_first={ui_first}"
        );
        assert!(
            response.set_cookie("__Host-navree_session").is_some(),
            "the rotated id reaches the browser, ui_first={ui_first}"
        );
        assert_ne!(
            client.cookie("XSRF-TOKEN"),
            token,
            "a login rotates the token the runtime can see, ui_first={ui_first}"
        );
        assert_eq!(
            client.get("/who").send().await.text(),
            "Some(\"anu\")",
            "ui_first={ui_first}"
        );

        let response = client
            .post("/logout")
            .xsrf()
            .header("x-navree-ui", "1")
            .send()
            .await;
        assert_eq!(response.status(), 200, "ui_first={ui_first}");
        assert_eq!(
            client.get("/who").send().await.text(),
            "None",
            "ui_first={ui_first}"
        );
    }
}
