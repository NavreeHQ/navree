//! The [`MailTransport`] trait.

use std::any::Any;
use std::fmt;
use std::sync::Arc;

use navree_kernel::BoxFuture;

use crate::{MailError, Message};

/// Delivers messages: SMTP, an email API (SES, Mailgun, Postmark, Resend),
/// the log, an in-memory array, a failover chain, or your own
/// implementation.
///
/// [`MailModule`](crate::MailModule) provides and exports
/// `Arc<dyn MailTransport>` built from [`TransportConfig`](crate::TransportConfig);
/// replace it in tests with `AppBuilder::override_value::<dyn MailTransport>(..)`.
///
/// Transports receive messages the [`Mailer`](crate::Mailer) has already
/// validated, with the sender filled in.
///
/// Only [`send`](MailTransport::send) is required. A transport whose
/// provider returns a message id overrides [`deliver`](MailTransport::deliver)
/// too, so the delivery log can match the provider's events to the message;
/// one that can test its credentials overrides [`check`](MailTransport::check).
pub trait MailTransport: Any + Send + Sync {
    /// Delivers `message`.
    fn send<'a>(&'a self, message: &'a Message) -> BoxFuture<'a, Result<(), MailError>>;

    /// Delivers `message` and says how: the transport that took it and the
    /// provider's message id, if it has one. The default calls
    /// [`send`](MailTransport::send) and reports no id.
    fn deliver<'a>(&'a self, message: &'a Message) -> BoxFuture<'a, Result<Sent, MailError>> {
        Box::pin(async move {
            self.send(message).await?;
            Ok(Sent::new(self.name()))
        })
    }

    /// The transport's name in reports, logs and the delivery log (`smtp`,
    /// `ses`, ...); `custom` by default.
    fn name(&self) -> &'static str {
        "custom"
    }

    /// Checks that the transport can deliver, without sending anything:
    /// SMTP connects and authenticates, an API transport makes a read-only
    /// call with its credentials. The default does nothing. Used by
    /// [`TransportResolver::usable`](crate::TransportResolver::usable)
    /// before a tenant saves a transport.
    fn check(&self) -> BoxFuture<'_, Result<(), MailError>> {
        Box::pin(async { Ok(()) })
    }
}

/// Prints the type name only (transports may hold credentials).
impl fmt::Debug for dyn MailTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("dyn MailTransport")
    }
}

/// What [`MailTransport::deliver`] reports.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Sent {
    /// The transport that delivered the message (for a failover, the one
    /// that succeeded).
    pub transport: &'static str,
    /// The provider's message id (`MessageId` for SES, `id` for Mailgun and
    /// Resend, `MessageID` for Postmark), which its events refer to.
    pub provider_id: Option<String>,
}

impl Sent {
    /// Delivered by `transport`, without a provider id.
    pub fn new(transport: &'static str) -> Self {
        Self {
            transport,
            provider_id: None,
        }
    }

    /// With the provider's message id.
    #[must_use]
    pub fn with_provider_id(mut self, id: impl Into<String>) -> Self {
        self.provider_id = Some(id.into());
        self
    }
}

/// Writes messages to the log instead of delivering them (the `log`
/// transport), for development.
///
/// The envelope (sender, recipients, subject) is logged at `INFO` with
/// target `navree::mail`; the bodies only at `DEBUG` with target
/// `navree::mail::body`, because they may hold password-reset links or other
/// tokens. Enable `navree::mail::body=debug` to read them.
#[derive(Clone, Copy, Debug, Default)]
pub struct LogTransport;

impl MailTransport for LogTransport {
    fn send<'a>(&'a self, message: &'a Message) -> BoxFuture<'a, Result<(), MailError>> {
        tracing::info!(
            target: "navree::mail",
            from = message.from.as_deref().unwrap_or_default(),
            to = %message.to.join(", "),
            cc = %message.cc.join(", "),
            bcc = %message.bcc.join(", "),
            subject = %message.subject,
            attachments = %message
                .attachments
                .iter()
                .map(|file| format!("{} ({}, {} bytes)", file.filename, file.content_type, file.content.len()))
                .collect::<Vec<_>>()
                .join(", "),
            "mail (log transport)"
        );
        tracing::debug!(
            target: "navree::mail::body",
            subject = %message.subject,
            text = message.text.as_deref().unwrap_or_default(),
            html = message.html.as_deref().unwrap_or_default(),
            "mail body (log transport)"
        );
        Box::pin(async { Ok(()) })
    }

    fn name(&self) -> &'static str {
        "log"
    }
}

/// Tries its transports in order until one delivers the message (the
/// `failover` transport). Each failure is logged as a warning; if every
/// transport fails, the error is [`MailError::AllTransportsFailed`] with
/// each transport's error.
#[derive(Clone)]
pub struct FailoverTransport {
    transports: Vec<Arc<dyn MailTransport>>,
}

impl FailoverTransport {
    /// A failover over `transports`, tried first to last.
    pub fn new(transports: Vec<Arc<dyn MailTransport>>) -> Self {
        Self { transports }
    }

    /// The transports, in the order they are tried.
    pub fn transports(&self) -> &[Arc<dyn MailTransport>] {
        &self.transports
    }
}

impl MailTransport for FailoverTransport {
    fn send<'a>(&'a self, message: &'a Message) -> BoxFuture<'a, Result<(), MailError>> {
        Box::pin(async move { self.deliver(message).await.map(drop) })
    }

    fn deliver<'a>(&'a self, message: &'a Message) -> BoxFuture<'a, Result<Sent, MailError>> {
        Box::pin(async move {
            let mut errors = Vec::new();
            for (index, transport) in self.transports.iter().enumerate() {
                match transport.deliver(message).await {
                    Ok(sent) => return Ok(sent),
                    Err(error) => {
                        tracing::warn!(
                            target: "navree::mail",
                            transport = index + 1,
                            of = self.transports.len(),
                            error = %error,
                            "mail transport failed, trying the next one"
                        );
                        errors.push(error);
                    }
                }
            }
            Err(MailError::AllTransportsFailed { errors })
        })
    }

    fn name(&self) -> &'static str {
        "failover"
    }

    /// Usable when at least one of its transports is.
    fn check(&self) -> BoxFuture<'_, Result<(), MailError>> {
        Box::pin(async move {
            let mut errors = Vec::new();
            for transport in &self.transports {
                match transport.check().await {
                    Ok(()) => return Ok(()),
                    Err(error) => errors.push(error),
                }
            }
            Err(MailError::AllTransportsFailed { errors })
        })
    }
}

impl fmt::Debug for FailoverTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FailoverTransport")
            .field("transports", &self.transports.len())
            .finish()
    }
}
