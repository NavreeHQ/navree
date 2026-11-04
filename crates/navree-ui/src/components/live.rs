//! [`LiveStream`] and [`Live`]: where a live part of a page hears that what it
//! shows changed.

use crate::html::Href;
use crate::table::region::valid_event_name;

/// The path of the app's live hub: the stream route of `RealtimeModule`
/// (`GET /broadcasting/stream`).
pub const LIVE_STREAM_PATH: &str = "/broadcasting/stream";

/// The longest topic or channel name a stream takes.
const MAX_NAME: usize = 64;

/// Whether `name` can be a topic or a channel on the hub's stream: 1 to 64
/// letters, digits, `_`, `.` and `-` (what a channel name and an event name
/// share).
fn is_stream_name(name: &str) -> bool {
    (1..=MAX_NAME).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
}

/// A stream from the app's live hub (`RealtimeModule`): the topics and
/// channels one page listens to, on one connection.
///
/// A topic is scoped by the server: the page names `deals`, and hears the
/// nudges published for everyone, for the signed-in user and for the members
/// of the workspace the request is in. A channel (`private-crm.deals`) is
/// checked by the app's channel authorizer, as a WebSocket subscription is.
/// Build one `LiveStream` for the page and give each live part the names it
/// listens for with [`on`](Self::on): regions on the same stream share one
/// connection.
///
/// ```
/// use navree_ui::{LiveStream, Region, render};
///
/// let stream = LiveStream::topics(["orders", "stats"]);
/// assert_eq!(stream.href().as_str(), "/broadcasting/stream?topics=orders,stats");
/// let html = render(&Region::new("orders").src("/fragments/orders").live_on(stream.on("orders")).into());
/// assert!(html.contains(r#"data-ui-live-src="/broadcasting/stream?topics=orders,stats""#));
/// assert!(html.contains(r#"data-ui-live-event="orders""#));
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[must_use]
pub struct LiveStream {
    path: Option<String>,
    topics: Vec<String>,
    channels: Vec<String>,
}

/// Adds `name` to `list` when it is a stream name and not there yet.
fn add(list: &mut Vec<String>, name: String) {
    if is_stream_name(&name) && !list.contains(&name) {
        list.push(name);
    }
}

impl LiveStream {
    /// A stream of `topics`. A name that isn't 1 to 64 letters, digits, `_`,
    /// `.` and `-` is left out.
    pub fn topics<I, S>(topics: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::default().with_topics(topics)
    }

    /// A stream of `channels` (realtime channels, `private-…` ones checked by
    /// the app's channel authorizers).
    pub fn channels<I, S>(channels: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::default().with_channels(channels)
    }

    /// Adds topics.
    pub fn with_topics<I, S>(mut self, topics: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        for topic in topics {
            add(&mut self.topics, topic.into());
        }
        self
    }

    /// Adds channels.
    pub fn with_channels<I, S>(mut self, channels: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        for channel in channels {
            add(&mut self.channels, channel.into());
        }
        self
    }

