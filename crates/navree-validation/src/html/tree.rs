//! The tree [`HtmlPolicy::clean`](super::HtmlPolicy::clean) works on.
//!
//! html5ever parses the input as a fragment in a `<div>` into a node arena
//! (indices, no reference cycles), and its serializer writes the tree back.
//! Every walk over the tree uses its own stack, so a deeply nested input
//! can't overflow the call stack.

use std::borrow::Cow;
use std::cell::RefCell;
use std::io;
use std::rc::Rc;

use html5ever::serialize::{HtmlSerializer, Serialize, SerializeOpts, Serializer, TraversalScope};
use html5ever::tendril::{StrTendril, TendrilSink};
use html5ever::tree_builder::{ElementFlags, NodeOrText, QuirksMode, TreeSink};
use html5ever::{Attribute, ParseOpts, QualName, local_name, ns, parse_fragment};

/// A node's index in [`Tree::nodes`].
pub(crate) type Id = usize;

/// What a node is.
pub(crate) enum Data {
    /// The document, or a template's contents.
    Document,
    Element {
        name: Rc<QualName>,
        attrs: Vec<Attribute>,
        /// A `<template>`'s contents: a separate fragment, never part of the
        /// tree that is written out.
        template: Option<Id>,
        /// A MathML `annotation-xml` that holds HTML or SVG.
        integration_point: bool,
    },
    Text(StrTendril),
    Comment,
    /// A doctype or a processing instruction.
    Other,
}

pub(crate) struct Node {
    pub(crate) parent: Option<Id>,
    pub(crate) children: Vec<Id>,
    pub(crate) data: Data,
}

impl Node {
    fn new(data: Data) -> Self {
        Node {
            parent: None,
            children: Vec::new(),
            data,
        }
    }

    /// The element's name, `None` for other nodes.
    pub(crate) fn name(&self) -> Option<&QualName> {
        match &self.data {
            Data::Element { name, .. } => Some(name),
            _ => None,
        }
    }

    /// Whether this is the HTML element `local`.
    pub(crate) fn is_html(&self, local: &str) -> bool {
        self.name()
            .is_some_and(|name| name.ns == ns!(html) && &*name.local == local)
    }
}

/// A parsed fragment: `root` is the `<html>` element the fragment's nodes
/// are children of.
pub(crate) struct Tree {
    pub(crate) nodes: Vec<Node>,
    pub(crate) root: Option<Id>,
}

/// One step of a walk over a tree, for comparing two trees.
#[derive(Debug, PartialEq)]
pub(crate) enum Event<'a> {
    Open(&'a QualName, &'a [Attribute]),
    Close,
    /// Adjacent text, joined.
    Text(String),
    Comment,
    Other,
}

impl Tree {
    /// `html` parsed as the content of a `<div>`, the way a browser parses
    /// it when it is inserted into one.
    pub(crate) fn parse(html: &str) -> Tree {
        let parser = parse_fragment(
            Sink::default(),
            ParseOpts::default(),
            QualName::new(None, ns!(html), local_name!("div")),
            Vec::new(),
            false,
        );
        let nodes = parser.one(html).nodes.into_inner();
        let root = nodes
            .first()
            .and_then(|document| document.children.first().copied());
        Tree { nodes, root }
    }

    /// The root's children, written as HTML.
    pub(crate) fn to_html(&self) -> String {
        let mut out = Vec::new();
        let opts = SerializeOpts::default();
        let scope = opts.traversal_scope.clone();
        let mut serializer = HtmlSerializer::new(&mut out, opts);
        // Writing to a `Vec` can't fail.
        let _ = Serialize::serialize(self, &mut serializer, scope);
        String::from_utf8_lossy(&out).into_owned()
    }

    /// The walk over the root's children, with the elements `skip` returns
    /// `true` for left out (their children are kept).
    pub(crate) fn events(&self, skip: impl Fn(&QualName, &[Attribute]) -> bool) -> Vec<Event<'_>> {
        enum Step {
            Enter(Id),
            Leave,
        }
        let mut events = Vec::new();
        let mut stack: Vec<Step> = self
            .root
            .and_then(|root| self.nodes.get(root))
            .map(|root| {
                root.children
                    .iter()
                    .rev()
                    .map(|&id| Step::Enter(id))
                    .collect()
            })
            .unwrap_or_default();
        while let Some(step) = stack.pop() {
            let id = match step {
                Step::Enter(id) => id,
                Step::Leave => {
                    events.push(Event::Close);
                    continue;
                }
            };
            let Some(node) = self.nodes.get(id) else {
                continue;
            };
            match &node.data {
                Data::Element { name, attrs, .. } => {
                    if !skip(name, attrs) {
                        events.push(Event::Open(name, attrs));
                        stack.push(Step::Leave);
                    }
                    stack.extend(node.children.iter().rev().map(|&id| Step::Enter(id)));
                }
                Data::Text(text) => match events.last_mut() {
                    Some(Event::Text(previous)) => previous.push_str(text),
                    _ => events.push(Event::Text(text.to_string())),
                },
                Data::Comment => events.push(Event::Comment),
                Data::Document | Data::Other => events.push(Event::Other),
            }
        }
        events
    }

    /// All the text under the root, in order.
    pub(crate) fn text(&self) -> String {
        self.events(|_, _| true)
            .into_iter()
            .filter_map(|event| match event {
                Event::Text(text) => Some(text),
                _ => None,
            })
            .collect()
    }
}

