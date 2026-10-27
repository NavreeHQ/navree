//! [`Mailer`]: renders mailables and hands messages to the transport.

use std::any::Any;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use navree_contracts::{Dispatcher, JobEnvelope};
use navree_kernel::WeakAppContext;
use navree_telemetry::{EventKind, MailSent, Telemetry, TelemetryEvent};
use navree_view::Views;

use crate::delivery::Pipeline;
use crate::render::{self, RenderOptions};
use crate::{
    ArrayTransport, DeliveryStatus, MailError, MailScope, MailTransport, Mailable, Message,
    SendMailJob, SendReport, TransportTier,
};

/// Sends mail: renders [`Mailable`]s with the app's [`Views`], fills in the
/// default sender, inlines CSS and adds the plain-text body
/// ([`RenderOptions`]), validates the message and hands it to a
/// [`MailTransport`].
///
/// Provided and exported by the global [`MailModule`](crate::MailModule):
/// inject `Arc<Mailer>`.
///
/// - [`send`](Mailer::send) renders and delivers now;
///   [`deliver`](Mailer::deliver) does the same and returns a
///   [`SendReport`] (delivery id, transport, provider id);
/// - [`queue`](Mailer::queue) renders and validates now, then dispatches job
///   `mail.send` ([`SendMailJob`]) through the app's `dyn Dispatcher`, so a
///   queue worker delivers it (and retries it). Without a dispatcher (no
///   queue module), it delivers now;
/// - [`via`](Mailer::via) sends through a named transport
///   (`mail.transports.<name>`), [`for_scope`](Mailer::for_scope) for a
///   tenant: its transport, its suppressions and its delivery log.
///
/// With `mail.delivery.enabled`, every message is logged in
/// `mail_deliveries`, suppressed recipients are left out, held messages
/// ([`Mailable::hold`]) go to the outbox, tenant transports are resolved
/// and opens and clicks can be tracked.
pub struct Mailer {
    transport: Arc<dyn MailTransport>,
    /// Named transports (`mail.transports.<name>`).
    named: BTreeMap<String, Arc<dyn MailTransport>>,
    views: Arc<Views>,
    from: String,
    render: RenderOptions,
    /// The queue for queued mail; `None` keeps the job's default.
    queue: Option<String>,
    /// The app, to find the job dispatcher (weak: the app owns the mailer).
    app: Option<WeakAppContext>,
    /// Whether the "no dispatcher, sending now" warning was logged.
    warned_inline: AtomicBool,
    /// Where sent mail is reported (`MailSent`).
    telemetry: Telemetry,
    /// The delivery log, suppressions, outbox, tenant transports and
    /// tracking (`mail.delivery.enabled`).
    delivery: Option<Arc<Pipeline>>,
}

/// How one message is sent.
#[derive(Clone, Debug, Default)]
pub(crate) struct SendOptions {
    pub(crate) scope: Option<MailScope>,
    pub(crate) via: Option<String>,
    pub(crate) template: Option<String>,
    /// Release from the outbox: send even though the message is held.
    pub(crate) releasing: bool,
    /// The message is already rendered (queued, or from the outbox).
    pub(crate) rendered: bool,
}

impl Mailer {
    /// A mailer sending through `transport`, rendering with `views`, with
    /// `from` as the default sender (empty for none). A mailer built this
    /// way has no job dispatcher, so [`queue`](Mailer::queue) sends now.
    pub fn new(
        transport: Arc<dyn MailTransport>,
        views: Arc<Views>,
        from: impl Into<String>,
    ) -> Self {
        Self {
            transport,
            named: BTreeMap::new(),
            views,
            from: from.into(),
            render: RenderOptions::default(),
            queue: None,
            app: None,
            warned_inline: AtomicBool::new(false),
            telemetry: Telemetry::disabled(),
            delivery: None,
        }
    }

