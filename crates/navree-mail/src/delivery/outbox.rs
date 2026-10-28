//! [`Outbox`]: mail held for approval.

use std::fmt;
use std::str::FromStr;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use navree_db::record::{self, Filter};
use navree_db::{Page, PageQuery, Record as _, Uow};
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;
use uuid::Uuid;

use super::Store;
use super::entity::OutboxRow;
use crate::mailer::SendOptions;
use crate::{MailError, MailScope, Mailer, Message, SendReport};

/// Where a held message stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum OutboxStatus {
    /// Waiting for an admin.
    Held,
    /// Released and sent.
    Released,
    /// Discarded: never sent.
    Discarded,
}

impl OutboxStatus {
    /// The stored name.
    pub fn as_str(self) -> &'static str {
        match self {
            OutboxStatus::Held => "held",
            OutboxStatus::Released => "released",
            OutboxStatus::Discarded => "discarded",
        }
    }
}

impl FromStr for OutboxStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "held" => OutboxStatus::Held,
            "released" => OutboxStatus::Released,
            "discarded" => OutboxStatus::Discarded,
            other => return Err(format!("unknown outbox status `{other}`")),
        })
    }
}

/// A message held for approval (`mail_outbox`). The rendered message is
/// stored encrypted; read it with [`Outbox::message`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct HeldMail {
    /// The id.
    pub id: Uuid,
    /// The workspace it was sent for.
    pub workspace_id: Option<Uuid>,
    /// The organization it was sent for.
    pub organization_id: Option<Uuid>,
    /// Why it is held.
    pub reason: String,
    /// Where it stands.
    pub status: OutboxStatus,
    /// The subject line.
    pub subject: String,
    /// The recipients (`to`, `cc` and `bcc`).
    pub recipients: Vec<String>,
    /// The named transport it goes through.
    pub via: Option<String>,
    /// The template it was rendered from.
    pub template: Option<String>,
    /// The delivery, once released.
    pub delivery_id: Option<Uuid>,
    /// Who released or discarded it.
    pub decided_by: Option<Uuid>,
    /// When.
    pub decided_at: Option<DateTime<Utc>>,
    /// When it was held.
    pub created_at: DateTime<Utc>,
}

impl From<&OutboxRow> for HeldMail {
    fn from(row: &OutboxRow) -> Self {
        Self {
            id: row.id,
            workspace_id: row.workspace_id,
            organization_id: row.organization_id,
            reason: row.reason.clone(),
            status: row.status.parse().unwrap_or(OutboxStatus::Held),
            subject: row.subject.clone(),
            recipients: serde_json::from_value(row.recipients.clone()).unwrap_or_default(),
            via: row.via.clone(),
            template: row.template.clone(),
            delivery_id: row.delivery_id,
            decided_by: row.decided_by,
            decided_at: row.decided_at,
            created_at: row.created_at,
        }
    }
}

/// Mail held for approval: a message built with [`Mailable::hold`](crate::Mailable::hold)
/// (or [`Message::hold`]) waits here, encrypted with the app key, until an
/// admin [`release`](Outbox::release)s it (it is sent then, through the
/// same scope and transport) or [`discard`](Outbox::discard)s it.
///
/// Provided and exported by [`MailModule`](crate::MailModule); needs
/// `DatabaseModule`, `CryptoModule` with an app key, and
/// `mail.delivery.enabled`. `release` and `discard` join the task's current
/// unit of work when there is an unfinished one.
///
/// Releasing and discarding claim the row (`UPDATE .. WHERE status =
/// 'held'`), so of two admins acting on one message at the same time
/// exactly one succeeds and the other gets a [`MailError::Conflict`]; the
/// message is sent at most once.
#[derive(Clone)]
pub struct Outbox {
    store: Arc<Store>,
}

impl fmt::Debug for Outbox {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Outbox")
    }
}

/// Claims the held message `id` as `to`, by `by` at `now`: whether it was
/// still held.
async fn claim(
    uow: &Uow,
    id: Uuid,
    to: OutboxStatus,
    by: Option<Uuid>,
    now: DateTime<Utc>,
) -> Result<bool, MailError> {
    let claimed = OutboxRow::update_where(
        Filter::all()
            .eq("id", id)
            .eq("status", OutboxStatus::Held.as_str()),
    )
    .set("status", to.as_str())
    .set("decided_by", by)
    .set("decided_at", now)
    .set("updated_at", now)
    .execute(uow)
    .await?;
    Ok(claimed > 0)
}

impl Outbox {
    pub(crate) fn new(store: Arc<Store>) -> Self {
        Self { store }
    }

