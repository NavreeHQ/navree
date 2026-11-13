//! Walking and changing a tree of parts, and the rules for ids.
//!
//! Ids are not unique in real pages: a dialog and the form inside it may share
//! one. So a target is always looked up among the kinds the asker can mean, and
//! the first in document order wins, as a browser's lookup by id does. A target
//! that is not there is skipped.
//!
//! A client has to say which form a field belongs to and which form a button
//! sends, so every tree is given names on arrival ([`Names::name`]): each form
//! and each field ends up with an id that no other form or field of its screen
//! has. A form that came with none is named after its address.

use std::collections::{BTreeMap, BTreeSet};

use navree_native::Screen;
use navree_native::input::Input;
use navree_native::node::{Field, Form, Live, Method, Node};

use crate::event::FieldValue;

/// The deepest tree the store holds, in nodes: the deepest the protocol lets
/// one answer be. A page is nowhere near it; an op that would nest a screen
/// past it is skipped, so nothing that walks a tree can run out of stack.
pub(crate) const MAX_DEPTH: usize = navree_native::MAX_DEPTH;

/// A place in a node that holds nodes.
pub(crate) enum Slot<'a> {
    /// Any number of nodes.
    Many(&'a mut Vec<Node>),
    /// One node or none.
    Maybe(&'a mut Option<Box<Node>>),
    /// Exactly one node.
    One(&'a mut Node),
}

/// The places of `node` that hold nodes, in the order [`Node::children`] reads them.
pub(crate) fn slots(node: &mut Node) -> Vec<Slot<'_>> {
    match node {
        Node::Stack(n) => vec![Slot::Many(&mut n.children)],
        Node::Grid(n) => vec![Slot::Many(&mut n.children)],
        Node::Section(n) => vec![
            Slot::Many(&mut n.actions),
            Slot::Many(&mut n.children),
            Slot::Many(&mut n.footer),
        ],
        Node::List(n) => vec![Slot::Many(&mut n.children)],
        Node::Row(n) => vec![
            Slot::Maybe(&mut n.leading),
            Slot::Many(&mut n.detail),
            Slot::Maybe(&mut n.trailing),
            Slot::Many(&mut n.menu),
        ],
        Node::Split(n) => vec![Slot::One(&mut n.primary), Slot::One(&mut n.detail)],
        Node::Tabs(n) => n
            .items
            .iter_mut()
            .map(|tab| Slot::Many(&mut tab.children))
            .collect(),
        Node::Disclosure(n) => vec![Slot::Many(&mut n.children)],
        Node::Region(n) => vec![Slot::Many(&mut n.children)],
        Node::Banner(n) => vec![Slot::Many(&mut n.actions)],
        Node::Empty(n) => vec![Slot::Many(&mut n.actions)],
        Node::Facts(n) => n
            .items
            .iter_mut()
            .map(|fact| Slot::Many(&mut fact.value))
            .collect(),
        Node::Table(n) => {
            let mut out = Vec::with_capacity(n.rows.len() * 3 + 2);
            if let Some(query) = &mut n.query {
                out.push(Slot::Many(&mut query.children));
            }
            for row in &mut n.rows {
                out.push(Slot::Many(&mut row.cells));
                out.push(Slot::Many(&mut row.actions));
                out.push(Slot::Many(&mut row.detail));
            }
            out.push(Slot::Maybe(&mut n.empty));
            out
        }
        Node::Link(n) => vec![Slot::Many(&mut n.children)],
        Node::Menu(n) => vec![Slot::Many(&mut n.items)],
        Node::Form(n) => vec![Slot::Many(&mut n.children)],
        Node::Sheet(n) => vec![Slot::Many(&mut n.children), Slot::Many(&mut n.actions)],
        Node::Dialog(n) => vec![Slot::Many(&mut n.children), Slot::Many(&mut n.actions)],
        Node::Popover(n) => vec![Slot::Many(&mut n.children)],
        Node::Native(n) => vec![Slot::Maybe(&mut n.alt)],
        Node::Toolbar(n) => n.items.iter_mut().map(|item| Slot::Many(&mut item.menu)).collect(),
        Node::Timeline(n) => n.items.iter_mut().map(|item| Slot::Many(&mut item.detail)).collect(),
        Node::FeedRow(n) => vec![Slot::Many(&mut n.detail)],
        Node::Message(n) => vec![Slot::Many(&mut n.body)],
        Node::Pane(n) => vec![Slot::Many(&mut n.children)],
        Node::Unknown(n) => vec![Slot::Maybe(&mut n.alt)],
        _ => Vec::new(),
    }
}

/// Calls `visit` with `node` and every node inside it, in document order.
pub(crate) fn walk<'a>(node: &'a Node, visit: &mut dyn FnMut(&'a Node)) {
    visit(node);
    for child in node.children() {
        walk(child, visit);
    }
}

/// How many nodes deep `node` goes: `1` for a node that holds none.
pub(crate) fn depth(node: &Node) -> usize {
    1 + node.children().into_iter().map(depth).max().unwrap_or(0)
}

/// The first node, in document order, that `is` accepts.
pub(crate) fn find<'a>(node: &'a Node, is: &dyn Fn(&Node) -> bool) -> Option<&'a Node> {
    if is(node) {
        return Some(node);
    }
    node.children().into_iter().find_map(|child| find(child, is))
}