    /// This mailer, reporting every mail the transport accepted to
    /// `telemetry` as a `MailSent` event (recipients, subject, rendered
    /// bodies, transport and duration). `MailModule` gives its mailer the
    /// app's registry.
    #[must_use]
    pub fn with_telemetry(mut self, telemetry: Telemetry) -> Self {
        self.telemetry = telemetry;
        self
    }

    /// This mailer, with a named transport that [`via`](Mailer::via)
    /// selects (`MailModule` adds `mail.transports`).
    #[must_use]
    pub fn with_transport(
        mut self,
        name: impl Into<String>,
        transport: Arc<dyn MailTransport>,
    ) -> Self {
        self.named.insert(name.into(), transport);
        self
    }

    /// This mailer, post-processing mail with `options`.
    #[must_use]
    pub fn with_render(mut self, options: RenderOptions) -> Self {
        self.render = options;
        self
    }

    /// Queues mail on `queue` and finds the dispatcher in `app`.
    pub(crate) fn queued_through(mut self, app: WeakAppContext, queue: Option<String>) -> Self {
        self.app = Some(app);
        self.queue = queue.filter(|queue| !queue.is_empty());
        self
    }

    /// Logs, suppresses, holds and resolves tenant transports through
    /// `pipeline`.
    pub(crate) fn with_pipeline(mut self, pipeline: Arc<Pipeline>) -> Self {
        self.delivery = Some(pipeline);
        self
    }

    /// Renders `mailable` and sends it now.
    pub async fn send(&self, mailable: &(impl Mailable + ?Sized)) -> Result<(), MailError> {
        self.deliver(mailable).await.map(drop)
    }

    /// Renders `mailable`, sends it now and says what happened: sent (with
    /// the transport, its tier and the provider's id), held in the outbox,
    /// or suppressed.
    pub async fn deliver(
        &self,
        mailable: &(impl Mailable + ?Sized),
    ) -> Result<SendReport, MailError> {
        let options = SendOptions {
            template: mailable.template().map(str::to_owned),
            ..SendOptions::default()
        };
        self.deliver_with(mailable.build(&self.views)?, options)
            .await
    }

    /// Sends `message` now, after filling in the default sender and
    /// validating it (see [`Message::validate`]).
    pub async fn send_message(&self, message: Message) -> Result<(), MailError> {
        self.deliver_with(message, SendOptions::default())
            .await
            .map(drop)
    }

