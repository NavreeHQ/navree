//! Agent conversation memory: the `ai_conversations` and `ai_messages`
//! tables (migration set `ai`) and the [`Conversations`] service.

use std::sync::Arc;

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use navree_db::{Db, MigrationSet, Page, PageQuery, paginate};
use sea_orm::sea_query::{ColumnDef, ForeignKey, ForeignKeyAction, Index, Table};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DbErr, EntityTrait, IntoActiveModel, QueryFilter, QueryOrder,
    Set,
};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::agent::PendingApproval;
use crate::error::AiError;
use crate::message::{Message, Part, Role, ToolResult};

/// The name of the AI module's migration set.
pub const AI_MIGRATIONS: &str = "ai";

/// `ai_conversations`: one row per conversation.
pub(crate) mod conversation {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "ai_conversations")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        pub user_id: Uuid,
        pub agent: String,
        pub title: Option<String>,
        pub summary: Option<String>,
        pub summarized_count: i32,
        /// A [`Pending`](super::Pending) as JSON.
        pub pending: Option<String>,
        pub created_at: DateTimeUtc,
        pub updated_at: DateTimeUtc,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `ai_messages`: the messages of a conversation, in order.
pub(crate) mod message {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "ai_messages")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        pub conversation_id: Uuid,
        pub role: String,
        /// The [`Part`](crate::Part)s as JSON.
        pub content: String,
        pub created_at: DateTimeUtc,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// The `ai` migration set.
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new(AI_MIGRATIONS).migration(CreateConversationTables)
}

/// Creates `ai_conversations` and `ai_messages`.
struct CreateConversationTables;

impl MigrationName for CreateConversationTables {
    fn name(&self) -> &str {
        "m20261026_000001_create_ai_conversations"
    }
}

fn timestamp(name: &'static str) -> ColumnDef {
    ColumnDef::new(name)
        .timestamp_with_time_zone()
        .not_null()
        .to_owned()
}

#[async_trait]
impl MigrationTrait for CreateConversationTables {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table("ai_conversations")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("user_id").uuid().not_null())
                    .col(ColumnDef::new("agent").string().not_null())
                    .col(ColumnDef::new("title").string().null())
                    .col(ColumnDef::new("summary").text().null())
                    .col(
                        ColumnDef::new("summarized_count")
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(ColumnDef::new("pending").text().null())
                    .col(timestamp("created_at"))
                    .col(timestamp("updated_at"))
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("ai_conversations_user_id_index")
                    .table("ai_conversations")
                    .col("user_id")
                    .col("updated_at")
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table("ai_messages")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("conversation_id").uuid().not_null())
                    .col(ColumnDef::new("role").string().not_null())
                    .col(ColumnDef::new("content").text().not_null())
                    .col(timestamp("created_at"))
                    .foreign_key(
                        ForeignKey::create()
                            .name("ai_messages_conversation_id_fk")
                            .from("ai_messages", "conversation_id")
                            .to("ai_conversations", "id")
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("ai_messages_conversation_id_index")
                    .table("ai_messages")
                    .col("conversation_id")
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table("ai_messages").to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table("ai_conversations").to_owned())
            .await
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

fn db_error(error: DbErr) -> AiError {
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
        let select = conversation::Entity::find()
            .filter(conversation::Column::UserId.eq(user_id))
            .order_by_desc(conversation::Column::UpdatedAt)
            .order_by_desc(conversation::Column::Id);
        let page = paginate(select, self.db()?.conn(), page)
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
        let rows = message::Entity::find()
            .filter(message::Column::ConversationId.eq(conversation_id))
            .order_by_asc(message::Column::Id)
            .all(self.db()?.conn())
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
        let mut active = row.into_active_model();
        active.title = Set(Some(title.into()));
        active.updated_at = Set(Utc::now());
        active.update(self.db()?.conn()).await.map_err(db_error)?;
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
        let ids: Vec<Uuid> = conversation::Entity::find()
            .filter(conversation::Column::UpdatedAt.lt(cutoff))
            .all(self.db()?.conn())
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
        let conn = self.db()?.conn();
        message::Entity::delete_many()
            .filter(message::Column::ConversationId.is_in(ids.iter().copied()))
            .exec(conn)
            .await
            .map_err(db_error)?;
        conversation::Entity::delete_many()
            .filter(conversation::Column::Id.is_in(ids.iter().copied()))
            .exec(conn)
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
        conversation::Entity::find_by_id(conversation_id)
            .one(self.db()?.conn())
            .await
            .map_err(db_error)?
            .filter(|row| row.user_id == user_id)
            .ok_or_else(not_found)
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
        let row = conversation::ActiveModel {
            id: Set(Uuid::now_v7()),
            user_id: Set(user_id),
            agent: Set(agent.to_owned()),
            title: Set(Some(title_of(first_message)).filter(|t| !t.is_empty())),
            summary: Set(None),
            summarized_count: Set(0),
            pending: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        };
        row.insert(self.db()?.conn()).await.map_err(db_error)
    }

    /// Appends `messages` to conversation `id`.
    pub(crate) async fn append(&self, id: Uuid, messages: &[Message]) -> Result<(), AiError> {
        let conn = self.db()?.conn();
        for message in messages {
            let row = message::ActiveModel {
                id: Set(Uuid::now_v7()),
                conversation_id: Set(id),
                role: Set(match message.role {
                    Role::User => "user".to_owned(),
                    Role::Assistant => "assistant".to_owned(),
                }),
                content: Set(serde_json::to_string(&stored_parts(message))
                    .unwrap_or_else(|_| "[]".to_owned())),
                created_at: Set(Utc::now()),
            };
            row.insert(conn).await.map_err(db_error)?;
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
        let mut active = row.clone().into_active_model();
        active.pending = Set(pending.map(|p| serde_json::to_string(p).unwrap_or_default()));
        if let Some((summary, count)) = summary {
            active.summary = Set(Some(summary));
            active.summarized_count = Set(count);
        }
        active.updated_at = Set(Utc::now());
        active.update(self.db()?.conn()).await.map_err(db_error)?;
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
