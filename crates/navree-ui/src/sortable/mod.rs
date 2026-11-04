//! Sortable lists and boards. Items move within a list and between lists by
//! pointer, touch or keys, and every move is posted to the server, which has the
//! last word: the browser moves the item at once and puts it back if the answer
//! is a refusal.
//!
//! [`Sortable`] draws the lists. Each item carries a small form that posts its move,
//! which is what the `sortable` module fills in and sends after a drop, and what a
//! person with JavaScript off uses: a list to move it to and a position. [`Move`]
//! reads that post back, and [`Move::check`] holds it against the lists as the
//! server has them now: the item is where the browser saw it, the item it goes
//! before is still in that list, and the list has room and takes items.
//! [`Move::place`] does the same for lists kept by rank, reading only what it
//! needs, after locking the lists the move touches so that two moves at once run
//! one after the other. With the `orm` feature, `transaction` runs a move in a
//! transaction the toolkit begins, at READ COMMITTED with that lock taken first,
//! `lock_lists` is the lock, and `renew_ranks` gives a list fresh ranks in one
//! statement.

use std::borrow::Cow;

use crate::components::icon;
use crate::form::csrf_field;
use crate::html::Href;
use crate::icons;
use crate::node::{IntoNodes, Node, Uses, el, into_node};
use crate::op::Op;

mod moves;
#[cfg(feature = "orm")]
mod orm;
mod ranked;

pub use moves::{Landing, ListState, Move, MoveError, MoveRejected, Refusal};
#[cfg(feature = "orm")]
pub use orm::{lock_lists, renew_ranks, transaction};
pub use ranked::{Placement, RANK_GAP, RankedLists, Spot, rank_between};

/// How a sortable's lists sit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum SortLayout {
    /// One list under another: a checklist, a playlist, a set of steps. The default.
    #[default]
    Stack,
    /// Lists side by side as columns, the row scrolling sideways when it is wider
    /// than the page: a board.
    Board,
}

impl SortLayout {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Stack => "stack",
            Self::Board => "board",
        }
    }
}

/// The words a sortable says and shows. `{item}`, `{list}`, `{position}`,
/// `{total}`, `{limit}` and `{count}` are filled in.
#[derive(Clone, Debug)]
pub struct SortLabels {
    /// How to move with the keys, read with every handle.
    pub help: Cow<'static, str>,
    /// The handle's name.
    pub handle: Cow<'static, str>,
    /// Said when an item is picked up.
    pub picked: Cow<'static, str>,
    /// Said after each step of a move.
    pub moved: Cow<'static, str>,
    /// Said when an item is dropped.
    pub dropped: Cow<'static, str>,
    /// Said when a move is cancelled.
    pub cancelled: Cow<'static, str>,
    /// Said when a refused move puts the item back.
    pub returned: Cow<'static, str>,
    /// Said once the lists are drawn again from the server after a refusal or a
    /// failure: where the item really is.
    pub settled: Cow<'static, str>,
    /// A toast for a move that got no answer, or a failure, and didn't land: said
    /// once the lists are drawn again from the server and show it didn't.
    pub failed: Cow<'static, str>,
    /// A toast for a move that got no answer when the server can't be reached to
    /// ask whether it landed: the item goes back.
    pub offline: Cow<'static, str>,
    /// Said and shown when a list is full.
    pub full: Cow<'static, str>,
    /// Said when an item that can't move is picked up, before its reason.
    pub locked: Cow<'static, str>,
    /// A list's count, beside its title.
    pub count: Cow<'static, str>,
    /// A list's count when it has a limit.
    pub count_limit: Cow<'static, str>,
    /// The words for that count.
    pub count_limit_text: Cow<'static, str>,
    /// A list with nothing in it.
    pub empty: Cow<'static, str>,
    /// The disclosure that holds an item's move form (JavaScript off).
    pub move_summary: Cow<'static, str>,
    /// The move form's list field.
    pub move_to: Cow<'static, str>,
    /// The move form's position field.
    pub move_position: Cow<'static, str>,
    /// The move form's button.
    pub move_submit: Cow<'static, str>,
    /// A full list in the move form's choices.
    pub full_option: Cow<'static, str>,
    /// The button that loads a list's next items.
    pub more: Cow<'static, str>,
}

