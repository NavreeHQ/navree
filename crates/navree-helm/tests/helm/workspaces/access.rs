//! Who may work in a workspace, with a real `WorkspaceModule`: a workspace
//! named by a header or the cookie is entered only after the check, on every
//! database, and a role that doesn't bypass row-level security gains nothing
//! by naming one.

use navree_db::Statement;
use navree_db::navree_schema::{Migration, Schema, SchemaMigration, SchemaResult};
use navree_kernel::{Module, ModuleBuilder};
use navree_workspace::{
    DemoWorkspaces, MEMBER, Members, ModuleBuilderWorkspaceExt, NewWorkspace, Workspace,
    WorkspaceModule, WorkspaceRole, Workspaces as Tenants,
};

use super::*;

mod demo;
mod pinned;
mod roles;
mod staff_audit;

/// `users(id, name, email)`, as the auth module creates it.
struct CreateUsers;

impl Migration for CreateUsers {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let mysql = schema.target().await?.dialect() == Dialect::MySql;
        schema
            .create("users", |t| {
                uuid(t, "id", mysql).primary();
                t.string("name");
                t.string("email").unique();
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists("users").await
    }
}

/// A row of `users`.
#[derive(navree_orm::Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(table = "users")]
struct Account {
    id: Uuid,
    name: String,
    email: String,
}

struct UsersModule;

impl Module for UsersModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(MigrationSet::new("auth").migration(SchemaMigration::new(
            "m20261027_000001_create_users",
            CreateUsers,
        )))
        .global();
    }
}

fn with_members(builder: TestAppBuilder) -> TestAppBuilder {
    builder
        .config("http.per_ip_rate_limit", Value::Null)
        .config("workspace.routes", false)
        .module(navree_db::DatabaseModule)
        .module(navree_access::AccessModule::for_root(options()))
        .module(UsersModule)
        .module(WorkspaceModule::from_config())
        .module(MemberNotesModule)
        .module(navree_helm::HelmModule::from_config())
}

/// The notes of [`NotesModule`], without what lets its tests skip the check.
struct MemberNotesModule;

impl Module for MemberNotesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(
            MigrationSet::new("notes")
                .after(["auth"])
                .migration(SchemaMigration::new(
                    "m20261101_000001_create_notes",
                    CreateNotes,
                )),
        )
        .resource::<NoteResource>()
        .resource::<EveryNote>();
    }
}

async fn app_with_members() -> TestApp {
    with_members(crate::servers::database(TestApp::builder()))
        .build()
        .await
}

/// Inserts a row of `users`.
async fn account(app: &TestApp, name: &str) -> Uuid {
    let id = Uuid::now_v7();
    let db = app.get::<Db>().await;
    Account::insert(
        &*db,
        [Account {
            id,
            name: name.to_owned(),
            email: format!("{name}-{id}@navree.test"),
        }],
    )
    .await
    .unwrap();
    id
}

async fn workspace(app: &TestApp, owner: Uuid, name: &str) -> Workspace {
    let tenants = app.get::<Tenants>().await;
    let db = app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    let made = uow
        .scope(tenants.create(&uow, owner, NewWorkspace::named(name)))
        .await
        .unwrap();
    uow.commit().await.unwrap();
    made
}

async fn join(app: &TestApp, workspace: Uuid, user: Uuid) {
    let members = app.get::<Members>().await;
    let db = app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    uow.scope(members.add(&uow, workspace, user, WorkspaceRole::from(MEMBER)))
        .await
        .unwrap();
    uow.commit().await.unwrap();
}

async fn leave(app: &TestApp, workspace: Uuid, user: Uuid) {
    let members = app.get::<Members>().await;
    let db = app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    uow.scope(members.remove(&uow, workspace, user))
        .await
        .unwrap();
    uow.commit().await.unwrap();
}

/// Two workspaces, a note in each, and users of the panel with every ability
/// on notes: a member of Acme only, and someone in neither.
struct World {
    app: TestApp,
    acme: Uuid,
    globex: Uuid,
    acme_note: Uuid,
    globex_note: Uuid,
    member_id: Uuid,
    member: TestClient,
    outsider_id: Uuid,
    outsider: TestClient,
}

