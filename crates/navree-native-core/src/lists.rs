//! Lists and tables that change while the person reads them.
//!
//! - A list that names where to ask what changed (`changes`) and the revision
//!   its rows are at (`since`) is brought up to date by `merge` ops: a row
//!   that changed takes the place of the one with its id where it stands, a
//!   new one joins at the end the op names, and the ones that left go. What
//!   the person typed into a row that stays is kept. A stream (`live`) says
//!   when to ask; nothing in its messages is read. A burst of messages costs
//!   one ask, and an answer that says more changes follow is asked again at
//!   once, as long as the revision moved.
//! - A list also loads the rows before its first by cursor (`before`), one
//!   fetch at a time, as it loads the ones after its last.
//! - A table takes rows by `rows` ops, and one row is changed by a `row` op,
//!   which keeps whether the person chose it.
//! - The rows the person chose in a table are sent, each by its id, with the
//!   form its `select` names, in the table's order.
//! - A row the person dragged stands where they put it at once, and the move
//!   is posted. A move the server refuses puts the row back where it was.

use std::collections::BTreeSet;

use navree_native::node::{Method, Node, Table, TableRow};
use navree_native::op::Edge;
use navree_native::Screen;

use crate::decode::Answer;
use crate::effect::{Body, Part};
use crate::forms::encode;
use crate::origin::Target;
use crate::reduce::SEND_LIMIT;
use crate::regions::{MORE_LIMIT, QUIET, pages, table_of_mut};
use crate::state::{Content, Notice, Pending, ScreenId, ScreenView, TimerFor};
use crate::tree::{MAX_DEPTH, Names, Slot, Survey, depth, depth_of, find_in, find_in_mut, form_of, slots, walk};
use crate::Core;

fn is_list(node: &Node, id: &str) -> bool {
    matches!(node, Node::List(list) if list.id.as_deref() == Some(id))
}

fn has_row(node: &Node, id: &str) -> bool {
    matches!(node, Node::Table(table) if table.rows.iter().any(|row| row.id.as_deref() == Some(id)))
}

/// The table row `id`, in the first table that has it.
fn row_mut<'a>(node: &'a mut Node, id: &str) -> Option<&'a mut TableRow> {
    if has_row(node, id) {
        return match node {
            Node::Table(table) => table.rows.iter_mut().find(|row| row.id.as_deref() == Some(id)),
            _ => None,
        };
    }
    for slot in slots(node) {
        let found = match slot {
            Slot::Many(list) => list.iter_mut().find_map(|child| row_mut(child, id)),
            Slot::Maybe(child) => child.as_deref_mut().and_then(|child| row_mut(child, id)),
            Slot::One(child) => row_mut(child, id),
        };
        if found.is_some() {
            return found;
        }
    }
    None
}

fn text(name: &str, value: &str) -> Part {
    Part::Text {
        name: name.to_owned(),
        value: value.to_owned(),
    }
}

/// The ids of the rows chosen in `table`, in its order.
pub(crate) fn chosen_in(table: &Table) -> Vec<&str> {
    table.rows.iter().filter(|row| row.selected).filter_map(|row| row.id.as_deref()).collect()
}

/// A value of a list node that is read by the list's id.
fn of_list<T>(screen: &Screen, id: &str, read: impl Fn(&navree_native::node::List) -> T) -> Option<T> {
    match find_in(screen, &|node| is_list(node, id)) {
        Some(Node::List(list)) => Some(read(list)),
        _ => None,
    }
}

impl Core {
    // What changed in a list.

