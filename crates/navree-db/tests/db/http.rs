//! The unit-of-work layer, the `Uow` extractor and database error mapping,
//! through the full navree-http router.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use axum::body::Body;
use axum::extract::Path;
use http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use navree_contracts::{AuthVia, Principal};
use navree_db::{
    ChangeEvent, ChangeObserver, DatabaseModule, Db, MigrationSet, ModuleBuilderDbExt, Repo, Uow,
};
use navree_http::{
    AppError, Authenticator, Controller, HttpModule, HttpOptions, Json, ModuleBuilderHttpExt,
    RouteDef, build_router,
};
use navree_kernel::{
    App, BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Navree, ProviderKey, Resolver,
};
use sea_orm::{DatabaseTransaction, DbErr};
use serde_json::{Value, json};
use tokio::sync::{Mutex, Notify, oneshot};
use tower::ServiceExt;
use uuid::Uuid;

use crate::fixtures::{CreateNotes, count, new_note, note};
use crate::support::TempDb;

type Notes = Repo<note::Entity>;

/// Signals for the disconnect test: the handler reports that it has
/// written, then waits until the test lets it finish.
#[derive(Default)]
struct Gate {
    written: Mutex<Option<oneshot::Sender<()>>>,
    proceed: Notify,
    committed: Mutex<Option<oneshot::Sender<()>>>,
}

/// Makes [`RefusingObserver`] fail the commit.
#[derive(Default)]
struct Refuse(AtomicBool);

struct RefusingObserver(Arc<Refuse>);

impl Injectable for RefusingObserver {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Refuse>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move { Ok(Self(r.get::<Refuse>().await?)) })
    }
}

impl ChangeObserver for RefusingObserver {
    fn observe<'a>(
        &'a self,
        _tx: &'a DatabaseTransaction,
        _uow: &'a Uow,
        _changes: &'a [ChangeEvent],
    ) -> BoxFuture<'a, Result<(), DbErr>> {
        Box::pin(async move {
            if self.0.0.load(Ordering::SeqCst) {
                Err(DbErr::Custom("refused".into()))
            } else {
                Ok(())
            }
        })
    }
}

struct NotesController {
    gate: Arc<Gate>,
}

impl Injectable for NotesController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Gate>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(Self {
                gate: r.get::<Gate>().await?,
            })
        })
    }
}

impl Controller for NotesController {
    fn prefix(&self) -> &'static str {
        "/notes"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let gate = self.gate.clone();
        vec![
            RouteDef::on(
                Method::POST,
                "/{title}",
                |uow: Uow, Path(title): Path<String>| async move {
                    let note = Notes::insert(&uow, new_note(&title)).await?;
                    Ok::<_, AppError>((StatusCode::CREATED, Json(json!({ "id": note.id }))))
                },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "/{title}/fail",
                |uow: Uow, Path(title): Path<String>| async move {
                    Notes::insert(&uow, new_note(&title)).await?;
                    Err::<(), _>(AppError::bad_request("changed my mind"))
                },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "/{title}/panic",
                |uow: Uow, Path(title): Path<String>| async move {
                    Notes::insert(&uow, new_note(&title)).await.unwrap();
                    if title.len() < 1000 {
                        panic!("handler panicked");
                    }
                },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "/{title}/slow",
                move |uow: Uow, Path(title): Path<String>| {
                    let gate = gate.clone();
                    async move {
                        Notes::insert(&uow, new_note(&title)).await?;
                        let committed = gate.committed.lock().await.take();
                        uow.after_commit(move || {
                            Box::pin(async move {
                                if let Some(tx) = committed {
                                    let _ = tx.send(());
                                }
                            })
                        });
                        if let Some(written) = gate.written.lock().await.take() {
                            let _ = written.send(());
                        }
                        gate.proceed.notified().await;
                        Ok::<_, AppError>(StatusCode::CREATED)
                    }
                },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "/by-id/{id}",
                |uow: Uow, Path(id): Path<Uuid>| async move {
                    let note = Notes::find_or_fail(&uow, id).await?;
                    Ok::<_, AppError>(Json(json!({ "title": note.title })))
                },
            )
            .public(),
            RouteDef::on(Method::GET, "/current", |uow: Uow| async move {
                let current = Uow::current();
                Json(json!({
                    "same": current.as_ref().is_some_and(|current| current.same_as(&uow)),
                    "finished": match current {
                        Some(current) => Some(current.is_finished().await),
                        None => None,
                    },
                }))
            })
            .public(),
            RouteDef::on(Method::GET, "/whoami", |uow: Uow| async move {
                Json(json!({
                    "actor": uow.actor(),
                    "request_id": uow.request_id(),
                }))
            })
            .authenticated(),
        ]
    }
}

struct NotesModule;

impl Module for NotesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(MigrationSet::new("notes").migration(CreateNotes))
            .value(Gate::default())
            .value(Refuse::default())
            .change_observer::<RefusingObserver>()
            .controller::<NotesController>();
    }
}

/// Authenticates `x-user-id: <uuid>`.
struct HeaderAuthenticator;

impl Authenticator for HeaderAuthenticator {
    fn authenticate<'a>(
        &'a self,
        parts: &'a mut http::request::Parts,
    ) -> BoxFuture<'a, Result<Option<Principal>, AppError>> {
        Box::pin(async move {
            Ok(parts
                .headers
                .get("x-user-id")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| Uuid::parse_str(v).ok())
                .map(|user_id| Principal {
                    user_id,
                    via: AuthVia::Test,
                    token_abilities: None,
                    session_id: None,
                }))
        })
    }
}

fn http_options() -> HttpOptions {
    HttpOptions {
        per_ip_rate_limit: None,
        ..HttpOptions::default()
    }
}