async fn world(app: TestApp) -> World {
    let owner = account(&app, "owner").await;
    let acme = workspace(&app, owner, "Acme").await.id;
    let globex = workspace(&app, owner, "Globex").await.id;
    let acme_note = seed(&app, acme, "Acme secret").await;
    let globex_note = seed(&app, globex, "Globex secret").await;
    let member_id = account(&app, "member").await;
    join(&app, acme, member_id).await;
    let outsider_id = account(&app, "outsider").await;
    give_role(&app, member_id, "editor", &["helm.view", "notes.*"]).await;
    give_role(&app, outsider_id, "editor", &["helm.view", "notes.*"]).await;
    let member = app.acting_as(navree_testing::principal(member_id));
    let outsider = app.acting_as(navree_testing::principal(outsider_id));
    World {
        app,
        acme,
        globex,
        acme_note,
        globex_note,
        member_id,
        member,
        outsider_id,
        outsider,
    }
}

const NOTES: &str = "/admin/api/resources/notes";
const EVERY: &str = "/admin/api/resources/all-notes";

#[tokio::test]
async fn a_member_works_in_their_workspace_and_nobody_else_gets_in_by_naming_one() {
    let world = world(app_with_members().await).await;
    let ghost = Uuid::now_v7();

    // The member, in their own workspace: reads, writes, and every record stays there.
    let body = in_workspace(&world.member, world.acme, NOTES)
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(titles_of(&body), ["Acme secret"]);
    let created = world
        .member
        .post(NOTES)
        .header("X-Workspace", world.acme.to_string())
        .json(&json!({ "title": "Mine" }))
        .send()
        .await
        .assert_status(201)
        .json_value();
    let made: Uuid = created["data"]["id"].as_str().unwrap().parse().unwrap();
    assert_eq!(
        note_in(&world.app, world.acme, made)
            .await
            .unwrap()
            .workspace_id,
        world.acme
    );

    // The same user in another workspace, a user in none, and a workspace nobody made.
    for (client, target) in [
        (&world.member, world.globex),
        (&world.outsider, world.acme),
        (&world.outsider, world.globex),
        (&world.member, ghost),
        (&world.outsider, ghost),
    ] {
        in_workspace(client, target, NOTES)
            .send()
            .await
            .assert_error(403, None);
        client
            .post(NOTES)
            .header("X-Workspace", target.to_string())
            .json(&json!({ "title": "Planted" }))
            .send()
            .await
            .assert_error(403, None);
    }
    world
        .outsider
        .patch(&format!("{NOTES}/{}", world.acme_note))
        .header("X-Workspace", world.acme.to_string())
        .json(&json!({ "title": "Changed" }))
        .send()
        .await
        .assert_error(403, None);
    world
        .outsider
        .delete(&format!("{NOTES}/{}", world.globex_note))
        .header("X-Workspace", world.globex.to_string())
        .send()
        .await
        .assert_error(403, None);
    assert_eq!(
        note_in(&world.app, world.acme, world.acme_note)
            .await
            .unwrap()
            .title,
        "Acme secret"
    );
    assert!(
        note_in(&world.app, world.globex, world.globex_note)
            .await
            .is_some()
    );
    // Nothing was planted anywhere.
    let body = in_workspace(&world.member, world.acme, NOTES)
        .send()
        .await
        .json_value();
    assert_eq!(titles_of(&body), ["Acme secret", "Mine"]);
}

#[tokio::test]
async fn a_member_who_is_removed_is_out_at_once() {
    let world = world(app_with_members().await).await;
    in_workspace(&world.member, world.acme, NOTES)
        .send()
        .await
        .assert_status(200);
    leave(&world.app, world.acme, world.member_id).await;
    in_workspace(&world.member, world.acme, NOTES)
        .send()
        .await
        .assert_error(403, None);
}

