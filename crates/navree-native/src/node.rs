//! [`Node`]: the closed vocabulary of parts a screen is made of.
//!
//! Every node is a JSON object whose `t` names its kind. A node says what a part
//! *is* (a list, a field, a button with a role); the client's own design layer
//! decides where it goes and how it looks on its system. A field left out of the
//! JSON holds its default, and a client passes over a field it does not know.
//! A kind it does not know is read as [`Node::Unknown`], which keeps the `alt`
//! a newer server wrote for it.

use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize, Serializer};

use crate::action::Action;
use crate::input::Input;

pub(crate) fn is_false(value: &bool) -> bool {
    !*value
}

pub(crate) fn is_zero(value: &u8) -> bool {
    *value == 0
}

pub(crate) fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

/// One part of a screen: `{"t": kind, ...}` in JSON, with the kind's fields
/// beside `t`.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Node {
    /// Parts one after another, down or across.
    Stack(Stack),
    /// Parts in columns, as many as fit.
    Grid(Grid),
    /// A titled group of parts.
    Section(Section),
    /// Rows that may go on: more load by cursor.
    List(List),
    /// One row of a list.
    Row(Row),
    /// A primary pane and its detail.
    Split(Split),
    /// Views a reader switches between.
    Tabs(Tabs),
    /// A part that opens and closes under its label.
    Disclosure(Disclosure),
    /// A line between parts.
    Divider(Divider),
    /// Room that takes what is left.
    Spacer(Spacer),
    /// A part that fetches itself: late, or again when a stream speaks.
    Region(Region),
    /// Text in one style.
    Text(Text),
    /// Text in runs: bold, italic, code and links.
    Rich(Rich),
    /// A picture.
    Image(Image),
    /// An icon, as path data.
    Icon(Icon),
    /// A person's picture or initials.
    Avatar(Avatar),
    /// A short label with a tone.
    Badge(Badge),
    /// How far something has got.
    Progress(Progress),
    /// A message in place, with a tone.
    Banner(Banner),
    /// What a place shows when it holds nothing.
    Empty(Empty),
    /// Labels with their values.
    Facts(Facts),
    /// Records in columns.
    Table(Table),
    /// A chart, as the numbers it draws.
    Chart(Chart),
    /// Code, in a fixed-width face.
    Code(Code),
    /// A button.
    Button(Button),
    /// A link in or beside text.
    Link(Link),
    /// A button that opens a list of actions.
    Menu(Menu),
    /// A form.
    Form(Form),
    /// One field of a form.
    Field(Field),
    /// A sheet over the screen. It draws nothing until it is opened.
    Sheet(Sheet),
    /// A dialog. It draws nothing until it is opened.
    Dialog(Dialog),
    /// A small panel anchored to what opened it.
    Popover(Popover),
    /// A plugin's own view, by name.
    Native(NativeView),
    /// HTML for a small region. A client may refuse it and show the whole page in
    /// its web view.
    Web(WebRegion),
    /// A kind this build has never heard of, from a newer server. A client
    /// draws its `alt`, or shows the page in its web view when it has none.
    Unknown(Unknown),
}

/// A part of a kind this build of the protocol does not know. It keeps what an
/// older client can use: the kind's name, the id ops may name it by, and the
/// part a newer server wrote to stand in its place. It is written back as it
/// came, with only those.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Unknown {
    /// The kind, as the part's `t` named it.
    pub kind: String,
    /// The part's id.
    pub id: Option<String>,
    /// What stands in its place for a client that does not know the kind.
    pub alt: Option<Box<Node>>,
}

impl Serialize for Unknown {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("t", &self.kind)?;
        if let Some(id) = &self.id {
            map.serialize_entry("id", id)?;
        }
        if let Some(alt) = &self.alt {
            map.serialize_entry("alt", alt)?;
        }
        map.end()
    }
}

