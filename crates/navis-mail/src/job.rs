//! [`SendMailJob`]: the queued form of a rendered message.

use navis_contracts::{BoxError, Job, JobContext};
use serde::{Deserialize, Serialize};

use crate::{Mailer, Message};

/// Job `mail.send`: sends an already rendered [`Message`] through the app's
/// [`Mailer`]. [`Mailer::queue`] dispatches it, and
/// [`MailModule`](crate::MailModule) registers it with the queue.
///
/// The payload is the whole message, bodies included, so it is stored with
/// the queued job until a worker runs it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SendMailJob {
    /// The rendered message, sender filled in.
    pub message: Message,
}

impl Job for SendMailJob {
    const NAME: &'static str = "mail.send";

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        let mailer = ctx.app.get::<Mailer>().await?;
        mailer.send_message(self.message).await?;
        Ok(())
    }
}
