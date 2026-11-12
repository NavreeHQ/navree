//! [`ViewState`]: everything a client draws from. A client keeps no state of
//! its own about any of it; scroll positions and carets are the toolkit's.
//!
//! The rest of this file is what the core keeps for itself: what each request
//! on its way was for, and what each screen needs remembered.

use std::collections::{BTreeMap, BTreeSet};

use navree_native::action::{Pair, Present, State};
use navree_native::envelope::Upgrade;
use navree_native::node::{Icon, Method};
use navree_native::op::ToastAction;
use navree_native::shell::{NavItem, Route, Theme, User};
use navree_native::{Action, Screen, Tone};
use serde::{Deserialize, Serialize};

use crate::event::FieldValue;
use crate::origin::Origin;

/// The number of a screen. It never changes while the screen is in a stack,
/// and is never used again.
pub type ScreenId = u64;

/// How far the app has got in starting.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// The shell is not here yet.
    #[default]
    Starting,
    /// The app is running.
    Ready,
    /// The app could not start: see `failure`.
    Failed,
    /// The server asks for a newer app: see `upgrade`.
    Upgrade,
}

/// Why something could not be loaded. The words for each are the client's, in
/// the person's language.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Failure {
    /// The server could not be reached.
    Offline,
    /// No answer came in time.
    Timeout,
    /// The server answered with an error.
    Server,
    /// The answer was not one this client understands.
    Protocol,
    /// The app was built with a configuration the core can't use.
    Config,
}

/// What a message of the core's own says. The words are the client's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Notice {
    /// The server could not be reached.
    Offline,
    /// No answer came in time.
    Timeout,
    /// That didn't work.
    Failed,
    /// The latest change wasn't saved: check it and save again.
    Unsaved,
    /// The screen is still loading: try again in a moment.
    Loading,
    /// Text was copied.
    Copied,
    /// Fields need attention: `count` says how many.
    FieldsNeedAttention,
    /// More rows were loaded: `count` says how many.
    Loaded,
}

/// The width of the window, by class. Navigation and layout follow it, never
/// the kind of device.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Width {
    /// A phone held upright, or a narrow window.
    #[default]
    Compact,
    /// A small tablet, a phone on its side, an unfolded foldable.
    Medium,
    /// A tablet or a desktop window.
    Expanded,
}

/// The window the app is drawn in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Window {
    /// Its width.
    pub width: Width,
}

/// The app's own part of the chrome.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct App {
    /// The app's name.
    pub name: String,
    /// A line under the name: the workspace, the environment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    /// The address of the app's first screen.
    pub home: String,
    /// The brand's part in the look.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<Theme>,
    /// The person signed in, with their menu.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<User>,
}

/// A top-level destination and the screens stacked in it. The stack is empty
/// until the destination is first shown.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Destination {
    /// Its name.
    pub label: String,
    /// Its icon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<Icon>,
    /// Its address.
    pub href: String,
    /// A count beside it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge: Option<String>,
    /// The label of the group it stands in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// The destinations under it, as links a wide layout may show.
    pub items: Vec<NavItem>,
    /// Its screens, the first at the bottom.
    pub stack: Vec<ScreenId>,
}

/// Screens presented over everything else, with a stack of their own.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Modal {
    /// `sheet` or `full`.
    pub present: Present,
    /// Whether the person can't take it away: the sign-in a session that
    /// ended is sent to. A back with nothing else to do leaves the app.
    pub locked: bool,
    /// Its screens, the first at the bottom.
    pub stack: Vec<ScreenId>,
}

/// What a screen shows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Content {
    /// Nothing yet.
    Blank,
    /// A tree of parts, ready to draw: what is open, hidden, disabled or
    /// waiting is already in it.
    Native {
        /// The screen.
        screen: Box<Screen>,
    },
    /// The page in the web view.
    Web {
        /// The whole address to load.
        url: String,
        /// The page's name, for the client's own bar.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        /// What could not be drawn natively: for a developer's overlay.
        why: Vec<String>,
        /// Moves on whenever the web view is to load the address again.
        serial: u64,
    },
}

/// What one field holds and what is wrong with it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldView {
    /// The name its value is sent under.
    pub name: String,
    /// What it holds now.
    pub value: FieldValue,
    /// What is wrong with it, in the server's words.
    pub errors: Vec<String>,
}

/// The state of one form.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormView {
    /// Whether it has been sent and not yet answered. A second send that says
    /// the same is dropped.
    pub pending: bool,
    /// Its fields, by id.
    pub fields: BTreeMap<String, FieldView>,
}

/// The state of a region: a part that fetches itself.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegionView {
    /// Whether a fetch of it is on its way.
    pub loading: bool,
    /// Why its last fetch failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<Failure>,
}

