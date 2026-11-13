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
pub use crate::charts::{
    CalendarHeatmap, Category, Centre, ClockMoment, ColorStrip, Colour, Curve, DayClock, DayPager,
    DayPick, HeatDay, Hint, HintMark, Hour, Hue, MatrixHeatmap, Measure, Meter, MiniColumn,
    MiniColumns, PagerWords, Point, PointSeries, RangeScale, Reference, ReferenceKind, Relation, Ring,
    RingValue, Rings, RingsCentre, Scatter, Segment, Stacking, StripCell, Zone,
};
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
    /// Actions on a bar, each with words and a symbol.
    Toolbar(Toolbar),
    /// A value within a range, read at a glance.
    Gauge(Gauge),
    /// A key figure and which way it is going.
    Figure(Figure),
    /// Moments in order: what has happened and what is to come.
    Timeline(Timeline),
    /// One entry of an activity feed: who did what, and when.
    FeedRow(FeedRow),
    /// One message of a conversation.
    Message(Message),
    /// A pane of a split screen, with the part it plays.
    Pane(Pane),
    /// One value against its whole, as a ring.
    Ring(Ring),
    /// Several values on rings inside each other.
    Rings(Rings),
    /// A day as 24 hours round a dial, with its moments.
    DayClock(DayClock),
    /// The control that steps a screen from day to day.
    DayPager(DayPager),
    /// One bar split into segments that add up to the whole.
    Meter(Meter),
    /// Small multiples: a column for each item, each split into the same parts.
    Columns(MiniColumns),
    /// A row of cells, each in the colour of its category.
    Strip(ColorStrip),
    /// Where one value sits on a scale between a low and a high end.
    Range(RangeScale),
    /// A calendar of days, each shaded by its value.
    CalendarHeatmap(CalendarHeatmap),
    /// Every measure against every other, each pair shaded by how they relate.
    MatrixHeatmap(MatrixHeatmap),
    /// A point for each pair of values.
    Scatter(Scatter),
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
            Self::Toolbar(node) => node.id.as_deref(),
            Self::Gauge(node) => node.id.as_deref(),
            Self::Figure(node) => node.id.as_deref(),
            Self::Timeline(node) => node.id.as_deref(),
            Self::FeedRow(node) => node.id.as_deref(),
            Self::Message(node) => node.id.as_deref(),
            Self::Pane(node) => node.id.as_deref(),
            Self::Ring(node) => node.id.as_deref(),
            Self::Rings(node) => node.id.as_deref(),
            Self::DayClock(node) => node.id.as_deref(),
            Self::DayPager(node) => node.id.as_deref(),
            Self::Meter(node) => node.id.as_deref(),
            Self::Columns(node) => node.id.as_deref(),
            Self::Strip(node) => node.id.as_deref(),
            Self::Range(node) => node.id.as_deref(),
            Self::CalendarHeatmap(node) => node.id.as_deref(),
            Self::MatrixHeatmap(node) => node.id.as_deref(),
            Self::Scatter(node) => node.id.as_deref(),
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
                .chain(
                    node.rows
                        .iter()
                        .flat_map(TableRow::nodes),
                )
                .chain(boxed(&node.empty))
                .collect(),
            Self::Link(node) => node.children.iter().collect(),
            Self::Figure(node) => boxed(&node.aside).chain(boxed(&node.chart)).collect(),
            Self::DayPager(node) => boxed(&node.scores).collect(),
            Self::Menu(node) => node.items.iter().collect(),
            Self::Form(node) => node.children.iter().collect(),
            Self::Sheet(node) => node.children.iter().chain(&node.actions).collect(),
            Self::Dialog(node) => node.children.iter().chain(&node.actions).collect(),
            Self::Popover(node) => node.children.iter().collect(),
            Self::Native(node) => boxed(&node.alt).collect(),
            Self::Toolbar(node) => node.items.iter().flat_map(|item| &item.menu).collect(),
            Self::Timeline(node) => node.items.iter().flat_map(|item| &item.detail).collect(),
            Self::FeedRow(node) => node.detail.iter().collect(),
            Self::Message(node) => node.body.iter().collect(),
            Self::Pane(node) => node.children.iter().collect(),
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
            | Self::Web(_)
            | Self::Gauge(_)
            | Self::Ring(_)
            | Self::Rings(_)
            | Self::DayClock(_)
            | Self::Meter(_)
            | Self::Columns(_)
            | Self::Strip(_)
            | Self::Range(_)
            | Self::CalendarHeatmap(_)
            | Self::MatrixHeatmap(_)
            | Self::Scatter(_) => Vec::new(),
        }
    }

    /// The nodes directly inside this one, to change in place: the same nodes,
    /// in the same order, as [`children`](Self::children).
    pub fn children_mut(&mut self) -> Vec<&mut Node> {
        fn boxed(node: &mut Option<Box<Node>>) -> impl Iterator<Item = &mut Node> {
            node.iter_mut().map(Box::as_mut)
        }
        match self {
            Self::Stack(node) => node.children.iter_mut().collect(),
            Self::Grid(node) => node.children.iter_mut().collect(),
            Self::Section(node) => node
                .actions
                .iter_mut()
                .chain(&mut node.children)
                .chain(&mut node.footer)
                .collect(),
            Self::List(node) => node.children.iter_mut().collect(),
            Self::Row(node) => boxed(&mut node.leading)
                .chain(&mut node.detail)
                .chain(boxed(&mut node.trailing))
                .chain(&mut node.menu)
                .collect(),
            Self::Split(node) => vec![&mut *node.primary, &mut *node.detail],
            Self::Tabs(node) => node
                .items
                .iter_mut()
                .flat_map(|tab| &mut tab.children)
                .collect(),
            Self::Disclosure(node) => node.children.iter_mut().collect(),
            Self::Region(node) => node.children.iter_mut().collect(),
            Self::Banner(node) => node.actions.iter_mut().collect(),
            Self::Empty(node) => node.actions.iter_mut().collect(),
            Self::Facts(node) => node
                .items
                .iter_mut()
                .flat_map(|fact| &mut fact.value)
                .collect(),
            Self::Table(node) => node
                .query
                .iter_mut()
                .flat_map(|form| &mut form.children)
                .chain(
                    node.rows
                        .iter_mut()
                        .flat_map(TableRow::nodes_mut),
                )
                .chain(boxed(&mut node.empty))
                .collect(),
            Self::Link(node) => node.children.iter_mut().collect(),
            Self::Figure(node) => boxed(&mut node.aside).chain(boxed(&mut node.chart)).collect(),
            Self::DayPager(node) => boxed(&mut node.scores).collect(),
            Self::Menu(node) => node.items.iter_mut().collect(),
            Self::Form(node) => node.children.iter_mut().collect(),
            Self::Sheet(node) => node
                .children
                .iter_mut()
                .chain(&mut node.actions)
                .collect(),
            Self::Dialog(node) => node
                .children
                .iter_mut()
                .chain(&mut node.actions)
                .collect(),
            Self::Popover(node) => node.children.iter_mut().collect(),
            Self::Native(node) => boxed(&mut node.alt).collect(),
            Self::Toolbar(node) => node
                .items
                .iter_mut()
                .flat_map(|item| &mut item.menu)
                .collect(),
            Self::Timeline(node) => node
                .items
                .iter_mut()
                .flat_map(|item| &mut item.detail)
                .collect(),
            Self::FeedRow(node) => node.detail.iter_mut().collect(),
            Self::Message(node) => node.body.iter_mut().collect(),
            Self::Pane(node) => node.children.iter_mut().collect(),
            Self::Unknown(node) => boxed(&mut node.alt).collect(),
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
            | Self::Web(_)
            | Self::Gauge(_)
            | Self::Ring(_)
            | Self::Rings(_)
            | Self::DayClock(_)
            | Self::Meter(_)
            | Self::Columns(_)
            | Self::Strip(_)
            | Self::Range(_)
            | Self::CalendarHeatmap(_)
            | Self::MatrixHeatmap(_)
            | Self::Scatter(_) => Vec::new(),
        }
    }

    /// How many nodes deep the tree is: 1 for a node with none inside it. A
    /// server answers a tree deeper than [`MAX_DEPTH`](crate::MAX_DEPTH) with
    /// the web view. It walks with a list of its own, so any depth is measured.
    pub fn depth(&self) -> usize {
        let mut deepest = 0;
        let mut stack = vec![(self, 1)];
        while let Some((node, depth)) = stack.pop() {
            deepest = deepest.max(depth);
            stack.extend(node.children().into_iter().map(|child| (child, depth + 1)));
        }
        deepest
    }

    /// The first node with this id, here or anywhere inside, in the order a
    /// reader meets them. It walks with a list of its own, so a tree of any
    /// depth is searched without using up the stack.
    pub fn find(&self, id: &str) -> Option<&Node> {
        let mut stack = vec![self];
        while let Some(node) = stack.pop() {
            if node.id() == Some(id) {
                return Some(node);
            }
            stack.extend(node.children().into_iter().rev());
        }
        None
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

/// How loud a control, or a chart, is among its neighbours.
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
    #[serde(default, skip_serializing_if = "is_default", deserialize_with = "crate::lenient::or_default")]
    pub axis: Axis,
    /// The room between parts, in steps of the system's spacing scale: `0` is none.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub gap: u8,
    /// Where parts sit across the axis.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "crate::lenient::or_none")]
    pub align: Option<Align>,
    /// How the room along the axis is shared.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "crate::lenient::or_none")]
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
    #[serde(default, skip_serializing_if = "is_default", deserialize_with = "crate::lenient::or_default")]
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
    /// A note about what the section shows, shown when asked: a client puts its
    /// mark beside the heading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<Hint>,
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
    /// Where the rows before the first are, by cursor: newer ones above a list
    /// of the newest first. The answer is a `prepend` op for this list, with
    /// the address before it or none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    /// Whether the rows are numbered.
    #[serde(default, skip_serializing_if = "is_false")]
    pub ordered: bool,
    /// The revision the rows are at: what the list asks `changes` since. Every
    /// row of a list with a revision has an `id`: an op merges a row into the
    /// list by it, and a row with none can't be merged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
    /// Where the list asks what changed: this address with `since` added as a
    /// query value. The answer is a `merge` op for this list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changes: Option<String>,
    /// The stream that says when to ask `changes`. Its messages are a nudge:
    /// nothing in them is read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live: Option<Live>,
    /// Whether the rows can be moved by dragging, and where a move is posted.
    /// Every row of such a list has an `id`: a row with none can't be moved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reorder: Option<Reorder>,
}

