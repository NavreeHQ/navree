//! Helpers shared by the navree-api integration tests: a small deals API
//! written as a hand-built controller, and an app around it.

#![allow(dead_code)]

use std::sync::Arc;

use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, Method, Request, StatusCode, request::Parts};
use http_body_util::BodyExt;
use navree_api::{ApiDocs, ApiModule, ApiOptions};
use navree_contracts::Principal;
use navree_http::{
    ApiMeta, AppError, Authenticator, Controller, HttpModule, HttpOptions, ModuleBuilderHttpExt,
    RouteAuthorizer, RouteDef, RouteMeta, build_router, schema_fn,
};
use navree_kernel::{
    App, AppBuilder, BoxFuture, Injectable, Navree, KernelError, Module, ModuleBuilder, Resolver,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tower::ServiceExt;
use utoipa::ToSchema;

/// The owner of a deal (referenced by [`Deal`], so hoisted into components).
#[derive(Serialize, Deserialize, ToSchema)]
pub struct Owner {
    pub id: String,
    pub name: String,
}

/// A deal as the API returns it.
#[derive(Serialize, Deserialize, ToSchema)]
pub struct Deal {
    pub id: String,
    pub title: String,
    pub owner: Owner,
}

/// The body of `POST /deals`.
#[derive(Serialize, Deserialize, ToSchema)]
pub struct CreateDeal {
    pub title: String,
    pub amount: Option<u64>,
}

/// The deals API: one route per access kind.
pub struct DealsController;

impl Injectable for DealsController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(DealsController) })
    }
}

impl Controller for DealsController {
    fn prefix(&self) -> &'static str {
        "/deals"
    }

    fn version(&self) -> Option<&'static str> {
        Some("1")
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "/", || async { "[]" })
                .name("deals.index")
                .public()
                .api(ApiMeta {
                    summary: Some("List deals".to_owned()),
                    tags: vec!["Deals".to_owned()],
                    ..ApiMeta::default()
                }),
            RouteDef::on(Method::POST, "/", || async { "{}" })
                .name("deals.store")
                .permissions(&["deals.create"])
                .api(ApiMeta {
                    summary: Some("Create a deal".to_owned()),
                    description: Some("Creates a deal owned by the caller.".to_owned()),
                    tags: vec!["Deals".to_owned()],
                    responses: vec![422],
                    success_status: 201,
                    request: Some(schema_fn::<CreateDeal>),
                    response: Some(schema_fn::<Deal>),
                }),
            RouteDef::on(Method::GET, "/{id}", || async { "{}" })
                .name("deals.show")
                .authenticated()
                .api(ApiMeta {
                    tags: vec!["Deals".to_owned()],
                    responses: vec![404],
                    response: Some(schema_fn::<Deal>),
                    ..ApiMeta::default()
                }),
            RouteDef::on(Method::DELETE, "/{id}/notes/{note_id}", || async { "" })
                .roles(&["admin"]),
            RouteDef::on(Method::GET, "!/files/{*path}", || async { "file" }).public(),
        ]
    }
}

/// The feature module serving [`DealsController`].
pub struct DealsModule;

impl Module for DealsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<DealsController>();
    }
}

/// Routes served by [`ExtraController`] under `/api/extra`.
#[derive(Clone)]
pub struct ExtraRoutes(pub Vec<RouteDef>);

/// A controller serving the [`ExtraRoutes`] value of its module.
pub struct ExtraController {
    routes: Arc<ExtraRoutes>,
}

impl Injectable for ExtraController {
    fn dependencies() -> Vec<navree_kernel::ProviderKey> {
        vec![navree_kernel::ProviderKey::of::<ExtraRoutes>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(Self {
                routes: r.get::<ExtraRoutes>().await?,
            })
        })
    }
}

impl Controller for ExtraController {
    fn prefix(&self) -> &'static str {
        "/extra"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        self.routes.0.clone()
    }
}

