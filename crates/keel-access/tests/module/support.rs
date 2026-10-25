//! Shared fixtures: a `posts` entity with its controller, an abilities
//! function, a recording audit sink and helpers to seed roles and posts.

#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use axum::extract::Path;
use chrono::{DateTime, Utc};
use http::Method;
use keel_access::{
    AbilityCtx, AccessModule, AccessOptions, AccessService, Accessible, Authorized, DELETE, READ,
    Read, Rules, Subject, UPDATE, Update, field,
};
use keel_contracts::{AuditEntry, AuditSink, BoxError, BoxFuture};
use keel_db::{Db, MigrationSet, ModuleBuilderDbExt, Uow};
use keel_http::{AppError, Controller, Json, ModuleBuilderHttpExt, RouteDef};
use keel_kernel::{Injectable, KernelError, Module, ModuleBuilder, Resolver};
use keel_testing::{TestApp, TestAppBuilder};
use sea_orm::sea_query::{ColumnDef, Table};
use sea_orm::{ActiveModelTrait, EntityTrait, QueryOrder, Set};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};
use serde_json::{Value, json};
use uuid::Uuid;

/// The role that passes every check in the test apps.
pub const SUPER_ADMIN: &str = "super-admin";

/// The `posts` entity.
pub mod post {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "posts")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        pub author_id: Option<Uuid>,
        pub title: String,
        pub status: String,
        pub published: bool,
        pub created_at: DateTimeUtc,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

impl Subject for post::Entity {
    const SUBJECT: &'static str = "Post";
    const PERMISSION_PREFIX: &'static str = "posts";
}

/// Creates `posts`.
pub struct CreatePosts;

impl MigrationName for CreatePosts {
    fn name(&self) -> &str {
        "m20261025_000001_create_posts"
    }
}

#[async_trait]
impl MigrationTrait for CreatePosts {
    async fn up(&self, manager: &SchemaManager) -> Result<(), sea_orm::DbErr> {
        manager
            .create_table(
                Table::create()
                    .table("posts")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("author_id").uuid().null())
                    .col(ColumnDef::new("title").string().not_null())
                    .col(ColumnDef::new("status").string().not_null())
                    .col(ColumnDef::new("published").boolean().not_null())
                    .col(
                        ColumnDef::new("created_at")
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), sea_orm::DbErr> {
        manager
            .drop_table(Table::drop().table("posts").to_owned())
            .await
    }
}

/// The rules of the test apps:
///
/// - guests read published posts;
/// - users get their database permissions, read published posts and their
///   own, update the title and status of their own posts, delete their own
///   unless locked, and may `publish` their own posts (a conditional rule,
///   so the `posts.publish` permission route still needs the permission).
pub fn abilities(ctx: &AbilityCtx, rules: &mut Rules) {
    ctx.grant_permissions(rules);
    rules.can(READ, "Post").when(field("published").eq(true));
    let Some(user) = &ctx.user else {
        return;
    };
    rules.can(READ, "Post").when(field("author_id").eq(user.id));
    rules
        .can(UPDATE, "Post")
        .fields(["title", "status"])
        .when(field("author_id").eq(user.id));
    rules
        .can(DELETE, "Post")
        .when(field("author_id").eq(user.id));
    rules
        .can("publish", "Post")
        .when(field("author_id").eq(user.id));
    rules
        .cannot(DELETE, "Post")
        .when(field("status").eq("locked"))
        .because("locked posts cannot be deleted");
}

/// The access options of the test apps.
pub fn options() -> AccessOptions {
    AccessOptions {
        abilities,
        super_admin_role: Some(SUPER_ADMIN.to_owned()),
    }
}

/// Routes exercising every kind of access declaration.
pub struct PostController;

impl Injectable for PostController {
    fn construct(_r: Resolver) -> keel_kernel::BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(PostController) })
    }
}

impl Controller for PostController {
    fn prefix(&self) -> &'static str {
        "/posts"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "/", index).can(READ, "Post", None),
            RouteDef::on(Method::GET, "/{id}", show).can(READ, "Post", Some("id")),
            RouteDef::on(Method::PATCH, "/{id}", update).can(UPDATE, "Post", Some("id")),
            RouteDef::on(Method::DELETE, "/{id}", destroy).can(DELETE, "Post", Some("id")),
            RouteDef::on(Method::POST, "/{id}/publish", publish).permissions(&["posts.publish"]),
            RouteDef::on(Method::POST, "/{id}/archive", || async { "archived" }).can(
                "archive",
                "Post",
                Some("id"),
            ),
            RouteDef::on(Method::GET, "!/admin/dashboard", || async { "admin area" })
                .roles(&["admin", "owner"]),
            // `reports` is no registered subject: the permission stands alone.
            RouteDef::on(Method::GET, "!/reports/sales", || async { "sales report" })
                .permissions(&["reports.view"]),
        ]
    }
}

