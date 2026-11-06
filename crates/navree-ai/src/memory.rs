//! Agent conversation memory: the `ai_conversations` and `ai_messages`
//! tables (migration set `ai`) and the [`Conversations`] service.

use std::sync::Arc;

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use navree_db::navree_schema::{
    Blueprint, ColumnBuilder, Dialect, Migration, Schema, SchemaMigration, SchemaResult,
};
use navree_db::record::{self, Compare, Filter};
use navree_db::{Db, DbError, MigrationSet, Page, PageQuery, Record};
use navree_openapi::ToSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::agent::PendingApproval;
use crate::error::AiError;
use crate::message::{Message, Part, Role, ToolResult};

/// The name of the AI module's migration set.
pub const AI_MIGRATIONS: &str = "ai";

/// `ai_conversations`: one row per conversation.
pub(crate) mod conversation {
    use chrono::{DateTime, Utc};
    use uuid::Uuid;

    navree_db::record! {
        /// A conversation row.
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub(crate) struct Model in "ai_conversations" {
            pub(crate) id: Uuid,
            pub(crate) user_id: Uuid,
            pub(crate) agent: String,
            pub(crate) title: Option<String>,
            pub(crate) summary: Option<String>,
            pub(crate) summarized_count: i32,
            /// A [`Pending`](super::Pending) as JSON.
            pub(crate) pending: Option<String>,
            pub(crate) created_at: DateTime<Utc>,
            pub(crate) updated_at: DateTime<Utc>,
        }
    }
}

/// `ai_messages`: the messages of a conversation, in order.
pub(crate) mod message {
    use chrono::{DateTime, Utc};
    use uuid::Uuid;

    navree_db::record! {
        /// A message row.
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub(crate) struct Model in "ai_messages" {
            pub(crate) id: Uuid,
            pub(crate) conversation_id: Uuid,
            pub(crate) role: String,
            /// The [`Part`](crate::Part)s as JSON.
            pub(crate) content: String,
            pub(crate) created_at: DateTime<Utc>,
        }
    }
}

/// The `ai` migration set.
///
/// The migration keeps the name it always had, so a database migrated by
/// an earlier version finds nothing pending. UUIDs are `BINARY(16)` on
/// MySQL and MariaDB (where navree-db writes them as 16 bytes), names are
/// `VARCHAR` without a length on PostgreSQL and SQLite and `VARCHAR(255)`
/// on MySQL.
pub(crate) fn migrations() -> MigrationSet {
    let set = MigrationSet::new(AI_MIGRATIONS).migration(SchemaMigration::new(
        "m20261026_000001_create_ai_conversations",
        CreateConversationTables,
    ));
    // The assistant's items, runs and button posts, with the `ui` feature.
    #[cfg(feature = "ui")]
    let set = set.migration(crate::ui::store::migration());
    set
}

/// A UUID column: `BINARY(16)` on MySQL and MariaDB, `uuid` elsewhere.
fn uuid<'a>(t: &'a mut Blueprint, name: &str, dialect: Dialect) -> ColumnBuilder<'a> {
    if dialect == Dialect::MySql {
        t.binary(name).length(16).fixed()
    } else {
        t.uuid(name)
    }
}

/// A text column of no fixed length: `VARCHAR(255)` on MySQL and MariaDB,
/// `VARCHAR` elsewhere.
fn varchar<'a>(t: &'a mut Blueprint, name: &str, dialect: Dialect) -> ColumnBuilder<'a> {
    if dialect == Dialect::MySql {
        t.string(name).length(255)
    } else {
        t.raw_column(name, "VARCHAR")
    }
}

/// Creates `ai_conversations` and `ai_messages`.
struct CreateConversationTables;

