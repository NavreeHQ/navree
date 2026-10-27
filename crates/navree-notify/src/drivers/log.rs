//! [`LogSms`] and [`LogWhatsApp`]: drivers for development that write each
//! message to the log instead of sending it.

use std::sync::atomic::{AtomicU64, Ordering};

use navree_contracts::BoxFuture;

use super::{ChannelError, SentMessage, SmsDriver, WhatsAppDriver};
use crate::{SmsMessage, WhatsAppMessage};

/// Writes each text message to the log (`INFO` at target `navree::notify`
/// with the recipient and sender, the text at `DEBUG` under
/// `navree::notify::body`) instead of sending it. Message ids are
/// `log-1`, `log-2`, ...
#[derive(Debug, Default)]
pub struct LogSms {
    from: Option<String>,
    sent: AtomicU64,
}

impl LogSms {
    /// A driver that names `from` as the default sender.
    pub fn new(from: Option<String>) -> Self {
        Self {
            from,
            sent: AtomicU64::new(0),
        }
    }
}

impl SmsDriver for LogSms {
    fn send<'a>(
        &'a self,
        to: &'a str,
        message: &'a SmsMessage,
    ) -> BoxFuture<'a, Result<SentMessage, ChannelError>> {
        let id = self.sent.fetch_add(1, Ordering::Relaxed) + 1;
        let from = message.from.as_deref().or(self.from.as_deref()).unwrap_or_default();
        tracing::info!(target: "navree::notify", to, from, "sms (log driver)");
        tracing::debug!(target: "navree::notify::body", to, body = %message.body, "sms body (log driver)");
        Box::pin(async move { Ok(SentMessage::new("log").id(format!("log-{id}"))) })
    }
}

/// Writes each WhatsApp message to the log instead of sending it (see
/// [`LogSms`]).
#[derive(Debug, Default)]
pub struct LogWhatsApp {
    sent: AtomicU64,
}

impl LogWhatsApp {
    /// A driver.
    pub fn new() -> Self {
        Self::default()
    }
}

impl WhatsAppDriver for LogWhatsApp {
    fn send<'a>(
        &'a self,
        to: &'a str,
        message: &'a WhatsAppMessage,
    ) -> BoxFuture<'a, Result<SentMessage, ChannelError>> {
        let id = self.sent.fetch_add(1, Ordering::Relaxed) + 1;
        let template = message.template.as_ref().map(|t| t.name.as_str()).unwrap_or_default();
        tracing::info!(target: "navree::notify", to, template, "whatsapp (log driver)");
        tracing::debug!(
            target: "navree::notify::body",
            to,
            body = message.body.as_deref().unwrap_or_default(),
            "whatsapp body (log driver)"
        );
        Box::pin(async move { Ok(SentMessage::new("log").id(format!("log-{id}"))) })
    }
}