#[tokio::test]
async fn the_cookie_is_checked_like_the_header_and_a_stale_one_locks_nobody_out() {
    let world = world(app_with_members().await).await;
    let page = |client: &TestClient, path: &str| {
        client
            .get(&format!("/admin{path}"))
            .header("accept", "text/html")
            .send()
    };
    // A cookie for a workspace of someone else's, however it got there.
    world
        .outsider
        .set_cookie(WORKSPACE_COOKIE, &world.globex.to_string());
    let response = world.outsider.get(NOTES).send().await.assert_status(400);
    assert!(response.text().contains("Choose a workspace first"));
    // The panel still answers, and shows the form to choose another.
    let home = page(&world.outsider, "").await.assert_status(200).text();
    assert!(!home.contains(&world.globex.to_string()), "{home}");

    // The form in the sidebar sets it only for a workspace the user may enter.
    for target in [world.globex, Uuid::now_v7()] {
        let response = world
            .member
            .post("/admin/workspace")
            .header("accept", "text/html")
            .form(&[("workspace", target.to_string().as_str())])
            .send()
            .await
            .assert_status(303);
        assert!(response.set_cookie(WORKSPACE_COOKIE).is_none());
    }
    world
        .member
        .post("/admin/workspace")
        .header("accept", "text/html")
        .form(&[("workspace", world.acme.to_string().as_str())])
        .send()
        .await
        .assert_status(303);
    let listed = page(&world.member, "/resources/notes")
        .await
        .assert_status(200)
        .text();
    assert!(listed.contains("Acme secret"), "{listed}");

    // Removed: the cookie stops counting, and the panel stays open.
    leave(&world.app, world.acme, world.member_id).await;
    world.member.get(NOTES).send().await.assert_status(400);
    page(&world.member, "").await.assert_status(200);
    // From there a form can still be sent, to leave the workspace.
    let response = world
        .member
        .post("/admin/workspace")
        .header("accept", "text/html")
        .form(&[("workspace", "")])
        .send()
        .await
        .assert_status(303);
    assert_eq!(response.set_cookie(WORKSPACE_COOKIE).unwrap().value(), "");
}

#[tokio::test]
async fn the_cookie_lasts_a_sign_in_and_does_not_outlive_it() {
    let world = world(app_with_members().await).await;
    let response = world
        .member
        .post("/admin/workspace")
        .header("accept", "text/html")
        .form(&[("workspace", world.acme.to_string().as_str())])
        .send()
        .await
        .assert_status(303);
    let cookie = response.set_cookie(WORKSPACE_COOKIE).expect("a cookie");
    assert_eq!(
        cookie.max_age().map(|age| age.whole_seconds()),
        Some(480 * 60)
    );
    let response = world
        .member
        .post("/admin/logout")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(303);
    let cleared = response.set_cookie(WORKSPACE_COOKIE).expect("cleared");
    assert_eq!(cleared.value(), "");
    assert_eq!(cleared.max_age().map(|age| age.whole_seconds()), Some(0));
    assert_eq!(world.member.cookie(WORKSPACE_COOKIE), None);
}

#[tokio::test]
async fn the_panel_reads_the_header_the_app_configured() {
    let app = with_members(crate::servers::database(TestApp::builder()))
        .config("workspace.header", "X-Tenant")
        .build()
        .await;
    let world = world(app).await;
    world
        .member
        .get(NOTES)
        .header("X-Tenant", world.acme.to_string())
        .send()
        .await
        .assert_status(200)
        .assert_json_path("meta.total", json!(1));
    world
        .member
        .get(NOTES)
        .header("X-Tenant", world.globex.to_string())
        .send()
        .await
        .assert_error(403, None);
    // The other name is no name here.
    world
        .member
        .get(NOTES)
        .header("X-Workspace", world.acme.to_string())
        .send()
        .await
        .assert_status(400);
}

#[tokio::test]
async fn a_read_only_demo_takes_no_write_from_the_panel() {
    let world = world(app_with_members().await).await;
    let owner = account(&world.app, "demo-owner").await;
    let demos = world.app.get::<DemoWorkspaces>().await;
    let db = world.app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    let demo = uow
        .scope(demos.create(&uow, owner, NewWorkspace::named("Demo")))
        .await
        .unwrap();
    uow.commit().await.unwrap();
    join(&world.app, demo.id, world.member_id).await;
    in_workspace(&world.member, demo.id, NOTES)
        .send()
        .await
        .assert_status(200);
    world
        .member
        .post(NOTES)
        .header("X-Workspace", demo.id.to_string())
        .json(&json!({ "title": "Into the demo" }))
        .send()
        .await
        .assert_error_code(403, "demo_read_only");
}

