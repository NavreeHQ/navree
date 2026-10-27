//! [`Suppressions`]: addresses mail is not sent to.

use std::collections::HashSet;
use std::fmt;
use std::str::FromStr;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use navree_db::{Page, PageQuery, Repo, Uow, paginate};
use sea_orm::ActiveValue::Set;
use sea_orm::{ColumnTrait as _, Condition, EntityTrait as _, QueryFilter as _, QueryOrder as _};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::Store;
use super::entity::suppression;
use crate::MailError;
use crate::message::{bare_address, mailbox};

/// Why an address is suppressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SuppressionReason {
    /// A hard bounce: the address does not exist.
    HardBounce,
    /// The recipient marked mail as spam.
    Complaint,
    /// The recipient unsubscribed.
    Unsubscribed,
    /// An admin added it.
    Manual,
}

impl SuppressionReason {
    /// The stored name.
    pub fn as_str(self) -> &'static str {
        match self {
            SuppressionReason::HardBounce => "hard_bounce",
            SuppressionReason::Complaint => "complaint",
            SuppressionReason::Unsubscribed => "unsubscribed",
            SuppressionReason::Manual => "manual",
        }
    }
}

impl fmt::Display for SuppressionReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for SuppressionReason {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "hard_bounce" => SuppressionReason::HardBounce,
            "complaint" => SuppressionReason::Complaint,
            "unsubscribed" => SuppressionReason::Unsubscribed,
            "manual" => SuppressionReason::Manual,
            other => {
                return Err(format!(
                    "unknown suppression reason `{other}` (hard_bounce, complaint, unsubscribed or manual)"
                ));
            }
        })
    }
}

/// A suppressed address (`mail_suppressions`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Suppression {
    /// The id.
    pub id: Uuid,
    /// The workspace it applies to; `None` for every workspace.
    pub workspace_id: Option<Uuid>,
    /// The address, lower case.
    pub email: String,
    /// Why.
    pub reason: SuppressionReason,
    /// The provider's diagnostic or an admin's note.
    pub detail: Option<String>,
    /// The delivery that caused it.
    pub delivery_id: Option<Uuid>,
    /// When it was added.
    pub created_at: DateTime<Utc>,
}

impl From<&suppression::Model> for Suppression {
    fn from(model: &suppression::Model) -> Self {
        Self {
            id: model.id,
            workspace_id: model.workspace_id,
            email: model.email.clone(),
            reason: model.reason.parse().unwrap_or(SuppressionReason::Manual),
            detail: model.detail.clone(),
            delivery_id: model.delivery_id,
            created_at: model.created_at,
        }
    }
}

/// Addresses mail is not sent to, per workspace or everywhere.
///
/// With `mail.delivery.enabled`, the [`Mailer`](crate::Mailer) leaves
/// suppressed recipients out of every message (a message left with none is
/// logged as `suppressed` and not sent), and hard bounces and complaints
/// add their address automatically (`mail.delivery.auto_suppress`) in the
/// delivery's workspace. Provided and exported by
/// [`MailModule`](crate::MailModule); needs `DatabaseModule`.
#[derive(Clone)]
pub struct Suppressions {
    store: Arc<Store>,
}

impl fmt::Debug for Suppressions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Suppressions")
    }
}

impl Suppressions {
    pub(crate) fn new(store: Arc<Store>) -> Self {
        Self { store }
    }

    /// Suppresses `email` in `workspace_id` (`None`: everywhere). Adding an
    /// address that is already suppressed there returns the existing row.
    pub async fn add(
        &self,
        uow: &Uow,
        workspace_id: Option<Uuid>,
        email: &str,
        reason: SuppressionReason,
        detail: Option<String>,
    ) -> Result<Suppression, MailError> {
        mailbox("email", email)?;
        self.add_in(uow, workspace_id, email, reason, detail, None)
            .await
    }

    /// [`add`](Suppressions::add) with the delivery that caused it.
    pub(crate) async fn add_in(
        &self,
        uow: &Uow,
        workspace_id: Option<Uuid>,
        email: &str,
        reason: SuppressionReason,
        detail: Option<String>,
        delivery_id: Option<Uuid>,
    ) -> Result<Suppression, MailError> {
        let email = bare_address(email);
        let tx = uow.tx().await?;
        let existing = suppression::Entity::find()
            .filter(suppression::Column::Email.eq(email.as_str()))
            .filter(match workspace_id {
                Some(id) => suppression::Column::WorkspaceId.eq(id),
                None => suppression::Column::WorkspaceId.is_null(),
            })
            .one(&*tx)
            .await?;
        drop(tx);
        if let Some(existing) = existing {
            return Ok(Suppression::from(&existing));
        }
        let now = self.store.now().await;
        let model = Repo::<suppression::Entity>::insert(
            uow,
            suppression::ActiveModel {
                id: Set(Uuid::now_v7()),
                workspace_id: Set(workspace_id),
                email: Set(email),
                reason: Set(reason.as_str().to_owned()),
                detail: Set(detail.map(|d| d.chars().take(2000).collect())),
                delivery_id: Set(delivery_id),
                created_at: Set(now),
            },
        )
        .await?;
        Ok(Suppression::from(&model))
    }

