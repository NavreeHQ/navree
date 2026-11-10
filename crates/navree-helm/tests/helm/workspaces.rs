//! Models that belong to workspaces (`#[model(workspace)]`) in the panel:
//! the panel works in one workspace at a time, asks for one when the request
//! has none, writes inside the workspace of the record, and keeps working
//! under PostgreSQL row-level security with a role that does not bypass it.

use chrono::{DateTime, Utc};
use navree_db::navree_schema::{Dialect, Migration, Schema, SchemaMigration, SchemaResult};
use navree_db::{Db, MigrationSet, ModuleBuilderDbExt};
use navree_helm::{
    Action, ActionResult, Field, HelmActionJob, Metric, ModuleBuilderHelmExt, Resource,
    WORKSPACE_COOKIE, Workspaces,
};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_orm::{Model, Record};
use navree_queue::QueueFake;
use navree_testing::{TestApp, TestAppBuilder, TestClient, TestResponse};
use navree_workspace::RlsPolicy;
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::servers::{Server, run_admin};
use crate::support::*;

mod access;

/// A note, which belongs to a workspace.
#[derive(navree_orm::Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(
    table = "notes",
    subject = "Note",
    permission_prefix = "notes",
    workspace
)]
pub struct Note {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub title: String,
    pub body: Option<String>,
    pub created_at: DateTime<Utc>,
}

struct CreateNotes;

impl Migration for CreateNotes {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let mysql = schema.target().await?.dialect() == Dialect::MySql;
        schema
            .create("notes", |t| {
                uuid(t, "id", mysql).primary();
                uuid(t, "workspace_id", mysql);
                t.string("title");
                t.text("body").nullable();
                t.timestamp_tz("created_at");
            })
            .await?;
        // On PostgreSQL the database enforces the workspace as well.
        if schema.target().await?.dialect() == Dialect::Postgres {
            schema
                .statement(&RlsPolicy::new("notes").create_sql())
                .await?;
        }
        Ok(())
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists("notes").await
    }
}

/// Notes in the workspace the request works in.
struct NoteResource;

impl Injectable for NoteResource {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(NoteResource) })
    }
}

impl Resource for NoteResource {
    type Model = Note;

    const LABEL: &'static str = "Notes";
    const SINGULAR: &'static str = "Note";
    const GROUP: &'static str = "Notes";
    const TITLE: &'static str = "title";
    const SEARCH: &'static [&'static str] = &["title"];

    fn fields(&self) -> Vec<Field> {
        vec![
            Field::text("title").required().max_length(200).sortable(),
            Field::textarea("body"),
            Field::text("workspace_id")
                .label("Workspace")
                .hide_on_index(),
            Field::datetime("created_at").readonly().sortable(),
        ]
    }

    fn actions(&self) -> Vec<Action<Note>> {
        vec![
            Action::new("shout", "Shout").handle(|ctx, notes: Vec<Record<Note>>| async move {
                let count = notes.len();
                for mut note in notes {
                    note.update(ctx.uow(), |n| n.title = n.title.to_uppercase())
                        .await?;
                }
                Ok(ActionResult::message(format!("{count} notes shouted.")))
            }),
            Action::new("whisper", "Whisper").queued().handle(
                |ctx, notes: Vec<Record<Note>>| async move {
                    for mut note in notes {
                        note.update(ctx.uow(), |n| n.title = n.title.to_lowercase())
                            .await?;
                    }
                    Ok(ActionResult::message("Whispered."))
                },
            ),
        ]
    }

    fn metrics(&self) -> Vec<Metric<Note>> {
        vec![Metric::value("Notes").count().on_dashboard()]
    }
}

/// The same notes across every workspace.
struct EveryNote;

impl Injectable for EveryNote {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(EveryNote) })
    }
}

impl Resource for EveryNote {
    type Model = Note;

    const LABEL: &'static str = "All notes";
    const SINGULAR: &'static str = "Note";
    const GROUP: &'static str = "Notes";
    const SLUG: &'static str = "all-notes";
    const TITLE: &'static str = "title";
    const WORKSPACES: Workspaces = Workspaces::Every;

    fn fields(&self) -> Vec<Field> {
        vec![
            Field::text("title").required().max_length(200).sortable(),
            Field::text("workspace_id").label("Workspace"),
            Field::datetime("created_at").readonly().sortable(),
        ]
    }

