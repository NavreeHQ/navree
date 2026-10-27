//! The built-in sources beyond the HMAC schemes of `verify`: senders that
//! sign with their own keys ([`AwsSns`], [`GooglePubSub`]), that sign the
//! URL ([`Twilio`]), that answer a handshake ([`MicrosoftGraph`],
//! [`Slack`]) or that sign the body's own fields ([`Mailgun`]), and
//! [`Postmark`]'s credentials.

mod aws_sns;
mod google_pubsub;
mod mailgun;
mod microsoft_graph;
mod postmark;
mod slack;
mod twilio;

pub use aws_sns::{AwsSns, SnsMessage};
pub use google_pubsub::{GooglePubSub, PubSubMessage, PubSubPush};
pub use mailgun::Mailgun;
pub use microsoft_graph::{GraphNotification, GraphNotifications, MicrosoftGraph};
pub use postmark::Postmark;
pub use slack::Slack;
pub use twilio::Twilio;

/// Runs `future` to completion on a fresh runtime (unit tests of
/// verifiers).
#[cfg(test)]
pub(crate) fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a test runtime")
        .block_on(future)
}