async fn index(uow: Uow, posts: Accessible<post::Entity, Read>) -> keel_http::Result<Json<Value>> {
    let select = posts.apply(post::Entity::find().order_by_asc(post::Column::Title));
    let rows = select.all(&*uow.tx().await?).await?;
    let titles: Vec<String> = rows.into_iter().map(|p| p.title).collect();
    Ok(Json(json!({ "titles": titles })))
}

async fn show(post: Authorized<post::Entity, Read>) -> Json<post::Model> {
    Json(post.into_inner())
}

async fn update(
    uow: Uow,
    post: Authorized<post::Entity, Update>,
    Json(input): Json<serde_json::Map<String, Value>>,
) -> keel_http::Result<Json<post::Model>> {
    let changed: Vec<&str> = input.keys().map(String::as_str).collect();
    post.check_fields(&changed)?;
    let mut model: post::ActiveModel = post.into_inner().into();
    if let Some(title) = input.get("title").and_then(Value::as_str) {
        model.title = Set(title.to_owned());
    }
    if let Some(status) = input.get("status").and_then(Value::as_str) {
        model.status = Set(status.to_owned());
    }
    if let Some(author) = input.get("author_id") {
        model.author_id = Set(serde_json::from_value(author.clone())
            .map_err(|error| AppError::bad_request(error.to_string()))?);
    }
    let updated = model.update(&*uow.tx().await?).await?;
    Ok(Json(updated))
}

async fn destroy(_post: Authorized<post::Entity, keel_access::Delete>) -> http::StatusCode {
    http::StatusCode::NO_CONTENT
}

async fn publish(Path(id): Path<Uuid>) -> Json<Value> {
    Json(json!({ "published": id }))
}

/// The posts feature: its table, its subject loader and its routes.
pub struct PostsModule;

impl Module for PostsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.import(AccessModule::for_feature::<post::Entity>())
            .migrations(MigrationSet::new("posts").migration(CreatePosts))
            .controller::<PostController>();
    }
}

/// An `AuditSink` that keeps every entry.
#[derive(Clone, Default)]
pub struct RecordingAudit(Arc<Mutex<Vec<AuditEntry>>>);

impl RecordingAudit {
    pub fn entries(&self) -> Vec<AuditEntry> {
        self.0.lock().unwrap().clone()
    }

    pub fn actions(&self) -> Vec<String> {
        self.entries().into_iter().map(|e| e.action).collect()
    }
}

impl AuditSink for RecordingAudit {
    fn record(&self, entry: AuditEntry) -> BoxFuture<'_, Result<(), BoxError>> {
        self.0.lock().unwrap().push(entry);
        Box::pin(async { Ok(()) })
    }
}

/// The test app builder: a temporary database, the access module with
/// [`options`], the posts feature and a recording audit sink.
pub fn builder(audit: &RecordingAudit) -> TestAppBuilder {
    TestApp::builder()
        .temp_database()
        .config("http.per_ip_rate_limit", Value::Null)
        .module(keel_db::DatabaseModule)
        .module(AccessModule::for_root(options()))
        .module(PostsModule)
        .override_value::<dyn AuditSink>(Arc::new(audit.clone()))
}

/// A booted test app (see [`builder`]).
pub async fn app() -> (TestApp, RecordingAudit) {
    let audit = RecordingAudit::default();
    let app = builder(&audit).build().await;
    (app, audit)
}

/// Runs `f` in a committed unit of work.
pub async fn in_uow<F, Fut, T>(app: &TestApp, f: F) -> T
where
    F: FnOnce(Arc<AccessService>, Uow) -> Fut,
    Fut: Future<Output = Result<T, keel_access::AccessError>>,
{
    let db = app.get::<Db>().await;
    let service = app.get::<AccessService>().await;
    let uow = db.uow(None, None).await;
    let value = f(service, uow.clone()).await.expect("access change");
    uow.commit().await.expect("commit");
    value
}

/// Creates `role` (if needed) with `permissions` and gives it to `user`.
pub async fn give_role(app: &TestApp, user: Uuid, role: &str, permissions: &[&str]) {
    let role = role.to_owned();
    let permissions: Vec<String> = permissions.iter().map(|p| (*p).to_owned()).collect();
    in_uow(app, |service, uow| async move {
        if service.role(&role).await?.is_none() {
            service.create_role(&uow, &role, None).await?;
        }
        for permission in &permissions {
            service.grant(&uow, &role, permission).await?;
        }
        service.assign_role(&uow, user, &role).await?;
        Ok(())
    })
    .await;
}

/// Inserts a post.
pub async fn insert_post(
    app: &TestApp,
    title: &str,
    author: Option<Uuid>,
    status: &str,
    published: bool,
) -> post::Model {
    let db = app.get::<Db>().await;
    post::ActiveModel {
        id: Set(Uuid::now_v7()),
        author_id: Set(author),
        title: Set(title.to_owned()),
        status: Set(status.to_owned()),
        published: Set(published),
        created_at: Set(timestamp("2026-10-25T12:00:00.250000Z")),
    }
    .insert(db.conn())
    .await
    .expect("insert post")
}

/// Parses an RFC 3339 timestamp.
pub fn timestamp(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
}
