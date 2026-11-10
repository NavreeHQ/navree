//! Actions: abilities per record, input fields, rollbacks, queued runs and
//! the audit log.

use std::sync::Arc;

use navree_audit::AuditModule;
use navree_contracts::{Dispatcher, Job, JobContext};
use navree_helm::{Action, ActionResult, Field, HelmActionJob, ModuleBuilderHelmExt, Resource};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_orm::{Model, Record};
use navree_queue::QueueFake;
use serde_json::json;
use uuid::Uuid;

use crate::support::*;

#[tokio::test]
async fn actions_run_on_the_records_the_user_may_act_on() {
    let app = app().await;
    let (rep_id, rep) = user(&app, "rep").await;
    let own = deal(
        &app,
        DealSeed {
            title: "Own",
            owner: Some(rep_id),
            ..DealSeed::default()
        },
    )
    .await;
    let other = deal(
        &app,
        DealSeed {
            title: "Other",
            owner: Some(Uuid::now_v7()),
            ..DealSeed::default()
        },
    )
    .await;

    rep.post("/admin/api/resources/deals/actions/mark-won")
        .json(&json!({ "ids": [own.id, other.id, own.id, "not-an-id"] }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("type", json!("message"))
        .assert_json_path("message", json!("1 deals marked as won."))
        .assert_json_path("ran", json!(1))
        .assert_json_path("skipped", json!(2))
        .assert_json_path("queued", json!(false));
    assert_eq!(reload(&app, own.id).await.unwrap().stage, "won");
    assert_eq!(reload(&app, other.id).await.unwrap().stage, "lead");

    rep.post("/admin/api/resources/deals/actions/mark-won")
        .json(&json!({ "ids": [other.id] }))
        .send()
        .await
        .assert_error(403, "You may not run this action on the selected records.");
    rep.post("/admin/api/resources/deals/actions/mark-won")
        .json(&json!({ "ids": [] }))
        .send()
        .await
        .assert_validation_error("ids");
    rep.post("/admin/api/resources/deals/actions/teleport")
        .json(&json!({ "ids": [own.id] }))
        .send()
        .await
        .assert_error(404, None);
}

#[tokio::test]
async fn action_fields_are_validated_and_read_by_the_handler() {
    let app = app().await;
    let (rep_id, rep) = user(&app, "rep").await;
    let own = deal(
        &app,
        DealSeed {
            owner: Some(rep_id),
            ..DealSeed::default()
        },
    )
    .await;
    rep.post("/admin/api/resources/deals/actions/mark-lost")
        .json(&json!({ "ids": [own.id] }))
        .send()
        .await
        .assert_status(400)
        .assert_json_path("errors.lost_reason", json!(["Reason should not be empty"]));
    rep.post("/admin/api/resources/deals/actions/mark-lost")
        .json(&json!({ "ids": [own.id], "input": { "lost_reason": "Budget", "other": 1 } }))
        .send()
        .await
        .assert_validation_error("other");
    rep.post("/admin/api/resources/deals/actions/mark-lost")
        .json(&json!({ "ids": [own.id], "input": { "lost_reason": "Budget" } }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("message", json!("Deals marked as lost."));
    let saved = reload(&app, own.id).await.unwrap();
    assert_eq!(saved.stage, "lost");
    assert_eq!(saved.lost_reason.as_deref(), Some("Budget"));
}

#[tokio::test]
async fn a_failing_action_rolls_back_its_changes() {
    let app = app().await;
    let (_, manager) = user(&app, "manager").await;
    let deal = deal(
        &app,
        DealSeed {
            title: "Locked",
            ..DealSeed::default()
        },
    )
    .await;
    manager
        .post("/admin/api/resources/deals/actions/fail")
        .json(&json!({ "ids": [deal.id] }))
        .send()
        .await
        .assert_error(409, "This deal is locked.");
    assert_eq!(reload(&app, deal.id).await.unwrap().title, "Locked");
}

#[tokio::test]
async fn queued_actions_become_helm_action_jobs() {
    let fake = QueueFake::new();
    let app = builder()
        .override_value::<dyn Dispatcher>(fake.dispatcher())
        .build()
        .await;
    let (manager_id, manager) = user(&app, "manager").await;
    let first = deal(
        &app,
        DealSeed {
            title: "First",
            ..DealSeed::default()
        },
    )
    .await;
    manager
        .post("/admin/api/resources/deals/actions/export")
        .json(&json!({ "ids": [first.id] }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("type", json!("message"))
        .assert_json_path(
            "message",
            json!("Export is running in the background on 1 record."),
        )
        .assert_json_path("queued", json!(true));
    // Nothing ran yet.
    assert_eq!(
        reload(&app, first.id).await.unwrap().notes.as_deref(),
        Some("**Secret** notes")
    );
    let jobs = fake.dispatched::<HelmActionJob>();
    assert_eq!(
        jobs,
        [HelmActionJob {
            resource: "deals".into(),
            action: "export".into(),
            ids: vec![first.id.to_string()],
            input: Default::default(),
            user_id: manager_id,
            workspace: None,
        }]
    );
    assert_eq!(HelmActionJob::NAME, "helm.action");

    // The worker runs it as the user.
    let job = jobs.into_iter().next().unwrap();
    job.handle(JobContext {
        app: app.ctx().clone(),
        job_id: "1".into(),
        attempt: 1,
    })
    .await
    .unwrap();
    assert_eq!(
        reload(&app, first.id).await.unwrap().notes,
        Some(format!("exported by {manager_id}"))
    );
}

#[tokio::test]
async fn queued_actions_recheck_access_when_they_run() {
    let app = app().await;
    let (rep_id, _) = user(&app, "rep").await;
    let other = deal(
        &app,
        DealSeed {
            owner: Some(Uuid::now_v7()),
            ..DealSeed::default()
        },
    )
    .await;
    let job = HelmActionJob {
        resource: "deals".into(),
        action: "mark-won".into(),
        ids: vec![other.id.to_string()],
        input: Default::default(),
        user_id: rep_id,
        workspace: None,
    };
    let error = job
        .handle(JobContext {
            app: app.ctx().clone(),
            job_id: "2".into(),
            attempt: 1,
        })
        .await
        .unwrap_err();
    assert!(error.to_string().contains("403"), "{error}");
    assert_eq!(reload(&app, other.id).await.unwrap().stage, "lead");
}

#[tokio::test]
async fn without_a_queue_queued_actions_run_in_the_request() {
    let app = app().await;
    let (manager_id, manager) = user(&app, "manager").await;
    let deal = deal(&app, DealSeed::default()).await;
    manager
        .post("/admin/api/resources/deals/actions/export")
        .json(&json!({ "ids": [deal.id] }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("type", json!("download"))
        .assert_json_path("url", json!("/files/deals.csv"))
        .assert_json_path("queued", json!(false));
    assert_eq!(
        reload(&app, deal.id).await.unwrap().notes,
        Some(format!("exported by {manager_id}"))
    );
}

#[tokio::test]
async fn every_action_run_is_audited() {
    let app = builder()
        .config("audit.routes", false)
        .module(AuditModule::from_config())
        .build()
        .await;
    let (rep_id, rep) = user(&app, "rep").await;
    let own = deal(
        &app,
        DealSeed {
            owner: Some(rep_id),
            ..DealSeed::default()
        },
    )
    .await;
    rep.post("/admin/api/resources/deals/actions/mark-won")
        .json(&json!({ "ids": [own.id] }))
        .send()
        .await
        .assert_status(200);
    app.get::<navree_audit::AuditLog>().await.flush().await;
    let rows = navree_audit::AuditLog::list(
        &*app.get::<navree_audit::AuditLog>().await,
        &navree_audit::AuditFilter::default(),
        navree_db::PageQuery {
            page: 1,
            per_page: 100,
        },
    )
    .await
    .unwrap()
    .data;
    let entry = rows
        .iter()
        .find(|r| r.action == "helm.action")
        .expect("helm.action");
    assert_eq!(entry.actor_id, Some(rep_id));
    assert_eq!(entry.kind, "custom");
    assert_eq!(entry.subject_type.as_deref(), Some("Deal"));
    assert_eq!(
        entry.subject_id.as_deref(),
        Some(own.id.to_string().as_str())
    );
    assert_eq!(entry.metadata["action"], "mark-won");
    assert_eq!(entry.metadata["resource"], "deals");
    assert_eq!(entry.metadata["ids"], json!([own.id.to_string()]));
    // The change the action made is audited too.
    assert!(
        rows.iter()
            .any(|r| r.action == "deal.updated" && r.actor_id == Some(rep_id))
    );
    let _ = Arc::new(());
}

/// Deals that are read-only in the panel, with actions of each kind of ability.
struct ReadOnlyDeals;

impl Injectable for ReadOnlyDeals {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(ReadOnlyDeals) })
    }
}

impl Resource for ReadOnlyDeals {
    type Model = Deal;

    const LABEL: &'static str = "Read-only deals";
    const SLUG: &'static str = "ro-deals";
    const TITLE: &'static str = "title";
    const READ_ONLY: bool = true;
    const SIDEBAR_COUNT: bool = false;

    fn fields(&self) -> Vec<Field> {
        vec![Field::text("title").required()]
    }

    fn actions(&self) -> Vec<Action<Deal>> {
        vec![
            Action::new("touch", "Touch").handle(|ctx, deals: Vec<Record<Deal>>| async move {
                for mut deal in deals {
                    deal.update(ctx.uow(), |d| d.title = "TOUCHED".to_owned())
                        .await?;
                }
                Ok(ActionResult::message("touched"))
            }),
            Action::new("zap", "Zap")
                .ability(navree_access::Delete)
                .handle(|ctx, deals: Vec<Record<Deal>>| async move {
                    for deal in deals {
                        Deal::query()
                            .where_(Deal::ID.eq(deal.model().id))
                            .delete(ctx.uow())
                            .await?;
                    }
                    Ok(ActionResult::message("zapped"))
                }),
            Action::new("peek", "Peek")
                .ability(navree_access::Read)
                .handle(|_ctx, _deals: Vec<Record<Deal>>| async move {
                    Ok(ActionResult::message("peeked"))
                }),
        ]
    }
}

struct ReadOnlyModule;

impl Module for ReadOnlyModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.resource::<ReadOnlyDeals>();
    }
}

#[tokio::test]
async fn a_read_only_resource_refuses_an_action_that_writes_however_it_is_posted() {
    let app = builder().module(ReadOnlyModule).build().await;
    let d = deal(
        &app,
        DealSeed {
            title: "Keep",
            ..DealSeed::default()
        },
    )
    .await;
    let (_, manager) = user(&app, "manager").await;
    let id = d.id.to_string();
    for name in ["touch", "zap"] {
        let page = manager
            .post("/admin/resources/ro-deals/actions")
            .header("accept", "text/html")
            .form(&[("action", name), ("ids", id.as_str()), ("_confirmed", "1")])
            .send()
            .await;
        assert_eq!(page.status(), 303, "back with the reason: {name}");
        assert_eq!(page.header("location"), Some("/admin/resources/ro-deals"));
        let api = manager
            .post(&format!("/admin/api/resources/ro-deals/actions/{name}"))
            .json(&json!({ "ids": [id] }))
            .send()
            .await;
        assert_eq!(api.status(), 403, "{name}");
        assert_eq!(api.json_value()["code"], "read_only_resource", "{name}");
    }
    assert_eq!(reload(&app, d.id).await.unwrap().title, "Keep");
    // One that only reads runs, and is offered.
    let peek = manager
        .post("/admin/api/resources/ro-deals/actions/peek")
        .json(&json!({ "ids": [id] }))
        .send()
        .await;
    assert_eq!(peek.status(), 200, "{}", peek.text());
    let list = manager
        .get("/admin/resources/ro-deals")
        .header("accept", "text/html")
        .send()
        .await
        .text();
    assert!(list.contains("value=\"peek\""), "{list}");
    assert!(!list.contains("value=\"touch\""), "{list}");
    assert!(!list.contains("value=\"zap\""), "{list}");
}
