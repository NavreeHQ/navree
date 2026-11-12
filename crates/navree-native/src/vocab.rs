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
    Stack => "stack" since 0,
    Grid => "grid" since 0,
    Section => "section" since 0,
    List => "list" since 0,
    Row => "row" since 0,
    Split => "split" since 0,
    Tabs => "tabs" since 0,
    Disclosure => "disclosure" since 0,
    Divider => "divider" since 0,
    Spacer => "spacer" since 0,
    Region => "region" since 0,
    Text => "text" since 0,
    Rich => "rich" since 0,
    Image => "image" since 0,
    Icon => "icon" since 0,
    Avatar => "avatar" since 0,
    Badge => "badge" since 0,
    Progress => "progress" since 0,
    Banner => "banner" since 0,
    Empty => "empty" since 0,
    Facts => "facts" since 0,
    Table => "table" since 0,
    Chart => "chart" since 0,
    Code => "code" since 0,
    Button => "button" since 0,
    Link => "link" since 0,
    Menu => "menu" since 0,
    Form => "form" since 0,
    Field => "field" since 0,
    Sheet => "sheet" since 0,
    Dialog => "dialog" since 0,
    Popover => "popover" since 0,
    Native => "native" since 0,
    Web => "web" since 0,
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
