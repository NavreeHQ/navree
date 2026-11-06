//! A record across workspaces refers to records of its own workspace, whichever
//! the sidebar holds.

use navree_orm::Record;

use super::*;

#[derive(navree_orm::Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(
    table = "folders",
    subject = "Folder",
    permission_prefix = "folders",
    workspace
)]
pub struct Folder {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub name: String,
}

#[derive(navree_orm::Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(
    table = "filed_notes",
    subject = "FiledNote",
    permission_prefix = "filed",
    workspace
)]
pub struct FiledNote {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub folder_id: Option<Uuid>,
    pub title: String,
}

struct CreateFiled;

impl Migration for CreateFiled {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let mysql = schema.target().await?.dialect() == Dialect::MySql;
        schema
            .create("folders", |t| {
                uuid(t, "id", mysql).primary();
                uuid(t, "workspace_id", mysql);
                t.string("name");
            })
            .await?;
        schema
            .create("filed_notes", |t| {
                uuid(t, "id", mysql).primary();
                uuid(t, "workspace_id", mysql);
                uuid(t, "folder_id", mysql).nullable();
                t.string("title");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists("filed_notes").await?;
        schema.drop_if_exists("folders").await
    }
}

struct FolderResource;

impl Injectable for FolderResource {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(FolderResource) })
    }
}

impl Resource for FolderResource {
    type Model = Folder;

    const LABEL: &'static str = "Folders";
    const SINGULAR: &'static str = "Folder";
    const TITLE: &'static str = "name";
    const DEFAULT_SORT: &'static str = "name";

    fn fields(&self) -> Vec<Field> {
        vec![Field::text("name").required()]
    }
}

struct FiledResource;

impl Injectable for FiledResource {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(FiledResource) })
    }
}

impl Resource for FiledResource {
    type Model = FiledNote;

    const LABEL: &'static str = "Filed notes";
    const SINGULAR: &'static str = "Filed note";
    const SLUG: &'static str = "filed";
    const TITLE: &'static str = "title";
    const DEFAULT_SORT: &'static str = "title";
    const WORKSPACES: Workspaces = Workspaces::Every;

    fn fields(&self) -> Vec<Field> {
        vec![
            Field::text("title").required(),
            Field::text("workspace_id"),
            Field::belongs_to::<FolderResource>("folder_id"),
        ]
    }
}

struct FiledModule;

impl Module for FiledModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(
            MigrationSet::new("filed")
                .after(["auth"])
                .migration(SchemaMigration::new(
                    "m20261101_000003_create_filed",
                    CreateFiled,
                )),
        )
        .resource::<FolderResource>()
        .resource::<FiledResource>();
    }
}

async fn folder_in(app: &TestApp, workspace: Uuid, name: &str) -> Uuid {
    let db = app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    enter(&uow, workspace).await;
    let record: Record<Folder> = Folder::create(
        &uow,
        Folder {
            name: name.to_owned(),
            ..Folder::default()
        },
    )
    .await
    .unwrap();
    uow.commit().await.unwrap();
    record.id
}

#[tokio::test]
async fn a_form_across_workspaces_refers_to_records_of_the_workspace_it_names() {
    let builder = crate::servers::database(TestApp::builder()).module(FiledModule);
    let world = world(with_members(builder).build().await).await;
    join(&world.app, world.gulati, world.member_id).await;
    give_role(
        &world.app,
        world.member_id,
        "filer",
        &["helm.view", "filed.*", "folders.*"],
    )
    .await;
    let agra_folder = folder_in(&world.app, world.agra, "Agra folder").await;
    let gulati_folder = folder_in(&world.app, world.gulati, "Gulati folder").await;
    let member = &world.member;
    member.set_cookie(WORKSPACE_COOKIE, &world.agra.to_string());
    const FILED: &str = "/admin/api/resources/filed";
    // Another workspace's folder isn't one this form can refer to.
    member
        .post(FILED)
        .json(&json!({
            "title": "Across",
            "workspace_id": world.gulati,
            "folder_id": agra_folder,
        }))
        .send()
        .await
        .assert_status(400);
    // Its own workspace's is, whichever the sidebar holds.
    let created = member
        .post(FILED)
        .json(&json!({
            "title": "Filed",
            "workspace_id": world.gulati,
            "folder_id": gulati_folder,
        }))
        .send()
        .await
        .assert_status(201)
        .json_value();
    let id = created["data"]["id"].as_str().unwrap().to_owned();
    // A change is held to the workspace of its record as well.
    member
        .patch(&format!("{FILED}/{id}"))
        .json(&json!({ "folder_id": agra_folder }))
        .send()
        .await
        .assert_status(400);
    member
        .patch(&format!("{FILED}/{id}"))
        .json(&json!({ "folder_id": gulati_folder }))
        .send()
        .await
        .assert_status(200);
}
