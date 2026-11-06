//! What an assistant's page reads: the `ai_runs`, `ai_items` and `ai_action_uses` tables
//! and the [`Store`] over them.
//!
//! A conversation's page shows its **items**: the reader's messages, the assistant's
//! answers as they grow, the tools it called, the components it chose, its approvals and
//! the notes between. Each write takes the conversation's next **revision** inside the
//! transaction that writes it, from the conversation's row in `ai_runs`, which only one
//! transaction can update at a time: a page that asks for what changed since a revision
//! never misses a write that commits after it. The row also says whether a run is
//! going, and whether someone asked it to stop, so a stop reaches a run in any process.

use std::sync::Arc;

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use navree_db::navree_schema::{
    Blueprint, ColumnBuilder, Dialect, Migration, Schema, SchemaMigration, SchemaResult,
};
use navree_db::record::{self, Compare, Filter};
use navree_db::{Db, DbError, ErrorKind, ExecutorExt, Record, Statement, Uow, Value};
use serde_json::{Value as Json, json};
use uuid::Uuid;

use navree_http::AppError;

/// The migration that makes the assistant's tables, in the `ai` set.
pub(crate) fn migration() -> SchemaMigration<CreateItemTables> {
    SchemaMigration::new("m20261106_000001_create_ai_items", CreateItemTables)
}

/// A UUID column: `BINARY(16)` on MySQL and MariaDB, `uuid` elsewhere.
fn uuid<'a>(t: &'a mut Blueprint, name: &str, dialect: Dialect) -> ColumnBuilder<'a> {
    if dialect == Dialect::MySql {
        t.binary(name).length(16).fixed()
    } else {
        t.uuid(name)
    }
}

/// A short text column: `VARCHAR(64)` on MySQL and MariaDB, `VARCHAR` elsewhere.
fn short<'a>(t: &'a mut Blueprint, name: &str, dialect: Dialect) -> ColumnBuilder<'a> {
    if dialect == Dialect::MySql {
        t.string(name).length(64)
    } else {
        t.raw_column(name, "VARCHAR")
    }
}

/// Creates `ai_runs`, `ai_items` and `ai_action_uses`.
pub(crate) struct CreateItemTables;

impl Migration for CreateItemTables {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let dialect = schema.target().await?.dialect();
        schema
            .create("ai_runs", |t| {
                uuid(t, "conversation_id", dialect).primary();
                uuid(t, "user_id", dialect);
                t.big_integer("revision").default(0);
                short(t, "status", dialect);
                uuid(t, "run_id", dialect).nullable();
                t.integer("cancel").default(0);
                t.text("state").nullable();
                t.text("notes").nullable();
                t.timestamp_tz("updated_at");
                t.foreign("conversation_id")
                    .references("id")
                    .on("ai_conversations")
                    .name("ai_runs_conversation_id_fk")
                    .cascade_on_delete();
            })
            .await?;
        schema
            .create("ai_items", |t| {
                uuid(t, "id", dialect).primary();
                uuid(t, "conversation_id", dialect);
                short(t, "kind", dialect);
                short(t, "status", dialect);
                t.text("content");
                t.big_integer("revision");
                t.timestamp_tz("created_at");
                t.timestamp_tz("updated_at");
                t.foreign("conversation_id")
                    .references("id")
                    .on("ai_conversations")
                    .name("ai_items_conversation_id_fk")
                    .cascade_on_delete();
                t.index(["conversation_id", "revision"])
                    .name("ai_items_conversation_revision_index");
            })
            .await?;
        schema
            .create("ai_action_uses", |t| {
                short(t, "nonce", dialect).primary();
                uuid(t, "item_id", dialect);
                uuid(t, "user_id", dialect);
                t.timestamp_tz("created_at");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists("ai_action_uses").await?;
        schema.drop_if_exists("ai_items").await?;
        schema.drop_if_exists("ai_runs").await
    }
}

navree_db::record! {
    /// A conversation's run state.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub(crate) struct RunRow in "ai_runs" key "conversation_id" {
        pub(crate) conversation_id: Uuid,
        pub(crate) user_id: Uuid,
        pub(crate) revision: i64,
        /// `idle` or `running`.
        pub(crate) status: String,
        pub(crate) run_id: Option<Uuid>,
        /// 1 when someone asked the run to stop.
        pub(crate) cancel: i32,
        /// The working state, as JSON.
        pub(crate) state: Option<String>,
        /// Notes for the assistant's next run, as a JSON list.
        pub(crate) notes: Option<String>,
        pub(crate) updated_at: DateTime<Utc>,
    }
}