/// The state of a list or table that loads more by cursor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListView {
    /// Whether its next rows are on their way.
    pub loading: bool,
    /// Whether the last try failed. A `more` event tries again.
    pub failed: bool,
}

/// A request to move the focus. The client gives it and says so with `focused`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Focus {
    /// The id of the node to focus: a field's id, most often.
    pub target: String,
    /// The request's number.
    pub serial: u64,
}

/// One screen.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenView {
    /// Its number.
    pub id: ScreenId,
    /// Its address: a path and query on the app's origin, or a whole address
    /// on another origin the app allows.
    pub url: String,
    /// What it shows.
    pub content: Content,
    /// Whether a fetch of the page is on its way.
    pub loading: bool,
    /// Whether the person asked for that fetch and is watching for it.
    pub refreshing: bool,
    /// Whether what it shows was kept from an earlier visit and a fresh one
    /// is on its way. Nothing is sent from a screen in this state.
    pub cached: bool,
    /// Why the last fetch failed. With nothing to show this is the screen;
    /// with content it is a passing message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<Failure>,
    /// The HTTP status the page answered with: `404` for a page that says it
    /// does not exist.
    pub status: u16,
    /// Whether the app's navigation is drawn around it.
    pub chrome: bool,
    /// The sheets, dialogs and popovers that are open, the last opened last.
    pub overlays: Vec<String>,
    /// States set on nodes that have no field for them, by the node's id.
    pub states: BTreeMap<String, Vec<State>>,
    /// The state of every form, by the form's id. Fields in no form are under
    /// the empty id.
    pub forms: BTreeMap<String, FormView>,
    /// The state of every region, by id.
    pub regions: BTreeMap<String, RegionView>,
    /// The state of every list or table that has asked for more, by id.
    pub lists: BTreeMap<String, ListView>,
    /// Where the focus is asked to go.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<Focus>,
}

impl ScreenView {
    pub(crate) fn new(id: ScreenId, url: String, chrome: bool) -> Self {
        Self {
            id,
            url,
            content: Content::Blank,
            loading: false,
            refreshing: false,
            cached: false,
            failure: None,
            status: 0,
            chrome,
            overlays: Vec::new(),
            states: BTreeMap::new(),
            forms: BTreeMap::new(),
            regions: BTreeMap::new(),
            lists: BTreeMap::new(),
            focus: None,
        }
    }

    /// The tree, when the screen shows one.
    pub fn screen(&self) -> Option<&Screen> {
        match &self.content {
            Content::Native { screen } => Some(screen),
            _ => None,
        }
    }

    pub(crate) fn screen_mut(&mut self) -> Option<&mut Screen> {
        match &mut self.content {
            Content::Native { screen } => Some(screen),
            _ => None,
        }
    }
}

/// A short message waiting to be shown.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    /// Its number.
    pub id: u64,
    /// What it means.
    pub tone: Tone,
    /// The server's words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// A heading over the words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// A message of the core's own, for the client to word.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notice: Option<Notice>,
    /// What its button says, when it has one. A tap is a `message_action` event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
}

/// Everything a client draws from.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewState {
    /// How far the app has got in starting.
    pub stage: Stage,
    /// Why it could not start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<Failure>,
    /// The newer app the server asks for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upgrade: Option<Upgrade>,
    /// The app's own part of the chrome.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<App>,
    /// The top-level destinations, with their stacks.
    pub destinations: Vec<Destination>,
    /// The destination in front: its place in `destinations`.
    pub selected: usize,
    /// What is presented over the destinations, the last on top.
    pub modals: Vec<Modal>,
    /// The screen the person is looking at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub front: Option<ScreenId>,
    /// Whether a back is the app's to handle. When it is not, the system takes it.
    pub can_go_back: bool,
    /// The window.
    pub window: Window,
    /// Every screen that is in a stack, by number.
    pub screens: BTreeMap<ScreenId, ScreenView>,
    /// The messages waiting to be shown, the oldest first.
    pub messages: Vec<Message>,
}

/// A part of the view state to read.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "part", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Query {
    /// All of it.
    #[default]
    All,
    /// `app`.
    App,
    /// Everything about where the person is, and no screen.
    Nav,
    /// One screen. `null` when no stack holds it.
    Screen {
        /// The screen.
        id: ScreenId,
    },
    /// The state of one form.
    Form {
        /// The screen.
        screen: ScreenId,
        /// The form's id.
        form: String,
    },
    /// Some rows of a list or table: `{"nodes": [...]}` for a list,
    /// `{"rows": [...]}` for a table, each with the `more` it has now.
    Rows {
        /// The screen.
        screen: ScreenId,
        /// The id of the list, or of the table or its region.
        id: String,
        /// The first row wanted.
        from: usize,
        /// How many.
        count: usize,
    },
    /// The messages.
    Messages,
}

