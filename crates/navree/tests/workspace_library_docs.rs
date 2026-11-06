//! The examples of `content/docs/workspaces/shared-libraries.mdx` (and the
//! refusal example of `content/docs/orm/events-and-observers.mdx`), written
//! as an app writes them (the facade and the prelude), compiled and run:
//! the migration, the model, the module, the routes, the seeder and the
//! Helm resource of the global library.
// The page's examples are written as an app writes them, without doc comments.
#![allow(missing_docs)]

use http::Method;
use navree::BoxFuture;
use navree::access::AccessOptions;
use navree::db::{Db, Seeder};
use navree::helm::ModuleBuilderHelmExt;
use navree::http::{Controller, RouteDef};
use navree::prelude::*;
use navree::schema::{Migration, Schema, SchemaMigration, SchemaResult};
use navree::testing::{TestApp, TestAppBuilder, principal};
use navree::workspace::library::{
    LibraryColumns, LibraryOwner, LibraryRepo, LibraryViewer, LibraryVisibility, unguarded,
};
use navree::workspace::{NewOrganization, NewWorkspace, Organizations, Workspaces};
use serde_json::{Value, json};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// "The migration".

pub struct CreateTemplates;

impl Migration for CreateTemplates {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let library = LibraryColumns::new(schema.target().await?.dialect());
        schema
            .create("templates", |t| {
                library.uuid(t, "id").primary();
                library.add(t); // scope, organization_id, workspace_id, cloned_from_id
                t.string("slug");
                t.string("name");
                t.text("body");
                t.timestamp_tz("created_at");
                t.timestamp_tz("updated_at");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists("templates").await
    }
}

/// The blocks `Library::cloned` copies (the page's "Shaping the copy").
pub struct CreateTemplateBlocks;

impl Migration for CreateTemplateBlocks {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let library = LibraryColumns::new(schema.target().await?.dialect());
        schema
            .create("template_blocks", |t| {
                library.uuid(t, "id").primary();
                library.uuid(t, "template_id").index();
                t.text("body");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists("template_blocks").await
    }
}

// ---------------------------------------------------------------------------
// "The model", with "Shaping the copy".

pub mod model {
    use chrono::{DateTime, Utc};
    use navree::BoxFuture;
    use navree::db::Uow;
    use navree::orm::prelude::*;
    use navree::workspace::library::{Library, LibraryScope};
    use serde::Serialize;
    use uuid::Uuid;

    #[derive(Model, Clone, Debug, Default, Serialize)]
    #[model(
        table = "templates",
        timestamps,
        subject = "Template",
        permission_prefix = "templates"
    )]
    pub struct Template {
        pub id: Uuid,
        pub scope: LibraryScope,
        pub organization_id: Option<Uuid>,
        pub workspace_id: Option<Uuid>,
        pub cloned_from_id: Option<Uuid>,
        pub slug: String,
        pub name: String,
        pub body: String,
        pub created_at: DateTime<Utc>,
        pub updated_at: DateTime<Utc>,
    }

    #[derive(Model, Clone, Debug, Default, Serialize)]
    #[model(table = "template_blocks")]
    pub struct TemplateBlock {
        pub id: Uuid,
        pub template_id: Uuid,
        pub body: String,
    }

    impl Library for Template {
        fn cloning(&mut self, source: &Self) {
            self.name = format!("{} (copy)", source.name);
        }

        fn cloned<'a>(
            uow: &'a Uow,
            source: &'a Record<Self>,
            copy: &'a Record<Self>,
        ) -> BoxFuture<'a, navree::orm::Result<()>> {
            Box::pin(async move {
                let blocks = TemplateBlock::query()
                    .where_(TemplateBlock::TEMPLATE_ID.eq(source.id))
                    .get(uow)
                    .await?;
                for block in blocks {
                    let mut block = block.replicate();
                    block.template_id = copy.id;
                    block.save(uow).await?;
                }
                Ok(())
            })
        }
    }
}

use model::{Template, TemplateBlock};

// ---------------------------------------------------------------------------
// The routes: "Reading a library", "Writing items" and "Cloning".

#[derive(serde::Deserialize)]
pub struct RenameTemplate {
    pub name: String,
}

