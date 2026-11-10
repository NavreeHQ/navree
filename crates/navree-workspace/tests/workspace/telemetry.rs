//! Telemetry: a request that resolved its workspace reports it with
//! `RequestHandled` (and every event of the request carries it).

use std::sync::Arc;

use http::Method;
use navree_http::{Controller, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_telemetry::TelemetryRecorder;
use navree_workspace::CurrentWorkspace;

use crate::support::{World, builder};

/// `GET /api/slug`: the resolved workspace's slug.
struct SlugController;

impl Injectable for SlugController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(SlugController) })
    }
}

impl Controller for SlugController {
    fn prefix(&self) -> &'static str {
        "/slug"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "/", |ws: CurrentWorkspace| async move {
                ws.slug.clone()
            })
            .authenticated(),
        ]
    }
}

struct SlugModule;

impl Module for SlugModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<SlugController>();
    }
}

#[tokio::test]
async fn requests_report_the_workspace_they_resolved() {
    let recorder = TelemetryRecorder::new();
    let world = World::with(builder().module(SlugModule).module(recorder.module())).await;
    let ada = world.user("Anu", "anu@example.com").await;
    let agra = world.workspace(ada, "Agra").await;

    world
        .as_user(ada)
        .get("/api/slug")
        .header("x-workspace", "agra")
        .send()
        .await
        .assert_status(200);

    let request = recorder
        .requests()
        .into_iter()
        .find(|r| r.path == "/api/slug")
        .expect("the request is reported");
    assert_eq!(request.workspace_id, Some(agra.id.to_string()));
    assert_eq!(request.user_id, Some(ada.to_string()));
}