/// How the rows of a list are moved by dragging: within the list, and into
/// the lists of the same `group` (the columns of a board).
///
/// A client offers a move only for a row that has an `id`, and only into a list
/// of the same group that is not already full (see `limit`). It moves the row
/// on its own, for the feel of it, and then posts the move to `href` as a form,
/// as an action's post is sent. A move from one list into another is posted to
/// the list it joined, to that list's `href`. The fields, then `fields`, are:
///
/// - `item`: the row that moved, by its `key`, or by its `id` when it has no
///   key. [`Reorder::ITEM`].
/// - `from`: the list it left, by that list's `reorder.key`, or by its `id`
///   when it has none. [`Reorder::FROM`].
/// - `to`: the list it joined, named the same way; the same as `from` for a
///   move within a list. [`Reorder::TO`].
/// - `before`: the row it now stands before, named as `item` is; empty when it
///   stands at the end. [`Reorder::BEFORE`].
/// - `position`: where it now stands in the list it joined, counted from 0,
///   among the rows the client holds: a list that loads by cursor holds only
///   the first ones, so `before` is the place the server keeps. [`Reorder::POSITION`].
///
/// The answer is ops, as a post's are. The server is the judge of a move:
/// `limit` is advice to the client, which offers no drop into a list that
/// holds that many, and a move the server refuses (a `4xx`, with a toast or
/// errors saying why) puts the row back where it was.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reorder {
    /// Where a move is posted.
    pub href: String,
    /// The list's own key, as the app knows it (the status of the cards in a
    /// column: `doing`), which a move posts in place of the list's `id`. Left
    /// out, the `id` is posted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// The lists a row may move between: those with the same group. A list
    /// with none keeps its rows to itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// The most rows the list takes: a full list takes no more.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// What goes with each move: the page's token among them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<crate::action::Pair>,
}

impl Reorder {
    /// The field a move posts the row that moved under.
    pub const ITEM: &'static str = "item";
    /// The field a move posts the list the row left under.
    pub const FROM: &'static str = "from";
    /// The field a move posts the list the row joined under.
    pub const TO: &'static str = "to";
    /// The field a move posts the row it now stands before under.
    pub const BEFORE: &'static str = "before";
    /// The field a move posts its place in the list it joined under.
    pub const POSITION: &'static str = "position";
}

