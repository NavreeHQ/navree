//! The tree [`HtmlPolicy::clean`](super::HtmlPolicy::clean) works on.
//!
//! html5ever parses the input as a fragment in a `<div>` into a node arena
//! (indices, no reference cycles), and its serializer writes the tree back.
//! Every walk over the tree uses its own stack, so a deeply nested input
//! can't overflow the call stack.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::io;
use std::rc::Rc;

use html5ever::serialize::{HtmlSerializer, Serialize, SerializeOpts, Serializer, TraversalScope};
use html5ever::tendril::{StrTendril, TendrilSink};
use html5ever::tree_builder::{ElementFlags, NodeOrText, QuirksMode, TreeSink};
use html5ever::{Attribute, ParseOpts, QualName, local_name, ns, parse_fragment};

use super::HtmlError;

/// How much of the input goes to the parser between looks at the budget.
const PIECE: usize = 4096;

/// How much text goes to the serializer at a time (see [`write_text`]).
const TEXT_PIECE: usize = 1024;

/// How many bytes the serializer scans for the price of one step of the parser
/// (it scans them with `memchr`).
const BYTES_PER_STEP: u64 = 64;

/// An attribute's bytes that cost as much to hold and write out as a node: its
/// value is shared between the copies the parser makes of a formatting
/// element, but each copy is written out whole.
const BYTES_PER_NODE: usize = 32;

/// The stack and list scans the parser may do per node of the budget: more
/// than any document within the depth limit needs.
const STEPS_PER_NODE: u64 = 64;

/// The most attributes a tag may have. The tokenizer compares each attribute
/// with the tag's earlier ones, so this is what bounds that work; nothing
/// written by hand or by an editor comes near it.
const MAX_TAG_ATTRIBUTES: usize = 256;

/// What a parse may take: [`HtmlPolicy`](super::HtmlPolicy)'s limits.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Limits {
    pub(crate) max_input_bytes: usize,
    pub(crate) max_nodes: usize,
    pub(crate) max_depth: usize,
}

/// `html`, checked before the parser sees any of it.
fn check_input(html: &str, limits: Limits) -> Result<(), HtmlError> {
    if html.len() > limits.max_input_bytes {
        return Err(HtmlError::InputTooLarge {
            max: limits.max_input_bytes,
        });
    }
    if most_attributes(html.as_bytes()) > MAX_TAG_ATTRIBUTES {
        return Err(HtmlError::TooManyAttributes {
            max: MAX_TAG_ATTRIBUTES,
        });
    }
    Ok(())
}

/// Where the HTML tokenizer can be in a tag.
#[derive(Clone, Copy)]
enum InTag {
    Name,
    BeforeAttribute,
    Attribute,
    AfterAttribute,
    BeforeValue,
    DoubleQuoted,
    SingleQuoted,
    Unquoted,
    AfterQuoted,
}

const IN_TAG_STATES: usize = 9;

impl InTag {
    /// The state after `byte`, and whether it starts an attribute; `None`
    /// when it ends the tag.
    fn next(self, byte: u8) -> Option<(InTag, bool)> {
        use InTag::*;
        let blank = matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | b'\x0c');
        Some(match (self, byte) {
            (DoubleQuoted, b'"') | (SingleQuoted, b'\'') => (AfterQuoted, false),
            (DoubleQuoted | SingleQuoted, _) => (self, false),
            (_, b'>') => return None,
            (Name, _) if blank || byte == b'/' => (BeforeAttribute, false),
            (Name, _) => (Name, false),
            (BeforeAttribute | AfterQuoted, _) if blank || byte == b'/' => (BeforeAttribute, false),
            (BeforeAttribute | AfterQuoted, _) => (Attribute, true),
            (Attribute, b'=') => (BeforeValue, false),
            (Attribute, b'/') => (BeforeAttribute, false),
            (Attribute, _) if blank => (AfterAttribute, false),
            (Attribute, _) => (Attribute, false),
            (AfterAttribute, b'=') => (BeforeValue, false),
            (AfterAttribute, b'/') => (BeforeAttribute, false),
            (AfterAttribute, _) if blank => (AfterAttribute, false),
            (AfterAttribute, _) => (Attribute, true),
            (BeforeValue, b'"') => (DoubleQuoted, false),
            (BeforeValue, b'\'') => (SingleQuoted, false),
            (BeforeValue, _) if blank => (BeforeValue, false),
            (BeforeValue, _) => (Unquoted, false),
            (Unquoted, _) if blank => (BeforeAttribute, false),
            (Unquoted, _) => (Unquoted, false),
        })
    }
}

