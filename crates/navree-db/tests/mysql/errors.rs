//! MySQL's constraint errors in Navree's error shape: a duplicate key
//! (1062) and foreign key violations both ways (1452 on the child, 1451 on
//! the parent) are 409s, a missing record is a 404, and through HTTP the
//! request's unit of work commits below 400 and rolls back otherwise.

use std::sync::Arc;

use http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use navree_db::{
    DatabaseModule, Db, DbError, Executor, MigrationSet, ModuleBuilderDbExt, Uow, map_db_error,
    record,
};
use navree_http::body::Body;
use navree_http::{
    AppError, Controller, HttpModule, HttpOptions, Json, ModuleBuilderHttpExt, Path, RouteDef,
    build_router,
};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Navree, Resolver};
use navree_schema::{ReferentialAction, Schema};
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

use crate::fixtures::{CreateNotes, CreateTable, Note, create_notes, new_note};
use crate::support::{Scratch, count};

/// The status and message `map_db_error` gives `error`, if any.
fn mapped(error: &DbError) -> Option<(u16, String)> {
    map_db_error(error).map(|app| (app.status.as_u16(), app.body()["message"].to_string()))
}

#[tokio::test]
async fn constraint_errors_map_to_conflicts() {
    let Some(scratch) = Scratch::new("constraint_errors_map_to_conflicts").await else {
        return;
    };
    let db = scratch.db().await;
    create_notes(&db).await;
    let schema = Schema::new(db.pool());
    for migration in [
        CreateTable::boxed("m1", "users"),
        CreateTable::referencing("m2", "companies", "users", ReferentialAction::Restrict),
    ] {
        migration.up(&schema).await.unwrap();
    }

    record::insert(&db, &new_note("same")).await.unwrap();
    let duplicate = record::insert(&db, &new_note("same")).await.unwrap_err();
    assert_eq!(
        mapped(&duplicate),
        Some((
            409,
            "\"A record with the same unique value already exists.\"".to_owned()
        )),
        "{duplicate}"
    );

    let fk = "\"The change conflicts with a related record.\"".to_owned();
    let orphan = db
        .pool()
        .execute_script(&format!(
            "INSERT INTO companies (id, title, owner_id) VALUES (UNHEX('{}'), 'acme', UNHEX('{}'))",
            Uuid::now_v7().simple(),
            Uuid::now_v7().simple()
        ))
        .await
        .unwrap_err();
    assert_eq!(mapped(&orphan), Some((409, fk.clone())), "{orphan}");

    let owner = Uuid::now_v7();
    db.pool()
        .execute_script(&format!(
            "INSERT INTO users (id, title) VALUES (UNHEX('{}'), 'ann')",
            owner.simple()
        ))
        .await
        .unwrap();
    db.pool()
        .execute_script(&format!(
            "INSERT INTO companies (id, title, owner_id) VALUES (UNHEX('{}'), 'acme', UNHEX('{}'))",
            Uuid::now_v7().simple(),
            owner.simple()
        ))
        .await
        .unwrap();
    let referenced = db
        .pool()
        .execute_script("DELETE FROM users")
        .await
        .unwrap_err();
    assert_eq!(mapped(&referenced), Some((409, fk)), "{referenced}");

    let syntax = db.pool().execute_script("SELEC 1").await.unwrap_err();
    assert_eq!(mapped(&syntax), None, "anything else stays a 500");
}

struct NotesController;

impl Injectable for NotesController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(NotesController) })
    }
}

impl Controller for NotesController {
    fn prefix(&self) -> &'static str {
        "/notes"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(
                Method::POST,
                "/{title}",
                |uow: Uow, Path(title): Path<String>| async move {
                    let note = record::insert_recorded(&uow, &new_note(&title)).await?;
                    Ok::<_, AppError>((StatusCode::CREATED, Json(json!({ "id": note.id }))))
                },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "/{title}/fail",
                |uow: Uow, Path(title): Path<String>| async move {
                    record::insert_recorded(&uow, &new_note(&title)).await?;
                    Err::<(), _>(AppError::bad_request("changed my mind"))
                },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "/by-id/{id}",
                |uow: Uow, Path(id): Path<Uuid>| async move {
                    let note = record::find::<Note, _>(&uow, id)
                        .await?
                        .ok_or_else(|| DbError::not_found(format!("notes {id}")))?;
                    Ok::<_, AppError>(Json(json!({ "title": note.title })))
                },
            )
            .public(),
        ]
    }
}

struct NotesModule;

impl Module for NotesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(MigrationSet::new("notes").migration(CreateNotes))
            .controller::<NotesController>();
    }
}

async fn send(router: &axum::Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

fn post(path: &str) -> Request<Body> {
    Request::post(path).body(Body::empty()).unwrap()
}

#[tokio::test]
async fn http_requests_commit_roll_back_and_answer_in_the_error_shape() {
    let Some(scratch) =
        Scratch::new("http_requests_commit_roll_back_and_answer_in_the_error_shape").await
    else {
        return;
    };
    let mut options = scratch.options();
    options.migrate_on_boot = true;
    let app = Navree::app()
        .module(HttpModule::for_root(HttpOptions {
            per_ip_rate_limit: None,
            ..HttpOptions::default()
        }))
        .module(DatabaseModule::for_root(options))
        .module(NotesModule)
        .boot()
        .await
        .expect("boots");
    let router = build_router(app.ctx()).await.expect("router builds");
    let db = app.ctx().get::<Db>().await.unwrap();

    assert_eq!(
        send(&router, post("/api/notes/kept")).await.0,
        StatusCode::CREATED
    );
    assert_eq!(count(db.pool(), "notes").await, 1);

    let (status, body) = send(&router, post("/api/notes/dropped/fail")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(count(db.pool(), "notes").await, 1, "a 4xx rolls back");

    let (status, body) = send(&router, post("/api/notes/kept")).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(
        body,
        json!({
            "statusCode": 409,
            "message": "A record with the same unique value already exists.",
            "error": "Conflict",
            "code": "conflict",
        })
    );
    assert!(
        !body.to_string().contains("Duplicate entry"),
        "no SQL detail reaches the client: {body}"
    );

    let (status, body) = send(
        &router,
        Request::get(format!("/api/notes/by-id/{}", Uuid::now_v7()))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(
        body,
        json!({"statusCode": 404, "message": "Not Found", "error": "Not Found", "code": "not_found"})
    );
    assert_eq!(
        send(&router, post("/api/notes/after")).await.0,
        StatusCode::CREATED
    );
    assert_eq!(count(db.pool(), "notes").await, 2);
    app.shutdown().await.unwrap();
}