/// `text` escaped as HTML text (`<` is `&lt;`).
pub(crate) fn escape_text(text: &str) -> String {
    let mut out = Vec::new();
    let mut serializer = HtmlSerializer::new(&mut out, SerializeOpts::default());
    // Writing to a `Vec` can't fail.
    let _ = serializer.write_text(text);
    String::from_utf8_lossy(&out).into_owned()
}

impl Serialize for Tree {
    fn serialize<S: Serializer>(&self, serializer: &mut S, _: TraversalScope) -> io::Result<()> {
        enum Step<'a> {
            Open(Id),
            Close(&'a QualName),
        }
        let Some(root) = self.root.and_then(|root| self.nodes.get(root)) else {
            return Ok(());
        };
        let mut stack: Vec<Step<'_>> = root
            .children
            .iter()
            .rev()
            .map(|&id| Step::Open(id))
            .collect();
        while let Some(step) = stack.pop() {
            let id = match step {
                Step::Open(id) => id,
                Step::Close(name) => {
                    serializer.end_elem(name.clone())?;
                    continue;
                }
            };
            let Some(node) = self.nodes.get(id) else {
                continue;
            };
            match &node.data {
                Data::Element { name, attrs, .. } => {
                    serializer.start_elem(
                        (**name).clone(),
                        attrs.iter().map(|attr| (&attr.name, &attr.value[..])),
                    )?;
                    stack.push(Step::Close(name));
                    stack.extend(node.children.iter().rev().map(|&id| Step::Open(id)));
                }
                Data::Text(text) => serializer.write_text(text)?,
                // Comments, doctypes and processing instructions are never
                // written.
                Data::Document | Data::Comment | Data::Other => {}
            }
        }
        Ok(())
    }
}

/// html5ever's view of a node: its index, and an element's name (the tree
/// builder reads names by reference).
#[derive(Clone)]
struct Handle {
    id: Id,
    name: Rc<QualName>,
}

/// Builds a [`Tree`] as html5ever parses. The tree builder only ever calls
/// one method at a time, so each method borrows the arena for its own
/// duration.
struct Sink {
    nodes: RefCell<Vec<Node>>,
    /// The name of handles that aren't elements (never read).
    unnamed: Rc<QualName>,
}

impl Default for Sink {
    fn default() -> Self {
        Sink {
            nodes: RefCell::new(vec![Node::new(Data::Document)]),
            unnamed: Rc::new(QualName::new(None, ns!(), local_name!(""))),
        }
    }
}

impl Sink {
    fn push(&self, data: Data) -> Id {
        let mut nodes = self.nodes.borrow_mut();
        nodes.push(Node::new(data));
        nodes.len() - 1
    }

    fn handle(&self, id: Id) -> Handle {
        Handle {
            id,
            name: self.unnamed.clone(),
        }
    }

    /// The node's parent and its position among the parent's children.
    fn position(nodes: &[Node], id: Id) -> Option<(Id, usize)> {
        let parent = nodes.get(id)?.parent?;
        let index = nodes.get(parent)?.children.iter().position(|&c| c == id)?;
        Some((parent, index))
    }

    fn detach(nodes: &mut [Node], id: Id) {
        if let Some((parent, index)) = Self::position(nodes, id) {
            if let Some(parent) = nodes.get_mut(parent) {
                parent.children.remove(index);
            }
        }
        if let Some(node) = nodes.get_mut(id) {
            node.parent = None;
        }
    }

    /// Appends `text` to the text node `id`; `false` if it isn't one.
    fn extend_text(nodes: &mut [Node], id: Option<Id>, text: &StrTendril) -> bool {
        match id.and_then(|id| nodes.get_mut(id)) {
            Some(Node {
                data: Data::Text(existing),
                ..
            }) => {
                existing.push_tendril(text);
                true
            }
            _ => false,
        }
    }

    fn new_text(nodes: &mut Vec<Node>, text: StrTendril) -> Id {
        nodes.push(Node::new(Data::Text(text)));
        nodes.len() - 1
    }
}

impl TreeSink for Sink {
    type Handle = Handle;
    type Output = Sink;
    type ElemName<'a> = &'a QualName;

    fn finish(self) -> Sink {
        self
    }

