//! keel-access interop: `keel_auth::user::Entity` is the CASL subject
//! `User` (permission prefix `users`), so apps can load users for
//! `can(.., "User", from = "id")` with `AccessModule::for_feature`.

use std::sync::Arc;

use axum::http::Method;
use keel_access::{
    AbilityCtx, AccessModule, AccessOptions, AccessService, Authorized, READ, Read, Rules, Subject,
    field,
};
use keel_auth::{USER_PERMISSION_PREFIX, USER_SUBJECT, UserResource, user};
use keel_contracts::BoxFuture;
use keel_db::Db;
use keel_http::{Controller, Json, ModuleBuilderHttpExt, RouteDef};
use keel_kernel::{Injectable, KernelError, Module, ModuleBuilder, Resolver};
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

async fn show(person: Authorized<user::Entity, Read>) -> Json<UserResource> {
    Json(UserResource::from(person.into_inner()))
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
        m.import(AccessModule::for_feature::<user::Entity>())
            .controller::<PeopleController>();
    }
}

#[test]
fn the_user_entity_is_the_user_subject() {
    assert_eq!(<user::Entity as Subject>::SUBJECT, "User");
    assert_eq!(<user::Entity as Subject>::PERMISSION_PREFIX, "users");
    assert_eq!(<user::Entity as Subject>::SUBJECT, USER_SUBJECT);
    assert_eq!(
        <user::Entity as Subject>::PERMISSION_PREFIX,
        USER_PERMISSION_PREFIX
    );
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