/// One row of a list: who or what, a line about it, and what stands at each end.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Row {
    /// The node's id. A row of a list that is merged into by revision or moved
    /// by dragging has one, and a row with none can be neither.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The record's own key, as the app knows it (`7`, the card's key in its
    /// store), which a move posts in place of the row's `id`; the `id` names the
    /// row on the screen. Left out, the `id` is posted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "crate::lenient::or_none")]
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
    /// Whether it is drawn quieter than its neighbours: what a machine did, in
    /// a list of what people did.
    #[serde(default, skip_serializing_if = "is_false")]
    pub muted: bool,
}

/// Where a row stands in a sequence, such as the steps of an order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RowState {
    /// It has happened.
    Done,
    /// It is happening now; in a list beside what it opens, the row that is
    /// open now (the conversation shown beside the inbox).
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

/// Views a reader switches between. The tab that is `selected` is the one that
/// shows. A lazy [`Region`] inside a tab that is not selected is not fetched
/// with the screen: the client fetches it when its tab is shown, as it fetches
/// the regions of a screen when the screen is.
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
    /// Whether it is the one shown. A lazy region in the view of a tab that is
    /// not selected waits until its tab is.
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
    #[serde(default, skip_serializing_if = "is_default", deserialize_with = "crate::lenient::or_default")]
    pub style: TextStyle,
    /// What its colour means.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "crate::lenient::or_none")]
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

/// Text in runs: one paragraph of them, or several blocks.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rich {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The runs of a single paragraph, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runs: Vec<Run>,
    /// The blocks of a longer text, in order: paragraphs, headings, quotes,
    /// items of lists and code. A rich node holds runs or blocks, not both.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<Block>,
    /// What the text is for.
    #[serde(default, skip_serializing_if = "is_default", deserialize_with = "crate::lenient::or_default")]
    pub style: TextStyle,
    /// Whether it is secondary.
    #[serde(default, skip_serializing_if = "is_false")]
    pub muted: bool,
}

/// What a block of rich text is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum BlockKind {
    /// A paragraph.
    #[default]
    Paragraph,
    /// A heading.
    Heading,
    /// Words quoted from elsewhere.
    Quote,
    /// An item of a list whose items are marked, not counted.
    Bullet,
    /// An item of a list whose items are counted.
    Numbered,
    /// Code, in a fixed-width face, its lines kept as they are.
    Code,
}

/// One block of rich text.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Block {
    /// What it is.
    #[serde(default, skip_serializing_if = "is_default", deserialize_with = "crate::lenient::or_default")]
    pub kind: BlockKind,
    /// Its runs, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runs: Vec<Run>,
    /// A heading's level, from 1; how deep an item of a list sits, from 0.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub level: u8,
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
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "crate::lenient::or_none")]
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
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "crate::lenient::or_none")]
    pub tone: Option<Tone>,
    /// The system's own symbol that means the same, where there is one: a
    /// client that has it draws it, filled when it stands for the selected
    /// destination, and the paths otherwise. Set from the name by
    /// [`Icon::with_symbol`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<SystemSymbol>,
}

/// The names of one symbol in each system's own set.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemSymbol {
    /// The SF Symbols name, drawn on Apple systems: `cart`.
    pub apple: String,
    /// The SF Symbols name of its filled form, where it has one: `cart.fill`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apple_filled: Option<String>,
    /// The Material Symbols name, drawn on Android, filled by the set's own
    /// fill: `shopping_cart`. An app bundles the symbols it names when it is
    /// built, since Android can't look one up by name.
    pub material: String,
}

/// The shapes of an icon, without its name: what an envelope's icon table
/// holds by name, so a shape that recurs is sent once.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IconShape {
    /// The shapes a client strokes, as an icon's `paths`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
    /// The shapes a client fills, as an icon's `fills`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fills: Vec<String>,
    /// The box they are drawn in, as an icon's `view_box`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view_box: Option<String>,
}

impl Icon {
    /// The icon's own shapes, when it carries any.
    pub fn shape(&self) -> Option<IconShape> {
        (!self.paths.is_empty() || !self.fills.is_empty()).then(|| IconShape {
            paths: self.paths.clone(),
            fills: self.fills.clone(),
            view_box: self.view_box.clone(),
        })
    }

    /// Gives the icon these shapes.
    pub fn set_shape(&mut self, shape: IconShape) {
        self.paths = shape.paths;
        self.fills = shape.fills;
        self.view_box = shape.view_box;
    }

    /// Takes the icon's shapes away, leaving its name to look them up by.
    pub fn clear_shape(&mut self) {
        self.set_shape(IconShape::default());
    }
}

