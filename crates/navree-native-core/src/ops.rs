//! Ops: what the server changes on the screen in front.
//!
//! - An op finds its target by id among the kinds it can mean: `open` and
//!   `close` a sheet, a dialog, a popover or a disclosure; `errors` a form;
//!   `invalidate` a region; `append` and `prepend` a node that holds others.
//!   Among several the first in document order wins.
//! - An op whose target is not on the screen is skipped, and the ops after it
//!   still apply: an action written for two pages names nodes of both.
//! - `morph` keeps what the person did to the nodes that are in both: what
//!   they typed, and what is open. `replace` keeps nothing.
//! - A row is never in a list twice.

use std::collections::{BTreeMap, BTreeSet};

use navree_native::action::{Present, State};
use navree_native::node::Node;
use navree_native::op::Toast;
use navree_native::Op;

use crate::effect::{Change, Effect};
use crate::forms::field_view;
use crate::origin::Target;
use crate::state::{Content, Focus, FormMeta, FormView, Message, MessageInner, Notice, RegionMeta, ScreenId, ScreenView};
use crate::tree::{
    MAX_DEPTH, Names, Slot, Survey, depth, depth_of, fields_of, find_in, find_in_mut, find_mut, fits, form_of, has_id,
    holds, main_list, opens, rewrite_in, roots, slots, walk,
};
use crate::Core;

/// Opens or closes a sheet, a dialog or a disclosure. True when it changed;
/// false also for a node that has no such state of its own.
pub(crate) fn set_open_flag(node: &mut Node, open: bool) -> bool {
    let flag = match node {
        Node::Sheet(sheet) => &mut sheet.open,
        Node::Dialog(dialog) => &mut dialog.open,
        Node::Disclosure(disclosure) => &mut disclosure.open,
        _ => return false,
    };
    let changed = *flag != open;
    *flag = open;
    changed
}

/// Sets or clears a state the tree has no field for. True when it changed.
pub(crate) fn set_state(states: &mut BTreeMap<String, Vec<State>>, id: &str, state: State, on: bool) -> bool {
    let has = states.get(id).is_some_and(|set| set.contains(&state));
    if has == on {
        return false;
    }
    if on {
        states.entry(id.to_owned()).or_default().push(state);
    } else if let Some(set) = states.get_mut(id) {
        set.retain(|other| *other != state);
        if set.is_empty() {
            states.remove(id);
        }
    }
    true
}

fn is_open(node: &Node, states: &BTreeMap<String, Vec<State>>) -> bool {
    match node {
        Node::Sheet(sheet) => sheet.open,
        Node::Dialog(dialog) => dialog.open,
        Node::Disclosure(disclosure) => disclosure.open,
        Node::Popover(popover) => states.get(&popover.id).is_some_and(|set| set.contains(&State::Open)),
        _ => false,
    }
}

/// Takes the row `id` out of the first table that has it.
fn remove_row(node: &mut Node, id: &str) -> bool {
    if let Node::Table(table) = node {
        if let Some(at) = table.rows.iter().position(|row| row.id.as_deref() == Some(id)) {
            table.rows.remove(at);
            return true;
        }
    }
    slots(node).into_iter().any(|slot| match slot {
        Slot::Many(list) => list.iter_mut().any(|child| remove_row(child, id)),
        Slot::Maybe(child) => child.as_deref_mut().is_some_and(|child| remove_row(child, id)),
        Slot::One(child) => remove_row(child, id),
    })
}