#[derive(serde::Deserialize)]
pub struct NewTemplate {
    pub slug: String,
    pub name: String,
}

async fn index(_ws: CurrentWorkspace, uow: Uow) -> Result<Json<Vec<Template>>> {
    let templates = Template::query().order_by(Template::NAME).get(&uow).await?;
    Ok(Json(
        templates.into_iter().map(Record::into_inner).collect(),
    ))
}

async fn store(
    _ws: CurrentWorkspace,
    uow: Uow,
    Json(input): Json<NewTemplate>,
) -> Result<Json<Template>> {
    let template = Template::create(
        &uow,
        Template {
            slug: input.slug,
            name: input.name,
            ..Template::default()
        },
    )
    .await?;
    Ok(Json(template.into_inner()))
}

/// Renames any template the workspace sees, through the model: the guard
/// decides.
async fn rename_any(
    _ws: CurrentWorkspace,
    uow: Uow,
    Path(id): Path<Uuid>,
    Json(input): Json<RenameTemplate>,
) -> Result<Json<Template>> {
    let mut template = Template::find_or_fail(&uow, id).await?;
    template.name = input.name;
    template.save(&uow).await?;
    Ok(Json(template.into_inner()))
}

async fn rename(
    ws: CurrentWorkspace,
    uow: Uow,
    Path(id): Path<Uuid>,
    Json(input): Json<RenameTemplate>,
) -> Result<Json<Template>> {
    let mut template = LibraryRepo::<Template>::find_editable_or_fail(&uow, &ws, id).await?;
    template.name = input.name;
    template.save(&uow).await?;
    Ok(Json(template.into_inner()))
}

async fn use_template(
    ws: CurrentWorkspace,
    uow: Uow,
    Path(id): Path<Uuid>,
) -> Result<Json<Template>> {
    let copy = LibraryRepo::<Template>::clone_into(&uow, id, ws.id()).await?;
    Ok(Json(copy.into_inner()))
}

async fn share(
    org: CurrentOrganization,
    uow: Uow,
    Json(input): Json<NewTemplate>,
) -> Result<Json<Template>> {
    org.authorize("templates.share")?;
    let template = LibraryRepo::<Template>::create(
        &uow,
        LibraryOwner::Organization(org.id()),
        Template {
            slug: input.slug,
            name: input.name,
            ..Template::default()
        },
    )
    .await?;
    Ok(Json(template.into_inner()))
}

/// What an organization's admins see, and the page's other reads.
async fn shared(org: CurrentOrganization, uow: Uow) -> Result<Json<Value>> {
    type Templates = LibraryRepo<Template>;

    let shared = Templates::visible_to(&org).get(&uow).await?;
    let own = Templates::owned_by(LibraryOwner::Organization(org.id()))
        .get(&uow)
        .await?;
    let catalog = Templates::global().get(&uow).await?;
    let names = |templates: Collection<Template>| {
        let mut names: Vec<String> = templates.into_iter().map(|t| t.name.clone()).collect();
        names.sort();
        names
    };
    Ok(Json(json!({
        "shared": names(shared),
        "own": names(own),
        "catalog": names(catalog),
    })))
}

/// Publishes a copy of a workspace's template to its organization
/// ("`unguarded`").
async fn publish(ws: CurrentWorkspace, uow: Uow, Path(id): Path<Uuid>) -> Result<Json<Template>> {
    let organization_id = ws
        .workspace()
        .organization_id
        .ok_or_else(|| AppError::bad_request("The workspace has no organization."))?;
    let published = unguarded(LibraryRepo::<Template>::clone_to(
        &uow,
        id,
        LibraryOwner::Organization(organization_id),
    ))
    .await?;
    Ok(Json(published.into_inner()))
}

#[injectable]
pub struct TemplatesController;