impl Default for SortLabels {
    fn default() -> Self {
        Self {
            help: Cow::Borrowed(
                "Press Space to pick it up, the arrow keys to move it, Space again to drop it, Escape to cancel.",
            ),
            handle: Cow::Borrowed("Move {item}"),
            picked: Cow::Borrowed("Picked up {item}. Position {position} of {total} in {list}."),
            moved: Cow::Borrowed("Position {position} of {total} in {list}."),
            dropped: Cow::Borrowed("Dropped {item} at position {position} of {total} in {list}."),
            cancelled: Cow::Borrowed(
                "Move cancelled. {item} is back at position {position} of {total} in {list}.",
            ),
            returned: Cow::Borrowed(
                "{item} wasn't moved. It is back at position {position} of {total} in {list}.",
            ),
            settled: Cow::Borrowed("{item} is at position {position} of {total} in {list}."),
            failed: Cow::Borrowed("That move wasn't saved. Try again."),
            offline: Cow::Borrowed("No connection. That move may not have been saved."),
            full: Cow::Borrowed("{list} holds at most {limit}."),
            locked: Cow::Borrowed("{item} can't be moved."),
            count: Cow::Borrowed("{count}"),
            count_limit: Cow::Borrowed("{count}/{limit}"),
            count_limit_text: Cow::Borrowed("{count} of at most {limit}"),
            empty: Cow::Borrowed("Nothing here yet"),
            move_summary: Cow::Borrowed("Move"),
            move_to: Cow::Borrowed("To"),
            move_position: Cow::Borrowed("Position"),
            move_submit: Cow::Borrowed("Move"),
            full_option: Cow::Borrowed("{list} (full)"),
            more: Cow::Borrowed("Load more"),
        }
    }
}

/// One item of a list.
#[derive(Clone, Debug)]
#[must_use]
pub struct SortItem {
    key: String,
    name: String,
    body: Vec<Node>,
    locked: Option<String>,
    position: Option<usize>,
}

impl SortItem {
    /// An item: `key` is what a move posts and must be unique in the sortable,
    /// `name` is what it is called when it is announced and in its move form. With
    /// no [`body`](Self::body), the name is what shows.
    pub fn new(key: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            name: name.into(),
            body: Vec::new(),
            locked: None,
            position: None,
        }
    }

    /// Sets what the item shows: a card, a row, any nodes. Links in it still work:
    /// a press that doesn't move is a click.
    pub fn body(mut self, body: impl IntoNodes) -> Self {
        self.body.clear();
        body.push_into(&mut self.body);
        self
    }

    /// Keeps it where it is, and says `reason` to someone who tries to move it. The
    /// server must refuse its moves too: this is only what the page shows.
    pub fn locked(mut self, reason: impl Into<String>) -> Self {
        self.locked = Some(reason.into());
        self
    }

    /// Its place in its whole list, 1 being first, when the page draws only some of
    /// the list (a filter): its move form starts at this number, which is what the
    /// server reads [`Move::position`] against. Without it, the place is counted
    /// among the items drawn, from [`SortList::from`].
    pub fn position(mut self, position: usize) -> Self {
        self.position = Some(position.max(1));
        self
    }
}

/// One list: a column of a board, or the only list of a checklist.
#[derive(Clone, Debug)]
#[must_use]
pub struct SortList {
    key: String,
    title: String,
    items: Vec<SortItem>,
    total: Option<usize>,
    start: usize,
    more: Option<More>,
    limit: Option<u32>,
    refuse: Option<String>,
    actions: Vec<Node>,
    footer: Vec<Node>,
    empty: Option<Cow<'static, str>>,
    hide_title: bool,
}