impl Core {
    /// Applies ops to a screen, in order. `after_send`: they answer something
    /// the person just sent from it. True when they lead away from the screen.
    pub(crate) fn apply_ops(&mut self, id: ScreenId, ops: Vec<Op>, after_send: bool) -> bool {
        let mut current = id;
        let mut left = false;
        let focuses = ops.iter().any(|op| matches!(op, Op::Focus { .. }));
        for op in ops {
            match op {
                Op::Morph { target, node } => self.put(current, &target, node, true),
                Op::Replace { target, node } => self.put(current, &target, node, false),
                Op::Append { target, nodes, more } => self.add(current, &target, nodes, true, Some(more)),
                Op::Prepend { target, nodes } => self.add(current, &target, nodes, false, None),
                Op::Remove { target } => self.take_out(current, &target),
                // Without a focus of the server's own, the first field in error is brought into view.
                Op::Errors { form, fields } => self.errors(current, &form, &fields, after_send && !focuses),
                Op::Toast(toast) => self.toast(Some(current), toast),
                Op::Open { target } => self.set_open(current, &target, true),
                Op::Close { target } => self.set_open(current, &target, false),
                Op::Focus { target } => self.focus(current, target),
                Op::Url { href, .. } => {
                    if let Target::Inside(url) = self.resolve(&href, current) {
                        if let Some(view) = self.view.screens.get_mut(&current) {
                            view.url = url;
                            self.touch_screen(current);
                        }
                    }
                }
                Op::Navigate { href } => {
                    left = true;
                    let blank = self.view.screens.get(&current).is_some_and(|view| matches!(view.content, Content::Blank));
                    match self.resolve(&href, current) {
                        Target::Outside(url) => self.effects.push(Effect::OpenUrl { url }),
                        Target::Refused => {}
                        // A page that answers only "go there" is that other page.
                        Target::Inside(url) if blank && !after_send => self.replace(current, &url),
                        Target::Inside(url) if after_send => current = self.land(current, &url, None),
                        Target::Inside(url) => current = self.go(Some(current), &url, Present::Push).unwrap_or(current),
                    }
                }
                Op::NavigateExternal { href } => {
                    left = true;
                    match self.resolve(&href, current) {
                        Target::Outside(url) => self.effects.push(Effect::OpenUrl { url }),
                        Target::Inside(url) => {
                            let url = self.inner.origin.absolute(&url);
                            self.effects.push(Effect::OpenUrl { url });
                        }
                        Target::Refused => {}
                    }
                }
                Op::Reload => {
                    left = true;
                    self.fetch_page(current, false);
                }
                Op::Invalidate { target } => {
                    let known = self.inner.screens.get(&current).is_some_and(|inner| inner.regions.contains_key(&target));
                    if known {
                        self.fetch_region(current, &target, None);
                    }
                }
                // An op this build does not know changes nothing.
                _ => {}
            }
        }
        left
    }

    fn too_deep(&mut self, target: &str) {
        self.effects.push(Effect::Report {
            what: "depth".to_owned(),
            detail: format!("an op on `{target}` would nest the screen past {MAX_DEPTH} nodes"),
        });
    }

    /// Puts `node` in place of the node `target`.
    fn put(&mut self, id: ScreenId, target: &str, node: Node, morph: bool) {
        let (Some(view), Some(inner)) = (self.view.screens.get_mut(&id), self.inner.screens.get_mut(&id)) else {
            return;
        };
        let ScreenView { content, states, .. } = view;
        let Content::Native { screen } = content else {
            return;
        };
        // A dialog and the form in it may share an id: the node of the arriving
        // node's own kind is meant, when there is one.
        let kind = node.kind();
        let same = |other: &Node| has_id(other, target) && other.kind() == kind;
        let any = |other: &Node| has_id(other, target);
        let is: &dyn Fn(&Node) -> bool = if find_in(screen, &same).is_some() { &same } else { &any };
        let Some(old) = find_in(screen, is) else {
            return;
        };
        if depth_of(screen, is).unwrap_or(1) + depth(&node) - 1 > MAX_DEPTH {
            return self.too_deep(target);
        }
        // What the old node held is let go of: its names, and, unless this is
        // a morph, what the person did to it.
        let survey = Survey::of(std::iter::once(old));
        for field in survey.forms.iter().flat_map(|form| &form.fields).chain(&survey.loose) {
            if let Some(field) = &field.id {
                inner.field_ids.remove(field);
                if !morph {
                    inner.dirty.remove(field);
                }
            }
        }
        for form in &survey.forms {
            inner.form_ids.remove(&form.handle);
        }
        let mut was_open = Vec::new();
        let mut ids = Vec::new();
        walk(old, &mut |other| {
            let Some(id) = other.id() else {
                return;
            };
            if morph && opens(other) {
                was_open.push((id.to_owned(), is_open(other, states)));
            }
            ids.push(id.to_owned());
        });
        for id in &ids {
            states.remove(id);
        }
        let mut nodes = vec![node];
        Names {
            forms: &mut inner.form_ids,
            fields: &mut inner.field_ids,
            server: &mut inner.server_ids,
        }
        .name(&mut nodes, None);
        let Some(mut node) = nodes.pop() else {
            return;
        };
        for (open_id, open) in was_open {
            if let Some(found) = find_mut(&mut node, &|other| has_id(other, &open_id) && opens(other)) {
                if matches!(found, Node::Popover(_)) {
                    set_state(states, &open_id, State::Open, open);
                } else {
                    set_open_flag(found, open);
                }
            }
        }
        let fresh: BTreeSet<String> = Survey::of(std::iter::once(&node)).forms.iter().map(|form| form.handle.clone()).collect();
        let mut node = Some(node);
        rewrite_in(screen, is, &mut |_| node.take());
        self.reconcile(id, Some(&fresh));
        self.touch_screen(id);
    }

