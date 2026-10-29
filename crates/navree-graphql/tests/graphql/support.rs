//! Shared fixtures: a `posts` table with access rules, resolvers over it
//! (one per module), an author dataloader, a recording audit sink and the
//! test app builder.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_graphql::futures_util::{Stream, StreamExt};
use async_graphql::{ComplexObject, InputObject, SimpleObject};
use garde::Validate;
use navree_access::{
    AbilityCtx, AccessModule, AccessOptions, AccessService, READ, Read, Rules, UPDATE, Update,
    field,
};
use navree_contracts::{AuditEntry, AuditSink, BoxError, BoxFuture};
use navree_db::navree_schema::{Migration, Schema, SchemaMigration, SchemaResult};
use navree_db::{DatabaseModule, Db, DbError, MigrationSet, ModuleBuilderDbExt, Uow};
use navree_events::{Event, EventBus, EventsModule};
use navree_graphql::{
    Context, ContextExt, EventStream, GraphqlModule, Loader, ModuleBuilderGraphqlExt, Result,
};
use navree_http::{AppError, ModuleBuilderHttpExt};
use navree_kernel::ModuleBuilder;
use navree_macros::{injectable, module, resolver};
use navree_orm::Model as _;
use navree_orm::access::AccessibleQuery as _;
use navree_testing::{TestApp, TestAppBuilder};
use navree_validation::Valid;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

/// The role that passes every check.
pub const SUPER_ADMIN: &str = "super-admin";

/// The `posts` model: the access subject `Post` (permissions `posts.*`).
pub mod post {
    use serde::Serialize;
    use uuid::Uuid;

    #[derive(Clone, Debug, Default, PartialEq, Eq, navree_orm::Model, Serialize)]
    #[model(table = "posts", subject = "Post", permission_prefix = "posts")]
    pub struct Post {
        pub id: Uuid,
        pub author_id: Option<Uuid>,
        pub title: String,
        pub status: String,
        pub published: bool,
    }
}

pub use post::Post;

/// Creates `posts`.
pub struct CreatePosts;

impl Migration for CreatePosts {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .create("posts", |t| {
                t.uuid("id").primary();
                t.uuid("author_id").nullable();
                t.string("title");
                t.string("status");
                t.boolean("published");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists("posts").await
    }
}

/// The rules: guests read published posts; users also read and update
/// their own (title and status only).
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
}

pub fn access_options() -> AccessOptions {
    AccessOptions {
        abilities,
        super_admin_role: Some(SUPER_ADMIN.to_owned()),
    }
}

/// A post, as the schema shows it.
#[derive(Clone, Debug, SimpleObject)]
#[graphql(name = "Post", complex)]
pub struct PostObject {
    pub id: Uuid,
    pub title: String,
    pub status: String,
    pub published: bool,
    #[graphql(skip)]
    pub author_id: Option<Uuid>,
}

impl From<Post> for PostObject {
    fn from(post: Post) -> Self {
        Self {
            id: post.id,
            title: post.title,
            status: post.status,
            published: post.published,
            author_id: post.author_id,
        }
    }
}

#[ComplexObject]
impl PostObject {
    /// The author, through the operation's dataloader.
    async fn author(&self, ctx: &Context<'_>) -> Result<Option<Author>> {
        match self.author_id {
            Some(id) => Ok(ctx.loader::<AuthorLoader>().load_one(id).await?),
            None => Ok(None),
        }
    }
}

/// An author.
#[derive(Clone, Debug, SimpleObject)]
pub struct Author {
    pub id: Uuid,
    pub name: String,
}

/// Loads authors by id, recording every batch.
#[injectable]
pub struct AuthorLoader {
    #[inject(default)]
    batches: Mutex<Vec<Vec<Uuid>>>,
}

impl AuthorLoader {
    /// The batches loaded so far.
    pub fn batches(&self) -> Vec<Vec<Uuid>> {
        self.batches.lock().unwrap().clone()
    }
}

impl Loader<Uuid> for AuthorLoader {
    type Value = Author;
    type Error = Arc<DbError>;

    async fn load(&self, ids: &[Uuid]) -> std::result::Result<HashMap<Uuid, Author>, Self::Error> {
        let mut sorted = ids.to_vec();
        sorted.sort();
        self.batches.lock().unwrap().push(sorted);
        Ok(ids
            .iter()
            .map(|id| {
                (
                    *id,
                    Author {
                        id: *id,
                        name: format!("author {}", &id.to_string()[..8]),
                    },
                )
            })
            .collect())
    }
}

/// The input of `renamePost`.
#[derive(Debug, InputObject, Validate)]
pub struct RenameInput {
    #[garde(length(chars, min = 2, max = 50))]
    pub title: String,
    #[garde(range(min = 0))]
    pub position: Option<i64>,
    #[garde(length(min = 1))]
    pub tag_names: Option<Vec<String>>,
}

/// Emitted when a post is created.
#[derive(Clone, Debug, Serialize, Deserialize, SimpleObject)]
pub struct PostCreated {
    pub id: Uuid,
    pub title: String,
}