impl SortList {
    /// A list called `title`; `key` is what a move into it posts.
    pub fn new(key: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            title: title.into(),
            items: Vec::new(),
            total: None,
            start: 0,
            more: None,
            limit: None,
            refuse: None,
            actions: Vec::new(),
            footer: Vec::new(),
            empty: None,
            hide_title: false,
        }
    }

    /// Adds an item.
    pub fn item(mut self, item: SortItem) -> Self {
        self.items.push(item);
        self
    }

    /// Adds items, in order.
    pub fn items(mut self, items: impl IntoIterator<Item = SortItem>) -> Self {
        self.items.extend(items);
        self
    }

    /// How many items the list holds when not all of them are drawn: a filter
    /// shows some. The count beside the title, and the limit, go by this.
    pub fn total(mut self, total: usize) -> Self {
        self.total = Some(total);
        self
    }

    /// Says the items drawn start at this place in the list, 0 being first: a page
    /// after the first. Their move forms count positions from there.
    pub fn from(mut self, start: usize) -> Self {
        self.start = start;
        self
    }

    /// Ends the list with a button that loads its next items: a `GET` of `action`
    /// with `fields`, such as a cursor and a count. The `sortable` module presses it
    /// as it scrolls into sight; the answer is [`Sortable::page`]. With JavaScript
    /// off it is a plain form: answer it with the page, the list showing more.
    pub fn more<K, V>(
        mut self,
        action: impl Into<Href>,
        fields: impl IntoIterator<Item = (K, V)>,
    ) -> Self
    where
        K: Into<String>,
        V: Into<String>,
    {
        self.more = Some(More::Form {
            action: action.into(),
            fields: fields
                .into_iter()
                .map(|(name, value)| (name.into(), value.into()))
                .collect(),
        });
        self
    }

    /// Ends the list with a note in place of more items: the page holds as many as
    /// it will ("Showing the first 200 of 12,034").
    pub fn end_note(mut self, note: impl Into<String>) -> Self {
        self.more = Some(More::Note(note.into()));
        self
    }

    /// The most items the list holds: shown beside its title, and a drop into it
    /// is refused in the browser when it is full. Hold the server to it too, with
    /// [`ListState::limit`].
    pub fn limit(mut self, limit: u32) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Takes no item from another list, and says `reason` when one is brought to
    /// it: "Only a lead can move cards to Done." Items already in it still move
    /// within it. Hold the server to it too, with [`ListState::refuse`].
    pub fn refuse(mut self, reason: impl Into<String>) -> Self {
        self.refuse = Some(reason.into());
        self
    }

    /// Adds controls to the list's header, after its title and count.
    pub fn actions(mut self, actions: impl IntoNodes) -> Self {
        actions.push_into(&mut self.actions);
        self
    }

    /// Adds something under the items: a form to add one, a link to the rest.
    pub fn footer(mut self, footer: impl IntoNodes) -> Self {
        footer.push_into(&mut self.footer);
        self
    }

    /// What an empty list says.
    pub fn empty(mut self, text: impl Into<Cow<'static, str>>) -> Self {
        self.empty = Some(text.into());
        self
    }

    /// Keeps the title and the count for assistive technology only: for a list
    /// whose name the page already shows.
    pub fn hide_title(mut self) -> Self {
        self.hide_title = true;
        self
    }
}

/// What ends a list the page holds only the start of.
#[derive(Clone, Debug)]
enum More {
    /// A form that asks for the next items.
    Form {
        action: Href,
        fields: Vec<(String, String)>,
    },
    /// A note that no more will come into the page.
    Note(String),
}

impl SortList {
    /// How many items it holds, drawn or not.
    fn count(&self) -> usize {
        self.total.unwrap_or(0).max(self.items.len())
    }
}

