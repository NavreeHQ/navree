//! Parts of a screen that have an address of their own: regions that fetch
//! themselves, lists and tables that load more by cursor, and the streams
//! that tell a region to fetch itself again.
//!
//! - A region has one fetch of its own that counts: a newer one overtakes an
//!   older, whose answer is dropped. An answer asked for before something was
//!   written may be from before the write: it is not drawn, and is asked for again.
//! - The answer for a region is a screen. The region of the same id in it
//!   takes the region's place; without one, the screen's body becomes what the
//!   region holds. What can't be drawn sends the whole page to the web view:
//!   a page is never drawn with a hole in it.
//! - A list asks for its next rows one fetch at a time. An answer for a list
//!   that has since changed is dropped; one that leaves `more` as it was ends
//!   the list, or asking would never end.
//! - An event of a stream is a nudge and nothing more: its payload is never
//!   read. A burst of events costs one fetch; events that come while a fetch
//!   is out cost one more after it lands. A stream that fails comes back
//!   after 1 s, 2 s, 4 s … 30 s, and its regions then fetch once. Only the
//!   screen in front holds streams, and a hidden app holds none.

use navree_native::node::{Method, Node, Region, Table};
use navree_native::Screen;
use serde_json::{Value, json};

use crate::decode::Answer;
use crate::effect::{Change, Effect};
use crate::origin::{Target, path_of};
use crate::state::{Content, ListView, Notice, Pending, ScreenId, ScreenView, Stream, TimerFor};
use crate::tree::{Names, Survey, find_in, find_in_mut, has_id, rewrite_in};
use crate::Core;

/// How long a region may take, in milliseconds.
const REGION_LIMIT: u32 = 15_000;
/// How long the next rows of a list may take.
const MORE_LIMIT: u32 = 10_000;
/// Quiet time before a fetch, so a burst of events costs one.
const QUIET: u32 = 300;
/// The first and the longest wait before a stream that failed is tried again.
const FIRST: u32 = 1_000;
const LONGEST: u32 = 30_000;
/// What a stream's refusal means for good: nothing to send, or not allowed.
const FINAL: [u16; 3] = [204, 401, 403];

/// The table a paging id means: the table itself, or the one in the region of that id.
pub(crate) fn table_of(node: &Node) -> Option<&Table> {
    match node {
        Node::Table(table) => Some(table),
        Node::Region(region) => region.children.iter().find_map(|child| match child {
            Node::Table(table) => Some(table),
            _ => None,
        }),
        _ => None,
    }
}

fn table_of_mut(node: &mut Node) -> Option<&mut Table> {
    match node {
        Node::Table(table) => Some(table),
        Node::Region(region) => region.children.iter_mut().find_map(|child| match child {
            Node::Table(table) => Some(table),
            _ => None,
        }),
        _ => None,
    }
}

fn pages(node: &Node, id: &str) -> bool {
    has_id(node, id) && matches!(node, Node::List(_) | Node::Table(_) | Node::Region(_))
}

fn more_of(screen: &Screen, id: &str) -> Option<String> {
    match find_in(screen, &|node| pages(node, id))? {
        Node::List(list) => list.more.clone(),
        other => table_of(other)?.more.clone(),
    }
}

/// Some rows of a list or table, with the state of the forms that stand in
/// them and what the list says of itself now.
pub(crate) fn rows(view: &ScreenView, id: &str, from: usize, count: usize) -> Option<Value> {
    let screen = view.screen()?;
    let state = view.lists.get(id).copied().unwrap_or_default();
    let forms = |nodes: &mut dyn Iterator<Item = &Node>| -> Value {
        let survey = Survey::of(nodes);
        let found = survey.forms.iter().filter_map(|form| Some((form.handle.clone(), json!(view.forms.get(&form.handle)?))));
        Value::Object(found.collect())
    };
    match find_in(screen, &|node| pages(node, id))? {
        Node::List(list) => {
            let nodes: Vec<&Node> = list.children.iter().skip(from).take(count).collect();
            Some(json!({
                "nodes": nodes,
                "forms": forms(&mut nodes.iter().copied()),
                "more": list.more,
                "loading": state.loading,
                "failed": state.failed,
            }))
        }
        other => {
            let table = table_of(other)?;
            let rows: Vec<_> = table.rows.iter().skip(from).take(count).collect();
            Some(json!({
                "rows": rows,
                "forms": forms(&mut rows.iter().flat_map(|row| &row.cells)),
                "more": table.more,
                "summary": table.summary,
                "loading": state.loading,
                "failed": state.failed,
            }))
        }
    }
}

