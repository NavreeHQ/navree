//! [`Kind`]: the name of every node kind and the vocabulary it arrived in.
//!
//! An app in a store can't be reloaded like a web page, so a server writes each
//! answer for the vocabulary the client says it draws. A kind the client is too
//! old for is found with [`newer_than`] before an answer leaves.

use std::fmt;

use serde::de::value::MapAccessDeserializer;
use serde::de::{Error as _, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::node::{self, Node};

macro_rules! kinds {
    ($($variant:ident($ty:ident) => $name:literal since $since:literal,)+) => {
        /// The kind of a node: its `t`.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        #[non_exhaustive]
        pub enum Kind {
            $(
                #[doc = concat!("`", $name, "`.")]
                $variant,
            )+
            /// A kind this build of the protocol has never heard of, from a
            /// newer server: [`Node::Unknown`] holds its name.
            Unknown,
        }

        impl Kind {
            /// Every kind of the current vocabulary.
            pub const ALL: &'static [Kind] = &[$(Kind::$variant,)+];

            /// The kind as a node's `t` names it. An unknown kind's own name is
            /// in its node.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $name,)+
                    Self::Unknown => "unknown",
                }
            }

            /// The vocabulary the kind arrived in. A client that draws an older
            /// one has never heard of it. No vocabulary holds an unknown kind.
            pub const fn since(self) -> u32 {
                match self {
                    $(Self::$variant => $since,)+
                    Self::Unknown => u32::MAX,
                }
            }

            /// The kind a node's `t` names, when this build knows it.
            pub fn parse(name: &str) -> Option<Self> {
                match name {
                    $($name => Some(Self::$variant),)+
                    _ => None,
                }
            }

            /// The name of the struct a node of this kind holds, as the schema
            /// names its definition.
            pub(crate) const fn type_name(self) -> &'static str {
                match self {
                    $(Self::$variant => stringify!($ty),)+
                    Self::Unknown => "Unknown",
                }
            }
        }

        impl Node {
            /// The node's kind.
            pub const fn kind(&self) -> Kind {
                match self {
                    $(Self::$variant(_) => Kind::$variant,)+
                    Self::Unknown(_) => Kind::Unknown,
                }
            }
        }

        /// A node of a known kind as it is written, by reference.
        #[derive(Serialize)]
        #[serde(tag = "t")]
        enum Written<'a> {
            $(
                #[serde(rename = $name)]
                $variant(&'a node::$ty),
            )+
        }

        /// A node of a known kind as it is read.
        #[derive(Deserialize)]
        #[serde(tag = "t")]
        enum Read {
            $(
                #[serde(rename = $name)]
                $variant(node::$ty),
            )+
        }

        impl Serialize for Node {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                match self {
                    $(Self::$variant(node) => Written::$variant(node).serialize(serializer),)+
                    Self::Unknown(node) => node.serialize(serializer),
                }
            }
        }

        /// A node of a kind this build knows is held to that kind's shape; a
        /// node of any other kind is read as [`Node::Unknown`], keeping its id
        /// and its `alt`.
        ///
        /// A server writes `t` first, so the rest of the node is read straight
        /// into its kind's struct, with nothing held aside: a deep tree costs
        /// little stack for each level. A node whose `t` comes later is read
        /// whole first, then as the same.
        impl<'de> Deserialize<'de> for Node {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let _inside = Inside::enter().ok_or_else(|| {
                    D::Error::custom(format!(
                        "a tree more than {} nodes deep",
                        crate::MAX_DEPTH
                    ))
                })?;
                deserializer.deserialize_map(NodeVisitor)
            }
        }

        struct NodeVisitor;

        impl<'de> Visitor<'de> for NodeVisitor {
            type Value = Node;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a node: an object that names its kind in `t`")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Node, A::Error> {
                let mut before = serde_json::Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    if key != "t" {
                        before.insert(key, map.next_value()?);
                        continue;
                    }
                    let name: String = map.next_value()?;
                    if !before.is_empty() {
                        before.insert(key, name.into());
                        while let Some((key, value)) = map.next_entry::<String, serde_json::Value>()? {
                            before.insert(key, value);
                        }
                        return Node::from_whole(serde_json::Value::Object(before))
                            .map_err(A::Error::custom);
                    }
                    let rest = MapAccessDeserializer::new(map);
                    return match Kind::parse(&name) {
                        $(Some(Kind::$variant) => node::$ty::deserialize(rest).map(Node::$variant),)+
                        _ => UnknownRest::deserialize(rest).map(|rest| {
                            Node::Unknown(node::Unknown {
                                kind: name,
                                id: rest.id.as_ref().and_then(serde_json::Value::as_str).map(str::to_owned),
                                alt: rest.alt,
                            })
                        }),
                    };
                }
                Err(A::Error::custom("a node names its kind in `t`"))
            }
        }

        impl Node {
            /// Reads a node held whole, whatever the order of its fields.
            fn from_whole(value: serde_json::Value) -> Result<Self, serde_json::Error> {
                let Some(name) = value.get("t").and_then(serde_json::Value::as_str) else {
                    return Err(serde_json::Error::custom("a node names its kind in `t`"));
                };
                if Kind::parse(name).is_none() {
                    let name = name.to_owned();
                    return node::Unknown::read(&name, &value).map(Node::Unknown);
                }
                Ok(match Read::deserialize(value)? {
                    $(Read::$variant(node) => Node::$variant(node),)+
                })
            }
        }
    };
}