impl Controller for TemplatesController {
    fn prefix(&self) -> &'static str {
        "/templates"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let in_workspace = |route: RouteDef| route.authenticated().guard(InWorkspace::guard());
        vec![
            in_workspace(RouteDef::on(Method::GET, "/", index)),
            in_workspace(RouteDef::on(Method::POST, "/", store)),
            in_workspace(RouteDef::on(Method::PATCH, "/{id}", rename_any)),
            in_workspace(RouteDef::on(Method::PATCH, "/{id}/name", rename)),
            in_workspace(RouteDef::on(Method::POST, "/{id}/clone", use_template)),
            in_workspace(RouteDef::on(Method::POST, "/{id}/publish", publish)),
            RouteDef::on(Method::POST, "/shared", share)
                .authenticated()
                .guard(InOrganization::guard()),
            RouteDef::on(Method::GET, "/shared", shared)
                .authenticated()
                .guard(InOrganization::guard()),
        ]
    }
}

// ---------------------------------------------------------------------------
// "Global items from seeders".

#[injectable]
pub struct TemplateLibrarySeeder;

impl Seeder for TemplateLibrarySeeder {
    fn name(&self) -> &'static str {
        "template-library"
    }

    fn run<'a>(&'a self, _ctx: &'a AppContext, db: &'a Db) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            let items = [
                (
                    "welcome",
                    "Welcome aboard",
                    "Hello {{ name }}, welcome to the team.",
                ),
                (
                    "reset",
                    "Reset your password",
                    "Use this link to choose a new password.",
                ),
            ]
            .map(|(slug, name, body)| Template {
                slug: slug.into(),
                name: name.into(),
                body: body.into(),
                ..Template::default()
            });
            let seeded = LibraryRepo::<Template>::seed_global(db, "slug", items).await?;
            println!(
                "templates: {} created, {} updated",
                seeded.created, seeded.updated
            );
            Ok(())
        })
    }
}

// ---------------------------------------------------------------------------
// "Registering it".

pub struct TemplatesModule;

impl Module for TemplatesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(
            MigrationSet::new("templates")
                .after(["workspace"])
                .migration(SchemaMigration::new(
                    "m20261028_000001_create_templates",
                    CreateTemplates,
                )),
        )
        .library::<Template>()
        .controller::<TemplatesController>();
        m.migrations(
            MigrationSet::new("template_blocks")
                .after(["templates"])
                .migration(SchemaMigration::new(
                    "m20261028_000002_create_template_blocks",
                    CreateTemplateBlocks,
                )),
        )
        .seeder::<TemplateLibrarySeeder>();
    }
}

// ---------------------------------------------------------------------------
// "Managing global items in Helm".

pub mod helm {
    use navree::BoxFuture;
    use navree::helm::{Field, FormData, RecordOf, Resource, SaveContext};
    use navree::orm::Builder;
    use navree::prelude::*;
    use navree::workspace::library::{LibraryOwner, LibraryRepo, LibraryScope};

    use super::model::Template;

    #[derive(serde::Deserialize, navree::validation::Validate)]
    struct TemplateForm {
        #[validate(length(min = 1))]
        slug: String,
        #[validate(length(min = 1))]
        name: String,
        #[validate(skip)]
        body: Option<String>,
    }

    #[injectable]
    pub struct TemplateLibraryResource;

    impl Resource for TemplateLibraryResource {
        type Model = Template;
        const LABEL: &'static str = "Template library";
        const SINGULAR: &'static str = "Template";
        const SLUG: &'static str = "template-library";
        const TITLE: &'static str = "name";
        const SEARCH: &'static [&'static str] = &["name", "slug"];

        fn fields(&self) -> Vec<Field> {
            vec![
                Field::text("slug").required(),
                Field::text("name").required().sortable(),
                Field::textarea("body").hide_on_index(),
            ]
        }

        fn query(&self, query: Builder<Template>) -> Builder<Template> {
            query.where_(Template::SCOPE.eq(LibraryScope::Global))
        }

        fn create<'a>(
            &'a self,
            ctx: &'a SaveContext,
            form: FormData,
        ) -> BoxFuture<'a, Result<RecordOf<Self>, AppError>> {
            Box::pin(async move {
                let input: TemplateForm = form.parse().await?;
                let template = Template {
                    slug: input.slug,
                    name: input.name,
                    body: input.body.unwrap_or_default(),
                    ..Template::default()
                };
                Ok(
                    LibraryRepo::<Template>::create(ctx.uow(), LibraryOwner::Global, template)
                        .await?,
                )
            })
        }
    }
}

