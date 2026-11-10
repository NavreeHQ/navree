//! Workspace scoping: every read and write path of a workspace-scoped model
//! tried from one workspace against another's records, the current
//! workspace's sources, the explicit opt-outs, and binding.

use std::sync::Arc;

use http::Method;
use http::request::Parts;
use navree_db::{Db, Uow};
use navree_http::extract::FromRequestParts;
use navree_http::{AppError, Controller, Json, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_orm::binding::Bound;
use navree_orm::factory::{HasMany, Sequence};
use navree_orm::testing::assert_database_count;
use navree_orm::{Model, OrmError, Record};
use serde_json::json;
use uuid::Uuid;

use crate::models::{Project, Task};
use crate::support::{app, sqlite};

/// Two workspaces with two projects each (each with two tasks).
struct Two {
    a: Uuid,
    b: Uuid,
    a_projects: Vec<Record<Project>>,
    b_projects: Vec<Record<Project>>,
}

async fn two_workspaces(db: &Db) -> Two {
    let a = Uuid::now_v7();
    let b = Uuid::now_v7();
    let mut made = Vec::new();
    for (workspace, label) in [(a, "A"), (b, "B")] {
        let uow = db.uow(None, None).await;
        uow.set_workspace(Some(workspace));
        let projects = Project::factory()
            .count(2)
            .sequence(Sequence::from_fn(move |project: &mut Project, at| {
                project.name = format!("{label} project {}", at.index() + 1);
            }))
            .has(Task::factory().count(2), HasMany::new(Task::PROJECT_ID))
            .create(&uow)
            .await
            .unwrap();
        uow.commit().await.unwrap();
        made.push(projects.into_vec());
    }
    let b_projects = made.pop().unwrap();
    let a_projects = made.pop().unwrap();
    Two {
        a,
        b,
        a_projects,
        b_projects,
    }
}

/// A unit of work in `workspace`.
async fn uow_in(db: &Db, workspace: Uuid) -> Uow {
    let uow = db.uow(None, None).await;
    uow.set_workspace(Some(workspace));
    uow
}

#[tokio::test]
async fn new_records_get_the_current_workspace() {
    let test = sqlite().await;
    let two = two_workspaces(&test.db).await;
    assert!(two.a_projects.iter().all(|p| p.workspace_id == two.a));
    assert!(two.b_projects.iter().all(|p| p.workspace_id == two.b));
    let tasks = Task::query()
        .without_workspace_scope()
        .where_(Task::PROJECT_ID.eq(two.a_projects[0].id))
        .get(&test.db)
        .await
        .unwrap();
    assert!(
        tasks.iter().all(|t| t.workspace_id == two.a),
        "children too"
    );

    // Bulk inserts fill it as well.
    let uow = uow_in(&test.db, two.a).await;
    assert_eq!(Project::factory().count(3).insert(&uow).await.unwrap(), 3);
    uow.commit().await.unwrap();
    assert_eq!(
        Project::query()
            .for_workspace(two.a)
            .count(&test.db)
            .await
            .unwrap(),
        5
    );
}

#[tokio::test]
async fn reads_see_the_current_workspace_only() {
    let test = sqlite().await;
    let two = two_workspaces(&test.db).await;
    let uow = uow_in(&test.db, two.a).await;
    let b_key = two.b_projects[0].id;
    let a_key = two.a_projects[0].id;

    let all = Project::query().get(&uow).await.unwrap();
    assert_eq!(all.len(), 2);
    assert!(all.iter().all(|p| p.workspace_id == two.a));
    assert_eq!(Project::all(&uow).await.unwrap().len(), 2);
    assert!(Project::find(&uow, b_key).await.unwrap().is_none());
    assert!(Project::find(&uow, a_key).await.unwrap().is_some());
    assert!(
        Project::find_or_fail(&uow, b_key)
            .await
            .unwrap_err()
            .is_not_found()
    );
    assert_eq!(
        Project::find_many(&uow, [a_key, b_key])
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(
        Project::query()
            .where_key(b_key)
            .first(&uow)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        Project::query()
            .where_key(b_key)
            .doesnt_exist(&uow)
            .await
            .unwrap()
    );
    assert_eq!(Project::query().count(&uow).await.unwrap(), 2);
    let a_budget: i64 = two.a_projects.iter().map(|p| p.budget).sum();
    assert_eq!(
        Project::query()
            .sum::<i64>(&uow, Project::BUDGET)
            .await
            .unwrap(),
        a_budget
    );
    let names = Project::query().pluck(&uow, Project::NAME).await.unwrap();
    assert_eq!(names.len(), 2);
    assert!(
        Project::query()
            .where_key(b_key)
            .value(&uow, Project::NAME)
            .await
            .unwrap()
            .is_none()
    );
    let page = Project::query().paginate(&uow, (1, 10)).await.unwrap();
    assert_eq!(page.meta.total, 2);
    let simple = Project::query()
        .simple_paginate(&uow, (1, 10))
        .await
        .unwrap();
    assert_eq!(simple.data.len(), 2);
    assert_eq!(Task::query().count(&uow).await.unwrap(), 4);
}

#[tokio::test]
async fn subqueries_stay_inside_the_workspace() {
    let test = sqlite().await;
    let two = two_workspaces(&test.db).await;
    let uow = uow_in(&test.db, two.a).await;
    // Subqueries take the workspace of the task's unit of work.
    uow.scope(async {
        let with_tasks = Project::query()
            .where_in_sub(Project::ID, Task::query().select([Task::PROJECT_ID]))
            .count(&uow)
            .await
            .unwrap();
        assert_eq!(with_tasks, 2);
        // A subquery of another model scoped like the outer query: tasks of
        // workspace B never satisfy an EXISTS from workspace A.
        let b_project = two.b_projects[0].id;
        let reachable = Project::query()
            .without_workspace_scope()
            .where_exists(Task::query().where_(Task::PROJECT_ID.eq(b_project)))
            .count(&uow)
            .await
            .unwrap();
        assert_eq!(reachable, 0);
    })
    .await;
}

#[tokio::test]
async fn writes_never_reach_another_workspaces_records() {
    let test = sqlite().await;
    let two = two_workspaces(&test.db).await;
    let uow = uow_in(&test.db, two.a).await;

    // A record of B, loaded on purpose, cannot be saved, incremented or
    // deleted from A: it does not exist here.
    let mut theirs = Project::query()
        .for_workspace(two.b)
        .first(&uow)
        .await
        .unwrap()
        .unwrap();
    theirs.name = "Hijacked".into();
    assert!(theirs.save(&uow).await.unwrap_err().is_not_found());
    theirs.discard_changes();
    assert!(
        theirs
            .increment(&uow, Project::BUDGET, 1)
            .await
            .unwrap_err()
            .is_not_found()
    );
    let mut theirs = Project::query()
        .for_workspace(two.b)
        .first(&uow)
        .await
        .unwrap()
        .unwrap();
    assert!(theirs.delete(&uow).await.unwrap_err().is_not_found());
    assert_eq!(Project::destroy(&uow, [theirs.id]).await.unwrap(), 0);

    // A record cannot move to another workspace.
    let mut mine = Project::query().first(&uow).await.unwrap().unwrap();
    mine.workspace_id = two.b;
    let moved = mine.save(&uow).await.unwrap_err();
    assert!(matches!(moved, OrmError::Workspace { .. }), "{moved}");
    assert_eq!(
        moved.to_string(),
        "the workspace of a Project cannot change (`workspace_id` was changed)"
    );

    // Nor be created in another one.
    let foreign = Project::create(
        &uow,
        Project {
            workspace_id: two.b,
            name: "Foreign".into(),
            ..Project::default()
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(foreign, OrmError::Workspace { .. }), "{foreign}");

    // Mass writes touch the current workspace's rows only, and cannot
    // move rows.
    let renamed = Project::query()
        .update(&uow, [Project::NAME.set("Renamed")])
        .await
        .unwrap();
    assert_eq!(renamed, 2);
    assert_eq!(
        Project::query()
            .increment(&uow, Project::BUDGET, 10)
            .await
            .unwrap(),
        2
    );
    let error = Project::query()
        .update(&uow, [Project::WORKSPACE_ID.set(two.b)])
        .await
        .unwrap_err();
    assert!(matches!(error, OrmError::Workspace { .. }), "{error}");
    assert_eq!(Task::query().delete(&uow).await.unwrap(), 4);
    uow.commit().await.unwrap();

    assert_eq!(
        Project::query()
            .for_workspace(two.b)
            .where_(Project::NAME.eq("Renamed"))
            .count(&test.db)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        Task::query()
            .for_workspace(two.b)
            .count(&test.db)
            .await
            .unwrap(),
        4
    );
    assert_database_count(&test.db, "tasks", 4).await;
}

#[tokio::test]
async fn find_or_create_stays_inside_the_workspace() {
    let test = sqlite().await;
    let two = two_workspaces(&test.db).await;
    let b_name = two.b_projects[0].name.clone();
    let uow = uow_in(&test.db, two.a).await;
    let created = Project::first_or_create(&uow, [Project::NAME.set(b_name.clone())], |_| {})
        .await
        .unwrap();
    assert!(
        created.was_recently_created(),
        "B's project is not found from A"
    );
    assert_eq!(created.workspace_id, two.a);
    let updated = Project::update_or_create(&uow, [Project::NAME.set(b_name)], |p| p.budget = 1)
        .await
        .unwrap();
    assert_eq!(updated.id, created.id);
}

#[tokio::test]
async fn the_current_workspace_has_several_sources() {
    let test = sqlite().await;
    let two = two_workspaces(&test.db).await;
    let db = &test.db;

    // No workspace at all: the query fails instead of reading everything.
    let error = Project::query().count(db).await.unwrap_err();
    assert!(
        error.to_string().contains(
            "Project is scoped to workspaces, but there is no current workspace to query it in"
        ),
        "{error}"
    );
    let error = Project::create(db, Project::default()).await.unwrap_err();
    assert!(
        error
            .to_string()
            .contains("no current workspace to create it in"),
        "{error}"
    );

    // The executor's unit of work.
    let in_b = uow_in(db, two.b).await;
    assert_eq!(Project::query().count(&in_b).await.unwrap(), 2);
    // The task's unit of work, through the pool.
    let counted = in_b
        .scope(async { Project::query().count(db).await.unwrap() })
        .await;
    assert_eq!(counted, 2);
    // An explicit scope wins.
    let explicit = navree_orm::workspace::scope(two.a, async {
        assert_eq!(navree_orm::workspace::current(), Some(two.a));
        Project::query().get(&in_b).await.unwrap()
    })
    .await;
    assert!(explicit.iter().all(|p| p.workspace_id == two.a));
    // (SQLite: release the unit of work's write lock before the pool writes.)
    in_b.rollback().await.unwrap();
    // Explicit opt-outs.
    assert_eq!(
        Project::query()
            .without_workspace_scope()
            .count(db)
            .await
            .unwrap(),
        4
    );
    assert_eq!(
        Project::query()
            .for_workspace(two.b)
            .count(db)
            .await
            .unwrap(),
        2
    );
    // Creating with an explicit workspace needs no current one.
    let system = Project::create(
        db,
        Project {
            workspace_id: two.b,
            name: "System".into(),
            ..Project::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(system.workspace_id, two.b);
    // A model that is not workspace-scoped refuses for_workspace().
    let error = crate::models::Tag::query()
        .for_workspace(two.a)
        .count(db)
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("Tag is not scoped to workspaces"),
        "{error}"
    );
}

/// Enters the workspace named by `X-Workspace` on the request's unit of
/// work (what navree-workspace's CurrentWorkspace does).
struct TestWorkspace;

impl FromRequestParts for TestWorkspace {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, AppError> {
        let workspace = parts
            .headers
            .get("x-workspace")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| Uuid::parse_str(value).ok())
            .ok_or_else(|| AppError::bad_request("no workspace"))?;
        let uow = parts
            .extensions
            .get::<Uow>()
            .ok_or_else(|| AppError::internal("no unit of work"))?;
        uow.set_workspace(Some(workspace));
        Ok(TestWorkspace)
    }
}

struct Routes;

impl Injectable for Routes {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Routes) })
    }
}

