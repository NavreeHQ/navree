//! The live hub's publishing side: [`Topic`]s, who hears them ([`Audience`]),
//! and [`Live`], which tells the pages that listen.
//!
//! A topic is a name a page listens for (`orders`, `conversation.42`). The
//! publisher says who may hear it: everyone who can open the page, one user,
//! or the members of one workspace. A page names only the topic; the stream
//! adds the reader's own user and workspace, so a page can never ask for
//! another user's or another workspace's news.
//!
//! Each topic travels on a realtime channel of its own, under the reserved
//! `navree-live.` prefix, so a broadcast from anywhere (this process, a queue
//! worker through the relay, another `serve` process through the backplane)
//! reaches it like any other channel.

use std::fmt;
use std::sync::Arc;

use futures_util::FutureExt as _;
use navree_contracts::Broadcaster;
use navree_db::Uow;
use navree_kernel::BoxError;
use serde_json::Value;
use uuid::Uuid;

use crate::RealtimeError;

/// The prefix of the channels topics travel on. App channel patterns may not
/// start with it, and a stream may not name such a channel directly: only
/// through `topics`, which scopes them to the reader.
pub(crate) const RESERVED: &str = "navree-live.";

/// The event a [`Live::nudge`] broadcasts. A page never reads it.
pub const NUDGE_EVENT: &str = "changed";

/// The longest topic name.
pub const MAX_TOPIC_NAME: usize = 64;

/// Whether `name` can be a topic: 1 to 64 letters, digits, `_`, `.` and `-`.
/// These are the characters a channel name and a page's event name share,
/// so a topic is both.
pub fn is_topic_name(name: &str) -> bool {
    (1..=MAX_TOPIC_NAME).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
}

/// Who hears a [`Topic`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Audience {
    /// Everyone who can open the page, guests included.
    Public,
    /// One signed-in user, in every workspace.
    User(Uuid),
    /// The members of one workspace (as the app's workspace resolver and
    /// membership rules decide for each stream).
    Workspace(Uuid),
}

/// A topic and who hears it: what [`Live::nudge`] publishes.
///
/// ```
/// use navree_realtime::Topic;
/// use uuid::Uuid;
///
/// let workspace = Uuid::now_v7();
/// let topic = Topic::workspace(workspace, "deals");
/// assert_eq!(topic.name(), "deals");
/// assert_eq!(
///     topic.channel(),
///     format!("private-navree-live.workspace.{workspace}.deals")
/// );
/// assert_eq!(Topic::public("orders").channel(), "navree-live.orders");
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Topic {
    audience: Audience,
    name: String,
}

impl Topic {
    /// `name`, heard by everyone who can open the page.
    pub fn public(name: impl Into<String>) -> Topic {
        Topic {
            audience: Audience::Public,
            name: name.into(),
        }
    }

    /// `name`, heard only by user `user`.
    pub fn user(user: Uuid, name: impl Into<String>) -> Topic {
        Topic {
            audience: Audience::User(user),
            name: name.into(),
        }
    }

    /// `name`, heard only by the members of workspace `workspace`.
    pub fn workspace(workspace: Uuid, name: impl Into<String>) -> Topic {
        Topic {
            audience: Audience::Workspace(workspace),
            name: name.into(),
        }
    }

    /// The name a page listens for.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Who hears it.
    pub fn audience(&self) -> Audience {
        self.audience
    }

    /// The realtime channel it travels on: `navree-live.<name>` (public),
    /// `private-navree-live.user.<id>.<name>` or
    /// `private-navree-live.workspace.<id>.<name>`.
    pub fn channel(&self) -> String {
        channel_of(self.audience, &self.name)
    }

    /// An error unless the name is a topic name ([`is_topic_name`]).
    fn check(&self) -> Result<(), RealtimeError> {
        if is_topic_name(&self.name) {
            Ok(())
        } else {
            Err(RealtimeError::InvalidTopic(self.name.clone()))
        }
    }
}

impl fmt::Display for Topic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.channel())
    }
}

/// The channel `name` travels on for `audience`.
pub(crate) fn channel_of(audience: Audience, name: &str) -> String {
    match audience {
        Audience::Public => format!("{RESERVED}{name}"),
        Audience::User(user) => format!("private-{RESERVED}user.{user}.{name}"),
        Audience::Workspace(workspace) => {
            format!("private-{RESERVED}workspace.{workspace}.{name}")
        }
    }
}

/// The topic a channel of the hub carries, if it is one: the name a page
/// listens for.
#[cfg(test)]
pub(crate) fn topic_of(channel: &str) -> Option<&str> {
    if let Some(name) = channel.strip_prefix(RESERVED) {
        return Some(name);
    }
    let scoped = channel.strip_prefix("private-")?.strip_prefix(RESERVED)?;
    let rest = scoped
        .strip_prefix("user.")
        .or_else(|| scoped.strip_prefix("workspace."))?;
    // `<uuid>.<name>`: a UUID has no dot.
    rest.split_once('.').map(|(_, name)| name)
}

/// Whether `channel`, with or without its `private-` or `presence-` prefix,
/// is under the hub's reserved prefix.
pub(crate) fn is_reserved(channel: &str) -> bool {
    let bare = channel
        .strip_prefix("private-")
        .or_else(|| channel.strip_prefix("presence-"))
        .unwrap_or(channel);
    bare.starts_with(RESERVED)
}

