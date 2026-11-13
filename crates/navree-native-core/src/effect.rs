//! [`Effect`]: everything the core asks a client to do, and [`Update`]: what
//! one event led to. The core does nothing itself.

use serde::{Deserialize, Serialize};

use crate::state::{Message, Notice, ScreenId};

/// One part of a multipart request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Part {
    /// A value.
    Text {
        /// The field's name.
        name: String,
        /// Its value.
        value: String,
    },
    /// A file. The client's transport puts in the bytes it keeps under `token`.
    File {
        /// The field's name.
        name: String,
        /// The client's name for the bytes.
        token: String,
        /// The file's name.
        filename: String,
        /// Its media type.
        #[serde(rename = "type")]
        media_type: String,
    },
}

/// What a request carries.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Body {
    /// `application/x-www-form-urlencoded`, already encoded: send `text` as it is.
    Form {
        /// The encoded pairs.
        text: String,
    },
    /// `multipart/form-data`: the client's transport writes the parts, with a
    /// boundary of its own, and sets the content type.
    Multipart {
        /// The parts, in order.
        parts: Vec<Part>,
    },
    /// `application/json`, already written: send `text` as it is. Empty for a
    /// post with no body. Only the framework's own routes take it.
    Json {
        /// The JSON.
        text: String,
    },
}

/// How a request is sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verb {
    /// A read.
    Get,
    /// A write.
    Post,
    /// A removal: the device's push token, when the person signs out.
    Delete,
}

impl From<navree_native::node::Method> for Verb {
    fn from(method: navree_native::node::Method) -> Self {
        match method {
            navree_native::node::Method::Get => Self::Get,
            navree_native::node::Method::Post => Self::Post,
        }
    }
}

/// A request to the app's origin. The client adds its cookies and nothing else.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    /// The number the answer comes back with.
    pub request: u64,
    /// `get`, `post` or `delete`.
    pub method: Verb,
    /// The whole address.
    pub url: String,
    /// The headers to send, each a name and a value.
    pub headers: Vec<(String, String)>,
    /// The body of a post.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<Body>,
    /// How long to wait for the answer before giving up, in milliseconds.
    pub timeout_ms: u32,
    /// Whether the session's `XSRF-TOKEN` cookie, decoded, also goes in the
    /// `x-xsrf-token` header, as a browser's runtime sends it with every post.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub xsrf: bool,
}

