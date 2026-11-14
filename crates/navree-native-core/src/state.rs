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
    /// Signing in needs one more step, which this app can't take the person
    /// through: they finish it another way.
    SecondStep,
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
    /// The options a search found, for a choice that is searched on the
    /// server: drawn in place of the control's own while it is there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<navree_native::input::ChoiceOption>>,
    /// Whether more options than these can be asked for.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub more: bool,
    /// Whether a search for options is on its way.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub searching: bool,
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
    /// Whether the rows before its first are on their way.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub loading_before: bool,
    /// Whether the last try for those failed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub failed_before: bool,
    /// Whether the person chose every record that matches the table's query,
    /// not only the rows loaded.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub across: bool,
}

/// A question the core holds until the person answers it: what a control
/// asks before it does what can't be undone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Question {
    /// Its number: what the answer names.
    pub id: u64,
    /// What is asked: `Cancel order CH-1042?`.
    pub title: String,
    /// What follows from it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The words of the button that goes ahead.
    pub confirm: String,
    /// The words of the button that goes back, when not the system's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancel: Option<String>,
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
    /// Its title, as its page last said it, or as a snapshot kept it: there
    /// while the screen has nothing to show yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
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
    /// The question waiting for the person's answer, in the system's own
    /// confirmation. An `answer` event takes it away.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question: Option<Question>,
    /// The id of the part whose action opened this screen, on the screen it
    /// was opened from: the card the person tapped, as the `act` event named
    /// it. A client that grows the screen out of that part (a card whose
    /// `transition` is `zoom`) shrinks it back into the same part when the
    /// person leaves. None for a screen no action with a `from` opened.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
}

impl ScreenView {
    pub(crate) fn new(id: ScreenId, url: String, chrome: bool) -> Self {
        Self {
            id,
            url,
            title: None,
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
            question: None,
            from: None,
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
    /// The rows a `redraw` change names: each with its content and the id of
    /// the row it now stands before (none at the end), in the list's order,
    /// with what the list says of itself.
    Redraw {
        /// The screen.
        screen: ScreenId,
        /// The id of the list or of the table.
        id: String,
        /// The ids of the rows wanted.
        rows: Vec<String>,
        /// The ids of the rows that left: given back as they came.
        #[serde(default)]
        gone: Vec<String>,
    },
    /// What a client stores so the app can start again where the person was
    /// after the system ended it. Read from the core, not from the view state.
    Snapshot,
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
            Query::Form { screen, form } => json(
                &self
                    .screens
                    .get(screen)
                    .and_then(|screen| screen.forms.get(form)),
            ),
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
            Query::Redraw {
                screen,
                id,
                rows,
                gone,
            } => self
                .screens
                .get(screen)
                .and_then(|screen| crate::regions::redrawn(screen, id, rows, gone))
                .unwrap_or(serde_json::Value::Null),
            // The core answers this one: see `Core::part`.
            Query::Snapshot => serde_json::Value::Null,
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
            .chain(
                self.destinations
                    .iter_mut()
                    .map(|destination| &mut destination.stack),
            )
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
    /// The rows before a list's first.
    Before {
        screen: ScreenId,
        visit: u64,
        id: String,
        was: String,
    },
    /// What changed in a list since its revision.
    Changes {
        screen: ScreenId,
        visit: u64,
        list: String,
    },
    /// A row's move: where it came from, to put it back if the server refuses.
    Move {
        screen: ScreenId,
        visit: u64,
        item: String,
        from: String,
        at: usize,
    },
    /// A search for a choice's options, or the options after those it has.
    Options {
        screen: ScreenId,
        visit: u64,
        form: String,
        field: String,
        turn: u64,
        more: bool,
    },
    /// What a device call needs from the server before the client can run
    /// it: a confirmation's challenge, a sign-in's nonce.
    BeforeCall { call: u64 },
    /// A sign-in session's code, exchanged for a session of the app's own.
    Exchange,
    /// The device's push token, posted for the session that is signed in.
    PushPut { token: String },
    /// The device's push token, taken away because the person signed out.
    PushGone,
    /// A platform sign-in's token, posted to the app's own sign-in route, with
    /// the page the call names for a second step.
    SignIn {
        screen: ScreenId,
        second_step: Option<String>,
    },
}

impl Pending {
    /// The screen the request is for, when it is for one's content.
    pub(crate) fn screen(&self) -> Option<ScreenId> {
        match self {
            Self::Page { screen, .. }
            | Self::Send { screen, .. }
            | Self::Region { screen, .. }
            | Self::More { screen, .. }
            | Self::Before { screen, .. }
            | Self::Changes { screen, .. }
            | Self::Move { screen, .. }
            | Self::Options { screen, .. } => Some(*screen),
            Self::Shell
            | Self::BeforeCall { .. }
            | Self::SignIn { .. }
            | Self::Exchange
            | Self::PushPut { .. }
            | Self::PushGone => None,
        }
    }

    /// Whether the request may have changed something on the server: its
    /// answer is wanted even when its screen is gone.
    pub(crate) fn writes(&self) -> bool {
        matches!(self, Self::Send { .. } | Self::Move { .. })
    }
}

/// A request on its way.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Asked {
    /// What it is for.
    pub purpose: Pending,
    /// The address it went to: an address inside.
    pub url: String,
    /// The address that was asked for first, before any redirect was followed.
    pub first: String,
    pub method: Method,
    pub body: Option<crate::effect::Body>,
    pub limit: u32,
    /// Whether it is a removal, not a read or a write.
    pub delete: bool,
    /// How many redirects were followed to get here.
    pub hops: u8,
    /// The part of the screen asked for, by id: a region, or a list's next rows.
    pub region: Option<String>,
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
    /// A stream spoke for a list: ask what changed once it is quiet.
    Changes { screen: ScreenId, list: String },
    /// A field that sends its form as it changes was typed in: send when the typing pauses.
    Changed {
        screen: ScreenId,
        form: String,
        action: Box<Action>,
    },
    /// A searched choice was typed in: ask for options when the typing pauses.
    Options {
        screen: ScreenId,
        form: String,
        field: String,
    },
}

/// A list that asks what changed in it since its revision.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ListLive {
    /// Where it asks, as an address inside.
    pub changes: String,
    /// The stream that says when, and the event that means it.
    pub live: Option<(String, Option<String>)>,
    /// An ask is on its way.
    pub busy: bool,
    /// The stream spoke while one was: one more follows.
    pub dirty: bool,
    /// The answer said more changes follow: the next ask goes at once.
    pub again: bool,
    /// The timer that waits for the stream to go quiet.
    pub timer: Option<u64>,
}