    /// Stores `message` (rendered, validated) as held.
    pub(crate) async fn hold(
        &self,
        message: &Message,
        options: &SendOptions,
    ) -> Result<HeldMail, MailError> {
        let reason = message.hold.clone().unwrap_or_default();
        let sealed = self
            .store
            .seal(&serde_json::to_string(message).map_err(|e| MailError::Config(e.to_string()))?)
            .await?;
        let now = self.store.now().await;
        let scope = options.scope.clone().unwrap_or_default();
        let recipients: Vec<&str> = message.recipients().collect();
        let row = OutboxRow {
            id: Uuid::now_v7(),
            workspace_id: scope.workspace_id,
            organization_id: scope.organization_id,
            reason,
            status: OutboxStatus::Held.as_str().to_owned(),
            subject: message.subject.clone(),
            recipients: json!(recipients),
            message: sealed,
            via: options.via.clone(),
            template: options.template.clone(),
            delivery_id: None,
            decided_by: None,
            decided_at: None,
            created_at: now,
            updated_at: now,
        };
        let row = self
            .store
            .in_uow(|uow| async move {
                Ok(record::insert_returning(&uow, &row).await?)
            })
            .await?;
        tracing::info!(target: "navree::mail", outbox_id = %row.id, subject = %row.subject, "mail held for approval");
        Ok(HeldMail::from(&row))
    }

    /// One held message.
    pub async fn find(&self, id: Uuid) -> Result<HeldMail, MailError> {
        Ok(HeldMail::from(&self.row(id).await?))
    }

    /// The message itself (decrypted), to preview it.
    pub async fn message(&self, id: Uuid) -> Result<Message, MailError> {
        let row = self.row(id).await?;
        let json = self.store.open(&row.message).await?;
        serde_json::from_str(&json).map_err(|error| MailError::Store {
            source: Box::new(error),
        })
    }

    /// Held messages, newest first: those with `status` when given, every
    /// one otherwise; `workspace_id` narrows to one workspace.
    pub async fn list(
        &self,
        status: Option<OutboxStatus>,
        workspace_id: Option<Uuid>,
        page: PageQuery,
    ) -> Result<Page<HeldMail>, MailError> {
        let db = self.store.db().await?;
        let mut select = OutboxRow::select();
        if let Some(status) = status {
            select = select.eq("status", status.as_str());
        }
        if let Some(workspace) = workspace_id {
            select = select.eq("workspace_id", workspace);
        }
        let page = select
            .order_by_desc("created_at")
            .order_by_desc("id")
            .page(&db.read_executor().await, page)
            .await?;
        Ok(page.map(|row| HeldMail::from(&row)))
    }

    /// Sends a held message now, through its scope and named transport, and
    /// marks it `released` by `by`. Anything but a held message is a
    /// [`MailError::Conflict`] (two admins cannot both release it); if
    /// sending fails, it stays held.
    pub async fn release(&self, id: Uuid, by: Option<Uuid>) -> Result<SendReport, MailError> {
        let row = self.row(id).await?;
        let message = self.message(id).await?;
        let mailer = self
            .store
            .app()?
            .get::<Mailer>()
            .await
            .map_err(|error| MailError::Store {
                source: Box::new(error),
            })?;
        let store = self.store.clone();
        self.store
            .in_uow(|uow| async move {
                let now = store.now().await;
                if !claim(&uow, id, OutboxStatus::Released, by, now).await? {
                    return Err(MailError::Conflict(
                        "only a held message can be released".to_owned(),
                    ));
                }
                let options = SendOptions {
                    scope: Some(MailScope {
                        workspace_id: row.workspace_id,
                        organization_id: row.organization_id,
                    }),
                    via: row.via.clone(),
                    template: row.template.clone(),
                    releasing: true,
                    rendered: true,
                };
                let report = mailer.deliver_with(message, options).await?;
                if let Some(delivery_id) = report.delivery_id {
                    OutboxRow::update_where(Filter::all().eq("id", id))
                        .set("delivery_id", delivery_id)
                        .execute(&uow)
                        .await?;
                }
                Ok(report)
            })
            .await
    }

    /// Discards a held message (it is never sent), by `by`. Anything but a
    /// held message is a [`MailError::Conflict`].
    pub async fn discard(&self, id: Uuid, by: Option<Uuid>) -> Result<HeldMail, MailError> {
        self.row(id).await?;
        let store = self.store.clone();
        self.store
            .in_uow(|uow| async move {
                let now = store.now().await;
                if !claim(&uow, id, OutboxStatus::Discarded, by, now).await? {
                    return Err(MailError::Conflict(
                        "only a held message can be discarded".to_owned(),
                    ));
                }
                let row = record::find::<OutboxRow, _>(&uow, id)
                    .await?
                    .ok_or(MailError::NotFound("held message"))?;
                Ok(HeldMail::from(&row))
            })
            .await
    }

    async fn row(&self, id: Uuid) -> Result<OutboxRow, MailError> {
        let db = self.store.db().await?;
        record::find::<OutboxRow, _>(&db.read_executor().await, id)
            .await?
            .ok_or(MailError::NotFound("held message"))
    }
}