impl Unknown {
    /// Reads the parts of `value` an older client keeps: its id and its `alt`.
    pub(crate) fn read(kind: &str, value: &serde_json::Value) -> Result<Self, serde_json::Error> {
        Ok(Self {
            kind: kind.to_owned(),
            id: value
                .get("id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
            alt: match value.get("alt") {
                Some(alt) => Some(Box::new(Node::deserialize(alt)?)),
                None => None,
            },
        })
    }
}

impl Node {
    /// The node's id, when it has one: what ops and commands name.
    pub fn id(&self) -> Option<&str> {
        match self {
            Self::Stack(node) => node.id.as_deref(),
            Self::Grid(node) => node.id.as_deref(),
            Self::Section(node) => node.id.as_deref(),
            Self::List(node) => node.id.as_deref(),
            Self::Row(node) => node.id.as_deref(),
            Self::Split(node) => node.id.as_deref(),
            Self::Tabs(node) => node.id.as_deref(),
            Self::Disclosure(node) => node.id.as_deref(),
            Self::Divider(node) => node.id.as_deref(),
            Self::Spacer(node) => node.id.as_deref(),
            Self::Region(node) => Some(&node.id),
            Self::Text(node) => node.id.as_deref(),
            Self::Rich(node) => node.id.as_deref(),
            Self::Image(node) => node.id.as_deref(),
            Self::Icon(node) => node.id.as_deref(),
            Self::Avatar(node) => node.id.as_deref(),
            Self::Badge(node) => node.id.as_deref(),
            Self::Progress(node) => node.id.as_deref(),
            Self::Banner(node) => node.id.as_deref(),
            Self::Empty(node) => node.id.as_deref(),
            Self::Facts(node) => node.id.as_deref(),
            Self::Table(node) => node.id.as_deref(),
            Self::Chart(node) => node.id.as_deref(),
            Self::Code(node) => node.id.as_deref(),
            Self::Button(node) => node.id.as_deref(),
            Self::Link(node) => node.id.as_deref(),
            Self::Menu(node) => node.id.as_deref(),
            Self::Form(node) => node.id.as_deref(),
            Self::Field(node) => node.id.as_deref(),
            Self::Sheet(node) => Some(&node.id),
            Self::Dialog(node) => Some(&node.id),
            Self::Popover(node) => Some(&node.id),
            Self::Native(node) => node.id.as_deref(),
            Self::Web(node) => node.id.as_deref(),
            Self::Unknown(node) => node.id.as_deref(),
        }
    }

    /// The nodes directly inside this one, in the order a reader meets them.
    pub fn children(&self) -> Vec<&Node> {
        fn boxed(node: &Option<Box<Node>>) -> impl Iterator<Item = &Node> {
            node.iter().map(Box::as_ref)
        }
        match self {
            Self::Stack(node) => node.children.iter().collect(),
            Self::Grid(node) => node.children.iter().collect(),
            Self::Section(node) => node
                .actions
                .iter()
                .chain(&node.children)
                .chain(&node.footer)
                .collect(),
            Self::List(node) => node.children.iter().collect(),
            Self::Row(node) => boxed(&node.leading)
                .chain(&node.detail)
                .chain(boxed(&node.trailing))
                .chain(&node.menu)
                .collect(),
            Self::Split(node) => vec![&*node.primary, &*node.detail],
            Self::Tabs(node) => node.items.iter().flat_map(|tab| &tab.children).collect(),
            Self::Disclosure(node) => node.children.iter().collect(),
            Self::Region(node) => node.children.iter().collect(),
            Self::Banner(node) => node.actions.iter().collect(),
            Self::Empty(node) => node.actions.iter().collect(),
            Self::Facts(node) => node.items.iter().flat_map(|fact| &fact.value).collect(),
            Self::Table(node) => node
                .query
                .iter()
                .flat_map(|form| &form.children)
                .chain(node.rows.iter().flat_map(|row| &row.cells))
                .chain(boxed(&node.empty))
                .collect(),
            Self::Link(node) => node.children.iter().collect(),
            Self::Menu(node) => node.items.iter().collect(),
            Self::Form(node) => node.children.iter().collect(),
            Self::Sheet(node) => node.children.iter().chain(&node.actions).collect(),
            Self::Dialog(node) => node.children.iter().chain(&node.actions).collect(),
            Self::Popover(node) => node.children.iter().collect(),
            Self::Native(node) => boxed(&node.alt).collect(),
            Self::Unknown(node) => boxed(&node.alt).collect(),
            Self::Divider(_)
            | Self::Spacer(_)
            | Self::Text(_)
            | Self::Rich(_)
            | Self::Image(_)
            | Self::Icon(_)
            | Self::Avatar(_)
            | Self::Badge(_)
            | Self::Progress(_)
            | Self::Chart(_)
            | Self::Code(_)
            | Self::Button(_)
            | Self::Field(_)
            | Self::Web(_) => Vec::new(),
        }
    }

