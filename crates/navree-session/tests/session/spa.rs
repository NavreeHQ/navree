//! The single-page-app flow at the session level: an authenticator that
//! reads the session key `auth.user_id` (which the test's login writes),
//! login with session regeneration, authenticated requests and logout.

use std::sync::Arc;

use http::Method;
use http::request::Parts;
use navree_contracts::{AuthVia, BoxFuture, Principal};
use navree_http::{
    AppError, Authenticator, Controller, CurrentPrincipal, Json, ModuleBuilderHttpExt, NoContent,
    RouteDef,
};
use navree_kernel::{Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_session::{Session, SessionModule, SessionOptions};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::{self, COOKIE, XSRF};

/// Authenticates from the session key `auth.user_id`.
struct SessionAuthenticator;

impl Authenticator for SessionAuthenticator {
    fn authenticate<'a>(
        &'a self,
        parts: &'a mut Parts,
    ) -> BoxFuture<'a, Result<Option<Principal>, AppError>> {
        let principal = parts.extensions.get::<Session>().and_then(|session| {
            let user_id = session.get::<Uuid>("auth.user_id")?;
            Some(Principal {
                user_id,
                via: AuthVia::Session,
                token_abilities: None,
                session_id: Some(session.id()),
            })
        });
        Box::pin(async move { Ok(principal) })
    }
}

struct AuthController;

impl Injectable for AuthController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(AuthController) })
    }
}

async fn login(session: Session, Json(body): Json<Value>) -> navree_http::Result<Json<Value>> {
    let user_id: Uuid = serde_json::from_value(body["user_id"].clone())?;
    session.regenerate();
    session.insert("auth.user_id", &user_id)?;
    Ok(Json(json!({ "user_id": user_id })))
}

async fn me(CurrentPrincipal(me): CurrentPrincipal, session: Session) -> Json<Value> {
    Json(json!({
        "user_id": me.user_id,
        "same_session": me.session_id.as_deref() == Some(session.id().as_str()),
    }))
}

async fn logout(session: Session) -> NoContent {
    session.invalidate();
    NoContent
}

impl Controller for AuthController {
    fn prefix(&self) -> &'static str {
        "/auth"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::POST, "/login", login).public(),
            RouteDef::on(Method::GET, "/user", me).authenticated(),
            RouteDef::on(Method::POST, "/logout", logout).authenticated(),
        ]
    }
}

struct AuthModule;

impl Module for AuthModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.value_arc::<dyn Authenticator>(Arc::new(SessionAuthenticator))
            .controller::<AuthController>();
    }
}

#[tokio::test]
async fn spa_flow_through_an_authenticator_reading_the_session() {
    let app = support::base()
        .module(SessionModule::for_root(SessionOptions::default()))
        .module(AuthModule)
        .build()
        .await;
    let client = app.client();
    let user = Uuid::now_v7();

    client.csrf_cookie().await;
    let guest_cookie = client.cookie(COOKIE).unwrap();
    client
        .post("/api/auth/login")
        .json(&json!({ "user_id": user }))
        .send()
        .await
        .assert_error(403, "Invalid CSRF token");
    client
        .post("/api/auth/login")
        .xsrf()
        .json(&json!({ "user_id": user }))
        .send()
        .await
        .assert_status(200);
    assert_ne!(
        client.cookie(COOKIE).unwrap(),
        guest_cookie,
        "login rotates the session"
    );

    client
        .get("/api/auth/user")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("user_id", json!(user))
        .assert_json_path("same_session", json!(true));

    // A cross-site form post carries the cookie but cannot read XSRF-TOKEN.
    client
        .post("/api/auth/logout")
        .send()
        .await
        .assert_error(403, "Invalid CSRF token");
    client.get("/api/auth/user").send().await.assert_status(200);

    let token_before = client.cookie(XSRF).unwrap();
    client
        .post("/api/auth/logout")
        .xsrf()
        .send()
        .await
        .assert_status(204);
    client
        .get("/api/auth/user")
        .send()
        .await
        .assert_error(401, "Unauthorized");
    assert_ne!(client.cookie(XSRF).unwrap(), token_before);

    // The fresh token after logout lets the user sign in again.
    client
        .post("/api/auth/login")
        .xsrf()
        .json(&json!({ "user_id": user }))
        .send()
        .await
        .assert_status(200);
}