impl Node {
    /// The icons this node holds itself, not those of the nodes inside it.
    pub fn icons_mut(&mut self) -> Vec<&mut Icon> {
        match self {
            Self::Icon(icon) => vec![icon],
            Self::Tabs(node) => node.items.iter_mut().filter_map(|tab| tab.icon.as_mut()).collect(),
            Self::Row(node) => node
                .swipe
                .iter_mut()
                .filter_map(|button| button.icon.as_mut())
                .collect(),
            Self::Banner(node) => node.icon.iter_mut().collect(),
            Self::Empty(node) => node.icon.iter_mut().collect(),
            Self::Facts(node) => node
                .items
                .iter_mut()
                .filter_map(|fact| fact.icon.as_mut())
                .collect(),
            Self::Button(node) => node.icon.iter_mut().collect(),
            Self::Menu(node) => node.icon.iter_mut().collect(),
            Self::Toolbar(node) => node.items.iter_mut().map(|item| &mut item.icon).collect(),
            Self::Timeline(node) => node
                .items
                .iter_mut()
                .filter_map(|moment| moment.icon.as_mut())
                .collect(),
            Self::FeedRow(node) => node.icon.iter_mut().collect(),
            Self::Figure(node) => node.icon.iter_mut().collect(),
            Self::Ring(node) => node.icon.iter_mut().collect(),
            Self::Rings(node) => node.icon.iter_mut().collect(),
            Self::DayClock(node) => node
                .icon
                .iter_mut()
                .chain(node.moments.iter_mut().filter_map(|moment| moment.icon.as_mut()))
                .collect(),
            _ => Vec::new(),
        }
    }
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
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "crate::lenient::or_none")]
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
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "crate::lenient::or_none")]
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
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "crate::lenient::or_none")]
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
    /// What stands before the name: a picture, an avatar, an icon.
    Leading,
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
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "crate::lenient::or_none")]
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
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "crate::lenient::or_none")]
    pub align: Option<Align>,
    /// How it sorts, when it does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort: Option<Sort>,
    /// What it is to its row.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "crate::lenient::or_none")]
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
    ///
    /// A form in a cell is the row's inline edit. A client draws it as an
    /// action in the cell, named by the form's submit (its label and its icon),
    /// which opens the form as a task ([`Present::Task`](crate::Present::Task))
    /// with its fields, and not as the fields themselves in the cell. The
    /// cell's other nodes are drawn as they are.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cells: Vec<Node>,
    /// What a tap on the row does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<Action>,
    /// What a reader may do with the record: buttons and menus. The row's menu:
    /// a narrow screen offers them from the row's menu and swipe, a wide one at
    /// the row's end, in the column whose role is `actions` when there is one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<Node>,
    /// Whether the row is chosen, in a table whose rows can be
    /// ([`Table::select`]).
    #[serde(default, skip_serializing_if = "is_false")]
    pub selected: bool,
    /// Whether the row can't be chosen, in a table whose rows can be: a record
    /// the actions on the rows chosen do not apply to. A client draws its box as
    /// it draws a control that can't be used now, or leaves it out; choosing
    /// the rows of the page skips it, and its id is never sent with the ones
    /// chosen. A row that can't be chosen still opens and acts as any other.
    #[serde(default, skip_serializing_if = "is_false")]
    pub unchoosable: bool,
    /// Whether the detail shows now, open under the row. A client keeps what the
    /// reader did to it (open or shut) when the server draws the row again, as
    /// it keeps whether the row is chosen.
    #[serde(default, skip_serializing_if = "is_false")]
    pub open: bool,
    /// What opens under the row: more about the record than its cells hold.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub detail: Vec<Node>,
}

impl TableRow {
    /// The nodes the row holds: its cells, then its actions, then its detail.
    pub fn nodes(&self) -> impl Iterator<Item = &Node> {
        self.cells.iter().chain(&self.actions).chain(&self.detail)
    }

    /// The same nodes, to change in place.
    pub fn nodes_mut(&mut self) -> impl Iterator<Item = &mut Node> {
        self.cells
            .iter_mut()
            .chain(&mut self.actions)
            .chain(&mut self.detail)
    }
}

/// How the rows of a table are chosen, for the actions that apply to the rows
/// chosen: the id of each chosen row is sent under `name`, as a box ticked in
/// a form would be, with the form `form`.
///
/// A table with a selection gives every row an `id`: it is what is sent. A row
/// with none can't be chosen, and a client draws no box for it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selection {
    /// The name each chosen row's id is sent under.
    pub name: String,
    /// The form the ids go with: the one whose buttons act on the rows chosen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub form: Option<String>,
    /// Whether the person may choose every record that matches the table's
    /// query, and not only the rows loaded: a table loads by cursor, so what a
    /// page holds is only part of what matches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub across: Option<Across>,
}

/// Every record that matches a table's query, offered to an action on the rows
/// chosen in place of the rows chosen. A client offers it (`Choose all 1,204`)
/// when the person has chosen the rows it holds, as its system's own lists do.
///
/// When the person takes it, the client sends `name` with `value`, as a box
/// ticked in the form would be, in place of the ids of the rows, which are only
/// part of what matches. It keeps to it until the person clears a row, or the
/// table's query changes (the answer then holds the new `across`), and sends the
/// ids again.
///
/// The value is the server's own words for "every record that matches": the
/// query as it stands, or a token that stands for it. The client sends it as it
/// came and never makes one, and the server reads the query from it, not from
/// the rows the client holds.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Across {
    /// The name the value is sent under: `all_matching`.
    pub name: String,
    /// What is sent.
    pub value: String,
    /// How many records match, for the words (`Choose all 1,204`): a client
    /// writes the number as its system and the person's locale do (`1,204`,
    /// `1,00,000`). Left out when the server does not count them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u32>,
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
    /// The form that asks for other records: each field says what it is for
    /// by its [`purpose`](Field::purpose) (the search, a filter, the order, the
    /// rows a page). A client sends it as a query when a value changes, and
    /// draws the answer in the table's place.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<Form>,
    /// Whether the rows can be chosen, and how the chosen ones are sent. Every
    /// row of such a table has an `id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub select: Option<Selection>,
    /// Where the next records are, by cursor. The answer is a `rows` op for
    /// this table, with the address after it or none.
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
    /// A small line with no axes: only the shape of its series, beside a figure
    /// or in a row. Its axes, reference lines and zones are left out.
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
    /// Its colour. A client takes its colours in turn when there is none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour: Option<Colour>,
    /// What its values count, when it is not what the value axis counts:
    /// `bpm`, `ms`. A client writes it after a value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