    /// Removes the suppression `id`.
    pub async fn remove(&self, uow: &Uow, id: Uuid) -> Result<Suppression, MailError> {
        let model = Repo::<suppression::Entity>::find(uow, id)
            .await?
            .ok_or(MailError::NotFound("suppression"))?;
        let removed = Suppression::from(&model);
        Repo::<suppression::Entity>::delete(uow, model).await?;
        Ok(removed)
    }

    /// Removes `email`'s suppression in `workspace_id` (`None`: the
    /// app-wide one); returns whether there was one.
    pub async fn remove_email(
        &self,
        uow: &Uow,
        workspace_id: Option<Uuid>,
        email: &str,
    ) -> Result<bool, MailError> {
        let tx = uow.tx().await?;
        let existing = suppression::Entity::find()
            .filter(suppression::Column::Email.eq(bare_address(email)))
            .filter(match workspace_id {
                Some(id) => suppression::Column::WorkspaceId.eq(id),
                None => suppression::Column::WorkspaceId.is_null(),
            })
            .one(&*tx)
            .await?;
        drop(tx);
        match existing {
            Some(model) => {
                Repo::<suppression::Entity>::delete(uow, model).await?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// One suppression.
    pub async fn find(&self, id: Uuid) -> Result<Suppression, MailError> {
        let db = self.store.db().await?;
        suppression::Entity::find_by_id(id)
            .one(&db.reader().await)
            .await?
            .map(|model| Suppression::from(&model))
            .ok_or(MailError::NotFound("suppression"))
    }

    /// Whether mail to `email` sent for `workspace_id` is suppressed (by
    /// that workspace's list or the app-wide one).
    pub async fn is_suppressed(
        &self,
        workspace_id: Option<Uuid>,
        email: &str,
    ) -> Result<bool, MailError> {
        Ok(!self
            .suppressed_among(workspace_id, std::iter::once(email))
            .await?
            .is_empty())
    }

    /// Suppressions, newest first: `workspace_id`'s (and only those) when
    /// given, every one otherwise; `email` narrows to one address.
    pub async fn list(
        &self,
        workspace_id: Option<Uuid>,
        email: Option<&str>,
        page: PageQuery,
    ) -> Result<Page<Suppression>, MailError> {
        let db = self.store.db().await?;
        let mut select = suppression::Entity::find();
        if let Some(workspace) = workspace_id {
            select = select.filter(suppression::Column::WorkspaceId.eq(workspace));
        }
        if let Some(email) = email {
            select = select.filter(suppression::Column::Email.eq(bare_address(email)));
        }
        let select = select
            .order_by_desc(suppression::Column::CreatedAt)
            .order_by_desc(suppression::Column::Id);
        let page = paginate(select, &db.reader().await, page).await?;
        Ok(page.map(|model| Suppression::from(&model)))
    }

    /// The bare, lower-case addresses among `recipients` suppressed for
    /// `workspace_id`.
    pub(crate) async fn suppressed_among(
        &self,
        workspace_id: Option<Uuid>,
        recipients: impl IntoIterator<Item = &str>,
    ) -> Result<HashSet<String>, MailError> {
        let emails: Vec<String> = recipients.into_iter().map(bare_address).collect();
        if emails.is_empty() {
            return Ok(HashSet::new());
        }
        let db = self.store.db().await?;
        let mut scope = Condition::any().add(suppression::Column::WorkspaceId.is_null());
        if let Some(workspace) = workspace_id {
            scope = scope.add(suppression::Column::WorkspaceId.eq(workspace));
        }
        let select = suppression::Entity::find()
            .filter(suppression::Column::Email.is_in(emails))
            .filter(scope);
        let models = match navree_db::Uow::current() {
            Some(uow) if std::ptr::eq(uow.db().conn(), db.conn()) && !uow.is_finished().await => {
                let tx = uow.tx().await?;
                select.all(&*tx).await?
            }
            _ => select.all(&db.reader().await).await?,
        };
        Ok(models.into_iter().map(|model| model.email).collect())
    }
}