    /// The first node with this id, here or anywhere inside.
    pub fn find(&self, id: &str) -> Option<&Node> {
        if self.id() == Some(id) {
            return Some(self);
        }
        self.children().into_iter().find_map(|child| child.find(id))
    }

    /// A text node holding `value`, in the body style.
    pub fn text(value: impl Into<String>) -> Self {
        Self::Text(Text {
            value: value.into(),
            ..Text::default()
        })
    }
}

/// What a control means, wherever a system puts it. A client places a screen's
/// actions by role, never by the order they came in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Role {
    /// No role of its own.
    #[default]
    None,
    /// The one thing this screen or form is for.
    Primary,
    /// Something else a reader may do here.
    Secondary,
    /// It deletes or can't be undone.
    Destructive,
    /// It leaves without doing anything.
    Cancel,
    /// It agrees to what a dialog asks.
    Confirm,
    /// It starts a new thing.
    Create,
    /// It goes somewhere.
    Navigation,
}

/// What a colour means.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Tone {
    /// No meaning.
    #[default]
    Neutral,
    /// The app's accent.
    Accent,
    /// It worked, or it is well.
    Success,
    /// It needs attention.
    Warning,
    /// It failed, or it is dangerous.
    Danger,
    /// For your information.
    Info,
}

/// How loud a control is among its neighbours.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Prominence {
    /// The loudest: one a group.
    High,
    /// The usual.
    #[default]
    Medium,
    /// Quiet: text or an icon alone.
    Low,
}

/// The direction a stack runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    /// Down the screen.
    #[default]
    Vertical,
    /// Across it. A client may wrap or stack what does not fit.
    Horizontal,
}

/// Where parts sit across a stack's axis, or text in a column.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    /// At the start: the left, where text runs left to right.
    Start,
    /// In the middle.
    Center,
    /// At the end.
    End,
    /// Stretched from start to end.
    Stretch,
    /// On the text's baseline.
    Baseline,
}

/// How a stack shares the room left along its axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Justify {
    /// Parts at the start.
    Start,
    /// Parts in the middle.
    Center,
    /// Parts at the end.
    End,
    /// The first at the start, the last at the end, the room shared between.
    Between,
}

/// Parts one after another.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stack {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Down or across.
    #[serde(default, skip_serializing_if = "is_default")]
    pub axis: Axis,
    /// The room between parts, in steps of the system's spacing scale: `0` is none.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub gap: u8,
    /// Where parts sit across the axis.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<Align>,
    /// How the room along the axis is shared.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub justify: Option<Justify>,
    /// Whether parts that do not fit go to a new line.
    #[serde(default, skip_serializing_if = "is_false")]
    pub wrap: bool,
    /// Whether it is not drawn until a command shows it.
    #[serde(default, skip_serializing_if = "is_false")]
    pub hidden: bool,
    /// The parts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Node>,
}

/// Parts in columns.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grid {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The most columns, on the widest screen. A narrow screen draws fewer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cols: Option<u8>,
    /// The least a column may be wide, in points.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<u16>,
    /// The room between parts, in steps of the system's spacing scale.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub gap: u8,
    /// The parts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Node>,
}