/// An upper bound on the attributes of any one tag in `html`, in one pass.
///
/// What counts as a tag depends on the parser's state (`<a` in a comment is
/// not one), so every `<` that could open a tag is followed at once: each
/// state holds the most attributes of any tag that could be in it here. The
/// tag the tokenizer really reads is among them, whatever came before it.
fn most_attributes(html: &[u8]) -> usize {
    // Too few bytes that can end an attribute: nothing to look for.
    let splits = html
        .iter()
        .filter(|byte| {
            matches!(
                byte,
                b' ' | b'\t' | b'\n' | b'\r' | b'\x0c' | b'/' | b'"' | b'\''
            )
        })
        .count();
    if splits <= MAX_TAG_ATTRIBUTES {
        return 0;
    }
    use InTag::*;
    const ALL: [InTag; IN_TAG_STATES] = [
        Name,
        BeforeAttribute,
        Attribute,
        AfterAttribute,
        BeforeValue,
        DoubleQuoted,
        SingleQuoted,
        Unquoted,
        AfterQuoted,
    ];
    let mut most = 0;
    let mut counts: [Option<usize>; IN_TAG_STATES] = [None; IN_TAG_STATES];
    let mut bytes = html.iter().copied().peekable();
    while let Some(byte) = bytes.next() {
        let mut after = [None; IN_TAG_STATES];
        for state in ALL {
            let Some(count) = counts.get(state as usize).copied().flatten() else {
                continue;
            };
            let Some((next, starts)) = state.next(byte) else {
                continue;
            };
            let count = count + usize::from(starts);
            most = most.max(count);
            if let Some(slot) = after.get_mut(next as usize) {
                *slot = Some(slot.unwrap_or(0).max(count));
            }
        }
        // `<x` and `</x`: `</` is followed for its letter by the next round.
        if byte == b'<' {
            let at_letter = bytes.peek().is_some_and(u8::is_ascii_alphabetic);
            let at_slash = bytes.peek() == Some(&b'/');
            if (at_letter || at_slash)
                && let Some(slot) = after.get_mut(Name as usize)
            {
                *slot = Some(slot.unwrap_or(0));
            }
        }
        counts = after;
    }
    most
}

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
    /// How far below the document it was when last attached.
    depth: usize,
}

