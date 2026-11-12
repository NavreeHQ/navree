//! [`Event`]: everything a client tells the core. One thing happened; the
//! core answers with what changed and what to do about it.

use navree_native::action::Pair;
use navree_native::client::{Contrast, Scheme};
use navree_native::Action;
use serde::{Deserialize, Serialize};

use crate::state::{ScreenId, Width};

/// A file the person picked. The client keeps the bytes and gives the core a
/// token for them; a multipart request names the token and the client's
/// transport puts the bytes in.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRef {
    /// The client's name for the bytes.
    pub token: String,
    /// The file's name.
    pub name: String,
    /// Its media type.
    #[serde(default, rename = "type")]
    pub media_type: String,
    /// Its size in bytes.
    #[serde(default)]
    pub size: u64,
}

/// What a field holds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum FieldValue {
    /// Typed text, a date, a colour, a slider's number, or the one choice made.
    Text {
        /// The value as a form sends it.
        text: String,
    },
    /// A switch or a checkbox.
    Flag {
        /// Whether it is on.
        on: bool,
    },
    /// The choices made, of several.
    Many {
        /// Their values.
        values: Vec<String>,
    },
    /// The files picked.
    Files {
        /// The files.
        files: Vec<FileRef>,
    },
    /// Nothing is chosen.
    None,
}

fn is_zero(value: &u16) -> bool {
    *value == 0
}

/// Why a request got no answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportError {
    /// The server could not be reached.
    Offline,
    /// No answer came in the time the request allowed.
    Timeout,
    /// The connection was refused as unsafe, or failed in some other way.
    Other,
}

/// The answer to a `fetch` effect. Exactly one of an answer (`status`),
/// `error` or `left` is given.
///
/// A redirect is best handed to the core as it came, with its `status` and
/// `location`: the core checks where it leads and asks again. A transport that
/// follows redirects itself must stop at the first that leaves the app's
/// origins and say so with `left`, and gives the last address as `url`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Response {
    /// The number of the request this answers.
    pub request: u64,
    /// The HTTP status.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub status: u16,
    /// The address the answer came from in the end, as a whole address. Left
    /// out, it is the address that was asked for.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub url: String,
    /// The answer's `Content-Type`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    /// The answer's `Location`, when the answer is a redirect the client's
    /// transport did not follow. The core follows it, if it stays inside.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    /// The answer's body, as text.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub body: String,
    /// No answer came.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<TransportError>,
    /// A redirect led to another site, at this address. Nothing was fetched from it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left: Option<String>,
}

/// What a stream of server-sent events did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamState {
    /// The connection is open.
    Open,
    /// An event came. Only its name is read.
    Event,
    /// The connection ended or could not be made.
    Closed,
}

/// One thing that happened.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Event {
    /// The app started, or the person asked to try again after it failed to.
    Start,
    /// A request was answered.
    Response(Response),
    /// The person chose a top-level destination. Choosing the one in front
    /// goes back to its first screen.
    Select {
        /// Its place in `destinations`.
        destination: usize,
    },
    /// The action of something the person tapped: a button, a link, a row.
    Act {
        /// The screen it stands on. The screen in front when left out: the
        /// user's menu belongs to no screen.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        screen: Option<ScreenId>,
        /// The action, as the node holds it.
        action: Action,
        /// The id of the form the control stands in.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        form: Option<String>,
        /// The id of the sheet, dialog or popover the control stands in.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        overlay: Option<String>,
        /// What the control holds, when it is one that holds text.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
    },
    /// The person changed a field.
    Field {
        /// The screen.
        screen: ScreenId,
        /// The id of the form the field stands in.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        form: Option<String>,
        /// The field's id.
        field: String,
        /// What it holds now.
        value: FieldValue,
    },
    /// The person searched with the screen's own search.
    Search {
        /// The screen.
        screen: ScreenId,
        /// The words typed. Empty to clear the search.
        text: String,
    },
    /// The system's back.
    Back,
    /// The person went back to a screen further down the stack in front.
    PopTo {
        /// The screen that is now on top.
        screen: ScreenId,
    },
    /// The person took a presented screen away: swiped a sheet down.
    Dismissed {
        /// A screen of the sheet or cover.
        screen: ScreenId,
    },
    /// The person opened or closed a sheet, a dialog, a popover or a
    /// disclosure of the tree themselves.
    Overlay {
        /// The screen.
        screen: ScreenId,
        /// The node's id.
        id: String,
        /// Whether it is open now.
        open: bool,
    },
    /// The person pulled to refresh, or chose to try again after a failure.
    Refresh {
        /// The screen.
        screen: ScreenId,
    },
    /// The person asked for a region that failed to load again.
    RegionRetry {
        /// The screen.
        screen: ScreenId,
        /// The region's id.
        region: String,
    },
    /// The end of a list or table came near, or its "more" was tapped.
    More {
        /// The screen.
        screen: ScreenId,
        /// The id of the list, of the table, or of the region the table stands in.
        id: String,
    },
    /// A table goes to another address of itself: a column's sort, the page
    /// before, the page after when pages replace each other.
    RegionGo {
        /// The screen.
        screen: ScreenId,
        /// The id of the region the table stands in.
        region: String,
        /// The address, as the tree holds it.
        href: String,
    },
    /// A message was shown and went away.
    MessageShown {
        /// The message.
        id: u64,
    },
    /// The person used the button of a message.
    MessageAction {
        /// The message.
        id: u64,
    },
    /// A link was tapped in the web view. The web view does not follow it.
    WebLink {
        /// The screen the web view stands on.
        screen: ScreenId,
        /// Where the link leads: the whole address.
        url: String,
    },
    /// The app was opened with an address from outside.
    OpenLink {
        /// The address.
        url: String,
    },
    /// The person tapped a notification.
    Notification {
        /// The address the notification carries.
        href: String,
    },
    /// The window's width changed class.
    Window {
        /// The width now.
        width: Width,
    },
    /// The person's settings changed. Later requests say so.
    Environment {
        /// Light or dark.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scheme: Option<Scheme>,
        /// The contrast asked for.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        contrast: Option<Contrast>,
        /// The text size, in percent of the system's default.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text_percent: Option<u16>,
        /// The locale.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        locale: Option<String>,
        /// The time zone.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tz: Option<String>,
    },
    /// The app came back to the front.
    Foreground,
    /// The app left the front.
    Background,
    /// The session is over and the client has thrown its cookies away.
    SignedOut,
    /// A timer the core asked for ran out.
    Timer {
        /// The timer.
        timer: u64,
    },
    /// A stream the core asked for did something.
    Stream {
        /// The stream.
        stream: u64,
        /// What it did.
        state: StreamState,
        /// The event's name, for an event that has one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        /// The HTTP status a connection was refused with.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<u16>,
    },
    /// A `cache_read` finished.
    Cache {
        /// The key that was read.
        key: String,
        /// What was kept under it, or nothing.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<String>,
    },
    /// A device call finished.
    DeviceResult {
        /// The call.
        call: u64,
        /// Whether it did what was asked.
        ok: bool,
        /// What it found, as a form's fields.
        #[serde(default)]
        fields: Vec<Pair>,
    },
    /// The system's sign-in session ended.
    SignInDone {
        /// The address it came back with, or nothing when the person gave up.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
    },
    /// A focus the view state asked for was given.
    Focused {
        /// The screen.
        screen: ScreenId,
        /// The number of the focus request.
        serial: u64,
    },
    /// The person agreed to go and get the newer app.
    OpenUpgrade,
}
