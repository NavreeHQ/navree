//! [`Notifier`]: renders notifications and delivers them over their channels.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use chrono::{DateTime, SubsecRound, Utc};
use navree_contracts::{Broadcaster, Clock, Dispatcher, JobEnvelope};
use navree_db::{Db, Uow};
use navree_kernel::{AppContext, WeakAppContext};
use navree_mail::Mailer;
use navree_orm::{Executor, Model as _};
use navree_telemetry::{EventKind, NotificationSent, Telemetry, TelemetryEvent};
use sea_orm::DbErr;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::channels::ChannelDrivers;
use crate::deliveries::Outcome;
use crate::recipient::within;
use crate::{
    Channel, ChannelError, Delivery, DeliveryLog, Notifiable, Notification, NotifyError,
    NotifyOptions, OptOutSource, OptOuts, PushSubscriptions, SendNotificationJob, SentMessage,
    SlackRoute, TeamsRoute, WebPushMessage, notifications,
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
    /// Where delivered channels are reported (`NotificationSent`).
    telemetry: Telemetry,
    /// The drivers configured in `notify`.
    drivers: ChannelDrivers,
    /// The delivery log of the recorded channels.
    log: Arc<DeliveryLog>,
    /// The SMS and WhatsApp opt-outs.
    opt_outs: Arc<OptOuts>,
    /// The web push subscriptions.
    push: Arc<PushSubscriptions>,
}

/// What [`Notifier::new`] needs besides the options.
pub(crate) struct Parts {
    pub(crate) app: WeakAppContext,
    pub(crate) telemetry: Telemetry,
    pub(crate) drivers: ChannelDrivers,
    pub(crate) log: Arc<DeliveryLog>,
    pub(crate) opt_outs: Arc<OptOuts>,
    pub(crate) push: Arc<PushSubscriptions>,
}

impl Notifier {
    /// A notifier for the app in `parts` with `options`, reporting each
    /// delivered channel to the telemetry as a `NotificationSent` event.
    pub(crate) fn new(options: NotifyOptions, parts: Parts) -> Self {
        Self {
            options,
            app: parts.app,
            warned_inline: AtomicBool::new(false),
            telemetry: parts.telemetry,
            drivers: parts.drivers,
            log: parts.log,
            opt_outs: parts.opt_outs,
            push: parts.push,
        }
    }

