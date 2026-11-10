//! `TestApp::isolation_suite`: cross-tenant requests generated from a
//! module's route table, against a toy app that keeps notes per tenant
//! (the `X-Tenant` header) in memory. `NotesModule` keeps tenants apart;
//! `LeakyModule` does not.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use http::{HeaderMap, Method, StatusCode};
use navree_http::response::{IntoResponse, Response};
use navree_http::{Controller, Json, ModuleBuilderHttpExt, Path, RouteDef};
use navree_kernel::{
    BoxError, BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver,
};
use navree_testing::{IsolationOutcome, RecordContext, Tenancy, Tenant, TestApp};
use serde_json::{Value, json};
use uuid::Uuid;

/// Notes by id: (tenant, body).
#[derive(Default)]
struct Store {
    notes: Mutex<BTreeMap<String, (String, String)>>,
}

impl Store {
    fn insert(&self, tenant: &str, body: &str) -> String {
        let id = Uuid::now_v7().to_string();
        self.notes
            .lock()
            .unwrap()
            .insert(id.clone(), (tenant.to_owned(), body.to_owned()));
        id
    }

    fn get(&self, id: &str) -> Option<(String, String)> {
        self.notes.lock().unwrap().get(id).cloned()
    }
}

fn tenant(headers: &HeaderMap) -> String {
    headers
        .get("x-tenant")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned()
}

fn note(id: &str, tenant: &str, body: &str) -> Value {
    json!({ "id": id, "tenant_id": tenant, "body": body })
}

fn not_found() -> Response {
    StatusCode::NOT_FOUND.into_response()
}

/// Which behavior a controller has.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Scopes every query by the request's tenant.
    Scoped,
    /// Ignores the tenant.
    Leaky,
    /// Odd: its update changes another tenant's record and still answers
    /// 404, and its summary route is a 404 everywhere.
    Broken,
}

struct NotesController {
    store: Arc<Store>,
    kind: Kind,
}

fn routes(store: Arc<Store>, kind: Kind) -> Vec<RouteDef> {
    let s = store.clone();
    let index = RouteDef::on(Method::GET, "/", move |headers: HeaderMap| {
        let store = s.clone();
        async move {
            let me = tenant(&headers);
            let notes: Vec<Value> = store
                .notes
                .lock()
                .unwrap()
                .iter()
                .filter(|(_, (t, _))| kind == Kind::Leaky || *t == me)
                .map(|(id, (t, b))| note(id, t, b))
                .collect();
            Json(json!({ "data": notes }))
        }
    })
    .authenticated();
    let s = store.clone();
    let show = RouteDef::on(
        Method::GET,
        "/{id}",
        move |headers: HeaderMap, Path(id): Path<String>| {
            let store = s.clone();
            async move {
                match store.get(&id) {
                    Some((t, b)) if kind == Kind::Leaky || t == tenant(&headers) => {
                        Json(note(&id, &t, &b)).into_response()
                    }
                    _ => not_found(),
                }
            }
        },
    )
    .authenticated();
    let s = store.clone();
    let update = RouteDef::on(
        Method::PATCH,
        "/{id}",
        move |headers: HeaderMap, Path(id): Path<String>, Json(input): Json<Value>| {
            let store = s.clone();
            async move {
                let body = input["body"].as_str().unwrap_or("changed").to_owned();
                let mut notes = store.notes.lock().unwrap();
                match notes.get_mut(&id) {
                    Some((t, b)) if kind != Kind::Scoped || *t == tenant(&headers) => {
                        *b = body;
                        if kind == Kind::Broken {
                            return not_found();
                        }
                        Json(note(&id, t, b)).into_response()
                    }
                    _ => not_found(),
                }
            }
        },
    )
    .authenticated();
    let s = store.clone();
    let destroy = RouteDef::on(
        Method::DELETE,
        "/{id}",
        move |headers: HeaderMap, Path(id): Path<String>| {
            let store = s.clone();
            async move {
                let mut notes = store.notes.lock().unwrap();
                match notes.get(&id).cloned() {
                    Some((t, _)) if t == tenant(&headers) => {
                        notes.remove(&id);
                        StatusCode::NO_CONTENT.into_response()
                    }
                    // The leaky controller tells the caller the note exists.
                    Some(_) if kind == Kind::Leaky => StatusCode::FORBIDDEN.into_response(),
                    _ => not_found(),
                }
            }
        },
    )
    .authenticated();
    let s = store;
    let store_route = RouteDef::on(
        Method::POST,
        "/",
        move |headers: HeaderMap, Json(input): Json<Value>| {
            let store = s.clone();
            async move {
                let mut owner = tenant(&headers);
                if kind == Kind::Leaky
                    && let Some(given) = input["tenant_id"].as_str()
                {
                    owner = given.to_owned();
                }
                let body = input["body"].as_str().unwrap_or_default().to_owned();
                let id = store.insert(&owner, &body);
                (StatusCode::CREATED, Json(note(&id, &owner, &body))).into_response()
            }
        },
    )
    .authenticated();
    let mut all = vec![index, show, update, destroy, store_route];
    if kind == Kind::Leaky {
        all.push(
            RouteDef::on(Method::GET, "/{id}/history/{entry}", || async { "[]" }).authenticated(),
        );
    }
    if kind == Kind::Broken {
        // The show and update routes, and a summary that is never found.
        all = all.into_iter().skip(1).take(2).collect();
        all.push(
            RouteDef::on(Method::GET, "/{id}/summary", || async { not_found() }).authenticated(),
        );
    }
    all
}