#[tokio::test]
async fn an_update_naming_another_workspace_is_a_400() {
    let world = world(app_with_members().await).await;
    // Through a resource in one workspace, and through one across all of them.
    world
        .member
        .patch(&format!("{NOTES}/{}", world.acme_note))
        .header("X-Workspace", world.acme.to_string())
        .json(&json!({ "workspace_id": world.globex }))
        .send()
        .await
        .assert_status(400);
    if Server::current() != Server::Postgres {
        world
            .member
            .patch(&format!("{EVERY}/{}", world.acme_note))
            .json(&json!({ "workspace_id": world.globex }))
            .send()
            .await
            .assert_status(400);
    }
    assert_eq!(
        note_anywhere(&world.app, world.acme_note)
            .await
            .unwrap()
            .workspace_id,
        world.acme
    );
    // Naming the one it is in changes nothing.
    world
        .member
        .patch(&format!("{NOTES}/{}", world.acme_note))
        .header("X-Workspace", world.acme.to_string())
        .json(&json!({ "workspace_id": world.acme, "title": "Same place" }))
        .send()
        .await
        .assert_status(200);
}

#[tokio::test]
async fn a_form_across_workspaces_names_its_workspace_whatever_the_sidebar_holds() {
    let world = world(app_with_members().await).await;
    join(&world.app, world.globex, world.member_id).await;
    world
        .member
        .set_cookie(WORKSPACE_COOKIE, &world.acme.to_string());
    let created = world
        .member
        .post(EVERY)
        .json(&json!({ "title": "For Globex", "workspace_id": world.globex }))
        .send()
        .await
        .assert_status(201)
        .json_value();
    let id: Uuid = created["data"]["id"].as_str().unwrap().parse().unwrap();
    assert_eq!(
        note_in(&world.app, world.globex, id)
            .await
            .unwrap()
            .workspace_id,
        world.globex
    );
    // A form naming none takes the sidebar's.
    let created = world
        .member
        .post(EVERY)
        .json(&json!({ "title": "For Acme" }))
        .send()
        .await
        .assert_status(201)
        .json_value();
    let id: Uuid = created["data"]["id"].as_str().unwrap().parse().unwrap();
    assert!(note_in(&world.app, world.acme, id).await.is_some());
    // A header is the client's own choice, and a form may not contradict it.
    world
        .member
        .post(EVERY)
        .header("X-Workspace", world.acme.to_string())
        .json(&json!({ "title": "Mixed", "workspace_id": world.globex }))
        .send()
        .await
        .assert_status(400);
}

#[tokio::test]
async fn a_create_across_workspaces_names_a_workspace_that_exists() {
    let world = world(app_with_members().await).await;
    world
        .member
        .post(EVERY)
        .json(&json!({ "title": "Nowhere", "workspace_id": Uuid::now_v7() }))
        .send()
        .await
        .assert_status(400);
}

#[tokio::test]
async fn a_queued_action_checks_the_member_again_when_it_runs() {
    use navree_contracts::{Job, JobContext};

    let fake = QueueFake::new();
    let app = with_members(crate::servers::database(TestApp::builder()))
        .override_value::<dyn navree_contracts::Dispatcher>(fake.dispatcher())
        .build()
        .await;
    let world = world(app).await;
    let start = || async {
        world
            .member
            .post(&format!("{NOTES}/actions/whisper"))
            .header("X-Workspace", world.acme.to_string())
            .json(&json!({ "ids": [world.acme_note] }))
            .send()
            .await
            .assert_status(200)
            .assert_json_path("queued", json!(true));
    };
    let context = |job_id: &str| JobContext {
        app: world.app.ctx().clone(),
        job_id: job_id.into(),
        attempt: 1,
    };
    start().await;
    start().await;
    let mut jobs = fake.dispatched::<HelmActionJob>().into_iter();
    jobs.next().unwrap().handle(context("1")).await.unwrap();
    assert_eq!(
        note_in(&world.app, world.acme, world.acme_note)
            .await
            .unwrap()
            .title,
        "acme secret"
    );

    // The second one finds they have gone.
    let mut note = note_in(&world.app, world.acme, world.acme_note)
        .await
        .unwrap();
    let db = world.app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    enter(&uow, world.acme).await;
    note.title = "LOUD AGAIN".to_owned();
    Note::query()
        .where_key(world.acme_note)
        .update(
            &uow,
            vec![navree_orm::Assignment::value("title", note.title.clone())],
        )
        .await
        .unwrap();
    uow.commit().await.unwrap();
    leave(&world.app, world.acme, world.member_id).await;
    let error = jobs
        .next()
        .unwrap()
        .handle(context("2"))
        .await
        .expect_err("not a member any more");
    assert!(error.to_string().contains("may not work"), "{error}");
    assert_eq!(
        note_in(&world.app, world.acme, world.acme_note)
            .await
            .unwrap()
            .title,
        "LOUD AGAIN"
    );
}