/// How a section is set off from what is around it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SectionStyle {
    /// A heading and its parts, with nothing around them.
    #[default]
    Plain,
    /// A card.
    Card,
    /// A group inset from the screen's edges, as settings are.
    Inset,
}

/// A titled group of parts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Section {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The heading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// A line under the heading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    /// How it is set off.
    #[serde(default, skip_serializing_if = "is_default")]
    pub style: SectionStyle,
    /// What a reader may do with the whole section, beside its heading.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<Node>,
    /// The parts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Node>,
    /// What stands under the parts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub footer: Vec<Node>,
}

/// Rows that may go on.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct List {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The rows: [`Row`]s, or any other parts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Node>,
    /// Where the next rows are, by cursor. The answer is an `append` op for this
    /// list, with the address after it or none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub more: Option<String>,
    /// Whether the rows are numbered.
    #[serde(default, skip_serializing_if = "is_false")]
    pub ordered: bool,
}

/// One row of a list: who or what, a line about it, and what stands at each end.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Row {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// What stands before the words: an icon, an avatar, a picture.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leading: Option<Box<Node>>,
    /// The row's name.
    pub title: String,
    /// A line under the name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    /// More under the two lines, when a row holds more than words.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub detail: Vec<Node>,
    /// What stands after the words: a value, a time, a control.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trailing: Option<Box<Node>>,
    /// A badge beside the name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge: Option<Badge>,
    /// How far along the thing the row stands for is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<RowState>,
    /// What a tap on the row does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<Action>,
    /// What a swipe on the row offers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub swipe: Vec<Button>,
    /// What a long press or a secondary click offers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub menu: Vec<Node>,
}

/// Where a row stands in a sequence, such as the steps of an order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RowState {
    /// It has happened.
    Done,
    /// It is happening now.
    Current,
    /// It is still to come.
    Upcoming,
}

/// A primary pane and its detail: side by side where there is room, one over the
/// other where there is not.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Split {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The pane a reader chooses in.
    pub primary: Box<Node>,
    /// The pane that shows what was chosen.
    pub detail: Box<Node>,
}

/// Views a reader switches between.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tabs {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The tabs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<Tab>,
}

/// One tab: a view held here, or a link to the page that holds it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tab {
    /// The tab's name.
    pub label: String,
    /// Its icon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<Icon>,
    /// Whether it is the one shown.
    #[serde(default, skip_serializing_if = "is_false")]
    pub selected: bool,
    /// The page it leads to, when its view is not held here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub href: Option<String>,
    /// Its view.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Node>,
}

/// A part that opens and closes under its label.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Disclosure {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// What a reader taps.
    pub label: String,
    /// Whether it starts open.
    #[serde(default, skip_serializing_if = "is_false")]
    pub open: bool,
    /// What it holds.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Node>,
}

/// A line between parts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Divider {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

/// Room that takes what is left along a stack.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spacer {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

/// A part that fetches itself from `src`: once, after the screen is drawn
/// (`lazy`), and again whenever its stream speaks (`live`). The answer is an
/// envelope whose screen's body takes the place of the region's children.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Region {
    /// The region's id.
    pub id: String,
    /// Where its content comes from.
    pub src: String,
    /// Whether it is fetched once the screen is drawn, because it came empty.
    #[serde(default, skip_serializing_if = "is_false")]
    pub lazy: bool,
    /// The stream that says when to fetch it again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live: Option<Live>,
    /// What it holds now.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Node>,
}

/// A stream of server-sent events a region listens to.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Live {
    /// The stream's address.
    pub src: String,
    /// The event that means "fetch again". Any message does when there is none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,
}

/// The style of a piece of text, by what it is for. Each system has its own
/// sizes and faces for these.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum TextStyle {
    /// The name of a screen.
    LargeTitle,
    /// The heading of a part of a screen.
    Title,
    /// A heading inside a part.
    Headline,
    /// Running text.
    #[default]
    Body,
    /// Text set a little apart from the body.
    Callout,
    /// Small text under or beside something.
    Caption,
    /// Fixed-width text.
    Mono,
}

