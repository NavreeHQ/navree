//! [`ChannelDriver`]: a delivery channel a module adds.

use navree_contracts::BoxFuture;
use serde_json::Value;

use crate::{ChannelError, SentMessage};

/// A delivery channel a module adds: `m.notification_channel::<C>()`
/// registers it (it is built by the container, so it can inject what it
/// needs), and a notification lists it in [`via`](crate::Notification::via)
/// as `Channel::named(C::NAME)`.
///
/// For each notification that lists it, the notifier asks the
/// notification for the message
/// ([`to_channel`](crate::Notification::to_channel)) and the notifiable
/// for the address ([`route_channel`](crate::Notifiable::route_channel)),
/// then calls [`send`](ChannelDriver::send); both must give one, or the
/// channel is skipped for that recipient. Each attempt is recorded in
/// `notification_deliveries`: an address that is an `http` or `https` URL
/// (a webhook, whose URL may be a secret) as `webhook:{host}`, any other
/// as given. Errors carry no message bodies, tokens or webhook URLs, as
/// [`ChannelError`] says.
///
/// ```
/// use navree_contracts::BoxFuture;
/// use navree_kernel::{Injectable, KernelError, Resolver};
/// use navree_notify::{ChannelDriver, ChannelError, SentMessage};
/// use serde_json::Value;
///
/// /// Posts notifications to a team's Doot chat room.
/// struct DootChannel;
///
/// impl ChannelDriver for DootChannel {
///     const NAME: &'static str = "doot";
///
///     fn send<'a>(
///         &'a self,
///         to: &'a str,
///         message: &'a Value,
///     ) -> BoxFuture<'a, Result<SentMessage, ChannelError>> {
///         Box::pin(async move {
///             // Post `message` to the room `to` here.
///             let _ = (to, message);
///             Ok(SentMessage::new("doot"))
///         })
///     }
/// }
///
/// // Built by the container: `m.notification_channel::<DootChannel>()`.
/// impl Injectable for DootChannel {
///     fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
///         Box::pin(async { Ok(DootChannel) })
///     }
/// }
/// ```
pub trait ChannelDriver: Send + Sync + 'static {
    /// The channel's name: a lower-case letter followed by up to 63
    /// lower-case letters, digits or `_`, not a built-in channel's.
    /// `Channel::named(NAME)` selects it, and its settings are
    /// `notify.{NAME}`.
    const NAME: &'static str;

    /// Sends `message` (what [`to_channel`](crate::Notification::to_channel)
    /// gave) to `to` (what [`route_channel`](crate::Notifiable::route_channel)
    /// gave). Do the whole operation and do not retry: a queued
    /// notification is retried by its job.
    fn send<'a>(
        &'a self,
        to: &'a str,
        message: &'a Value,
    ) -> BoxFuture<'a, Result<SentMessage, ChannelError>>;
}