    fn actions(&self) -> Vec<Action<Note>> {
        vec![
            Action::new("locate", "Locate").handle(|ctx, _notes: Vec<Record<Note>>| async move {
                Ok(ActionResult::message(format!(
                    "working in {:?}",
                    ctx.workspace()
                )))
            }),
        ]
    }
}

struct NotesModule;

impl Module for NotesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(MigrationSet::new("notes").migration(SchemaMigration::new(
            "m20261101_000001_create_notes",
            CreateNotes,
        )))
        .resource::<NoteResource>()
        .resource::<EveryNote>()
        // These tests are about what the panel does in a workspace, whoever
        // named it: `access` is about who may.
        .helm_staff_workspaces(|_| async { Ok(true) });
    }
}

fn with_notes(builder: TestAppBuilder) -> TestAppBuilder {
    builder
        .config("http.per_ip_rate_limit", Value::Null)
        .module(navree_db::DatabaseModule)
        .module(navree_access::AccessModule::for_root(options()))
        .module(NotesModule)
        .module(navree_helm::HelmModule::from_config())
}

async fn notes_app() -> TestApp {
    with_notes(crate::servers::database(TestApp::builder()))
        .build()
        .await
}

/// A user who may use the panel and everything about notes, acting as them.
async fn editor(app: &TestApp) -> TestClient {
    let id = Uuid::now_v7();
    give_role(app, id, "editor", &["helm.view", "notes.*"]).await;
    app.acting_as(navree_testing::principal(id))
}

/// Makes `workspace` the one `uow` works in, as a workspace request does.
async fn enter(uow: &navree_db::Uow, workspace: Uuid) {
    uow.set_workspace(Some(workspace));
    uow.set_local("navree.workspace", &workspace.to_string())
        .await
        .unwrap();
}

/// Inserts a note into `workspace`.
async fn seed(app: &TestApp, workspace: Uuid, title: &str) -> Uuid {
    let db = app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    enter(&uow, workspace).await;
    let note = Note {
        title: title.to_owned(),
        created_at: now(),
        ..Note::default()
    };
    let record = Note::create(&uow, note).await.unwrap();
    uow.commit().await.unwrap();
    record.id
}

/// The note `id` as `workspace` sees it, if it does.
async fn note_in(app: &TestApp, workspace: Uuid, id: Uuid) -> Option<Note> {
    let db = app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    enter(&uow, workspace).await;
    let found = Note::query()
        .where_key(id)
        .first(&uow)
        .await
        .unwrap()
        .map(Record::into_inner);
    uow.rollback().await.unwrap();
    found
}

/// The note `id` wherever it is, whatever the workspace (a diagnosis).
async fn note_anywhere(app: &TestApp, id: Uuid) -> Option<Note> {
    let db = app.get::<Db>().await;
    Note::query()
        .without_workspace_scope()
        .where_key(id)
        .first(&*db)
        .await
        .unwrap()
        .map(Record::into_inner)
}

fn titles_of(body: &Value) -> Vec<String> {
    let mut titles = titles(body);
    titles.sort();
    titles
}

/// `client`'s request `method` `path`, working in `workspace`.
fn in_workspace(client: &TestClient, workspace: Uuid, path: &str) -> navree_testing::TestRequest {
    client
        .get(path)
        .header("X-Workspace", workspace.to_string())
}

// ---------------------------------------------------------------------------
// What the panel does with the workspace of the request, on any database
// ---------------------------------------------------------------------------

#[tokio::test]
async fn without_a_workspace_a_resource_of_workspace_notes_asks_for_one() {
    let app = notes_app().await;
    let (agra, gulati) = (Uuid::now_v7(), Uuid::now_v7());
    seed(&app, agra, "Agra note").await;
    seed(&app, gulati, "Gulati note").await;
    let editor = editor(&app).await;

    for path in [
        "/admin/api/resources/notes",
        "/admin/api/resources/notes/options",
        "/admin/api/resources/notes/metrics/notes",
    ] {
        let response = editor.get(path).send().await;
        match path {
            // A picker has no records to offer without a workspace.
            "/admin/api/resources/notes/options" => {
                response
                    .assert_status(200)
                    .assert_json_path("data", json!([]));
            }
            _ => {
                let response = response.assert_status(400);
                assert!(
                    response.text().contains("Choose a workspace first"),
                    "{path}: {}",
                    response.text()
                );
            }
        }
    }
    // Neither does a write.
    editor
        .post("/admin/api/resources/notes")
        .json(&json!({ "title": "Orphan" }))
        .send()
        .await
        .assert_status(400);
    // The page says what is missing.
    let response = editor
        .get("/admin/resources/notes")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(400);
    assert!(response.text().contains("Choose a workspace first"));
    // A workspace is named by a header or the panel's cookie.
    in_workspace(&editor, agra, "/admin/api/resources/notes")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("meta.total", json!(1));
}

