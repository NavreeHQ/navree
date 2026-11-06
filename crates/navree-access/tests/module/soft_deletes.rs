//! A soft-deleted record is missing to every check that loads one by id: a
//! 404 before any rule is looked at, so a user the rules deny cannot tell
//! that it was there.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use http::Method;
use navree_access::{
    AbilityCtx, AbilityFactory, AccessModule, Authorized, READ, Read, Rules, Subject,
    SubjectRecords, field, row_json,
};
use navree_contracts::{BoxError, BoxFuture};
use navree_db::navree_schema::{Dialect, Migration, Schema, SchemaMigration, SchemaResult};
use navree_db::record;
use navree_db::{Db, MigrationSet, ModuleBuilderDbExt, Record, Uow};
use navree_http::{Controller, Json, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_testing::{TestApp, principal};
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::{RecordingAudit, options, timestamp};

navree_db::record! {
    /// A note (`notes`), soft-deleted by setting `deleted_at`.
    #[derive(Clone, Debug, PartialEq, Eq, Serialize)]
    pub struct Note in "notes" {
        pub id: Uuid,
        pub author_id: Uuid,
        pub title: String,
        pub deleted_at: Option<DateTime<Utc>>,
    }
}

impl Subject for Note {
    const SUBJECT: &'static str = "Note";
    const PERMISSION_PREFIX: &'static str = "notes";
}

/// Authors read their own notes.
fn note_rules(ctx: &AbilityCtx, rules: &mut Rules) {
    if let Some(user) = &ctx.user {
        rules.can(READ, "Note").when(field("author_id").eq(user.id));
    }
}

/// Loaded by id with no filter of its own: the loader's `deleted_at` check
/// is what leaves deleted notes out.
impl SubjectRecords for Note {
    type Record = Note;

    fn load<'a>(uow: &'a Uow, id: &'a str) -> BoxFuture<'a, Result<Option<Note>, BoxError>> {
        Box::pin(async move {
            let Ok(id) = Uuid::parse_str(id) else {
                return Ok(None);
            };
            Ok(Note::select().eq("id", id).one(uow).await?)
        })
    }

    fn record_json(record: &Note) -> Value {
        row_json(record)
    }

    fn policy() -> Option<fn(&AbilityCtx, &mut Rules)> {
        Some(note_rules)
    }
}

/// Creates `notes`.
struct CreateNotes;

impl Migration for CreateNotes {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let mysql = schema.target().await?.dialect() == Dialect::MySql;
        schema
            .create("notes", |t| {
                if mysql {
                    t.binary("id").length(16).fixed().primary();
                    t.binary("author_id").length(16).fixed();
                } else {
                    t.uuid("id").primary();
                    t.uuid("author_id");
                }
                t.string("title");
                t.timestamp_tz("deleted_at").nullable();
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop("notes").await
    }
}

struct NoteController;

impl Injectable for NoteController {
    fn construct(_r: Resolver) -> navree_kernel::BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(NoteController) })
    }
}

impl Controller for NoteController {
    fn prefix(&self) -> &'static str {
        "/notes"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![RouteDef::on(Method::GET, "/{id}", show).can(READ, "Note", Some("id"))]
    }
}

async fn show(note: Authorized<Note, Read>) -> Json<Note> {
    Json(note.into_inner())
}

struct NotesModule;

impl Module for NotesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.import(AccessModule::for_feature::<Note>())
            .migrations(MigrationSet::new("notes").migration(SchemaMigration::new(
                "m20261106_000001_create_notes",
                CreateNotes,
            )))
            .controller::<NoteController>();
    }
}

async fn app() -> TestApp {
    crate::database::database(TestApp::builder())
        .config("http.per_ip_rate_limit", Value::Null)
        .module(navree_db::DatabaseModule)
        .module(AccessModule::for_root(options()))
        .module(NotesModule)
        .override_value::<dyn navree_contracts::AuditSink>(Arc::new(RecordingAudit::default()))
        .build()
        .await
}

async fn insert_note(app: &TestApp, author: Uuid, deleted: bool) -> Note {
    let db = app.get::<Db>().await;
    record::insert_returning(
        &*db,
        &Note {
            id: Uuid::now_v7(),
            author_id: author,
            title: "Quarterly figures for Ludhiana".to_owned(),
            deleted_at: deleted.then(|| timestamp("2026-11-06T04:30:00.000000Z")),
        },
    )
    .await
    .expect("insert note")
}

#[tokio::test]
async fn a_soft_deleted_record_is_a_404_whoever_asks() {
    let app = app().await;
    let (author, other) = (Uuid::now_v7(), Uuid::now_v7());
    let live = insert_note(&app, author, false).await;
    let gone = insert_note(&app, author, true).await;

    // A live note: its author reads it, anyone else is denied.
    app.acting_as(principal(author))
        .get(&format!("/api/notes/{}", live.id))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("title", json!("Quarterly figures for Ludhiana"));
    app.acting_as(principal(other))
        .get(&format!("/api/notes/{}", live.id))
        .send()
        .await
        .assert_error(403, None);

    // A deleted note: the user the rules deny gets the 404 a missing note
    // gets, not a 403 that says it exists, and so does its author.
    for user in [other, author] {
        app.acting_as(principal(user))
            .get(&format!("/api/notes/{}", gone.id))
            .send()
            .await
            .assert_error(404, None);
    }
}

#[tokio::test]
async fn outside_a_request_a_soft_deleted_record_is_missing_too() {
    let app = app().await;
    let author = Uuid::now_v7();
    let gone = insert_note(&app, author, true).await;
    let live = insert_note(&app, author, false).await;
    let factory = app.get::<AbilityFactory>().await;
    let db = app.get::<Db>().await;
    let access = factory.access_for(Some(&principal(author))).await.unwrap();
    let uow = db.uow(Some(author), None).await;

    let missing = factory
        .authorize_record(&access, &uow, READ, "Note", &gone.id.to_string())
        .await
        .unwrap_err();
    assert_eq!(missing.status.as_u16(), 404);
    let missing =
        Authorized::<Note, Read>::load(&uow, access.ability.clone(), &gone.id.to_string())
            .await
            .unwrap_err();
    assert_eq!(missing.status.as_u16(), 404);
    let found = Authorized::<Note, Read>::load(&uow, access.ability.clone(), &live.id.to_string())
        .await
        .expect("a live note");
    assert_eq!(found.id, live.id);
    uow.rollback().await.unwrap();
}

/// A subject looks at `deleted_at` unless it names another member.
#[test]
fn the_deleted_at_member_is_the_default() {
    assert_eq!(<Note as SubjectRecords>::DELETED_AT, Some("deleted_at"));
}
