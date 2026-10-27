//! [`Deliveries`]: the delivery log and the events recorded against it.

use std::fmt;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use navree_db::{Page, PageQuery, paginate};
use sea_orm::ActiveValue::{Set, Unchanged};
use sea_orm::{ActiveModelTrait as _, ColumnTrait as _, EntityTrait as _, QueryFilter as _, QueryOrder as _};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::entity::{delivery, event};
use super::events::{MailBounced, MailClicked, MailComplained, MailDelivered, MailOpened};
use super::suppressions::{SuppressionReason, Suppressions};
use super::{Store, recipient_hash};
use crate::{DeliveryStatus, MailError, MailEventKind, ProviderEvent, TransportTier};

/// One message in the delivery log (`mail_deliveries`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Delivery {
    /// The id.
    pub id: Uuid,
    /// The workspace it was sent for.
    pub workspace_id: Option<Uuid>,
    /// The organization it was sent for.
    pub organization_id: Option<Uuid>,
    /// Where it stands.
    pub status: DeliveryStatus,
    /// The transport (`ses`, `smtp`, ...; `none` when suppressed).
    pub transport: String,
    /// Where the transport came from.
    pub tier: TransportTier,
    /// The provider's message id.
    pub provider_id: Option<String>,
    /// The template it was rendered from.
    pub template: Option<String>,
    /// The subject line.
    pub subject: String,
    /// The sender's address.
    pub sender: String,
    /// SHA-256 (hex) of the first recipient's address, lower-cased.
    pub recipient_hash: String,
    /// How many recipients it had.
    pub recipients: u32,
    /// Why the transport refused it.
    pub error: Option<String>,
    /// When it was first opened.
    pub opened_at: Option<DateTime<Utc>>,
    /// When a link was first clicked.
    pub clicked_at: Option<DateTime<Utc>>,
    /// When it was handed to the transport.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<&delivery::Model> for Delivery {
    fn from(model: &delivery::Model) -> Self {
        Self {
            id: model.id,
            workspace_id: model.workspace_id,
            organization_id: model.organization_id,
            status: model.status.parse().unwrap_or(DeliveryStatus::Sent),
            transport: model.transport.clone(),
            tier: model.tier.parse().unwrap_or(TransportTier::Config),
            provider_id: model.provider_id.clone(),
            template: model.template.clone(),
            subject: model.subject.clone(),
            sender: model.sender.clone(),
            recipient_hash: model.recipient_hash.clone(),
            recipients: u32::try_from(model.recipients).unwrap_or_default(),
            error: model.error.clone(),
            opened_at: model.opened_at,
            clicked_at: model.clicked_at,
            created_at: model.created_at,
            updated_at: model.updated_at,
        }
    }
}

/// One thing that happened to a delivery (`mail_events`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct DeliveryEvent {
    /// The id.
    pub id: Uuid,
    /// The delivery, when it was found.
    pub delivery_id: Option<Uuid>,
    /// Who reported it.
    pub provider: String,
    /// What happened.
    pub kind: MailEventKind,
    /// SHA-256 (hex) of the recipient's address.
    pub recipient_hash: Option<String>,
    /// `hard` or `soft`, for bounces.
    pub bounce_type: Option<String>,
    /// The link, for clicks.
    pub url: Option<String>,
    /// The provider's diagnostic.
    pub detail: Option<String>,
    /// When it happened.
    pub occurred_at: DateTime<Utc>,
}

impl From<&event::Model> for DeliveryEvent {
    fn from(model: &event::Model) -> Self {
        Self {
            id: model.id,
            delivery_id: model.delivery_id,
            provider: model.provider.clone(),
            kind: model.kind.parse().unwrap_or(MailEventKind::Delivered),
            recipient_hash: model.recipient_hash.clone(),
            bounce_type: model.bounce_type.clone(),
            url: model.url.clone(),
            detail: model.detail.clone(),
            occurred_at: model.occurred_at,
        }
    }
}

/// Which deliveries [`Deliveries::list`] returns.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct DeliveryFilter {
    /// Only this workspace's.
    pub workspace_id: Option<Uuid>,
    /// Only those in this state.
    pub status: Option<DeliveryStatus>,
    /// Only those whose first recipient is this address.
    pub recipient: Option<String>,
    /// Only those rendered from this template.
    pub template: Option<String>,
}

/// The delivery log: one row per message sent with
/// `mail.delivery.enabled`, and the events providers report about it.
///
/// Provided and exported by [`MailModule`](crate::MailModule); needs
/// `DatabaseModule`. Provider webhooks and tracking call
/// [`record`](Deliveries::record); so can your own sources.
#[derive(Clone)]
pub struct Deliveries {
    store: Arc<Store>,
    suppressions: Arc<Suppressions>,
}