// ---------------------------------------------------------------------------
// PostgreSQL row-level security, with a role that does not bypass it
// ---------------------------------------------------------------------------

async fn rls_world(bypass: bool) -> World {
    let database = RlsDatabase::create(bypass);
    let builder = TestApp::builder()
        .config("database.url", database.url.clone())
        .config("database.migrate_on_boot", true)
        .config("database.max_connections", 4)
        .hold(database);
    world(with_members(builder).build().await).await
}

#[tokio::test]
async fn under_row_level_security_naming_a_workspace_takes_being_in_it() {
    if !postgres_only() {
        return;
    }
    let world = rls_world(false).await;
    assert_bound_by_row_level_security(&world.app).await;
    let body = in_workspace(&world.member, world.acme, NOTES)
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(titles_of(&body), ["Acme secret"]);
    in_workspace(&world.member, world.globex, NOTES)
        .send()
        .await
        .assert_error(403, None);
    for target in [world.globex, Uuid::now_v7()] {
        world
            .outsider
            .post(NOTES)
            .header("X-Workspace", target.to_string())
            .json(&json!({ "title": "Planted" }))
            .send()
            .await
            .assert_error(403, None);
    }
}

#[tokio::test]
async fn under_row_level_security_a_create_across_workspaces_is_closed_like_its_reads() {
    if !postgres_only() {
        return;
    }
    let world = rls_world(false).await;
    // Sees nothing, without a workspace...
    world
        .outsider
        .get(EVERY)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("meta.total", json!(0));
    // ...and makes nothing, by naming one it isn't in.
    world
        .outsider
        .post(EVERY)
        .json(&json!({ "title": "Through Every", "workspace_id": world.globex }))
        .send()
        .await
        .assert_error(403, None);
    let body = in_workspace(&world.member, world.acme, NOTES)
        .send()
        .await
        .json_value();
    assert_eq!(titles_of(&body), ["Acme secret"]);
    let db = world.app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    enter(&uow, world.globex).await;
    let count = Note::query().count(&uow).await.unwrap();
    uow.rollback().await.unwrap();
    assert_eq!(count, 1, "Globex holds a planted note");
    // A member names their own.
    world
        .member
        .post(EVERY)
        .json(&json!({ "title": "Mine", "workspace_id": world.acme }))
        .send()
        .await
        .assert_status(201);
}

#[tokio::test]
async fn a_role_that_bypasses_row_level_security_makes_records_across_workspaces() {
    if !postgres_only() {
        return;
    }
    let world = rls_world(true).await;
    world
        .outsider
        .post(EVERY)
        .json(&json!({ "title": "Maintenance", "workspace_id": world.globex }))
        .send()
        .await
        .assert_status(201);
}

// ---------------------------------------------------------------------------
// Schema and database isolation
// ---------------------------------------------------------------------------

/// Notes in every workspace's schema, and a table of the same name in `public`.
struct SchemaNotesModule;

impl Module for SchemaNotesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(
            MigrationSet::new("notes")
                .after(["auth"])
                .migration(SchemaMigration::new(
                    "m20261101_000001_create_notes",
                    CreateNotes,
                )),
        )
        .workspace_migrations(
            MigrationSet::new("workspace_notes").migration(SchemaMigration::new(
                "m20261101_000002_create_notes",
                CreateNotes,
            )),
        )
        .resource::<NoteResource>();
    }
}

