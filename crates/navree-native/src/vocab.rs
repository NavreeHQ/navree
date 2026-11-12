//! [`Kind`]: the name of every node kind and the vocabulary it arrived in.
//!
//! An app in a store can't be reloaded like a web page, so a server writes each
//! answer for the vocabulary the client says it draws. A kind the client is too
//! old for is found with [`newer_than`] before an answer leaves.

use crate::node::Node;

macro_rules! kinds {
    ($($variant:ident => $name:literal since $since:literal,)+) => {
        /// The kind of a node: its `t`.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        #[non_exhaustive]
        pub enum Kind {
            $(
                #[doc = concat!("`", $name, "`.")]
                $variant,
            )+
        }

        impl Kind {
            /// Every kind of the current vocabulary.
            pub const ALL: &'static [Kind] = &[$(Kind::$variant,)+];

            /// The kind as a node's `t` names it.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $name,)+
                }
            }

            /// The vocabulary the kind arrived in. A client that draws an older
            /// one has never heard of it.
            pub const fn since(self) -> u32 {
                match self {
                    $(Self::$variant => $since,)+
                }
            }
        }

        impl Node {
            /// The node's kind.
            pub const fn kind(&self) -> Kind {
                match self {
                    $(Self::$variant(_) => Kind::$variant,)+
                }
            }
        }
    };
}

kinds! {
    Stack => "stack" since 1,
    Grid => "grid" since 1,
    Section => "section" since 1,
    List => "list" since 1,
    Row => "row" since 1,
    Split => "split" since 1,
    Tabs => "tabs" since 1,
    Disclosure => "disclosure" since 1,
    Divider => "divider" since 1,
    Spacer => "spacer" since 1,
    Region => "region" since 1,
    Text => "text" since 1,
    Rich => "rich" since 1,
    Image => "image" since 1,
    Icon => "icon" since 1,
    Avatar => "avatar" since 1,
    Badge => "badge" since 1,
    Progress => "progress" since 1,
    Banner => "banner" since 1,
    Empty => "empty" since 1,
    Facts => "facts" since 1,
    Table => "table" since 1,
    Chart => "chart" since 1,
    Code => "code" since 1,
    Button => "button" since 1,
    Link => "link" since 1,
    Menu => "menu" since 1,
    Form => "form" since 1,
    Field => "field" since 1,
    Sheet => "sheet" since 1,
    Dialog => "dialog" since 1,
    Popover => "popover" since 1,
    Native => "native" since 1,
    Web => "web" since 1,
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
/// use navree_native::{Kind, Node, vocab};
///
/// let node = Node::text("Orders");
/// assert_eq!(vocab::newer_than(&node, 1), None);
/// assert_eq!(vocab::newer_than(&node, 0), Some(Kind::Text));
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
            assert!((1..=crate::VOCAB).contains(&kind.since()), "{kind:?}");
        }
        for (kind, field, since) in LATER_FIELDS {
            assert!(*since > kind.since(), "{kind:?}.{field}");
            assert!(*since <= crate::VOCAB, "{kind:?}.{field}");
        }
    }

    #[test]
    fn a_kind_newer_than_the_client_is_found_however_deep() {
        let tree = Node::Section(Section {
            children: vec![Node::text("a"), Node::text("b")],
            ..Section::default()
        });
        assert_eq!(newer_than(&tree, crate::VOCAB), None);
        assert_eq!(newer_than(&tree, 0), Some(Kind::Section));
    }
}
