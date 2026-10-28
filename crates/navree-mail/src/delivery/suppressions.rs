//! [`Suppressions`]: addresses mail is not sent to.

use std::collections::HashSet;
use std::fmt;
use std::str::FromStr;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use navree_db::record::{self, Filter};
use navree_db::{Page, PageQuery, Record as _, Select, Uow};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::Store;
use super::entity::SuppressionRow;
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

impl From<&SuppressionRow> for Suppression {
    fn from(row: &SuppressionRow) -> Self {
        Self {
            id: row.id,
            workspace_id: row.workspace_id,
            email: row.email.clone(),
            reason: row.reason.parse().unwrap_or(SuppressionReason::Manual),
            detail: row.detail.clone(),
            delivery_id: row.delivery_id,
            created_at: row.created_at,
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

/// The rows of `email` (bare, lower case) in `workspace_id` (`None`: the
/// app-wide list).
fn address(email: &str, workspace_id: Option<Uuid>) -> Select<SuppressionRow> {
    SuppressionRow::select()
        .eq("email", email)
        .eq("workspace_id", workspace_id)
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
        if let Some(existing) = address(&email, workspace_id).one(uow).await? {
            return Ok(Suppression::from(&existing));
        }
        let row = SuppressionRow {
            id: Uuid::now_v7(),
            workspace_id,
            email,
            reason: reason.as_str().to_owned(),
            detail: detail.map(|d| d.chars().take(2000).collect()),
            delivery_id,
            created_at: self.store.now().await,
        };
        let row = record::insert_recorded(uow, &row).await?;
        Ok(Suppression::from(&row))
    }

    /// Removes the suppression `id`.
    pub async fn remove(&self, uow: &Uow, id: Uuid) -> Result<Suppression, MailError> {
        let row = record::find::<SuppressionRow, _>(uow, id)
            .await?
            .ok_or(MailError::NotFound("suppression"))?;
        record::delete_recorded(uow, &row).await?;
        Ok(Suppression::from(&row))
    }

    /// Removes `email`'s suppression in `workspace_id` (`None`: the
    /// app-wide one); returns whether there was one.
    pub async fn remove_email(
        &self,
        uow: &Uow,
        workspace_id: Option<Uuid>,
        email: &str,
    ) -> Result<bool, MailError> {
        match address(&bare_address(email), workspace_id).one(uow).await? {
            Some(row) => {
                record::delete_recorded(uow, &row).await?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// One suppression.
    pub async fn find(&self, id: Uuid) -> Result<Suppression, MailError> {
        let db = self.store.db().await?;
        record::find::<SuppressionRow, _>(&db.read_executor().await, id)
            .await?
            .map(|row| Suppression::from(&row))
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
        let mut select = SuppressionRow::select();
        if let Some(workspace) = workspace_id {
            select = select.eq("workspace_id", workspace);
        }
        if let Some(email) = email {
            select = select.eq("email", bare_address(email));
        }
        let page = select
            .order_by_desc("created_at")
            .order_by_desc("id")
            .page(&db.read_executor().await, page)
            .await?;
        Ok(page.map(|row| Suppression::from(&row)))
    }

    /// The bare, lower-case addresses among `recipients` suppressed for
    /// `workspace_id`. Reads in the task's unit of work when its
    /// transaction is open (a request's), so a suppression it added counts.
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
        let mut scope = Filter::any().is_null("workspace_id");
        if let Some(workspace) = workspace_id {
            scope = scope.eq("workspace_id", workspace);
        }
        let rows = SuppressionRow::select()
            .filter(Filter::all().is_in("email", emails))
            .filter(scope)
            .all(&db.read_executor().await)
            .await?;
        Ok(rows.into_iter().map(|row| row.email).collect())
    }
}