/// Text in one style.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Text {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The words.
    pub value: String,
    /// What the text is for.
    #[serde(default, skip_serializing_if = "is_default")]
    pub style: TextStyle,
    /// What its colour means.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone: Option<Tone>,
    /// Whether it is secondary: drawn quieter than the text around it.
    #[serde(default, skip_serializing_if = "is_false")]
    pub muted: bool,
    /// The most lines it takes before it is cut short.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lines: Option<u8>,
    /// The level of the heading it is, from 1, for assistive technology.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading: Option<u8>,
}

/// Text in runs.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rich {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The runs, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runs: Vec<Run>,
    /// What the text is for.
    #[serde(default, skip_serializing_if = "is_default")]
    pub style: TextStyle,
    /// Whether it is secondary.
    #[serde(default, skip_serializing_if = "is_false")]
    pub muted: bool,
}

/// One run of rich text.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    /// The words.
    pub text: String,
    /// Bold.
    #[serde(default, skip_serializing_if = "is_false")]
    pub bold: bool,
    /// Italic.
    #[serde(default, skip_serializing_if = "is_false")]
    pub italic: bool,
    /// Code.
    #[serde(default, skip_serializing_if = "is_false")]
    pub code: bool,
    /// Where the run leads, when it is a link.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub href: Option<String>,
}

/// How a picture fills its box.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fit {
    /// All of the picture shows.
    Contain,
    /// The box is filled, and the picture's edges may be cut.
    Cover,
}

/// A picture.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Image {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Where the picture is.
    pub src: String,
    /// What it shows, for a reader who can't see it. Empty for decoration.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub alt: String,
    /// Its width, in points, when the page gave one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u16>,
    /// Its height, in points, when the page gave one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u16>,
    /// How it fills its box.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fit: Option<Fit>,
}

/// An icon: path data a client strokes and fills, and the name a system symbol
/// can be found by.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Icon {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The icon's name in its set, such as `shopping-bag`, when it is one of the
    /// toolkit's own. A client that knows a system symbol for the name draws
    /// that; any other draws the paths.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The shapes a client strokes, each as SVG path data (`M3 6h18…`): two
    /// units wide in a box of 24, with round ends and joins.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
    /// The shapes a client fills, each as SVG path data. An icon may hold both
    /// kinds: an outline with a solid dot in it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fills: Vec<String>,
    /// The box the paths are drawn in, as `min-x min-y width height`. `0 0 24 24`
    /// when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view_box: Option<String>,
    /// What the icon says, when it stands alone. A decoration has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// What its colour means.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone: Option<Tone>,
}

/// A person's picture, or their initials when there is none.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Avatar {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Whose it is.
    pub name: String,
    /// The picture.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub src: Option<String>,
    /// The initials the page drew.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initials: Option<String>,
}

/// A short label with a tone.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Badge {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The words.
    pub text: String,
    /// What its colour means.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone: Option<Tone>,
    /// Whether a dot in the tone's colour stands before the words.
    #[serde(default, skip_serializing_if = "is_false")]
    pub dot: bool,
}

/// How far something has got.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Progress {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// How far, in thousandths. Nothing when it is not known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<u16>,
    /// What is progressing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// A message in place.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Banner {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// What it means.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone: Option<Tone>,
    /// Its heading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Its words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Its icon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<Icon>,
    /// What a reader may do about it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<Node>,
}

/// What a place shows when it holds nothing.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Empty {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Its icon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<Icon>,
    /// What is missing.
    pub title: String,
    /// What to do about it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The ways on from here.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<Node>,
}

/// Labels with their values.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Facts {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The facts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<Fact>,
}