/// A chart, as the numbers it draws. The client draws it with its system's own
/// charts.
///
/// Lines, areas and bars run over one shared axis: `labels` are the places
/// along it and each series gives a value at each. `x` and `y` say what the two
/// axes measure (a name, a unit, bounds); a client takes bounds from the data
/// when none are said. Reference lines and zones lie across the value axis,
/// each with its name and colour, so a reader can tell a night from the need or
/// a rate from its usual range. A value axis can be named in `levels` instead
/// of numbered: value 0 is the first name, 1 the second; the stages of sleep
/// are drawn so, as a stepped area.
///
/// The chart is named by its `summary`, one sentence that says what it shows.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chart {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The kind.
    #[serde(default, skip_serializing_if = "is_default", deserialize_with = "crate::lenient::or_default")]
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
    /// What the labels are along the axis they stand on: `Night`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<Measure>,
    /// What the value axis measures: its unit and its bounds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<Measure>,
    /// Lines across the chart at a level, each with its name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<Reference>,
    /// Bands of the value axis, each with its name and colour: a target, a
    /// usual range.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub zones: Vec<Zone>,
    /// The names of the values of the value axis, from value 0.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub levels: Vec<String>,
    /// How a line or an area runs from one value to the next.
    #[serde(default, skip_serializing_if = "is_default", deserialize_with = "crate::lenient::or_default")]
    pub curve: Curve,
    /// How the series of a bar or area chart sit.
    #[serde(default, skip_serializing_if = "is_default", deserialize_with = "crate::lenient::or_default")]
    pub stacking: Stacking,
    /// Whether bars lie along rows, each with its label: for long names and
    /// ranked lists.
    #[serde(default, skip_serializing_if = "is_false")]
    pub horizontal: bool,
    /// One sentence that says the whole chart to a reader who can't see it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// A note about what the chart means, shown when asked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<Hint>,
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
    #[serde(default, skip_serializing_if = "is_default", deserialize_with = "crate::lenient::or_default")]
    pub role: Role,
    /// The standard action it is, when it is one. A client draws the system's
    /// own symbol and words for it, whatever icon and label it carries.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "crate::lenient::or_none")]
    pub standard: Option<Standard>,
    /// How loud it is.
    #[serde(default, skip_serializing_if = "is_default", deserialize_with = "crate::lenient::or_default")]
    pub prominence: Prominence,
    /// Whether what it does can be undone. A destructive action that can be
    /// undone asks nothing first on a system whose guidelines say so, and
    /// offers to undo it after.
    #[serde(default, skip_serializing_if = "is_false")]
    pub undoable: bool,
    /// The question it asks before it acts, in the system's own confirmation,
    /// for what can't be undone. A client asks it unless `undoable` lets the
    /// system's guidelines skip it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ask: Option<Question>,
    /// Whether it is ticked, for a menu item that turns something on or off;
    /// none for a plain action.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked: Option<bool>,
    /// The choice of one it belongs to: the ticked items of a menu with the
    /// same name are a group of which one is ticked, drawn as the system draws
    /// a choice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub one_of: Option<String>,
    /// Whether it can't be used now.
    #[serde(default, skip_serializing_if = "is_false")]
    pub disabled: bool,
    /// Why it can't be used now, in the page's words: `It went out at 09:41. It
    /// can go again after 09:46.`. Said only of a `disabled` button. A client
    /// puts it where its system explains a control that does not respond (a
    /// hint, a tip on long press, the control's description to assistive
    /// technology), and never in place of the label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Whether it is waiting for an answer.
    #[serde(default, skip_serializing_if = "is_false")]
    pub pending: bool,
    /// What it does. A button with none does nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<Action>,
}

/// The question a control asks before it does what can't be undone: the
/// words of a system's own confirmation, which names the thing and the act
/// ("Delete 3 orders?", not "Are you sure?").
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Question {
    /// What is asked: `Cancel order CH-1042?`.
    pub title: String,
    /// What follows from it: `₹1,24,050 goes back to the customer.`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The words of the button that goes ahead: `Cancel order`.
    pub confirm: String,
    /// The words of the button that goes back, when not the system's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancel: Option<String>,
}

/// An action every system has its own symbol and words for. A control that is
/// one of these is drawn as the system draws it, so a share button looks like
/// every other share button there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Standard {
    /// Sends something to another app or person.
    Share,
    /// Deletes something.
    Delete,
    /// Adds a new thing.
    Add,
    /// Changes something.
    Edit,
    /// Looks for something.
    Search,
    /// Finishes a task, keeping what was done.
    Done,
    /// Leaves a task, keeping nothing.
    Cancel,
    /// Offers more actions.
    More,
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
///
/// A form in a cell of a table is that row's inline edit, and is drawn as an
/// action that opens it as a task: see [`TableRow::cells`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Form {
    /// The form's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// How it is sent.
    #[serde(default, skip_serializing_if = "is_default", deserialize_with = "crate::lenient::or_default")]
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

/// One field of a form. Its default is a nameless text field, so a field is
/// built with `..Field::default()` and keeps building when fields are added.
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
    /// What the field is for, in a form that asks for records (a table's
    /// query), so a client puts it where its system puts that: the search in
    /// the search bar, filters with the filters, the order with the sorting.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "crate::lenient::or_none")]
    pub purpose: Option<Purpose>,
    /// What the client does when the person changes the value, besides
    /// holding it: send the form (`submit`: a box that saves as it is ticked, a
    /// search that answers as it is typed, once typing pauses as the system's
    /// own searches do), or run commands (a count of the characters typed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changed: Option<Action>,
}

impl Default for Field {
    fn default() -> Self {
        Self {
            id: None,
            name: String::new(),
            label: None,
            help: None,
            error: None,
            required: false,
            disabled: false,
            input: Input::Text(crate::input::TextInput::default()),
            purpose: None,
            changed: None,
        }
    }
}

/// What a field of a form that asks for records is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Purpose {
    /// The words searched for.
    Search,
    /// A filter: which records are shown.
    Filter,
    /// The order the records are in.
    Sort,
    /// How many records come at a time. A client that loads by cursor as the
    /// reader scrolls may leave it out.
    PageSize,
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
    #[serde(default, skip_serializing_if = "is_default", deserialize_with = "crate::lenient::or_default")]
    pub size: SheetSize,
    /// Whether it is a task the person finishes or cancels (a form), not a
    /// glance at something: each system presents the two its own way, as with
    /// [`Present::Task`](crate::Present::Task).
    #[serde(default, skip_serializing_if = "is_false")]
    pub task: bool,
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

/// Actions on a bar: a screen's own, or a group of them at its foot. Each
/// item has both words and a symbol, because a system shows one, the other
/// or both, by where the item ends up: on the bar, in its overflow menu, in
/// the menu bar.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Toolbar {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The items. A client places them by role, never by their order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<ToolbarItem>,
}

/// One action on a toolbar.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolbarItem {
    /// The item's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// What it says: in an overflow menu, beside its symbol, and to assistive
    /// technology.
    pub title: String,
    /// Its symbol.
    pub icon: Icon,
    /// The standard action it is, when it is one: the system's own symbol and
    /// words take the place of `icon` and `title`.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "crate::lenient::or_none")]
    pub standard: Option<Standard>,
    /// What it means.
    #[serde(default, skip_serializing_if = "is_default", deserialize_with = "crate::lenient::or_default")]
    pub role: Role,
    /// How loud it is.
    #[serde(default, skip_serializing_if = "is_default", deserialize_with = "crate::lenient::or_default")]
    pub prominence: Prominence,
    /// Whether what it does can be undone.
    #[serde(default, skip_serializing_if = "is_false")]
    pub undoable: bool,
    /// The question it asks before it acts, as a button's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ask: Option<Question>,
    /// Whether it can't be used now.
    #[serde(default, skip_serializing_if = "is_false")]
    pub disabled: bool,
    /// Why it can't be used now, as a button's `reason`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// What it does. An item with a menu opens the menu instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<Action>,
    /// The menu it opens: buttons, links, sections of them and dividers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub menu: Vec<Node>,
}