#[derive(Serialize)]
struct NavPart<'a> {
    stage: Stage,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure: Option<Failure>,
    #[serde(skip_serializing_if = "Option::is_none")]
    upgrade: &'a Option<Upgrade>,
    destinations: &'a [Destination],
    selected: usize,
    modals: &'a [Modal],
    #[serde(skip_serializing_if = "Option::is_none")]
    front: Option<ScreenId>,
    can_go_back: bool,
    window: Window,
}

impl ViewState {
    /// The part of the view state `query` asks for, as JSON.
    pub fn part(&self, query: &Query) -> serde_json::Value {
        fn json<T: Serialize + ?Sized>(value: &T) -> serde_json::Value {
            serde_json::to_value(value).unwrap_or(serde_json::Value::Null)
        }
        match query {
            Query::All => json(self),
            Query::App => json(&self.app),
            Query::Nav => json(&NavPart {
                stage: self.stage,
                failure: self.failure,
                upgrade: &self.upgrade,
                destinations: &self.destinations,
                selected: self.selected,
                modals: &self.modals,
                front: self.front,
                can_go_back: self.can_go_back,
                window: self.window,
            }),
            Query::Screen { id } => json(&self.screens.get(id)),
            Query::Form { screen, form } => {
                json(&self.screens.get(screen).and_then(|screen| screen.forms.get(form)))
            }
            Query::Rows {
                screen,
                id,
                from,
                count,
            } => self
                .screens
                .get(screen)
                .and_then(|screen| crate::regions::rows(screen, id, *from, *count))
                .unwrap_or(serde_json::Value::Null),
            Query::Messages => json(&self.messages),
        }
    }

    /// The stack in front: the top modal's, or the selected destination's.
    pub(crate) fn front_stack(&self) -> &[ScreenId] {
        match self.modals.last() {
            Some(modal) => &modal.stack,
            None => self
                .destinations
                .get(self.selected)
                .map_or(&[], |destination| &destination.stack),
        }
    }

    /// Whether any stack holds the screen.
    pub(crate) fn holds(&self, id: ScreenId) -> bool {
        self.screens.contains_key(&id)
    }

    /// Whether the screen is presented over the destinations.
    pub(crate) fn in_modal(&self, id: ScreenId) -> bool {
        self.modals.iter().any(|modal| modal.stack.contains(&id))
    }

    /// The stack that holds the screen.
    pub(crate) fn stack_of(&mut self, id: ScreenId) -> Option<&mut Vec<ScreenId>> {
        self.modals
            .iter_mut()
            .map(|modal| &mut modal.stack)
            .chain(self.destinations.iter_mut().map(|destination| &mut destination.stack))
            .find(|stack| stack.contains(&id))
    }
}

// What the core keeps for itself.

/// What a request on its way was for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Pending {
    /// The shell.
    Shell,
    /// The page of a screen, on its `visit`.
    Page { screen: ScreenId, visit: u64 },
    /// A form or a button's own request.
    Send {
        screen: ScreenId,
        visit: u64,
        /// What is being sent: a form's id, or the request itself.
        key: String,
        /// Whether it may have changed something on the server.
        write: bool,
        /// Whether a screen in the answer takes this screen's place.
        in_place: bool,
    },
    /// A region, on its `turn`. `go` when the region went to another address
    /// of itself.
    Region {
        screen: ScreenId,
        visit: u64,
        region: String,
        turn: u64,
        epoch: u64,
        go: bool,
        url: String,
    },
    /// The next rows of a list or table.
    More {
        screen: ScreenId,
        visit: u64,
        id: String,
        was: String,
    },
}

/// What a timer the core asked for is for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TimerFor {
    /// A table's query form was typed in: ask when the typing pauses.
    Query { screen: ScreenId, form: String },
    /// A stream spoke for a region: fetch once it is quiet.
    Live { screen: ScreenId, region: String },
    /// A stream that failed is tried again.
    Reopen { stream: u64 },
}

/// A stream of server-sent events the core has asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Stream {
    pub id: u64,
    /// Its address, as a region names it after resolving.
    pub url: String,
    pub open: bool,
    /// How many times in a row it has failed.
    pub failures: u32,
    /// The timer that will try it again.
    pub reopen: Option<u64>,
    /// It was refused for good, or is closed while the app is not in front.
    pub stopped: bool,
    /// It has been open before: regions that listen may have missed something.
    pub missed: bool,
}

/// How a form is sent, as its tree says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FormMeta {
    pub method: Method,
    pub action: String,
    /// The id of the region or table whose query it is.
    pub query_of: Option<String>,
}