macro_rules! notes_module {
    ($module:ident, $controller:ident, $prefix:literal, $kind:expr) => {
        struct $controller(NotesController);

        impl Injectable for $controller {
            fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
                Box::pin(async move {
                    Ok($controller(NotesController {
                        store: r.get::<Store>().await?,
                        kind: $kind,
                    }))
                })
            }
        }

        impl Controller for $controller {
            fn prefix(&self) -> &'static str {
                $prefix
            }

            fn routes(self: Arc<Self>) -> Vec<RouteDef> {
                routes(self.0.store.clone(), self.0.kind)
            }
        }

        struct $module;

        impl Module for $module {
            fn register(&self, m: &mut ModuleBuilder) {
                m.controller::<$controller>();
            }
        }
    };
}

notes_module!(NotesModule, ScopedNotes, "/notes", Kind::Scoped);
notes_module!(LeakyModule, LeakyNotes, "/leaky", Kind::Leaky);
notes_module!(BrokenModule, BrokenNotes, "/broken", Kind::Broken);

/// A module without routes.
struct EmptyModule;

impl Module for EmptyModule {
    fn register(&self, _m: &mut ModuleBuilder) {}
}

/// Tenants addressed by the `X-Tenant` header; their member may do
/// everything (the toy app checks nothing else).
struct HeaderTenancy;

impl Tenancy for HeaderTenancy {
    fn create<'a>(
        &'a self,
        _app: &'a TestApp,
        name: &'a str,
        _member: Uuid,
    ) -> BoxFuture<'a, Result<Tenant, BoxError>> {
        Box::pin(async move {
            let id = Uuid::now_v7().to_string();
            Ok(Tenant::new(id.clone(), name).header("x-tenant", &id))
        })
    }

    fn owner_field(&self) -> Option<&str> {
        Some("tenant_id")
    }
}

async fn app() -> (TestApp, Arc<Store>) {
    let store = Arc::new(Store::default());
    let app = TestApp::builder()
        .config("http.per_ip_rate_limit", Value::Null)
        .override_value(store.clone())
        .module(NotesModule)
        .module(LeakyModule)
        .module(BrokenModule)
        .module(EmptyModule)
        .build()
        .await;
    (app, store)
}

/// Creates a note in the tenant the suite asks for.
async fn create_note(cx: RecordContext) -> Result<String, BoxError> {
    let store = cx.app().get::<Store>().await?;
    Ok(store.insert(&cx.tenant().id, &format!("note {}", cx.n())))
}

fn outcomes(report: &navree_testing::IsolationReport) -> Vec<(String, String, String)> {
    report
        .routes
        .iter()
        .map(|r| {
            let (status, detail) = match &r.outcome {
                IsolationOutcome::Passed(d) => ("ok", d.clone()),
                IsolationOutcome::Failed(d) => ("FAIL", d.clone()),
                IsolationOutcome::Skipped(d) => ("skip", d.clone()),
            };
            (
                format!("{} {}", r.method, r.path),
                status.to_owned(),
                detail,
            )
        })
        .collect()
}

#[tokio::test]
async fn a_module_that_scopes_by_tenant_passes() {
    let (app, store) = app().await;
    let report = app
        .isolation_suite::<NotesModule>()
        .tenancy(HeaderTenancy)
        .record("id", create_note)
        .body("POST", "/api/notes", json!({ "body": "hello" }))
        .run()
        .await;
    assert!(report.is_ok(), "{report}");
    assert_eq!(
        outcomes(&report),
        [
            (
                "GET /api/notes".to_owned(),
                "ok".to_owned(),
                "another tenant's records are not listed".to_owned()
            ),
            (
                "GET /api/notes/{id}".to_owned(),
                "ok".to_owned(),
                "404 from another tenant, 200 in its own".to_owned()
            ),
            (
                "PATCH /api/notes/{id}".to_owned(),
                "ok".to_owned(),
                "404 from another tenant; the record is unchanged".to_owned()
            ),
            (
                "DELETE /api/notes/{id}".to_owned(),
                "ok".to_owned(),
                "404 from another tenant; the record is unchanged".to_owned()
            ),
            (
                "POST /api/notes".to_owned(),
                "ok".to_owned(),
                "201; the record stays in the caller's tenant".to_owned()
            ),
        ]
    );
    // Every record the suite made is still where it was put.
    assert!(store.notes.lock().unwrap().len() >= 5);
}