navree_db::record! {
    /// One item of a conversation's page.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub(crate) struct ItemRow in "ai_items" {
        pub(crate) id: Uuid,
        pub(crate) conversation_id: Uuid,
        /// `user`, `assistant`, `tool`, `surface`, `approval` or `notice`.
        pub(crate) kind: String,
        pub(crate) status: String,
        /// What the kind holds, as JSON.
        pub(crate) content: String,
        pub(crate) revision: i64,
        pub(crate) created_at: DateTime<Utc>,
        pub(crate) updated_at: DateTime<Utc>,
    }
}

navree_db::record! {
    /// A button's post that ran: its nonce, so it never runs twice.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub(crate) struct UseRow in "ai_action_uses" key "nonce" {
        pub(crate) nonce: String,
        pub(crate) item_id: Uuid,
        pub(crate) user_id: Uuid,
        pub(crate) created_at: DateTime<Utc>,
    }
}

impl ItemRow {
    /// The item's content.
    pub(crate) fn json(&self) -> Json {
        serde_json::from_str(&self.content).unwrap_or(Json::Null)
    }

    /// Text field `name` of the content.
    pub(crate) fn text(&self, name: &str) -> String {
        self.json()
            .get(name)
            .and_then(Json::as_str)
            .unwrap_or_default()
            .to_owned()
    }
}

/// The kinds of item.
pub(crate) mod kind {
    pub(crate) const USER: &str = "user";
    pub(crate) const ASSISTANT: &str = "assistant";
    pub(crate) const TOOL: &str = "tool";
    pub(crate) const SURFACE: &str = "surface";
    pub(crate) const APPROVAL: &str = "approval";
    pub(crate) const NOTICE: &str = "notice";
}

/// The states of an item.
pub(crate) mod status {
    pub(crate) const DONE: &str = "done";
    pub(crate) const WRITING: &str = "writing";
    /// An answer with no words, or a tool's card an approval took the place of: kept,
    /// and drawn as nothing.
    pub(crate) const EMPTY: &str = "empty";
    pub(crate) const STOPPED: &str = "stopped";
    pub(crate) const FAILED: &str = "failed";
    pub(crate) const RUNNING: &str = "running";
    pub(crate) const PENDING: &str = "pending";
    pub(crate) const APPROVED: &str = "approved";
    pub(crate) const REJECTED: &str = "rejected";
}

/// The run states.
pub(crate) const IDLE: &str = "idle";
pub(crate) const RUNNING: &str = "running";

/// How long a run may go without a write before another may start in its place: the
/// process that ran it is taken to have gone.
const STALE_AFTER_MINUTES: i64 = 5;

/// The most items one page of a conversation holds.
pub(crate) const PAGE: u64 = 60;

fn db_error(error: DbError) -> AppError {
    AppError::internal(format!("assistant items: {error}"))
}

/// The answer for a conversation that isn't the reader's, or isn't there.
pub(crate) fn no_conversation() -> AppError {
    AppError::not_found().with_detail("no such conversation")
}

/// The conversation's items and run state.
#[derive(Clone)]
pub(crate) struct Store {
    db: Arc<Db>,
}

/// A run that began: its id, and the answer it writes into.
pub(crate) struct Begun {
    pub(crate) run_id: Uuid,
    pub(crate) answer: Uuid,
}

impl Store {
    pub(crate) fn new(db: Arc<Db>) -> Self {
        Self { db }
    }

    /// A unit of work for `user`.
    pub(crate) async fn uow(&self, user: Uuid) -> Uow {
        self.db.uow(Some(user), None).await
    }

    /// The conversation's next revision, taken in `uow`'s transaction from its row,
    /// which the update holds until the transaction ends.
    pub(crate) async fn bump(&self, uow: &Uow, conversation: Uuid) -> Result<i64, AppError> {
        let changed = uow
            .execute(&Statement::portable(
                "UPDATE ai_runs SET revision = revision + 1, updated_at = ? WHERE conversation_id = ?",
                [Value::from(Utc::now()), Value::from(conversation)],
            ))
            .await
            .map_err(db_error)?;
        if changed.rows_affected == 0 {
            return Err(no_conversation());
        }
        uow.fetch_scalar::<i64>(&Statement::portable(
            "SELECT revision FROM ai_runs WHERE conversation_id = ?",
            [Value::from(conversation)],
        ))
        .await
        .map_err(db_error)
    }