/// A region as its tree says, with its address resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RegionMeta {
    pub src: String,
    /// The resolved address of its stream, and the event it listens for.
    pub live: Option<(String, Option<String>)>,
}

/// Whether a live region's fetch is on its way and whether the stream spoke
/// meanwhile.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct LiveSub {
    pub busy: bool,
    pub dirty: bool,
    pub timer: Option<u64>,
}

/// A send that waits for the one before it: the latest only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Waiting {
    pub said: String,
    pub with: Option<Pair>,
    pub href: Option<String>,
}

/// What the core remembers about a screen that the client has no use for.
#[derive(Clone, Debug, Default)]
pub(crate) struct ScreenInner {
    /// Moves on whenever the screen is asked for again or changes address:
    /// an answer for an earlier visit is dropped.
    pub visit: u64,
    /// What it shows may be out of date: it is fetched again when it comes to the front.
    pub stale: bool,
    pub form_ids: BTreeSet<String>,
    pub field_ids: BTreeSet<String>,
    /// The id the server gave each form, by the form's id here.
    pub server_ids: BTreeMap<String, Option<String>>,
    pub forms: BTreeMap<String, FormMeta>,
    /// The fields the person changed, by id.
    pub dirty: BTreeSet<String>,
    /// The sends on their way, with what each said.
    pub sending: BTreeMap<String, String>,
    pub waiting: BTreeMap<String, Waiting>,
    /// The forms an op took away or put a new one in place of, since the last
    /// send was answered: what waited to be sent from one has nowhere to go.
    pub replaced: BTreeSet<String>,
    /// The newest fetch of each region.
    pub turns: BTreeMap<String, u64>,
    pub regions: BTreeMap<String, RegionMeta>,
    pub live: BTreeMap<String, LiveSub>,
    /// The lists whose next rows are on their way, with the address asked.
    pub more: BTreeMap<String, String>,
    pub query_timers: BTreeMap<String, u64>,
    /// The button actions with a request on its way, each with the send it belongs to.
    pub busy: Vec<(String, Action)>,
    /// This screen is the first of a cover the server put up because the
    /// session ended; the route that said so.
    pub cover: Option<String>,
    /// It held live regions and left the front: they may have missed something.
    pub missed: bool,
}

/// What the core remembers about a message.
#[derive(Clone, Debug, Default)]
pub(crate) struct MessageInner {
    pub action: Option<ToastAction>,
    pub screen: Option<ScreenId>,
    /// The server's name for the toast: a later one of the same name takes its place.
    pub name: Option<String>,
}

/// A device call on its way.
#[derive(Clone, Debug)]
pub(crate) struct DeviceCall {
    pub screen: Option<ScreenId>,
    /// Where the result is posted.
    pub then: Option<String>,
    /// The server's one-time ticket for the result.
    pub ticket: Option<String>,
}

/// The numbers the core hands out.
#[derive(Clone, Debug, Default)]
pub(crate) struct Counters {
    pub screen: u64,
    pub request: u64,
    pub message: u64,
    pub timer: u64,
    pub stream: u64,
    pub focus: u64,
    pub call: u64,
    pub web: u64,
}

/// Everything the core keeps for itself.
#[derive(Debug, Default)]
pub(crate) struct Inner {
    pub origin: Origin,
    /// Whether the configuration gave an origin the core can use.
    pub usable: bool,
    pub next: Counters,
    /// The requests on their way: what each was for, and the address it went to.
    pub pending: BTreeMap<u64, (Pending, String)>,
    pub screens: BTreeMap<ScreenId, ScreenInner>,
    /// The build the kept trees belong to.
    pub build: Option<String>,
    /// The stamps of the app and of the shell, as the last answer gave them.
    pub app_stamp: Option<String>,
    pub shell_stamp: Option<String>,
    pub routes: Vec<Route>,
    /// Who the shell is for now, and who was last signed in. When the person
    /// changes, nothing of the one before may show.
    pub user: Option<(String, Option<String>)>,
    pub person: Option<(String, Option<String>)>,
    pub has_shell: bool,
    /// An address the app was opened with before the shell had arrived.
    pub launch: Option<String>,
    /// Moves on with every write: an answer asked for before one may be from before it.
    pub epoch: u64,
    pub cache: crate::cache::Cache,
    pub timers: BTreeMap<u64, TimerFor>,
    pub streams: Vec<Stream>,
    pub background: bool,
    pub messages: BTreeMap<u64, MessageInner>,
    pub calls: BTreeMap<u64, DeviceCall>,
    /// The sign-in session is open; the screen to come back to.
    pub signing_in: bool,
}