impl fmt::Debug for Deliveries {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Deliveries")
    }
}

impl Deliveries {
    pub(crate) fn new(store: Arc<Store>, suppressions: Arc<Suppressions>) -> Self {
        Self {
            store,
            suppressions,
        }
    }

    /// One delivery.
    pub async fn find(&self, id: Uuid) -> Result<Delivery, MailError> {
        let db = self.store.db().await?;
        delivery::Entity::find_by_id(id)
            .one(&db.reader().await)
            .await?
            .map(|model| Delivery::from(&model))
            .ok_or(MailError::NotFound("delivery"))
    }

    /// The delivery whose provider id is `provider_id`.
    pub async fn find_by_provider_id(&self, provider_id: &str) -> Result<Option<Delivery>, MailError> {
        let db = self.store.db().await?;
        Ok(delivery::Entity::find()
            .filter(delivery::Column::ProviderId.eq(provider_id))
            .one(&db.reader().await)
            .await?
            .map(|model| Delivery::from(&model)))
    }

    /// Deliveries matching `filter`, newest first.
    pub async fn list(
        &self,
        filter: &DeliveryFilter,
        page: PageQuery,
    ) -> Result<Page<Delivery>, MailError> {
        let db = self.store.db().await?;
        let mut select = delivery::Entity::find();
        if let Some(workspace) = filter.workspace_id {
            select = select.filter(delivery::Column::WorkspaceId.eq(workspace));
        }
        if let Some(status) = filter.status {
            select = select.filter(delivery::Column::Status.eq(status.as_str()));
        }
        if let Some(recipient) = &filter.recipient {
            select = select.filter(delivery::Column::RecipientHash.eq(recipient_hash(recipient)));
        }
        if let Some(template) = &filter.template {
            select = select.filter(delivery::Column::Template.eq(template.as_str()));
        }
        let select = select
            .order_by_desc(delivery::Column::CreatedAt)
            .order_by_desc(delivery::Column::Id);
        let page = paginate(select, &db.reader().await, page).await?;
        Ok(page.map(|model| Delivery::from(&model)))
    }

    /// A delivery's events, oldest first.
    pub async fn events(&self, delivery_id: Uuid) -> Result<Vec<DeliveryEvent>, MailError> {
        let db = self.store.db().await?;
        let models = event::Entity::find()
            .filter(event::Column::DeliveryId.eq(delivery_id))
            .order_by_asc(event::Column::OccurredAt)
            .order_by_asc(event::Column::Id)
            .all(&db.reader().await)
            .await?;
        Ok(models.iter().map(DeliveryEvent::from).collect())
    }

    /// Records `event` against its delivery (found by delivery id, or by
    /// provider id): logs it in `mail_events`, moves the delivery to
    /// `delivered`, `bounced` (hard bounces) or `complained`, stamps the
    /// first open and click, suppresses the recipient after a hard bounce
    /// or a complaint (`mail.delivery.auto_suppress`), and emits
    /// `MailDelivered`, `MailBounced`, `MailComplained`, `MailOpened` or
    /// `MailClicked` once committed. Returns the delivery, if found; an
    /// event for an unknown message is still logged.
    ///
    /// Joins the task's current unit of work when there is an unfinished
    /// one (a request handler's), and uses its own otherwise.
    pub async fn record(&self, event: ProviderEvent) -> Result<Option<Delivery>, MailError> {
        let now = self.store.now().await;
        let auto_suppress = self.store.options().delivery.auto_suppress;
        let store = self.store.clone();
        let suppressions = self.suppressions.clone();
        self.store
            .in_uow(|uow| async move {
                let tx = uow.tx().await?;
                let found = match (event.delivery_id, event.provider_id.as_deref()) {
                    (Some(id), _) => delivery::Entity::find_by_id(id).one(&*tx).await?,
                    (None, Some(provider_id)) => {
                        delivery::Entity::find()
                            .filter(delivery::Column::ProviderId.eq(provider_id))
                            .one(&*tx)
                            .await?
                    }
                    (None, None) => None,
                };
                let occurred_at = event.occurred_at.unwrap_or(now);
                let workspace_id = found.as_ref().and_then(|d| d.workspace_id);
                event::ActiveModel {
                    id: Set(Uuid::now_v7()),
                    delivery_id: Set(found.as_ref().map(|d| d.id)),
                    workspace_id: Set(workspace_id),
                    provider: Set(event.provider.clone()),
                    kind: Set(event.kind.as_str().to_owned()),
                    recipient_hash: Set(event.recipient.as_deref().map(recipient_hash)),
                    bounce_type: Set((event.kind == MailEventKind::Bounced).then(|| {
                        if event.permanent { "hard" } else { "soft" }.to_owned()
                    })),
                    url: Set(event.url.clone()),
                    detail: Set(event
                        .detail
                        .as_ref()
                        .map(|d| d.chars().take(2000).collect())),
                    occurred_at: Set(occurred_at),
                    created_at: Set(now),
                }
                .insert(&*tx)
                .await?;
                let updated = match &found {
                    Some(model) => Some(match changes(model, &event, occurred_at, now) {
                        Some(changes) => changes.update(&*tx).await?,
                        None => model.clone(),
                    }),
                    None => None,
                };
                drop(tx);
                if auto_suppress
                    && event.suppresses()
                    && let Some(recipient) = &event.recipient
                {
                    let reason = if event.kind == MailEventKind::Complained {
                        SuppressionReason::Complaint
                    } else {
                        SuppressionReason::HardBounce
                    };
                    suppressions
                        .add_in(
                            &uow,
                            workspace_id,
                            recipient,
                            reason,
                            event.detail.clone(),
                            found.as_ref().map(|d| d.id),
                        )
                        .await?;
                }
                emit(&store, &uow, &event, found.as_ref(), occurred_at).await;
                Ok(updated.as_ref().map(Delivery::from))
            })
            .await
    }