/// A value within a range, read at a glance: a level, a meter, a dial. The
/// numbers are the page's own; `shown` is the value in the page's words.
///
/// The scale may be divided into named zones, each its stretch of the scale in
/// its own colour (what is below, in and above the usual range). The value is
/// in the zone that holds it, the last one where they overlap; a client draws
/// the zones on the scale, draws the value in the colour of its zone, and says
/// the zone's name beside the value so that the colour is never alone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gauge {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The value.
    pub value: serde_json::Number,
    /// The least it can be.
    pub min: serde_json::Number,
    /// The most it can be.
    pub max: serde_json::Number,
    /// What it measures.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// The value in the page's words: `72%`, `₹42,000 of ₹60,000`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shown: Option<String>,
    /// A quieter line after the label: `Optimal`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
    /// The zones of the scale, with their names and bounds.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub zones: Vec<Zone>,
    /// What its colour means.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "crate::lenient::or_none")]
    pub tone: Option<Tone>,
    /// Its colour by hue, when it has no zones to take it from and its colour
    /// is not a tone's. It wins over `tone`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour: Option<Colour>,
    /// One sentence that says the whole gauge to a reader who can't see it:
    /// the value, the scale and the zone the value is in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// A note about what the gauge means, shown when asked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<Hint>,
}

impl Default for Gauge {
    fn default() -> Self {
        Self {
            id: None,
            value: 0.into(),
            min: 0.into(),
            max: 100.into(),
            label: None,
            shown: None,
            caption: None,
            zones: Vec::new(),
            tone: None,
            colour: None,
            summary: None,
            hint: None,
        }
    }
}

/// Which way a figure is going.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// Up.
    Up,
    /// Down.
    Down,
    /// Neither: also what a way this build does not know is read as.
    #[default]
    Flat,
}

/// Which way a figure is going, and whether that is good: up is not always
/// good, so the tone says.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trend {
    /// Which way.
    #[serde(deserialize_with = "crate::lenient::or_default")]
    pub direction: Direction,
    /// By how much, in the page's words: `12%`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Whether it is good (`success`) or bad (`danger`) news.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "crate::lenient::or_none")]
    pub tone: Option<Tone>,
}

/// A key figure: what it is, its value in the page's words, and which way it
/// is going. A tile on a dashboard can carry more: an icon, the unit of the
/// value, a status word, a small graphic beside it or a small chart under it,
/// and the page it leads to. Every piece is optional, and a figure that has
/// none is the plain one.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Figure {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// What the figure is.
    pub label: String,
    /// Its value, as the page wrote it: `₹1,24,050`, `58`.
    pub value: String,
    /// What the value counts, written after it and smaller: `bpm`, `%`, `h`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// An icon before the label, in a chip. It is decoration: the label says
    /// what the figure is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<Icon>,
    /// Which way it is going.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trend: Option<Trend>,
    /// A line under it, after the trend: what the trend is measured against
    /// (`vs last week`). It is the figure's caption.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// A word about the figure at the end of its label's line, with its tone:
    /// `Athletic`, `Fair`. The figure itself stays neutral: only a status or a
    /// trend says good or bad.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<Badge>,
    /// What its colour means.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "crate::lenient::or_none")]
    pub tone: Option<Tone>,
    /// The hue of its icon, when the thing it is about has a colour of its
    /// own (sleep is violet) and the colour says nothing good or bad. It is
    /// the icon's colour first, and never the figure's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour: Option<Colour>,
    /// A small graphic beside the figure, at the end of its line: a ring, or a
    /// few columns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aside: Option<Box<Node>>,
    /// A small chart under the figure: a sparkline of its last periods.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chart: Option<Box<Node>>,
    /// What a tap on the figure does: usually the page it is about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<Action>,
    /// A note about what the figure means, shown when asked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<Hint>,
}

/// Moments in order: what has happened to something and what is still to
/// come.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Timeline {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The moments, the earliest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<Moment>,
}

/// One moment of a timeline.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Moment {
    /// The moment's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// What happened, or will.
    pub title: String,
    /// A line under it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    /// When, in the page's words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,
    /// Whether it has happened, is happening or is to come.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "crate::lenient::or_none")]
    pub state: Option<RowState>,
    /// Its icon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<Icon>,
    /// What its colour means.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "crate::lenient::or_none")]
    pub tone: Option<Tone>,
    /// More under it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub detail: Vec<Node>,
    /// Whether it is drawn quieter than its neighbours: what a machine did, in
    /// a timeline of what people did.
    #[serde(default, skip_serializing_if = "is_false")]
    pub muted: bool,
}

/// One entry of an activity feed: who did what, and when.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedRow {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Who did it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<Avatar>,
    /// What kind of thing happened, when no person did it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<Icon>,
    /// What happened, in runs, so names and links stand out.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runs: Vec<Run>,
    /// When, in the page's words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,
    /// What its colour means.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "crate::lenient::or_none")]
    pub tone: Option<Tone>,
    /// More under it: a comment, a change from one value to another.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub detail: Vec<Node>,
    /// What a tap on the entry does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<Action>,
    /// Whether it is drawn quieter than its neighbours: what a machine did, in
    /// a feed of what people did.
    #[serde(default, skip_serializing_if = "is_false")]
    pub muted: bool,
}

/// How far a message has got to the person it is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Delivery {
    /// It is on its way to the server.
    Sending,
    /// The server has it.
    Sent,
    /// It has reached them.
    Delivered,
    /// They have read it.
    Read,
    /// It could not be sent.
    Failed,
}

/// One message of a conversation.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Who wrote it.
    pub author: String,
    /// Their picture or initials.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar: Option<Avatar>,
    /// Whether it comes from the reader's side of the conversation: the person
    /// reading, or the team they answer for (every agent's replies in a
    /// support inbox). A client sets that side apart, as its system's own
    /// conversations do.
    #[serde(default, skip_serializing_if = "is_false")]
    pub mine: bool,
    /// Whether it is a note only the team sees, not sent to the other side.
    #[serde(default, skip_serializing_if = "is_false")]
    pub note: bool,
    /// What it says: text, rich text, pictures, files.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub body: Vec<Node>,
    /// When it was written, in the page's words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,
    /// How far it has got.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "crate::lenient::or_none")]
    pub delivery: Option<Delivery>,
}

