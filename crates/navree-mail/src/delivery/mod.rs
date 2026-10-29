//! The delivery pipeline (`mail.delivery.enabled`): the delivery log,
//! provider events, suppressions, the outbox, tenant transports and
//! tracking.

pub(crate) mod controller;
pub mod entity;
pub mod events;
pub(crate) mod ingest;
pub(crate) mod log;
pub(crate) mod migration;
pub(crate) mod outbox;
pub(crate) mod resolver;
pub(crate) mod suppressions;
pub(crate) mod tracking;

use std::future::Future;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use navree_contracts::{Clock, SystemClock};
use navree_crypto::Encrypter;
use navree_db::record::{self, Filter};
use navree_db::{Db, Record as _, Uow};
use navree_events::{Event, EventBus};
use navree_kernel::{AppContext, WeakAppContext};
use sha2::{Digest as _, Sha256};
use tokio::sync::OnceCell;
use uuid::Uuid;

use crate::mailer::SendOptions;
use crate::message::bare_address;
use crate::{
    DeliveryStatus, MailError, MailOptions, MailTransport, Message, SendReport, Sent, TransportTier,
};
use entity::DeliveryRow;
use outbox::Outbox;
use resolver::TransportResolver;
use suppressions::Suppressions;
use tracking::Tracking;

/// SHA-256 (hex) of an address, lower-cased: how the log names recipients.
pub(crate) fn recipient_hash(address: &str) -> String {
    hex::encode(Sha256::digest(bare_address(address).as_bytes()))
}

/// What the delivery services use from the app, resolved on first use.
struct Deps {
    db: Option<Db>,
    encrypter: Option<Arc<Encrypter>>,
    events: Option<Arc<EventBus>>,
    clock: Arc<dyn Clock>,
}

/// Shared by the delivery services: the app, the options and the
/// database.
pub(crate) struct Store {
    app: WeakAppContext,
    options: Arc<MailOptions>,
    deps: OnceCell<Deps>,
}

impl Store {
    pub(crate) fn new(app: &AppContext, options: Arc<MailOptions>) -> Self {
        Self {
            app: app.downgrade(),
            options,
            deps: OnceCell::new(),
        }
    }

    pub(crate) fn options(&self) -> &MailOptions {
        &self.options
    }

    pub(crate) fn app(&self) -> Result<AppContext, MailError> {
        self.app
            .upgrade()
            .ok_or(MailError::Missing("the app has shut down"))
    }

    async fn deps(&self) -> Result<&Deps, MailError> {
        self.deps
            .get_or_try_init(|| async {
                let app = self.app()?;
                Ok(Deps {
                    db: app.try_get::<Db>().await.map(|db| (*db).clone()),
                    encrypter: app.try_get::<Encrypter>().await,
                    events: app.try_get::<EventBus>().await,
                    clock: match app.try_get::<dyn Clock>().await {
                        Some(clock) => clock,
                        None => Arc::new(SystemClock),
                    },
                })
            })
            .await
    }

    /// The database.
    pub(crate) async fn db(&self) -> Result<Db, MailError> {
        self.deps().await?.db.clone().ok_or(MailError::Missing(
            "the mail delivery tables need DatabaseModule",
        ))
    }

    /// The current time (the app's `dyn Clock` when it has one).
    pub(crate) async fn now(&self) -> DateTime<Utc> {
        match self.deps().await {
            Ok(deps) => deps.clock.now(),
            Err(_) => Utc::now(),
        }
    }

    /// `plaintext` encrypted with the app key.
    pub(crate) async fn seal(&self, plaintext: &str) -> Result<String, MailError> {
        let encrypter = self.encrypter().await?;
        Ok(encrypter.encrypt_string(plaintext)?)
    }

    /// `sealed` decrypted with the app key.
    pub(crate) async fn open(&self, sealed: &str) -> Result<String, MailError> {
        let encrypter = self.encrypter().await?;
        Ok(encrypter.decrypt_string(sealed)?)
    }

