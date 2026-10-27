//! The structure tree of a tagged PDF, built while laying out: nodes
//! (paragraphs, headings, lists, tables, figures, links) and the slots
//! drawn content fills.

/// What a structure node is.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum NodeKind {
    P,
    /// A heading, level 1 to 6.
    H(u8),
    /// A list, ordered or not.
    L(bool),
    Li,
    Lbl,
    LBody,
    Table,
    THead,
    TBody,
    Tr,
    Th,
    Td,
    /// An image, with its alt text.
    Figure(Option<String>),
    Link,
}

/// A node's child: another node or a slot of drawn content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Child {
    Node(NodeId),
    Slot(SlotId),
}

pub(crate) type NodeId = usize;
pub(crate) type SlotId = usize;

#[derive(Clone, Debug)]
pub(crate) struct Node {
    pub(crate) kind: NodeKind,
    pub(crate) children: Vec<Child>,
}

/// The tree: top-level nodes in reading order.
#[derive(Clone, Debug, Default)]
pub(crate) struct StructTree {
    pub(crate) nodes: Vec<Node>,
    pub(crate) roots: Vec<NodeId>,
    pub(crate) slots: usize,
}

impl StructTree {
    /// A new node under `parent` (or at the top).
    pub(crate) fn node(&mut self, parent: Option<NodeId>, kind: NodeKind) -> NodeId {
        let id = self.nodes.len();
        self.nodes.push(Node {
            kind,
            children: Vec::new(),
        });
        match parent {
            Some(parent) => self.nodes[parent].children.push(Child::Node(id)),
            None => self.roots.push(id),
        }
        id
    }

    /// A new slot of content in `node`.
    pub(crate) fn slot(&mut self, node: NodeId) -> SlotId {
        let id = self.slots;
        self.slots += 1;
        self.nodes[node].children.push(Child::Slot(id));
        id
    }
}

/// How a drawn item is tagged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mark {
    /// Content of a structure node, through this slot.
    Content(SlotId),
    /// Not content: page furniture.
    Artifact(Furniture),
}

/// Kinds of artifacts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Furniture {
    Header,
    Footer,
    /// Rules, borders, backgrounds, underlines.
    Layout,
    /// A table header repeated on a continuation page.
    Pagination,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nodes_and_slots_keep_their_order() {
        let mut tree = StructTree::default();
        let p = tree.node(None, NodeKind::P);
        let first = tree.slot(p);
        let link = tree.node(Some(p), NodeKind::Link);
        let inside = tree.slot(link);
        let last = tree.slot(p);
        assert_eq!(tree.roots, vec![p]);
        assert_eq!(
            tree.nodes[p].children,
            vec![Child::Slot(first), Child::Node(link), Child::Slot(last)]
        );
        assert_eq!(tree.nodes[link].children, vec![Child::Slot(inside)]);
        assert_eq!(tree.slots, 3);
    }
}
