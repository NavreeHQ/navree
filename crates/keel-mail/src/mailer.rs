//! [`Mailer`]: renders mailables and hands messages to the transport.

use std::any::Any;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use keel_contracts::{Dispatcher, JobEnvelope};
use keel_kernel::WeakAppContext;
use keel_view::Views;

use crate::{ArrayTransport, MailError, MailTransport, Mailable, Message, SendMailJob};

/// Sends mail: renders [`Mailable`]s with the app's [`Views`], fills in the
/// default sender, validates the message and hands it to the
/// [`MailTransport`].
///
/// Provided and exported by the global [`MailModule`](crate::MailModule):
/// inject `Arc<Mailer>`.
///
/// - [`send`](Mailer::send) renders and delivers now;
/// - [`queue`](Mailer::queue) renders and validates now, then dispatches job
///   `mail.send` ([`SendMailJob`]) through the app's `dyn Dispatcher`, so a
///   queue worker delivers it (and retries it). Without a dispatcher (no
///   queue module), it delivers now.
pub struct Mailer {
    transport: Arc<dyn MailTransport>,
    views: Arc<Views>,
    from: String,
    /// The queue for queued mail; `None` keeps the job's default.
    queue: Option<String>,
    /// The app, to find the job dispatcher (weak: the app owns the mailer).
    app: Option<WeakAppContext>,
    /// Whether the "no dispatcher, sending now" warning was logged.
    warned_inline: AtomicBool,
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
            views,
            from: from.into(),
            queue: None,
            app: None,
            warned_inline: AtomicBool::new(false),
        }
    }

    /// Queues mail on `queue` and finds the dispatcher in `app`.
    pub(crate) fn queued_through(mut self, app: WeakAppContext, queue: Option<String>) -> Self {
        self.app = Some(app);
        self.queue = queue.filter(|queue| !queue.is_empty());
        self
    }

    /// Renders `mailable` and sends it now.
    pub async fn send(&self, mailable: &(impl Mailable + ?Sized)) -> Result<(), MailError> {
        let message = mailable.build(&self.views)?;
        self.send_message(message).await
    }

    /// Sends `message` now, after filling in the default sender and
    /// validating it (see [`Message::validate`]).
    pub async fn send_message(&self, message: Message) -> Result<(), MailError> {
        let message = self.prepare(message)?;
        self.transport.send(&message).await
    }

    /// Renders and validates `mailable` now, then dispatches job `mail.send`
    /// carrying the rendered message (on `mail.queue` when configured).
    /// Rendering or validation errors are returned here, before anything is
    /// queued. Without a `dyn Dispatcher` in the app, the message is sent
    /// now (a warning is logged once).
    pub async fn queue(&self, mailable: &(impl Mailable + ?Sized)) -> Result<(), MailError> {
        let message = self.prepare(mailable.build(&self.views)?)?;
        let Some(dispatcher) = self.dispatcher().await else {
            self.warn_inline();
            return self.transport.send(&message).await;
        };
        let mut envelope = JobEnvelope::of(&SendMailJob { message })
            .map_err(|source| MailError::Queue { source })?;
        if let Some(queue) = &self.queue {
            envelope.options.queue.clone_from(queue);
        }
        let job_id = dispatcher
            .dispatch_envelope(envelope)
            .await
            .map_err(|source| MailError::Queue { source })?;
        tracing::debug!(target: "keel::mail", job_id, "mail queued");
        Ok(())
    }

    /// The transport messages are handed to.
    pub fn transport(&self) -> &Arc<dyn MailTransport> {
        &self.transport
    }

    /// The [`ArrayTransport`] recording this mailer's messages, when that is
    /// the transport (`transport = "array"`, or an `ArrayTransport` given to
    /// `override_value::<dyn MailTransport>`): for assertions in tests.
    pub fn array_transport(&self) -> Option<&ArrayTransport> {
        (self.transport.as_ref() as &dyn Any).downcast_ref::<ArrayTransport>()
    }

    /// The message with the default sender filled in, validated.
    fn prepare(&self, mut message: Message) -> Result<Message, MailError> {
        if message.from.is_none() && !self.from.is_empty() {
            message.from = Some(self.from.clone());
        }
        message.validate()?;
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
                target: "keel::mail",
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
            .finish_non_exhaustive()
    }
}