    async fn encrypter(&self) -> Result<Arc<Encrypter>, MailError> {
        match &self.deps().await?.encrypter {
            Some(encrypter) if encrypter.current_kid().is_some() => Ok(encrypter.clone()),
            _ => Err(MailError::Missing(
                "stored transports and held mail are encrypted: add CryptoModule and set APP_KEY",
            )),
        }
    }

    /// Emits `event` once `uow` commits (when the app has `EventsModule`).
    pub(crate) async fn emit_after_commit<E: Event>(&self, uow: &Uow, event: E) {
        let Ok(deps) = self.deps().await else {
            return;
        };
        let Some(bus) = deps.events.clone() else {
            return;
        };
        uow.after_commit(move || {
            Box::pin(async move {
                if let Err(error) = bus.emit(event).await {
                    tracing::warn!(target: "navree::mail", error = %error, event = E::NAME, "a mail event listener failed");
                }
            })
        });
    }

    /// Runs `f` in the task's current unit of work when it is unfinished
    /// and on this database (a request's: the request commits it), else in
    /// a new one that commits when `f` succeeds.
    pub(crate) async fn in_uow<T, F, Fut>(&self, f: F) -> Result<T, MailError>
    where
        F: FnOnce(Uow) -> Fut,
        Fut: Future<Output = Result<T, MailError>>,
    {
        let db = self.db().await?;
        if let Some(current) = Uow::current()
            && current.db().same_as(&db)
            && !current.is_finished().await
        {
            return f(current).await;
        }
        let uow = db.uow(None, None).await;
        match uow.scope(f(uow.clone())).await {
            Ok(value) => {
                uow.commit().await?;
                Ok(value)
            }
            Err(error) => {
                if let Err(rollback) = uow.rollback().await {
                    tracing::warn!(target: "navree::mail", error = %rollback, "rollback failed");
                }
                Err(error)
            }
        }
    }
}

/// What the [`Mailer`](crate::Mailer) asks of the delivery services.
pub(crate) struct Pipeline {
    store: Arc<Store>,
    suppressions: Arc<Suppressions>,
    outbox: Arc<Outbox>,
    resolver: Arc<TransportResolver>,
    tracking: Option<Tracking>,
}

impl Pipeline {
    pub(crate) fn new(
        store: Arc<Store>,
        suppressions: Arc<Suppressions>,
        outbox: Arc<Outbox>,
        resolver: Arc<TransportResolver>,
        tracking: Option<Tracking>,
    ) -> Self {
        Self {
            store,
            suppressions,
            outbox,
            resolver,
            tracking,
        }
    }

    /// Stores a held message in the outbox.
    pub(crate) async fn hold(
        &self,
        message: &Message,
        options: &SendOptions,
    ) -> Result<SendReport, MailError> {
        let held = self.outbox.hold(message, options).await?;
        let mut report = SendReport::new(DeliveryStatus::Held);
        report.outbox_id = Some(held.id);
        Ok(report)
    }

    /// Leaves suppressed recipients out of `message`; returns them.
    pub(crate) async fn suppress(
        &self,
        message: &mut Message,
        options: &SendOptions,
    ) -> Result<Vec<String>, MailError> {
        if !self.store.options().delivery.suppress {
            return Ok(Vec::new());
        }
        let workspace = options.scope.as_ref().and_then(|s| s.workspace_id);
        let suppressed = self
            .suppressions
            .suppressed_among(workspace, message.recipients())
            .await?;
        if suppressed.is_empty() {
            return Ok(Vec::new());
        }
        let mut removed = Vec::new();
        for list in [&mut message.to, &mut message.cc, &mut message.bcc] {
            list.retain(|address| {
                if suppressed.contains(&bare_address(address)) {
                    removed.push(address.clone());
                    false
                } else {
                    true
                }
            });
        }
        Ok(removed)
    }