    /// Records every event of `events`; returns how many found their
    /// delivery.
    pub async fn record_all(&self, events: Vec<ProviderEvent>) -> Result<usize, MailError> {
        let mut found = 0;
        for event in events {
            if self.record(event).await?.is_some() {
                found += 1;
            }
        }
        Ok(found)
    }
}

/// How `event` changes `model`, if at all.
fn changes(
    model: &delivery::Model,
    event: &ProviderEvent,
    occurred_at: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Option<delivery::ActiveModel> {
    let status: DeliveryStatus = model.status.parse().unwrap_or(DeliveryStatus::Sent);
    let mut changes = delivery::ActiveModel {
        id: Unchanged(model.id),
        updated_at: Set(now),
        ..Default::default()
    };
    let mut changed = false;
    let mut set_status = |to: DeliveryStatus| {
        changes.status = Set(to.as_str().to_owned());
        changed = true;
    };
    match event.kind {
        MailEventKind::Delivered
            if matches!(status, DeliveryStatus::Sending | DeliveryStatus::Sent) =>
        {
            set_status(DeliveryStatus::Delivered);
        }
        MailEventKind::Bounced if event.permanent && status != DeliveryStatus::Complained => {
            set_status(DeliveryStatus::Bounced);
        }
        MailEventKind::Complained => set_status(DeliveryStatus::Complained),
        _ => {}
    }
    if matches!(event.kind, MailEventKind::Opened | MailEventKind::Clicked)
        && model.opened_at.is_none()
    {
        changes.opened_at = Set(Some(occurred_at));
        changed = true;
    }
    if event.kind == MailEventKind::Clicked && model.clicked_at.is_none() {
        changes.clicked_at = Set(Some(occurred_at));
        changed = true;
    }
    changed.then_some(changes)
}

/// Emits the event matching `event` once the unit of work commits.
async fn emit(
    store: &Store,
    uow: &navree_db::Uow,
    event: &ProviderEvent,
    delivery: Option<&delivery::Model>,
    occurred_at: DateTime<Utc>,
) {
    let delivery_id = delivery.map(|d| d.id);
    let workspace_id = delivery.and_then(|d| d.workspace_id);
    let provider = event.provider.clone();
    let recipient = event.recipient.clone();
    match event.kind {
        MailEventKind::Delivered => {
            store
                .emit_after_commit(
                    uow,
                    MailDelivered {
                        delivery_id,
                        workspace_id,
                        provider,
                        recipient,
                        occurred_at,
                    },
                )
                .await;
        }
        MailEventKind::Bounced => {
            store
                .emit_after_commit(
                    uow,
                    MailBounced {
                        delivery_id,
                        workspace_id,
                        provider,
                        recipient,
                        permanent: event.permanent,
                        detail: event.detail.clone(),
                        occurred_at,
                    },
                )
                .await;
        }
        MailEventKind::Complained => {
            store
                .emit_after_commit(
                    uow,
                    MailComplained {
                        delivery_id,
                        workspace_id,
                        provider,
                        recipient,
                        occurred_at,
                    },
                )
                .await;
        }
        MailEventKind::Opened => {
            store
                .emit_after_commit(
                    uow,
                    MailOpened {
                        delivery_id,
                        workspace_id,
                        provider,
                        recipient,
                        occurred_at,
                    },
                )
                .await;
        }
        MailEventKind::Clicked => {
            store
                .emit_after_commit(
                    uow,
                    MailClicked {
                        delivery_id,
                        workspace_id,
                        provider,
                        recipient,
                        url: event.url.clone().unwrap_or_default(),
                        occurred_at,
                    },
                )
                .await;
        }
        MailEventKind::Deferred => {}
    }
}