impl Controller for Routes {
    fn prefix(&self) -> &'static str {
        "/projects"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(
                Method::GET,
                "/{project}",
                |_: TestWorkspace, project: Bound<Project>| async move {
                    Json(json!({ "name": project.name }))
                },
            )
            .public(),
            RouteDef::on(Method::POST, "/", |_: TestWorkspace, uow: Uow| async move {
                let project = Project::factory().create_one(&uow).await?;
                Ok::<_, AppError>(Json(json!({ "workspace_id": project.workspace_id })))
            })
            .public(),
        ]
    }
}

struct RoutesModule;

impl Module for RoutesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<Routes>();
    }
}

#[tokio::test]
async fn binding_another_workspaces_record_is_a_404() {
    let (app, db, _clock) = app(vec![Box::new(RoutesModule)]).await;
    let two = two_workspaces(&db).await;
    let theirs = &two.b_projects[0];
    app.client()
        .get(&format!("/api/projects/{}", theirs.id))
        .header("x-workspace", &two.a.to_string())
        .send()
        .await
        .assert_error(404, "Not Found");
    app.client()
        .get(&format!("/api/projects/{}", theirs.id))
        .header("x-workspace", &two.b.to_string())
        .send()
        .await
        .assert_status(200)
        .assert_json_path("name", json!(theirs.name));
    app.client()
        .post("/api/projects")
        .header("x-workspace", &two.a.to_string())
        .send()
        .await
        .assert_status(200)
        .assert_json_path("workspace_id", json!(two.a));
    assert_eq!(
        <Project as Model>::meta().workspace_column(),
        Some("workspace_id")
    );
}