/// The part a pane plays on a split screen. Each system lays panes out by
/// these: side by side where there is room, one at a time where there is not.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PaneRole {
    /// The list a reader chooses from.
    List,
    /// The screen's main content.
    #[default]
    Content,
    /// What was chosen in the list.
    Detail,
    /// What helps with the content beside it: filters, a summary.
    Supporting,
    /// The details of what is selected in the content, to read or change.
    Inspector,
}

/// A pane of a split screen.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pane {
    /// The pane's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The part it plays.
    #[serde(deserialize_with = "crate::lenient::or_default")]
    pub role: PaneRole,
    /// Its title, where a system names panes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Whether it is the one a narrow screen shows, when only one fits.
    #[serde(default, skip_serializing_if = "is_false")]
    pub current: bool,
    /// What it holds.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Node>,
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
    fn a_disabled_control_may_say_why() {
        let button: Node = serde_json::from_str(
            r#"{"t":"button","label":"Resend confirmation","disabled":true,"reason":"It went out at 09:41."}"#,
        )
        .unwrap();
        let Node::Button(button) = button else {
            panic!("not a button");
        };
        assert!(button.disabled);
        assert_eq!(button.reason.as_deref(), Some("It went out at 09:41."));
        let item: Node = serde_json::from_str(
            r#"{"t":"toolbar","items":[{"title":"Close the day","icon":{"name":"lock"},"disabled":true,"reason":"3 orders are still to be packed."}]}"#,
        )
        .unwrap();
        let Node::Toolbar(toolbar) = item else {
            panic!("not a toolbar");
        };
        assert_eq!(toolbar.items[0].reason.as_deref(), Some("3 orders are still to be packed."));
        // A control that can be used says nothing of why it can't.
        assert!(!serde_json::to_string(&Button::default()).unwrap().contains("reason"));
    }

    #[test]
    fn a_table_row_says_whether_its_detail_shows_and_whether_it_can_be_chosen() {
        let row: TableRow = serde_json::from_str(
            r#"{"id":"1041","selected":true,"open":true,"unchoosable":false,"detail":[{"t":"text","value":"2 units"}]}"#,
        )
        .unwrap();
        assert!(row.open && row.selected && !row.unchoosable);
        // Both are false unless said, and false is left out.
        let plain = serde_json::to_string(&TableRow::default()).unwrap();
        assert_eq!(plain, "{}");
        let held = TableRow {
            unchoosable: true,
            ..TableRow::default()
        };
        assert_eq!(serde_json::to_string(&held).unwrap(), r#"{"unchoosable":true}"#);
    }

    #[test]
    fn a_selection_may_offer_every_record_that_matches() {
        let select: Selection = serde_json::from_str(
            r#"{"name":"ids[]","form":"orders-bulk","across":{"name":"all_matching","value":"eyJzdGF0dXMiOiJvcGVuIn0","total":1204}}"#,
        )
        .unwrap();
        assert_eq!(
            select.across,
            Some(Across {
                name: "all_matching".to_owned(),
                value: "eyJzdGF0dXMiOiJvcGVuIn0".to_owned(),
                total: Some(1204),
            })
        );
        // The number is only for the words, so it may be left out; the field and its value are not.
        let bare: Selection = serde_json::from_str(r#"{"name":"ids[]","across":{"name":"all_matching","value":"q"}}"#).unwrap();
        assert_eq!(bare.across.map(|across| across.total), Some(None));
        assert!(serde_json::from_str::<Selection>(r#"{"name":"ids[]","across":{"name":"all_matching"}}"#).is_err());
        assert_eq!(serde_json::from_str::<Selection>(r#"{"name":"ids[]"}"#).unwrap().across, None);
    }

    #[test]
    fn a_move_names_a_row_and_its_lists_by_key_when_they_have_one() {
        let node: Node = serde_json::from_str(
            r#"{"t":"list","id":"board-doing","reorder":{"href":"/cards/move","key":"doing","group":"board"},
                "children":[{"t":"row","id":"c-5","key":"5","title":"Call the Jalandhar supplier"},
                            {"t":"row","id":"c-6","title":"Book the stall"}]}"#,
        )
        .unwrap();
        let Node::List(list) = node else {
            panic!("not a list");
        };
        assert_eq!(list.reorder.as_ref().and_then(|reorder| reorder.key.as_deref()), Some("doing"));
        let keys: Vec<_> = list
            .children
            .iter()
            .map(|row| match row {
                Node::Row(row) => row.key.as_deref(),
                other => panic!("{other:?}"),
            })
            .collect();
        // A row with none is named by its id, as before.
        assert_eq!(keys, [Some("5"), None]);
        // What a move posts is named once.
        assert_eq!(
            [Reorder::ITEM, Reorder::FROM, Reorder::TO, Reorder::BEFORE, Reorder::POSITION],
            ["item", "from", "to", "before", "position"]
        );
    }

    #[test]
    fn a_moment_may_be_quiet_like_a_row_and_a_feed_entry() {
        let moment: Moment =
            serde_json::from_str(r#"{"title":"Reminder sent to the carrier","muted":true}"#).unwrap();
        assert!(moment.muted);
        assert_eq!(serde_json::to_string(&Moment::default()).unwrap(), r#"{"title":""}"#);
    }

    #[test]
    fn a_figure_may_carry_what_a_dashboard_tile_does() {
        let node: Node = serde_json::from_str(
            r#"{"t":"figure","label":"Resting heart rate","value":"58","unit":"bpm",
                "status":{"text":"Athletic","tone":"success"},"detail":"vs last week",
                "colour":{"hue":"rose"},
                "aside":{"t":"ring","id":"rhr-ring","value":58,"max":100,"centre":"nothing","prominence":"low"},
                "chart":{"t":"chart","id":"rhr-days","kind":"sparkline","series":[{"label":"Resting","values":[61,60,58]}]},
                "action":{"t":"navigate","href":"/heart"},
                "hint":{"about":"resting heart rate","text":"Measured while you are still."}}"#,
        )
        .unwrap();
        let Node::Figure(figure) = &node else {
            panic!("not a figure");
        };
        assert_eq!(figure.unit.as_deref(), Some("bpm"));
        assert_eq!(figure.status.as_ref().map(|status| status.text.as_str()), Some("Athletic"));
        assert_eq!(figure.colour, Some(Colour::new(Hue::Rose)));
        assert!(figure.action.is_some() && figure.hint.is_some());
        // The graphics are parts of the tree: they are found, and walked.
        assert_eq!(node.children().len(), 2);
        assert!(matches!(node.find("rhr-ring"), Some(Node::Ring(ring)) if ring.centre == Centre::Nothing));
        assert!(matches!(node.find("rhr-days"), Some(Node::Chart(_))));
        // A plain figure writes none of it.
        assert_eq!(
            serde_json::to_string(&Figure::default()).unwrap(),
            r#"{"label":"","value":""}"#
        );
    }

    #[test]
    fn a_gauge_reads_as_it_did_before_it_had_zones_and_takes_a_zone_of_a_newer_hue() {
        let node: Node = serde_json::from_str(
            r#"{"t":"gauge","value":72,"min":0,"max":100,"label":"Space","shown":"72%","tone":"warning"}"#,
        )
        .unwrap();
        let Node::Gauge(gauge) = node else {
            panic!("not a gauge");
        };
        assert!(gauge.zones.is_empty() && gauge.colour.is_none() && gauge.hint.is_none());
        assert_eq!(gauge.tone, Some(Tone::Warning));
        // A zone names its colour; a hue this build lacks is neutral, and the zone still holds.
        let node: Node = serde_json::from_str(
            r#"{"t":"gauge","value":1.12,"min":0,"max":2,"zones":[{"from":0.8,"to":1.3,"label":"Optimal","colour":{"hue":"teal"}}]}"#,
        )
        .unwrap();
        let Node::Gauge(gauge) = node else {
            panic!("not a gauge");
        };
        assert_eq!(gauge.zones[0].label.as_deref(), Some("Optimal"));
        assert_eq!(gauge.zones[0].colour, Some(Colour::new(Hue::Neutral)));
        // A zone needs both its ends.
        assert!(serde_json::from_str::<Zone>(r#"{"from":1}"#).is_err());
    }

    #[test]
    fn a_day_clock_holds_hours_with_no_reading_and_the_hour_that_is_now() {
        let node: Node = serde_json::from_str(
            r#"{"t":"day_clock","hours":[{"value":120,"category":1},{}],
                "key":[{"name":"Rest"},{"name":"Calm","colour":{"hue":"cyan"}}],
                "moments":[{"at":6.5,"name":"Woke up"}],"now":20}"#,
        )
        .unwrap();
        let Node::DayClock(clock) = node else {
            panic!("not a clock");
        };
        assert_eq!(clock.now, Some(20));
        assert_eq!(clock.hours[0].category, Some(1));
        // An hour with no reading is not an hour of zero: it holds nothing.
        assert_eq!(clock.hours[1], crate::charts::Hour::default());
        assert_eq!(serde_json::to_string(&clock.hours[1]).unwrap(), "{}");
        assert_eq!(clock.moments[0].at.as_f64(), Some(6.5));
        // A moment says when and what.
        assert!(serde_json::from_str::<Node>(r#"{"t":"day_clock","moments":[{"at":6.5}]}"#).is_err());
    }

    #[test]
    fn a_day_pager_says_the_day_twice_and_where_its_neighbours_are() {
        let node: Node = serde_json::from_str(
            r#"{"t":"day_pager","day":"Yesterday","date":"2026-11-12",
                "previous":"/?date=2026-11-11","next":"/?date=2026-11-13","today":"/",
                "first":"2026-01-02","last":"2026-11-13","pick":{"href":"/","name":"date"}}"#,
        )
        .unwrap();
        let Node::DayPager(pager) = &node else {
            panic!("not a pager");
        };
        assert_eq!((pager.day.as_str(), pager.date.as_str()), ("Yesterday", "2026-11-12"));
        assert_eq!(pager.today.as_deref(), Some("/"));
        assert_eq!(pager.pick.as_ref().map(|pick| pick.name.as_str()), Some("date"));
        assert!(node.children().is_empty());
        // The day is said as words and as a date: a pager with only one is refused, as is a pick
        // that does not say how the day is asked for.
        assert!(serde_json::from_str::<Node>(r#"{"t":"day_pager","day":"Today"}"#).is_err());
        assert!(serde_json::from_str::<Node>(
            r#"{"t":"day_pager","day":"Today","date":"2026-11-13","pick":{"href":"/"}}"#
        )
        .is_err());
    }

    #[test]
    fn a_chart_reads_as_it_did_before_axes_and_marks_and_forgives_a_word_it_lacks() {
        let old: Chart = serde_json::from_str(
            r#"{"kind":"bar","title":"Orders","labels":["Mon"],"series":[{"label":"UPI","values":[41]}]}"#,
        )
        .unwrap();
        assert!(old.x.is_none() && old.y.is_none() && old.references.is_empty() && old.zones.is_empty());
        assert_eq!((old.curve, old.stacking, old.horizontal), (Curve::Linear, Stacking::Apart, false));
        // A newer server's words read as the usual ones, and the chart still draws.
        let newer: Chart = serde_json::from_str(
            r#"{"kind":"line","curve":"wiggly","stacking":"layers",
                "references":[{"kind":"orbit","value":8,"label":"Your need"}],
                "series":[{"label":"Asleep","colour":{"hue":"teal"}}]}"#,
        )
        .unwrap();
        assert_eq!((newer.curve, newer.stacking), (Curve::Linear, Stacking::Apart));
        assert_eq!(newer.references[0].kind, ReferenceKind::Level);
        assert_eq!(newer.series[0].colour, Some(Colour::new(Hue::Neutral)));
        // The diagonal where x equals y has no level of its own.
        let diagonal: Reference =
            serde_json::from_str(r#"{"kind":"diagonal","label":"Equal"}"#).unwrap();
        assert_eq!((diagonal.kind, diagonal.value), (ReferenceKind::Diagonal, None));
        // Nothing is written for what a chart does not say.
        assert_eq!(serde_json::to_string(&Chart::default()).unwrap(), "{}");
    }

    #[test]
    fn a_section_may_carry_a_hint_beside_its_heading() {
        let node: Node = serde_json::from_str(
            r#"{"t":"section","title":"Sleep stages","hint":{"about":"the sleep stages","title":"An estimate","text":"Stages are estimated from movement and heart rate.","mark":"info"}}"#,
        )
        .unwrap();
        let Node::Section(section) = node else {
            panic!("not a section");
        };
        let hint = section.hint.expect("a hint");
        assert_eq!((hint.about.as_str(), hint.mark), ("the sleep stages", HintMark::Info));
        assert!(!serde_json::to_string(&Section::default()).unwrap().contains("hint"));
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