/// One fact: a label, its value, and lines of detail under it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fact {
    /// What the value is.
    pub label: String,
    /// The value: words, or parts such as a flag beside a city.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub value: Vec<Node>,
    /// Its icon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<Icon>,
    /// What its colour means.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone: Option<Tone>,
    /// Lines under the value.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<String>,
}

/// What a column is to its row, so a narrow screen can draw each record as a
/// row of a list: its name, a line under it, what stands at its end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ColumnRole {
    /// The record's name.
    Title,
    /// A line about it, under the name.
    Subtitle,
    /// Its state, as a badge.
    Status,
    /// What stands at the row's end: an amount, a time.
    Trailing,
    /// Controls that act on the record.
    Actions,
}

/// The way a column is sorted now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortDir {
    /// Smallest first.
    Asc,
    /// Largest first.
    Desc,
}

/// How a column sorts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sort {
    /// The page a tap on the heading asks for.
    pub href: String,
    /// The way the table is sorted by this column now, when it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir: Option<SortDir>,
}

/// One column of a table.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Column {
    /// The column's name in the data, when the page gave one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// Its heading.
    pub label: String,
    /// Whether it holds numbers, set at the end and in figures of one width.
    #[serde(default, skip_serializing_if = "is_false")]
    pub numeric: bool,
    /// Where its cells sit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<Align>,
    /// How it sorts, when it does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort: Option<Sort>,
    /// What it is to its row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<ColumnRole>,
    /// Whether a narrow screen may leave it out.
    #[serde(default, skip_serializing_if = "is_false")]
    pub secondary: bool,
}

/// One record of a table.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableRow {
    /// The record's id: the row's key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// One cell a column, in the columns' order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cells: Vec<Node>,
    /// What a tap on the row does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<Action>,
}

/// Records in columns. A wide screen draws a table; a narrow one draws each
/// record as a row, from the columns' roles.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Table {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// What the table lists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
    /// The columns.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<Column>,
    /// The records.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<TableRow>,
    /// The form that asks for other records: its search field (a text control
    /// of the `search` kind) and one field a filter. A client sends it as a
    /// query when a value changes, and draws the answer in the table's place.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<Form>,
    /// Where the next records are, by cursor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub more: Option<String>,
    /// Where the records before these are.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev: Option<String>,
    /// How many there are, in the page's words: `1–25 of 1,204`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// What stands in place of the records when there are none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub empty: Option<Box<Node>>,
}

/// The kind of a chart.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ChartKind {
    /// Lines over a shared axis.
    #[default]
    Line,
    /// Lines with the area under them filled.
    Area,
    /// Bars.
    Bar,
    /// Shares of a whole.
    Donut,
    /// A small line with no axes.
    Sparkline,
    /// Stages that narrow.
    Funnel,
}

/// One series of a chart.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Series {
    /// The series' name.
    pub label: String,
    /// Its values, one a label; a gap where there is none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<Option<serde_json::Number>>,
    /// Its values in the page's words (`₹1,24,050`), one a label, when the page
    /// formatted them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shown: Vec<String>,
}

/// A chart, as the numbers it draws. The client draws it with its system's own
/// charts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chart {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The kind.
    #[serde(default, skip_serializing_if = "is_default")]
    pub kind: ChartKind,
    /// What it shows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The labels along its axis, or of its shares.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    /// The series.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub series: Vec<Series>,
}

/// Code, in a fixed-width face.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Code {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The code.
    pub text: String,
    /// Its language, when the page said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
}

/// A button.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Button {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// What it says. A button drawn as an icon alone still has its label, for
    /// assistive technology.
    pub label: String,
    /// Its icon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<Icon>,
    /// Whether the page drew the icon alone.
    #[serde(default, skip_serializing_if = "is_false")]
    pub icon_only: bool,
    /// What it means.
    #[serde(default, skip_serializing_if = "is_default")]
    pub role: Role,
    /// How loud it is.
    #[serde(default, skip_serializing_if = "is_default")]
    pub prominence: Prominence,
    /// Whether it can't be used now.
    #[serde(default, skip_serializing_if = "is_false")]
    pub disabled: bool,
    /// Whether it is waiting for an answer.
    #[serde(default, skip_serializing_if = "is_false")]
    pub pending: bool,
    /// What it does. A button with none does nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<Action>,
}