/// A search for a field's options: the newest, and the timer that waits for
/// the typing to pause.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct OptionsAsk {
    pub turn: u64,
    pub timer: Option<u64>,
    /// The words to search for when the timer runs out.
    pub words: String,
    /// Where the options after those found are.
    pub more: Option<String>,
}

/// What a control was about to do when it asked its question.
#[derive(Clone, Debug)]
pub(crate) struct Held {
    pub question: u64,
    pub action: Action,
    pub form: Option<String>,
    pub overlay: Option<String>,
    pub text: Option<String>,
    pub now: Option<i64>,
    pub from: Option<String>,
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
    /// The lists that ask what changed in them, by id.
    pub live_lists: BTreeMap<String, ListLive>,
    /// The lists whose rows before the first are on their way, with the address asked.
    pub before: BTreeMap<String, String>,
    /// The searches for options, by the field's id.
    pub options: BTreeMap<String, OptionsAsk>,
    /// What waits for the answer to the screen's question.
    pub held: Option<Held>,
    /// What a snapshot kept of the screen, until its page arrives.
    pub restored: Option<Restored>,
    /// This screen finishes a sign-in's second step: when a write from it
    /// leads away, the person may be signed in, and the shell says.
    pub second_step: bool,
}

/// What the core remembers about a message.
#[derive(Clone, Debug, Default)]
pub(crate) struct MessageInner {
    pub action: Option<ToastAction>,
    pub screen: Option<ScreenId>,
    /// The server's name for the toast: a later one of the same name takes its place.
    pub name: Option<String>,
}

/// A device call on its way, whose result goes to the server.
#[derive(Clone, Debug)]
pub(crate) struct DeviceCall {
    pub screen: ScreenId,
    /// The call, as the client is told to run it.
    pub call: navree_native::device::Call,
    pub goes: Goes,
}