impl Migration for CreateConversationTables {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let dialect = schema.target().await?.dialect();
        schema
            .create("ai_conversations", |t| {
                uuid(t, "id", dialect).primary();
                uuid(t, "user_id", dialect);
                varchar(t, "agent", dialect);
                varchar(t, "title", dialect).nullable();
                t.text("summary").nullable();
                t.integer("summarized_count").default(0);
                t.text("pending").nullable();
                t.timestamp_tz("created_at");
                t.timestamp_tz("updated_at");
                t.index(["user_id", "updated_at"])
                    .name("ai_conversations_user_id_index");
            })
            .await?;
        schema
            .create("ai_messages", |t| {
                uuid(t, "id", dialect).primary();
                uuid(t, "conversation_id", dialect);
                varchar(t, "role", dialect);
                t.text("content");
                t.timestamp_tz("created_at");
                t.foreign("conversation_id")
                    .references("id")
                    .on("ai_conversations")
                    .name("ai_messages_conversation_id_fk")
                    .cascade_on_delete();
                t.index(["conversation_id"])
                    .name("ai_messages_conversation_id_index");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists("ai_messages").await?;
        schema.drop_if_exists("ai_conversations").await
    }
}

/// A conversation in a list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ConversationSummary {
    /// The id.
    pub id: Uuid,
    /// The agent's `NAME`.
    pub agent: String,
    /// The title: the opening words of the first message, or what
    /// [`Conversations::rename`] set.
    pub title: Option<String>,
    /// When it started.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

/// A stored message of a conversation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoredMessage {
    /// The id (UUID v7, in order).
    pub id: Uuid,
    /// Who wrote it (tool results are `user` messages).
    pub role: Role,
    /// Its text parts, joined (empty for tool calls and results).
    pub text: String,
    /// Its parts: text, tool calls and tool results.
    pub parts: Vec<Part>,
    /// When it was written.
    pub created_at: DateTime<Utc>,
}

/// A tool call waiting for the user's decision, with the results of the
/// other calls of its step.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Pending {
    pub(crate) approval: PendingApproval,
    /// The call ids of the step, in order.
    pub(crate) order: Vec<String>,
    /// Results of the step's other calls.
    pub(crate) results: Vec<ToolResult>,
}

/// A loaded conversation: its row and messages.
pub(crate) struct Loaded {
    pub(crate) row: conversation::Model,
    pub(crate) messages: Vec<Message>,
}

/// Stored conversations, per user. Inject `Arc<Conversations>`. Every
/// method takes the user's id and fails with [`AiError::NotFound`] (a 404)
/// for conversations that aren't theirs.
#[derive(Clone)]
pub struct Conversations {
    db: Option<Arc<Db>>,
}

impl std::fmt::Debug for Conversations {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Conversations").finish_non_exhaustive()
    }
}

fn db_error(error: DbError) -> AiError {
    AiError::internal(format!("conversation memory: {error}"))
}

fn not_found() -> AiError {
    AiError::not_found("no such conversation")
}

/// The first words of `text`, at most 60 characters.
pub(crate) fn title_of(text: &str) -> String {
    let mut title = String::new();
    for word in text.split_whitespace() {
        let next = if title.is_empty() {
            word.to_owned()
        } else {
            format!("{title} {word}")
        };
        if next.chars().count() > 60 {
            if title.is_empty() {
                title = word.chars().take(57).collect::<String>() + "...";
            } else {
                title.push_str("...");
            }
            return title;
        }
        title = next;
    }
    title
}

impl Conversations {
    /// The service over `db` (`None`: every call fails with
    /// [`AiError::Config`]).
    pub fn new(db: Option<Arc<Db>>) -> Self {
        Self { db }
    }

    fn db(&self) -> Result<&Db, AiError> {
        self.db
            .as_deref()
            .ok_or_else(|| AiError::config("conversation memory needs DatabaseModule in the app"))
    }

    /// The user's conversations, most recently changed first.
    pub async fn list_for(
        &self,
        user_id: Uuid,
        page: PageQuery,
    ) -> Result<Page<ConversationSummary>, AiError> {
        let page = conversation::Model::select()
            .eq("user_id", user_id)
            .order_by_desc("updated_at")
            .order_by_desc("id")
            .page(self.db()?, page)
            .await
            .map_err(db_error)?;
        Ok(page.map(|row| ConversationSummary {
            id: row.id,
            agent: row.agent,
            title: row.title,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }))
    }