impl Core {
    /// Fetches a region: from its own address, or from `go`, another address
    /// of itself (a sort, a page, a query).
    pub(crate) fn fetch_region(&mut self, id: ScreenId, region: &str, go: Option<String>) {
        let (Some(view), Some(inner)) = (self.view.screens.get_mut(&id), self.inner.screens.get_mut(&id)) else {
            return;
        };
        let Some(meta) = inner.regions.get(region) else {
            return;
        };
        let url = go.clone().unwrap_or_else(|| meta.src.clone());
        let turn = inner.turns.entry(region.to_owned()).or_default();
        *turn += 1;
        let (turn, visit) = (*turn, inner.visit);
        let state = view.regions.entry(region.to_owned()).or_default();
        state.loading = true;
        state.failure = None;
        // A fetch of the region that began before this one is out of date.
        self.drop_pending(|purpose| matches!(purpose, Pending::Region { screen, region: other, .. } if *screen == id && other == region));
        let purpose = Pending::Region {
            screen: id,
            visit,
            region: region.to_owned(),
            turn,
            epoch: self.inner.epoch,
            go: go.is_some(),
            url: url.clone(),
        };
        self.fetch(purpose, Method::Get, &url, None, REGION_LIMIT);
        self.touch_screen(id);
    }

    /// A table goes to another address of itself.
    pub(crate) fn region_go(&mut self, id: ScreenId, region: &str, url: String) {
        self.fetch_region(id, region, Some(url));
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn region_answered(&mut self, id: ScreenId, visit: u64, region: &str, turn: u64, epoch: u64, go: bool, url: &str, answer: Answer) {
        let (Some(view), Some(inner)) = (self.view.screens.get_mut(&id), self.inner.screens.get_mut(&id)) else {
            return;
        };
        // The screen was asked for again, or a newer fetch of the region began.
        if inner.visit != visit || inner.turns.get(region) != Some(&turn) {
            return;
        }
        if let Some(state) = view.regions.get_mut(region) {
            state.loading = false;
        }
        self.touch_screen(id);
        // Something was written since this was asked: the answer may be from
        // before the write. It is not drawn, and the region is asked again.
        if epoch != self.inner.epoch {
            return self.fetch_region(id, region, go.then(|| url.to_owned()));
        }
        self.live_landed(id, region);
        let fail = |core: &mut Core, failure| {
            if let Some(state) = core.view.screens.get_mut(&id).and_then(|view| view.regions.get_mut(region)) {
                state.failure = Some(failure);
            }
        };
        match answer {
            Answer::Failed(failure) => fail(self, failure),
            Answer::Left(_) | Answer::Empty => {}
            Answer::Html { .. } => self.whole_page_to_web(id, vec!["html".to_owned()]),
            Answer::Tree { url: from, status, mut read, body } => {
                if self.note(&mut read, Some(id)) {
                    return;
                }
                // The session ended: the answer is the sign-in, not the region.
                if let Some(route) = self.covers(url, &from) {
                    let shown = self.shown(&from, status, &mut read, body);
                    return self.cover(id, route, &from, shown);
                }
                if read.envelope.web.is_some() || !read.missing.is_empty() {
                    let why = read.envelope.web.take().map_or(read.missing.clone(), |web| web.why);
                    return self.whole_page_to_web(id, why);
                }
                if let Some(screen) = read.envelope.screen.take() {
                    self.fill_region(id, region, screen, go.then_some(url));
                }
                self.apply_ops(id, std::mem::take(&mut read.envelope.ops), false);
            }
        }
    }

    /// A part of the page can't be drawn: the page is shown whole in the web view.
    fn whole_page_to_web(&mut self, id: ScreenId, why: Vec<String>) {
        if let Some(url) = self.view.screens.get(&id).map(|view| view.url.clone()) {
            self.show_web(id, &url, &url, None, why);
        }
    }

    /// Puts what a region's fetch answered in the region's place, as a morph:
    /// what was typed in it and what is open in it stay.
    fn fill_region(&mut self, id: ScreenId, region: &str, mut answer: Screen, went: Option<&str>) {
        let is = |node: &Node| matches!(node, Node::Region(other) if other.id == region);
        let Some(Node::Region(old)) = self.view.screens.get(&id).and_then(ScreenView::screen).and_then(|screen| find_in(screen, &is)) else {
            return;
        };
        // The region of the same id in the answer, when the answer is the whole
        // page; otherwise the answer's body is what the region holds.
        let mut taken = None;
        rewrite_in(&mut answer, &is, &mut |found| {
            taken = Some(found);
            None
        });
        let mut new = match taken {
            Some(Node::Region(new)) => new,
            _ => Region {
                id: old.id.clone(),
                src: old.src.clone(),
                lazy: false,
                live: old.live.clone(),
                children: vec![answer.body],
            },
        };
        new.lazy = false;
        if let Some(went) = went {
            new.src = went.to_owned();
        }
        self.apply_ops(
            id,
            vec![navree_native::Op::Morph {
                target: region.to_owned(),
                node: Node::Region(new),
            }],
            false,
        );
        // A table that is its page's content takes the page's address with it.
        if let Some(went) = went {
            if let Some(view) = self.view.screens.get_mut(&id) {
                if path_of(&view.url) == path_of(went) && view.url != went {
                    view.url = went.to_owned();
                }
            }
        }
    }

    // Lists and tables that load more.

    /// Asks for the next rows of a list or table.
    pub(crate) fn more(&mut self, id: ScreenId, list: &str) {
        let (Some(view), Some(inner)) = (self.view.screens.get_mut(&id), self.inner.screens.get_mut(&id)) else {
            return;
        };
        // One fetch at a time.
        if inner.more.contains_key(list) {
            return;
        }
        let Some(more) = view.screen().and_then(|screen| more_of(screen, list)) else {
            return;
        };
        let Target::Inside(url) = self.inner.origin.resolve(&more, &view.url) else {
            return;
        };
        inner.more.insert(list.to_owned(), more.clone());
        view.lists.insert(list.to_owned(), ListView { loading: true, failed: false });
        let purpose = Pending::More {
            screen: id,
            visit: inner.visit,
            id: list.to_owned(),
            was: more,
        };
        self.fetch(purpose, Method::Get, &url, None, MORE_LIMIT);
        self.list_changed(id, list);
    }

    /// Says that a list's own state changed, and no row of it.
    fn list_changed(&mut self, id: ScreenId, list: &str) {
        let held = self.view.screens.get(&id).and_then(ScreenView::screen).and_then(|screen| find_in(screen, &|node| pages(node, list))).map_or(0, |node| match node {
            Node::List(list) => list.children.len(),
            other => table_of(other).map_or(0, |table| table.rows.len()),
        });
        self.touch(Change::Rows {
            screen: id,
            id: list.to_owned(),
            from: held,
            count: 0,
        });
    }

    pub(crate) fn more_answered(&mut self, id: ScreenId, visit: u64, list: &str, was: &str, answer: Answer) {
        let (Some(view), Some(inner)) = (self.view.screens.get_mut(&id), self.inner.screens.get_mut(&id)) else {
            return;
        };
        inner.more.remove(list);
        // The list changed while this was on its way: the answer is for a list that is gone.
        let unchanged = view.screen().and_then(|screen| more_of(screen, list)).as_deref() == Some(was);
        let wanted = inner.visit == visit && unchanged;
        let failed = wanted && !matches!(answer, Answer::Tree { .. });
        if let Some(state) = view.lists.get_mut(list) {
            state.loading = false;
            state.failed = failed;
        }
        self.list_changed(id, list);
        if !wanted {
            return;
        }
        match answer {
            Answer::Failed(_) | Answer::Html { .. } | Answer::Left(_) | Answer::Empty => return,
            Answer::Tree { mut read, .. } => {
                if self.note(&mut read, Some(id)) {
                    return;
                }
                let ops = std::mem::take(&mut read.envelope.ops);
                if !ops.is_empty() {
                    self.apply_ops(id, ops, false);
                } else if let Some(mut page) = read.envelope.screen.take().filter(|_| read.missing.is_empty()) {
                    // The next rows came as a whole screen: the list or table of the same name in it is the page.
                    let mut taken = None;
                    rewrite_in(&mut page, &|node| pages(node, list), &mut |found| {
                        taken = Some(found);
                        None
                    });
                    match taken {
                        Some(Node::List(page)) => self.add(id, list, page.children, true, Some(page.more)),
                        Some(mut other) => {
                            if let Some(table) = table_of_mut(&mut other) {
                                let table = std::mem::take(table);
                                self.add_rows(id, list, table);
                            }
                        }
                        None => {}
                    }
                }
            }
        }
        // An answer that left `more` as it was has no page after it.
        let same = self.view.screens.get(&id).and_then(ScreenView::screen).and_then(|screen| more_of(screen, list)).as_deref() == Some(was);
        if same {
            if let Some(node) = self.view.screens.get_mut(&id).and_then(ScreenView::screen_mut).and_then(|screen| find_in_mut(screen, &|node| pages(node, list))) {
                match node {
                    Node::List(list) => list.more = None,
                    other => {
                        if let Some(table) = table_of_mut(other) {
                            table.more = None;
                        }
                    }
                }
            }
            self.list_changed(id, list);
        }
    }

    /// Adds the rows of the next page to a table, and takes the page's `more`.
    fn add_rows(&mut self, id: ScreenId, list: &str, mut page: Table) {
        let (Some(view), Some(inner)) = (self.view.screens.get_mut(&id), self.inner.screens.get_mut(&id)) else {
            return;
        };
        let Content::Native { screen } = &mut view.content else {
            return;
        };
        let Some(table) = find_in_mut(screen, &|node| pages(node, list)).and_then(table_of_mut) else {
            return;
        };
        // A record is never in a table twice.
        page.rows.retain(|row| row.id.as_ref().is_none_or(|id| !table.rows.iter().any(|held| held.id.as_ref() == Some(id))));
        let mut names = Names {
            forms: &mut inner.form_ids,
            fields: &mut inner.field_ids,
            server: &mut inner.server_ids,
        };
        for row in &mut page.rows {
            let scope = row.id.clone();
            names.name(&mut row.cells, scope.as_deref().or(Some(list)));
        }
        let from = table.rows.len();
        let count = page.rows.len();
        table.rows.append(&mut page.rows);
        table.more = page.more;
        if page.summary.is_some() {
            table.summary = page.summary;
        }
        if self.absorb(id, list, from) {
            self.touch(Change::Rows {
                screen: id,
                id: list.to_owned(),
                from,
                count,
            });
        } else {
            self.reconcile(id, Some(&std::collections::BTreeSet::new()));
            self.touch_screen(id);
        }
        if count > 0 {
            self.effects.push(Effect::Announce {
                text: None,
                notice: Some(Notice::Loaded),
                count: Some(u32::try_from(count).unwrap_or(u32::MAX)),
            });
        }
    }

    // Streams.

    /// The streams the screen in front listens to, each with the regions that listen.
    fn wanted(&self) -> Vec<String> {
        if self.inner.background {
            return Vec::new();
        }
        let Some(inner) = self.front().and_then(|front| self.inner.screens.get(&front)) else {
            return Vec::new();
        };
        let mut urls: Vec<String> = inner.regions.values().filter_map(|meta| Some(meta.live.as_ref()?.0.clone())).collect();
        urls.sort();
        urls.dedup();
        urls
    }

    /// Opens the streams the screen in front needs and closes the others.
    pub(crate) fn sync_streams(&mut self) {
        let wanted = self.wanted();
        let gone: Vec<Stream> = self.inner.streams.iter().filter(|stream| !wanted.contains(&stream.url)).cloned().collect();
        for stream in gone {
            if let Some(timer) = stream.reopen {
                self.cancel_timer(timer);
            }
            self.effects.push(Effect::StreamClose { stream: stream.id });
            self.inner.streams.retain(|other| other.id != stream.id);
        }
        let missed = self.front().and_then(|front| self.inner.screens.get(&front)).is_some_and(|inner| inner.missed);
        for url in wanted {
            if self.inner.streams.iter().any(|stream| stream.url == url) {
                continue;
            }
            self.inner.next.stream += 1;
            let stream = Stream {
                id: self.inner.next.stream,
                url,
                open: false,
                failures: 0,
                reopen: None,
                stopped: false,
                // A screen that was away may have missed something: it fetches once when its stream is back.
                missed,
            };
            self.open_stream(&stream);
            self.inner.streams.push(stream);
        }
    }

    fn open_stream(&mut self, stream: &Stream) {
        let mut headers = vec![("accept".to_owned(), "text/event-stream".to_owned())];
        headers.push((navree_native::NATIVE_HEADER.to_owned(), navree_native::MAJOR.to_string()));
        self.effects.push(Effect::StreamOpen {
            stream: stream.id,
            url: self.inner.origin.absolute(&stream.url),
            headers,
        });
    }

    /// The regions of the screen in front that listen to a stream: for an
    /// event of this name, or for any when `name` is none.
    fn listeners(&self, url: &str, name: Option<Option<&str>>) -> Vec<String> {
        let Some(inner) = self.front().and_then(|front| self.inner.screens.get(&front)) else {
            return Vec::new();
        };
        inner
            .regions
            .iter()
            .filter(|(_, meta)| {
                meta.live.as_ref().is_some_and(|(src, event)| {
                    src == url
                        && match name {
                            None => true,
                            // A region that names no event hears every message.
                            Some(name) => event.as_deref().is_none_or(|event| Some(event) == name),
                        }
                })
            })
            .map(|(id, _)| id.clone())
            .collect()
    }

    pub(crate) fn stream_open(&mut self, id: u64) {
        let Some(stream) = self.inner.streams.iter_mut().find(|stream| stream.id == id) else {
            return;
        };
        stream.open = true;
        let missed = std::mem::replace(&mut stream.missed, false);
        let url = stream.url.clone();
        let Some(front) = self.front() else {
            return;
        };
        if let Some(inner) = self.inner.screens.get_mut(&front) {
            inner.missed = false;
        }
        if missed {
            for region in self.listeners(&url, None) {
                self.live_fire(front, &region);
            }
        }
    }

    pub(crate) fn stream_spoke(&mut self, id: u64, name: Option<&str>) {
        let Some(stream) = self.inner.streams.iter_mut().find(|stream| stream.id == id) else {
            return;
        };
        // A stream that speaks is a good one: its failures are forgiven.
        stream.failures = 0;
        let url = stream.url.clone();
        let Some(front) = self.front() else {
            return;
        };
        for region in self.listeners(&url, Some(name)) {
            let Some(sub) = self.inner.screens.get_mut(&front).map(|inner| inner.live.entry(region.clone()).or_default()) else {
                continue;
            };
            if sub.busy {
                sub.dirty = true;
            } else if sub.timer.is_none() {
                let timer = self.timer(QUIET, TimerFor::Live { screen: front, region: region.clone() });
                if let Some(sub) = self.inner.screens.get_mut(&front).and_then(|inner| inner.live.get_mut(&region)) {
                    sub.timer = Some(timer);
                }
            }
        }
    }

    pub(crate) fn stream_closed(&mut self, id: u64, status: Option<u16>) {
        let Some(stream) = self.inner.streams.iter_mut().find(|stream| stream.id == id) else {
            return;
        };
        stream.open = false;
        stream.missed = true;
        if status.is_some_and(|status| FINAL.contains(&status)) {
            stream.stopped = true;
            return;
        }
        stream.failures += 1;
        let wait = FIRST.saturating_mul(1 << (stream.failures - 1).min(5)).min(LONGEST);
        let timer = self.timer(wait, TimerFor::Reopen { stream: id });
        if let Some(stream) = self.inner.streams.iter_mut().find(|stream| stream.id == id) {
            stream.reopen = Some(timer);
        }
    }

    pub(crate) fn reopen(&mut self, id: u64) {
        let Some(stream) = self.inner.streams.iter_mut().find(|stream| stream.id == id) else {
            return;
        };
        stream.reopen = None;
        let stream = stream.clone();
        self.open_stream(&stream);
    }

    /// Fetches a live region now, or, if its last fetch is still out, once more when it lands.
    pub(crate) fn live_fire(&mut self, id: ScreenId, region: &str) {
        let Some(inner) = self.inner.screens.get_mut(&id) else {
            return;
        };
        let sub = inner.live.entry(region.to_owned()).or_default();
        sub.timer = None;
        if sub.busy {
            sub.dirty = true;
            return;
        }
        sub.busy = true;
        sub.dirty = false;
        self.fetch_region(id, region, None);
    }

    /// A live region's fetch landed: events that came meanwhile cost one more.
    fn live_landed(&mut self, id: ScreenId, region: &str) {
        let Some(sub) = self.inner.screens.get_mut(&id).and_then(|inner| inner.live.get_mut(region)) else {
            return;
        };
        sub.busy = false;
        if std::mem::take(&mut sub.dirty) && sub.timer.is_none() {
            let timer = self.timer(QUIET, TimerFor::Live { screen: id, region: region.to_owned() });
            if let Some(sub) = self.inner.screens.get_mut(&id).and_then(|inner| inner.live.get_mut(region)) {
                sub.timer = Some(timer);
            }
        }
    }
}
