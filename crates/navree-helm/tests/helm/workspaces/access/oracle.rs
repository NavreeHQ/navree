//! What a role row-level security binds learns from a form across workspaces:
//! nothing of whether a workspace exists, and a table name with capitals is
//! asked about as that table.

use super::*;

#[tokio::test]
async fn under_row_level_security_a_real_workspace_and_a_ghost_answer_the_same() {
    if !postgres_only() {
        return;
    }
    let world = rls_world(false).await;
    let real = world
        .outsider
        .post(EVERY)
        .json(&json!({ "title": "X", "workspace_id": world.gulati }))
        .send()
        .await
        .assert_error(403, None);
    let ghost = world
        .outsider
        .post(EVERY)
        .json(&json!({ "title": "X", "workspace_id": Uuid::now_v7() }))
        .send()
        .await
        .assert_error(403, None);
    assert_eq!(
        real.json_value()["code"],
        ghost.json_value()["code"],
        "{} / {}",
        real.text(),
        ghost.text()
    );
    assert_eq!(real.json_value()["message"], ghost.json_value()["message"]);
}

/// A note in a table whose name has capitals.
#[derive(navree_orm::Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(
    table = "MixedNotes",
    subject = "MixedNote",
    permission_prefix = "mixed",
    workspace
)]
pub struct MixedNote {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub title: String,
    pub created_at: DateTime<Utc>,
}

struct CreateMixedNotes;

impl Migration for CreateMixedNotes {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let mysql = schema.target().await?.dialect() == Dialect::MySql;
        schema
            .create("MixedNotes", |t| {
                uuid(t, "id", mysql).primary();
                uuid(t, "workspace_id", mysql);
                t.string("title");
                t.timestamp_tz("created_at");
            })
            .await?;
        schema
            .statement(&RlsPolicy::new("MixedNotes").create_sql())
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists("MixedNotes").await
    }
}

struct MixedResource;

impl Injectable for MixedResource {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(MixedResource) })
    }
}

impl Resource for MixedResource {
    type Model = MixedNote;

    const LABEL: &'static str = "Mixed notes";
    const SINGULAR: &'static str = "Mixed note";
    const SLUG: &'static str = "mixed";
    const TITLE: &'static str = "title";
    const WORKSPACES: Workspaces = Workspaces::Every;

    fn fields(&self) -> Vec<Field> {
        vec![
            Field::text("title").required(),
            Field::text("workspace_id"),
            Field::datetime("created_at").readonly(),
        ]
    }
}

struct MixedModule;

impl Module for MixedModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(
            MigrationSet::new("mixed")
                .after(["auth"])
                .migration(SchemaMigration::new(
                    "m20261101_000002_create_mixed_notes",
                    CreateMixedNotes,
                )),
        )
        .resource::<MixedResource>();
    }
}

#[tokio::test]
async fn a_table_name_with_capitals_is_asked_about_as_that_table() {
    if !postgres_only() {
        return;
    }
    let database = RlsDatabase::create(false);
    let builder = TestApp::builder()
        .config("database.url", database.url.clone())
        .config("database.migrate_on_boot", true)
        .config("database.max_connections", 4)
        .hold(database)
        .module(MixedModule);
    let world = world(with_members(builder).build().await).await;
    give_role(
        &world.app,
        world.member_id,
        "mixed-editor",
        &["helm.view", "mixed.*"],
    )
    .await;
    // Not an error about a table called `mixednotes`: the refusal.
    world
        .member
        .post("/admin/api/resources/mixed")
        .json(&json!({ "title": "X", "workspace_id": world.gulati }))
        .send()
        .await
        .assert_error_code(403, "helm_workspace_refused");
}