thread_local! {
    /// How many nodes are being read inside one another on this thread.
    static READING: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// One level of nodes being read, counted while it lasts: a tree deeper than
/// [`MAX_DEPTH`](crate::MAX_DEPTH) is refused with an error before it can use
/// up the stack, in a debug build too.
struct Inside;

impl Inside {
    fn enter() -> Option<Self> {
        READING.with(|reading| {
            let depth = reading.get() + 1;
            (depth <= crate::MAX_DEPTH).then(|| {
                reading.set(depth);
                Self
            })
        })
    }
}

impl Drop for Inside {
    fn drop(&mut self) {
        READING.with(|reading| reading.set(reading.get() - 1));
    }
}

/// What a node of an unknown kind keeps, read from the fields after its `t`.
#[derive(Deserialize)]
struct UnknownRest {
    #[serde(default)]
    id: Option<serde_json::Value>,
    #[serde(default)]
    alt: Option<Box<Node>>,
}

kinds! {
    Stack(Stack) => "stack" since 0,
    Grid(Grid) => "grid" since 0,
    Section(Section) => "section" since 0,
    List(List) => "list" since 0,
    Row(Row) => "row" since 0,
    Split(Split) => "split" since 0,
    Tabs(Tabs) => "tabs" since 0,
    Disclosure(Disclosure) => "disclosure" since 0,
    Divider(Divider) => "divider" since 0,
    Spacer(Spacer) => "spacer" since 0,
    Region(Region) => "region" since 0,
    Text(Text) => "text" since 0,
    Rich(Rich) => "rich" since 0,
    Image(Image) => "image" since 0,
    Icon(Icon) => "icon" since 0,
    Avatar(Avatar) => "avatar" since 0,
    Badge(Badge) => "badge" since 0,
    Progress(Progress) => "progress" since 0,
    Banner(Banner) => "banner" since 0,
    Empty(Empty) => "empty" since 0,
    Facts(Facts) => "facts" since 0,
    Table(Table) => "table" since 0,
    Chart(Chart) => "chart" since 0,
    Code(Code) => "code" since 0,
    Button(Button) => "button" since 0,
    Link(Link) => "link" since 0,
    Menu(Menu) => "menu" since 0,
    Form(Form) => "form" since 0,
    Field(Field) => "field" since 0,
    Sheet(Sheet) => "sheet" since 0,
    Dialog(Dialog) => "dialog" since 0,
    Popover(Popover) => "popover" since 0,
    Native(NativeView) => "native" since 0,
    Web(WebRegion) => "web" since 0,
    Toolbar(Toolbar) => "toolbar" since 0,
    Gauge(Gauge) => "gauge" since 0,
    Figure(Figure) => "figure" since 0,
    Timeline(Timeline) => "timeline" since 0,
    FeedRow(FeedRow) => "feed_row" since 0,
    Message(Message) => "message" since 0,
    Pane(Pane) => "pane" since 0,
    Ring(Ring) => "ring" since 0,
    Rings(Rings) => "rings" since 0,
    DayClock(DayClock) => "day_clock" since 0,
    DayPager(DayPager) => "day_pager" since 0,
    Meter(Meter) => "meter" since 0,
    Columns(MiniColumns) => "columns" since 0,
    Strip(ColorStrip) => "strip" since 0,
    Range(RangeScale) => "range" since 0,
    CalendarHeatmap(CalendarHeatmap) => "calendar_heatmap" since 0,
}

/// A field that arrived after its type did: the type's name in the schema
/// (`Button`, `ChoiceInput`), the field's name in the JSON, and the vocabulary
/// it arrived in. Every other field arrived with its type: a kind's in the
/// kind's vocabulary, any other type's in vocabulary 0. The schema records
/// each field's vocabulary from this list.
///
/// An older client passes over a field it does not know, so nothing is done
/// about these when an answer is written: a later field must mean nothing
/// that an older client would get wrong by not seeing it.
pub const LATER_FIELDS: &[(&str, &str, u32)] = &[];

/// A node written for an older vocabulary.
// The node is moved through once, on its way into an envelope; boxing it would
// only add an allocation.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Downgraded {
    /// The node as the older client draws it: each kind it lacks replaced by
    /// that kind's older form ([`Node::older`]). Unchanged when the client
    /// lacks nothing.
    Node(Node),
    /// A kind the client lacks has no older form, so the page goes to the web
    /// view. `why` names each such kind as `vocab:<kind>`, for the coverage
    /// report.
    Web {
        /// What could not be written for the client.
        why: Vec<String>,
    },
}

/// `node` written for a client that draws vocabulary `to`: each kind newer
/// than `to` takes its recorded older form, or the page goes to the web view.
/// A node of a kind no vocabulary holds ([`Node::Unknown`]) takes its `alt`.
///
/// ```
/// use navree_native::vocab::{self, Downgraded};
/// use navree_native::{Node, VOCAB};
///
/// let node = Node::text("Orders");
/// assert_eq!(vocab::downgrade(node.clone(), VOCAB), Downgraded::Node(node));
/// ```
pub fn downgrade(node: Node, to: u32) -> Downgraded {
    downgrade_by(node, to, Kind::since)
}

/// [`downgrade`] with the vocabulary each kind arrived in given by `since`
/// instead of [`Kind::since`]: how a rule that sends a kind to the web view
/// for some clients is applied, and how the rewriting is tested before a
/// second vocabulary exists. It walks with a list of its own, so a tree of any
/// depth is rewritten without using up the stack.
pub fn downgrade_by(mut node: Node, to: u32, since: impl Fn(Kind) -> u32) -> Downgraded {
    // An older form is written in older kinds; a few steps always reach one
    // the client draws, and a form that does not is a mistake in the table.
    const STEPS: usize = 8;
    let mut why = std::collections::BTreeSet::new();
    let mut stack: Vec<&mut Node> = vec![&mut node];
    while let Some(at) = stack.pop() {
        let mut steps = 0;
        while since(at.kind()) > to && steps < STEPS {
            match at.older() {
                Some(older) => *at = older,
                None => break,
            }
            steps += 1;
        }
        if since(at.kind()) > to {
            why.insert(format!("vocab:{}", at.kind_name()));
            continue;
        }
        stack.extend(at.children_mut());
    }
    if why.is_empty() {
        Downgraded::Node(node)
    } else {
        Downgraded::Web {
            why: why.into_iter().collect(),
        }
    }
}

impl Node {
    /// The name the node's `t` gives its kind, an unknown kind's own among them.
    pub fn kind_name(&self) -> &str {
        match self {
            Self::Unknown(node) => &node.kind,
            node => node.kind().as_str(),
        }
    }