/// The first node, in document order, that `is` accepts, to change in place.
pub(crate) fn find_mut<'a>(
    node: &'a mut Node,
    is: &dyn Fn(&Node) -> bool,
) -> Option<&'a mut Node> {
    if is(node) {
        return Some(node);
    }
    for slot in slots(node) {
        match slot {
            Slot::Many(list) => {
                for child in list {
                    if let Some(found) = find_mut(child, is) {
                        return Some(found);
                    }
                }
            }
            Slot::Maybe(child) => {
                if let Some(found) = child.as_deref_mut().and_then(|child| find_mut(child, is)) {
                    return Some(found);
                }
            }
            Slot::One(child) => {
                if let Some(found) = find_mut(child, is) {
                    return Some(found);
                }
            }
        }
    }
    None
}

fn nothing() -> Node {
    Node::Stack(navree_native::node::Stack::default())
}

// A node is half a kilobyte, and a debug build keeps every one it moves in
// the frame of the function that moves it. So what moves a node is done in
// these three, which do not call themselves: the walk that finds the place
// holds only references, and stays small at every level of a deep tree.

#[inline(never)]
fn change_in_list(list: &mut Vec<Node>, at: usize, change: &mut dyn FnMut(Node) -> Option<Node>) {
    let old = list.remove(at);
    if let Some(new) = change(old) {
        list.insert(at, new);
    }
}

#[inline(never)]
fn change_in_place(place: &mut Option<Box<Node>>, change: &mut dyn FnMut(Node) -> Option<Node>) {
    let old = place.take().map(|old| *old);
    *place = old.and_then(change).map(Box::new);
}

#[inline(never)]
fn change_where_one_must_be(child: &mut Node, change: &mut dyn FnMut(Node) -> Option<Node>) {
    let old = std::mem::replace(child, nothing());
    if let Some(new) = change(old) {
        *child = new;
    }
}

/// Puts the first node inside `node` that `is` accepts through `change`: none
/// takes it out, one takes its place. A place that must hold a node keeps an
/// empty stack when it is given none. Returns whether one was found.
pub(crate) fn rewrite(
    node: &mut Node,
    is: &dyn Fn(&Node) -> bool,
    change: &mut dyn FnMut(Node) -> Option<Node>,
) -> bool {
    for slot in slots(node) {
        match slot {
            Slot::Many(list) => {
                let mut at = 0;
                while at < list.len() {
                    if is(&list[at]) {
                        change_in_list(list, at, change);
                        return true;
                    }
                    if rewrite(&mut list[at], is, change) {
                        return true;
                    }
                    at += 1;
                }
            }
            Slot::Maybe(place) => {
                if place.as_deref().is_some_and(is) {
                    change_in_place(place, change);
                    return true;
                }
                if let Some(child) = place.as_deref_mut()
                    && rewrite(child, is, change)
                {
                    return true;
                }
            }
            Slot::One(child) => {
                if is(child) {
                    change_where_one_must_be(child, change);
                    return true;
                }
                if rewrite(child, is, change) {
                    return true;
                }
            }
        }
    }
    false
}