    /// Sends through the transport named `name` in `mail.transports`
    /// (a lane: `simulation`, `notifications`), whatever the tenant
    /// transports say.
    pub fn via(&self, name: impl Into<String>) -> ScopedMailer<'_> {
        ScopedMailer {
            mailer: self,
            options: SendOptions {
                via: Some(name.into()),
                ..SendOptions::default()
            },
        }
    }

    /// Sends for `scope`: through its transport (domain, workspace,
    /// organization, platform, then the configured one), leaving out the
    /// recipients it suppresses, logged under it.
    pub fn for_scope(&self, scope: &MailScope) -> ScopedMailer<'_> {
        ScopedMailer {
            mailer: self,
            options: SendOptions {
                scope: Some(scope.clone()),
                ..SendOptions::default()
            },
        }
    }

    /// The pipeline every send goes through.
    pub(crate) async fn deliver_with(
        &self,
        message: Message,
        options: SendOptions,
    ) -> Result<SendReport, MailError> {
        let mut message = self.prepare(message, !options.rendered)?;
        let Some(pipeline) = &self.delivery else {
            if message.hold.is_some() && !options.releasing {
                return Err(MailError::Missing(
                    "holding mail for approval needs `mail.delivery.enabled`",
                ));
            }
            let (transport, tier) = self.configured_transport(options.via.as_deref())?;
            let sent = self.hand_over(&transport, &message).await?;
            let mut report = SendReport::new(DeliveryStatus::Sent);
            report.transport = Some(sent.transport);
            report.tier = Some(tier);
            report.provider_id = sent.provider_id;
            return Ok(report);
        };
        if message.hold.is_some() && !options.releasing {
            return pipeline.hold(&message, &options).await;
        }
        let suppressed = pipeline.suppress(&mut message, &options).await?;
        if message.recipients().next().is_none() {
            return pipeline
                .log_suppressed(&message, &options, suppressed)
                .await;
        }
        let (transport, tier) = match options.via.as_deref() {
            Some(name) => self.configured_transport(Some(name))?,
            None => match pipeline.resolve(&message, &options).await? {
                Some(resolved) => resolved,
                None => (self.transport.clone(), TransportTier::Config),
            },
        };
        let delivery_id = pipeline
            .start(&message, &options, transport.name(), tier)
            .await?;
        pipeline.track(&mut message, delivery_id)?;
        let outcome = self.hand_over(&transport, &message).await;
        pipeline.finish(delivery_id, &outcome).await?;
        let sent = outcome?;
        let mut report = SendReport::new(DeliveryStatus::Sent);
        report.delivery_id = Some(delivery_id);
        report.transport = Some(sent.transport);
        report.tier = Some(tier);
        report.provider_id = sent.provider_id;
        report.suppressed = suppressed;
        Ok(report)
    }

    /// The transport named `via`, or the default one.
    fn configured_transport(
        &self,
        via: Option<&str>,
    ) -> Result<(Arc<dyn MailTransport>, TransportTier), MailError> {
        match via {
            None => Ok((self.transport.clone(), TransportTier::Config)),
            Some(name) => self
                .named
                .get(name)
                .map(|transport| (transport.clone(), TransportTier::Config))
                .ok_or_else(|| {
                    MailError::Config(format!(
                        "no mail transport named `{name}` (set `mail.transports.{name}`)"
                    ))
                }),
        }
    }

    /// Hands `message` to `transport` and reports it once accepted.
    async fn hand_over(
        &self,
        transport: &Arc<dyn MailTransport>,
        message: &Message,
    ) -> Result<crate::Sent, MailError> {
        let started = Instant::now();
        let sent = transport.deliver(message).await?;
        self.telemetry.emit_with(EventKind::MailSent, || {
            TelemetryEvent::MailSent(MailSent {
                to: message.to.clone(),
                cc: message.cc.clone(),
                bcc: message.bcc.clone(),
                from: message.from.clone(),
                subject: message.subject.clone(),
                text: message.text.clone(),
                html: message.html.clone(),
                transport: sent.transport.to_owned(),
                duration: started.elapsed(),
            })
        });
        Ok(sent)
    }

    /// Renders and validates `mailable` now, then dispatches job `mail.send`
    /// carrying the rendered message (on `mail.queue` when configured).
    /// Rendering or validation errors are returned here, before anything is
    /// queued. Without a `dyn Dispatcher` in the app, the message is sent
    /// now (a warning is logged once). A held mailable goes to the outbox
    /// now instead.
    pub async fn queue(&self, mailable: &(impl Mailable + ?Sized)) -> Result<(), MailError> {
        let options = SendOptions {
            template: mailable.template().map(str::to_owned),
            ..SendOptions::default()
        };
        self.queue_with(mailable.build(&self.views)?, options).await
    }

    /// Queues `message` with `options`.
    pub(crate) async fn queue_with(
        &self,
        message: Message,
        mut options: SendOptions,
    ) -> Result<(), MailError> {
        let message = self.prepare(message, !options.rendered)?;
        options.rendered = true;
        if message.hold.is_some() {
            return self.deliver_with(message, options).await.map(drop);
        }
        let Some(dispatcher) = self.dispatcher().await else {
            self.warn_inline();
            return self.deliver_with(message, options).await.map(drop);
        };
        let job = SendMailJob {
            message,
            scope: options.scope,
            via: options.via,
            template: options.template,
        };
        let mut envelope = JobEnvelope::of(&job).map_err(|source| MailError::Queue { source })?;
        if let Some(queue) = &self.queue {
            envelope.options.queue.clone_from(queue);
        }
        let job_id = dispatcher
            .dispatch_envelope(envelope)
            .await
            .map_err(|source| MailError::Queue { source })?;
        tracing::debug!(target: "navree::mail", job_id, "mail queued");
        Ok(())
    }

    /// The transport messages are handed to.
    pub fn transport(&self) -> &Arc<dyn MailTransport> {
        &self.transport
    }

    /// The transport named `name` in `mail.transports`.
    pub fn named_transport(&self, name: &str) -> Option<&Arc<dyn MailTransport>> {
        self.named.get(name)
    }

    /// The [`ArrayTransport`] recording this mailer's messages, when that is
    /// the transport (`transport = "array"`, or an `ArrayTransport` given to
    /// `override_value::<dyn MailTransport>`): for assertions in tests.
    pub fn array_transport(&self) -> Option<&ArrayTransport> {
        (self.transport.as_ref() as &dyn Any).downcast_ref::<ArrayTransport>()
    }

    /// The views mailables render with.
    pub fn views(&self) -> &Arc<Views> {
        &self.views
    }

    /// The message with the default sender filled in, validated and, when
    /// `render` is true, rendered (CSS, text, locale).
    fn prepare(&self, mut message: Message, render: bool) -> Result<Message, MailError> {
        if message.from.is_none() && !self.from.is_empty() {
            message.from = Some(self.from.clone());
        }
        message.validate()?;
        if render {
            render::prepare(&mut message, &self.render)?;
        }
        Ok(message)
    }

    /// The app's job dispatcher, if the app is alive and has one.
    async fn dispatcher(&self) -> Option<Arc<dyn Dispatcher>> {
        let app = self.app.as_ref()?.upgrade()?;
        app.try_get::<dyn Dispatcher>().await
    }

    /// Logs once per mailer that queued mail is sent immediately.
    fn warn_inline(&self) {
        if !self.warned_inline.swap(true, Ordering::Relaxed) {
            tracing::warn!(
                target: "navree::mail",
                "no job dispatcher is registered (add QueueModule): queued mail is sent immediately"
            );
        }
    }
}