#[tokio::test]
async fn under_schema_isolation_the_panel_works_in_the_schema_of_the_workspace() {
    if !postgres_only() {
        return;
    }
    let (url, guard) = crate::servers::scratch();
    let app = TestApp::builder()
        .config("database.url", url)
        .config("database.migrate_on_boot", true)
        .config("database.max_connections", 4)
        .config("workspace.isolation", "schema")
        .config("http.per_ip_rate_limit", Value::Null)
        .config("workspace.routes", false)
        .hold(guard)
        .module(navree_db::DatabaseModule)
        .module(navree_access::AccessModule::for_root(options()))
        .module(UsersModule)
        .module(WorkspaceModule::from_config())
        .module(SchemaNotesModule)
        .module(navree_helm::HelmModule::from_config())
        .build()
        .await;
    let owner = account(&app, "owner").await;
    let acme = workspace(&app, owner, "Acme").await.id;
    let member_id = account(&app, "member").await;
    join(&app, acme, member_id).await;
    give_role(&app, member_id, "editor", &["helm.view", "notes.*"]).await;
    let member = app.acting_as(navree_testing::principal(member_id));
    // What the shared table holds isn't the workspace's.
    let db = app.get::<Db>().await;
    use navree_db::Executor as _;
    db.execute(&Statement::text(format!(
        "INSERT INTO public.notes (id, workspace_id, title, created_at) \
         VALUES ('{}', '{acme}', 'In public', now())",
        Uuid::now_v7()
    )))
    .await
    .unwrap();

    let body = in_workspace(&member, acme, NOTES)
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(titles_of(&body), Vec::<String>::new());
    member
        .post(NOTES)
        .header("X-Workspace", acme.to_string())
        .json(&json!({ "title": "In the schema" }))
        .send()
        .await
        .assert_status(201);
    let body = in_workspace(&member, acme, NOTES).send().await.json_value();
    assert_eq!(titles_of(&body), ["In the schema"]);
    let row = db
        .fetch_optional(&Statement::text(
            "SELECT count(*) AS n FROM public.notes WHERE title = 'In the schema'",
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        row.get::<i64>("n").unwrap(),
        0,
        "written to the shared table"
    );
}

#[tokio::test]
async fn under_database_isolation_the_panel_says_it_cannot_work_in_a_workspace() {
    if Server::current() != Server::Sqlite {
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let url = format!(
        "sqlite://{}/{{id}}.db?mode=rwc",
        directory.path().display().to_string().replace('\\', "/")
    );
    let app = with_members(TestApp::builder().temp_database())
        .config("workspace.isolation", "database")
        .config("workspace.database_url", url)
        .build()
        .await;
    let world = world_of_members_only(&app).await;
    let response = in_workspace(&world.0, world.1, NOTES).send().await;
    let response = response.assert_status(400);
    assert!(response.text().contains("database"), "{}", response.text());
}

/// A member of a workspace, without seeding notes in it (they live elsewhere).
async fn world_of_members_only(app: &TestApp) -> (TestClient, Uuid) {
    let owner = account(app, "owner").await;
    let acme = workspace(app, owner, "Acme").await.id;
    let member_id = account(app, "member").await;
    join(app, acme, member_id).await;
    give_role(app, member_id, "editor", &["helm.view", "notes.*"]).await;
    (app.acting_as(navree_testing::principal(member_id)), acme)
}

// ---------------------------------------------------------------------------
// Who may enter without being a member, and an app with no way to check
// ---------------------------------------------------------------------------

/// Notes, and staff are whoever holds the permission `support.enter`.
struct StaffNotesModule;

impl Module for StaffNotesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.helm_staff_workspaces(|entry| async move {
            Ok(entry.access.ability.can("enter", "support", None))
        });
    }
}

async fn staff_world() -> (World, TestClient) {
    let builder = crate::servers::database(TestApp::builder())
        .config("audit.routes", false)
        .module(navree_audit::AuditModule::from_config())
        .module(StaffNotesModule);
    let world = world(with_members(builder).build().await).await;
    let staff_id = account(&world.app, "staff").await;
    give_role(
        &world.app,
        staff_id,
        "support",
        &["helm.view", "notes.*", "support.enter"],
    )
    .await;
    let staff = world.app.acting_as(navree_testing::principal(staff_id));
    (world, staff)
}

