//! navree-access interop: the `User` model is the CASL subject `User`
//! (permission prefix `users`), so apps can load users for
//! `can(.., "User", from = "id")` with `AccessModule::for_feature`:
//! through the ORM, in the request's unit of work, on either kind of unit
//! of work.

use std::sync::Arc;

use axum::http::Method;
use navree_access::{
    AbilityCtx, AccessModule, AccessOptions, AccessService, Authorized, READ, Read, Rules, Subject,
    UPDATE, Update, field,
};
use navree_auth::{CurrentUser, USER_PERMISSION_PREFIX, USER_SUBJECT, User, UserResource, user};
use navree_contracts::BoxFuture;
use navree_db::Db;
use navree_http::{Controller, Json, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{Injectable, KernelError, Module, ModuleBuilder, Resolver};
use serde_json::json;
use uuid::Uuid;

use crate::support::{self, signed_in};

/// Users read their own record; permissions (`users.read`) add the rest.
fn abilities(ctx: &AbilityCtx, rules: &mut Rules) {
    ctx.grant_permissions(rules);
    if let Some(user) = &ctx.user {
        rules.can(READ, USER_SUBJECT).when(field("id").eq(user.id));
    }
}

/// `GET /api/people/{id}`: a user record, checked with `can(read, User)`.
struct PeopleController;

impl Injectable for PeopleController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(PeopleController) })
    }
}

async fn show(person: Authorized<User, Read>) -> Json<UserResource> {
    Json(UserResource::from(person.into_inner().into_inner()))
}

impl Controller for PeopleController {
    fn prefix(&self) -> &'static str {
        "/people"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![RouteDef::on(Method::GET, "/{id}", show).can(READ, USER_SUBJECT, Some("id"))]
    }
}

struct PeopleModule;

impl Module for PeopleModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.import(AccessModule::for_feature::<User>())
            .controller::<PeopleController>();
    }
}

#[test]
fn the_user_model_is_the_user_subject() {
    assert_eq!(<User as Subject>::SUBJECT, "User");
    assert_eq!(<User as Subject>::PERMISSION_PREFIX, "users");
    assert_eq!(<User as Subject>::SUBJECT, USER_SUBJECT);
    assert_eq!(<User as Subject>::PERMISSION_PREFIX, USER_PERMISSION_PREFIX);
    // COMPAT: the SeaORM entity names the same subject.
    assert_eq!(<user::Entity as Subject>::SUBJECT, USER_SUBJECT);
}

#[tokio::test]
async fn users_load_for_can_checks_and_users_permissions_apply() {
    let app = support::builder()
        .module(AccessModule::for_root(AccessOptions {
            abilities,
            super_admin_role: None,
        }))
        .module(PeopleModule)
        .build()
        .await;
    let (ada, access, _) = signed_in(&app, "ada@example.com").await;
    let grace = support::create_user(&app, "grace@example.com").await;
    let client = app.client();
    let get = |id: Uuid| {
        client
            .get(&format!("/api/people/{id}"))
            .bearer(&access)
            .send()
    };

    get(ada.id)
        .await
        .assert_status(200)
        .assert_json_path("__type", json!("User"))
        .assert_json_path("email", json!("ada@example.com"));
    get(grace.id).await.assert_error(403, None);
    get(Uuid::now_v7()).await.assert_error(404, None);

    // `users.read` is about the `User` subject (prefix `users`).
    let db = app.get::<Db>().await;
    let access_service = app.get::<AccessService>().await;
    let uow = db.uow(None, None).await;
    access_service
        .create_role(&uow, "manager", None)
        .await
        .unwrap();
    access_service
        .grant(&uow, "manager", "users.read")
        .await
        .unwrap();
    access_service
        .assign_role(&uow, ada.id, "manager")
        .await
        .unwrap();
    uow.commit().await.unwrap();
    get(grace.id)
        .await
        .assert_status(200)
        .assert_json_path("email", json!("grace@example.com"));
}

/// `PATCH /api/people/{id}`: a record check (which begins the request's
/// transaction) plus `CurrentUser`.
struct EditorController;

impl Injectable for EditorController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(EditorController) })
    }
}

async fn edit(
    person: Authorized<User, Update>,
    CurrentUser(me): CurrentUser,
) -> Json<serde_json::Value> {
    Json(json!({ "person": person.into_inner().id, "me": me.id }))
}

impl Controller for EditorController {
    fn prefix(&self) -> &'static str {
        "/people"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![RouteDef::on(Method::PATCH, "/{id}", edit).can(UPDATE, USER_SUBJECT, Some("id"))]
    }
}

struct EditorModule;

impl Module for EditorModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.import(AccessModule::for_feature::<User>())
            .controller::<EditorController>();
    }
}

/// Users update their own record.
fn self_editing(ctx: &AbilityCtx, rules: &mut Rules) {
    if let Some(user) = &ctx.user {
        rules
            .can(UPDATE, USER_SUBJECT)
            .when(field("id").eq(user.id));
    }
}

/// Once the record check has begun the request's transaction (on SQLite:
/// holding the write lock on one pool connection), `CurrentUser` reads
/// through it instead of waiting for a second connection. With a pool of
/// one, waiting would only end at the acquire timeout; under load it
/// starves the pool.
#[tokio::test]
async fn current_user_after_a_record_check_needs_no_second_connection() {
    let app = support::builder()
        .config("database.max_connections", 1)
        .config("database.connect_timeout_secs", 1)
        .module(AccessModule::for_root(AccessOptions {
            abilities: self_editing,
            super_admin_role: None,
        }))
        .module(EditorModule)
        .build()
        .await;
    let (ada, access, _) = signed_in(&app, "ada@example.com").await;
    let started = std::time::Instant::now();
    app.client()
        .patch(&format!("/api/people/{}", ada.id))
        .bearer(&access)
        .json(&json!({}))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("me", json!(ada.id));
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
}