impl fmt::Debug for Mailer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Mailer")
            .field("from", &self.from)
            .field("queue", &self.queue)
            .field("transports", &self.named.keys().collect::<Vec<_>>())
            .field("delivery", &self.delivery.is_some())
            .finish_non_exhaustive()
    }
}

/// A [`Mailer`] bound to a tenant ([`Mailer::for_scope`]) or a named
/// transport ([`Mailer::via`]); combine both with
/// `mailer.for_scope(&scope).via("simulation")`.
#[derive(Debug)]
pub struct ScopedMailer<'a> {
    mailer: &'a Mailer,
    options: SendOptions,
}

impl ScopedMailer<'_> {
    /// Sends through the transport named `name` instead.
    #[must_use]
    pub fn via(mut self, name: impl Into<String>) -> Self {
        self.options.via = Some(name.into());
        self
    }

    /// Sends for `scope` instead.
    #[must_use]
    pub fn for_scope(mut self, scope: &MailScope) -> Self {
        self.options.scope = Some(scope.clone());
        self
    }

    /// Renders `mailable` and sends it now.
    pub async fn send(&self, mailable: &(impl Mailable + ?Sized)) -> Result<(), MailError> {
        self.deliver(mailable).await.map(drop)
    }

    /// Renders `mailable`, sends it now and reports what happened.
    pub async fn deliver(
        &self,
        mailable: &(impl Mailable + ?Sized),
    ) -> Result<SendReport, MailError> {
        let mut options = self.options.clone();
        options.template = mailable.template().map(str::to_owned);
        self.mailer
            .deliver_with(mailable.build(&self.mailer.views)?, options)
            .await
    }

    /// Renders `mailable` now and queues it (see [`Mailer::queue`]); the
    /// job keeps the scope and the transport name.
    pub async fn queue(&self, mailable: &(impl Mailable + ?Sized)) -> Result<(), MailError> {
        let mut options = self.options.clone();
        options.template = mailable.template().map(str::to_owned);
        self.mailer
            .queue_with(mailable.build(&self.mailer.views)?, options)
            .await
    }
}