    /// The application server key browsers subscribe with (base64url), when
    /// `notify.web_push` has a key pair.
    pub fn application_server_key(&self) -> Option<String> {
        self.drivers.application_server_key()
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
            if self.telemetry.enabled(EventKind::NotificationSent) {
                // Stored with the transaction: reported once it commits.
                let telemetry = self.telemetry.clone();
                uow.after_commit(move || {
                    Box::pin(async move {
                        for job in &stored {
                            report(&telemetry, job);
                        }
                    })
                });
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
        if self.deliver_channel(ctx, job).await? {
            report(&self.telemetry, job);
        }
        Ok(())
    }

    /// Delivers one channel; `false` when there was nothing to send (an
    /// opted-out number, no push subscriptions).
    async fn deliver_channel(
        &self,
        ctx: &AppContext,
        job: &SendNotificationJob,
    ) -> Result<bool, NotifyError> {
        match &job.delivery {
            Delivery::Database { data } => {
                let db = ctx
                    .try_get::<Db>()
                    .await
                    .ok_or_else(|| unavailable(Channel::Database, "add DatabaseModule"))?;
                store(db.conn(), job, data).await?;
                Ok(true)
            }
            Delivery::Mail { mail } => {
                let mailer = ctx
                    .try_get::<Mailer>()
                    .await
                    .ok_or_else(|| unavailable(Channel::Mail, "add MailModule"))?;
                // The template renders here, in the recipient's context.
                let sent = within(
                    ctx,
                    &job.notifiable_type,
                    &job.notifiable_id,
                    mailer.send(mail),
                )
                .await;
                sent?;
                Ok(true)
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
                    .map_err(NotifyError::Broadcast)?;
                Ok(true)
            }
            Delivery::Slack { route, message } => {
                let driver = self.drivers.slack(ctx).await?;
                let result = driver.send(route, message).await;
                let recipient = slack_recipient(route);
                self.finish(ctx, job, Channel::Slack, Some(&recipient), result)
                    .await
            }
            Delivery::MicrosoftTeams { route, message } => {
                let driver = self.drivers.teams(ctx).await?;
                let result = driver.send(route, message).await;
                let recipient = teams_recipient(route);
                self.finish(ctx, job, Channel::MicrosoftTeams, Some(&recipient), result)
                    .await
            }
            Delivery::Sms { to, message } => {
                let driver = self.drivers.sms(ctx).await.ok_or_else(|| {
                    unavailable(
                        Channel::Sms,
                        "set notify.sms.driver (twilio, vonage, sns or log), or register a `dyn SmsDriver`",
                    )
                })?;
                let Some(number) = self.number(ctx, job, Channel::Sms, to).await? else {
                    return Ok(false);
                };
                let result = driver.send(&number, message).await;
                self.finish(ctx, job, Channel::Sms, Some(&number), result)
                    .await
            }
            Delivery::WhatsApp { to, message } => {
                let driver = self.drivers.whatsapp(ctx).await.ok_or_else(|| {
                    unavailable(
                        Channel::WhatsApp,
                        "set notify.whatsapp.driver (twilio, meta or log), or register a `dyn WhatsAppDriver`",
                    )
                })?;
                let Some(number) = self.number(ctx, job, Channel::WhatsApp, to).await? else {
                    return Ok(false);
                };
                let result = driver.send(&number, message).await;
                self.finish(ctx, job, Channel::WhatsApp, Some(&number), result)
                    .await
            }
            Delivery::WebPush { message } => self.push_all(ctx, job, message).await,
        }
    }

    /// `to` in E.164, unless the number has opted out of `channel` (then
    /// the delivery is recorded `opted_out` and `None` returned).
    async fn number(
        &self,
        ctx: &AppContext,
        job: &SendNotificationJob,
        channel: Channel,
        to: &str,
    ) -> Result<Option<String>, NotifyError> {
        let number = match self.opt_outs.normalize(to) {
            Ok(number) => number,
            Err(error) => {
                let outcome = Outcome::Failed {
                    provider: None,
                    error: error.to_string(),
                };
                self.record(ctx, job, channel, Some(to), outcome).await;
                return Err(error);
            }
        };
        if has_database(ctx).await && self.opt_outs.is_opted_out(channel, &number).await? {
            self.record(ctx, job, channel, Some(&number), Outcome::OptedOut)
                .await;
            tracing::debug!(
                target: "navree::notify",
                notification = %job.id,
                %channel,
                "skipped an opted-out number"
            );
            return Ok(None);
        }
        Ok(Some(number))
    }

    /// Records a driver's answer; an opt-out at the provider opts the
    /// number out and is not an error.
    async fn finish(
        &self,
        ctx: &AppContext,
        job: &SendNotificationJob,
        channel: Channel,
        recipient: Option<&str>,
        result: Result<SentMessage, ChannelError>,
    ) -> Result<bool, NotifyError> {
        match result {
            Ok(sent) => {
                self.record(ctx, job, channel, recipient, Outcome::Sent(&sent))
                    .await;
                Ok(true)
            }
            Err(ChannelError::OptedOut { .. }) => {
                if let Some(number) = recipient
                    && matches!(channel, Channel::Sms | Channel::WhatsApp)
                    && has_database(ctx).await
                    && let Err(error) = self
                        .opt_outs
                        .opt_out(channel, number, OptOutSource::Provider)
                        .await
                {
                    tracing::warn!(target: "navree::notify", %channel, %error, "recording an opt-out failed");
                }
                self.record(ctx, job, channel, recipient, Outcome::OptedOut)
                    .await;
                Ok(false)
            }
            Err(error) => {
                let outcome = Outcome::Failed {
                    provider: provider_of(&error),
                    error: error.to_string(),
                };
                self.record(ctx, job, channel, recipient, outcome).await;
                Err(NotifyError::Channel {
                    channel,
                    source: error,
                })
            }
        }
    }

    /// Pushes `message` to every subscription of the job's notifiable;
    /// subscriptions the push service reports gone are deleted.
    async fn push_all(
        &self,
        ctx: &AppContext,
        job: &SendNotificationJob,
        message: &WebPushMessage,
    ) -> Result<bool, NotifyError> {
        let driver = self.drivers.web_push(ctx).await.ok_or_else(|| {
            unavailable(
                Channel::WebPush,
                "set notify.web_push.public_key and private_key (see `notify:vapid`), or register a `dyn WebPushDriver`",
            )
        })?;
        if !has_database(ctx).await {
            return Err(unavailable(
                Channel::WebPush,
                "push subscriptions are stored in the database: add DatabaseModule",
            ));
        }
        let subscriptions = self
            .push
            .of(&job.notifiable_type, &job.notifiable_id)
            .await?;
        // A retried job skips the subscriptions an earlier attempt reached.
        let reached = if self.options.deliveries {
            self.log.sent_recipients(job.id, Channel::WebPush).await?
        } else {
            Vec::new()
        };
        let mut sent_any = false;
        let mut errors = Vec::new();
        for subscription in subscriptions {
            let recipient = subscription.id.to_string();
            if reached.contains(&recipient) {
                continue;
            }
            let outcome = match driver.send(&subscription.target(), message).await {
                Ok(sent) => {
                    sent_any = true;
                    if let Err(error) = self.push.touch(subscription.id, now(ctx).await).await {
                        tracing::debug!(target: "navree::notify", %error, "updating a push subscription failed");
                    }
                    let outcome = Outcome::Sent(&sent);
                    self.record(ctx, job, Channel::WebPush, Some(&recipient), outcome)
                        .await;
                    continue;
                }
                Err(ChannelError::Gone) => {
                    if let Err(error) = self.push.forget(subscription.id).await {
                        tracing::warn!(target: "navree::notify", %error, "deleting a gone push subscription failed");
                    }
                    Outcome::Failed {
                        provider: None,
                        error: ChannelError::Gone.to_string(),
                    }
                }
                Err(error) => {
                    let outcome = Outcome::Failed {
                        provider: provider_of(&error),
                        error: error.to_string(),
                    };
                    errors.push(NotifyError::Channel {
                        channel: Channel::WebPush,
                        source: error,
                    });
                    outcome
                }
            };
            self.record(ctx, job, Channel::WebPush, Some(&recipient), outcome)
                .await;
        }
        match NotifyError::collect(errors) {
            Some(error) => Err(error),
            None => Ok(sent_any),
        }
    }

    /// Records an attempt in the delivery log, with `notify.deliveries`
    /// and `DatabaseModule`.
    async fn record(
        &self,
        ctx: &AppContext,
        job: &SendNotificationJob,
        channel: Channel,
        recipient: Option<&str>,
        outcome: Outcome<'_>,
    ) {
        if self.options.deliveries && has_database(ctx).await {
            self.log.record(job, channel, recipient, outcome).await;
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
/// dropped, channels that render nothing skipped), with a new id; in the
/// recipient's context when the app has a
/// [`RecipientScope`](crate::RecipientScope).
async fn render<N: Notification>(
    ctx: &AppContext,
    notifiable: &dyn Notifiable,
    notification: &N,
) -> (Uuid, Vec<SendNotificationJob>) {
    let id = Uuid::now_v7();
    let created_at = now(ctx).await;
    let notifiable_id = notifiable.notifiable_id();
    let deliveries = within(ctx, notifiable.notifiable_type(), &notifiable_id, async {
        let mut channels = notification.via(notifiable);
        dedup(&mut channels);
        channels
            .into_iter()
            .filter_map(|channel| render_channel(channel, notifiable, notification))
            .collect::<Vec<_>>()
    })
    .await;
    let jobs = deliveries
        .into_iter()
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
        Channel::Slack => {
            let message = notification.to_slack(notifiable)?;
            let route = message.to.clone().or_else(|| notifiable.route_slack())?;
            Some(Delivery::Slack { route, message })
        }
        Channel::MicrosoftTeams => {
            let message = notification.to_teams(notifiable)?;
            let route = message.to.clone().or_else(|| notifiable.route_teams())?;
            Some(Delivery::MicrosoftTeams { route, message })
        }
        Channel::Sms => {
            let message = notification.to_sms(notifiable)?;
            let to = message.to.clone().or_else(|| notifiable.route_sms())?;
            Some(Delivery::Sms { to, message })
        }
        Channel::WhatsApp => {
            let message = notification.to_whatsapp(notifiable)?;
            let to = message.to.clone().or_else(|| notifiable.route_whatsapp())?;
            Some(Delivery::WhatsApp { to, message })
        }
        Channel::WebPush => Some(Delivery::WebPush {
            message: notification.to_web_push(notifiable)?,
        }),
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

/// Emits a [`NotificationSent`] for one delivered channel.
fn report(telemetry: &Telemetry, job: &SendNotificationJob) {
    telemetry.emit_with(EventKind::NotificationSent, || {
        TelemetryEvent::NotificationSent(NotificationSent {
            notification_id: job.id.to_string(),
            notification_type: job.notification_type.clone(),
            notifiable_type: job.notifiable_type.clone(),
            notifiable_id: job.notifiable_id.clone(),
            channel: job.delivery.channel().to_string(),
        })
    });
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

/// Whether the app has a database (the channel tables).
async fn has_database(ctx: &AppContext) -> bool {
    ctx.try_get::<Db>().await.is_some()
}

/// What the log records as a Slack route's recipient: the channel, or the
/// webhook's host (its URL is a secret).
fn slack_recipient(route: &SlackRoute) -> String {
    match route {
        SlackRoute::Webhook { url } => webhook_recipient(url),
        SlackRoute::Bot { channel, .. } => channel.clone(),
    }
}

/// What the log records as a Teams route's recipient: the conversation,
/// or the webhook's host.
fn teams_recipient(route: &TeamsRoute) -> String {
    match route {
        TeamsRoute::Webhook { url } => webhook_recipient(url),
        TeamsRoute::Bot {
            conversation_id, ..
        } => conversation_id.clone(),
    }
}

/// `webhook:<host>`.
fn webhook_recipient(url: &str) -> String {
    let host = url::Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_default();
    format!("webhook:{host}")
}

/// The driver an error came from, when it names one.
fn provider_of(error: &ChannelError) -> Option<String> {
    match error {
        ChannelError::Refused { provider, .. }
        | ChannelError::Unreachable { provider, .. }
        | ChannelError::OptedOut { provider } => Some(provider.clone()),
        _ => None,
    }
}

/// [`now`] for a weak app (the system clock once it has shut down).
pub(crate) async fn now_in(app: &WeakAppContext) -> DateTime<Utc> {
    match app.upgrade() {
        Some(ctx) => now(&ctx).await,
        None => Utc::now().trunc_subsecs(6),
    }
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

/// Inserts the `notifications` row through `exec` (the pool, or the unit
/// of work's transaction), nothing when it exists: a retried job. A plain
/// insert, so it records no change events.
async fn store<E: Executor + ?Sized>(
    exec: &E,
    job: &SendNotificationJob,
    data: &Value,
) -> Result<(), NotifyError> {
    let row = notifications::Model {
        id: job.id,
        notifiable_type: job.notifiable_type.clone(),
        notifiable_id: job.notifiable_id.clone(),
        notification_type: job.notification_type.clone(),
        data: data.clone(),
        read_at: None,
        created_at: job.created_at,
    };
    notifications::Model::insert_or_ignore(exec, [row])
        .await
        .map_err(DbErr::from)?;
    Ok(())
}

/// [`NotifyError::Unavailable`] for `channel`.
fn unavailable(channel: Channel, reason: &str) -> NotifyError {
    NotifyError::Unavailable {
        channel: Some(channel),
        reason: reason.to_owned(),
    }
}