/// Extra registrations on an [`ExtraModule`].
pub type Registrations = Arc<dyn Fn(&mut ModuleBuilder) + Send + Sync>;

/// A feature module serving extra routes, with optional extra registrations.
pub struct ExtraModule {
    pub routes: Vec<RouteDef>,
    pub register: Option<Registrations>,
}

impl Module for ExtraModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.value(ExtraRoutes(self.routes.clone()))
            .controller::<ExtraController>();
        if let Some(register) = &self.register {
            register(m);
        }
    }
}

/// A public `GET path` documented with `api`.
pub fn documented(path: &str, api: ApiMeta) -> RouteDef {
    RouteDef::on(Method::GET, path, || async { "{}" })
        .public()
        .api(api)
}

/// Lets every request through as a guest (non-public routes need an
/// authenticator to boot).
pub struct GuestAuthenticator;

impl Authenticator for GuestAuthenticator {
    fn authenticate<'a>(
        &'a self,
        _parts: &'a mut Parts,
    ) -> BoxFuture<'a, Result<Option<Principal>, AppError>> {
        Box::pin(async { Ok(None) })
    }
}

/// Allows everything (routes with roles or permissions need an authorizer to boot).
pub struct AllowAll;

impl RouteAuthorizer for AllowAll {
    fn authorize<'a>(
        &'a self,
        _parts: &'a mut Parts,
        _meta: &'a RouteMeta,
    ) -> BoxFuture<'a, Result<(), AppError>> {
        Box::pin(async { Ok(()) })
    }
}

/// HTTP options for tests: no per-IP rate limit.
pub fn http_options() -> HttpOptions {
    HttpOptions {
        per_ip_rate_limit: None,
        ..HttpOptions::default()
    }
}

/// HttpModule, the configured ApiModule, the deals module, a guest
/// authenticator and an allow-all authorizer.
pub fn app(options: ApiOptions) -> AppBuilder {
    Navree::app()
        .module(HttpModule::for_root(http_options()))
        .module(ApiModule::for_root(options))
        .module(DealsModule)
        .override_value::<dyn Authenticator>(Arc::new(GuestAuthenticator))
        .override_value::<dyn RouteAuthorizer>(Arc::new(AllowAll))
}

/// [`app`] plus an [`ExtraModule`] serving `routes`.
pub fn app_with(options: ApiOptions, routes: Vec<RouteDef>) -> AppBuilder {
    app(options).module(ExtraModule {
        routes,
        register: None,
    })
}

/// [`app_with`], the extra module also running `register`.
pub fn app_registering(
    routes: Vec<RouteDef>,
    register: impl Fn(&mut ModuleBuilder) + Send + Sync + 'static,
) -> AppBuilder {
    app(ApiOptions::default()).module(ExtraModule {
        routes,
        register: Some(Arc::new(register)),
    })
}

/// The boot error of `builder` as text.
pub async fn boot_error(builder: AppBuilder) -> String {
    match builder.boot().await {
        Ok(_) => panic!("boot must fail"),
        Err(errors) => errors.to_string(),
    }
}

/// Boots [`app`] with default options.
pub async fn boot() -> App {
    app(ApiOptions::default()).boot().await.expect("app boots")
}

/// The document built by the app's [`ApiDocs`].
pub async fn document(app: &App) -> Value {
    let docs = app.ctx().get::<ApiDocs>().await.expect("ApiDocs provider");
    docs.document()
        .await
        .expect("document builds")
        .as_ref()
        .clone()
}

/// A collected response.
pub struct TestResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Bytes,
}

impl TestResponse {
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).expect("JSON body")
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }
}

/// `GET path` through the app's full router.
pub async fn get(app: &App, path: &str) -> TestResponse {
    let router = build_router(app.ctx()).await.expect("router builds");
    let response = router
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .expect("router is infallible");
    let status = response.status();
    let headers = response.headers().clone();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body collects")
        .to_bytes();
    TestResponse {
        status,
        headers,
        body,
    }
}