    /// The node in older kinds, for a client that lacks this one: its
    /// recorded older form. A kind with none sends the page to the web view
    /// when a client lacks it. An unknown kind's older form is its `alt`.
    pub fn older(&self) -> Option<Node> {
        use crate::node as n;
        let text = |value: &str, muted: bool| {
            Box::new(Node::Text(n::Text {
                value: value.to_owned(),
                muted,
                ..n::Text::default()
            }))
        };
        Some(match self {
            Self::Toolbar(node) => Node::Stack(n::Stack {
                id: node.id.clone(),
                axis: n::Axis::Horizontal,
                gap: 2,
                wrap: true,
                children: node
                    .items
                    .iter()
                    .map(|item| {
                        if item.menu.is_empty() {
                            Node::Button(n::Button {
                                id: item.id.clone(),
                                label: item.title.clone(),
                                icon: Some(item.icon.clone()),
                                role: item.role,
                                standard: item.standard,
                                prominence: item.prominence,
                                undoable: item.undoable,
                                disabled: item.disabled,
                                action: item.action.clone(),
                                ..n::Button::default()
                            })
                        } else {
                            Node::Menu(n::Menu {
                                id: item.id.clone(),
                                label: item.title.clone(),
                                icon: Some(item.icon.clone()),
                                items: item.menu.clone(),
                            })
                        }
                    })
                    .collect(),
                ..n::Stack::default()
            }),
            Self::Gauge(node) => {
                let number = |value: &serde_json::Number| value.as_f64().unwrap_or_default();
                let (value, min, max) = (number(&node.value), number(&node.min), number(&node.max));
                let share = if max > min {
                    ((value - min) / (max - min)).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let label = match (&node.label, &node.shown) {
                    (Some(label), Some(shown)) => Some(format!("{label}: {shown}")),
                    (label, shown) => label.clone().or_else(|| shown.clone()),
                };
                // The zone the value is in, where the scale has them, is said beside it.
                let zone = node
                    .zones
                    .iter()
                    .rev()
                    .find(|zone| number(&zone.from) <= value && value <= number(&zone.to))
                    .and_then(|zone| zone.label.clone());
                let label = match (label, zone) {
                    (Some(label), Some(zone)) => Some(format!("{label} ({zone})")),
                    (None, Some(zone)) => Some(zone),
                    (label, None) => label,
                };
                Node::Progress(n::Progress {
                    id: node.id.clone(),
                    // A share between 0 and 1, in thousandths.
                    value: Some((share * 1000.0).round() as u16),
                    label,
                })
            }
            Self::Figure(node) => {
                let trend = node.trend.as_ref().map(|trend| {
                    let arrow = match trend.direction {
                        n::Direction::Up => "↑",
                        n::Direction::Down => "↓",
                        n::Direction::Flat => "→",
                    };
                    match &trend.text {
                        Some(text) => format!("{arrow} {text}"),
                        None => arrow.to_owned(),
                    }
                });
                // What it holds besides words (a graphic, a chart) stays with the value.
                let value = std::iter::once(Node::text(words(
                    Some(&node.value),
                    None,
                    node.unit.as_deref(),
                )))
                .chain(node.aside.as_deref().cloned())
                .chain(node.chart.as_deref().cloned())
                .collect();
                Node::Facts(n::Facts {
                    id: node.id.clone(),
                    items: vec![n::Fact {
                        label: node.label.clone(),
                        value,
                        icon: node.icon.clone(),
                        tone: node
                            .tone
                            .or_else(|| node.trend.as_ref().and_then(|trend| trend.tone)),
                        details: trend
                            .into_iter()
                            .chain(node.detail.clone())
                            .chain(node.status.as_ref().map(|status| status.text.clone()))
                            .collect(),
                    }],
                })
            }
            Self::Timeline(node) => Node::List(n::List {
                id: node.id.clone(),
                children: node
                    .items
                    .iter()
                    .map(|moment| {
                        Node::Row(n::Row {
                            id: moment.id.clone(),
                            leading: moment.icon.clone().map(|icon| {
                                Box::new(Node::Icon(n::Icon {
                                    tone: moment.tone.or(icon.tone),
                                    ..icon
                                }))
                            }),
                            title: moment.title.clone(),
                            subtitle: moment.subtitle.clone(),
                            detail: moment.detail.clone(),
                            trailing: moment.time.as_deref().map(|time| text(time, true)),
                            state: moment.state,
                            ..n::Row::default()
                        })
                    })
                    .collect(),
                ..n::List::default()
            }),
            Self::FeedRow(node) => Node::Row(n::Row {
                id: node.id.clone(),
                leading: match (&node.actor, &node.icon) {
                    (Some(actor), _) => Some(Box::new(Node::Avatar(actor.clone()))),
                    (None, Some(icon)) => Some(Box::new(Node::Icon(icon.clone()))),
                    (None, None) => None,
                },
                title: node.runs.iter().map(|run| run.text.as_str()).collect(),
                detail: node.detail.clone(),
                trailing: node.time.as_deref().map(|time| text(time, true)),
                action: node.action.clone(),
                ..n::Row::default()
            }),
            Self::Message(node) => Node::Row(n::Row {
                id: node.id.clone(),
                leading: node.avatar.clone().map(|avatar| Box::new(Node::Avatar(avatar))),
                title: node.author.clone(),
                subtitle: node.time.clone(),
                detail: node.body.clone(),
                ..n::Row::default()
            }),
            Self::Pane(node) => Node::Section(n::Section {
                id: node.id.clone(),
                title: node.title.clone(),
                children: node.children.clone(),
                ..n::Section::default()
            }),
            Self::Ring(node) => {
                let goal = format!("of {}", words(None, Some(&node.max), node.unit.as_deref()));
                Node::Figure(n::Figure {
                    id: node.id.clone(),
                    label: node.label.clone().unwrap_or_default(),
                    value: words(node.shown.as_deref(), node.value.as_ref(), node.unit.as_deref()),
                    detail: Some(match &node.caption {
                        Some(caption) => format!("{caption} · {goal}"),
                        None => goal,
                    }),
                    tone: node.colour.as_ref().and_then(|colour| colour.hue.tone()),
                    ..n::Figure::default()
                })
            }
            Self::Rings(node) => Node::Facts(n::Facts {
                id: node.id.clone(),
                items: node
                    .rings
                    .iter()
                    .map(|ring| {
                        let goal = words(None, Some(&ring.max), ring.unit.as_deref());
                        n::Fact {
                            label: ring.name.clone(),
                            value: vec![Node::text(match (&ring.shown, &ring.value) {
                                (None, None) => format!("– of {goal}"),
                                (shown, value) => {
                                    let value = words(shown.as_deref(), value.as_ref(), None);
                                    format!("{value} of {goal}")
                                }
                            })],
                            icon: None,
                            tone: ring.colour.as_ref().and_then(|colour| colour.hue.tone()),
                            details: Vec::new(),
                        }
                    })
                    .collect(),
            }),
            Self::DayClock(node) => {
                // The day's total first, then each moment at its time.
                let total: f64 = node
                    .hours
                    .iter()
                    .filter_map(|hour| hour.value.as_ref().and_then(serde_json::Number::as_f64))
                    .sum();
                let mut items = vec![n::Fact {
                    label: node
                        .title
                        .clone()
                        .or_else(|| node.label.clone())
                        .unwrap_or_default(),
                    value: vec![Node::text(words(
                        node.shown.as_deref(),
                        Some(&quantity(total)),
                        node.unit.as_deref(),
                    ))],
                    icon: node.icon.clone(),
                    tone: None,
                    details: Vec::new(),
                }];
                items.extend(node.moments.iter().map(|moment| {
                    let minutes = (moment.at.as_f64().unwrap_or_default() * 60.0).round() as u32;
                    n::Fact {
                        label: moment.name.clone(),
                        value: vec![Node::text(format!(
                            "{:02}:{:02}",
                            minutes / 60 % 24,
                            minutes % 60
                        ))],
                        icon: moment.icon.clone(),
                        tone: moment.colour.as_ref().and_then(|colour| colour.hue.tone()),
                        details: Vec::new(),
                    }
                }));
                Node::Facts(n::Facts {
                    id: node.id.clone(),
                    items,
                })
            }
            Self::DayPager(node) => {
                // The day between two buttons; a button with no address is dimmed.
                let step = |address: &Option<String>, word: Option<&str>, fallback: &str| {
                    Node::Button(n::Button {
                        label: word.unwrap_or(fallback).to_owned(),
                        role: n::Role::Navigation,
                        disabled: address.is_none(),
                        action: address.as_deref().map(crate::action::Action::navigate),
                        ..n::Button::default()
                    })
                };
                let words = node.words.as_ref();
                Node::Stack(n::Stack {
                    id: node.id.clone(),
                    axis: n::Axis::Horizontal,
                    gap: 2,
                    align: Some(n::Align::Center),
                    justify: Some(n::Justify::Between),
                    children: [
                        Some(step(
                            &node.previous,
                            words.map(|words| words.previous.as_str()),
                            "Previous day",
                        )),
                        node.scores.as_deref().cloned(),
                        Some(Node::text(node.day.clone())),
                        Some(step(&node.next, words.map(|words| words.next.as_str()), "Next day")),
                    ]
                    .into_iter()
                    .flatten()
                    .collect(),
                    ..n::Stack::default()
                })
            }
            Self::Meter(node) => {
                // Each segment with its words, its share of the whole and its detail.
                let amount = |segment: &n::Segment| {
                    segment
                        .value
                        .as_ref()
                        .and_then(serde_json::Number::as_f64)
                        .filter(|value| *value > 0.0)
                };
                let total: f64 = node.segments.iter().filter_map(amount).sum();
                Node::Facts(n::Facts {
                    id: node.id.clone(),
                    items: node
                        .segments
                        .iter()
                        .map(|segment| n::Fact {
                            label: segment.name.clone(),
                            value: vec![Node::text(words(
                                segment.shown.as_deref(),
                                segment.value.as_ref(),
                                None,
                            ))],
                            icon: None,
                            tone: segment.colour.as_ref().and_then(|colour| colour.hue.tone()),
                            details: amount(segment)
                                .filter(|_| total > 0.0)
                                .map(|value| format!("{}%", (value / total * 100.0).round()))
                                .into_iter()
                                .chain(segment.detail.clone())
                                .collect(),
                        })
                        .collect(),
                })
            }
            Self::Columns(node) => Node::Facts(n::Facts {
                id: node.id.clone(),
                items: node
                    .columns
                    .iter()
                    .map(|column| {
                        let amounts = column.values.iter().map(|value| {
                            value
                                .as_ref()
                                .and_then(serde_json::Number::as_f64)
                                .filter(|value| *value > 0.0)
                        });
                        let sum: f64 = amounts.clone().flatten().sum();
                        n::Fact {
                            label: column.label.clone(),
                            value: vec![Node::text(words(
                                column.shown.as_deref(),
                                Some(&quantity(sum)),
                                None,
                            ))],
                            icon: None,
                            tone: None,
                            // Each part with its amount: `Deep 36`.
                            details: node
                                .parts
                                .iter()
                                .zip(amounts)
                                .filter_map(|(part, amount)| {
                                    amount.map(|amount| format!("{} {}", part.name, quantity(amount)))
                                })
                                .collect(),
                        }
                    })
                    .collect(),
            }),
            Self::Strip(node) => Node::Facts(n::Facts {
                id: node.id.clone(),
                // Each category with its share of the cells.
                items: node
                    .key
                    .iter()
                    .enumerate()
                    .map(|(at, category)| {
                        let count = node
                            .cells
                            .iter()
                            .filter(|cell| cell.category == u32::try_from(at).ok())
                            .count();
                        let share = if node.cells.is_empty() {
                            0.0
                        } else {
                            count as f64 / node.cells.len() as f64 * 100.0
                        };
                        n::Fact {
                            label: category.name.clone(),
                            value: vec![Node::text(format!("{}%", share.round()))],
                            icon: None,
                            tone: category.colour.as_ref().and_then(|colour| colour.hue.tone()),
                            details: Vec::new(),
                        }
                    })
                    .collect(),
            }),
            Self::Range(node) => {
                // The zone the value is in, then the scale in words.
                let value = node.value.as_ref().and_then(serde_json::Number::as_f64);
                let zone = value.and_then(|value| {
                    node.zones
                        .iter()
                        .rev()
                        .find(|zone| {
                            let (from, to) = (zone.from.as_f64(), zone.to.as_f64());
                            from.is_some_and(|from| from <= value) && to.is_some_and(|to| value <= to)
                        })
                        .and_then(|zone| zone.label.clone())
                });
                let scale = match (&node.low, &node.high) {
                    (Some(low), Some(high)) => format!("from {low} to {high}"),
                    _ => format!(
                        "on a scale from {} to {}",
                        words(None, Some(&node.min), node.unit.as_deref()),
                        words(None, Some(&node.max), node.unit.as_deref())
                    ),
                };
                Node::Figure(n::Figure {
                    id: node.id.clone(),
                    label: node.title.clone().unwrap_or_default(),
                    value: words(node.shown.as_deref(), node.value.as_ref(), node.unit.as_deref()),
                    detail: Some(match zone {
                        Some(zone) => format!("{zone} · {scale}"),
                        None => scale,
                    }),
                    ..n::Figure::default()
                })
            }
            Self::CalendarHeatmap(node) => {
                // A bar for each of the latest days, a day with no reading a gap.
                let days = &node.days[node.days.len().saturating_sub(OLDER_DAYS)..];
                Node::Chart(n::Chart {
                    id: node.id.clone(),
                    kind: n::ChartKind::Bar,
                    title: node.title.clone(),
                    labels: days.iter().map(|day| day.date.clone()).collect(),
                    series: vec![n::Series {
                        label: node
                            .label
                            .clone()
                            .or_else(|| node.title.clone())
                            .unwrap_or_default(),
                        values: days.iter().map(|day| day.value.clone()).collect(),
                        ..n::Series::default()
                    }],
                    ..n::Chart::default()
                })
            }
            Self::Unknown(node) => return node.alt.as_deref().cloned(),
            _ => return None,
        })
    }
}

/// The most days of a calendar an older client is shown, as bars.
const OLDER_DAYS: usize = 90;

/// A number written without a fraction when it has none.
fn quantity(value: f64) -> serde_json::Number {
    if value.fract() == 0.0 && value.abs() < 9.0e15 {
        (value as i64).into()
    } else {
        serde_json::Number::from_f64(value).unwrap_or_else(|| 0.into())
    }
}

/// A value in words, for an older form: the page's own words when it gave
/// them, else the number, then its unit. A value that is not there is a dash.
fn words(shown: Option<&str>, value: Option<&serde_json::Number>, unit: Option<&str>) -> String {
    let number = match (shown, value) {
        (Some(shown), _) => shown.to_owned(),
        (None, Some(value)) => value.to_string(),
        (None, None) => return "–".to_owned(),
    };
    match unit {
        // A word keeps a space before it; a sign (`%`, `°C`) does not.
        Some(unit) if unit.starts_with(char::is_alphanumeric) => format!("{number} {unit}"),
        Some(unit) => format!("{number}{unit}"),
        None => number,
    }
}

/// The first kind in `node`, or anywhere inside it, that a client drawing
/// vocabulary `vocab` has never heard of. `None` when the client can draw all
/// of it.
///
/// ```
/// use navree_native::{Node, VOCAB, vocab};
///
/// let node = Node::text("Orders");
/// assert_eq!(vocab::newer_than(&node, VOCAB), None);
/// ```
pub fn newer_than(node: &Node, vocab: u32) -> Option<Kind> {
    let mut stack = vec![node];
    while let Some(node) = stack.pop() {
        if node.kind().since() > vocab {
            return Some(node.kind());
        }
        stack.extend(node.children().into_iter().rev());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::{
        Avatar, Badge, Chart, ClockMoment, Colour, DayClock, DayPager, Direction, FeedRow, Figure,
        CalendarHeatmap, Category, ColorStrip, Gauge, HeatDay, Hour, Hue, Icon, Message, Meter,
        MiniColumn, MiniColumns, Moment, PagerWords, Pane, PaneRole, RangeScale, Ring, RingValue, Rings, RingsCentre,
        RowState, Run, Section, Segment, Stack, Standard, StripCell, Timeline, Tone, Toolbar,
        ToolbarItem, Trend, Unknown, Zone,
    };

    #[test]
    fn every_kind_names_itself_as_its_node_does() {
        let node = Node::Badge(Badge {
            text: "Paid".to_owned(),
            ..Badge::default()
        });
        let json = serde_json::to_value(&node).unwrap();
        assert_eq!(json["t"], node.kind().as_str());
        let mut names: Vec<&str> = Kind::ALL.iter().map(|kind| kind.as_str()).collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count, "two kinds share a name");
        // No kind or field is newer than the newest vocabulary, and a field
        // later than its type needs a vocabulary after the first.
        let at_most_the_newest = |since: u32| crate::VOCAB.checked_sub(since).is_some();
        for kind in Kind::ALL {
            assert!(at_most_the_newest(kind.since()), "{kind:?}");
        }
        for (ty, field, since) in LATER_FIELDS {
            assert!(*since > 0 && at_most_the_newest(*since), "{ty}.{field}");
        }
    }

    #[test]
    fn the_vocabulary_is_0_until_it_is_frozen() {
        assert_eq!(crate::VOCAB, 0);
        assert!(Kind::ALL.iter().all(|kind| kind.since() == 0));
        assert!(LATER_FIELDS.is_empty());
    }

    #[test]
    fn a_client_of_the_first_vocabulary_draws_every_kind() {
        let tree = Node::Section(Section {
            children: vec![Node::text("a"), Node::text("b")],
            ..Section::default()
        });
        assert_eq!(newer_than(&tree, crate::VOCAB), None);
        assert_eq!(newer_than(&tree, 0), None);
    }

    fn icon() -> Icon {
        Icon {
            name: Some("package".to_owned()),
            paths: vec!["M3 7l9-4 9 4v10l-9 4-9-4z".to_owned()],
            ..Icon::default()
        }
    }

    /// One of each kind that arrived with the vocabulary of design section 18.
    fn newer_parts() -> Vec<Node> {
        vec![
            Node::Toolbar(Toolbar {
                id: Some("bar".to_owned()),
                items: vec![
                    ToolbarItem {
                        title: "Share".to_owned(),
                        icon: icon(),
                        standard: Some(Standard::Share),
                        ..ToolbarItem::default()
                    },
                    ToolbarItem {
                        title: "More".to_owned(),
                        icon: icon(),
                        menu: vec![Node::text("Export")],
                        ..ToolbarItem::default()
                    },
                ],
            }),
            Node::Gauge(Gauge {
                id: Some("space".to_owned()),
                value: 30.into(),
                min: 20.into(),
                max: 60.into(),
                label: Some("Space".to_owned()),
                shown: Some("25%".to_owned()),
                ..Gauge::default()
            }),
            Node::Figure(Figure {
                label: "Revenue".to_owned(),
                value: "₹1,24,050".to_owned(),
                trend: Some(Trend {
                    direction: Direction::Up,
                    text: Some("12%".to_owned()),
                    tone: Some(Tone::Success),
                }),
                ..Figure::default()
            }),
            Node::Timeline(Timeline {
                id: None,
                items: vec![Moment {
                    title: "Packed".to_owned(),
                    time: Some("11:05".to_owned()),
                    state: Some(RowState::Done),
                    icon: Some(icon()),
                    ..Moment::default()
                }],
            }),
            Node::FeedRow(FeedRow {
                actor: Some(Avatar {
                    name: "Ravneet Kaur".to_owned(),
                    ..Avatar::default()
                }),
                runs: vec![
                    Run {
                        text: "Ravneet Kaur".to_owned(),
                        bold: true,
                        ..Run::default()
                    },
                    Run {
                        text: " packed CH-1042".to_owned(),
                        ..Run::default()
                    },
                ],
                ..FeedRow::default()
            }),
            Node::Message(Message {
                author: "Harpreet Sandhu".to_owned(),
                body: vec![Node::text("Where is my order?")],
                ..Message::default()
            }),
            Node::Pane(Pane {
                role: PaneRole::Detail,
                title: Some("Order".to_owned()),
                children: vec![Node::text("CH-1042")],
                ..Pane::default()
            }),
            Node::Ring(Ring {
                id: Some("steps".to_owned()),
                value: Some(12_450.into()),
                max: 10_000.into(),
                unit: Some("steps".to_owned()),
                shown: Some("12,450".to_owned()),
                label: Some("Steps".to_owned()),
                caption: Some("Goal reached".to_owned()),
                colour: Some(Colour::new(Hue::Success)),
                ..Ring::default()
            }),
            Node::CalendarHeatmap(CalendarHeatmap {
                id: Some("calendar".to_owned()),
                title: Some("Training calendar".to_owned()),
                days: (1..=100)
                    .map(|day| HeatDay {
                        date: format!("2026-{:02}-{:02}", 1 + (day - 1) / 28, 1 + (day - 1) % 28),
                        value: (day % 10 != 0).then(|| (day % 7).into()),
                    })
                    .collect(),
                label: Some("Load".to_owned()),
                ..CalendarHeatmap::default()
            }),
            Node::Range(RangeScale {
                id: Some("resting".to_owned()),
                title: Some("Resting heart rate".to_owned()),
                value: Some(54.into()),
                min: 40.into(),
                max: 120.into(),
                unit: Some("bpm".to_owned()),
                zones: vec![
                    Zone {
                        from: 40.into(),
                        to: 50.into(),
                        label: Some("Lower".to_owned()),
                        colour: None,
                    },
                    Zone {
                        from: 50.into(),
                        to: 70.into(),
                        label: Some("Usual".to_owned()),
                        colour: Some(Colour::new(Hue::Green)),
                    },
                ],
                ..RangeScale::default()
            }),
            Node::Strip(ColorStrip {
                id: Some("night".to_owned()),
                key: vec![
                    Category {
                        name: "Deep".to_owned(),
                        colour: Some(Colour::new(Hue::Violet)),
                    },
                    Category {
                        name: "Awake".to_owned(),
                        colour: Some(Colour::new(Hue::Warning)),
                    },
                ],
                cells: vec![
                    StripCell {
                        label: "11:00 pm".to_owned(),
                        category: Some(1),
                        value: None,
                    },
                    StripCell {
                        label: "11:30 pm".to_owned(),
                        category: Some(0),
                        value: None,
                    },
                    StripCell {
                        label: "12:00 am".to_owned(),
                        category: Some(0),
                        value: None,
                    },
                    // A cell with no reading is in no category.
                    StripCell {
                        label: "12:30 am".to_owned(),
                        ..StripCell::default()
                    },
                ],
                shares: true,
                ..ColorStrip::default()
            }),
            Node::Columns(MiniColumns {
                id: Some("cycles".to_owned()),
                parts: vec![
                    Category {
                        name: "Deep".to_owned(),
                        colour: Some(Colour::new(Hue::Violet)),
                    },
                    Category {
                        name: "REM".to_owned(),
                        colour: None,
                    },
                ],
                columns: vec![
                    MiniColumn {
                        label: "Cycle 1".to_owned(),
                        values: vec![Some(36.into()), Some(18.into())],
                        shown: Some("54 min".to_owned()),
                    },
                    // A part with no amount is left out of the column and of its details.
                    MiniColumn {
                        label: "Cycle 2".to_owned(),
                        values: vec![Some(22.into()), None],
                        shown: None,
                    },
                ],
                highlight: Some(1),
                ..MiniColumns::default()
            }),
            Node::Meter(Meter {
                id: Some("stages".to_owned()),
                segments: vec![
                    Segment {
                        name: "Deep".to_owned(),
                        value: Some(83.into()),
                        shown: Some("1 h 23 min".to_owned()),
                        colour: Some(Colour::new(Hue::Violet)),
                        ..Segment::default()
                    },
                    Segment {
                        name: "Core".to_owned(),
                        value: Some(253.into()),
                        detail: Some("Most of the night".to_owned()),
                        ..Segment::default()
                    },
                    // A part with no amount stays in the list, and has no share.
                    Segment {
                        name: "Awake".to_owned(),
                        ..Segment::default()
                    },
                ],
                ..Meter::default()
            }),
            Node::DayPager(DayPager {
                id: Some("pager".to_owned()),
                day: "Wed, 11 Nov".to_owned(),
                date: "2026-11-11".to_owned(),
                previous: Some("/?date=2026-11-10".to_owned()),
                next: None,
                scores: Some(Box::new(Node::Ring(Ring::default()))),
                words: Some(PagerWords {
                    previous: "Day before".to_owned(),
                    next: "Day after".to_owned(),
                    choose: "Pick a day".to_owned(),
                }),
                ..DayPager::default()
            }),
            Node::DayClock(DayClock {
                id: Some("day".to_owned()),
                title: Some("Steps by hour".to_owned()),
                hours: vec![
                    Hour {
                        value: Some(120.into()),
                        ..Hour::default()
                    },
                    Hour {
                        value: Some(1850.into()),
                        ..Hour::default()
                    },
                    Hour::default(),
                ],
                moments: vec![
                    ClockMoment {
                        at: quantity(6.5),
                        name: "Woke up".to_owned(),
                        colour: Some(Colour::new(Hue::Amber)),
                        ..ClockMoment::default()
                    },
                    ClockMoment {
                        at: 18.into(),
                        name: "Evening walk".to_owned(),
                        colour: Some(Colour::new(Hue::Success)),
                        ..ClockMoment::default()
                    },
                ],
                unit: Some("steps".to_owned()),
                ..DayClock::default()
            }),
            Node::Rings(Rings {
                id: Some("activity".to_owned()),
                rings: vec![
                    RingValue {
                        name: "Move".to_owned(),
                        value: Some(540.into()),
                        max: 600.into(),
                        unit: Some("kcal".to_owned()),
                        shown: Some("540".to_owned()),
                        colour: Some(Colour::new(Hue::Red)),
                    },
                    RingValue {
                        name: "Exercise".to_owned(),
                        value: None,
                        max: 30.into(),
                        unit: Some("min".to_owned()),
                        ..RingValue::default()
                    },
                ],
                centre: RingsCentre::Values,
                ..Rings::default()
            }),
        ]
    }

    fn arrived_later(kind: Kind) -> u32 {
        match kind {
            Kind::Toolbar
            | Kind::Gauge
            | Kind::Figure
            | Kind::Timeline
            | Kind::FeedRow
            | Kind::Message
            | Kind::Pane
            | Kind::Ring
            | Kind::Rings
            | Kind::DayClock
            | Kind::DayPager
            | Kind::Meter
            | Kind::Columns
            | Kind::Strip
            | Kind::Range
            | Kind::CalendarHeatmap => 1,
            kind => kind.since(),
        }
    }

    /// A vocabulary in which only `newer` has yet to arrive, so the older form
    /// of a kind is seen as it is written, not as the kinds it is written in
    /// would be rewritten in turn.
    fn only(newer: Kind) -> impl Fn(Kind) -> u32 {
        move |kind| u32::from(kind == newer)
    }

    /// The part of `kind` among the newer parts.
    fn part(kind: Kind) -> Node {
        newer_parts()
            .into_iter()
            .find(|part| part.kind() == kind)
            .unwrap_or_else(|| panic!("no example of {kind:?}"))
    }

    fn kinds_in(node: &Node) -> Vec<Kind> {
        let mut kinds = Vec::new();
        let mut stack = vec![node];
        while let Some(node) = stack.pop() {
            kinds.push(node.kind());
            stack.extend(node.children());
        }
        kinds
    }

    #[test]
    fn nothing_changes_for_the_vocabulary_a_client_draws() {
        let tree = Node::Stack(Stack {
            children: newer_parts(),
            ..Stack::default()
        });
        assert_eq!(downgrade(tree.clone(), crate::VOCAB), Downgraded::Node(tree.clone()));
        assert_eq!(downgrade_by(tree.clone(), 1, arrived_later), Downgraded::Node(tree));
    }

    #[test]
    fn a_newer_kind_takes_its_older_form() {
        for part in newer_parts() {
            let kind = part.kind();
            let Downgraded::Node(older) = downgrade_by(part, 0, arrived_later) else {
                panic!("{kind:?} has no older form");
            };
            assert!(
                kinds_in(&older).iter().all(|kind| arrived_later(*kind) == 0),
                "{kind:?} became {older:?}"
            );
        }
        let Downgraded::Node(Node::Progress(progress)) =
            downgrade_by(newer_parts().swap_remove(1), 0, arrived_later)
        else {
            panic!("a gauge becomes a progress");
        };
        assert_eq!(progress.value, Some(250));
        assert_eq!(progress.label.as_deref(), Some("Space: 25%"));
    }

    #[test]
    fn a_ring_becomes_a_figure_with_its_words_and_its_goal() {
        let ring = part(Kind::Ring);
        let Downgraded::Node(Node::Figure(figure)) = downgrade_by(ring, 0, only(Kind::Ring)) else {
            panic!("a ring becomes a figure");
        };
        assert_eq!(figure.id.as_deref(), Some("steps"));
        assert_eq!(figure.label, "Steps");
        assert_eq!(figure.value, "12,450 steps");
        assert_eq!(figure.detail.as_deref(), Some("Goal reached · of 10000 steps"));
        assert_eq!(figure.tone, Some(Tone::Success));
        // A day with no reading is a dash, and a unit that is a sign keeps no space.
        let empty = Node::Ring(Ring {
            label: Some("Recovery".to_owned()),
            unit: Some("%".to_owned()),
            ..Ring::default()
        });
        let Downgraded::Node(Node::Figure(figure)) = downgrade_by(empty, 0, only(Kind::Ring)) else {
            panic!("a ring becomes a figure");
        };
        assert_eq!(figure.value, "–");
        assert_eq!(figure.detail.as_deref(), Some("of 100%"));
        assert_eq!(figure.tone, None);
    }

    fn zone(from: f64, to: f64, label: &str, hue: Hue) -> Zone {
        Zone {
            from: quantity(from),
            to: quantity(to),
            label: Some(label.to_owned()),
            colour: Some(Colour::new(hue)),
        }
    }

    #[test]
    fn a_gauge_with_zones_says_the_zone_its_value_is_in() {
        let gauge = Node::Gauge(Gauge {
            value: quantity(1.12),
            max: 2.into(),
            label: Some("Load ratio".to_owned()),
            shown: Some("1.12".to_owned()),
            zones: vec![
                zone(0.0, 0.8, "Undertraining", Hue::Violet),
                zone(0.8, 1.3, "Optimal", Hue::Green),
                // Where zones overlap, the last holds the value.
                zone(1.0, 2.0, "Overuse", Hue::Red),
            ],
            ..Gauge::default()
        });
        let Some(Node::Progress(progress)) = gauge.older() else {
            panic!("a gauge becomes a progress");
        };
        assert_eq!(progress.label.as_deref(), Some("Load ratio: 1.12 (Overuse)"));
        assert_eq!(progress.value, Some(560));
    }

    #[test]
    fn a_calendar_becomes_a_bar_for_each_of_its_latest_days() {
        let calendar = part(Kind::CalendarHeatmap);
        let Downgraded::Node(Node::Chart(chart)) =
            downgrade_by(calendar, 0, only(Kind::CalendarHeatmap))
        else {
            panic!("a calendar becomes a chart");
        };
        assert_eq!(chart.kind, crate::node::ChartKind::Bar);
        // 100 days are given and the 90 latest are shown.
        assert_eq!(chart.labels.len(), 90);
        assert_eq!(chart.labels.last().map(String::as_str), Some("2026-04-16"));
        assert_eq!(chart.series[0].label, "Load");
        assert_eq!(chart.series[0].values.len(), 90);
        // The 100th day has no reading, the 99th came to 99 % 7 = 1, and the 90th has none either.
        assert_eq!(chart.series[0].values.last(), Some(&None));
        assert_eq!(chart.series[0].values[88], Some(1.into()));
        assert_eq!(chart.series[0].values[0], Some((11 % 7).into()));
        assert_eq!(chart.series[0].values[79], None);
    }

    #[test]
    fn a_range_becomes_a_figure_that_says_the_zone_and_the_scale() {
        let range = part(Kind::Range);
        let Downgraded::Node(Node::Figure(figure)) = downgrade_by(range, 0, only(Kind::Range))
        else {
            panic!("a range becomes a figure");
        };
        assert_eq!(figure.label, "Resting heart rate");
        assert_eq!(figure.value, "54 bpm");
        assert_eq!(
            figure.detail.as_deref(),
            Some("Usual · on a scale from 40 bpm to 120 bpm")
        );
        // Words under the ends take the place of their numbers; with no reading there is a dash.
        let described = Node::Range(RangeScale {
            low: Some("Slower".to_owned()),
            high: Some("Faster".to_owned()),
            ..RangeScale::default()
        });
        let Some(Node::Figure(figure)) = described.older() else {
            panic!("a range becomes a figure");
        };
        assert_eq!((figure.value.as_str(), figure.detail.as_deref()), ("–", Some("from Slower to Faster")));
    }

    #[test]
    fn a_strip_becomes_each_category_with_its_share_of_the_cells() {
        let strip = part(Kind::Strip);
        let Downgraded::Node(Node::Facts(facts)) = downgrade_by(strip, 0, only(Kind::Strip)) else {
            panic!("a strip becomes facts");
        };
        assert_eq!(facts.id.as_deref(), Some("night"));
        let share = |at: usize| match &facts.items[at].value[..] {
            [Node::Text(text)] => text.value.clone(),
            other => panic!("{other:?}"),
        };
        // Two of the four cells are deep, one is awake and one has no reading.
        assert_eq!((facts.items[0].label.as_str(), share(0)), ("Deep", "50%".to_owned()));
        assert_eq!((facts.items[1].label.as_str(), share(1)), ("Awake", "25%".to_owned()));
        assert_eq!(facts.items[1].tone, Some(Tone::Warning));
    }

    #[test]
    fn columns_become_a_list_of_facts_one_for_each_column_with_its_parts() {
        let columns = part(Kind::Columns);
        let Downgraded::Node(Node::Facts(facts)) = downgrade_by(columns, 0, only(Kind::Columns))
        else {
            panic!("columns become facts");
        };
        assert_eq!(facts.id.as_deref(), Some("cycles"));
        let value = |at: usize| match &facts.items[at].value[..] {
            [Node::Text(text)] => text.value.clone(),
            other => panic!("{other:?}"),
        };
        assert_eq!((facts.items[0].label.as_str(), value(0)), ("Cycle 1", "54 min".to_owned()));
        assert_eq!(facts.items[0].details, ["Deep 36", "REM 18"]);
        // With no total in words, the sum of the parts; an empty part has no detail.
        assert_eq!((facts.items[1].label.as_str(), value(1)), ("Cycle 2", "22".to_owned()));
        assert_eq!(facts.items[1].details, ["Deep 22"]);
    }

    #[test]
    fn a_meter_becomes_a_list_of_facts_with_the_share_of_each_part() {
        let meter = part(Kind::Meter);
        let Downgraded::Node(Node::Facts(facts)) = downgrade_by(meter, 0, only(Kind::Meter)) else {
            panic!("a meter becomes facts");
        };
        assert_eq!(facts.id.as_deref(), Some("stages"));
        let labels: Vec<&str> = facts.items.iter().map(|fact| fact.label.as_str()).collect();
        assert_eq!(labels, ["Deep", "Core", "Awake"]);
        // The page's words when it gave them, else the number; 83 of 336 is a quarter.
        let value = |at: usize| match &facts.items[at].value[..] {
            [Node::Text(text)] => text.value.clone(),
            other => panic!("{other:?}"),
        };
        assert_eq!((value(0), value(1), value(2)), ("1 h 23 min".into(), "253".into(), "–".into()));
        assert_eq!(facts.items[0].details, ["25%"]);
        assert_eq!(facts.items[1].details, ["75%", "Most of the night"]);
        assert!(facts.items[2].details.is_empty());
    }

    #[test]
    fn a_day_pager_becomes_the_day_between_two_buttons() {
        let pager = part(Kind::DayPager);
        let Downgraded::Node(Node::Stack(stack)) = downgrade_by(pager, 0, only(Kind::DayPager))
        else {
            panic!("a pager becomes a stack");
        };
        assert_eq!(stack.id.as_deref(), Some("pager"));
        let kinds: Vec<Kind> = stack.children.iter().map(Node::kind).collect();
        assert_eq!(kinds, [Kind::Button, Kind::Ring, Kind::Text, Kind::Button]);
        let (Node::Button(before), Node::Button(after)) = (&stack.children[0], &stack.children[3])
        else {
            panic!("buttons stand at the ends");
        };
        // The day before is a button that goes there, in the page's words; there is no day after.
        assert_eq!(before.label, "Day before");
        assert_eq!(
            before.action,
            Some(crate::Action::navigate("/?date=2026-11-10"))
        );
        assert!(!before.disabled);
        assert_eq!(after.label, "Day after");
        assert!(after.disabled && after.action.is_none());
    }

    #[test]
    fn a_day_clock_becomes_its_total_and_its_moments_at_their_times() {
        let clock = part(Kind::DayClock);
        let Downgraded::Node(Node::Facts(facts)) = downgrade_by(clock, 0, only(Kind::DayClock))
        else {
            panic!("a clock becomes facts");
        };
        assert_eq!(facts.id.as_deref(), Some("day"));
        let at = |index: usize| match &facts.items[index].value[..] {
            [Node::Text(text)] => text.value.clone(),
            other => panic!("{other:?}"),
        };
        // 120 + 1850 steps, and no hour of zero among the hours with no reading.
        assert_eq!((facts.items[0].label.as_str(), at(0)), ("Steps by hour", "1970 steps".to_owned()));
        assert_eq!((facts.items[1].label.as_str(), at(1)), ("Woke up", "06:30".to_owned()));
        assert_eq!((facts.items[2].label.as_str(), at(2)), ("Evening walk", "18:00".to_owned()));
        assert_eq!(facts.items[2].tone, Some(Tone::Success));
    }

    #[test]
    fn a_figure_that_carries_a_tile_keeps_its_unit_its_status_and_its_graphics() {
        let tile = Node::Figure(Figure {
            label: "Resting heart rate".to_owned(),
            value: "58".to_owned(),
            unit: Some("bpm".to_owned()),
            icon: Some(icon()),
            status: Some(Badge {
                text: "Athletic".to_owned(),
                tone: Some(Tone::Success),
                ..Badge::default()
            }),
            detail: Some("vs last week".to_owned()),
            aside: Some(Box::new(Node::Ring(Ring::default()))),
            chart: Some(Box::new(Node::Chart(Chart::default()))),
            ..Figure::default()
        });
        let Some(Node::Facts(facts)) = tile.older() else {
            panic!("a figure becomes facts");
        };
        let fact = &facts.items[0];
        assert_eq!(fact.icon, Some(icon()));
        assert_eq!(fact.details, ["vs last week", "Athletic"]);
        let kinds: Vec<Kind> = fact.value.iter().map(Node::kind).collect();
        assert_eq!(kinds, [Kind::Text, Kind::Ring, Kind::Chart]);
        assert!(matches!(&fact.value[0], Node::Text(text) if text.value == "58 bpm"));
    }

    #[test]
    fn rings_become_a_list_of_facts_one_for_each_ring() {
        let rings = part(Kind::Rings);
        let Downgraded::Node(Node::Facts(facts)) = downgrade_by(rings, 0, only(Kind::Rings)) else {
            panic!("rings become facts");
        };
        assert_eq!(facts.id.as_deref(), Some("activity"));
        let value = |at: usize| match &facts.items[at].value[..] {
            [Node::Text(text)] => text.value.clone(),
            other => panic!("{other:?}"),
        };
        assert_eq!(facts.items[0].label, "Move");
        assert_eq!(value(0), "540 of 600 kcal");
        assert_eq!(facts.items[0].tone, None);
        assert_eq!(facts.items[1].label, "Exercise");
        assert_eq!(value(1), "– of 30 min");
    }

    #[test]
    fn a_newer_kind_with_no_older_form_sends_the_page_to_the_web_view() {
        let tree = Node::Stack(Stack {
            children: vec![
                Node::text("Sales"),
                Node::Chart(Chart::default()),
                Node::Section(Section {
                    children: vec![Node::Chart(Chart::default())],
                    ..Section::default()
                }),
            ],
            ..Stack::default()
        });
        let since = |kind: Kind| if kind == Kind::Chart { 1 } else { 0 };
        assert_eq!(
            downgrade_by(tree, 0, since),
            Downgraded::Web {
                why: vec!["vocab:chart".to_owned()]
            }
        );
    }

    #[test]
    fn a_kind_no_vocabulary_holds_takes_its_alt_or_sends_the_page_to_the_web_view() {
        let with_alt = Node::Unknown(Unknown {
            kind: "hologram".to_owned(),
            id: None,
            alt: Some(Box::new(Node::text("A view of the warehouse"))),
        });
        assert_eq!(
            downgrade(with_alt, crate::VOCAB),
            Downgraded::Node(Node::text("A view of the warehouse"))
        );
        let bare = Node::Unknown(Unknown {
            kind: "hologram".to_owned(),
            ..Unknown::default()
        });
        assert_eq!(
            downgrade(bare, crate::VOCAB),
            Downgraded::Web {
                why: vec!["vocab:hologram".to_owned()]
            }
        );
    }

    #[test]
    fn a_tree_500_deep_is_downgraded_and_searched_without_running_out_of_stack() {
        let mut tree = newer_parts().swap_remove(1);
        for depth in 0..500 {
            tree = Node::Stack(Stack {
                id: Some(format!("level-{depth}")),
                children: vec![tree],
                ..Stack::default()
            });
        }
        assert!(tree.find("space").is_some());
        assert_eq!(newer_than(&tree, crate::VOCAB), None);
        let Downgraded::Node(older) = downgrade_by(tree, 0, arrived_later) else {
            panic!("a gauge has an older form");
        };
        assert!(older.find("space").is_some_and(|node| node.kind() == Kind::Progress));
    }
}