    /// Serves the stream from `path` instead of [`LIVE_STREAM_PATH`] (an app
    /// that mounts the hub elsewhere).
    pub fn at(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// The stream's address: `/broadcasting/stream?topics=a,b&channels=c`.
    pub fn href(&self) -> Href {
        let mut url = self.path.as_deref().unwrap_or(LIVE_STREAM_PATH).to_owned();
        let mut separator = if url.contains('?') { '&' } else { '?' };
        for (key, names) in [("topics", &self.topics), ("channels", &self.channels)] {
            if names.is_empty() {
                continue;
            }
            url.push(separator);
            url.push_str(key);
            url.push('=');
            url.push_str(&names.join(","));
            separator = '&';
        }
        Href::from(url)
    }

    /// Listens on this stream for the nudges of `name`: one of its topics or
    /// channels. Add more with [`Live::event`].
    pub fn on(&self, name: impl Into<String>) -> Live {
        Live::new(self.href()).event(name)
    }
}

/// Where a live part of a page hears that it changed: a server-sent event
/// stream on this site, and the event names it listens for (the stream's
/// unnamed events when there are none). An event is a nudge: the part fetches
/// its own source again, and never reads what the event says.
///
/// Make one with [`LiveStream::on`] for the app's live hub, or from any
/// stream's address (`Live::new("/events")`, or just `"/events"` where a
/// `Live` is taken).
///
/// ```
/// use navree_ui::{Live, LiveStream};
///
/// let stream = LiveStream::topics(["conversation.42", "clock"]);
/// // The heading of a conversation: its changes, and the clock for its timers.
/// let heading = stream.on("conversation.42").event("clock");
/// assert_eq!(heading.events(), ["conversation.42", "clock"]);
/// assert!(Live::from("/events").events().is_empty());
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
#[must_use]
pub struct Live {
    src: Href,
    events: Vec<String>,
}

impl Live {
    /// The unnamed events of the stream at `src`.
    pub fn new(src: impl Into<Href>) -> Self {
        Self {
            src: src.into(),
            events: Vec::new(),
        }
    }

    /// Listens for events called `name` too (`event: name` on the wire): a
    /// name of letters, digits and `_ . : -`, at most 64 long. `open` and
    /// `error` are the connection's own; those and any other name are ignored.
    pub fn event(mut self, name: impl Into<String>) -> Self {
        let name = name.into();
        if valid_event_name(&name) && !self.events.contains(&name) {
            self.events.push(name);
        }
        self
    }

    /// The stream's address.
    pub fn src(&self) -> &Href {
        &self.src
    }

    /// The event names it listens for; empty for the unnamed ones.
    pub fn events(&self) -> &[String] {
        &self.events
    }

    /// `data-ui-live-event`: the names, separated by spaces.
    pub(crate) fn event_attr(&self) -> Option<String> {
        (!self.events.is_empty()).then(|| self.events.join(" "))
    }
}

impl From<&str> for Live {
    fn from(src: &str) -> Self {
        Live::new(src)
    }
}

impl From<String> for Live {
    fn from(src: String) -> Self {
        Live::new(src)
    }
}

impl From<&String> for Live {
    fn from(src: &String) -> Self {
        Live::new(src)
    }
}

impl From<Href> for Live {
    fn from(src: Href) -> Self {
        Live::new(src)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stream_names_its_topics_and_channels_once_and_drops_bad_ones() {
        // MOD-004
        let stream = LiveStream::topics(["orders", "orders", "has space", "ui:live", ""])
            .with_channels(["private-crm.deals", "private-a=b"])
            .with_topics(["stats"]);
        assert_eq!(
            stream.href().as_str(),
            "/broadcasting/stream?topics=orders,stats&channels=private-crm.deals"
        );
        assert_eq!(
            LiveStream::channels(["news"]).href().as_str(),
            "/broadcasting/stream?channels=news"
        );
        assert_eq!(
            LiveStream::topics(["a"]).at("/live?app=1").href().as_str(),
            "/live?app=1&topics=a"
        );
        assert_eq!(
            LiveStream::topics([&"x".repeat(65)]).href().as_str(),
            "/broadcasting/stream"
        );
    }

    #[test]
    fn a_live_part_listens_for_several_names() {
        // MOD-004: the inbox's heading hears its conversation and the clock.
        let stream = LiveStream::topics(["conversation.42", "clock"]);
        let live = stream.on("conversation.42").event("clock").event("clock");
        assert_eq!(live.events(), ["conversation.42", "clock"]);
        assert_eq!(live.event_attr().as_deref(), Some("conversation.42 clock"));
        assert_eq!(live.src(), &stream.href());
        // The connection's own names, and bad ones, are not heard.
        let live = Live::new("/events").event("open").event("error").event("a b");
        assert!(live.events().is_empty());
        assert_eq!(live.event_attr(), None);
    }
}