/// The nodes a screen starts from: its actions, then its body.
pub(crate) fn roots(screen: &Screen) -> impl Iterator<Item = &Node> {
    screen.actions.iter().chain(std::iter::once(&screen.body))
}

/// The first node of the screen that `is` accepts.
pub(crate) fn find_in<'a>(screen: &'a Screen, is: &dyn Fn(&Node) -> bool) -> Option<&'a Node> {
    roots(screen).find_map(|root| find(root, is))
}

/// The first node of the screen that `is` accepts, to change in place.
pub(crate) fn find_in_mut<'a>(
    screen: &'a mut Screen,
    is: &dyn Fn(&Node) -> bool,
) -> Option<&'a mut Node> {
    for action in &mut screen.actions {
        if let Some(found) = find_mut(action, is) {
            return Some(found);
        }
    }
    find_mut(&mut screen.body, is)
}

/// [`rewrite`] over a whole screen. A body that is taken out leaves an empty stack.
pub(crate) fn rewrite_in(
    screen: &mut Screen,
    is: &dyn Fn(&Node) -> bool,
    change: &mut dyn FnMut(Node) -> Option<Node>,
) -> bool {
    let mut at = 0;
    while at < screen.actions.len() {
        if is(&screen.actions[at]) {
            change_in_list(&mut screen.actions, at, change);
            return true;
        }
        if rewrite(&mut screen.actions[at], is, change) {
            return true;
        }
        at += 1;
    }
    if is(&screen.body) {
        change_where_one_must_be(&mut screen.body, change);
        return true;
    }
    rewrite(&mut screen.body, is, change)
}

/// How deep in the screen the first node that `is` accepts stands: `1` for a root.
pub(crate) fn depth_of(screen: &Screen, is: &dyn Fn(&Node) -> bool) -> Option<usize> {
    fn seek(node: &Node, is: &dyn Fn(&Node) -> bool, at: usize) -> Option<usize> {
        if is(node) {
            return Some(at);
        }
        node.children().into_iter().find_map(|child| seek(child, is, at + 1))
    }
    roots(screen).find_map(|root| seek(root, is, 1))
}

/// Whether the node has this id.
pub(crate) fn has_id(node: &Node, id: &str) -> bool {
    node.id() == Some(id)
}

/// Whether `open` and `close` can mean the node: a sheet, a dialog, a popover
/// or a disclosure.
pub(crate) fn opens(node: &Node) -> bool {
    matches!(
        node,
        Node::Sheet(_) | Node::Dialog(_) | Node::Popover(_) | Node::Disclosure(_)
    )
}

/// The list of nodes an `append` or a `prepend` adds to, when the node has one.
pub(crate) fn main_list(node: &mut Node) -> Option<&mut Vec<Node>> {
    match node {
        Node::Stack(n) => Some(&mut n.children),
        Node::Grid(n) => Some(&mut n.children),
        Node::Section(n) => Some(&mut n.children),
        Node::List(n) => Some(&mut n.children),
        Node::Disclosure(n) => Some(&mut n.children),
        Node::Region(n) => Some(&mut n.children),
        Node::Form(n) => Some(&mut n.children),
        Node::Sheet(n) => Some(&mut n.children),
        Node::Dialog(n) => Some(&mut n.children),
        Node::Popover(n) => Some(&mut n.children),
        Node::Menu(n) => Some(&mut n.items),
        Node::Pane(n) => Some(&mut n.children),
        _ => None,
    }
}

/// Whether an `append` or a `prepend` can mean the node.
pub(crate) fn holds(node: &Node) -> bool {
    matches!(
        node,
        Node::Stack(_)
            | Node::Grid(_)
            | Node::Section(_)
            | Node::List(_)
            | Node::Disclosure(_)
            | Node::Region(_)
            | Node::Form(_)
            | Node::Sheet(_)
            | Node::Dialog(_)
            | Node::Popover(_)
            | Node::Menu(_)
            | Node::Pane(_)
    )
}