/// A link: words, or parts that lead somewhere together.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// What it says: its words, or its name for assistive technology.
    pub label: String,
    /// What it does.
    pub action: Action,
    /// What it holds, when that is more than its words.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Node>,
}

/// A button that opens a list of actions.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Menu {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// What the button says.
    pub label: String,
    /// Its icon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<Icon>,
    /// The items: buttons, links, sections of them and dividers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<Node>,
}

/// How a form is sent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    /// As a query: the answer is a screen.
    #[default]
    Get,
    /// As a post: the answer is ops.
    Post,
}

/// A value a form sends that the reader never sees.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hidden {
    /// The field's name.
    pub name: String,
    /// Its value.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub value: String,
    /// The field's id, when commands may set it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

/// A form. The client holds its fields and sends them, with the hidden values,
/// as `application/x-www-form-urlencoded`, or as `multipart/form-data` when a
/// field holds a file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Form {
    /// The form's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// How it is sent.
    #[serde(default, skip_serializing_if = "is_default")]
    pub method: Method,
    /// Where it is sent. The screen's own address when empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub action: String,
    /// What it sends that the reader never sees: its token among them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden: Vec<Hidden>,
    /// Its fields and whatever stands among them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Node>,
}

/// One field of a form.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Field {
    /// The id of the field's control: what a `focus` names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The name its value is sent under.
    pub name: String,
    /// Its label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// A line of help under it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
    /// What is wrong with its value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Whether it must be filled.
    #[serde(default, skip_serializing_if = "is_false")]
    pub required: bool,
    /// Whether it can't be changed now.
    #[serde(default, skip_serializing_if = "is_false")]
    pub disabled: bool,
    /// The control.
    pub input: Input,
}

/// How much of the screen a sheet takes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SheetSize {
    /// About half.
    Medium,
    /// Most of it.
    #[default]
    Large,
    /// All of it.
    Full,
}

/// A sheet over the screen. It is part of the tree where the page put it and
/// draws nothing there: an `open` command or op presents it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sheet {
    /// The sheet's id.
    pub id: String,
    /// Its title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// A line under the title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    /// How much of the screen it takes.
    #[serde(default, skip_serializing_if = "is_default")]
    pub size: SheetSize,
    /// Whether it is presented as the screen arrives.
    #[serde(default, skip_serializing_if = "is_false")]
    pub open: bool,
    /// What it holds.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Node>,
    /// The actions at its foot, by role.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<Node>,
}

/// A dialog. Like a sheet, it draws nothing until it is opened.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dialog {
    /// The dialog's id.
    pub id: String,
    /// What it asks or says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// More about it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Whether it interrupts to ask before something that can't be undone: the
    /// system's alert, where it has one.
    #[serde(default, skip_serializing_if = "is_false")]
    pub alert: bool,
    /// Whether it is presented as the screen arrives.
    #[serde(default, skip_serializing_if = "is_false")]
    pub open: bool,
    /// What it holds besides its words: a form, most often.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Node>,
    /// Its answers, by role.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<Node>,
}

/// A small panel anchored to what opened it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Popover {
    /// The popover's id.
    pub id: String,
    /// What it holds.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Node>,
}

/// A plugin's own view. A client that has no view of this name draws `alt`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeView {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The view's name.
    pub name: String,
    /// What the view is given.
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub props: serde_json::Value,
    /// What stands in its place where the view is missing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt: Option<Box<Node>>,
}