    /// The messages of the user's conversation, oldest first.
    pub async fn messages(
        &self,
        user_id: Uuid,
        conversation_id: Uuid,
    ) -> Result<Vec<StoredMessage>, AiError> {
        self.owned(user_id, conversation_id).await?;
        let rows = message::Model::select()
            .eq("conversation_id", conversation_id)
            .order_by("id")
            .all(self.db()?)
            .await
            .map_err(db_error)?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let parts: Vec<Part> = serde_json::from_str(&row.content).unwrap_or_default();
                let role = role_of(&row.role);
                let text = Message::new(role, parts.clone()).text();
                StoredMessage {
                    id: row.id,
                    role,
                    text,
                    parts,
                    created_at: row.created_at,
                }
            })
            .collect())
    }

    /// Renames the user's conversation.
    pub async fn rename(
        &self,
        user_id: Uuid,
        conversation_id: Uuid,
        title: impl Into<String>,
    ) -> Result<(), AiError> {
        let row = self.owned(user_id, conversation_id).await?;
        let changed = conversation::Model::update_where(Filter::all().eq("id", row.id))
            .set("title", Some(title.into()))
            .set("updated_at", Utc::now())
            .execute(self.db()?)
            .await
            .map_err(db_error)?;
        if changed == 0 {
            return Err(not_found());
        }
        Ok(())
    }

    /// Deletes the user's conversation and its messages.
    pub async fn delete(&self, user_id: Uuid, conversation_id: Uuid) -> Result<(), AiError> {
        self.owned(user_id, conversation_id).await?;
        self.remove(&[conversation_id]).await
    }

    /// Deletes conversations untouched for `days` days (what `ai:prune`
    /// runs); returns how many.
    pub async fn prune(&self, days: u64, now: DateTime<Utc>) -> Result<u64, AiError> {
        let cutoff = now - ChronoDuration::days(days.min(365_000) as i64);
        let ids: Vec<Uuid> = conversation::Model::select()
            .filter(Filter::all().compare("updated_at", Compare::Lt, cutoff))
            .all(self.db()?)
            .await
            .map_err(db_error)?
            .into_iter()
            .map(|row| row.id)
            .collect();
        for chunk in ids.chunks(500) {
            self.remove(chunk).await?;
        }
        Ok(ids.len() as u64)
    }

    async fn remove(&self, ids: &[Uuid]) -> Result<(), AiError> {
        let db = self.db()?;
        message::Model::delete_where(Filter::all().is_in("conversation_id", ids.iter().copied()))
            .execute(db)
            .await
            .map_err(db_error)?;
        conversation::Model::delete_where(Filter::all().is_in("id", ids.iter().copied()))
            .execute(db)
            .await
            .map_err(db_error)?;
        Ok(())
    }

    /// The conversation row, if it is `user_id`'s.
    async fn owned(
        &self,
        user_id: Uuid,
        conversation_id: Uuid,
    ) -> Result<conversation::Model, AiError> {
        record::find::<conversation::Model, _>(self.db()?, conversation_id)
            .await
            .map_err(db_error)?
            .filter(|row| row.user_id == user_id)
            .ok_or_else(not_found)
    }

    /// Whether conversation `id` is `user_id`'s, with agent `agent`: a 404 otherwise.
    pub(crate) async fn check_owner(
        &self,
        user_id: Uuid,
        agent: &str,
        id: Uuid,
    ) -> Result<(), AiError> {
        let row = self.owned(user_id, id).await?;
        if row.agent != agent {
            return Err(not_found());
        }
        Ok(())
    }

    /// Loads `user_id`'s conversation `id` with agent `agent`.
    pub(crate) async fn load(
        &self,
        user_id: Uuid,
        agent: &str,
        id: Uuid,
    ) -> Result<Loaded, AiError> {
        let row = self.owned(user_id, id).await?;
        if row.agent != agent {
            return Err(not_found());
        }
        let messages = self
            .messages(user_id, id)
            .await?
            .into_iter()
            .map(|stored| Message::new(stored.role, stored.parts))
            .collect();
        Ok(Loaded { row, messages })
    }

    /// Creates a conversation titled after `first_message`.
    pub(crate) async fn create(
        &self,
        user_id: Uuid,
        agent: &str,
        first_message: &str,
    ) -> Result<conversation::Model, AiError> {
        let now = Utc::now();
        let row = conversation::Model {
            id: Uuid::now_v7(),
            user_id,
            agent: agent.to_owned(),
            title: Some(title_of(first_message)).filter(|t| !t.is_empty()),
            summary: None,
            summarized_count: 0,
            pending: None,
            created_at: now,
            updated_at: now,
        };
        record::insert_returning(self.db()?, &row)
            .await
            .map_err(db_error)
    }

    /// Appends `messages` to conversation `id`.
    pub(crate) async fn append(&self, id: Uuid, messages: &[Message]) -> Result<(), AiError> {
        let db = self.db()?;
        for message in messages {
            let row = message::Model {
                id: Uuid::now_v7(),
                conversation_id: id,
                role: match message.role {
                    Role::User => "user".to_owned(),
                    Role::Assistant => "assistant".to_owned(),
                },
                content: serde_json::to_string(&stored_parts(message))
                    .unwrap_or_else(|_| "[]".to_owned()),
                created_at: Utc::now(),
            };
            record::insert(db, &row).await.map_err(db_error)?;
        }
        Ok(())
    }

    /// Saves the pending call (or clears it), the summary and the time.
    pub(crate) async fn save_state(
        &self,
        row: &conversation::Model,
        pending: Option<&Pending>,
        summary: Option<(String, i32)>,
    ) -> Result<(), AiError> {
        let mut changes = conversation::Model::update_where(Filter::all().eq("id", row.id)).set(
            "pending",
            pending.map(|p| serde_json::to_string(p).unwrap_or_default()),
        );
        if let Some((summary, count)) = summary {
            changes = changes
                .set("summary", Some(summary))
                .set("summarized_count", count);
        }
        changes
            .set("updated_at", Utc::now())
            .execute(self.db()?)
            .await
            .map_err(db_error)?;
        Ok(())
    }
}

