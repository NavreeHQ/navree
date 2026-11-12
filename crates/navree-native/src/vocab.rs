//! [`Kind`]: the name of every node kind and the vocabulary it arrived in.
//!
//! An app in a store can't be reloaded like a web page, so a server writes each
//! answer for the vocabulary the client says it draws. A kind the client is too
//! old for is found with [`newer_than`] before an answer leaves.

use serde::de::Error as _;
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
        impl<'de> Deserialize<'de> for Node {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let value = serde_json::Value::deserialize(deserializer)?;
                let Some(name) = value.get("t").and_then(serde_json::Value::as_str) else {
                    return Err(D::Error::custom("a node names its kind in `t`"));
                };
                if Kind::parse(name).is_none() {
                    let name = name.to_owned();
                    return node::Unknown::read(&name, &value)
                        .map(Node::Unknown)
                        .map_err(D::Error::custom);
                }
                match Read::deserialize(value).map_err(D::Error::custom)? {
                    $(Read::$variant(node) => Ok(Node::$variant(node)),)+
                }
            }
        }
    };
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
}

/// A field that arrived after its kind did: the kind, the field's name in the
/// JSON, and the vocabulary it arrived in. An older client passes over it, so
/// nothing is done about these when an answer is written; the list is what the
/// generated models and the reference are made from.
pub const LATER_FIELDS: &[(Kind, &str, u32)] = &[];

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
    use crate::node::{Badge, Section};

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
        for kind in Kind::ALL {
            assert!(kind.since() <= crate::VOCAB, "{kind:?}");
        }
        for (kind, field, since) in LATER_FIELDS {
            assert!(*since > kind.since(), "{kind:?}.{field}");
            assert!(*since <= crate::VOCAB, "{kind:?}.{field}");
        }
    }

    #[test]
    fn the_vocabulary_is_0_until_it_is_frozen() {
        assert_eq!(crate::VOCAB, 0);
        assert!(Kind::ALL.iter().all(|kind| kind.since() == 0));
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
}
