//! [`Outbox`]: mail held for approval.

use std::fmt;
use std::str::FromStr;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use navree_db::{Page, PageQuery, paginate};
use sea_orm::ActiveValue::{Set, Unchanged};
use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveModelTrait as _, ColumnTrait as _, EntityTrait as _, QueryFilter as _, QueryOrder as _,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;
use uuid::Uuid;

use super::Store;
use super::entity::outbox;
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

impl From<&outbox::Model> for HeldMail {
    fn from(model: &outbox::Model) -> Self {
        Self {
            id: model.id,
            workspace_id: model.workspace_id,
            organization_id: model.organization_id,
            reason: model.reason.clone(),
            status: model.status.parse().unwrap_or(OutboxStatus::Held),
            subject: model.subject.clone(),
            recipients: serde_json::from_value(model.recipients.clone()).unwrap_or_default(),
            via: model.via.clone(),
            template: model.template.clone(),
            delivery_id: model.delivery_id,
            decided_by: model.decided_by,
            decided_at: model.decided_at,
            created_at: model.created_at,
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
#[derive(Clone)]
pub struct Outbox {
    store: Arc<Store>,
}

impl fmt::Debug for Outbox {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Outbox")
    }
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
        let model = outbox::ActiveModel {
            id: Set(Uuid::now_v7()),
            workspace_id: Set(scope.workspace_id),
            organization_id: Set(scope.organization_id),
            reason: Set(reason),
            status: Set(OutboxStatus::Held.as_str().to_owned()),
            subject: Set(message.subject.clone()),
            recipients: Set(json!(recipients)),
            message: Set(sealed),
            via: Set(options.via.clone()),
            template: Set(options.template.clone()),
            delivery_id: Set(None),
            decided_by: Set(None),
            decided_at: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        };
        let model = self
            .store
            .in_uow(|uow| async move {
                let tx = uow.tx().await?;
                Ok(model.insert(&*tx).await?)
            })
            .await?;
        tracing::info!(target: "navree::mail", outbox_id = %model.id, subject = %model.subject, "mail held for approval");
        Ok(HeldMail::from(&model))
    }

    /// One held message.
    pub async fn find(&self, id: Uuid) -> Result<HeldMail, MailError> {
        Ok(HeldMail::from(&self.model(id).await?))
    }

    /// The message itself (decrypted), to preview it.
    pub async fn message(&self, id: Uuid) -> Result<Message, MailError> {
        let model = self.model(id).await?;
        let json = self.store.open(&model.message).await?;
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
        let mut select = outbox::Entity::find();
        if let Some(status) = status {
            select = select.filter(outbox::Column::Status.eq(status.as_str()));
        }
        if let Some(workspace) = workspace_id {
            select = select.filter(outbox::Column::WorkspaceId.eq(workspace));
        }
        let select = select
            .order_by_desc(outbox::Column::CreatedAt)
            .order_by_desc(outbox::Column::Id);
        let page = paginate(select, &db.reader().await, page).await?;
        Ok(page.map(|model| HeldMail::from(&model)))
    }

    /// Sends a held message now, through its scope and named transport, and
    /// marks it `released` by `by`. Anything but a held message is a
    /// [`MailError::Conflict`] (two admins cannot both release it); if
    /// sending fails, it stays held.
    pub async fn release(&self, id: Uuid, by: Option<Uuid>) -> Result<SendReport, MailError> {
        let model = self.model(id).await?;
        let message = self.message(id).await?;
        let mailer = self.store.app()?.get::<Mailer>().await.map_err(|error| {
            MailError::Store {
                source: Box::new(error),
            }
        })?;
        let store = self.store.clone();
        self.store
            .in_uow(|uow| async move {
                let now = store.now().await;
                let tx = uow.tx().await?;
                let claimed = outbox::Entity::update_many()
                    .col_expr(outbox::Column::Status, Expr::value(OutboxStatus::Released.as_str()))
                    .col_expr(outbox::Column::DecidedBy, Expr::value(by))
                    .col_expr(outbox::Column::DecidedAt, Expr::value(now))
                    .col_expr(outbox::Column::UpdatedAt, Expr::value(now))
                    .filter(outbox::Column::Id.eq(id))
                    .filter(outbox::Column::Status.eq(OutboxStatus::Held.as_str()))
                    .exec(&*tx)
                    .await?;
                drop(tx);
                if claimed.rows_affected == 0 {
                    return Err(MailError::Conflict(
                        "only a held message can be released".to_owned(),
                    ));
                }
                let options = SendOptions {
                    scope: Some(MailScope {
                        workspace_id: model.workspace_id,
                        organization_id: model.organization_id,
                    }),
                    via: model.via.clone(),
                    template: model.template.clone(),
                    releasing: true,
                    rendered: true,
                };
                let report = mailer.deliver_with(message, options).await?;
                if let Some(delivery_id) = report.delivery_id {
                    let tx = uow.tx().await?;
                    outbox::ActiveModel {
                        id: Unchanged(id),
                        delivery_id: Set(Some(delivery_id)),
                        ..Default::default()
                    }
                    .update(&*tx)
                    .await?;
                }
                Ok(report)
            })
            .await
    }

    /// Discards a held message (it is never sent), by `by`. Anything but a
    /// held message is a [`MailError::Conflict`].
    pub async fn discard(&self, id: Uuid, by: Option<Uuid>) -> Result<HeldMail, MailError> {
        self.model(id).await?;
        let store = self.store.clone();
        self.store
            .in_uow(|uow| async move {
                let now = store.now().await;
                let tx = uow.tx().await?;
                let claimed = outbox::Entity::update_many()
                    .col_expr(outbox::Column::Status, Expr::value(OutboxStatus::Discarded.as_str()))
                    .col_expr(outbox::Column::DecidedBy, Expr::value(by))
                    .col_expr(outbox::Column::DecidedAt, Expr::value(now))
                    .col_expr(outbox::Column::UpdatedAt, Expr::value(now))
                    .filter(outbox::Column::Id.eq(id))
                    .filter(outbox::Column::Status.eq(OutboxStatus::Held.as_str()))
                    .exec(&*tx)
                    .await?;
                if claimed.rows_affected == 0 {
                    return Err(MailError::Conflict(
                        "only a held message can be discarded".to_owned(),
                    ));
                }
                let model = outbox::Entity::find_by_id(id)
                    .one(&*tx)
                    .await?
                    .ok_or(MailError::NotFound("held message"))?;
                Ok(HeldMail::from(&model))
            })
            .await
    }

    async fn model(&self, id: Uuid) -> Result<outbox::Model, MailError> {
        let db = self.store.db().await?;
        outbox::Entity::find_by_id(id)
            .one(&db.reader().await)
            .await?
            .ok_or(MailError::NotFound("held message"))
    }
}