pub struct TemplateLibraryAdminModule;

impl Module for TemplateLibraryAdminModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.resource::<helm::TemplateLibraryResource>();
    }
}

// ---------------------------------------------------------------------------
// The refusal example of the ORM's events page.

pub mod deals {
    use navree::orm::hooks::Flow;
    use navree::orm::{ExecContext, Observer, OrmError};
    use navree::prelude::*;

    #[derive(Model, Clone, Debug, Default, serde::Serialize)]
    #[model(table = "won_deals")]
    pub struct Deal {
        pub id: uuid::Uuid,
        pub title: String,
        #[model(cast = "string_enum")]
        pub stage: Stage,
    }

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum Stage {
        #[default]
        Open,
        Won,
    }

    pub struct DealObserver;

    #[derive(Debug, thiserror::Error)]
    #[error("won deals are kept")]
    pub struct WonDealsAreKept;

    impl Observer<Deal> for DealObserver {
        async fn deleting(
            &self,
            deal: &mut Record<Deal>,
            _ctx: &ExecContext,
        ) -> navree::orm::Result<Flow> {
            if deal.stage == Stage::Won {
                return Err(OrmError::refused("Deal", WonDealsAreKept));
            }
            Ok(Flow::Continue)
        }
    }
}

// ---------------------------------------------------------------------------
// Running them.

fn builder() -> TestAppBuilder {
    TestApp::builder()
        .temp_database()
        .config("http.per_ip_rate_limit", Value::Null)
        .config("workspace.users_table", "")
        .config("workspace.organizations.enabled", true)
        .module(DatabaseModule::from_config())
        .module(EventsModule::for_root())
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(WorkspaceModule::from_config())
        .module(TemplatesModule)
}

/// Organization `group` with workspace `agra`, the standalone `gulati`,
/// all owned by `anu`.
async fn tenants(app: &TestApp, ada: Uuid) -> (Uuid, Uuid, Uuid) {
    let db = app.get::<Db>().await;
    let workspaces = app.get::<Workspaces>().await;
    let organizations = app.get::<Organizations>().await;
    let uow = db.uow(None, None).await;
    let group = organizations
        .create(&uow, ada, NewOrganization::named("Group"))
        .await
        .unwrap();
    let agra = workspaces
        .create(&uow, ada, NewWorkspace::named("Agra"))
        .await
        .unwrap();
    let gulati = workspaces
        .create(&uow, ada, NewWorkspace::named("Gulati"))
        .await
        .unwrap();
    organizations.attach(&uow, group.id, agra.id).await.unwrap();
    uow.commit().await.unwrap();
    (group.id, agra.id, gulati.id)
}

