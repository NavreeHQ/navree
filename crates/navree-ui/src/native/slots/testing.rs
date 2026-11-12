//! What the readers' tests share: reading a component and asking what it
//! lowered to, and putting a hook no reader knows into a component to see that
//! the reading stops.

use super::super::lower::{N, lower, slot};
use crate::node::{Attr, AttrValue, Node};

/// `node` read; it must be read whole.
pub(crate) fn read(node: impl Into<Node>) -> N {
    let lowered = lower(&node.into());
    assert!(lowered.is_native(), "{:?}", lowered.misses);
    lowered.node
}

/// What stopped the reading of `node`, each thing once.
pub(crate) fn misses(node: impl Into<Node>) -> Vec<String> {
    let lowered = lower(&node.into());
    assert!(!lowered.is_native(), "the reading did not stop");
    lowered.why()
}

/// `node` with the hook `hook` put on its first element whose slot is `name`.
pub(crate) fn tamper(node: impl Into<Node>, name: &str, hook: &'static str) -> Node {
    let mut node = node.into();
    assert!(add(&mut node, name, hook), "no element with the slot `{name}`");
    node
}

fn add(node: &mut Node, name: &str, hook: &'static str) -> bool {
    match node {
        Node::Element(element) => {
            if slot(element).as_deref() == Some(name) {
                element.attrs.push(Attr {
                    name: hook,
                    value: AttrValue::Flag,
                });
                return true;
            }
            element.children.iter_mut().any(|child| add(child, name, hook))
        }
        Node::Fragment(nodes) => nodes.iter_mut().any(|child| add(child, name, hook)),
        _ => false,
    }
}
