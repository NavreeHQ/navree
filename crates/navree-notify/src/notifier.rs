//! [`Notifier`]: renders notifications and delivers them over their channels.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use chrono::{DateTime, SubsecRound, Utc};
use navree_contracts::{Broadcaster, Clock, Dispatcher, JobEnvelope};
use navree_db::{Db, Uow};
use navree_kernel::{AppContext, WeakAppContext};
use navree_mail::Mailer;
use sea_orm::sea_query::OnConflict;
use sea_orm::{ActiveValue::Set, ConnectionTrait, EntityTrait};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    Channel, Delivery, Notifiable, Notification, NotifyError, NotifyOptions, SendNotificationJob,
    notifications,
};

/// The event the broadcast channel sends.
pub const NOTIFICATION_CREATED: &str = "notification.created";

/// Sends notifications: asks each [`Notification`] for its channels and
/// renders it for each one, then delivers (or queues) the result.
///
/// Provided and exported by the global [`NotifyModule`](crate::NotifyModule):
/// inject `Arc<Notifier>`.
pub struct Notifier {
    options: NotifyOptions,
    /// The app, to find the database, mailer, broadcaster and dispatcher
    /// when sending (weak: the app owns the notifier).
    app: WeakAppContext,
    /// Whether the "no dispatcher, delivering now" warning was logged.
    warned_inline: AtomicBool,
}

impl Notifier {
    /// A notifier for `app` with `options`.
    pub(crate) fn new(options: NotifyOptions, app: WeakAppContext) -> Self {
        Self {
            options,
            app,
            warned_inline: AtomicBool::new(false),
        }
    }

    /// The options.
    pub fn options(&self) -> &NotifyOptions {
        &self.options
    }

    /// Sends `notification` to `notifiable` over every channel its
    /// [`via`](Notification::via) lists; returns the notification id.
    ///
    /// The notification is rendered now. With
    /// [`NotifyOptions::queue`] on, each channel is then dispatched as job
    /// `notify.send` ([`SendNotificationJob`]) through the app's
    /// `dyn Dispatcher`, so a queue worker delivers (and retries) it; without
    /// a dispatcher in the app, or with queueing off, it is delivered now
    /// as with [`send_now`](Notifier::send_now). Every channel is attempted;
    /// the error lists the channels that failed.
    ///
    /// Inside a unit of work's scope (`navree_db::Uow::current`: a request
    /// handler, `Db::transaction`) it behaves like
    /// [`send_in`](Notifier::send_in) with that unit of work while it is
    /// unfinished.
    pub async fn send<N: Notification>(
        &self,
        notifiable: &dyn Notifiable,
        notification: N,
    ) -> Result<Uuid, NotifyError> {
        let queue = self.options.queue;
        if let Some(uow) = current_uow().await {
            return self.send_with(&uow, notifiable, &notification, queue).await;
        }
        let ctx = self.app()?;
        let (id, jobs) = render(&ctx, notifiable, &notification).await;
        outcome(id, self.send_jobs(&ctx, &jobs, queue).await)
    }

    /// Sends `notification` to `notifiable` now, over every channel its
    /// [`via`](Notification::via) lists, even when queueing is on. Every
    /// channel is attempted; the error lists the channels that failed.
    /// Returns the notification id.
    ///
    /// Inside an unfinished unit of work's scope (`navree_db::Uow::current`)
    /// the database row is written in its transaction and the other
    /// channels are sent (not queued) once it commits, as by
    /// [`send_in`](Notifier::send_in).
    pub async fn send_now<N: Notification>(
        &self,
        notifiable: &dyn Notifiable,
        notification: N,
    ) -> Result<Uuid, NotifyError> {
        if let Some(uow) = current_uow().await {
            return self.send_with(&uow, notifiable, &notification, false).await;
        }
        let ctx = self.app()?;
        let (id, jobs) = render(&ctx, notifiable, &notification).await;
        outcome(id, self.send_jobs(&ctx, &jobs, false).await)
    }

    /// Sends `notification` to `notifiable` as part of the unit of work
    /// `uow` (in handlers, [`send`](Notifier::send) does this with the
    /// request's `Uow`): the
    /// database channel's row is written in `uow`'s transaction, and the
    /// other channels are sent (or queued, as by [`send`](Notifier::send))
    /// only once `uow` commits, nothing if it rolls back. Errors of those
    /// later channels are logged, not returned. Returns the notification id.
    ///
    /// On SQLite this is also what keeps a handler from blocking on itself:
    /// the request's transaction holds the database's only write lock, so
    /// a notification written through another connection would wait for it
    /// until `busy_timeout`.
    pub async fn send_in<N: Notification>(
        &self,
        uow: &Uow,
        notifiable: &dyn Notifiable,
        notification: N,
    ) -> Result<Uuid, NotifyError> {
        self.send_with(uow, notifiable, &notification, self.options.queue)
            .await
    }