#[tokio::test]
async fn the_panel_works_in_the_workspace_of_the_request_only() {
    let app = notes_app().await;
    let (agra, gulati) = (Uuid::now_v7(), Uuid::now_v7());
    let a1 = seed(&app, agra, "Agra one").await;
    seed(&app, agra, "Agra two").await;
    let g1 = seed(&app, gulati, "Gulati one").await;
    let editor = editor(&app).await;

    let body = in_workspace(&editor, agra, "/admin/api/resources/notes")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(titles_of(&body), ["Agra one", "Agra two"]);
    let body = in_workspace(&editor, gulati, "/admin/api/resources/notes")
        .send()
        .await
        .json_value();
    assert_eq!(titles_of(&body), ["Gulati one"]);

    // A record of another workspace is not found, whichever way it is asked for.
    in_workspace(&editor, agra, &format!("/admin/api/resources/notes/{g1}"))
        .send()
        .await
        .assert_error(404, None);
    editor
        .patch(&format!("/admin/api/resources/notes/{g1}"))
        .header("X-Workspace", agra.to_string())
        .json(&json!({ "title": "Taken over" }))
        .send()
        .await
        .assert_error(404, None);
    editor
        .delete(&format!("/admin/api/resources/notes/{g1}"))
        .header("X-Workspace", agra.to_string())
        .send()
        .await
        .assert_error(404, None);
    assert_eq!(note_in(&app, gulati, g1).await.unwrap().title, "Gulati one");

    // The same note, asked for in its own workspace.
    in_workspace(&editor, agra, &format!("/admin/api/resources/notes/{a1}"))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.values.title", json!("Agra one"));
}

