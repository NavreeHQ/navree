//! [`SendMailJob`]: the queued form of a rendered message.

use navree_contracts::{BoxError, Job, JobContext};
use serde::{Deserialize, Serialize};

use crate::mailer::SendOptions;
use crate::{MailScope, Mailer, Message};

/// Job `mail.send`: sends an already rendered [`Message`] through the app's
/// [`Mailer`]. [`Mailer::queue`] dispatches it, and
/// [`MailModule`](crate::MailModule) registers it with the queue.
///
/// The payload is the whole message, bodies included, so it is stored with
/// the queued job until a worker runs it. A message queued with
/// [`Mailer::for_scope`] or [`Mailer::via`] keeps its scope and transport
/// name; suppressions and tenant transports are checked when the job runs.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SendMailJob {
    /// The rendered message, sender filled in.
    pub message: Message,
    /// The tenant it is sent for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<MailScope>,
    /// The named transport it goes through.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via: Option<String>,
    /// The template it was rendered from, for the delivery log.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
}

impl SendMailJob {
    /// A job sending `message` with the configured transport.
    pub fn new(message: Message) -> Self {
        Self {
            message,
            scope: None,
            via: None,
            template: None,
        }
    }
}

impl Job for SendMailJob {
    const NAME: &'static str = "mail.send";

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        let mailer = ctx.app.get::<Mailer>().await?;
        let options = SendOptions {
            scope: self.scope,
            via: self.via,
            template: self.template,
            releasing: false,
            rendered: true,
        };
        mailer.deliver_with(self.message, options).await?;
        Ok(())
    }
}