/// Something the core asks the client to do.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Effect {
    /// Send this and come back with a `response` event that carries its number.
    Fetch(Request),
    /// The answer to this request is no longer wanted. The client may drop it.
    Cancel {
        /// The request.
        request: u64,
    },
    /// Open this address outside the app: the browser, the mail app, the dialler.
    OpenUrl {
        /// The address.
        url: String,
        /// Whether the page asked for the system's browser inside the app.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        inside: bool,
    },
    /// Open the system's sign-in session at this address, and come back with
    /// a `sign_in_done` event.
    SignIn {
        /// The whole address the session starts at.
        url: String,
        /// The scheme the session comes back on, when it is not a link to the
        /// app's own origin.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        callback: Option<String>,
        /// Whether the session shares nothing with the system's browser.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        ephemeral: bool,
    },
    /// Show this message, as the system shows short messages, and come back
    /// with `message_shown` when it has gone.
    Message(Message),
    /// Play a haptic.
    Haptic {
        /// Which: `success`, `warning`, `error`, `selection`, `light`,
        /// `medium` or `heavy`.
        kind: String,
    },
    /// Run a device call: one a tap started, or one of the few a server's
    /// reply may start. When `posts`, come back with a `device_result` event.
    Device {
        /// The number the result comes back with.
        call: u64,
        /// The call, as the protocol writes it: `{"f":"share","title":"…"}`.
        device: serde_json::Value,
        /// Whether the call's result is wanted.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        posts: bool,
        /// What the server gave for this call to start with, which the core
        /// asked for: for `confirm` the passkey request (`{"publicKey": …}`),
        /// for `platform_sign_in` `{"nonce": "…"}`. The client asks the server
        /// for nothing itself.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        options: Option<serde_json::Value>,
    },
    /// Hand the core bytes from the system's own secure random generator, and
    /// come back with a `random` event that carries `request`. The core has
    /// no randomness of its own; a sign-in's secret is made from these.
    Random {
        /// The number the bytes come back with.
        request: u64,
        /// How many bytes, at least.
        bytes: u32,
    },
    /// Forget the session: every cookie of the app's origins, in the web
    /// view's store too. The person signed out.
    ForgetSession,
    /// Read what the client keeps under a key, and come back with a `cache` event.
    CacheRead {
        /// The key.
        key: String,
    },
    /// Keep this under a key, in place of what was there. Nothing takes the key away.
    CacheWrite {
        /// The key.
        key: String,
        /// What to keep, or nothing.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<String>,
    },
    /// Throw away everything kept for the cache.
    CacheClear,
    /// Say this to assistive technology.
    Announce {
        /// The server's own words.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        /// What happened, for the client to say in the person's language.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        notice: Option<Notice>,
        /// How many, when the notice counts something.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        count: Option<u32>,
    },
    /// Put this text on the clipboard.
    Copy {
        /// The text.
        text: String,
        /// Whether it is a secret the system should not show or sync.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        sensitive: bool,
    },
    /// Come back with a `timer` event after this long.
    Timer {
        /// The timer.
        timer: u64,
        /// How long, in milliseconds.
        ms: u32,
    },
    /// This timer is no longer wanted.
    CancelTimer {
        /// The timer.
        timer: u64,
    },
    /// Open a stream of server-sent events, and come back with `stream` events.
    StreamOpen {
        /// The stream.
        stream: u64,
        /// The whole address.
        url: String,
        /// The headers to send.
        headers: Vec<(String, String)>,
    },
    /// Close a stream.
    StreamClose {
        /// The stream.
        stream: u64,
    },
    /// There is nothing to go back to: the system takes the back.
    Exit,
    /// Something the app's developers will want to know: an answer that did
    /// not read, a kind this build has never heard of.
    Report {
        /// What it is about: `decode`, `kind`, `depth`.
        what: String,
        /// The detail.
        detail: String,
    },
}

/// A part of the view state that changed. The client reads the part again.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Change {
    /// The app's name, theme or user.
    App,
    /// Anything about where the person is: the stage, the destinations and
    /// their stacks, what is presented over them, the screen in front, the window.
    Nav,
    /// Everything about one screen. A screen no stack holds any more is gone.
    Screen {
        /// The screen.
        id: ScreenId,
    },
    /// The state of one form of a screen, and nothing else of the screen.
    Form {
        /// The screen.
        screen: ScreenId,
        /// The form's id.
        form: String,
    },
    /// Rows were added at the end of a list or table, and nothing else of the
    /// screen changed but that list's `more`. A client that does not read rows
    /// by themselves reads the screen.
    Rows {
        /// The screen.
        screen: ScreenId,
        /// The id of the list, or of the table or its region.
        id: String,
        /// How many rows it held before.
        from: usize,
        /// How many were added.
        count: usize,
    },
    /// Some rows of a list or table changed, and nothing else of the screen
    /// but what that list says of itself. Each row named in `rows` is new, has
    /// new content or stands in a new place: the part read with the query of
    /// the same shape gives its content and the row it now stands before. The
    /// rows named in `gone` left. A client that does not redraw rows by
    /// themselves reads the screen.
    Redraw {
        /// The screen.
        screen: ScreenId,
        /// The id of the list or of the table.
        id: String,
        /// The ids of the rows to draw again.
        rows: Vec<String>,
        /// The ids of the rows that left.
        gone: Vec<String>,
    },
    /// The messages waiting to be shown.
    Messages,
}

/// What one event led to.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Update {
    /// The parts of the view state that changed.
    pub changed: Vec<Change>,
    /// What the client is asked to do, in order.
    pub effects: Vec<Effect>,
}
