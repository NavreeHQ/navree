//! Record checks for any access subject, not only ORM models: a subject
//! whose records load through its own `SubjectRecords` (as ORM models'
//! do) works with `cx.authorize`, `cx.accessible` and
//! `can(.., from = ..)` items.

use std::sync::{Mutex, OnceLock};

use navree_access::{
    AbilityCtx, AccessModule, Authorized, READ, Read, Rules, Subject, SubjectRecords, field,
};
use navree_contracts::{BoxError, BoxFuture};
use navree_db::Uow;
use navree_http::AppError;
use navree_kernel::{Module, ModuleBuilder};
use navree_macros::{injectable, mcp_server};
use navree_mcp::testing::McpTestClient;
use navree_mcp::{McpContext, ModuleBuilderMcpExt};
use navree_validation::Validate;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::{builder, user};

/// Notes kept in memory: a subject that is not an entity.
struct Note;

/// One note.
#[derive(Clone, Debug)]
struct NoteRecord {
    id: Uuid,
    owner_id: Uuid,
    text: String,
}

/// Every note the tests made (each test makes notes of its own).
fn notes() -> &'static Mutex<Vec<NoteRecord>> {
    static NOTES: OnceLock<Mutex<Vec<NoteRecord>>> = OnceLock::new();
    NOTES.get_or_init(Mutex::default)
}

/// Keeps a note of `owner`; returns its id.
fn keep_note(owner: Uuid, text: &str) -> Uuid {
    let id = Uuid::now_v7();
    notes().lock().unwrap().push(NoteRecord {
        id,
        owner_id: owner,
        text: text.to_owned(),
    });
    id
}

impl Subject for Note {
    const SUBJECT: &'static str = "Note";
    const PERMISSION_PREFIX: &'static str = "notes";
}

impl SubjectRecords for Note {
    type Record = NoteRecord;

    fn load<'a>(_uow: &'a Uow, id: &'a str) -> BoxFuture<'a, Result<Option<NoteRecord>, BoxError>> {
        Box::pin(async move {
            let Ok(id) = Uuid::parse_str(id) else {
                return Ok(None);
            };
            Ok(notes()
                .lock()
                .unwrap()
                .iter()
                .find(|note| note.id == id)
                .cloned())
        })
    }

    fn record_json(record: &NoteRecord) -> Value {
        json!({ "id": record.id, "owner_id": record.owner_id })
    }

    /// Users read their own notes.
    fn policy() -> Option<fn(&AbilityCtx, &mut Rules)> {
        Some(|ctx, rules| {
            if let Some(user) = &ctx.user {
                rules.can(READ, "Note").when(field("owner_id").eq(user.id));
            }
        })
    }
}

#[derive(Debug, Deserialize, Validate, navree_openapi::ToSchema)]
struct NoteArgs {
    /// The note to read.
    #[validate(skip)]
    note_id: Uuid,
}

/// The notes part of an MCP server.
#[injectable]
struct NotesMcp;

#[mcp_server]
impl NotesMcp {
    /// A note's text, loaded and checked by the tool.
    #[mcp_tool(read_only)]
    #[can(Read, Note)]
    async fn read_note(&self, cx: &McpContext, args: NoteArgs) -> Result<String, AppError> {
        let note = cx.authorize::<Note, Read>(args.note_id).await?;
        Ok(note.text.clone())
    }

    /// Whether the user may read some notes (the list filter of a subject
    /// that is not an entity).
    #[mcp_tool(read_only)]
    #[authenticated]
    async fn reads_notes(&self, cx: &McpContext) -> Result<bool, AppError> {
        Ok(cx
            .accessible::<Note, Read>()
            .ability()
            .can(READ, "Note", None))
    }

    /// A note, loaded and checked before the method.
    #[mcp_resource("notes://{id}", mime = "text/plain")]
    #[can(Read, Note, from = "id")]
    async fn kept_note(&self, note: Authorized<Note, Read>) -> Result<String, AppError> {
        Ok(note.text.clone())
    }
}

/// The notes feature: its subject and its MCP items.
struct NotesModule;

impl Module for NotesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.import(AccessModule::for_feature::<Note>())
            .mcp_server::<NotesMcp>();
    }
}

#[tokio::test]
async fn records_of_any_subject_are_loaded_and_checked() {
    let app = builder().module(NotesModule).build().await;
    let owner = user();
    let mine = keep_note(owner.user_id, "Call Acme back");
    let theirs = keep_note(Uuid::now_v7(), "Someone else's");
    let mcp = McpTestClient::new(app.acting_as(owner.clone()))
        .initialize()
        .await;

    // `cx.authorize` in a tool.
    let read = mcp
        .call_tool("read_note", json!({ "note_id": mine }))
        .await
        .assert_ok();
    assert_eq!(read.text(), "Call Acme back");
    mcp.call_tool("read_note", json!({ "note_id": theirs }))
        .await
        .assert_error(403, "Forbidden");
    mcp.call_tool("read_note", json!({ "note_id": Uuid::now_v7() }))
        .await
        .assert_error(404, "Not Found");

    // `can(Read, Note, from = "id")` on a resource.
    let note = mcp
        .read_resource(&format!("notes://{mine}"))
        .await
        .assert_ok();
    assert_eq!(note.text().as_deref(), Some("Call Acme back"));
    mcp.read_resource(&format!("notes://{theirs}"))
        .await
        .assert_error(403, "Forbidden");

    // `cx.accessible` hands over the principal's abilities.
    let reads = mcp.call_tool("reads_notes", json!({})).await.assert_ok();
    assert_eq!(reads.json_value(), json!(true));
}