#[tokio::test]
async fn workspaces_read_shared_templates_and_clone_them_to_change_them() {
    let app = builder().build().await;
    app.run_command("db:seed", &["template-library"])
        .await
        .unwrap();
    let ada = Uuid::now_v7();
    let (group, agra, _gulati) = tenants(&app, ada).await;
    let bus = app.get::<navree::events::EventBus>().await;
    let mut cloned = bus.subscribe_json("library.item_cloned");
    let client = app.acting_as(principal(ada));

    // The organization shares a template with its workspaces.
    client
        .post("/api/templates/shared")
        .header("x-organization", "group")
        .json(&json!({ "slug": "handbook", "name": "Group handbook" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("scope", json!("organization"))
        .assert_json_path("organization_id", json!(group));
    client
        .get("/api/templates/shared")
        .header("x-organization", "group")
        .send()
        .await
        .assert_status(200)
        .assert_json_path(
            "shared",
            json!(["Group handbook", "Reset your password", "Welcome aboard"]),
        )
        .assert_json_path("own", json!(["Group handbook"]))
        .assert_json_path("catalog", json!(["Reset your password", "Welcome aboard"]));

    // Agra reads the three levels as one list.
    let names = |body: Value| -> Vec<String> {
        body.as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_owned())
            .collect()
    };
    let listed = client
        .get("/api/templates")
        .header("x-workspace", "agra")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(
        names(listed),
        ["Group handbook", "Reset your password", "Welcome aboard"]
    );
    let gulati = client
        .get("/api/templates")
        .header("x-workspace", "gulati")
        .send()
        .await
        .json_value();
    assert_eq!(names(gulati), ["Reset your password", "Welcome aboard"]);

    // Shared templates are read-only in the workspace.
    let db = app.get::<Db>().await;
    let welcome = LibraryRepo::<Template>::global()
        .where_(Template::SLUG.eq("welcome"))
        .first(db.as_ref())
        .await
        .unwrap()
        .unwrap();
    for path in [
        format!("/api/templates/{}", welcome.id),
        format!("/api/templates/{}/name", welcome.id),
    ] {
        let refused = client
            .patch(&path)
            .header("x-workspace", "agra")
            .json(&json!({ "name": "Ours" }))
            .send()
            .await
            .assert_status(403)
            .json_value();
        assert_eq!(refused["code"], "library_read_only");
        assert_eq!(
            refused["message"],
            "This library item is shared: clone it to change it."
        );
    }

    // A new template joins Agra's library; cloning gives Agra its own copy.
    client
        .post("/api/templates")
        .header("x-workspace", "agra")
        .json(&json!({ "slug": "follow-up", "name": "Follow-up" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("workspace_id", json!(agra));
    TemplateBlock::create(
        db.as_ref(),
        TemplateBlock {
            template_id: welcome.id,
            body: "Hello".into(),
            ..TemplateBlock::default()
        },
    )
    .await
    .unwrap();
    let copy = client
        .post(&format!("/api/templates/{}/clone", welcome.id))
        .header("x-workspace", "agra")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(copy["name"], "Welcome aboard (copy)");
    assert_eq!(copy["cloned_from_id"], json!(welcome.id));
    assert_eq!(copy["workspace_id"], json!(agra));
    let copy_id: Uuid = serde_json::from_value(copy["id"].clone()).unwrap();
    assert_eq!(
        TemplateBlock::query()
            .where_(TemplateBlock::TEMPLATE_ID.eq(copy_id))
            .count(db.as_ref())
            .await
            .unwrap(),
        1
    );
    let event = cloned.try_recv().expect("library.item_cloned");
    assert_eq!(event["library"], "templates");
    assert_eq!(event["item_id"], welcome.id.to_string());
    assert_eq!(event["clone_id"], copy_id.to_string());
    assert_eq!(event["source"], json!({ "scope": "global" }));
    assert_eq!(event["owner"], json!({ "scope": "workspace", "id": agra }));
    assert_eq!(event["cloned_by"], json!(ada));
    client
        .patch(&format!("/api/templates/{copy_id}/name"))
        .header("x-workspace", "agra")
        .json(&json!({ "name": "Our welcome" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("name", json!("Our welcome"));

    // Publishing Agra's copy to the organization, deliberately.
    client
        .post(&format!("/api/templates/{copy_id}/publish"))
        .header("x-workspace", "agra")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("scope", json!("organization"))
        .assert_json_path("cloned_from_id", json!(copy_id));

    // The page's reads.
    let copy = LibraryRepo::<Template>::find_visible_or_fail(
        db.as_ref(),
        LibraryViewer::workspace(agra).in_organization(Some(group)),
        copy_id,
    )
    .await
    .unwrap();
    assert_eq!(copy.cloned_from().unwrap(), Some(welcome.id));
    assert_eq!(
        LibraryRepo::<Template>::clones_of(welcome.id)
            .without_global_scope::<LibraryVisibility>()
            .count(db.as_ref())
            .await
            .unwrap(),
        1
    );

    // Seeding again changes nothing.
    app.run_command("db:seed", &["template-library"])
        .await
        .unwrap();
    assert_eq!(
        LibraryRepo::<Template>::global()
            .count(db.as_ref())
            .await
            .unwrap(),
        2
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn helm_manages_the_global_templates() {
    let app = builder()
        .config("app.key", navree::crypto::generate_key())
        .config("hashing.memory_kib", 1024)
        .config("hashing.iterations", 1)
        .fake_auth()
        .module(CryptoModule::from_config())
        .module(AuthModule::from_config())
        .module(ViewModule::from_config())
        .module(TemplateLibraryAdminModule)
        .module(HelmModule::from_config())
        .build()
        .await;
    app.run_command("db:seed", &["template-library"])
        .await
        .unwrap();
    let ada = Uuid::now_v7();
    let (group, _agra, _gulati) = tenants(&app, ada).await;
    let db = app.get::<Db>().await;
    LibraryRepo::<Template>::create(
        db.as_ref(),
        LibraryOwner::Organization(group),
        Template {
            slug: "handbook".into(),
            name: "Group handbook".into(),
            ..Template::default()
        },
    )
    .await
    .unwrap();

    let user = app
        .get::<AuthService>()
        .await
        .create_user("Admin", "admin@example.com", "a long password")
        .await
        .unwrap();
    let access = app.get::<navree::access::AccessService>().await;
    let uow = db.uow(None, None).await;
    access.create_role(&uow, "admin", None).await.unwrap();
    access.grant(&uow, "admin", "helm.view").await.unwrap();
    access.grant(&uow, "admin", "templates.*").await.unwrap();
    access.assign_role(&uow, user.id, "admin").await.unwrap();
    uow.commit().await.unwrap();
    let admin = app.acting_as(principal(user.id));

    admin
        .get("/admin/api/resources/template-library")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("meta.total", 2.into());
    let created = admin
        .post("/admin/api/resources/template-library")
        .json(&json!({ "slug": "invoice", "name": "Invoice reminder" }))
        .send()
        .await
        .json_value();
    let id: Uuid = serde_json::from_value(created["data"]["id"].clone())
        .unwrap_or_else(|_| panic!("{created}"));
    let template = LibraryRepo::<Template>::global()
        .find_or_fail(db.as_ref(), id)
        .await
        .unwrap();
    assert_eq!(template.name, "Invoice reminder");
    assert_eq!(template.library_owner().unwrap(), LibraryOwner::Global);
    admin
        .get("/admin/api/resources/template-library")
        .send()
        .await
        .assert_json_path("meta.total", 3.into());
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_hook_s_refusal_answers_with_its_mapping() {
    use deals::{Deal, DealObserver, Stage, WonDealsAreKept};

    struct CreateDeals;

    impl Migration for CreateDeals {
        async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
            schema
                .create("won_deals", |t| {
                    t.uuid("id").primary();
                    t.string("title");
                    t.string("stage");
                })
                .await
        }

        async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
            schema.drop_if_exists("won_deals").await
        }
    }

    async fn destroy(uow: Uow, Path(id): Path<Uuid>) -> Result<NoContent> {
        let mut deal = Deal::find_or_fail(&uow, id).await?;
        deal.delete(&uow).await?;
        Ok(NoContent)
    }

    #[injectable]
    struct DealsController;

    impl Controller for DealsController {
        fn prefix(&self) -> &'static str {
            "/won-deals"
        }

        fn routes(self: Arc<Self>) -> Vec<RouteDef> {
            vec![RouteDef::on(Method::DELETE, "/{id}", destroy).public()]
        }
    }

    struct DealsModule;

    impl Module for DealsModule {
        fn register(&self, m: &mut ModuleBuilder) {
            m.migrations(
                MigrationSet::new("won_deals").migration(SchemaMigration::new(
                    "m20261028_000003_create_won_deals",
                    CreateDeals,
                )),
            )
            .observe::<Deal>(DealObserver)
            .controller::<DealsController>();
            // In the module: a 409 instead of a 500.
            m.map_error::<WonDealsAreKept>(|_| Some(AppError::conflict("Won deals are kept.")));
        }
    }

    let app = TestApp::builder()
        .temp_database()
        .module(DatabaseModule::from_config())
        .module(DealsModule)
        .build()
        .await;
    let db = app.get::<Db>().await;
    let won = Deal::create(
        db.as_ref(),
        Deal {
            title: "Renewal".into(),
            stage: Stage::Won,
            ..Deal::default()
        },
    )
    .await
    .unwrap();
    app.client()
        .delete(&format!("/api/won-deals/{}", won.id))
        .send()
        .await
        .assert_status(409)
        .assert_json_path("message", json!("Won deals are kept."));
    app.shutdown().await.unwrap();
}