    fn parse_error(&self, _: Cow<'static, str>) {}

    fn get_document(&self) -> Handle {
        self.handle(0)
    }

    fn elem_name<'a>(&'a self, target: &'a Handle) -> &'a QualName {
        &target.name
    }

    fn create_element(&self, name: QualName, attrs: Vec<Attribute>, flags: ElementFlags) -> Handle {
        let template = flags.template.then(|| self.push(Data::Document));
        let name = Rc::new(name);
        let id = self.push(Data::Element {
            name: name.clone(),
            attrs,
            template,
            integration_point: flags.mathml_annotation_xml_integration_point,
        });
        Handle { id, name }
    }

    fn create_comment(&self, _: StrTendril) -> Handle {
        self.handle(self.push(Data::Comment))
    }

    fn create_pi(&self, _: StrTendril, _: StrTendril) -> Handle {
        self.handle(self.push(Data::Other))
    }

    fn append(&self, parent: &Handle, child: NodeOrText<Handle>) {
        let mut nodes = self.nodes.borrow_mut();
        let child = match child {
            NodeOrText::AppendText(text) => {
                let last = nodes
                    .get(parent.id)
                    .and_then(|p| p.children.last().copied());
                if Self::extend_text(&mut nodes, last, &text) {
                    return;
                }
                Self::new_text(&mut nodes, text)
            }
            NodeOrText::AppendNode(node) => {
                Self::detach(&mut nodes, node.id);
                node.id
            }
        };
        if let Some(parent_node) = nodes.get_mut(parent.id) {
            parent_node.children.push(child);
            if let Some(child) = nodes.get_mut(child) {
                child.parent = Some(parent.id);
            }
        }
    }

    fn append_before_sibling(&self, sibling: &Handle, new_node: NodeOrText<Handle>) {
        let mut nodes = self.nodes.borrow_mut();
        let Some((parent, index)) = Self::position(&nodes, sibling.id) else {
            return;
        };
        let child = match new_node {
            NodeOrText::AppendText(text) => {
                let previous = index
                    .checked_sub(1)
                    .and_then(|i| nodes.get(parent)?.children.get(i).copied());
                if Self::extend_text(&mut nodes, previous, &text) {
                    return;
                }
                Self::new_text(&mut nodes, text)
            }
            NodeOrText::AppendNode(node) => node.id,
        };
        // The position is the sibling's before the node leaves its old
        // parent, as in html5ever's reference tree.
        Self::detach(&mut nodes, child);
        if let Some(parent_node) = nodes.get_mut(parent) {
            let index = index.min(parent_node.children.len());
            parent_node.children.insert(index, child);
        }
        if let Some(child) = nodes.get_mut(child) {
            child.parent = Some(parent);
        }
    }

    fn append_based_on_parent_node(
        &self,
        element: &Handle,
        prev_element: &Handle,
        child: NodeOrText<Handle>,
    ) {
        let has_parent = self
            .nodes
            .borrow()
            .get(element.id)
            .is_some_and(|node| node.parent.is_some());
        if has_parent {
            self.append_before_sibling(element, child);
        } else {
            self.append(prev_element, child);
        }
    }

    fn append_doctype_to_document(&self, _: StrTendril, _: StrTendril, _: StrTendril) {
        let doctype = self.handle(self.push(Data::Other));
        self.append(&self.handle(0), NodeOrText::AppendNode(doctype));
    }

    fn get_template_contents(&self, target: &Handle) -> Handle {
        let contents = match self.nodes.borrow().get(target.id).map(|node| &node.data) {
            Some(Data::Element {
                template: Some(contents),
                ..
            }) => Some(*contents),
            _ => None,
        };
        // The tree builder only asks for a template's contents; anything
        // else gets a detached fragment.
        self.handle(contents.unwrap_or_else(|| self.push(Data::Document)))
    }

    fn same_node(&self, x: &Handle, y: &Handle) -> bool {
        x.id == y.id
    }

    fn set_quirks_mode(&self, _: QuirksMode) {}

    fn add_attrs_if_missing(&self, target: &Handle, new: Vec<Attribute>) {
        if let Some(Node {
            data: Data::Element { attrs, .. },
            ..
        }) = self.nodes.borrow_mut().get_mut(target.id)
        {
            let missing: Vec<Attribute> = new
                .into_iter()
                .filter(|attr| !attrs.iter().any(|existing| existing.name == attr.name))
                .collect();
            attrs.extend(missing);
        }
    }

    fn remove_from_parent(&self, target: &Handle) {
        Self::detach(&mut self.nodes.borrow_mut(), target.id);
    }

    fn reparent_children(&self, node: &Handle, new_parent: &Handle) {
        let mut nodes = self.nodes.borrow_mut();
        let Some(children) = nodes
            .get_mut(node.id)
            .map(|n| std::mem::take(&mut n.children))
        else {
            return;
        };
        for &child in &children {
            if let Some(child) = nodes.get_mut(child) {
                child.parent = Some(new_parent.id);
            }
        }
        if let Some(new_parent) = nodes.get_mut(new_parent.id) {
            new_parent.children.extend(children);
        }
    }

    fn is_mathml_annotation_xml_integration_point(&self, handle: &Handle) -> bool {
        matches!(
            self.nodes.borrow().get(handle.id).map(|node| &node.data),
            Some(Data::Element {
                integration_point: true,
                ..
            })
        )
    }
}