#[tokio::test]
async fn a_module_that_ignores_the_tenant_fails_route_by_route() {
    let (app, _store) = app().await;
    let report = app
        .isolation_suite::<LeakyModule>()
        .tenancy(HeaderTenancy)
        .record("id", create_note)
        .body("POST", "/api/leaky", json!({ "body": "hello" }))
        .try_run()
        .await
        .unwrap();
    assert!(!report.is_ok());
    assert_eq!(report.failed(), 6, "{report}");
    let found = outcomes(&report);
    let detail = |route: &str| {
        found
            .iter()
            .find(|(r, _, _)| r == route)
            .map(|(_, _, d)| d.clone())
            .unwrap_or_else(|| panic!("{route} not in {found:?}"))
    };
    assert_eq!(
        detail("GET /api/leaky"),
        "another tenant's record is listed (200)"
    );
    assert_eq!(
        detail("GET /api/leaky/{id}"),
        "answered 200 to another tenant: its record was read"
    );
    assert_eq!(
        detail("PATCH /api/leaky/{id}"),
        "answered 200 to another tenant: its record was changed"
    );
    assert_eq!(
        detail("DELETE /api/leaky/{id}"),
        "answered 403 to another tenant: a record of another tenant must be a 404, so its existence does not show"
    );
    assert_eq!(
        detail("POST /api/leaky"),
        "answered 201 and put the record in another tenant (tenant_id)"
    );
    assert_eq!(
        detail("GET /api/leaky/{id}/history/{entry}"),
        "path parameter {entry} has no record: bind it with .record(\"entry\", ..), or skip the route"
    );
}

#[tokio::test]
async fn controls_and_unchanged_records_are_checked() {
    let (app, _store) = app().await;
    let report = app
        .isolation_suite::<BrokenModule>()
        .tenancy(HeaderTenancy)
        .record("id", create_note)
        .try_run()
        .await
        .unwrap();
    assert_eq!(
        outcomes(&report),
        [
            (
                "GET /api/broken/{id}".to_owned(),
                "ok".to_owned(),
                "404 from another tenant, 200 in its own".to_owned()
            ),
            (
                "PATCH /api/broken/{id}".to_owned(),
                "FAIL".to_owned(),
                "answered 404 to another tenant but changed the record".to_owned()
            ),
            (
                "GET /api/broken/{id}/summary".to_owned(),
                "FAIL".to_owned(),
                "the control request failed: GET in the record's own tenant answered 404, so the suite cannot tell isolation from a broken route".to_owned()
            ),
        ]
    );
}

#[tokio::test]
async fn routes_can_be_skipped_and_bodiless_creates_are_skipped() {
    let (app, _store) = app().await;
    let report = app
        .isolation_suite::<NotesModule>()
        .tenancy(HeaderTenancy)
        .record("id", create_note)
        .skip("delete", "/api/notes/{id}", "covered by the archive tests")
        .run()
        .await;
    assert!(report.is_ok(), "{report}");
    let found = outcomes(&report);
    assert_eq!(
        found[3],
        (
            "DELETE /api/notes/{id}".to_owned(),
            "skip".to_owned(),
            "covered by the archive tests".to_owned()
        )
    );
    assert_eq!(
        found[4],
        (
            "POST /api/notes".to_owned(),
            "skip".to_owned(),
            "no request body: give one with .body(\"POST\", \"/api/notes\", json)".to_owned()
        )
    );
    assert_eq!(report.skipped(), 2);
    assert_eq!(report.passed(), 3);
}

#[tokio::test]
async fn the_report_reads_as_a_table() {
    let (app, _store) = app().await;
    let report = app
        .isolation_suite::<LeakyModule>()
        .tenancy(HeaderTenancy)
        .record("id", create_note)
        .try_run()
        .await
        .unwrap();
    let text = report.to_string();
    assert!(
        text.starts_with(
            "Isolation suite for LeakyModule: 6 routes, 0 passed, 5 failed, 1 skipped\n"
        ),
        "{text}"
    );
    assert!(
        text.contains("\n  FAIL  GET     /api/leaky/{id}  "),
        "{text}"
    );
    assert!(text.contains("\n  skip  POST    /api/leaky  "), "{text}");
}

#[tokio::test]
#[should_panic(expected = "Isolation suite for LeakyModule")]
async fn run_panics_with_the_report_when_a_route_leaks() {
    let (app, _store) = app().await;
    app.isolation_suite::<LeakyModule>()
        .tenancy(HeaderTenancy)
        .record("id", create_note)
        .run()
        .await;
}

#[tokio::test]
async fn the_suite_needs_a_tenancy_and_routes() {
    let (app, _store) = app().await;
    let error = app
        .isolation_suite::<NotesModule>()
        .record("id", create_note)
        .try_run()
        .await
        .unwrap_err();
    assert!(error.to_string().contains("no tenancy"), "{error}");
    assert!(!app.isolation_suite::<NotesModule>().has_tenancy());

    let error = app
        .isolation_suite::<EmptyModule>()
        .tenancy(HeaderTenancy)
        .try_run()
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "EmptyModule has no routes to test");
}
