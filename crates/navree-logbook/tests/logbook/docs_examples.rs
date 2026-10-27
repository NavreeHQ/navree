//! The examples of `tools/logbook.mdx`, in crate terms (the facade versions
//! are in `crates/navree/tests/logbook_docs.rs`).

use std::sync::Arc;

use http::Method;
use navree_db::{DatabaseModule, Uow};
use navree_http::{AppError, Controller, Json, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_logbook::{Logbook, LogbookModule};
use navree_testing::{TestApp, principal};
use sea_orm::{EntityTrait, QueryOrder};
use serde_json::json;
use uuid::Uuid;

use crate::app::{FixturesModule, deal};

/// The deals board, loaded in one query, and the same board loading each
/// deal one at a time (the loop Logbook's duplicate detection points at).
struct Board;

impl Injectable for Board {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Board) })
    }
}

impl Controller for Board {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "!/deals/board", |uow: Uow| async move {
                let tx = uow.tx().await?;
                let deals = deal::Entity::find()
                    .order_by_asc(deal::Column::Title)
                    .all(&*tx)
                    .await?;
                Ok::<_, AppError>(Json(json!({ "deals": deals.len() })))
            })
            .authenticated(),
            RouteDef::on(
                Method::GET,
                "!/deals/board-one-by-one",
                |uow: Uow| async move {
                    let tx = uow.tx().await?;
                    let ids: Vec<Uuid> = deal::Entity::find()
                        .all(&*tx)
                        .await?
                        .into_iter()
                        .map(|d| d.id)
                        .collect();
                    for id in &ids {
                        deal::Entity::find_by_id(*id).one(&*tx).await?;
                    }
                    Ok::<_, AppError>(Json(json!({ "deals": ids.len() })))
                },
            )
            .authenticated(),
        ]
    }
}

struct DealsModule;

impl Module for DealsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<Board>();
    }
}

async fn seed_board(app: &TestApp, deals: usize) {
    for n in 0..deals {
        app.client()
            .post("/deals")
            .json(&json!({ "title": format!("Deal {n}") }))
            .send()
            .await
            .assert_status(201);
    }
}

fn rep_id() -> Uuid {
    Uuid::from_u128(7)
}

#[tokio::test]
async fn the_board_loads_in_three_queries() {
    let app = TestApp::builder()
        .temp_database()
        .config("logbook.enabled", true)
        .config("logbook.store", "memory")
        .config("http.per_ip_rate_limit", serde_json::Value::Null)
        .module(DatabaseModule::from_config())
        .module(LogbookModule::from_config())
        .module(FixturesModule)
        .module(DealsModule)
        .build()
        .await;
    seed_board(&app, 20).await;

    app.acting_as(principal(rep_id()))
        .get("/deals/board")
        .send()
        .await
        .assert_status(200);

    let batch = app.get::<Logbook>().await.last_request().await.unwrap();
    assert!(batch.queries().len() <= 3, "{:#?}", batch.queries());
    assert!(batch.duplicate_queries().is_empty());
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn loading_one_at_a_time_is_flagged_as_duplicate_queries() {
    let app = TestApp::builder()
        .temp_database()
        .config("logbook.enabled", true)
        .config("logbook.store", "memory")
        .config("http.per_ip_rate_limit", serde_json::Value::Null)
        .module(DatabaseModule::from_config())
        .module(LogbookModule::from_config())
        .module(FixturesModule)
        .module(DealsModule)
        .build()
        .await;
    seed_board(&app, 6).await;
    app.acting_as(principal(rep_id()))
        .get("/deals/board-one-by-one")
        .send()
        .await
        .assert_status(200);
    let batch = app.get::<Logbook>().await.last_request().await.unwrap();
    let duplicates = batch.duplicate_queries();
    assert_eq!(duplicates.len(), 1, "{:#?}", batch.queries());
    assert_eq!(duplicates[0].count, 6);
    assert_eq!(batch.request().unwrap().summary.duplicate_queries, 1);
    // The list marks the batch.
    let html = crate::ui::unescape(&app.client().get("/_logbook").send().await.text());
    assert!(html.contains("repeated queries"), "{html}");
    app.shutdown().await.unwrap();
}

#[test]
fn leaving_the_module_out_of_release_builds() {
    let mut modules: Vec<&str> = vec!["config", "deals"];
    if cfg!(debug_assertions) {
        modules.push("logbook");
    }
    assert_eq!(modules.contains(&"logbook"), cfg!(debug_assertions));
}