/// HTML for a small region, drawn in a web view the height of its content.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebRegion {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The markup.
    pub html: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::Present;

    #[test]
    fn a_node_is_an_object_named_by_its_kind_with_only_what_it_holds() {
        let node = Node::Stack(Stack {
            gap: 2,
            children: vec![
                Node::text("Orders"),
                Node::Button(Button {
                    label: "New order".to_owned(),
                    role: Role::Create,
                    action: Some(Action::Navigate {
                        href: "/orders/new".to_owned(),
                        present: Present::Sheet,
                    }),
                    ..Button::default()
                }),
                Node::Divider(Divider::default()),
            ],
            ..Stack::default()
        });
        let json = serde_json::to_string(&node).unwrap();
        assert_eq!(
            json,
            concat!(
                r#"{"t":"stack","gap":2,"children":["#,
                r#"{"t":"text","value":"Orders"},"#,
                r#"{"t":"button","label":"New order","role":"create","#,
                r#""action":{"t":"navigate","href":"/orders/new","present":"sheet"}},"#,
                r#"{"t":"divider"}]}"#
            )
        );
        assert_eq!(serde_json::from_str::<Node>(&json).unwrap(), node);
    }

    #[test]
    fn a_field_a_client_does_not_know_is_passed_over() {
        let node: Node =
            serde_json::from_str(r#"{"t":"badge","text":"Paid","tone":"success","glow":3}"#)
                .unwrap();
        assert_eq!(
            node,
            Node::Badge(Badge {
                text: "Paid".to_owned(),
                tone: Some(Tone::Success),
                ..Badge::default()
            })
        );
    }

    #[test]
    fn a_kind_outside_the_vocabulary_is_kept_with_its_alt() {
        let node: Node = serde_json::from_str(
            r#"{"t":"marquee","id":"news","speed":3,"alt":{"t":"text","value":"Diwali sale","glow":1}}"#,
        )
        .unwrap();
        assert_eq!(
            node,
            Node::Unknown(Unknown {
                kind: "marquee".to_owned(),
                id: Some("news".to_owned()),
                alt: Some(Box::new(Node::text("Diwali sale"))),
            })
        );
        assert_eq!(node.kind(), crate::Kind::Unknown);
        assert_eq!(node.id(), Some("news"));
        assert_eq!(node.children(), vec![&Node::text("Diwali sale")]);
        let bare: Node = serde_json::from_str(r#"{"t":"marquee"}"#).unwrap();
        assert_eq!(
            bare,
            Node::Unknown(Unknown {
                kind: "marquee".to_owned(),
                ..Unknown::default()
            })
        );
    }

    #[test]
    fn an_unknown_kind_writes_itself_back_as_it_came() {
        let json = r#"{"t":"marquee","id":"news","alt":{"t":"text","value":"Diwali sale"}}"#;
        let node: Node = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&node).unwrap(), json);
    }

    #[test]
    fn a_node_that_names_no_kind_is_refused() {
        assert!(serde_json::from_str::<Node>(r#"{"text":"x"}"#).is_err());
        assert!(serde_json::from_str::<Node>(r#"{"t":7}"#).is_err());
        assert!(serde_json::from_str::<Node>(r#""text""#).is_err());
    }

    #[test]
    fn a_known_kind_that_is_malformed_is_refused() {
        // A known kind is held to its shape: only a kind this build has never
        // heard of is read as unknown.
        assert!(serde_json::from_str::<Node>(r#"{"t":"text"}"#).is_err());
        assert!(serde_json::from_str::<Node>(r#"{"t":"badge","text":7}"#).is_err());
    }

    #[test]
    fn a_node_is_found_by_its_id_however_deep() {
        let dialog = Node::Dialog(Dialog {
            id: "refund".to_owned(),
            title: Some("Refund".to_owned()),
            ..Dialog::default()
        });
        let tree = Node::Section(Section {
            children: vec![Node::Sheet(Sheet {
                id: "order".to_owned(),
                children: vec![dialog.clone()],
                ..Sheet::default()
            })],
            ..Section::default()
        });
        assert_eq!(tree.find("refund"), Some(&dialog));
        assert!(tree.find("order").is_some());
        assert!(tree.find("cancel").is_none());
    }
}
