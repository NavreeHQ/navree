//! Shared fixtures: a test app with the database, crypto, the session, auth
//! with its sign-in pages, Navree UI, access and Lighthouse, plus a demo
//! module whose routes fail, run queries, report and take their time.

#![allow(dead_code)]

use std::sync::Arc;
use std::time::Duration;

use http::Method;
use navree_access::{AbilityCtx, AccessModule, AccessOptions, AccessService, Rules};
use navree_auth::{AuthModule, AuthPages, AuthService, ClientMeta};
use navree_crypto::CryptoModule;
use navree_db::{DatabaseModule, Db, Executor, Statement};
use navree_http::{AppError, Controller, ModuleBuilderHttpExt, NoContent, Path, RouteDef};
use navree_kernel::{
    BoxFuture, Injectable, KernelError, Module, ModuleBuilder, ProviderKey, Resolver,
};
use navree_lighthouse::{Lighthouse, LighthouseModule, LighthouseStore, Report};
use navree_session::{SessionModule, SessionOptions};
use navree_testing::{TestApp, TestAppBuilder};
use navree_ui::UiModule;
use serde_json::Value;
use uuid::Uuid;

/// The password every test user has.
pub const PASSWORD: &str = "correct horse battery";

/// The demo routes under `/api/demo`.
pub struct DemoController {
    lighthouse: Arc<Lighthouse>,
    db: Arc<Db>,
}

impl Injectable for DemoController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Lighthouse>(), ProviderKey::of::<Db>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(DemoController {
                lighthouse: r.get::<Lighthouse>().await?,
                db: r.get::<Db>().await?,
            })
        })
    }
}

/// An error with a source, for the source chain.
#[derive(Debug, thiserror::Error)]
pub enum InvoiceError {
    #[error("invoice {0} not found")]
    NotFound(String),
    #[error("cannot charge invoice")]
    Charge(#[source] std::io::Error),
}

impl Controller for DemoController {
    fn prefix(&self) -> &'static str {
        "/demo"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let (report, query, tagged) = (self.clone(), self.clone(), self.clone());
        vec![
            RouteDef::on(Method::GET, "/ok", || async { "ok" }).public(),
            RouteDef::on(Method::GET, "/boom/{n}", |Path(n): Path<u32>| async move {
                Err::<&'static str, AppError>(AppError::internal(InvoiceError::NotFound(format!(
                    "inv_{n}"
                ))))
            })
            .public(),
            RouteDef::on(Method::GET, "/panic/{n}", |Path(n): Path<u32>| async move {
                if n > 0 {
                    panic!("index {n} out of range");
                }
                "never"
            })
            .public(),
            RouteDef::on(Method::POST, "/charge", || async {
                Err::<&'static str, AppError>(AppError::internal(InvoiceError::Charge(
                    std::io::Error::other("card declined"),
                )))
            })
            .public(),
            RouteDef::on(Method::GET, "/slow", || async {
                tokio::time::sleep(Duration::from_millis(30)).await;
                "slow"
            })
            .public(),
            RouteDef::on(Method::GET, "/query", move || {
                let this = query.clone();
                async move {
                    this.db
                        .fetch_all(&Statement::text("SELECT 1 WHERE 1 = 1"))
                        .await
                        .map_err(AppError::internal)?;
                    Ok::<_, AppError>("queried")
                }
            })
            .public(),
            RouteDef::on(Method::POST, "/report", move || {
                let this = report.clone();
                async move {
                    let error = InvoiceError::Charge(std::io::Error::other("card declined"));
                    this.lighthouse.report(&error);
                    NoContent
                }
            })
            .public(),
            RouteDef::on(Method::POST, "/tagged", move || {
                let this = tagged.clone();
                async move {
                    this.lighthouse.tag_current("plan", "pro");
                    this.lighthouse.capture(
                        Report::message("payment event without a customer")
                            .context("invoice_id", 1042)
                            .context("api_token", "secret-value"),
                    );
                    NoContent
                }
            })
            .public(),
        ]
    }
}

/// Registers the demo controller.
pub struct DemoModule;

impl Module for DemoModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<DemoController>();
    }
}

fn abilities(ctx: &AbilityCtx, rules: &mut Rules) {
    ctx.grant_permissions(rules);
}

/// The platform: database, crypto, the session, auth (with fast hashing) and
/// its sign-in pages on Navree UI, access with the super-admin role `admin`,
/// the demo routes, and Lighthouse configured from `lighthouse.*`.
pub fn builder() -> TestAppBuilder {
    crate::servers::database(TestApp::builder())
        .config("http.per_ip_rate_limit", Value::Null)
        .config("app.key", navree_crypto::generate_key())
        .config("app.url", "https://api.acme.test")
        .config("hashing.memory_kib", 1024)
        .config("hashing.iterations", 1)
        .config("lighthouse.release", "r1")
        .config("lighthouse.environment", "test")
        // A busy machine must not open slow-query or slow-request issues the
        // test doesn't expect. Tests of slowness set their own.
        .config("lighthouse.thresholds.slow_query_ms", 60_000)
        .config("lighthouse.thresholds.slow_request_ms", 60_000)
        .module(CryptoModule::from_config())
        .module(DatabaseModule::from_config())
        .module(SessionModule::for_root(SessionOptions::default()))
        .module(AuthModule::from_config())
        .module(UiModule)
        .module(AuthPages::new().app_name("Acme CRM"))
        .module(AccessModule::for_root(AccessOptions {
            abilities,
            super_admin_role: Some("admin".to_owned()),
        }))
        .module(DemoModule)
        .module(LighthouseModule::from_config())
}

/// The app's Lighthouse.
pub async fn lighthouse(app: &TestApp) -> Arc<Lighthouse> {
    app.get::<Lighthouse>().await
}

/// Writes everything recorded and returns the store.
pub async fn flushed(app: &TestApp) -> LighthouseStore {
    let lighthouse = lighthouse(app).await;
    lighthouse.flush().await;
    lighthouse.store().expect("a store").clone()
}

/// Creates a user with `password`, gives them `role` with `permissions`
/// (none: no role), and returns the id.
pub async fn user(app: &TestApp, email: &str, role: Option<&str>, permissions: &[&str]) -> Uuid {
    let auth = app.get::<AuthService>().await;
    let user = auth
        .create_user("Ada", email, PASSWORD)
        .await
        .unwrap();
    if let Some(role) = role {
        let db = app.get::<Db>().await;
        let service = app.get::<AccessService>().await;
        let uow = db.uow(None, None).await;
        if service.role(role).await.unwrap().is_none() {
            service.create_role(&uow, role, None).await.unwrap();
        }
        for permission in permissions {
            service.grant(&uow, role, permission).await.unwrap();
        }
        service.assign_role(&uow, user.id, role).await.unwrap();
        uow.commit().await.unwrap();
    }
    user.id
}

/// An access token for `email`.
pub async fn token(app: &TestApp, email: &str) -> String {
    let auth = app.get::<AuthService>().await;
    let (_, tokens) = auth
        .login(email, PASSWORD, &ClientMeta::default())
        .await
        .unwrap();
    tokens.access_token
}