/// What a control holds, as the tree says it.
pub(crate) fn value_of(input: &Input) -> FieldValue {
    let text = |text: &str| FieldValue::Text {
        text: text.to_owned(),
    };
    match input {
        Input::Text(input) => text(&input.value),
        Input::Toggle(input) => FieldValue::Flag { on: input.on },
        Input::Choice(input) => input.value.as_deref().map_or(FieldValue::None, text),
        Input::Choices(input) => FieldValue::Many {
            values: input.values.clone(),
        },
        Input::Date(input) => text(&input.value),
        Input::Slider(input) => text(&input.value),
        Input::Colour(input) => text(&input.value),
        Input::File(_) => FieldValue::Files { files: Vec::new() },
        Input::Range(input) => FieldValue::Range {
            from: input.from.value.clone(),
            to: input.to.value.clone(),
        },
        _ => FieldValue::None,
    }
}

/// Whether a value is of the kind the control holds, so a value typed into a
/// field is not kept for a control of another kind that took its place.
pub(crate) fn fits(input: &Input, value: &FieldValue) -> bool {
    matches!(
        (input, value),
        (
            Input::Text(_) | Input::Date(_) | Input::Slider(_) | Input::Colour(_),
            FieldValue::Text { .. },
        ) | (Input::Text(_), FieldValue::Phone { .. })
            | (Input::Toggle(_), FieldValue::Flag { .. })
            | (Input::Choice(_), FieldValue::Text { .. } | FieldValue::None)
            | (Input::Choices(_), FieldValue::Many { .. })
            | (Input::File(_), FieldValue::Files { .. })
            | (Input::Range(_), FieldValue::Range { .. })
    )
}

/// Gives every form and field of arriving nodes an id of its own.
pub(crate) struct Names<'a> {
    /// The form ids the screen already has.
    pub forms: &'a mut BTreeSet<String>,
    /// The field ids the screen already has.
    pub fields: &'a mut BTreeSet<String>,
    /// The id the server gave each form, by the id the form has here.
    pub server: &'a mut BTreeMap<String, Option<String>>,
}

fn base_of(form: &Form) -> String {
    form.id
        .clone()
        .unwrap_or_else(|| format!("@{}", form.action))
}

fn count_bases(node: &Node, counts: &mut BTreeMap<String, usize>) {
    match node {
        Node::Form(form) => *counts.entry(base_of(form)).or_default() += 1,
        Node::Table(table) => {
            if let Some(query) = &table.query {
                *counts.entry(base_of(query)).or_default() += 1;
            }
        }
        _ => {}
    }
    for child in node.children() {
        count_bases(child, counts);
    }
}

fn free(taken: &BTreeSet<String>, stem: String) -> String {
    if !taken.contains(&stem) {
        return stem;
    }
    (2u32..)
        .map(|n| format!("{stem}~{n}"))
        .find(|name| !taken.contains(name))
        .unwrap_or(stem)
}