    /// Adds nodes to the children of the node `target`. `more` is given for
    /// an append: the address after these rows, or none when they are the last.
    pub(crate) fn add(&mut self, id: ScreenId, target: &str, nodes: Vec<Node>, at_end: bool, more: Option<Option<String>>) {
        let (Some(view), Some(inner)) = (self.view.screens.get_mut(&id), self.inner.screens.get_mut(&id)) else {
            return;
        };
        let Content::Native { screen } = &mut view.content else {
            return;
        };
        let is = |other: &Node| has_id(other, target) && holds(other);
        let Some(at) = depth_of(screen, &is) else {
            return;
        };
        if at + nodes.iter().map(depth).max().unwrap_or(0) > MAX_DEPTH {
            return self.too_deep(target);
        }
        // An item is never in a list twice: one whose id the list already has is passed over.
        let mut nodes = {
            let held: BTreeSet<&str> = find_in(screen, &is).map(Node::children).unwrap_or_default().into_iter().filter_map(Node::id).collect();
            let mut arriving = BTreeSet::new();
            let keep: Vec<bool> = nodes.iter().map(|node| node.id().is_none_or(|id| !held.contains(id) && arriving.insert(id.to_owned()))).collect();
            let mut keep = keep.into_iter();
            let mut nodes = nodes;
            nodes.retain(|_| keep.next().unwrap_or(true));
            nodes
        };
        Names {
            forms: &mut inner.form_ids,
            fields: &mut inner.field_ids,
            server: &mut inner.server_ids,
        }
        .name(&mut nodes, Some(target));
        let Some(parent) = find_in_mut(screen, &is) else {
            return;
        };
        let mut rows = false;
        if let Node::List(list) = parent {
            rows = at_end;
            if let (true, Some(more)) = (at_end, more) {
                list.more = more;
            }
        }
        let Some(children) = main_list(parent) else {
            return;
        };
        let from = children.len();
        let count = nodes.len();
        if at_end {
            children.extend(nodes);
        } else {
            children.splice(0..0, nodes);
        }
        if rows && self.absorb(id, target, from) {
            self.touch(Change::Rows {
                screen: id,
                id: target.to_owned(),
                from,
                count,
            });
        } else {
            self.reconcile(id, Some(&BTreeSet::new()));
            self.touch_screen(id);
        }
    }