    /// [`send_in`](Notifier::send_in), queueing the later channels when
    /// `queue` is set.
    async fn send_with<N: Notification>(
        &self,
        uow: &Uow,
        notifiable: &dyn Notifiable,
        notification: &N,
        queue: bool,
    ) -> Result<Uuid, NotifyError> {
        let ctx = self.app()?;
        let (id, jobs) = render(&ctx, notifiable, notification).await;
        let (stored, later): (Vec<_>, Vec<_>) = jobs
            .into_iter()
            .partition(|job| job.delivery.channel() == Channel::Database);
        if !stored.is_empty() {
            let tx = uow.tx().await?;
            for job in &stored {
                if let Delivery::Database { data } = &job.delivery {
                    store(&*tx, job, data).await?;
                }
            }
        }
        if !later.is_empty() {
            let app = self.app.clone();
            uow.after_commit(move || Box::pin(send_after_commit(app, id, later, queue)));
        }
        Ok(id)
    }

    /// Delivers `jobs` now, or dispatches them when `queue` is set and the
    /// app has a dispatcher; returns the failures.
    async fn send_jobs(
        &self,
        ctx: &AppContext,
        jobs: &[SendNotificationJob],
        queue: bool,
    ) -> Vec<NotifyError> {
        let dispatcher = if queue {
            self.dispatcher(ctx).await
        } else {
            None
        };
        let mut errors = Vec::new();
        for job in jobs {
            let result = match &dispatcher {
                Some(dispatcher) => dispatch(dispatcher.as_ref(), job).await,
                None => self.deliver_with(ctx, job).await,
            };
            if let Err(error) = result {
                errors.push(error);
            }
        }
        errors
    }

    /// Delivers one rendered channel (what job `notify.send` runs).
    pub(crate) async fn deliver(&self, job: &SendNotificationJob) -> Result<(), NotifyError> {
        let ctx = self.app()?;
        self.deliver_with(&ctx, job).await
    }

    async fn deliver_with(
        &self,
        ctx: &AppContext,
        job: &SendNotificationJob,
    ) -> Result<(), NotifyError> {
        match &job.delivery {
            Delivery::Database { data } => {
                let db = ctx
                    .try_get::<Db>()
                    .await
                    .ok_or_else(|| unavailable(Channel::Database, "add DatabaseModule"))?;
                store(db.conn(), job, data).await
            }
            Delivery::Mail { mail } => {
                let mailer = ctx
                    .try_get::<Mailer>()
                    .await
                    .ok_or_else(|| unavailable(Channel::Mail, "add MailModule"))?;
                Ok(mailer.send(mail).await?)
            }
            Delivery::Broadcast { channel, data } => {
                let broadcaster = ctx.try_get::<dyn Broadcaster>().await.ok_or_else(|| {
                    unavailable(
                        Channel::Broadcast,
                        "no `dyn Broadcaster` is registered (add RealtimeModule)",
                    )
                })?;
                broadcaster
                    .broadcast(
                        vec![channel.clone()],
                        NOTIFICATION_CREATED.to_owned(),
                        broadcast_payload(job, data),
                    )
                    .await
                    .map_err(NotifyError::Broadcast)
            }
        }
    }

    /// The current time (see [`now`]).
    pub(crate) async fn now(&self) -> DateTime<Utc> {
        match self.app.upgrade() {
            Some(ctx) => now(&ctx).await,
            None => Utc::now().trunc_subsecs(6),
        }
    }

    /// The app's job dispatcher; `None` (logged once) when it has none.
    async fn dispatcher(&self, ctx: &AppContext) -> Option<Arc<dyn Dispatcher>> {
        let dispatcher = ctx.try_get::<dyn Dispatcher>().await;
        if dispatcher.is_none() && !self.warned_inline.swap(true, Ordering::Relaxed) {
            tracing::warn!(
                target: "navree::notify",
                "no job dispatcher is registered (add QueueModule): queued notifications are delivered immediately"
            );
        }
        dispatcher
    }

    /// The app, unless it has been dropped.
    fn app(&self) -> Result<AppContext, NotifyError> {
        self.app.upgrade().ok_or_else(|| NotifyError::Unavailable {
            channel: None,
            reason: "the app has shut down".to_owned(),
        })
    }
}

impl fmt::Debug for Notifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Notifier")
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

/// The notification rendered for each channel `via` lists (duplicates
/// dropped, channels that render nothing skipped), with a new id.
async fn render<N: Notification>(
    ctx: &AppContext,
    notifiable: &dyn Notifiable,
    notification: &N,
) -> (Uuid, Vec<SendNotificationJob>) {
    let id = Uuid::now_v7();
    let created_at = now(ctx).await;
    let mut channels = notification.via(notifiable);
    dedup(&mut channels);
    let jobs = channels
        .into_iter()
        .filter_map(|channel| render_channel(channel, notifiable, notification))
        .map(|delivery| SendNotificationJob {
            id,
            notification_type: N::TYPE.to_owned(),
            notifiable_type: notifiable.notifiable_type().to_owned(),
            notifiable_id: notifiable.notifiable_id(),
            created_at,
            delivery,
        })
        .collect();
    (id, jobs)
}