impl Event for PostCreated {
    const NAME: &'static str = "post.created";
}

/// An error type the posts module maps to a 409 with `map_error`.
#[derive(Debug)]
pub struct TitleTaken;

impl std::fmt::Display for TitleTaken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("title taken")
    }
}

impl std::error::Error for TitleTaken {}

/// The posts resolver: every kind of access declaration, units of work,
/// validation, errors and a subscription.
#[injectable]
pub struct PostResolver {
    #[inject(default)]
    prefix: String,
}

#[resolver]
impl PostResolver {
    /// A greeting anyone may read.
    #[query]
    #[public]
    async fn hello(&self) -> String {
        format!("{}hello", self.prefix)
    }

    /// The posts the viewer may read, by title.
    #[query]
    #[public]
    async fn posts(&self, ctx: &Context<'_>) -> Result<Vec<PostObject>> {
        let uow = ctx.uow()?;
        let readable = ctx.accessible::<Post, Read>().await?;
        let posts = readable.query().order_by(Post::TITLE).get(&uow).await?;
        Ok(posts.into_models().into_iter().map(Into::into).collect())
    }

    /// The viewer's user id.
    #[query]
    #[authenticated]
    async fn me(&self, ctx: &Context<'_>) -> Result<Uuid> {
        Ok(ctx.principal()?.user_id)
    }

    /// The viewer's user id, or null with an error for guests.
    #[query]
    #[authenticated]
    async fn whoami(&self, ctx: &Context<'_>) -> Result<Option<Uuid>> {
        Ok(Some(ctx.principal()?.user_id))
    }

    /// One post, checked against its record.
    #[query]
    #[can(Read, Post, from = "id")]
    async fn post(&self, ctx: &Context<'_>, id: Uuid) -> Result<PostObject> {
        let post = ctx.authorized::<Post, Read>()?.into_inner();
        assert_eq!(post.id, id);
        Ok(post.into_inner().into())
    }

    /// Admins only.
    #[query]
    #[roles("admin")]
    async fn admin_area(&self) -> bool {
        true
    }

    /// Holders of `posts.publish` only.
    #[query]
    #[permissions("posts.publish")]
    async fn can_publish(&self) -> bool {
        true
    }

    /// Whether the viewer may read posts at all (a type-level check).
    #[query]
    #[authenticated]
    async fn reads_posts(&self, ctx: &Context<'_>) -> Result<bool> {
        Ok(ctx.abilities().await?.can("read", "Post", None))
    }

    /// Fails in the way `kind` names.
    #[query]
    #[public]
    async fn failing(&self, kind: String) -> Result<String> {
        match kind.as_str() {
            "missing" => Err(DbError::not_found("post").into()),
            "conflict" => Err(AppError::conflict("That title is taken.").into()),
            "forbidden" => Err(AppError::forbidden().into()),
            "boom" => Err(std::io::Error::other("the disk is on fire").into()),
            "mapped" => Err(TitleTaken.into()),
            "box" => {
                let error: BoxError = "boxed failure".into();
                Err(error.into())
            }
            _ => Err(async_graphql::Error::new("Pick another kind")),
        }
    }

    /// The authorized record without a `from` check: a server error.
    #[query]
    #[authenticated]
    async fn misdeclared(&self, ctx: &Context<'_>) -> Result<String> {
        Ok(ctx.authorized::<Post, Read>()?.title.clone())
    }

    /// Renames a post the viewer may update.
    #[mutation]
    #[can(Update, Post, from = "id")]
    async fn rename_post(
        &self,
        ctx: &Context<'_>,
        id: Uuid,
        input: Valid<RenameInput>,
    ) -> Result<PostObject> {
        let Valid(RenameInput { title, .. }) = input;
        let mut post = ctx.authorized::<Post, Update>()?.into_inner();
        let uow = ctx.uow()?;
        let fail = title == "fail-after-write";
        post.title = title;
        post.save(&uow).await?;
        assert_eq!(post.id, id);
        if fail {
            return Err(async_graphql::Error::new("failed after writing"));
        }
        Ok(post.into_inner().into())
    }

    /// Creates a post owned by the viewer and announces it after commit.
    #[mutation]
    #[authenticated]
    async fn create_post(&self, ctx: &Context<'_>, title: String) -> Result<PostObject> {
        let uow = ctx.uow()?;
        assert!(Uow::current().is_some_and(|current| current.same_as(&uow)));
        let me = ctx.principal()?.user_id;
        let post = Post::create(
            &uow,
            Post {
                id: Uuid::now_v7(),
                author_id: Some(me),
                title,
                status: "draft".to_owned(),
                published: false,
            },
        )
        .await?
        .into_inner();
        let bus = ctx.app()?.get::<EventBus>().await?;
        let event = PostCreated {
            id: post.id,
            title: post.title.clone(),
        };
        uow.after_commit(move || {
            Box::pin(async move {
                let _ = bus.emit(event).await;
            })
        });
        Ok(post.into())
    }