    /// Takes in the forms of rows just added at the end of the list `target`,
    /// without reading the rest of the screen again. False when the rows hold
    /// more than forms, and the whole screen has to be read.
    pub(crate) fn absorb(&mut self, id: ScreenId, target: &str, from: usize) -> bool {
        let (Some(view), Some(inner)) = (self.view.screens.get_mut(&id), self.inner.screens.get_mut(&id)) else {
            return false;
        };
        let ScreenView { content, forms, lists, .. } = view;
        let Content::Native { screen } = content else {
            return false;
        };
        let added: Vec<&Node> = match find_in(screen, &|other| has_id(other, target) && matches!(other, Node::List(_) | Node::Table(_) | Node::Region(_))) {
            Some(Node::List(list)) => list.children.iter().skip(from).collect(),
            Some(other) => match crate::regions::table_of(other) {
                Some(table) => table.rows.iter().skip(from).flat_map(|row| &row.cells).collect(),
                None => return false,
            },
            None => return false,
        };
        let survey = Survey::of(added.into_iter());
        if !survey.regions.is_empty() || !survey.open.is_empty() || !survey.loose.is_empty() || !survey.more.is_empty() {
            return false;
        }
        for found in survey.forms {
            let mut form = FormView::default();
            for field in found.fields {
                if let Some(field_id) = &field.id {
                    form.fields.insert(field_id.clone(), field_view(field));
                }
            }
            inner.forms.insert(
                found.handle.clone(),
                FormMeta {
                    method: found.method,
                    action: found.action.to_owned(),
                    query_of: found.query_of,
                },
            );
            forms.insert(found.handle, form);
        }
        if !lists.contains_key(target) {
            // Nothing more is on its way for a list that was never asked.
        }
        true
    }

    /// Takes the node `target` out: a node, or a row of a table.
    fn take_out(&mut self, id: ScreenId, target: &str) {
        let (Some(view), Some(inner)) = (self.view.screens.get_mut(&id), self.inner.screens.get_mut(&id)) else {
            return;
        };
        let ScreenView { content, states, .. } = view;
        let Content::Native { screen } = content else {
            return;
        };
        let is = |other: &Node| has_id(other, target);
        let found = match find_in(screen, &is) {
            Some(old) => {
                let survey = Survey::of(std::iter::once(old));
                for field in survey.forms.iter().flat_map(|form| &form.fields).chain(&survey.loose) {
                    if let Some(field) = &field.id {
                        inner.dirty.remove(field);
                    }
                }
                let mut ids = Vec::new();
                walk(old, &mut |other| ids.extend(other.id().map(str::to_owned)));
                for id in &ids {
                    states.remove(id);
                }
                rewrite_in(screen, &is, &mut |_| None)
            }
            None => screen.actions.iter_mut().chain(std::iter::once(&mut screen.body)).any(|root| remove_row(root, target)),
        };
        if found {
            self.reconcile(id, Some(&BTreeSet::new()));
            self.touch_screen(id);
        }
    }

    /// Shows messages at the fields of a form, or clears them.
    fn errors(&mut self, id: ScreenId, form: &str, fields: &BTreeMap<String, Vec<String>>, focus: bool) {
        // A form that is not on the screen is skipped.
        let Some(handle) = self.form_named(id, form) else {
            return;
        };
        let Some(view) = self.view.screens.get_mut(&id) else {
            return;
        };
        let order: Vec<(String, String)> = view
            .screen()
            .and_then(|screen| form_of(screen, &handle))
            .map(|form| fields_of(form).into_iter().filter_map(|field| Some((field.id.clone()?, field.name.clone()))).collect())
            .unwrap_or_default();
        let Some(state) = view.forms.get_mut(&handle) else {
            return;
        };
        for field in state.fields.values_mut() {
            field.errors.clear();
        }
        let mut first = None;
        let mut placed = BTreeSet::new();
        for (field_id, name) in &order {
            let Some(messages) = fields.get(name).filter(|messages| !messages.is_empty()) else {
                continue;
            };
            // Fields that share a name (a group of checkboxes) say it once, at the first.
            if !placed.insert(name.as_str()) {
                continue;
            }
            if let Some(field) = state.fields.get_mut(field_id) {
                field.errors.clone_from(messages);
                first.get_or_insert_with(|| field_id.clone());
            }
        }
        self.changed.insert(Change::Form {
            screen: id,
            form: handle,
        });
        if let (true, Some(target)) = (focus, first) {
            self.inner.next.focus += 1;
            view.focus = Some(Focus {
                target,
                serial: self.inner.next.focus,
            });
            self.changed.insert(Change::Screen { id });
        }
        let count = fields.values().filter(|messages| !messages.is_empty()).count();
        // A message for a field the form does not have is still said.
        let loose: Vec<String> = fields.iter().filter(|(name, messages)| !messages.is_empty() && !placed.contains(name.as_str())).map(|(_, messages)| messages.join(" ")).collect();
        for text in loose {
            self.toast(
                Some(id),
                Toast {
                    tone: navree_native::Tone::Danger,
                    text,
                    ..Toast::default()
                },
            );
        }
        if count > 0 {
            self.effects.push(Effect::Announce {
                text: None,
                notice: Some(Notice::FieldsNeedAttention),
                count: Some(u32::try_from(count).unwrap_or(u32::MAX)),
            });
        }
    }

