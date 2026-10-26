//! [`Message`]: a rendered email.

use lettre::message::Mailbox;
use serde::{Deserialize, Serialize};

use crate::MailError;

/// A rendered email, ready for a [`MailTransport`](crate::MailTransport).
///
/// Addresses are `"ada@example.com"` or `"Ada Lovelace <ada@example.com>"`.
/// Build one with the chainable setters, which share the field names:
///
/// ```
/// use navis_mail::Message;
///
/// let message = Message::new()
///     .to("Ada <ada@example.com>")
///     .cc("team@example.com")
///     .subject("Your invoice")
///     .html("<p>Thanks!</p>")
///     .text("Thanks!");
/// assert_eq!(message.to, ["Ada <ada@example.com>"]);
/// assert_eq!(message.from, None); // the Mailer fills in its default sender
/// ```
///
/// A message is serializable, so it can travel inside a queued job.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Message {
    /// The sender; `None` lets the [`Mailer`](crate::Mailer) use its default
    /// (`mail.from`).
    pub from: Option<String>,
    /// Primary recipients.
    pub to: Vec<String>,
    /// Carbon-copy recipients.
    pub cc: Vec<String>,
    /// Blind carbon-copy recipients (never shown to the other recipients).
    pub bcc: Vec<String>,
    /// Where replies go, if not to the sender.
    pub reply_to: Option<String>,
    /// The subject line.
    pub subject: String,
    /// The HTML body.
    pub html: Option<String>,
    /// The plain-text body.
    pub text: Option<String>,
}

impl Message {
    /// An empty message.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the sender.
    pub fn from(mut self, address: impl Into<String>) -> Self {
        self.from = Some(address.into());
        self
    }

    /// Adds a primary recipient.
    pub fn to(mut self, address: impl Into<String>) -> Self {
        self.to.push(address.into());
        self
    }

    /// Adds a carbon-copy recipient.
    pub fn cc(mut self, address: impl Into<String>) -> Self {
        self.cc.push(address.into());
        self
    }

    /// Adds a blind carbon-copy recipient.
    pub fn bcc(mut self, address: impl Into<String>) -> Self {
        self.bcc.push(address.into());
        self
    }

    /// Sets the reply-to address.
    pub fn reply_to(mut self, address: impl Into<String>) -> Self {
        self.reply_to = Some(address.into());
        self
    }

    /// Sets the subject line.
    pub fn subject(mut self, subject: impl Into<String>) -> Self {
        self.subject = subject.into();
        self
    }

    /// Sets the HTML body.
    pub fn html(mut self, body: impl Into<String>) -> Self {
        self.html = Some(body.into());
        self
    }

    /// Sets the plain-text body.
    pub fn text(mut self, body: impl Into<String>) -> Self {
        self.text = Some(body.into());
        self
    }

    /// Checks that the message can be delivered: it has a sender and at
    /// least one recipient, every address parses, and the subject holds no
    /// line break. The [`Mailer`](crate::Mailer) runs this (after filling in
    /// the default sender) before handing the message to a transport.
    pub fn validate(&self) -> Result<(), MailError> {
        match &self.from {
            Some(from) => drop(mailbox("from", from)?),
            None => return Err(MailError::MissingSender),
        }
        if self.to.is_empty() && self.cc.is_empty() && self.bcc.is_empty() {
            return Err(MailError::NoRecipients);
        }
        for (field, addresses) in [("to", &self.to), ("cc", &self.cc), ("bcc", &self.bcc)] {
            for address in addresses {
                mailbox(field, address)?;
            }
        }
        if let Some(reply_to) = &self.reply_to {
            mailbox("reply_to", reply_to)?;
        }
        if self.subject.contains(['\r', '\n']) {
            return Err(MailError::InvalidHeader { field: "subject" });
        }
        Ok(())
    }
}

/// Parses `address`, the value of field `field`, as a mailbox.
pub(crate) fn mailbox(field: &'static str, address: &str) -> Result<Mailbox, MailError> {
    address
        .parse::<Mailbox>()
        .map_err(|error| MailError::InvalidAddress {
            field,
            address: address.to_owned(),
            reason: error.to_string(),
        })
}