/// Lists whose items move, within a list and between lists.
///
/// ```
/// use navree_ui::{Sortable, SortItem, SortList, render};
///
/// let board = Sortable::new("sprint", "Sprint board")
///     .board()
///     .post("/board/moves")
///     .list(SortList::new("ready", "Ready").item(SortItem::new("12", "Fix the sign-in redirect")))
///     .list(SortList::new("review", "Review").limit(2));
/// let html = render(&board.into());
/// assert!(html.contains(r#"data-ui-sortable"#));
/// assert!(html.contains(r#"name="to_list""#));
/// ```
///
/// Without [`post`](Self::post) the lists are drawn as they are, with no handles,
/// no forms and no script.
#[derive(Clone, Debug)]
#[must_use]
pub struct Sortable {
    id: String,
    label: String,
    action: Option<Href>,
    layout: SortLayout,
    lists: Vec<SortList>,
    labels: SortLabels,
    csrf: Option<String>,
    heading: u8,
    deadline: u32,
}

impl Sortable {
    /// Lists with the id `id`, named `label` for assistive technology. The lists'
    /// and items' ids are made from it.
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            action: None,
            layout: SortLayout::Stack,
            lists: Vec::new(),
            labels: SortLabels::default(),
            csrf: None,
            heading: 2,
            deadline: 15_000,
        }
    }

    /// Adds a list.
    pub fn list(mut self, list: SortList) -> Self {
        self.lists.push(list);
        self
    }

    /// Adds lists, in order.
    pub fn lists(mut self, lists: impl IntoIterator<Item = SortList>) -> Self {
        self.lists.extend(lists);
        self
    }

    /// Makes the items move, each move posted to `action` as a [`Move`]. Without
    /// it nothing moves.
    pub fn post(mut self, action: impl Into<Href>) -> Self {
        self.action = Some(action.into());
        self
    }

    /// Sets the lists side by side, as a board.
    pub fn board(self) -> Self {
        self.layout(SortLayout::Board)
    }

    /// Sets how the lists sit.
    pub fn layout(mut self, layout: SortLayout) -> Self {
        self.layout = layout;
        self
    }

    /// Sets the words.
    pub fn labels(mut self, labels: SortLabels) -> Self {
        self.labels = labels;
        self
    }

    /// Puts the session's token in every move form, for a server whose session
    /// checks a field when JavaScript is off.
    pub fn csrf(mut self, token: impl Into<String>) -> Self {
        self.csrf = Some(token.into());
        self
    }

    /// Sets the level of the lists' titles: `h2` by default, `h3` in a sheet whose
    /// title is the `h2`. Clamped to 2 through 6.
    pub fn heading(mut self, level: u8) -> Self {
        self.heading = level.clamp(2, 6);
        self
    }

    /// How long a move may wait for the server's answer before the item is put
    /// back, in milliseconds: 15 s by default, at least 1 s.
    pub fn deadline_ms(mut self, ms: u32) -> Self {
        self.deadline = ms.max(1_000);
        self
    }

    fn into_node(self) -> Node {
        let movable = self.action.is_some();
        let help_id = format!("{}-help", self.id);
        let heading = match self.heading {
            3 => "h3",
            4 => "h4",
            5 => "h5",
            6 => "h6",
            _ => "h2",
        };
        let labels = &self.labels;
        let choices = self.choices();
        let list_slugs = slugs(self.lists.iter().map(|list| list.key.as_str()));
        let item_slugs = slugs(
            self.lists
                .iter()
                .flat_map(|list| list.items.iter().map(|item| item.key.as_str())),
        );
        let mut item_slugs = item_slugs.into_iter();
        let mut lists = Vec::with_capacity(self.lists.len());
        for (list, list_slug) in self.lists.iter().zip(list_slugs) {
            let list_id = format!("{}-list-{list_slug}", self.id);
            let title_id = format!("{list_id}-title");
            let count = list.count();
            let mut items = Vec::with_capacity(list.items.len());
            for (at, item) in list.items.iter().enumerate() {
                let item_slug = item_slugs.next().unwrap_or_default();
                let item_id = format!("{}-item-{item_slug}", self.id);
                items.push(self.item_node(
                    item,
                    &item_id,
                    &list.key,
                    item.position.unwrap_or(list.start + at + 1),
                    &choices,
                    &help_id,
                ));
            }
            let count_text = list.limit.map_or_else(
                || labels.count.replace("{count}", &count.to_string()),
                |limit| {
                    labels
                        .count_limit
                        .replace("{count}", &count.to_string())
                        .replace("{limit}", &limit.to_string())
                },
            );
            let count_words = list.limit.map(|limit| {
                labels
                    .count_limit_text
                    .replace("{count}", &count.to_string())
                    .replace("{limit}", &limit.to_string())
            });
            let full = list.limit.is_some_and(|limit| count >= limit as usize);
            let over = list.limit.is_some_and(|limit| count > limit as usize);
            // A list whose name the page shows already keeps its header for assistive technology only.
            let header = el("div")
                .slot("sortable-list-header")
                .flag("data-ui-sr", list.hide_title)
                .children((
                    el(heading)
                        .slot("sortable-list-title")
                        .attr("id", title_id.clone())
                        .child(list.title.clone()),
                    el("span")
                        .slot("sortable-count")
                        .flag("data-full", full)
                        .flag("data-over", over)
                        .children((
                            el("span")
                                .attr_opt("aria-hidden", count_words.is_some().then_some("true"))
                                .flag("data-count", true)
                                .child(count_text),
                            count_words.map(|words| {
                                el("span")
                                    .flag("data-ui-sr", true)
                                    .flag("data-count-text", true)
                                    .child(words)
                            }),
                        )),
                    (!list.actions.is_empty()).then(|| {
                        el("div")
                            .slot("sortable-list-actions")
                            .child(list.actions.clone())
                    }),
                ));
            let empty = list.empty.clone().unwrap_or_else(|| labels.empty.clone());
            lists.push(
                el("section")
                    .slot("sortable-list")
                    .attr("id", list_id.clone())
                    .attr("data-list", list.key.clone())
                    .attr("aria-labelledby", title_id.clone())
                    .attr_opt("data-limit", list.limit.map(i64::from))
                    .attr("data-total", i64::try_from(count).unwrap_or(i64::MAX))
                    .flag("data-paged", list.more.is_some() || list.start > 0)
                    .attr_opt("data-refuse", list.refuse.clone())
                    .children((
                        header,
                        el("ol")
                            .slot("sortable-items")
                            .attr("id", format!("{list_id}-items"))
                            .attr("aria-labelledby", title_id)
                            .child(items),
                        el("p").slot("sortable-empty").child(empty),
                        list.more
                            .as_ref()
                            .map(|more| self.more_node(&list_id, more)),
                        (!list.footer.is_empty()).then(|| {
                            el("div")
                                .slot("sortable-list-footer")
                                .child(list.footer.clone())
                        }),
                    )),
            );
        }
        let words = movable.then(|| {
            serde_json::json!({
                "picked": labels.picked,
                "moved": labels.moved,
                "dropped": labels.dropped,
                "cancelled": labels.cancelled,
                "returned": labels.returned,
                "settled": labels.settled,
                "failed": labels.failed,
                "offline": labels.offline,
                "full": labels.full,
                "locked": labels.locked,
                "count": labels.count,
                "countLimit": labels.count_limit,
                "countText": labels.count_limit_text,
            })
            .to_string()
        });
        let mut root = el("div")
            .slot("sortable")
            .attr("id", self.id.clone())
            .attr("data-layout", self.layout.as_str())
            .attr("role", "group")
            .attr("aria-label", self.label.clone());
        if let Some(words) = words {
            root = root
                .flag("data-ui-sortable", true)
                .attr("data-l", words)
                .uses(Uses::CORE.with(Uses::SORTABLE));
        }
        root.children((
            movable.then(|| {
                el("p")
                    .slot("sortable-help")
                    .attr("id", help_id.clone())
                    .flag("hidden", true)
                    .child(labels.help.clone())
            }),
            movable.then(|| {
                el("div")
                    .slot("sortable-status")
                    .attr("id", format!("{}-status", self.id))
                    .attr("aria-live", "assertive")
                    .attr("aria-atomic", "true")
                    .flag("data-ui-sr", true)
            }),
            el("div").slot("sortable-lists").child(lists),
        ))
        .into()
    }

    /// The next items of the list `key`, as ops: they join the end of that list in
    /// the page, and its "Load more" button gives way to the list's own: another
    /// one for the page after, its note, or nothing when the list is all there.
    /// Build the sortable as for the whole page, with that list holding only the new
    /// items, [`from`](SortList::from) where they start.
    pub fn page(self, key: &str) -> Vec<Op> {
        let slugs_of_lists = slugs(self.lists.iter().map(|list| list.key.as_str()));
        let Some((list, list_slug)) = self
            .lists
            .iter()
            .zip(slugs_of_lists)
            .find(|(list, _)| list.key == key)
        else {
            return Vec::new();
        };
        let list_id = format!("{}-list-{list_slug}", self.id);
        let help_id = format!("{}-help", self.id);
        let choices = self.choices();
        let items: Vec<Node> = list
            .items
            .iter()
            .zip(slugs(list.items.iter().map(|item| item.key.as_str())))
            .enumerate()
            .map(|(at, (item, slug))| {
                let item_id = format!("{}-item-{slug}", self.id);
                self.item_node(
                    item,
                    &item_id,
                    &list.key,
                    item.position.unwrap_or(list.start + at + 1),
                    &choices,
                    &help_id,
                )
            })
            .collect();
        let more = list
            .more
            .as_ref()
            .map_or(Node::Empty, |more| self.more_node(&list_id, more));
        vec![
            Op::append(format!("{list_id}-items"), &Node::list(items)),
            Op::replace(format!("{list_id}-more"), &more),
        ]
    }

    /// The lists a move form offers: key, name, and whether it is closed to items
    /// from other lists.
    fn choices(&self) -> Vec<(String, String, bool)> {
        self.lists
            .iter()
            .map(|list| {
                let full = list
                    .limit
                    .is_some_and(|limit| list.count() >= limit as usize);
                let title = if full {
                    self.labels.full_option.replace("{list}", &list.title)
                } else {
                    list.title.clone()
                };
                (list.key.clone(), title, list.refuse.is_some() || full)
            })
            .collect()
    }

    /// The end of a list the page holds only the start of.
    fn more_node(&self, list_id: &str, more: &More) -> Node {
        let id = format!("{list_id}-more");
        match more {
            More::Form { action, fields } => el("form")
                .slot("sortable-more")
                .attr("id", id)
                .attr("method", "get")
                .attr("action", action.clone())
                .flag("data-ui-form", true)
                .flag("data-ui-autoload", true)
                .children((
                    fields
                        .iter()
                        .map(|(name, value)| {
                            Node::from(
                                el("input")
                                    .attr("type", "hidden")
                                    .attr("name", name.clone())
                                    .attr("value", value.clone()),
                            )
                        })
                        .collect::<Vec<_>>(),
                    el("button")
                        .slot("button")
                        .attr("type", "submit")
                        .attr("data-variant", "ghost")
                        .attr("data-size", "sm")
                        .child(self.labels.more.clone()),
                ))
                .into(),
            More::Note(note) => el("p")
                .slot("sortable-more")
                .attr("id", id)
                .child(note.clone())
                .into(),
        }
    }

    /// One item: its handle, what it shows, and the form that moves it.
    fn item_node(
        &self,
        item: &SortItem,
        item_id: &str,
        list_key: &str,
        position: usize,
        choices: &[(String, String, bool)],
        help_id: &str,
    ) -> Node {
        let labels = &self.labels;
        let body = if item.body.is_empty() {
            vec![Node::from(el("span").child(item.name.clone()))]
        } else {
            item.body.clone()
        };
        let handle = self.action.as_ref().map(|_| {
            el("button")
                .slot("sortable-handle")
                .attr("type", "button")
                .attr("aria-label", labels.handle.replace("{item}", &item.name))
                .attr("aria-describedby", help_id.to_owned())
                .attr_opt("aria-disabled", item.locked.as_ref().map(|_| "true"))
                .child(icon(icons::GRIP_VERTICAL))
        });
        let form = self
            .action
            .as_ref()
            .filter(|_| item.locked.is_none())
            .map(|action| {
                let to_id = format!("{item_id}-to");
                let position_id = format!("{item_id}-position");
                let options: Vec<Node> = choices
                    .iter()
                    .map(|(key, title, closed)| {
                        let here = key == list_key;
                        el("option")
                            .attr("value", key.clone())
                            .flag("selected", here)
                            .flag("disabled", *closed && !here)
                            .child(title.clone())
                            .into()
                    })
                    .collect();
                el("details").slot("sortable-move").children((
                    el("summary")
                        .slot("sortable-move-summary")
                        .child(labels.move_summary.clone()),
                    el("form")
                        .slot("sortable-form")
                        .attr("method", "post")
                        .attr("action", action.clone())
                        .flag("data-ui-form", true)
                        .flag("data-ui-move", true)
                        .attr("data-ui-deadline", i64::from(self.deadline))
                        .children((
                            self.csrf.clone().map(csrf_field),
                            hidden("item", item.key.clone()),
                            hidden("from_list", list_key.to_owned()),
                            hidden("before", String::new()),
                            hidden("after", String::new()),
                            el("label")
                                .slot("sortable-move-field")
                                .attr("for", to_id.clone())
                                .child(labels.move_to.clone()),
                            el("select")
                                .slot("native-select")
                                .attr("id", to_id)
                                .attr("name", "to_list")
                                .child(options),
                            el("label")
                                .slot("sortable-move-field")
                                .attr("for", position_id.clone())
                                .child(labels.move_position.clone()),
                            el("input")
                                .slot("input")
                                .attr("id", position_id)
                                .attr("type", "number")
                                .attr("name", "position")
                                .attr("min", 1_i64)
                                .attr("inputmode", "numeric")
                                .attr("value", i64::try_from(position).unwrap_or(i64::MAX)),
                            el("button")
                                .slot("button")
                                .attr("type", "submit")
                                .attr("data-variant", "outline")
                                .attr("data-size", "sm")
                                .child(labels.move_submit.clone()),
                        )),
                ))
            });
        el("li")
            .slot("sortable-item")
            .attr("id", item_id.to_owned())
            .attr("data-key", item.key.clone())
            .attr("data-name", item.name.clone())
            .attr_opt("data-locked", item.locked.clone())
            .children((handle, el("div").slot("sortable-body").child(body), form))
            .into()
    }
}

