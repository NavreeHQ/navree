//! Shared fixtures: a `posts` record with its controller, an abilities
//! function, a recording audit sink and helpers to seed roles and posts.

#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use http::Method;
use navree_access::{
    AbilityCtx, AccessModule, AccessOptions, AccessService, Accessible, Authorized, DELETE, READ,
    Read, Rules, Subject, SubjectRecords, TypedColumns, UPDATE, Update, field, row_json,
};
use navree_contracts::{AuditEntry, AuditSink, BoxError, BoxFuture};
use navree_db::navree_schema::{Dialect, Migration, Schema, SchemaMigration, SchemaResult};
use navree_db::record::{self, Filter};
use navree_db::{Db, Executor, Kind, MigrationSet, ModuleBuilderDbExt, Record, Uow};
use navree_http::{AppError, Controller, Json, ModuleBuilderHttpExt, Path, RouteDef};
use navree_kernel::{Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_testing::{TestApp, TestAppBuilder};
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

/// The role that passes every check in the test apps.
pub const SUPER_ADMIN: &str = "super-admin";

navree_db::record! {
    /// A post (`posts`).
    #[derive(Clone, Debug, PartialEq, Eq, Serialize)]
    pub struct Post in "posts" {
        pub id: Uuid,
        pub author_id: Option<Uuid>,
        pub title: String,
        pub status: String,
        pub published: bool,
        pub created_at: DateTime<Utc>,
    }
}

impl Subject for Post {
    const SUBJECT: &'static str = "Post";
    const PERMISSION_PREFIX: &'static str = "posts";
}

impl Post {
    /// The columns of `posts` for list filters.
    pub fn columns() -> TypedColumns {
        TypedColumns::new(Post::TABLE)
            .with("id", Kind::Uuid)
            .with("author_id", Kind::Uuid)
            .with("title", Kind::String)
            .with("status", Kind::String)
            .with("published", Kind::Bool)
            .with("created_at", Kind::Timestamp)
    }
}

/// Posts are loaded by id, in the request's unit of work.
impl SubjectRecords for Post {
    type Record = Post;

    fn load<'a>(uow: &'a Uow, id: &'a str) -> BoxFuture<'a, Result<Option<Post>, BoxError>> {
        Box::pin(async move {
            let Ok(id) = Uuid::parse_str(id) else {
                return Ok(None);
            };
            let mut select = Post::select().eq("id", id);
            if uow.lock_for_update() {
                select = select.for_update();
            }
            Ok(select.one(uow).await?)
        })
    }

    fn record_json(record: &Post) -> Value {
        row_json(record)
    }
}

/// Creates `posts`.
pub struct CreatePosts;

impl Migration for CreatePosts {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let mysql = schema.target().await?.dialect() == Dialect::MySql;
        schema
            .create("posts", |t| {
                if mysql {
                    t.binary("id").length(16).fixed().primary();
                    t.binary("author_id").length(16).fixed().nullable();
                } else {
                    t.uuid("id").primary();
                    t.uuid("author_id").nullable();
                }
                t.string("title");
                t.string("status");
                t.boolean("published");
                t.timestamp_tz("created_at");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop("posts").await
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
    fn construct(_r: Resolver) -> navree_kernel::BoxFuture<'static, Result<Self, KernelError>> {
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

async fn index(uow: Uow, posts: Accessible<Post, Read>) -> navree_http::Result<Json<Value>> {
    let (sql, values) = posts
        .sql_filter(&Post::columns())
        .to_statement(uow.backend())
        .into_parts();
    let rows = Post::select()
        .filter(Filter::all().raw(sql, values))
        .order_by("title")
        .all(&uow)
        .await
        .map_err(AppError::internal)?;
    let titles: Vec<String> = rows.into_iter().map(|p| p.title).collect();
    Ok(Json(json!({ "titles": titles })))
}

async fn show(post: Authorized<Post, Read>) -> Json<Post> {
    Json(post.into_inner())
}

async fn update(
    uow: Uow,
    post: Authorized<Post, Update>,
    Json(input): Json<serde_json::Map<String, Value>>,
) -> navree_http::Result<Json<Post>> {
    let changed: Vec<&str> = input.keys().map(String::as_str).collect();
    post.check_fields(&changed)?;
    let mut model = post.into_inner();
    if let Some(title) = input.get("title").and_then(Value::as_str) {
        title.clone_into(&mut model.title);
    }
    if let Some(status) = input.get("status").and_then(Value::as_str) {
        status.clone_into(&mut model.status);
    }
    if let Some(author) = input.get("author_id") {
        model.author_id = serde_json::from_value(author.clone())
            .map_err(|error| AppError::bad_request(error.to_string()))?;
    }
    record::update(&uow, &model)
        .await
        .map_err(AppError::internal)?;
    Ok(Json(model))
}

async fn destroy(_post: Authorized<Post, navree_access::Delete>) -> http::StatusCode {
    http::StatusCode::NO_CONTENT
}

async fn publish(Path(id): Path<Uuid>) -> Json<Value> {
    Json(json!({ "published": id }))
}

/// The posts feature: its table, its subject loader and its routes.
pub struct PostsModule;

impl Module for PostsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.import(AccessModule::for_feature::<Post>())
            .migrations(MigrationSet::new("posts").migration(SchemaMigration::new(
                "m20261025_000001_create_posts",
                CreatePosts,
            )))
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

/// The test app builder: a database of its own (see [`crate::database`]),
/// the access module with [`options`], the posts feature and a recording
/// audit sink.
pub fn builder(audit: &RecordingAudit) -> TestAppBuilder {
    crate::database::database(TestApp::builder())
        .config("http.per_ip_rate_limit", Value::Null)
        .module(navree_db::DatabaseModule)
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
    Fut: Future<Output = Result<T, navree_access::AccessError>>,
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

/// Inserts a post; the post as stored.
pub async fn insert_post(
    app: &TestApp,
    title: &str,
    author: Option<Uuid>,
    status: &str,
    published: bool,
) -> Post {
    let db = app.get::<Db>().await;
    record::insert_returning(
        &*db,
        &Post {
            id: Uuid::now_v7(),
            author_id: author,
            title: title.to_owned(),
            status: status.to_owned(),
            published,
            created_at: timestamp("2026-10-25T12:00:00.250000Z"),
        },
    )
    .await
    .expect("insert post")
}

/// Parses an RFC 3339 timestamp.
pub fn timestamp(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
}