impl Node {
    fn new(data: Data) -> Self {
        Node {
            parent: None,
            children: Vec::new(),
            data,
            depth: 0,
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

// How many parses this thread has begun: the tests count what an input costs.
#[cfg(test)]
thread_local! {
    pub(crate) static PARSES: Cell<usize> = const { Cell::new(0) };
}

impl Tree {
    /// `html` parsed as the content of a `<div>`, the way a browser parses
    /// it when it is inserted into one, within `limits`. The input goes in
    /// pieces, so a parse over a limit stops soon after it passes it.
    pub(crate) fn parse(html: &str, limits: Limits) -> Result<Tree, HtmlError> {
        #[cfg(test)]
        PARSES.with(|parses| parses.set(parses.get() + 1));
        check_input(html, limits)?;
        let mut parser = parse_fragment(
            Sink::new(limits),
            ParseOpts::default(),
            QualName::new(None, ns!(html), local_name!("div")),
            Vec::new(),
            false,
        );
        let mut rest = html;
        while !rest.is_empty() {
            let (piece, after) = rest.split_at(rest.floor_char_boundary(PIECE).max(1));
            parser.process(StrTendril::from_slice(piece));
            if let Some(error) = parser.tokenizer.sink.sink.exceeded.get() {
                return Err(error);
            }
            rest = after;
        }
        let sink = parser.finish();
        if let Some(error) = sink.exceeded.get() {
            return Err(error);
        }
        let nodes = sink.nodes.into_inner();
        let root = nodes
            .first()
            .and_then(|document| document.children.first().copied());
        let tree = Tree { nodes, root };
        // What moved after it was attached has the depth of where it was.
        if tree.depth() > limits.max_depth {
            return Err(HtmlError::TooDeep {
                max: limits.max_depth,
            });
        }
        Ok(tree)
    }

    /// How deep the elements under the root nest.
    fn depth(&self) -> usize {
        let mut deepest = 0;
        let mut stack: Vec<(Id, usize)> = self.root.map(|root| (root, 0)).into_iter().collect();
        while let Some((id, depth)) = stack.pop() {
            let Some(node) = self.nodes.get(id) else {
                continue;
            };
            if matches!(node.data, Data::Element { .. }) {
                deepest = deepest.max(depth);
            }
            stack.extend(node.children.iter().map(|&child| (child, depth + 1)));
        }
        deepest
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
    let _ = write_text(&mut serializer, text);
    String::from_utf8_lossy(&out).into_owned()
}

/// Writes `text` a little at a time. For each `&` the serializer looks for the
/// next `<` or `>` to the end of what it was given: given a long text of `&`,
/// that would be quadratic.
fn write_text<S: Serializer>(serializer: &mut S, mut text: &str) -> io::Result<()> {
    while !text.is_empty() {
        let (piece, rest) = text.split_at(text.floor_char_boundary(TEXT_PIECE).max(1));
        serializer.write_text(piece)?;
        text = rest;
    }
    Ok(())
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
                Data::Text(text) => write_text(serializer, text)?,
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

/// Builds a [`Tree`] as html5ever parses, within a budget. The tree builder
/// only ever calls one method at a time, so each method borrows the arena for
/// its own duration.
///
/// The budget counts what the input can multiply: the parser makes a new copy
/// of every open formatting element for each block it meets, so nodes, the
/// attributes they carry and the scans over the parser's stacks grow faster
/// than the input does. Once the budget is gone the sink holds nothing more,
/// and [`Tree::parse`] stops at the next piece of the input.
struct Sink {
    nodes: RefCell<Vec<Node>>,
    /// The name of handles that aren't elements (never read).
    unnamed: Rc<QualName>,
    limits: Limits,
    /// What is left of the budget for nodes.
    nodes_left: Cell<usize>,
    /// What is left of the budget for scans.
    steps_left: Cell<u64>,
    /// The limit that was passed.
    exceeded: Cell<Option<HtmlError>>,
}

/// The node every handle points to once the budget is gone. Never attached.
const SPENT: Id = 1;

/// How deep the root's children are in the arena: the document, then the root.
const TOP_DEPTH: usize = 2;

impl Sink {
    fn new(limits: Limits) -> Self {
        let nodes_left = limits.max_nodes;
        let steps = u64::try_from(nodes_left)
            .unwrap_or(u64::MAX)
            .saturating_mul(STEPS_PER_NODE);
        Sink {
            // The document, and the node of a spent budget.
            nodes: RefCell::new(vec![Node::new(Data::Document), Node::new(Data::Document)]),
            unnamed: Rc::new(QualName::new(None, ns!(), local_name!(""))),
            limits,
            nodes_left: Cell::new(nodes_left),
            steps_left: Cell::new(steps),
            exceeded: Cell::new(None),
        }
    }

    fn spent(&self) -> bool {
        self.exceeded.get().is_some()
    }

    fn too_many_nodes(&self) {
        self.exceeded.set(Some(HtmlError::TooManyNodes {
            max: self.limits.max_nodes,
        }));
    }

    /// Takes `cost` from the budget; `false` when it isn't there.
    fn charge(&self, cost: usize) -> bool {
        if self.spent() {
            return false;
        }
        match self.nodes_left.get().checked_sub(cost) {
            Some(left) => {
                self.nodes_left.set(left);
                true
            }
            None => {
                self.too_many_nodes();
                false
            }
        }
    }

    /// Takes what writing `attrs` out will cost. An attribute goes to the
    /// serializer whole, and for each `&` and each byte `0xC2` in it the
    /// serializer scans for the next `<`, `>` or `"`; a value is rarely long
    /// and full of them at once.
    fn charge_writing(&self, attrs: &[Attribute]) -> bool {
        let scanned = attrs
            .iter()
            .map(|attr| {
                let value = attr.value.as_bytes();
                let stops = memchr::memchr2_iter(b'&', 0xC2, value).count();
                u64::try_from(stops.saturating_mul(value.len())).unwrap_or(u64::MAX)
            })
            .fold(0, u64::saturating_add);
        match self.steps_left.get().checked_sub(scanned / BYTES_PER_STEP) {
            Some(left) => {
                self.steps_left.set(left);
                true
            }
            None => {
                self.too_many_nodes();
                false
            }
        }
    }

    /// Notes that `child` is now under `parent`: the depth it has there. Only
    /// an element is a level of nesting: the text or comment inside the
    /// deepest element is not another.
    fn place(&self, nodes: &mut [Node], parent: Id, child: Id) {
        let depth = nodes.get(parent).map_or(0, |parent| parent.depth) + 1;
        let mut element = false;
        if let Some(child) = nodes.get_mut(child) {
            child.depth = depth;
            child.parent = Some(parent);
            element = matches!(child.data, Data::Element { .. });
        }
        if element && depth > self.limits.max_depth.saturating_add(TOP_DEPTH - 1) {
            self.exceeded.set(Some(HtmlError::TooDeep {
                max: self.limits.max_depth,
            }));
        }
    }

    fn push(&self, data: Data) -> Id {
        if !self.charge(1) {
            return SPENT;
        }
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

    /// The node's parent and its position among the parent's children. The
    /// tree builder works at the end of a parent's children, so the search
    /// starts there.
    fn position(nodes: &[Node], id: Id) -> Option<(Id, usize)> {
        let parent = nodes.get(id)?.parent?;
        let index = nodes.get(parent)?.children.iter().rposition(|&c| c == id)?;
        Some((parent, index))
    }

    fn detach(nodes: &mut [Node], id: Id) {
        if let Some((parent, index)) = Self::position(nodes, id)
            && let Some(parent) = nodes.get_mut(parent)
        {
            parent.children.remove(index);
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

    /// Every scan of the parser's stack of open elements asks for the names.
    fn elem_name<'a>(&'a self, target: &'a Handle) -> &'a QualName {
        match self.steps_left.get().checked_sub(1) {
            Some(left) => self.steps_left.set(left),
            None => self.too_many_nodes(),
        }
        &target.name
    }

    fn create_element(&self, name: QualName, attrs: Vec<Attribute>, flags: ElementFlags) -> Handle {
        let name = Rc::new(name);
        let bytes: usize = attrs
            .iter()
            .map(|attr| attr.name.local.len() + attr.value.len())
            .sum();
        let cost = 1 + usize::from(flags.template) + attrs.len() + bytes / BYTES_PER_NODE;
        if !self.charge(cost) || !self.charge_writing(&attrs) {
            return Handle { id: SPENT, name };
        }
        let mut nodes = self.nodes.borrow_mut();
        let template = flags.template.then(|| {
            nodes.push(Node::new(Data::Document));
            nodes.len() - 1
        });
        nodes.push(Node::new(Data::Element {
            name: name.clone(),
            attrs,
            template,
            integration_point: flags.mathml_annotation_xml_integration_point,
        }));
        Handle {
            id: nodes.len() - 1,
            name,
        }
    }

    fn create_comment(&self, _: StrTendril) -> Handle {
        self.handle(self.push(Data::Comment))
    }

    fn create_pi(&self, _: StrTendril, _: StrTendril) -> Handle {
        self.handle(self.push(Data::Other))
    }

    fn append(&self, parent: &Handle, child: NodeOrText<Handle>) {
        if self.spent() {
            return;
        }
        let mut nodes = self.nodes.borrow_mut();
        let child = match child {
            NodeOrText::AppendText(text) => {
                let last = nodes
                    .get(parent.id)
                    .and_then(|p| p.children.last().copied());
                if Self::extend_text(&mut nodes, last, &text) || !self.charge(1) {
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
            self.place(&mut nodes, parent.id, child);
        }
    }

    fn append_before_sibling(&self, sibling: &Handle, new_node: NodeOrText<Handle>) {
        if self.spent() {
            return;
        }
        let mut nodes = self.nodes.borrow_mut();
        let Some((parent, index)) = Self::position(&nodes, sibling.id) else {
            return;
        };
        let child = match new_node {
            NodeOrText::AppendText(text) => {
                let previous = index
                    .checked_sub(1)
                    .and_then(|i| nodes.get(parent)?.children.get(i).copied());
                if Self::extend_text(&mut nodes, previous, &text) || !self.charge(1) {
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
        self.place(&mut nodes, parent, child);
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
        let contents = contents.unwrap_or_else(|| self.push(Data::Document));
        // The contents are a separate fragment, but they nest all the same.
        let mut nodes = self.nodes.borrow_mut();
        let depth = nodes.get(target.id).map_or(0, |target| target.depth);
        if let Some(contents) = nodes.get_mut(contents) {
            contents.depth = depth + 1;
        }
        self.handle(contents)
    }

    fn same_node(&self, x: &Handle, y: &Handle) -> bool {
        x.id == y.id
    }

    fn set_quirks_mode(&self, _: QuirksMode) {}

    /// The parser adds a later `<html>`'s attributes to the root (and, in a
    /// whole document, a later `<body>`'s to the body). A fragment's root is
    /// never written out, so they go.
    fn add_attrs_if_missing(&self, _: &Handle, _: Vec<Attribute>) {}

    fn remove_from_parent(&self, target: &Handle) {
        Self::detach(&mut self.nodes.borrow_mut(), target.id);
    }

    fn reparent_children(&self, node: &Handle, new_parent: &Handle) {
        if self.spent() {
            return;
        }
        let mut nodes = self.nodes.borrow_mut();
        let Some(children) = nodes
            .get_mut(node.id)
            .map(|n| std::mem::take(&mut n.children))
        else {
            return;
        };
        for &child in &children {
            self.place(&mut nodes, new_parent.id, child);
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
