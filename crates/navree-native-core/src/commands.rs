//! Commands: the closed list of changes a control makes on its own, with no
//! request. They touch state, never where a thing points or what it runs.
//!
//! A command whose target is not on the screen is skipped, like an op.

use navree_native::action::{Command, State};
use navree_native::node::Node;
use navree_native::op::Toast;

use crate::effect::Effect;
use crate::ops::set_state;
use crate::state::{Content, Notice, ScreenId, ScreenView};
use crate::tree::{find_in, find_in_mut, has_id, opens};
use crate::Core;

/// Unticks the items of the menu that holds the item `target` which belong
/// to the same choice of one. True when one was ticked.
fn untick_others(node: &mut Node, target: &str, group: &str) -> bool {
    if let Node::Menu(menu) = node
        && menu.items.iter().any(|item| has_id(item, target))
    {
        let mut changed = false;
        for item in &mut menu.items {
            if let Node::Button(button) = item
                && button.id.as_deref() != Some(target)
                && button.one_of.as_deref() == Some(group)
                && button.checked == Some(true)
            {
                button.checked = Some(false);
                changed = true;
            }
        }
        return changed;
    }
    let mut changed = false;
    for slot in crate::tree::slots(node) {
        changed |= match slot {
            crate::tree::Slot::Many(list) => {
                // Every child is looked at: a menu may stand beside another.
                let mut any = false;
                for child in list.iter_mut() {
                    any |= untick_others(child, target, group);
                }
                any
            }
            crate::tree::Slot::Maybe(child) => child.as_deref_mut().is_some_and(|child| untick_others(child, target, group)),
            crate::tree::Slot::One(child) => untick_others(child, target, group),
        };
    }
    changed
}

impl Core {
    /// Runs commands on a screen, in order. `text` is what the control they
    /// run on holds, when it holds text.
    pub(crate) fn commands(&mut self, id: ScreenId, list: Vec<Command>, text: Option<&str>) {
        for command in list {
            match command {
                Command::Toggle { id: target } => {
                    let hidden = self.has_state(id, &target, State::Hidden);
                    self.put_state(id, &target, State::Hidden, !hidden);
                }
                Command::Show { id: target } => self.put_state(id, &target, State::Hidden, false),
                Command::Hide { id: target } => self.put_state(id, &target, State::Hidden, true),
                Command::Open { id: target } => self.set_open(id, &target, true),
                Command::Close { id: target } => self.set_open(id, &target, false),
                Command::Copy { text } => {
                    self.effects.push(Effect::Copy { text, sensitive: false });
                    self.effects.push(Effect::Announce {
                        text: None,
                        notice: Some(Notice::Copied),
                        count: None,
                    });
                }
                // A state from a newer server is not one this build keeps.
                Command::Set { state: State::Unknown, .. } | Command::Flip { state: State::Unknown, .. } => {}
                Command::Set { id: target, state, on } => self.put_state(id, &target, state, on),
                Command::Flip { id: target, state } => {
                    let on = self.has_state(id, &target, state);
                    self.put_state(id, &target, state, !on);
                }
                Command::Focus { id: target } => self.focus(id, target),
                Command::Submit { id: form } => self.submit(id, &form, None, None),
                Command::Text { id: target, text } => self.put_text(id, &target, text),
                Command::Count { id: target, max } => {
                    // Characters as a person counts them, not bytes.
                    let held = text.unwrap_or("").chars().count();
                    let shown = match max {
                        Some(max) => format!("{held}/{max}"),
                        None => held.to_string(),
                    };
                    self.put_text(id, &target, shown);
                }
                Command::Value { id: target, value } => self.put_value(id, &target, value),
                Command::Toast { tone, text } => self.toast(
                    Some(id),
                    Toast {
                        tone,
                        text,
                        ..Toast::default()
                    },
                ),
                // A command this build does not know does nothing.
                _ => {}
            }
        }
    }