async fn staff_entries(app: &TestApp) -> Vec<String> {
    let log = app.get::<navree_audit::AuditLog>().await;
    log.flush().await;
    navree_audit::AuditLog::list(
        &log,
        &navree_audit::AuditFilter::default(),
        navree_db::PageQuery {
            page: 1,
            per_page: 100,
        },
    )
    .await
    .unwrap()
    .data
    .into_iter()
    .filter(|row| row.action == "helm.workspace.staff")
    .map(|row| row.subject_id.unwrap_or_default())
    .collect()
}

#[tokio::test]
async fn staff_enter_by_the_panels_own_word_and_it_is_written_down() {
    let (world, staff) = staff_world().await;
    let body = in_workspace(&staff, world.globex, NOTES)
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(titles_of(&body), ["Globex secret"]);
    assert_eq!(staff_entries(&world.app).await, [world.globex.to_string()]);

    // A workspace nobody made is not one staff can plant records in.
    in_workspace(&staff, Uuid::now_v7(), NOTES)
        .send()
        .await
        .assert_error(404, None);
    // The same app's other users don't get in, and what members do isn't audited as staff.
    in_workspace(&world.outsider, world.globex, NOTES)
        .send()
        .await
        .assert_error(403, None);
    in_workspace(&world.member, world.acme, NOTES)
        .send()
        .await
        .assert_status(200);
    assert_eq!(staff_entries(&world.app).await.len(), 1);
    // The form in the sidebar takes the same word.
    let response = staff
        .post("/admin/workspace")
        .header("accept", "text/html")
        .form(&[("workspace", world.globex.to_string().as_str())])
        .send()
        .await
        .assert_status(303);
    assert!(response.set_cookie(WORKSPACE_COOKIE).is_some());
}

/// Notes, with no `WorkspaceModule` and nobody named as staff.
async fn app_with_no_check() -> TestApp {
    crate::servers::database(TestApp::builder())
        .config("http.per_ip_rate_limit", Value::Null)
        .module(navree_db::DatabaseModule)
        .module(navree_access::AccessModule::for_root(options()))
        .module(MemberNotesModule)
        .module(navree_helm::HelmModule::from_config())
        .build()
        .await
}

#[tokio::test]
async fn a_panel_with_no_way_to_check_refuses_and_says_why() {
    let app = app_with_no_check().await;
    let acme = Uuid::now_v7();
    seed(&app, acme, "Acme secret").await;
    let editor = editor(&app).await;
    let response = in_workspace(&editor, acme, NOTES)
        .send()
        .await
        .assert_error_code(403, "helm_workspace_unchecked");
    assert!(
        response.text().contains("WorkspaceModule"),
        "{}",
        response.text()
    );
    editor
        .post(NOTES)
        .header("X-Workspace", acme.to_string())
        .json(&json!({ "title": "Planted" }))
        .send()
        .await
        .assert_error_code(403, "helm_workspace_unchecked");
    // The cookie names nothing; the form that sets it refuses.
    editor.set_cookie(WORKSPACE_COOKIE, &acme.to_string());
    editor.get(NOTES).send().await.assert_status(400);
    editor.remove_cookie(WORKSPACE_COOKIE);
    let response = editor
        .post("/admin/workspace")
        .header("accept", "text/html")
        .form(&[("workspace", acme.to_string().as_str())])
        .send()
        .await
        .assert_status(303);
    assert!(response.set_cookie(WORKSPACE_COOKIE).is_none());
    // A form across workspaces isn't taken at its word either.
    editor
        .post(EVERY)
        .json(&json!({ "title": "Planted", "workspace_id": acme }))
        .send()
        .await
        .assert_error_code(403, "helm_workspace_unchecked");
}

#[tokio::test]
async fn an_action_across_workspaces_runs_in_no_workspace_unless_one_was_named() {
    if Server::current() == Server::Postgres {
        // Under row-level security the database shows it no record to act on.
        return;
    }
    let world = world(app_with_members().await).await;
    world
        .outsider
        .post(&format!("{EVERY}/actions/locate"))
        .json(&json!({ "ids": [world.acme_note] }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("message", json!("working in None"));
}