    /// The conversation's run row, made when it has none yet.
    pub(crate) async fn run(&self, conversation: Uuid, user: Uuid) -> Result<RunRow, AppError> {
        if let Some(row) = record::find::<RunRow, _>(&*self.db, conversation)
            .await
            .map_err(db_error)?
        {
            return Ok(row);
        }
        let row = RunRow {
            conversation_id: conversation,
            user_id: user,
            revision: 0,
            status: IDLE.to_owned(),
            run_id: None,
            cancel: 0,
            state: None,
            notes: None,
            updated_at: Utc::now(),
        };
        match record::insert(&*self.db, &row).await {
            Ok(()) => Ok(row),
            // Two first messages at once: the other made it.
            Err(error) if error.kind() == ErrorKind::UniqueViolation => {
                record::find::<RunRow, _>(&*self.db, conversation)
                    .await
                    .map_err(db_error)?
                    .ok_or_else(|| AppError::internal("the assistant's run row went missing"))
            }
            Err(error) => Err(db_error(error)),
        }
    }

    /// The run row of `user`'s conversation, or a 404.
    pub(crate) async fn owned(&self, conversation: Uuid, user: Uuid) -> Result<RunRow, AppError> {
        record::find::<RunRow, _>(&*self.db, conversation)
            .await
            .map_err(db_error)?
            .filter(|row| row.user_id == user)
            .ok_or_else(|| no_conversation())
    }

    /// Begins a run: refuses one while another goes (unless that one went quiet past
    /// [`STALE_AFTER_MINUTES`]), writes the reader's message when there is one, and the
    /// answer the run writes into.
    pub(crate) async fn begin(
        &self,
        conversation: Uuid,
        user: Uuid,
        message: Option<&str>,
    ) -> Result<Begun, AppError> {
        self.run(conversation, user).await?;
        let uow = self.uow(user).await;
        let revision = self.bump(&uow, conversation).await?;
        // The bump holds the row: what is read now no other run can change.
        let row = RunRow::select()
            .eq("conversation_id", conversation)
            .one(&uow)
            .await
            .map_err(db_error)?
            .ok_or_else(|| no_conversation())?;
        if row.user_id != user {
            let _ = uow.rollback().await;
            return Err(no_conversation());
        }
        let stale = Utc::now() - ChronoDuration::minutes(STALE_AFTER_MINUTES);
        if row.status == RUNNING && row.updated_at > stale && row.run_id.is_some() {
            // The bump moved `updated_at`; what decides is the time before it.
            let _ = uow.rollback().await;
            return Err(AppError::conflict("The assistant is still answering."));
        }
        let run_id = Uuid::now_v7();
        RunRow::update_where(Filter::all().eq("conversation_id", conversation))
            .set("status", RUNNING)
            .set("run_id", Some(run_id))
            .set("cancel", 0)
            .execute(&uow)
            .await
            .map_err(db_error)?;
        if let Some(text) = message {
            self.insert(
                &uow,
                conversation,
                kind::USER,
                status::DONE,
                json!({ "text": text }),
                revision,
            )
            .await?;
        }
        let answer = self
            .insert(
                &uow,
                conversation,
                kind::ASSISTANT,
                status::WRITING,
                json!({ "text": "" }),
                revision,
            )
            .await?;
        uow.commit().await.map_err(db_error)?;
        Ok(Begun { run_id, answer })
    }

    /// Ends run `run_id`, unless another took its place.
    pub(crate) async fn end(
        &self,
        conversation: Uuid,
        user: Uuid,
        run_id: Uuid,
    ) -> Result<(), AppError> {
        let uow = self.uow(user).await;
        self.bump(&uow, conversation).await?;
        RunRow::update_where(
            Filter::all()
                .eq("conversation_id", conversation)
                .eq("run_id", run_id),
        )
        .set("status", IDLE)
        .set("run_id", None::<Uuid>)
        .set("cancel", 0)
        .execute(&uow)
        .await
        .map_err(db_error)?;
        uow.commit().await.map_err(db_error)
    }