#[tokio::test]
async fn records_are_made_changed_and_deleted_in_the_workspace_of_the_request() {
    let app = notes_app().await;
    let (agra, gulati) = (Uuid::now_v7(), Uuid::now_v7());
    let editor = editor(&app).await;

    let created = editor
        .post("/admin/api/resources/notes")
        .header("X-Workspace", agra.to_string())
        .json(&json!({ "title": "Made in Agra", "body": "text" }))
        .send()
        .await
        .assert_status(201)
        .json_value();
    let id: Uuid = created["data"]["id"].as_str().unwrap().parse().unwrap();
    let stored = match note_in(&app, agra, id).await {
        Some(stored) => stored,
        None => panic!(
            "not in Agra: {created} / {:?}",
            note_anywhere(&app, id).await
        ),
    };
    assert_eq!(stored.workspace_id, agra);
    assert_eq!(stored.title, "Made in Agra");
    assert!(note_in(&app, gulati, id).await.is_none());

    // A form that names another workspace is refused, whatever it holds.
    editor
        .post("/admin/api/resources/notes")
        .header("X-Workspace", agra.to_string())
        .json(&json!({ "title": "Smuggled", "workspace_id": gulati }))
        .send()
        .await
        .assert_status(400);
    assert!(
        in_workspace(&editor, gulati, "/admin/api/resources/notes")
            .send()
            .await
            .json_value()["data"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    editor
        .patch(&format!("/admin/api/resources/notes/{id}"))
        .header("X-Workspace", agra.to_string())
        .json(&json!({ "title": "Edited", "body": "new text" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.title", json!("Edited"));
    let stored = note_in(&app, agra, id).await.unwrap();
    assert_eq!(
        (stored.title.as_str(), stored.workspace_id),
        ("Edited", agra)
    );
    // The workspace of a record cannot be changed from a form.
    editor
        .patch(&format!("/admin/api/resources/notes/{id}"))
        .header("X-Workspace", agra.to_string())
        .json(&json!({ "workspace_id": gulati }))
        .send()
        .await;
    assert_eq!(note_in(&app, agra, id).await.unwrap().workspace_id, agra);

    editor
        .delete(&format!("/admin/api/resources/notes/{id}"))
        .header("X-Workspace", agra.to_string())
        .send()
        .await
        .assert_status(204);
    assert!(note_in(&app, agra, id).await.is_none());
}

#[tokio::test]
async fn actions_run_on_the_records_of_the_workspace_of_the_request() {
    let app = notes_app().await;
    let (agra, gulati) = (Uuid::now_v7(), Uuid::now_v7());
    let mine = seed(&app, agra, "quiet").await;
    let theirs = seed(&app, gulati, "quiet too").await;
    let editor = editor(&app).await;

    editor
        .post("/admin/api/resources/notes/actions/shout")
        .header("X-Workspace", agra.to_string())
        .json(&json!({ "ids": [mine, theirs] }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("message", json!("1 notes shouted."));
    assert_eq!(note_in(&app, agra, mine).await.unwrap().title, "QUIET");
    // The other workspace's note was not in the request's workspace: left alone.
    assert_eq!(
        note_in(&app, gulati, theirs).await.unwrap().title,
        "quiet too"
    );

    editor
        .post("/admin/api/resources/notes/actions/shout")
        .header("X-Workspace", agra.to_string())
        .json(&json!({ "ids": [theirs] }))
        .send()
        .await
        .assert_error(403, None);
    assert_eq!(
        note_in(&app, gulati, theirs).await.unwrap().title,
        "quiet too"
    );
}

#[tokio::test]
async fn a_queued_action_runs_in_the_workspace_it_was_started_in() {
    let fake = QueueFake::new();
    let app = with_notes(crate::servers::database(TestApp::builder()))
        .override_value::<dyn navree_contracts::Dispatcher>(fake.dispatcher())
        .build()
        .await;
    let agra = Uuid::now_v7();
    let id = seed(&app, agra, "LOUD").await;
    let editor = editor(&app).await;
    editor
        .post("/admin/api/resources/notes/actions/whisper")
        .header("X-Workspace", agra.to_string())
        .json(&json!({ "ids": [id] }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("queued", json!(true));
    let jobs = fake.dispatched::<HelmActionJob>();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].workspace, Some(agra));
    assert_eq!(note_in(&app, agra, id).await.unwrap().title, "LOUD");

    use navree_contracts::{Job, JobContext};
    jobs.into_iter()
        .next()
        .unwrap()
        .handle(JobContext {
            app: app.ctx().clone(),
            job_id: "1".into(),
            attempt: 1,
        })
        .await
        .unwrap();
    assert_eq!(note_in(&app, agra, id).await.unwrap().title, "loud");
}

#[tokio::test]
async fn metrics_are_counted_and_cached_per_workspace() {
    let app = notes_app().await;
    let (agra, gulati) = (Uuid::now_v7(), Uuid::now_v7());
    seed(&app, agra, "one").await;
    seed(&app, agra, "two").await;
    seed(&app, gulati, "three").await;
    let editor = editor(&app).await;
    let value = |response: TestResponse| {
        response.assert_status(200).json_value()["value"]
            .as_f64()
            .unwrap()
    };
    // Asked in turn, so that a cached result would show.
    for _ in 0..2 {
        let metric = in_workspace(&editor, agra, "/admin/api/resources/notes/metrics/notes")
            .send()
            .await;
        assert_eq!(value(metric), 2.0);
        let metric = in_workspace(&editor, gulati, "/admin/api/resources/notes/metrics/notes")
            .send()
            .await;
        assert_eq!(value(metric), 1.0);
    }
}

#[tokio::test]
async fn the_form_in_the_sidebar_picks_the_workspace_of_a_browser() {
    let app = notes_app().await;
    let (agra, gulati) = (Uuid::now_v7(), Uuid::now_v7());
    seed(&app, agra, "Agra only").await;
    seed(&app, gulati, "Gulati only").await;
    let editor = editor(&app).await;
    let page = |path: &str| editor.get(path).header("accept", "text/html").send();

    // A cookie names the workspace.
    let response = editor
        .post("/admin/workspace")
        .header("accept", "text/html")
        .form(&[("workspace", agra.to_string().as_str())])
        .send()
        .await
        .assert_status(303);
    let cookie = response.set_cookie(WORKSPACE_COOKIE).expect("a cookie");
    assert_eq!(cookie.value(), agra.to_string());
    assert!(cookie.http_only().unwrap_or(false));
    let listed = page("/admin/resources/notes")
        .await
        .assert_status(200)
        .text();
    assert!(
        listed.contains("Agra only") && !listed.contains("Gulati only"),
        "{listed}"
    );

    // Another workspace, another page; an empty field leaves the workspace.
    editor
        .post("/admin/workspace")
        .header("accept", "text/html")
        .form(&[("workspace", gulati.to_string().as_str())])
        .send()
        .await
        .assert_status(303);
    let listed = page("/admin/resources/notes")
        .await
        .assert_status(200)
        .text();
    assert!(
        listed.contains("Gulati only") && !listed.contains("Agra only"),
        "{listed}"
    );
    let response = editor
        .post("/admin/workspace")
        .header("accept", "text/html")
        .form(&[("workspace", "")])
        .send()
        .await
        .assert_status(303);
    assert_eq!(response.set_cookie(WORKSPACE_COOKIE).unwrap().value(), "");
    // Something that is not a workspace id changes nothing.
    let response = editor
        .post("/admin/workspace")
        .header("accept", "text/html")
        .form(&[("workspace", "agra")])
        .send()
        .await
        .assert_status(303);
    assert!(response.set_cookie(WORKSPACE_COOKIE).is_none());
    // A header that is not a workspace id is no workspace.
    editor
        .get("/admin/api/resources/notes")
        .header("X-Workspace", "agra")
        .send()
        .await
        .assert_status(400);
}

#[tokio::test]
async fn a_resource_across_workspaces_is_asked_for_and_writes_in_the_workspace_of_the_record() {
    if Server::current() == Server::Postgres {
        // Under row-level security the database decides what it sees: the
        // Postgres tests below cover it.
        return;
    }
    let app = notes_app().await;
    let (agra, gulati) = (Uuid::now_v7(), Uuid::now_v7());
    let a1 = seed(&app, agra, "Agra one").await;
    let g1 = seed(&app, gulati, "Gulati one").await;
    let editor = editor(&app).await;

    // No workspace entered: every workspace's records, because the resource asked for that.
    let body = editor
        .get("/admin/api/resources/all-notes")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(titles_of(&body), ["Agra one", "Gulati one"]);

    // A change is written in the workspace of the record, whichever it is.
    for (id, workspace, title) in [(a1, agra, "Agra edited"), (g1, gulati, "Gulati edited")] {
        editor
            .patch(&format!("/admin/api/resources/all-notes/{id}"))
            .json(&json!({ "title": title }))
            .send()
            .await
            .assert_status(200);
        assert_eq!(note_in(&app, workspace, id).await.unwrap().title, title);
    }
    editor
        .delete(&format!("/admin/api/resources/all-notes/{g1}"))
        .send()
        .await
        .assert_status(204);
    assert!(note_in(&app, gulati, g1).await.is_none());
    assert!(note_in(&app, agra, a1).await.is_some());

    // A create names its workspace.
    editor
        .post("/admin/api/resources/all-notes")
        .json(&json!({ "title": "No workspace" }))
        .send()
        .await
        .assert_status(400);
    let created = editor
        .post("/admin/api/resources/all-notes")
        .json(&json!({ "title": "In Gulati", "workspace_id": gulati }))
        .send()
        .await
        .assert_status(201)
        .json_value();
    let id: Uuid = created["data"]["id"].as_str().unwrap().parse().unwrap();
    assert_eq!(
        note_in(&app, gulati, id).await.unwrap().workspace_id,
        gulati
    );
    // With a workspace entered, the form cannot name another.
    editor
        .post("/admin/api/resources/all-notes")
        .header("X-Workspace", agra.to_string())
        .json(&json!({ "title": "Mixed", "workspace_id": gulati }))
        .send()
        .await
        .assert_status(400);
}

// ---------------------------------------------------------------------------
// PostgreSQL row-level security, with a role that does not bypass it
// ---------------------------------------------------------------------------

/// A database on the Postgres test server that the app connects to as a role
/// of its own: not a superuser, and (unless asked) without `BYPASSRLS`. The
/// database and the role are dropped with it; nothing else on the server is
/// touched.
struct RlsDatabase {
    admin: String,
    role: String,
    name: String,
    url: String,
}

impl RlsDatabase {
    fn create(bypass: bool) -> RlsDatabase {
        let admin = std::env::var("NAVREE_TEST_POSTGRES_URL")
            .ok()
            .filter(|url| !url.is_empty())
            .expect("NAVREE_TEST_DATABASE=postgres needs NAVREE_TEST_POSTGRES_URL");
        let unique = Uuid::now_v7().simple().to_string();
        let role = format!("navree_helm_rls_{unique}");
        let name = format!("navree_helm_rls_{unique}");
        let password = format!("pw{unique}");
        let attribute = if bypass { "BYPASSRLS" } else { "NOBYPASSRLS" };
        run_admin(
            &admin,
            format!("CREATE ROLE {role} LOGIN NOSUPERUSER {attribute} PASSWORD '{password}'"),
        )
        .unwrap_or_else(|error| panic!("{error}"));
        run_admin(&admin, format!("CREATE DATABASE {name} OWNER {role}"))
            .unwrap_or_else(|error| panic!("{error}"));
        // postgres://admin:secret@host:port/database -> postgres://role:password@host:port/name
        let base = admin.split('?').next().unwrap_or(&admin);
        let (scheme, rest) = base.split_once("://").expect("a database URL");
        let (_credentials, location) = rest.rsplit_once('@').expect("a URL with credentials");
        let (host, _) = location.split_once('/').expect("a URL with a database");
        let url = format!("{scheme}://{role}:{password}@{host}/{name}");
        RlsDatabase {
            admin,
            role,
            name,
            url,
        }
    }
}

impl Drop for RlsDatabase {
    fn drop(&mut self) {
        let dropped = run_admin(
            &self.admin,
            format!("DROP DATABASE IF EXISTS {} WITH (FORCE)", self.name),
        )
        .and_then(|()| run_admin(&self.admin, format!("DROP ROLE IF EXISTS {}", self.role)));
        if let Err(error) = dropped {
            eprintln!("could not drop a test database or role: {error}");
        }
    }
}

/// An app on a fresh database of `database`'s role.
async fn rls_app(database: RlsDatabase) -> TestApp {
    let builder = TestApp::builder()
        .config("database.url", database.url.clone())
        .config("database.migrate_on_boot", true)
        .config("database.max_connections", 4)
        .hold(database);
    with_notes(builder).build().await
}

/// Whether the connection runs as a role that row-level security binds.
async fn assert_bound_by_row_level_security(app: &TestApp) {
    use navree_db::Executor as _;
    let db = app.get::<Db>().await;
    let row = db
        .fetch_optional(&navree_db::Statement::text(
            "SELECT rolsuper AS super, rolbypassrls AS bypass FROM pg_roles WHERE rolname = current_user",
        ))
        .await
        .unwrap()
        .expect("the connection's role");
    assert!(
        !row.get::<bool>("super").unwrap(),
        "the role is a superuser"
    );
    assert!(
        !row.get::<bool>("bypass").unwrap(),
        "the role bypasses row-level security"
    );
}

fn postgres_only() -> bool {
    if Server::current() != Server::Postgres {
        eprintln!("skipped: needs NAVREE_TEST_DATABASE=postgres");
        return false;
    }
    true
}

#[tokio::test]
async fn under_row_level_security_the_panel_sees_and_changes_the_workspace_of_the_request_only() {
    if !postgres_only() {
        return;
    }
    let app = rls_app(RlsDatabase::create(false)).await;
    assert_bound_by_row_level_security(&app).await;
    let (agra, gulati) = (Uuid::now_v7(), Uuid::now_v7());
    let a1 = seed(&app, agra, "Agra one").await;
    seed(&app, agra, "Agra two").await;
    let g1 = seed(&app, gulati, "Gulati one").await;
    let editor = editor(&app).await;

    // The database hides the other workspace from a session that names none.
    let db = app.get::<Db>().await;
    {
        use navree_db::Executor as _;
        let counted = db
            .fetch_optional(&navree_db::Statement::text(
                "SELECT count(*) AS n FROM notes",
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            counted.get::<i64>("n").unwrap(),
            0,
            "a session without a workspace sees rows"
        );
    }

    // Reads.
    editor
        .get("/admin/api/resources/notes")
        .send()
        .await
        .assert_status(400);
    let body = in_workspace(&editor, agra, "/admin/api/resources/notes")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(titles_of(&body), ["Agra one", "Agra two"]);
    in_workspace(&editor, agra, &format!("/admin/api/resources/notes/{g1}"))
        .send()
        .await
        .assert_error(404, None);
    let metric = in_workspace(&editor, gulati, "/admin/api/resources/notes/metrics/notes")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(metric["value"].as_f64(), Some(1.0));

    // Writes: make, edit, act, delete, all in the request's workspace.
    let created = editor
        .post("/admin/api/resources/notes")
        .header("X-Workspace", agra.to_string())
        .json(&json!({ "title": "Made under RLS" }))
        .send()
        .await
        .assert_status(201)
        .json_value();
    let made: Uuid = created["data"]["id"].as_str().unwrap().parse().unwrap();
    assert_eq!(note_in(&app, agra, made).await.unwrap().workspace_id, agra);
    assert!(note_in(&app, gulati, made).await.is_none());
    editor
        .patch(&format!("/admin/api/resources/notes/{made}"))
        .header("X-Workspace", agra.to_string())
        .json(&json!({ "title": "Edited under RLS" }))
        .send()
        .await
        .assert_status(200);
    assert_eq!(
        note_in(&app, agra, made).await.unwrap().title,
        "Edited under RLS"
    );
    editor
        .post("/admin/api/resources/notes/actions/shout")
        .header("X-Workspace", agra.to_string())
        .json(&json!({ "ids": [a1] }))
        .send()
        .await
        .assert_status(200);
    assert_eq!(note_in(&app, agra, a1).await.unwrap().title, "AGRA ONE");
    editor
        .delete(&format!("/admin/api/resources/notes/{made}"))
        .header("X-Workspace", agra.to_string())
        .send()
        .await
        .assert_status(204);
    assert!(note_in(&app, agra, made).await.is_none());

    // The other workspace's note is out of reach of every write.
    editor
        .patch(&format!("/admin/api/resources/notes/{g1}"))
        .header("X-Workspace", agra.to_string())
        .json(&json!({ "title": "Taken over" }))
        .send()
        .await
        .assert_error(404, None);
    editor
        .delete(&format!("/admin/api/resources/notes/{g1}"))
        .header("X-Workspace", agra.to_string())
        .send()
        .await
        .assert_error(404, None);
    assert_eq!(note_in(&app, gulati, g1).await.unwrap().title, "Gulati one");
}

#[tokio::test]
async fn under_row_level_security_a_resource_across_workspaces_sees_what_the_database_allows() {
    if !postgres_only() {
        return;
    }
    // A role that is bound by the policies: the resource sees nothing, and changes nothing.
    let bound = rls_app(RlsDatabase::create(false)).await;
    let (agra, gulati) = (Uuid::now_v7(), Uuid::now_v7());
    let a1 = seed(&bound, agra, "Agra one").await;
    seed(&bound, gulati, "Gulati one").await;
    let editor_bound = editor(&bound).await;
    editor_bound
        .get("/admin/api/resources/all-notes")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("meta.total", json!(0));
    editor_bound
        .patch(&format!("/admin/api/resources/all-notes/{a1}"))
        .json(&json!({ "title": "Not mine" }))
        .send()
        .await
        .assert_error(404, None);
    assert_eq!(note_in(&bound, agra, a1).await.unwrap().title, "Agra one");

    // A maintenance role that bypasses them sees every workspace, and
    // each write still runs inside the workspace of its record.
    let bypassing = rls_app(RlsDatabase::create(true)).await;
    let a1 = seed(&bypassing, agra, "Agra one").await;
    let g1 = seed(&bypassing, gulati, "Gulati one").await;
    let editor_all = editor(&bypassing).await;
    let body = editor_all
        .get("/admin/api/resources/all-notes")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(titles_of(&body), ["Agra one", "Gulati one"]);
    for (id, workspace, title) in [(a1, agra, "Agra edited"), (g1, gulati, "Gulati edited")] {
        editor_all
            .patch(&format!("/admin/api/resources/all-notes/{id}"))
            .json(&json!({ "title": title }))
            .send()
            .await
            .assert_status(200);
        assert_eq!(
            note_in(&bypassing, workspace, id).await.unwrap().title,
            title
        );
    }
    editor_all
        .delete(&format!("/admin/api/resources/all-notes/{g1}"))
        .send()
        .await
        .assert_status(204);
    assert!(note_in(&bypassing, gulati, g1).await.is_none());
    let created = editor_all
        .post("/admin/api/resources/all-notes")
        .json(&json!({ "title": "In Gulati", "workspace_id": gulati }))
        .send()
        .await
        .assert_status(201)
        .json_value();
    let id: Uuid = created["data"]["id"].as_str().unwrap().parse().unwrap();
    assert_eq!(
        note_in(&bypassing, gulati, id).await.unwrap().workspace_id,
        gulati
    );
}