impl Names<'_> {
    /// Names the forms and fields of `nodes`, which are about to stand under a
    /// node whose id is `scope`.
    pub(crate) fn name(&mut self, nodes: &mut [Node], scope: Option<&str>) {
        let mut counts = BTreeMap::new();
        for node in nodes.iter() {
            count_bases(node, &mut counts);
        }
        for node in nodes {
            self.node(node, scope, None, &counts);
        }
    }

    fn form(&mut self, form: &mut Form, scope: Option<&str>, counts: &BTreeMap<String, usize>) -> String {
        let server = form.id.clone();
        let base = base_of(form);
        let repeated = counts.get(&base).copied().unwrap_or(0) > 1 || self.forms.contains(&base);
        let stem = match scope {
            Some(scope) if repeated => format!("{base}~{scope}"),
            _ => base,
        };
        let handle = free(self.forms, stem);
        self.forms.insert(handle.clone());
        self.server.insert(handle.clone(), server);
        form.id = Some(handle.clone());
        handle
    }

    fn field(&mut self, field: &mut Field, form: Option<&str>) {
        let own = field.id.clone().filter(|id| !self.fields.contains(id));
        let id = own.unwrap_or_else(|| {
            let mut stem = format!("{}.{}", form.unwrap_or(""), field.name);
            if let Input::Toggle(toggle) = &field.input
                && let Some(value) = &toggle.value
            {
                stem.push('=');
                stem.push_str(value);
            }
            free(self.fields, stem)
        });
        self.fields.insert(id.clone());
        field.id = Some(id);
    }

    fn node(
        &mut self,
        node: &mut Node,
        scope: Option<&str>,
        form: Option<&str>,
        counts: &BTreeMap<String, usize>,
    ) {
        match node {
            Node::Form(inner) => {
                // What stands in a form is scoped by the id the server gave it.
                let server = inner.id.clone();
                let handle = self.form(inner, scope, counts);
                let scope = server.as_deref().or(scope);
                for child in &mut inner.children {
                    self.node(child, scope, Some(&handle), counts);
                }
            }
            Node::Field(field) => self.field(field, form),
            Node::Table(table) => {
                let scope = table.id.clone().or_else(|| scope.map(str::to_owned));
                let scope = scope.as_deref();
                if let Some(query) = &mut table.query {
                    let handle = self.form(query, scope, counts);
                    for child in &mut query.children {
                        self.node(child, scope, Some(&handle), counts);
                    }
                }
                for row in &mut table.rows {
                    let own = row.id.clone();
                    let scope = own.as_deref().or(scope);
                    for cell in row.nodes_mut() {
                        self.node(cell, scope, form, counts);
                    }
                }
                if let Some(empty) = table.empty.as_deref_mut() {
                    self.node(empty, scope, form, counts);
                }
            }
            other => {
                let own = other.id().map(str::to_owned);
                let scope = own.as_deref().or(scope);
                for slot in slots(other) {
                    match slot {
                        Slot::Many(list) => {
                            for child in list {
                                self.node(child, scope, form, counts);
                            }
                        }
                        Slot::Maybe(child) => {
                            if let Some(child) = child.as_deref_mut() {
                                self.node(child, scope, form, counts);
                            }
                        }
                        Slot::One(child) => self.node(child, scope, form, counts),
                    }
                }
            }
        }
    }
}

/// A form found in a tree.
#[derive(Debug)]
pub(crate) struct FoundForm<'a> {
    /// Its id here.
    pub handle: String,
    /// How it is sent.
    pub method: Method,
    /// Where it is sent, as the tree says.
    pub action: &'a str,
    /// The id of the region or table whose query it is.
    pub query_of: Option<String>,
    /// Its fields, in document order.
    pub fields: Vec<&'a Field>,
}

/// A region found in a tree.
#[derive(Debug)]
pub(crate) struct FoundRegion<'a> {
    pub id: &'a str,
    pub src: &'a str,
    /// It came empty and asks to be fetched once the screen is drawn.
    pub wants: bool,
    pub live: Option<&'a Live>,
}

/// What a tree holds that the runtime keeps state for.
#[derive(Debug, Default)]
pub(crate) struct Survey<'a> {
    pub forms: Vec<FoundForm<'a>>,
    /// Fields that stand in no form.
    pub loose: Vec<&'a Field>,
    pub regions: Vec<FoundRegion<'a>>,
    /// The ids of the lists and tables that have more to load, and of the
    /// regions that hold such a table.
    pub more: Vec<&'a str>,
    /// The sheets, dialogs and popovers that are open, in document order.
    pub open: Vec<&'a str>,
    /// The lists that say where to ask what changed in them, each with the
    /// stream that says when.
    pub live: Vec<(&'a str, &'a str, Option<&'a navree_native::node::Live>)>,
    /// How many tabs that do not show the walk is inside of: a lazy region
    /// there waits for its tab.
    unseen: usize,
}

impl<'a> Survey<'a> {
    /// Reads `nodes`.
    pub(crate) fn of(nodes: impl Iterator<Item = &'a Node>) -> Self {
        let mut survey = Self::default();
        for node in nodes {
            survey.node(node, None, None);
        }
        survey
    }

    fn form(&mut self, form: &'a Form, query_of: Option<String>) -> usize {
        self.forms.push(FoundForm {
            handle: form.id.clone().unwrap_or_default(),
            method: form.method,
            action: &form.action,
            query_of,
            fields: Vec::new(),
        });
        self.forms.len() - 1
    }