    /// Writes a new item at `revision`, in `uow`: its id.
    pub(crate) async fn insert(
        &self,
        uow: &Uow,
        conversation: Uuid,
        kind: &str,
        status: &str,
        content: Json,
        revision: i64,
    ) -> Result<Uuid, AppError> {
        let now = Utc::now();
        let row = ItemRow {
            id: Uuid::now_v7(),
            conversation_id: conversation,
            kind: kind.to_owned(),
            status: status.to_owned(),
            content: content.to_string(),
            revision,
            created_at: now,
            updated_at: now,
        };
        record::insert(uow, &row).await.map_err(db_error)?;
        Ok(row.id)
    }

    /// Writes a new item at the conversation's next revision.
    pub(crate) async fn add(
        &self,
        conversation: Uuid,
        user: Uuid,
        kind: &str,
        status: &str,
        content: Json,
    ) -> Result<Uuid, AppError> {
        let uow = self.uow(user).await;
        let revision = self.bump(&uow, conversation).await?;
        let id = self
            .insert(&uow, conversation, kind, status, content, revision)
            .await?;
        uow.commit().await.map_err(db_error)?;
        Ok(id)
    }

    /// Changes item `id` of the conversation, at its next revision: its status, and its
    /// content when given.
    pub(crate) async fn change(
        &self,
        conversation: Uuid,
        user: Uuid,
        id: Uuid,
        status: &str,
        content: Option<Json>,
    ) -> Result<(), AppError> {
        let uow = self.uow(user).await;
        self.change_in(&uow, conversation, id, status, content).await?;
        uow.commit().await.map_err(db_error)
    }

    /// [`change`](Self::change), in `uow`.
    pub(crate) async fn change_in(
        &self,
        uow: &Uow,
        conversation: Uuid,
        id: Uuid,
        status: &str,
        content: Option<Json>,
    ) -> Result<(), AppError> {
        let revision = self.bump(uow, conversation).await?;
        let mut update = ItemRow::update_where(
            Filter::all()
                .eq("id", id)
                .eq("conversation_id", conversation),
        )
        .set("status", status)
        .set("revision", revision)
        .set("updated_at", Utc::now());
        if let Some(content) = content {
            update = update.set("content", content.to_string());
        }
        update.execute(uow).await.map_err(db_error)?;
        Ok(())
    }

    /// Item `id` of the conversation.
    pub(crate) async fn item(
        &self,
        conversation: Uuid,
        id: Uuid,
    ) -> Result<Option<ItemRow>, AppError> {
        Ok(record::find::<ItemRow, _>(&*self.db, id)
            .await
            .map_err(db_error)?
            .filter(|row| row.conversation_id == conversation))
    }

    /// The newest items, oldest first, and whether there are earlier ones; or those
    /// before item `before`.
    pub(crate) async fn page(
        &self,
        conversation: Uuid,
        before: Option<Uuid>,
    ) -> Result<(Vec<ItemRow>, bool), AppError> {
        let mut filter = Filter::all().eq("conversation_id", conversation);
        if let Some(before) = before {
            filter = filter.compare("id", Compare::Lt, before);
        }
        let mut rows = ItemRow::select()
            .filter(filter)
            .order_by_desc("id")
            .limit(PAGE + 1)
            .all(&*self.db)
            .await
            .map_err(db_error)?;
        let more = rows.len() as u64 > PAGE;
        rows.truncate(PAGE as usize);
        rows.reverse();
        Ok((rows, more))
    }

    /// The items after item `after`, oldest first, and whether there are more.
    pub(crate) async fn after(
        &self,
        conversation: Uuid,
        after: Uuid,
    ) -> Result<(Vec<ItemRow>, bool), AppError> {
        let mut rows = ItemRow::select()
            .filter(
                Filter::all()
                    .eq("conversation_id", conversation)
                    .compare("id", Compare::Gt, after),
            )
            .order_by("id")
            .limit(PAGE + 1)
            .all(&*self.db)
            .await
            .map_err(db_error)?;
        let more = rows.len() as u64 > PAGE;
        rows.truncate(PAGE as usize);
        Ok((rows, more))
    }

    /// What changed since `revision`, oldest change first, at most a page; whether more
    /// changed after them.
    pub(crate) async fn changes(
        &self,
        conversation: Uuid,
        revision: i64,
    ) -> Result<(Vec<ItemRow>, bool), AppError> {
        let mut rows = ItemRow::select()
            .filter(
                Filter::all()
                    .eq("conversation_id", conversation)
                    .compare("revision", Compare::Gt, revision),
            )
            .order_by("revision")
            .order_by("id")
            .limit(PAGE + 1)
            .all(&*self.db)
            .await
            .map_err(db_error)?;
        let more = rows.len() as u64 > PAGE;
        rows.truncate(PAGE as usize);
        Ok((rows, more))
    }