struct Setup {
    app: App,
    router: axum::Router,
    db: Db,
    _temp: TempDb,
}

async fn setup() -> Setup {
    let temp = TempDb::new();
    let mut options = temp.options();
    options.migrate_on_boot = true;
    let app = Navree::app()
        .module(HttpModule::for_root(http_options()))
        .module(DatabaseModule::for_root(options))
        .module(NotesModule)
        .override_value::<dyn Authenticator>(Arc::new(HeaderAuthenticator))
        .boot()
        .await
        .expect("app boots");
    let router = build_router(app.ctx()).await.expect("router builds");
    let db = (*app.ctx().get::<Db>().await.unwrap()).clone();
    Setup {
        app,
        router,
        db,
        _temp: temp,
    }
}

async fn send(router: &axum::Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json = serde_json::from_slice(&body).unwrap_or(Value::Null);
    (status, json)
}

fn post(path: &str) -> Request<Body> {
    Request::post(path).body(Body::empty()).unwrap()
}

#[tokio::test]
async fn uow_commits_on_2xx_and_rolls_back_on_error() {
    let s = setup().await;
    let (status, _) = send(&s.router, post("/api/notes/kept")).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(count(&s.db, "notes").await, 1);

    let (status, body) = send(&s.router, post("/api/notes/dropped/fail")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(count(&s.db, "notes").await, 1, "4xx rolls back");

    let (status, _) = send(&s.router, post("/api/notes/boom/panic")).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(count(&s.db, "notes").await, 1, "a panic rolls back");

    let (status, _) = send(&s.router, post("/api/notes/after")).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "the database is not left locked"
    );
    assert_eq!(count(&s.db, "notes").await, 2);
}

#[tokio::test]
async fn handlers_run_with_the_request_uow_as_current() {
    let s = setup().await;
    let request = Request::get("/api/notes/current")
        .body(Body::empty())
        .unwrap();
    let (status, body) = send(&s.router, request).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, json!({ "same": true, "finished": false }));
}

#[tokio::test]
async fn uow_completes_even_if_client_disconnects() {
    let s = setup().await;
    let gate = s.app.ctx().get::<Gate>().await.unwrap();
    let (written_tx, written_rx) = oneshot::channel();
    let (committed_tx, committed_rx) = oneshot::channel();
    *gate.written.lock().await = Some(written_tx);
    *gate.committed.lock().await = Some(committed_tx);

    let request = s.router.clone().oneshot(post("/api/notes/slow-one/slow"));
    tokio::select! {
        _ = request => panic!("the handler must still be waiting"),
        written = written_rx => written.expect("handler wrote the row"),
    }
    // The client is gone: the request future has been dropped.
    gate.proceed.notify_one();
    tokio::time::timeout(Duration::from_secs(10), committed_rx)
        .await
        .expect("the unit of work commits")
        .expect("after-commit hook ran");
    assert_eq!(count(&s.db, "notes").await, 1);
}

#[tokio::test]
async fn unique_violation_maps_to_409() {
    let s = setup().await;
    assert_eq!(
        send(&s.router, post("/api/notes/same")).await.0,
        StatusCode::CREATED
    );
    let (status, body) = send(&s.router, post("/api/notes/same")).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(
        body,
        serde_json::json!({
            "statusCode": 409,
            "message": "A record with the same unique value already exists.",
            "error": "Conflict",
            "code": "conflict",
        })
    );
    assert!(
        !body
            .to_string()
            .to_lowercase()
            .contains("unique constraint"),
        "no SQL details leak: {body}"
    );
    assert_eq!(count(&s.db, "notes").await, 1);
}

#[tokio::test]
async fn record_not_found_maps_to_404() {
    let s = setup().await;
    let (status, body) = send(
        &s.router,
        Request::get(format!("/api/notes/by-id/{}", Uuid::now_v7()))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(
        body,
        serde_json::json!({"statusCode": 404, "message": "Not Found", "error": "Not Found", "code": "not_found"})
    );
}

#[tokio::test]
async fn commit_failure_is_an_error_response() {
    let s = setup().await;
    s.app
        .ctx()
        .get::<Refuse>()
        .await
        .unwrap()
        .0
        .store(true, Ordering::SeqCst);
    let (status, body) = send(&s.router, post("/api/notes/refused")).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert_eq!(count(&s.db, "notes").await, 0);
}

#[tokio::test]
async fn uow_gets_actor_and_request_id_from_the_pipeline() {
    let s = setup().await;
    let user = Uuid::now_v7();
    let response = s
        .router
        .clone()
        .oneshot(
            Request::get("/api/notes/whoami")
                .header("x-user-id", user.to_string())
                .header("x-request-id", "req-42")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body["actor"], user.to_string());
    assert_eq!(body["request_id"], "req-42");
}

#[tokio::test]
async fn uow_extractor_without_database_module_is_500() {
    struct Bare;
    impl Injectable for Bare {
        fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
            Box::pin(async { Ok(Bare) })
        }
    }
    impl Controller for Bare {
        fn prefix(&self) -> &'static str {
            "/bare"
        }
        fn routes(self: Arc<Self>) -> Vec<RouteDef> {
            vec![RouteDef::on(Method::GET, "/", |_uow: Uow| async { "unreachable" }).public()]
        }
    }
    struct BareModule;
    impl Module for BareModule {
        fn register(&self, m: &mut ModuleBuilder) {
            m.controller::<Bare>();
        }
    }
    let app = Navree::app()
        .module(HttpModule::for_root(http_options()))
        .module(BareModule)
        .boot()
        .await
        .unwrap();
    let router = build_router(app.ctx()).await.unwrap();
    let (status, body) = send(
        &router,
        Request::get("/api/bare").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert_eq!(body["message"], "Internal server error");
}