    /// New posts, as they are created.
    #[subscription]
    #[can(Read, Post)]
    async fn post_created(
        &self,
        ctx: &Context<'_>,
        #[graphql(desc = "Only titles starting with this")] prefix: Option<String>,
    ) -> Result<impl Stream<Item = PostCreated>> {
        let events = EventStream::<PostCreated>::new(ctx)?;
        Ok(events.filter(move |event| {
            let keep = prefix
                .as_ref()
                .is_none_or(|prefix| event.title.starts_with(prefix.as_str()));
            std::future::ready(keep)
        }))
    }

    /// The ids of new posts, for admins.
    #[subscription]
    #[roles("admin")]
    async fn moderation_feed(&self, ctx: &Context<'_>) -> Result<impl Stream<Item = Uuid>> {
        Ok(EventStream::<PostCreated>::new(ctx)?.map(|event| event.id))
    }
}

/// A second resolver, in another module, with impl-level access.
#[injectable]
pub struct StatsResolver {
    db: Arc<Db>,
}

#[resolver]
#[public]
impl StatsResolver {
    /// How many posts exist.
    #[query]
    async fn post_count(&self) -> Result<u64> {
        Ok(Post::query().count(&*self.db).await?)
    }

    /// Which field resolves the viewer (public: a guest gets `None`).
    #[query]
    async fn viewer(&self, ctx: &Context<'_>) -> Option<Uuid> {
        ctx.principal().ok().map(|principal| principal.user_id)
    }
}

/// The posts feature.
#[module(register = Self::wire)]
pub struct PostsModule;

impl PostsModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.import(AccessModule::for_feature::<Post>())
            .migrations(MigrationSet::new("posts").migration(SchemaMigration::new(
                "m20261026_000001_create_posts",
                CreatePosts,
            )))
            .resolver::<PostResolver>()
            .dataloader::<AuthorLoader>()
            .map_error::<TitleTaken>(|_| Some(AppError::conflict("That title is taken.")));
    }
}

/// The stats feature.
#[module(register = Self::wire)]
pub struct StatsModule;

impl StatsModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.resolver::<StatsResolver>();
    }
}

/// An `AuditSink` that keeps every entry.
#[derive(Clone, Default)]
pub struct RecordingAudit(Arc<Mutex<Vec<AuditEntry>>>);

impl RecordingAudit {
    pub fn entries(&self) -> Vec<AuditEntry> {
        self.0.lock().unwrap().clone()
    }
}

impl AuditSink for RecordingAudit {
    fn record(&self, entry: AuditEntry) -> BoxFuture<'_, std::result::Result<(), BoxError>> {
        self.0.lock().unwrap().push(entry);
        Box::pin(async { Ok(()) })
    }
}

/// The test app builder: a temporary database, events, access, GraphQL
/// (from config) and both features.
pub fn builder(audit: &RecordingAudit) -> TestAppBuilder {
    TestApp::builder()
        .temp_database()
        .config("http.per_ip_rate_limit", Value::Null)
        .config("graphql.introspection", true)
        .config("graphql.graphiql", true)
        .module(DatabaseModule)
        .module(EventsModule::for_root())
        .module(AccessModule::for_root(access_options()))
        .module(GraphqlModule::from_config())
        .module(PostsModule)
        .module(StatsModule)
        .override_value::<dyn AuditSink>(Arc::new(audit.clone()))
}

/// A booted test app.
pub async fn app() -> (TestApp, RecordingAudit) {
    let audit = RecordingAudit::default();
    let app = builder(&audit).build().await;
    (app, audit)
}

/// Inserts a post.
pub async fn insert_post(
    app: &TestApp,
    title: &str,
    author: Option<Uuid>,
    published: bool,
) -> Post {
    let db = app.get::<Db>().await;
    Post::create(
        &*db,
        Post {
            id: Uuid::now_v7(),
            author_id: author,
            title: title.to_owned(),
            status: "draft".to_owned(),
            published,
        },
    )
    .await
    .expect("insert post")
    .into_inner()
}

/// The post `id` as stored.
pub async fn stored(app: &TestApp, id: Uuid) -> Post {
    Post::find(&*app.get::<Db>().await, id)
        .await
        .unwrap()
        .expect("the post exists")
        .into_inner()
}

/// Gives `user` the role `role` with `permissions`.
pub async fn give_role(app: &TestApp, user: Uuid, role: &str, permissions: &[&str]) {
    let db = app.get::<Db>().await;
    let service = app.get::<AccessService>().await;
    let uow = db.uow(None, None).await;
    if service.role(role).await.unwrap().is_none() {
        service.create_role(&uow, role, None).await.unwrap();
    }
    for permission in permissions {
        service.grant(&uow, role, permission).await.unwrap();
    }
    service.assign_role(&uow, user, role).await.unwrap();
    uow.commit().await.unwrap();
}

/// A GraphQL request body.
pub fn query(query: &str, variables: Value) -> Value {
    json!({ "query": query, "variables": variables })
}
