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
                Node::Facts(n::Facts {
                    id: node.id.clone(),
                    items: vec![n::Fact {
                        label: node.label.clone(),
                        value: vec![Node::text(node.value.clone())],
                        icon: None,
                        tone: node
                            .tone
                            .or_else(|| node.trend.as_ref().and_then(|trend| trend.tone)),
                        details: trend.into_iter().chain(node.detail.clone()).collect(),
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
            Self::Unknown(node) => return node.alt.as_deref().cloned(),
            _ => return None,
        })
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
        Avatar, Badge, Chart, Direction, FeedRow, Figure, Gauge, Icon, Message, Moment, Pane,
        PaneRole, RowState, Run, Section, Stack, Standard, Timeline, Tone, Toolbar, ToolbarItem,
        Trend, Unknown,
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
                tone: None,
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
            | Kind::Pane => 1,
            kind => kind.since(),
        }
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