/// The notification rendered for `channel`, if it renders anything there.
fn render_channel<N: Notification>(
    channel: Channel,
    notifiable: &dyn Notifiable,
    notification: &N,
) -> Option<Delivery> {
    match channel {
        Channel::Database => notification
            .to_database(notifiable)
            .map(|data| Delivery::Database { data }),
        Channel::Mail => {
            let mut mail = notification.to_mail(notifiable)?;
            if mail.to.is_empty() {
                mail.to.push(notifiable.route_mail()?);
            }
            Some(Delivery::Mail { mail })
        }
        Channel::Broadcast => {
            let channel = notifiable.broadcast_channel()?;
            let data = notification.to_broadcast(notifiable)?;
            Some(Delivery::Broadcast { channel, data })
        }
    }
}

/// `Ok(id)`, or the failures as one error.
fn outcome(id: Uuid, errors: Vec<NotifyError>) -> Result<Uuid, NotifyError> {
    match NotifyError::collect(errors) {
        Some(error) => Err(error),
        None => Ok(id),
    }
}

/// The unit of work of the current task, unless there is none or it has
/// finished.
async fn current_uow() -> Option<Uow> {
    let uow = Uow::current()?;
    (!uow.is_finished().await).then_some(uow)
}

/// The after-commit part of [`Notifier::send_in`]: sends (or, with `queue`,
/// queues) the channels other than the database, logging failures.
async fn send_after_commit(
    app: WeakAppContext,
    id: Uuid,
    jobs: Vec<SendNotificationJob>,
    queue: bool,
) {
    let Some(ctx) = app.upgrade() else {
        tracing::warn!(
            target: "navree::notify",
            notification = %id,
            "the app shut down before the notification was sent"
        );
        return;
    };
    let notifier = match ctx.get::<Notifier>().await {
        Ok(notifier) => notifier,
        Err(error) => {
            tracing::warn!(
                target: "navree::notify",
                notification = %id,
                %error,
                "cannot resolve the notifier"
            );
            return;
        }
    };
    for error in notifier.send_jobs(&ctx, &jobs, queue).await {
        tracing::warn!(
            target: "navree::notify",
            notification = %id,
            %error,
            "sending a notification after commit failed"
        );
    }
}

/// Dispatches `job` as `notify.send`.
async fn dispatch(
    dispatcher: &dyn Dispatcher,
    job: &SendNotificationJob,
) -> Result<(), NotifyError> {
    let envelope = JobEnvelope::of(job).map_err(NotifyError::Queue)?;
    let job_id = dispatcher
        .dispatch_envelope(envelope)
        .await
        .map_err(NotifyError::Queue)?;
    tracing::debug!(
        target: "navree::notify",
        job_id,
        notification = %job.id,
        channel = %job.delivery.channel(),
        "notification queued"
    );
    Ok(())
}

/// The `notification.created` event data: the notification as the
/// `/api/notifications` list shows it, `{id, type, data, read_at, created_at}`.
fn broadcast_payload(job: &SendNotificationJob, data: &Value) -> Value {
    json!({
        "id": job.id,
        "type": job.notification_type,
        "data": data,
        "read_at": null,
        "created_at": job.created_at,
    })
}

/// Removes repeated channels, keeping the first occurrence.
fn dedup(channels: &mut Vec<Channel>) {
    let mut seen = Vec::with_capacity(channels.len());
    channels.retain(|channel| {
        let first = !seen.contains(channel);
        seen.push(*channel);
        first
    });
}

/// The app clock's time (`dyn Clock` when registered), in microseconds
/// like a PostgreSQL timestamp, so stored rows and broadcasts agree.
async fn now(ctx: &AppContext) -> DateTime<Utc> {
    let now = match ctx.try_get::<dyn Clock>().await {
        Some(clock) => clock.now(),
        None => Utc::now(),
    };
    now.trunc_subsecs(6)
}

/// Inserts the `notifications` row (nothing when it exists: a retried job).
async fn store<C: ConnectionTrait>(
    conn: &C,
    job: &SendNotificationJob,
    data: &Value,
) -> Result<(), NotifyError> {
    let row = notifications::ActiveModel {
        id: Set(job.id),
        notifiable_type: Set(job.notifiable_type.clone()),
        notifiable_id: Set(job.notifiable_id.clone()),
        notification_type: Set(job.notification_type.clone()),
        data: Set(data.clone()),
        read_at: Set(None),
        created_at: Set(job.created_at),
    };
    notifications::Entity::insert(row)
        .on_conflict(
            OnConflict::column(notifications::Column::Id)
                .do_nothing()
                .to_owned(),
        )
        .exec_without_returning(conn)
        .await?;
    Ok(())
}

/// [`NotifyError::Unavailable`] for `channel`.
fn unavailable(channel: Channel, reason: &str) -> NotifyError {
    NotifyError::Unavailable {
        channel: Some(channel),
        reason: reason.to_owned(),
    }
}