    /// Shows a toast the server sent.
    pub(crate) fn toast(&mut self, screen: Option<ScreenId>, toast: Toast) {
        let message = Message {
            id: 0,
            tone: toast.tone,
            text: Some(toast.text),
            title: toast.title,
            notice: None,
            action: toast.action.as_ref().map(|action| action.label.clone()),
        };
        self.say(
            message,
            MessageInner {
                action: toast.action,
                screen,
                name: toast.id,
            },
        );
    }

    /// Opens or closes the sheet, dialog, popover or disclosure `target`.
    /// Closing one closes what is open inside it.
    pub(crate) fn set_open(&mut self, id: ScreenId, target: &str, open: bool) {
        let Some(view) = self.view.screens.get_mut(&id) else {
            return;
        };
        let ScreenView {
            content,
            states,
            overlays,
            ..
        } = view;
        let Content::Native { screen } = content else {
            return;
        };
        let Some(node) = find_in_mut(screen, &|other| has_id(other, target) && opens(other)) else {
            return;
        };
        let overlay = !matches!(node, Node::Disclosure(_));
        let mut changed = if matches!(node, Node::Popover(_)) {
            set_state(states, target, State::Open, open)
        } else {
            set_open_flag(node, open)
        };
        if !open && overlay {
            let mut inside = Vec::new();
            walk(node, &mut |other| {
                if matches!(other, Node::Sheet(_) | Node::Dialog(_) | Node::Popover(_)) && other.id() != Some(target) {
                    inside.extend(other.id().map(str::to_owned));
                }
            });
            for inner in &inside {
                if let Some(found) = find_mut(node, &|other| has_id(other, inner) && opens(other)) {
                    changed |= set_open_flag(found, false);
                }
                changed |= set_state(states, inner, State::Open, false);
            }
            overlays.retain(|other| !inside.contains(other));
        }
        if overlay {
            let was = overlays.len();
            overlays.retain(|other| other != target);
            changed |= open || overlays.len() != was;
            if open {
                overlays.push(target.to_owned());
            }
        }
        if changed {
            self.touch_screen(id);
        }
    }

    /// Closes the sheets, dialogs and popovers of a screen that is being left.
    pub(crate) fn close_overlays(&mut self, id: ScreenId) {
        let open = self.view.screens.get(&id).map(|view| view.overlays.clone()).unwrap_or_default();
        for target in open.iter().rev() {
            self.set_open(id, target, false);
        }
    }

    pub(crate) fn focus(&mut self, id: ScreenId, target: String) {
        if let Some(view) = self.view.screens.get_mut(&id) {
            self.inner.next.focus += 1;
            view.focus = Some(Focus {
                target,
                serial: self.inner.next.focus,
            });
            self.touch_screen(id);
        }
    }