    /// Whether a node holds a state now: in its own field when it has one for
    /// it, beside the tree otherwise.
    fn has_state(&self, id: ScreenId, target: &str, state: State) -> bool {
        let Some(view) = self.view.screens.get(&id) else {
            return false;
        };
        let beside = view.states.get(target).is_some_and(|set| set.contains(&state));
        let Some(screen) = view.screen() else {
            return beside;
        };
        let found = match state {
            State::Open => find_in(screen, &|node| has_id(node, target) && opens(node)),
            _ => find_in(screen, &|node| has_id(node, target)),
        };
        match (found, state) {
            (Some(Node::Stack(stack)), State::Hidden) => stack.hidden,
            (Some(Node::Sheet(sheet)), State::Open) => sheet.open,
            (Some(Node::Dialog(dialog)), State::Open) => dialog.open,
            (Some(Node::Disclosure(disclosure)), State::Open) => disclosure.open,
            (Some(Node::Button(button)), State::Disabled) => button.disabled,
            (Some(Node::Button(button)), State::Pending) => button.pending,
            (Some(Node::Button(button)), State::Checked) if button.checked.is_some() => button.checked == Some(true),
            (Some(Node::Field(field)), State::Disabled) => field.disabled,
            _ => beside,
        }
    }

    /// Sets or clears a state of a node: in the node's own field when it has
    /// one for it, beside the tree otherwise.
    fn put_state(&mut self, id: ScreenId, target: &str, state: State, on: bool) {
        if state == State::Open {
            return self.set_open(id, target, on);
        }
        let Some(view) = self.view.screens.get_mut(&id) else {
            return;
        };
        let ScreenView { content, states, .. } = view;
        let Content::Native { screen } = content else {
            return;
        };
        // A menu item that is ticked says so itself, and one of a choice of one
        // unticks the others of its menu.
        if state == State::Checked
            && let Some(Node::Button(button)) = find_in_mut(screen, &|node| has_id(node, target) && matches!(node, Node::Button(button) if button.checked.is_some()))
        {
            let mut changed = button.checked.replace(on) != Some(on);
            if let Some(group) = button.one_of.clone().filter(|_| on) {
                for root in screen.actions.iter_mut().chain(std::iter::once(&mut screen.body)) {
                    changed |= untick_others(root, target, &group);
                }
            }
            if changed {
                self.touch_screen(id);
            }
            return;
        }
        let Some(node) = find_in_mut(screen, &|node| has_id(node, target)) else {
            return;
        };
        let flag = match (node, state) {
            (Node::Stack(stack), State::Hidden) => Some(&mut stack.hidden),
            (Node::Button(button), State::Disabled) => Some(&mut button.disabled),
            (Node::Button(button), State::Pending) => Some(&mut button.pending),
            (Node::Field(field), State::Disabled) => Some(&mut field.disabled),
            _ => None,
        };
        let changed = match flag {
            Some(flag) => std::mem::replace(flag, on) != on,
            None => set_state(states, target, state, on),
        };
        if changed {
            self.touch_screen(id);
        }
    }

    /// Sets the words of a text node. Nothing else takes words from a
    /// command: not a button's label, not a field's value.
    fn put_text(&mut self, id: ScreenId, target: &str, text: String) {
        let Some(screen) = self.view.screens.get_mut(&id).and_then(ScreenView::screen_mut) else {
            return;
        };
        let Some(Node::Text(node)) = find_in_mut(screen, &|node| has_id(node, target) && matches!(node, Node::Text(_))) else {
            return;
        };
        if node.value != text {
            node.value = text;
            self.touch_screen(id);
        }
    }

    /// Sets a hidden value of a form: only one the page opened to commands by
    /// giving it an id. A form's token stays as the server wrote it.
    fn put_value(&mut self, id: ScreenId, target: &str, value: String) {
        /// Sets the hidden value `target` wherever a form holds it. `Some`
        /// when it was found: whether it changed.
        fn set(node: &mut Node, target: &str, value: &str) -> Option<bool> {
            let form = match node {
                Node::Form(form) => Some(form),
                Node::Table(table) => table.query.as_mut(),
                _ => None,
            };
            let hidden = form.and_then(|form| form.hidden.iter_mut().find(|hidden| hidden.id.as_deref() == Some(target)));
            if let Some(hidden) = hidden {
                let changed = hidden.value != value;
                value.clone_into(&mut hidden.value);
                return Some(changed);
            }
            crate::tree::slots(node).into_iter().find_map(|slot| match slot {
                crate::tree::Slot::Many(list) => list.iter_mut().find_map(|child| set(child, target, value)),
                crate::tree::Slot::Maybe(child) => child.as_deref_mut().and_then(|child| set(child, target, value)),
                crate::tree::Slot::One(child) => set(child, target, value),
            })
        }
        let Some(screen) = self.view.screens.get_mut(&id).and_then(ScreenView::screen_mut) else {
            return;
        };
        let found = screen.actions.iter_mut().chain(std::iter::once(&mut screen.body)).find_map(|root| set(root, target, &value));
        if found == Some(true) {
            self.touch_screen(id);
        }
    }
}