    fn node(&mut self, node: &'a Node, form: Option<usize>, region: Option<&'a str>) {
        match node {
            Node::Form(inner) => {
                let at = self.form(inner, None);
                for child in &inner.children {
                    self.node(child, Some(at), region);
                }
                return;
            }
            Node::Field(field) => match form {
                Some(at) => self.forms[at].fields.push(field),
                None => self.loose.push(field),
            },
            Node::Region(inner) => {
                self.regions.push(FoundRegion {
                    id: &inner.id,
                    src: &inner.src,
                    wants: inner.lazy && inner.children.is_empty() && self.unseen == 0,
                    live: inner.live.as_ref(),
                });
                for child in &inner.children {
                    self.node(child, form, Some(&inner.id));
                }
                return;
            }
            Node::Table(table) => {
                let owner = region.or(table.id.as_deref());
                if table.more.is_some() {
                    self.more.extend(owner);
                }
                if let Some(query) = &table.query {
                    let at = self.form(query, owner.map(str::to_owned));
                    for child in &query.children {
                        self.node(child, Some(at), region);
                    }
                }
                for cell in table.rows.iter().flat_map(navree_native::node::TableRow::nodes) {
                    self.node(cell, form, region);
                }
                if let Some(empty) = table.empty.as_deref() {
                    self.node(empty, form, region);
                }
                return;
            }
            Node::List(list) => {
                if list.more.is_some() || list.before.is_some() {
                    self.more.extend(list.id.as_deref());
                }
                if let (Some(id), Some(changes)) = (list.id.as_deref(), list.changes.as_deref()) {
                    self.live.push((id, changes, list.live.as_ref()));
                }
            }
            // Of a set of tabs the client can switch through the core (one
            // with an id), only the selected one shows.
            Node::Tabs(tabs) if tabs.id.is_some() => {
                for tab in &tabs.items {
                    let waits = usize::from(!tab.selected);
                    self.unseen += waits;
                    for child in &tab.children {
                        self.node(child, form, region);
                    }
                    self.unseen -= waits;
                }
                return;
            }
            Node::Sheet(sheet) if sheet.open => self.open.push(&sheet.id),
            Node::Dialog(dialog) if dialog.open => self.open.push(&dialog.id),
            _ => {}
        }
        for child in node.children() {
            self.node(child, form, region);
        }
    }
}

/// The form of the screen whose id here is `handle`: a form node, or a table's query.
pub(crate) fn form_of<'a>(screen: &'a Screen, handle: &str) -> Option<&'a Form> {
    fn seek<'a>(node: &'a Node, handle: &str) -> Option<&'a Form> {
        match node {
            Node::Form(form) if form.id.as_deref() == Some(handle) => return Some(form),
            Node::Table(table) => {
                if let Some(query) = table.query.as_ref().filter(|query| query.id.as_deref() == Some(handle)) {
                    return Some(query);
                }
            }
            _ => {}
        }
        node.children().into_iter().find_map(|child| seek(child, handle))
    }
    roots(screen).find_map(|root| seek(root, handle))
}

/// The fields of a form, in document order.
pub(crate) fn fields_of(form: &Form) -> Vec<&Field> {
    let mut fields = Vec::new();
    for child in &form.children {
        walk(child, &mut |node| {
            if let Node::Field(field) = node {
                fields.push(field);
            }
        });
    }
    fields
}

#[cfg(test)]
mod tests {
    use super::*;
    use navree_native::Envelope;
    use navree_native::node::{Dialog, Region, Sheet, Stack, Table, TableRow, Text};