fn hidden(name: &'static str, value: String) -> Node {
    el("input")
        .attr("type", "hidden")
        .attr("name", name)
        .attr("value", value)
        .into()
}

/// The part of an id made from each key: the key itself when it is short and plain,
/// otherwise a hash of it, so the id is the same in every drawing of the lists
/// whatever order the items are in. A hash that repeats one already given gets a
/// number after it.
fn slugs<'a>(keys: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for key in keys {
        let plain = !key.is_empty()
            && key.len() <= 64
            && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
        let base = if plain {
            key.to_owned()
        } else {
            format!("h{}", crate::assets::content_hash(key.as_bytes()))
        };
        let mut slug = base.clone();
        let mut n = 2;
        while seen.contains(&slug) {
            slug = format!("{base}-{n}");
            n += 1;
        }
        seen.push(slug);
    }
    seen
}

into_node!(Sortable);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_are_plain_keys_or_stable_hashes() {
        let got = slugs(["12", "a b", "12", "", "a b"].into_iter());
        assert_eq!(got[0], "12");
        assert!(got[1].starts_with('h') && got[1].len() == 13, "{}", got[1]);
        assert_eq!(got[2], "12-2");
        assert!(got[3].starts_with('h'));
        assert_eq!(got[4], format!("{}-2", got[1]));
        // The same key gets the same slug in another drawing, whatever comes first.
        assert_eq!(slugs(["a b"].into_iter())[0], got[1]);
    }
}