    /// Logs a message whose every recipient is suppressed.
    pub(crate) async fn log_suppressed(
        &self,
        message: &Message,
        options: &SendOptions,
        suppressed: Vec<String>,
    ) -> Result<SendReport, MailError> {
        let first = suppressed.first().cloned().unwrap_or_default();
        let id = self
            .insert(
                message,
                options,
                &first,
                suppressed.len(),
                ("none", TransportTier::Config),
                DeliveryStatus::Suppressed,
            )
            .await?;
        let mut report = SendReport::new(DeliveryStatus::Suppressed);
        report.delivery_id = Some(id);
        report.suppressed = suppressed;
        Ok(report)
    }

    /// The tenant transport for the message, if one applies.
    pub(crate) async fn resolve(
        &self,
        message: &Message,
        options: &SendOptions,
    ) -> Result<Option<(Arc<dyn MailTransport>, TransportTier)>, MailError> {
        if !self.store.options().delivery.tenants {
            return Ok(None);
        }
        let scope = options.scope.clone().unwrap_or_default();
        Ok(self
            .resolver
            .resolve(&scope, message.sender_domain().as_deref())
            .await?
            .map(|resolved| (resolved.transport, resolved.tier)))
    }

    /// Logs the message as `sending`; returns the delivery id.
    pub(crate) async fn start(
        &self,
        message: &Message,
        options: &SendOptions,
        transport: &'static str,
        tier: TransportTier,
    ) -> Result<Uuid, MailError> {
        let first = message.recipients().next().unwrap_or_default().to_owned();
        let count = message.recipients().count();
        self.insert(
            message,
            options,
            &first,
            count,
            (transport, tier),
            DeliveryStatus::Sending,
        )
        .await
    }

    /// Adds tracking to the HTML body.
    pub(crate) fn track(&self, message: &mut Message, delivery_id: Uuid) -> Result<(), MailError> {
        if let Some(tracking) = &self.tracking
            && let Some(html) = message.html.take()
        {
            message.html = Some(tracking.rewrite(&html, delivery_id));
        }
        Ok(())
    }

    /// Records the transport's answer.
    pub(crate) async fn finish(
        &self,
        delivery_id: Uuid,
        outcome: &Result<Sent, MailError>,
    ) -> Result<(), MailError> {
        let now = self.store.now().await;
        let row = Filter::all().eq("id", delivery_id);
        let changes = match outcome {
            Ok(sent) => DeliveryRow::update_where(row)
                .set("status", DeliveryStatus::Sent.as_str())
                .set("transport", sent.transport)
                .set("provider_id", sent.provider_id.clone()),
            Err(error) => DeliveryRow::update_where(row)
                .set("status", DeliveryStatus::Failed.as_str())
                .set(
                    "error",
                    error.to_string().chars().take(1000).collect::<String>(),
                ),
        }
        .set("updated_at", now);
        self.store
            .in_uow(|uow| async move {
                changes.execute(&uow).await?;
                Ok(())
            })
            .await
    }

    /// Inserts a delivery row.
    async fn insert(
        &self,
        message: &Message,
        options: &SendOptions,
        first_recipient: &str,
        recipients: usize,
        (transport, tier): (&'static str, TransportTier),
        status: DeliveryStatus,
    ) -> Result<Uuid, MailError> {
        let now = self.store.now().await;
        let id = Uuid::now_v7();
        let scope = options.scope.clone().unwrap_or_default();
        let row = DeliveryRow {
            id,
            workspace_id: scope.workspace_id,
            organization_id: scope.organization_id,
            status: status.as_str().to_owned(),
            transport: transport.to_owned(),
            tier: tier.as_str().to_owned(),
            provider_id: None,
            template: options.template.clone(),
            subject: message.subject.clone(),
            sender: message
                .from
                .as_deref()
                .map(bare_address)
                .unwrap_or_default(),
            recipient_hash: recipient_hash(first_recipient),
            recipients: i32::try_from(recipients).unwrap_or(i32::MAX),
            error: None,
            opened_at: None,
            clicked_at: None,
            created_at: now,
            updated_at: now,
        };
        self.store
            .in_uow(|uow| async move {
                record::insert(&uow, &row).await?;
                Ok(())
            })
            .await?;
        Ok(id)
    }
}
