//! Telemetry: every statement as a `QueryExecuted` event, carrying the
//! request it ran in.

use std::sync::Arc;

use axum::body::Body;
use http::{Method, Request};
use navree_db::{DatabaseModule, Db, DbOptions, Uow};
use navree_http::{
    AppError, Controller, HttpModule, HttpOptions, ModuleBuilderHttpExt, RouteDef, build_router,
};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Navree, Resolver};
use navree_telemetry::{EventKind, Telemetry, TelemetryRecorder};
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use tower::ServiceExt;

use crate::support::TempDb;

fn quiet(temp: &TempDb) -> DbOptions {
    DbOptions {
        slow_query_ms: 0,
        log_queries: false,
        ..temp.options()
    }
}

#[tokio::test]
async fn every_statement_is_reported_without_bound_values() {
    let temp = TempDb::new();
    let recorder = TelemetryRecorder::new();
    let telemetry = Telemetry::with_sinks(vec![Arc::new(recorder.clone())]);
    let db = Db::connect_with_telemetry(&quiet(&temp), telemetry)
        .await
        .expect("connects");

    db.conn()
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT ? AS answer",
            ["secret-binding".into()],
        ))
        .await
        .expect("query runs");
    let failed = db
        .conn()
        .query_one_raw(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT * FROM no_such_table",
        ))
        .await;
    assert!(failed.is_err());

    let queries = recorder.queries();
    let answer = queries
        .iter()
        .find(|q| q.sql == "SELECT ? AS answer")
        .expect("the query is reported");
    assert!(!answer.failed);
    assert_eq!(answer.rows, None);
    assert!(answer.connection.starts_with("sqlite://"), "{}", answer.connection);
    assert!(
        queries.iter().all(|q| !q.sql.contains("secret-binding")),
        "{queries:?}"
    );
    let missing = queries
        .iter()
        .find(|q| q.sql.contains("no_such_table"))
        .expect("failed statements are reported too");
    assert!(missing.failed);
}

#[tokio::test]
async fn a_plain_connection_reports_nothing() {
    let temp = TempDb::new();
    let recorder = TelemetryRecorder::new();
    let db = Db::connect(&quiet(&temp)).await.expect("connects");
    db.conn()
        .query_one_raw(Statement::from_string(DbBackend::Sqlite, "SELECT 1"))
        .await
        .unwrap();
    assert!(recorder.is_empty());
}

/// `GET /api/answer`: one query through the request's unit of work.
struct AnswerController;

impl Injectable for AnswerController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(AnswerController) })
    }
}

impl Controller for AnswerController {
    fn prefix(&self) -> &'static str {
        "/answer"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "/", |uow: Uow| async move {
                let tx = uow.tx().await?;
                tx.query_one_raw(Statement::from_string(DbBackend::Sqlite, "SELECT 42"))
                    .await?;
                drop(tx);
                Ok::<_, AppError>("42")
            })
            .public(),
        ]
    }
}

struct AnswerModule;

impl Module for AnswerModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<AnswerController>();
    }
}

#[tokio::test]
async fn queries_in_a_request_carry_its_request_id() {
    let temp = TempDb::new();
    let recorder = TelemetryRecorder::new();
    let app = Navree::app()
        .module(HttpModule::for_root(HttpOptions {
            per_ip_rate_limit: None,
            ..HttpOptions::default()
        }))
        .module(DatabaseModule::for_root(quiet(&temp)))
        .module(AnswerModule)
        .module(recorder.module())
        .boot()
        .await
        .expect("boots");
    let router = build_router(app.ctx()).await.expect("router");

    let response = router
        .oneshot(Request::get("/api/answer").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let request_id = response.headers()["x-request-id"].to_str().unwrap().to_owned();

    let query = recorder
        .of_kind(EventKind::QueryExecuted)
        .into_iter()
        .find(|record| matches!(&record.event, navree_telemetry::TelemetryEvent::QueryExecuted(q) if q.sql == "SELECT 42"))
        .expect("the handler's query");
    assert_eq!(query.context.request_id.as_deref(), Some(request_id.as_str()));
    assert!(query.context.trace.is_some());
    app.shutdown().await.unwrap();
}