    fn corpus(name: &str) -> Envelope {
        let path = format!("{}/../navree-native/corpus/{name}.json", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn label(node: &Node) -> String {
        format!("{}#{}", node.kind().as_str(), node.id().unwrap_or(""))
    }

    fn walk_mut(node: &mut Node, out: &mut Vec<String>) {
        out.push(label(node));
        for slot in slots(node) {
            match slot {
                Slot::Many(list) => list.iter_mut().for_each(|child| walk_mut(child, out)),
                Slot::Maybe(child) => {
                    if let Some(child) = child.as_deref_mut() {
                        walk_mut(child, out);
                    }
                }
                Slot::One(child) => walk_mut(child, out),
            }
        }
    }

    #[test]
    fn mutable_and_plain_walks_agree_on_the_corpus() {
        let mut seen = BTreeSet::new();
        let names = ["screen-list", "screen-detail", "screen-form", "screen-kinds", "screen-with-toast", "screen-dashboard", "screen-inbox", "screen-device", "from-the-future"];
        for name in names {
            let screen = corpus(name).screen.unwrap();
            // What a screen may do with itself is made of nodes too.
            for mut root in screen.actions.into_iter().chain(std::iter::once(screen.body)) {
                let mut plain = Vec::new();
                walk(&root, &mut |node| plain.push(label(node)));
                let mut changing = Vec::new();
                walk_mut(&mut root, &mut changing);
                assert_eq!(plain, changing, "{name}");
                // The protocol's own walk agrees too.
                let mut protocol = Vec::new();
                let mut stack = vec![&mut root];
                while let Some(node) = stack.pop() {
                    protocol.push(label(node));
                    stack.extend(node.children_mut().into_iter().rev());
                }
                assert_eq!(plain, protocol, "{name}");
                walk(&root, &mut |node| {
                    seen.insert(node.kind().as_str());
                });
            }
        }
        // A kind this file has never walked would be a kind whose children it may miss.
        for kind in navree_native::Kind::ALL {
            assert!(seen.contains(kind.as_str()), "no example of `{}` was walked", kind.as_str());
        }
    }

    fn text(id: &str, value: &str) -> Node {
        Node::Text(Text {
            id: Some(id.to_owned()),
            value: value.to_owned(),
            ..Text::default()
        })
    }

    fn stack(id: Option<&str>, children: Vec<Node>) -> Node {
        Node::Stack(Stack {
            id: id.map(str::to_owned),
            children,
            ..Stack::default()
        })
    }

    #[test]
    fn the_first_node_in_document_order_wins() {
        // The outer `refund` is a dialog, the inner one a form; `b` is there twice.
        let form = Node::Form(Form {
            id: Some("refund".to_owned()),
            children: vec![text("b", "inner")],
            ..Form::default()
        });
        let dialog = Node::Dialog(Dialog {
            id: "refund".to_owned(),
            children: vec![form],
            ..Dialog::default()
        });
        let mut screen = Screen::new("Order", stack(Some("page"), vec![dialog, text("b", "outer")]));
        let found = find_in(&screen, &|node| has_id(node, "refund")).unwrap();
        assert!(matches!(found, Node::Dialog(_)));
        let found = find_in(&screen, &|node| has_id(node, "refund") && matches!(node, Node::Form(_)));
        assert!(found.is_some());
        assert!(find_in(&screen, &|node| has_id(node, "gone")).is_none());
        assert_eq!(depth_of(&screen, &|node| has_id(node, "b")), Some(4));

        // A rewrite changes the first `b` only: the one inside the form.
        let changed = rewrite_in(&mut screen, &|node| has_id(node, "b"), &mut |_| Some(text("b", "new")));
        assert!(changed);
        let mut values = Vec::new();
        walk(&screen.body, &mut |node| {
            if let Node::Text(text) = node {
                values.push(text.value.clone());
            }
        });
        assert_eq!(values, ["new", "outer"]);
        // Taking a node out, and a target that is not there.
        assert!(rewrite_in(&mut screen, &|node| has_id(node, "b"), &mut |_| None));
        assert!(rewrite_in(&mut screen, &|node| has_id(node, "b"), &mut |_| None));
        assert!(!rewrite_in(&mut screen, &|node| has_id(node, "b"), &mut |_| None));
        // The body itself can be the target.
        assert!(rewrite_in(&mut screen, &|node| has_id(node, "page"), &mut |_| Some(text("page", "all"))));
        assert!(matches!(screen.body, Node::Text(_)));
    }

    fn field(name: &str, id: Option<&str>) -> Node {
        Node::Field(Field {
            id: id.map(str::to_owned),
            name: name.to_owned(),
            ..Field::default()
        })
    }

    fn form(id: Option<&str>, action: &str, children: Vec<Node>) -> Node {
        Node::Form(Form {
            id: id.map(str::to_owned),
            action: action.to_owned(),
            children,
            ..Form::default()
        })
    }

    fn form_ids(node: &Node) -> Vec<String> {
        let survey = Survey::of(std::iter::once(node));
        survey
            .forms
            .iter()
            .map(|form| {
                let fields: Vec<&str> = form.fields.iter().map(|field| field.id.as_deref().unwrap()).collect();
                format!("{} [{}]", form.handle, fields.join(" "))
            })
            .collect()
    }

    #[test]
    fn every_form_and_field_gets_an_id_of_its_own() {
        let row = |id: &str, cells: Vec<Node>| TableRow {
            id: Some(id.to_owned()),
            cells,
            ..TableRow::default()
        };
        let table = Node::Table(Table {
            query: Some(Form {
                id: Some("orders-f".to_owned()),
                action: "/orders".to_owned(),
                children: vec![field("search", None)],
                ..Form::default()
            }),
            rows: vec![
                row("18", vec![form(None, "/inventory/18/restock", vec![field("quantity", None)])]),
                row("19", vec![form(None, "/inventory/19/restock", vec![field("quantity", None)])]),
                // Two rows whose forms go to one address: the row tells them apart.
                row("20", vec![form(None, "/cart/remove", vec![field("quantity", Some("qty"))])]),
                row("21", vec![form(None, "/cart/remove", vec![field("quantity", Some("qty"))])]),
            ],
            ..Table::default()
        });
        let mut nodes = vec![Node::Region(Region {
            id: "orders".to_owned(),
            src: "/orders".to_owned(),
            children: vec![table],
            ..Region::default()
        })];
        let (mut forms, mut fields, mut server) = (BTreeSet::new(), BTreeSet::new(), BTreeMap::new());
        Names {
            forms: &mut forms,
            fields: &mut fields,
            server: &mut server,
        }
        .name(&mut nodes, None);
        assert_eq!(
            form_ids(&nodes[0]),
            [
                "orders-f [orders-f.search]",
                "@/inventory/18/restock [@/inventory/18/restock.quantity]",
                "@/inventory/19/restock [@/inventory/19/restock.quantity]",
                "@/cart/remove~20 [qty]",
                "@/cart/remove~21 [@/cart/remove~21.quantity]",
            ]
        );
        assert_eq!(server["orders-f"], Some("orders-f".to_owned()));
        assert_eq!(server["@/cart/remove~21"], None);
        let survey = Survey::of(nodes.iter());
        assert_eq!(survey.forms[0].query_of.as_deref(), Some("orders"));
        assert_eq!(survey.regions[0].id, "orders");

        // A node that arrives later is named around what the screen already has.
        let mut later = vec![form(Some("orders-f"), "/x", vec![field("search", Some("qty"))])];
        Names {
            forms: &mut forms,
            fields: &mut fields,
            server: &mut server,
        }
        .name(&mut later, Some("panel"));
        assert_eq!(form_ids(&later[0]), ["orders-f~panel [orders-f~panel.search]"]);
        assert_eq!(server["orders-f~panel"], Some("orders-f".to_owned()));
    }

    #[test]
    fn what_is_open_and_what_wants_fetching_is_found() {
        let body = stack(
            None,
            vec![
                Node::Region(Region {
                    id: "figures".to_owned(),
                    src: "/fragments/figures".to_owned(),
                    lazy: true,
                    ..Region::default()
                }),
                Node::Sheet(Sheet {
                    id: "order".to_owned(),
                    open: true,
                    children: vec![Node::Dialog(Dialog {
                        id: "refund".to_owned(),
                        ..Dialog::default()
                    })],
                    ..Sheet::default()
                }),
                field("loose", None),
            ],
        );
        let survey = Survey::of(std::iter::once(&body));
        assert!(survey.regions[0].wants);
        assert_eq!(survey.open, ["order"]);
        assert_eq!(survey.loose.len(), 1);
        assert_eq!(depth(&body), 3);
    }
}