/// The parts of `message` that are stored: everything but the provider's
/// reasoning blocks. A provider binds those to the exact request that
/// produced them (system prompt, tools, earlier messages), which a later
/// run changes (a new summary, a new date, other tools), so a later run
/// starts without them, as if they had been removed from the front.
fn stored_parts(message: &Message) -> Vec<Part> {
    message
        .parts
        .iter()
        .filter(|part| !matches!(part, Part::Reasoning { .. }))
        .cloned()
        .collect()
}

/// The pending call of a conversation row.
pub(crate) fn pending_of(row: &conversation::Model) -> Option<Pending> {
    row.pending
        .as_deref()
        .and_then(|json| serde_json::from_str(json).ok())
}

fn role_of(role: &str) -> Role {
    if role == "assistant" {
        Role::Assistant
    } else {
        Role::User
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasoning_is_not_stored() {
        let message = Message::new(
            Role::Assistant,
            vec![
                Part::Reasoning {
                    provider: "anthropic".into(),
                    data: serde_json::json!({ "type": "thinking", "thinking": "", "signature": "sig" }),
                },
                Part::Text {
                    text: "Looking.".into(),
                },
            ],
        );
        assert_eq!(
            stored_parts(&message),
            [Part::Text {
                text: "Looking.".into()
            }]
        );
    }

    #[test]
    fn titles_are_the_opening_words() {
        assert_eq!(title_of("Which deals are stuck?"), "Which deals are stuck?");
        let long =
            "Which of my deals in negotiation have had no activity for two weeks and need a call?";
        let title = title_of(long);
        assert!(title.chars().count() <= 63, "{title}");
        assert!(title.ends_with("..."));
        assert!(long.starts_with(title.trim_end_matches("...")));
        assert_eq!(title_of(""), "");
    }
}