/// Tells the pages that listen to a topic that it changed: the live hub's
/// publisher. `RealtimeModule` provides it (`Arc<Live>`), in every process of
/// the app: in `serve` it reaches this process's streams (and the others'
/// through the backplane); in `queue:work`, `schedule:work` and commands it
/// goes through the relay or the backplane, like every broadcast.
///
/// A nudge carries no data. A page that hears it fetches what it shows again,
/// or only what is new since its cursor, so what a page shows always comes from
/// the server under the reader's own access.
///
/// ```no_run
/// use navree_realtime::{Live, Topic};
/// use uuid::Uuid;
///
/// # async fn example(live: &Live, workspace: Uuid) -> Result<(), navree_kernel::BoxError> {
/// // In a handler: after the unit of work commits, so the page that asks
/// // again sees the change; never after a rollback.
/// live.nudge([Topic::workspace(workspace, "deals")]).await?;
/// # Ok(()) }
/// ```
#[derive(Clone)]
pub struct Live {
    broadcaster: Arc<dyn Broadcaster>,
}

impl Live {
    /// A publisher that broadcasts through `broadcaster` (the app's
    /// `dyn Broadcaster`; a recording one in tests).
    pub fn new(broadcaster: Arc<dyn Broadcaster>) -> Live {
        Live { broadcaster }
    }

    /// Tells every page listening to `topics` that they changed.
    ///
    /// Inside a unit of work's scope (a request handler, a job), the nudge
    /// waits for the commit and is dropped on a rollback: a page told sooner
    /// would ask before the change can be read, miss it, and have no reason
    /// to ask again. Outside one it goes now. Either way it goes out once,
    /// however many pages listen.
    ///
    /// An invalid topic name is an error, and nothing is sent. A failure to
    /// send after a commit is logged (the change is committed; the next
    /// nudge, or a page's own catch-up, brings the page up to date).
    pub async fn nudge<I>(&self, topics: I) -> Result<(), BoxError>
    where
        I: IntoIterator<Item = Topic>,
    {
        let topics: Vec<Topic> = topics.into_iter().collect();
        for topic in &topics {
            topic.check()?;
        }
        if topics.is_empty() {
            return Ok(());
        }
        let mut channels: Vec<String> = topics.iter().map(Topic::channel).collect();
        channels.sort();
        channels.dedup();
        match Uow::current() {
            Some(uow) => {
                let broadcaster = self.broadcaster.clone();
                uow.after_commit(move || {
                    async move {
                        if let Err(error) = send(&*broadcaster, channels).await {
                            tracing::warn!(
                                target: "navree::realtime",
                                %error,
                                "a live nudge could not be sent after the commit"
                            );
                        }
                    }
                    .boxed()
                });
                Ok(())
            }
            None => send(&*self.broadcaster, channels).await,
        }
    }
}

async fn send(broadcaster: &dyn Broadcaster, channels: Vec<String>) -> Result<(), BoxError> {
    broadcaster
        .broadcast(
            channels,
            NUDGE_EVENT.to_owned(),
            Value::Object(serde_json::Map::new()),
        )
        .await
}

impl fmt::Debug for Live {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Live").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topic_names_are_what_a_channel_and_an_event_name_share() {
        for good in ["orders", "conversation.42", "a_b-c.d", &"x".repeat(64)] {
            assert!(is_topic_name(good), "{good}");
        }
        for bad in [
            "",
            "has space",
            "ui:live",
            "a=b",
            "a@b",
            "a,b",
            "a;b",
            "é",
            &"x".repeat(65),
        ] {
            assert!(!is_topic_name(bad), "{bad}");
        }
    }

    #[test]
    fn channels_carry_the_audience_and_give_back_the_topic() {
        let user = Uuid::now_v7();
        let workspace = Uuid::now_v7();
        for topic in [
            Topic::public("orders"),
            Topic::user(user, "inbox.unread"),
            Topic::workspace(workspace, "conversation.42"),
        ] {
            let channel = topic.channel();
            assert!(crate::channel::is_valid_name(&channel), "{channel}");
            assert!(is_reserved(&channel), "{channel}");
            assert_eq!(topic_of(&channel), Some(topic.name()), "{channel}");
        }
        assert_eq!(
            crate::channel::ChannelKind::of(&Topic::public("x").channel()),
            crate::channel::ChannelKind::Public
        );
        assert_eq!(
            crate::channel::ChannelKind::of(&Topic::user(user, "x").channel()),
            crate::channel::ChannelKind::Private
        );
        assert_eq!(topic_of("private-crm.deals"), None);
        assert_eq!(topic_of("crm.deals"), None);
        assert!(is_reserved("presence-navree-live.room"));
        assert!(!is_reserved("private-crm.deals"));
    }

    #[test]
    fn the_longest_topic_still_makes_a_valid_channel() {
        // A UUID and a 64-character name fit Pusher's 200.
        let topic = Topic::workspace(Uuid::now_v7(), "x".repeat(MAX_TOPIC_NAME));
        assert!(crate::channel::is_valid_name(&topic.channel()));
    }
}
