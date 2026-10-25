//! The [`MailTransport`] trait.

use std::any::Any;
use std::fmt;
use std::sync::Arc;

use keel_kernel::BoxFuture;

use crate::{MailError, Message};

/// Delivers messages: SMTP, the log, an in-memory array, a failover chain,
/// or your own implementation.
///
/// [`MailModule`](crate::MailModule) provides and exports
/// `Arc<dyn MailTransport>` built from [`TransportConfig`](crate::TransportConfig);
/// replace it in tests with `AppBuilder::override_value::<dyn MailTransport>(..)`.
///
/// Transports receive messages the [`Mailer`](crate::Mailer) has already
/// validated, with the sender filled in.
pub trait MailTransport: Any + Send + Sync {
    /// Delivers `message`.
    fn send<'a>(&'a self, message: &'a Message) -> BoxFuture<'a, Result<(), MailError>>;
}

/// Prints the type name only (transports may hold credentials).
impl fmt::Debug for dyn MailTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("dyn MailTransport")
    }
}

/// Writes messages to the log instead of delivering them (Laravel's `log`
/// mailer), for development.
///
/// The envelope (sender, recipients, subject) is logged at `INFO` with
/// target `keel::mail`; the bodies only at `DEBUG` with target
/// `keel::mail::body`, because they may hold password-reset links or other
/// tokens. Enable `keel::mail::body=debug` to read them.
#[derive(Clone, Copy, Debug, Default)]
pub struct LogTransport;

impl MailTransport for LogTransport {
    fn send<'a>(&'a self, message: &'a Message) -> BoxFuture<'a, Result<(), MailError>> {
        tracing::info!(
            target: "keel::mail",
            from = message.from.as_deref().unwrap_or_default(),
            to = %message.to.join(", "),
            cc = %message.cc.join(", "),
            bcc = %message.bcc.join(", "),
            subject = %message.subject,
            "mail (log transport)"
        );
        tracing::debug!(
            target: "keel::mail::body",
            subject = %message.subject,
            text = message.text.as_deref().unwrap_or_default(),
            html = message.html.as_deref().unwrap_or_default(),
            "mail body (log transport)"
        );
        Box::pin(async { Ok(()) })
    }
}

/// Tries its transports in order until one delivers the message (Laravel's
/// `failover` mailer). Each failure is logged as a warning; if every
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
}

impl MailTransport for FailoverTransport {
    fn send<'a>(&'a self, message: &'a Message) -> BoxFuture<'a, Result<(), MailError>> {
        Box::pin(async move {
            let mut errors = Vec::new();
            for (index, transport) in self.transports.iter().enumerate() {
                match transport.send(message).await {
                    Ok(()) => return Ok(()),
                    Err(error) => {
                        tracing::warn!(
                            target: "keel::mail",
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
}

impl fmt::Debug for FailoverTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FailoverTransport")
            .field("transports", &self.transports.len())
            .finish()
    }
}