/// Where a device call's result goes.
#[derive(Clone, Debug)]
pub(crate) enum Goes {
    /// As a form to the call's continuation, with the server's one-time
    /// ticket; without it when the ticket is `spent` (a confirmation's, at
    /// its challenge route).
    Then {
        then: navree_native::device::Then,
        spent: bool,
    },
    /// As JSON to the app's own sign-in route, with the state the nonce route
    /// gave. `web` is where the web's sign-in starts, for a system with no
    /// sheet for the provider; `second_step` the page that finishes a second
    /// step, when the call names one of its own.
    SignIn {
        sign_in: String,
        state: String,
        web: Option<String>,
        second_step: Option<String>,
    },
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
    pub question: u64,
    pub random: u64,
}

/// A sign-in in the system's own session, handed over to the app with a
/// one-time code. One at a time.
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum HandOver {
    /// Random bytes were asked for: the session opens when they come.
    Wanted { request: u64, ephemeral: bool },
    /// The session is open. The return must carry this `state`; the verifier
    /// goes with the code and is then forgotten.
    Open { state: String, verifier: String },
    /// The code is on its way to the exchange route.
    Exchanging,
}

impl std::fmt::Debug for HandOver {
    /// Never the state or the verifier.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Wanted { .. } => "HandOver::Wanted",
            Self::Open { .. } => "HandOver::Open",
            Self::Exchanging => "HandOver::Exchanging",
        })
    }
}

/// The device's push token and what the server knows of it.
#[derive(Clone, Default, PartialEq, Eq)]
pub(crate) struct PushState {
    /// The token the system gave, with its environment.
    pub token: Option<String>,
    pub environment: navree_native::device::PushEnvironment,
    /// The token the server holds for the session that is signed in.
    pub registered: Option<String>,
    /// The token on its way there.
    pub posting: Option<String>,
    /// A token the server would not take: not posted again until it changes
    /// or another person signs in.
    pub refused: Option<String>,
}

impl std::fmt::Debug for PushState {
    /// Never the token.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "PushState {{ token: {}, registered: {} }}",
            self.token.is_some(),
            self.registered.is_some()
        )
    }
}

/// What a snapshot kept of one screen, to put back when its page arrives.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Restored {
    /// What the person had typed, by the field's id.
    pub fields: BTreeMap<String, FieldValue>,
    /// The sheets and dialogs that were open.
    pub open: Vec<String>,
}

/// Everything the core keeps for itself.
#[derive(Debug, Default)]
pub(crate) struct Inner {
    pub origin: Origin,
    /// Whether the configuration gave an origin the core can use.
    pub usable: bool,
    pub next: Counters,
    /// The requests on their way.
    pub pending: BTreeMap<u64, Asked>,
    /// Requests whose address was not on one of the app's origins: never
    /// sent, they fail before the update goes out.
    pub unsent: Vec<u64>,
    /// The number the first request asked while the event in hand is
    /// handled gets: what was asked before is from before the event.
    pub event_start: u64,
    /// An answer the core makes itself (a request never sent) is being read.
    pub synthetic: bool,
    /// An address an answer gave did not read, and that was said, in this event.
    pub refusal_said: bool,
    pub screens: BTreeMap<ScreenId, ScreenInner>,
    /// The build the kept trees belong to.
    pub build: Option<String>,
    /// The stamp of the shell held, when the shell said one; the stamp the
    /// last answer named; and the stamp the shell was last asked again for.
    pub shell_stamp: Option<String>,
    pub seen_stamp: Option<String>,
    pub asked_stamp: Option<String>,
    pub routes: Vec<Route>,
    /// Who the shell is for now, and who was last signed in. When the person
    /// changes, nothing of the one before may show.
    pub user: Option<crate::cache::Who>,
    pub person: Option<crate::cache::Who>,
    /// Who the shell about to be installed names, when its answer said it by
    /// id: taken by the install.
    pub naming: Option<String>,
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
    /// The sign-in that is being handed over, when the shell names the routes for one.
    pub hand_over: Option<HandOver>,
    /// The routes the shell names for the core to call.
    pub sign_in: Option<navree_native::shell::SignIn>,
    pub push_route: Option<String>,
    pub push: PushState,
    /// The person signed out on purpose and the push token's removal is on
    /// its way: the session is forgotten when it is answered.
    pub signing_out: bool,
    /// Where a reply said to land, which is opened whatever the rules for
    /// links from outside say.
    pub land: Option<String>,
    /// A snapshot to start from, once the shell says who is signed in.
    pub restore: Option<crate::snapshot::Snapshot>,
    /// The id of the part whose action is being carried out now: a screen
    /// that action opens keeps it.
    pub acting: Option<String>,
}