    /// Brings the list `target` up to date: `nodes` are the rows that are new
    /// or changed, `gone` the ids of the ones that left, `since` the revision
    /// it is at now.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn merge(&mut self, id: ScreenId, target: &str, mut nodes: Vec<Node>, gone: &[String], since: String, at: Edge, more: bool) {
        let (Some(view), Some(inner)) = (self.view.screens.get_mut(&id), self.inner.screens.get_mut(&id)) else {
            return;
        };
        let ScreenView { content, states, .. } = view;
        let Content::Native { screen } = content else {
            return;
        };
        let is = |other: &Node| is_list(other, target);
        let Some(stands) = depth_of(screen, &is) else {
            return;
        };
        if stands + nodes.iter().map(depth).max().unwrap_or(0) > MAX_DEPTH {
            return self.too_deep(target);
        }
        let Some(Node::List(list)) = find_in_mut(screen, &is) else {
            return;
        };
        // What the core kept by the names of the rows that leave goes; a row
        // a newer one takes the place of gives its names up to it.
        let mut forgotten = Vec::new();
        {
            let arriving: BTreeSet<&str> = nodes.iter().filter_map(Node::id).collect();
            for child in &list.children {
                let Some(child_id) = child.id() else {
                    continue;
                };
                let leaves = gone.iter().any(|gone| gone == child_id);
                if !leaves && !arriving.contains(child_id) {
                    continue;
                }
                let survey = Survey::of(std::iter::once(child));
                for field in survey.forms.iter().flat_map(|form| &form.fields).chain(&survey.loose) {
                    if let Some(field) = &field.id {
                        inner.field_ids.remove(field);
                        if leaves {
                            inner.dirty.remove(field);
                        }
                    }
                }
                for form in &survey.forms {
                    inner.form_ids.remove(&form.handle);
                    if leaves {
                        inner.replaced.insert(form.handle.clone());
                    }
                }
                if leaves {
                    walk(child, &mut |other| forgotten.extend(other.id().map(str::to_owned)));
                }
            }
        }
        for name in &forgotten {
            states.remove(name);
        }
        list.children.retain(|child| !child.id().is_some_and(|id| gone.iter().any(|gone| gone == id)));
        Names {
            forms: &mut inner.form_ids,
            fields: &mut inner.field_ids,
            server: &mut inner.server_ids,
        }
        .name(&mut nodes, Some(target));
        let fresh: BTreeSet<String> = Survey::of(nodes.iter()).forms.iter().map(|form| form.handle.clone()).collect();
        let mut new: Vec<Node> = Vec::new();
        for node in nodes {
            let place = node.id().and_then(|id| list.children.iter().position(|child| child.id() == Some(id)));
            match place {
                Some(place) => list.children[place] = node,
                // A row is never in a list twice: of two new ones with one id the first stays.
                None if node.id().is_some_and(|id| new.iter().any(|other| other.id() == Some(id))) => {}
                None => new.push(node),
            }
        }
        match at {
            Edge::Start => {
                list.children.splice(0..0, new);
            }
            _ => list.children.extend(new),
        }
        // More changes follow, as long as the revision moved: a server that
        // stands still is not asked for ever.
        let moved = list.since.as_deref() != Some(since.as_str());
        list.since = Some(since);
        if let Some(sub) = inner.live_lists.get_mut(target) {
            sub.again = more && moved;
        }
        self.reconcile(id, Some(&fresh));
        self.touch_screen(id);
    }

    /// Asks what changed in a list since its revision, or, if an ask is still
    /// out, once more when it lands.
    pub(crate) fn ask_changes(&mut self, id: ScreenId, list: &str) {
        let (Some(view), Some(inner)) = (self.view.screens.get(&id), self.inner.screens.get_mut(&id)) else {
            return;
        };
        let Some(sub) = inner.live_lists.get_mut(list) else {
            return;
        };
        sub.timer = None;
        if sub.busy {
            sub.dirty = true;
            return;
        }
        let since = view.screen().and_then(|screen| of_list(screen, list, |list| list.since.clone())).flatten();
        let mut url = sub.changes.clone();
        if let Some(since) = since {
            url.push(if url.contains('?') { '&' } else { '?' });
            url.push_str(&encode(&[text("since", &since)]));
        }
        sub.busy = true;
        sub.dirty = false;
        sub.again = false;
        let purpose = Pending::Changes {
            screen: id,
            visit: inner.visit,
            list: list.to_owned(),
        };
        self.fetch_part(purpose, &url, list, MORE_LIMIT);
    }

    pub(crate) fn changes_answered(&mut self, id: ScreenId, visit: u64, list: &str, answer: Answer) {
        let Some(inner) = self.inner.screens.get_mut(&id) else {
            return;
        };
        if let Some(sub) = inner.live_lists.get_mut(list) {
            sub.busy = false;
        }
        // The screen was asked for again: what it shows now is newer than this.
        if inner.visit != visit {
            return;
        }
        if let Answer::Tree { status, mut read, shell, .. } = answer {
            if self.note(&mut read, shell) {
                return;
            }
            if let Some(way_in) = Self::way_in(status, &read.envelope.ops) {
                return self.session_ended(id, &way_in);
            }
            if status < 400 {
                self.apply_ops(id, std::mem::take(&mut read.envelope.ops), false);
            }
        }
        let Some(sub) = self.inner.screens.get_mut(&id).and_then(|inner| inner.live_lists.get_mut(list)) else {
            return;
        };
        if std::mem::take(&mut sub.again) {
            return self.ask_changes(id, list);
        }
        if std::mem::take(&mut sub.dirty) && sub.timer.is_none() {
            self.changes_soon(id, list);
        }
    }

    /// Asks what changed in a list once its stream has gone quiet.
    fn changes_soon(&mut self, id: ScreenId, list: &str) {
        let waits = self.inner.screens.get(&id).and_then(|inner| inner.live_lists.get(list)).is_some_and(|sub| sub.timer.is_some());
        if waits {
            return;
        }
        let timer = self.timer(
            QUIET,
            TimerFor::Changes {
                screen: id,
                list: list.to_owned(),
            },
        );
        if let Some(sub) = self.inner.screens.get_mut(&id).and_then(|inner| inner.live_lists.get_mut(list)) {
            sub.timer = Some(timer);
        }
    }

    /// The lists of a screen that listen to a stream: for an event of this
    /// name, or for any when `name` is none.
    fn lists_listening(&self, id: ScreenId, url: &str, name: Option<Option<&str>>) -> Vec<String> {
        let Some(inner) = self.inner.screens.get(&id) else {
            return Vec::new();
        };
        inner
            .live_lists
            .iter()
            .filter(|(_, sub)| {
                sub.live.as_ref().is_some_and(|(src, event)| {
                    src == url
                        && match name {
                            None => true,
                            // A list that names no event hears every message.
                            Some(name) => event.as_deref().is_none_or(|event| Some(event) == name),
                        }
                })
            })
            .map(|(list, _)| list.clone())
            .collect()
    }

    /// A stream spoke: the lists that listen ask what changed, once it is quiet.
    pub(crate) fn lists_heard(&mut self, id: ScreenId, url: &str, name: Option<&str>) {
        for list in self.lists_listening(id, url, Some(name)) {
            let busy = self.inner.screens.get_mut(&id).and_then(|inner| inner.live_lists.get_mut(&list)).is_some_and(|sub| {
                sub.dirty |= sub.busy;
                sub.busy
            });
            if !busy {
                self.changes_soon(id, &list);
            }
        }
    }

    /// A stream is back after it was away: the lists that listen may have
    /// missed something, and ask now.
    pub(crate) fn lists_missed(&mut self, id: ScreenId, url: &str) {
        for list in self.lists_listening(id, url, None) {
            self.ask_changes(id, &list);
        }
    }

    // The rows before a list's first.

    /// Asks for the rows before the first of a list.
    pub(crate) fn rows_before(&mut self, id: ScreenId, list: &str) {
        let (Some(view), Some(inner)) = (self.view.screens.get_mut(&id), self.inner.screens.get_mut(&id)) else {
            return;
        };
        // One fetch at a time.
        if inner.before.contains_key(list) {
            return;
        }
        let Some(before) = view.screen().and_then(|screen| of_list(screen, list, |list| list.before.clone())).flatten() else {
            return;
        };
        let Target::Inside(url) = self.inner.origin.resolve(&before, &view.url) else {
            return;
        };
        inner.before.insert(list.to_owned(), before.clone());
        let state = view.lists.entry(list.to_owned()).or_default();
        state.loading_before = true;
        state.failed_before = false;
        let purpose = Pending::Before {
            screen: id,
            visit: inner.visit,
            id: list.to_owned(),
            was: before,
        };
        self.fetch_part(purpose, &url, list, MORE_LIMIT);
        self.list_changed(id, list);
    }

    pub(crate) fn before_answered(&mut self, id: ScreenId, visit: u64, list: &str, was: &str, answer: Answer) {
        let (Some(view), Some(inner)) = (self.view.screens.get_mut(&id), self.inner.screens.get_mut(&id)) else {
            return;
        };
        inner.before.remove(list);
        let now = |view: &ScreenView| view.screen().and_then(|screen| of_list(screen, list, |list| list.before.clone())).flatten();
        // The list changed while this was on its way: the answer is for a list that is gone.
        let wanted = inner.visit == visit && now(view).as_deref() == Some(was);
        let failed = wanted && !matches!(answer, Answer::Tree { .. });
        if let Some(state) = view.lists.get_mut(list) {
            state.loading_before = false;
            state.failed_before = failed;
        }
        self.list_changed(id, list);
        if !wanted {
            return;
        }
        if let Answer::Tree { status, mut read, shell, .. } = answer {
            if self.note(&mut read, shell) {
                return;
            }
            if let Some(way_in) = Self::way_in(status, &read.envelope.ops) {
                return self.session_ended(id, &way_in);
            }
            self.apply_ops(id, std::mem::take(&mut read.envelope.ops), false);
        } else {
            return;
        }
        // An answer that left `before` as it was has nothing before it.
        let same = self.view.screens.get(&id).and_then(now).as_deref() == Some(was);
        if same {
            if let Some(Node::List(found)) = self.view.screens.get_mut(&id).and_then(ScreenView::screen_mut).and_then(|screen| find_in_mut(screen, &|node| is_list(node, list))) {
                found.before = None;
            }
            self.list_changed(id, list);
        }
    }

    // Tables, row by row.

    /// Adds records to the table `target`: at its end, with where the ones
    /// after them are; or at its start.
    pub(crate) fn table_rows(&mut self, id: ScreenId, target: &str, mut rows: Vec<TableRow>, at: Edge, more: Option<String>) {
        let Some(screen) = self.view.screens.get(&id).and_then(ScreenView::screen) else {
            return;
        };
        let Some(stands) = depth_of(screen, &|node| pages(node, target)) else {
            return;
        };
        // A table may stand in the region that goes by this name.
        let deepest = rows.iter().flat_map(TableRow::nodes).map(depth).max().unwrap_or(0);
        if stands + 1 + deepest > MAX_DEPTH {
            return self.too_deep(target);
        }
        if at != Edge::Start {
            let page = Table {
                rows,
                more,
                ..Table::default()
            };
            return self.add_rows(id, target, page);
        }
        let (Some(view), Some(inner)) = (self.view.screens.get_mut(&id), self.inner.screens.get_mut(&id)) else {
            return;
        };
        let Content::Native { screen } = &mut view.content else {
            return;
        };
        let Some(table) = find_in_mut(screen, &|node| pages(node, target)).and_then(table_of_mut) else {
            return;
        };
        // A record is never in a table twice.
        rows.retain(|row| row.id.as_ref().is_none_or(|id| !table.rows.iter().any(|held| held.id.as_ref() == Some(id))));
        let mut names = Names {
            forms: &mut inner.form_ids,
            fields: &mut inner.field_ids,
            server: &mut inner.server_ids,
        };
        for row in &mut rows {
            let scope = row.id.clone();
            let scope = scope.as_deref().or(Some(target));
            names.name(&mut row.cells, scope);
            names.name(&mut row.actions, scope);
            names.name(&mut row.detail, scope);
        }
        table.rows.splice(0..0, rows);
        if more.is_some() {
            table.more = more;
        }
        self.reconcile(id, Some(&BTreeSet::new()));
        self.touch_screen(id);
    }

    /// Puts `row` in place of the table row `target`. Whether the person
    /// chose the row stays as it was.
    pub(crate) fn table_row(&mut self, id: ScreenId, target: &str, mut row: TableRow) {
        let (Some(view), Some(inner)) = (self.view.screens.get_mut(&id), self.inner.screens.get_mut(&id)) else {
            return;
        };
        let Content::Native { screen } = &mut view.content else {
            return;
        };
        let Some(stands) = depth_of(screen, &|node| has_row(node, target)) else {
            return;
        };
        if stands + row.nodes().map(depth).max().unwrap_or(0) > MAX_DEPTH {
            return self.too_deep(target);
        }
        let Some(old) = screen.actions.iter_mut().chain(std::iter::once(&mut screen.body)).find_map(|root| row_mut(root, target)) else {
            return;
        };
        {
            let survey = Survey::of(old.nodes());
            for field in survey.forms.iter().flat_map(|form| &form.fields).chain(&survey.loose) {
                if let Some(field) = &field.id {
                    inner.field_ids.remove(field);
                }
            }
            for form in &survey.forms {
                inner.form_ids.remove(&form.handle);
            }
        }
        let mut names = Names {
            forms: &mut inner.form_ids,
            fields: &mut inner.field_ids,
            server: &mut inner.server_ids,
        };
        let scope = row.id.clone();
        let scope = scope.as_deref().or(Some(target));
        names.name(&mut row.cells, scope);
        names.name(&mut row.actions, scope);
        names.name(&mut row.detail, scope);
        let fresh: BTreeSet<String> = Survey::of(row.nodes()).forms.iter().map(|form| form.handle.clone()).collect();
        row.selected = old.selected;
        *old = row;
        self.reconcile(id, Some(&fresh));
        self.touch_screen(id);
    }

    // The rows a person chose.

    /// Ticks or clears a row of a table whose rows can be chosen, or every
    /// row it holds when `row` is none.
    pub(crate) fn choose(&mut self, id: ScreenId, table: &str, row: Option<&str>, on: bool) {
        let Some(screen) = self.view.screens.get_mut(&id).and_then(ScreenView::screen_mut) else {
            return;
        };
        let Some(found) = find_in_mut(screen, &|node| pages(node, table)).and_then(table_of_mut) else {
            return;
        };
        if found.select.is_none() {
            return;
        }
        let mut changed = false;
        for held in &mut found.rows {
            // A row with no id has nothing to send, so it can't be chosen.
            if held.id.is_none() || row.is_some_and(|row| held.id.as_deref() != Some(row)) {
                continue;
            }
            changed |= std::mem::replace(&mut held.selected, on) != on;
        }
        if changed {
            self.list_changed(id, table);
        }
    }

    /// What the rows chosen on a screen send with the form `handle`: the id
    /// of each, under the name its table gives, for every table whose
    /// `select` names the form, or names none and stands in it.
    pub(crate) fn chosen(&self, id: ScreenId, handle: &str) -> Vec<Part> {
        let Some(screen) = self.view.screens.get(&id).and_then(ScreenView::screen) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut take = |table: &Table| {
            if let Some(select) = &table.select {
                out.extend(chosen_in(table).into_iter().map(|row| text(&select.name, row)));
            }
        };
        let mut named = Vec::new();
        for root in crate::tree::roots(screen) {
            walk(root, &mut |node| {
                if let Node::Table(table) = node
                    && table.select.as_ref().is_some_and(|select| select.form.is_some())
                {
                    named.push(table);
                }
            });
        }
        for table in named {
            let form = table.select.as_ref().and_then(|select| select.form.as_deref());
            if form.and_then(|form| self.form_named(id, form)).as_deref() == Some(handle) {
                take(table);
            }
        }
        if let Some(form) = form_of(screen, handle) {
            for child in &form.children {
                walk(child, &mut |node| {
                    if let Node::Table(table) = node
                        && table.select.as_ref().is_some_and(|select| select.form.is_none())
                    {
                        take(table);
                    }
                });
            }
        }
        out
    }

    // Rows moved by dragging.

    /// The person moved the row `item` from the list `from` into the list
    /// `to`, before the row `before` or at its end.
    pub(crate) fn moved(&mut self, id: ScreenId, item: &str, from: &str, to: &str, before: Option<&str>) {
        let Some(screen) = self.view.screens.get(&id).and_then(ScreenView::screen) else {
            return;
        };
        // Whatever comes of it, the client moved the row on its own and is
        // told to draw the lists again as they are.
        self.changed.insert(crate::effect::Change::Screen { id });
        let found = |name: &str| match find_in(screen, &|node| is_list(node, name)) {
            Some(Node::List(list)) => Some(list),
            _ => None,
        };
        let (Some(source), Some(target)) = (found(from), found(to)) else {
            return;
        };
        let (Some(leaves), Some(takes)) = (&source.reorder, &target.reorder) else {
            return;
        };
        let Some(at) = source.children.iter().position(|child| child.id() == Some(item)) else {
            return;
        };
        // Within one list, or between the lists of one group; and never into a full one.
        let together = from == to || (leaves.group.is_some() && leaves.group == takes.group);
        let full = from != to && takes.limit.is_some_and(|limit| target.children.len() >= usize::try_from(limit).unwrap_or(usize::MAX));
        let deep = depth_of(screen, &|node| is_list(node, to)).unwrap_or(1) + depth(&source.children[at]) > MAX_DEPTH;
        if !together || full || deep || before == Some(item) {
            return;
        }
        let (href, fields) = (takes.href.clone(), takes.fields.clone());
        let Target::Inside(url) = self.resolve(&href, id) else {
            return;
        };
        if !self.may_send(id) {
            return;
        }
        let Some(screen) = self.view.screens.get_mut(&id).and_then(ScreenView::screen_mut) else {
            return;
        };
        let Some(Node::List(source)) = find_in_mut(screen, &|node| is_list(node, from)) else {
            return;
        };
        let node = source.children.remove(at);
        let Some(Node::List(target)) = find_in_mut(screen, &|node| is_list(node, to)) else {
            return;
        };
        let stands = before.and_then(|before| target.children.iter().position(|child| child.id() == Some(before)));
        let position = stands.unwrap_or(target.children.len());
        target.children.insert(position, node);
        let mut sent = vec![
            text("item", item),
            text("from", from),
            text("to", to),
            text("before", stands.and(before).unwrap_or("")),
            text("position", &position.to_string()),
        ];
        sent.extend(fields.iter().map(|pair| text(&pair.name, &pair.value)));
        let visit = self.inner.screens.get(&id).map_or(0, |inner| inner.visit);
        let purpose = Pending::Move {
            screen: id,
            visit,
            item: item.to_owned(),
            from: from.to_owned(),
            at,
        };
        self.fetch(purpose, Method::Post, &url, Some(Body::Form { text: encode(&sent) }), SEND_LIMIT);
    }

    /// Puts a row back where it was before a move the server did not take.
    fn put_back(&mut self, id: ScreenId, item: &str, from: &str, at: usize) {
        let Some(screen) = self.view.screens.get_mut(&id).and_then(ScreenView::screen_mut) else {
            return;
        };
        // The list it came from is gone: it stays where it is.
        if find_in(screen, &|node| is_list(node, from)).is_none() {
            return;
        }
        let holds = |node: &Node| matches!(node, Node::List(list) if list.children.iter().any(|child| child.id() == Some(item)));
        let Some(Node::List(now)) = find_in_mut(screen, &holds) else {
            return;
        };
        let Some(place) = now.children.iter().position(|child| child.id() == Some(item)) else {
            return;
        };
        let node = now.children.remove(place);
        if let Some(Node::List(list)) = find_in_mut(screen, &|node| is_list(node, from)) {
            let at = at.min(list.children.len());
            list.children.insert(at, node);
        }
        self.touch_screen(id);
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn move_answered(&mut self, id: ScreenId, visit: u64, item: &str, from: &str, at: usize, answer: Answer) {
        // A screen that was asked for again since shows the server's own order.
        let same = self.inner.screens.get(&id).is_some_and(|inner| inner.visit == visit);
        match answer {
            Answer::Failed(failure) => {
                if same {
                    self.put_back(id, item, from, at);
                }
                self.failed(failure, Some(id));
            }
            Answer::Empty => self.wrote(id),
            Answer::Left(_) | Answer::Html { .. } => {
                // Not an answer a move has: the page says where its rows are.
                self.wrote(id);
                self.fetch_page(id, false);
            }
            Answer::Tree { status, mut read, shell, .. } => {
                if self.note(&mut read, shell) {
                    return;
                }
                if let Some(way_in) = Self::way_in(status, &read.envelope.ops) {
                    if same {
                        self.put_back(id, item, from, at);
                    }
                    return self.session_ended(id, &way_in);
                }
                let ops = std::mem::take(&mut read.envelope.ops);
                let taken = status < 400;
                if taken {
                    self.wrote(id);
                } else {
                    // The server said no: the row goes back, and what it said why is said.
                    if same {
                        self.put_back(id, item, from, at);
                    }
                    if ops.is_empty() {
                        self.notice(Notice::Failed, Some(id));
                    }
                }
                if self.view.holds(id) {
                    self.apply_ops(id, ops, taken);
                }
            }
        }
    }
}