    /// Asks the conversation's run to stop: the flag every process's run reads.
    pub(crate) async fn ask_to_stop(&self, conversation: Uuid) -> Result<(), AppError> {
        RunRow::update_where(
            Filter::all()
                .eq("conversation_id", conversation)
                .eq("status", RUNNING),
        )
        .set("cancel", 1)
        .execute(&*self.db)
        .await
        .map_err(db_error)?;
        Ok(())
    }

    /// Whether someone asked run `run_id` to stop, or another run took its place.
    pub(crate) async fn should_stop(&self, conversation: Uuid, run_id: Uuid) -> bool {
        match record::find::<RunRow, _>(&*self.db, conversation).await {
            Ok(Some(row)) => row.cancel != 0 || row.run_id != Some(run_id),
            Ok(None) => true,
            Err(error) => {
                tracing::warn!(target: "navree::ai", %error, "reading a run's stop flag failed");
                false
            }
        }
    }

    /// Records that a button's post `nonce` ran, in `uow`: a second post of it is a
    /// conflict.
    pub(crate) async fn use_nonce(
        &self,
        uow: &Uow,
        nonce: &str,
        item: Uuid,
        user: Uuid,
    ) -> Result<(), AppError> {
        let row = UseRow {
            nonce: nonce.to_owned(),
            item_id: item,
            user_id: user,
            created_at: Utc::now(),
        };
        match record::insert(uow, &row).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::UniqueViolation => {
                Err(AppError::conflict("This button was used already."))
            }
            Err(error) => Err(db_error(error)),
        }
    }

    /// The working state, as JSON.
    pub(crate) async fn state(&self, conversation: Uuid, user: Uuid) -> Result<Option<Json>, AppError> {
        let row = self.owned(conversation, user).await?;
        Ok(row.state.and_then(|text| serde_json::from_str(&text).ok()))
    }

    /// Replaces the working state, at the conversation's next revision.
    pub(crate) async fn set_state(
        &self,
        conversation: Uuid,
        user: Uuid,
        state: &Json,
    ) -> Result<(), AppError> {
        self.run(conversation, user).await?;
        let uow = self.uow(user).await;
        self.bump(&uow, conversation).await?;
        RunRow::update_where(
            Filter::all()
                .eq("conversation_id", conversation)
                .eq("user_id", user),
        )
        .set("state", Some(state.to_string()))
        .execute(&uow)
        .await
        .map_err(db_error)?;
        uow.commit().await.map_err(db_error)
    }

    /// Adds a note for the assistant's next run.
    pub(crate) async fn add_note(&self, uow: &Uow, conversation: Uuid, note: &str) -> Result<(), AppError> {
        let row = RunRow::select()
            .eq("conversation_id", conversation)
            .one(uow)
            .await
            .map_err(db_error)?
            .ok_or_else(|| no_conversation())?;
        let mut notes: Vec<String> = row
            .notes
            .as_deref()
            .and_then(|text| serde_json::from_str(text).ok())
            .unwrap_or_default();
        notes.push(note.chars().take(500).collect());
        if notes.len() > 10 {
            notes.drain(..notes.len() - 10);
        }
        RunRow::update_where(Filter::all().eq("conversation_id", conversation))
            .set("notes", Some(serde_json::to_string(&notes).unwrap_or_default()))
            .execute(uow)
            .await
            .map_err(db_error)?;
        Ok(())
    }

    /// Takes the notes for the next run.
    pub(crate) async fn take_notes(&self, conversation: Uuid) -> Result<Vec<String>, AppError> {
        let Some(row) = record::find::<RunRow, _>(&*self.db, conversation)
            .await
            .map_err(db_error)?
        else {
            return Ok(Vec::new());
        };
        let notes: Vec<String> = row
            .notes
            .as_deref()
            .and_then(|text| serde_json::from_str(text).ok())
            .unwrap_or_default();
        if !notes.is_empty() {
            RunRow::update_where(Filter::all().eq("conversation_id", conversation))
                .set("notes", None::<String>)
                .execute(&*self.db)
                .await
                .map_err(db_error)?;
        }
        Ok(notes)
    }
}