    /// Brings what the core keeps beside a screen's tree in line with the
    /// tree: the state of its forms, regions and lists, and what is open.
    /// `fresh` names the forms that just arrived, whose messages are the
    /// tree's own; without it every form is fresh.
    pub(crate) fn reconcile(&mut self, id: ScreenId, fresh: Option<&BTreeSet<String>>) {
        let (Some(view), Some(inner)) = (self.view.screens.get_mut(&id), self.inner.screens.get_mut(&id)) else {
            return;
        };
        let ScreenView {
            content,
            forms,
            regions,
            lists,
            overlays,
            states,
            url,
            ..
        } = view;
        let Content::Native { screen } = content else {
            return;
        };
        let survey = Survey::of(roots(screen));
        let mut old = std::mem::take(forms);
        inner.forms.clear();
        inner.form_ids.clear();
        inner.field_ids.clear();
        let loose = (!survey.loose.is_empty()).then(|| (String::new(), survey.loose.as_slice(), None));
        let found = survey.forms.iter().map(|form| {
            let meta = FormMeta {
                method: form.method,
                action: form.action.to_owned(),
                query_of: form.query_of.clone(),
            };
            (form.handle.clone(), form.fields.as_slice(), Some(meta))
        });
        for (handle, fields, meta) in found.chain(loose) {
            let is_fresh = fresh.is_none_or(|fresh| fresh.contains(&handle));
            let was = old.remove(&handle);
            let mut form = FormView {
                pending: inner.sending.contains_key(&handle),
                ..FormView::default()
            };
            for field in fields {
                let Some(field_id) = field.id.clone() else {
                    continue;
                };
                let mut now = field_view(field);
                if let Some(was) = was.as_ref().and_then(|was| was.fields.get(&field_id)) {
                    // What the person typed stays, in a control of the same kind.
                    if inner.dirty.contains(&field_id) && fits(&field.input, &was.value) {
                        now.value = was.value.clone();
                    }
                    if !is_fresh && field.error.is_none() {
                        now.errors.clone_from(&was.errors);
                    }
                }
                inner.field_ids.insert(field_id.clone());
                form.fields.insert(field_id, now);
            }
            if let Some(meta) = meta {
                inner.form_ids.insert(handle.clone());
                inner.forms.insert(handle.clone(), meta);
            }
            forms.insert(handle, form);
        }
        inner.dirty.retain(|field| inner.field_ids.contains(field));
        inner.server_ids.retain(|handle, _| inner.form_ids.contains(handle));
        inner.waiting.retain(|handle, _| inner.form_ids.contains(handle));

        let mut old = std::mem::take(regions);
        inner.regions.clear();
        let mut wants = Vec::new();
        for found in &survey.regions {
            let state = old.remove(found.id).unwrap_or_default();
            if found.wants && !state.loading && state.failure.is_none() {
                wants.push(found.id.to_owned());
            }
            regions.insert(found.id.to_owned(), state);
            let inside = |href: &str| match self.inner.origin.resolve(href, url) {
                Target::Inside(url) => Some(url),
                _ => None,
            };
            if let Some(src) = inside(found.src) {
                let live = found.live.and_then(|live| Some((inside(&live.src)?, live.event.clone())));
                inner.regions.insert(found.id.to_owned(), RegionMeta { src, live });
            }
        }
        inner.live.retain(|region, _| inner.regions.get(region).is_some_and(|meta| meta.live.is_some()));
        inner.turns.retain(|region, _| inner.regions.contains_key(region));
        lists.retain(|list, _| survey.more.iter().any(|other| other == list) || inner.more.contains_key(list));

        if !states.is_empty() {
            states.retain(|id, _| find_in(screen, &|node| has_id(node, id)).is_some());
        }
        let mut open: Vec<&str> = survey.open.clone();
        open.extend(states.iter().filter(|(_, set)| set.contains(&State::Open)).map(|(id, _)| id.as_str()));
        overlays.retain(|id| open.contains(&id.as_str()));
        for id in open {
            if !overlays.iter().any(|other| other == id) {
                overlays.push(id.to_owned());
            }
        }
        for region in wants {
            self.fetch_region(id, &region, None);
        }
    }
}
