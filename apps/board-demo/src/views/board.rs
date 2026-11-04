//! The board: five columns of cards that move by pointer, touch or keys, filters
//! over them, a drawer for one card, and a palette that jumps to any card.

use chrono::NaiveDate;
use navree::ui::*;

use crate::data::{Card, Check, Column, Comment, Label, PROJECT, Person, labels_of};
use crate::views::{due_label, person};

/// The region the board is drawn in: filters redraw it, and the stream refreshes it.
pub const REGION: &str = "board";
/// The drawer a card opens in.
pub const DRAWER: &str = "card";

/// Everything the board draws.
#[derive(Clone, Debug, Default)]
pub struct BoardData {
    pub people: Vec<Person>,
    /// Every card, in column order and then by position.
    pub cards: Vec<Card>,
    pub checks: Vec<Check>,
    pub comments: Vec<Comment>,
}

impl BoardData {
    /// How many of a card's checklist lines are ticked, and how many there are.
    pub fn checklist(&self, card: i64) -> (usize, usize) {
        let lines = self.checks.iter().filter(|check| check.card_id == card);
        let done = lines.clone().filter(|check| check.done == 1).count();
        (done, lines.count())
    }

    pub fn comment_count(&self, card: i64) -> usize {
        self.comments.iter().filter(|c| c.card_id == card).count()
    }
}

/// Which cards to show.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Filters {
    pub assignee: Option<i64>,
    pub label: Option<Label>,
    pub due: Due,
}

/// A window of due dates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Due {
    #[default]
    Any,
    Late,
    Week,
    Undated,
}

impl Due {
    pub const ALL: [(Self, &'static str, &'static str); 4] = [
        (Self::Any, "", "Any due date"),
        (Self::Late, "late", "Late"),
        (Self::Week, "week", "Due this week"),
        (Self::Undated, "none", "No due date"),
    ];

    fn key(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(due, ..)| *due == self)
            .map_or("", |(_, key, _)| key)
    }
}

impl Filters {
    /// From the query's text, a value at a time: anything unknown is no filter.
    pub fn parse(assignee: Option<&str>, label: Option<&str>, due: Option<&str>) -> Self {
        Self {
            assignee: assignee.and_then(|id| id.parse().ok()),
            label: label.and_then(Label::parse),
            due: due
                .and_then(|key| Due::ALL.iter().find(|(_, k, _)| *k == key && !k.is_empty()))
                .map_or(Due::Any, |(due, ..)| *due),
        }
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// The filters as a query, `?assignee=2&due=week`, or nothing.
    pub fn query(&self) -> String {
        let mut parts = Vec::new();
        if let Some(id) = self.assignee {
            parts.push(format!("assignee={id}"));
        }
        if let Some(label) = self.label {
            parts.push(format!("label={}", label.key()));
        }
        if self.due != Due::Any {
            parts.push(format!("due={}", self.due.key()));
        }
        if parts.is_empty() {
            String::new()
        } else {
            format!("?{}", parts.join("&"))
        }
    }

    /// Whether `card` passes.
    pub fn keeps(&self, card: &Card, today: NaiveDate) -> bool {
        let assignee = self.assignee.is_none_or(|id| card.assignee == Some(id));
        let label = self
            .label
            .is_none_or(|label| labels_of(&card.labels).contains(&label));
        let due = match self.due {
            Due::Any => true,
            Due::Late => card.due.is_some_and(|due| due < today),
            Due::Week => card
                .due
                .is_some_and(|due| due >= today && (due - today).num_days() < 7),
            Due::Undated => card.due.is_none(),
        };
        assignee && label && due
    }
}

/// The columns, each card drawn by [`card_face`]. A move posts to `/board/moves`,
/// where the server checks it against the board as it is then. A column counts
/// every card it holds, shown or filtered out, so its limit is read right.
pub fn columns(data: &BoardData, viewer: &Person, filters: &Filters, today: NaiveDate) -> Sortable {
    let lists = Column::ALL.map(|column| {
        let held: Vec<&Card> = data
            .cards
            .iter()
            .filter(|c| c.list == column.key())
            .collect();
        let shown = held.iter().filter(|card| filters.keeps(card, today));
        let mut list = SortList::new(column.key(), column.title())
            .items(shown.map(|card| {
                SortItem::new(card.id.to_string(), card.title.clone())
                    .body(card_face(card, data, today))
            }))
            .total(held.len())
            .empty(if filters.is_empty() {
                "No cards"
            } else {
                "No cards match"
            });
        if let Some(limit) = column.limit() {
            list = list.limit(limit);
        }
        if column == Column::Done && !viewer.is_lead() {
            list = list.refuse("Only a lead can move cards to Done.");
        }
        list
    });
    Sortable::new("columns", format!("{PROJECT} board"))
        .board()
        .post("/board/moves")
        .lists(lists)
}

/// One card as the board shows it: its labels, its title (which opens the drawer),
/// who has it, when it is due, its checklist and its comments.
pub fn card_face(card: &Card, data: &BoardData, today: NaiveDate) -> Node {
    let labels = labels_of(&card.labels);
    let (done, lines) = data.checklist(card.id);
    let comments = data.comment_count(card.id);
    let mut facts = Vec::new();
    if let Some(who) = person(&data.people, card.assignee) {
        let first = who.name.split(' ').next().unwrap_or_default().to_owned();
        facts.push(Meta::new(first).start(Avatar::placeholder(who.name.clone()).size(Size::Sm)));
    }
    if lines > 0 {
        facts.push(Meta::icon(icons::LIST_CHECKS, format!("{done}/{lines}")));
    }
    if comments > 0 {
        facts.push(Meta::icon(icons::MESSAGE_SQUARE, comments.to_string()));
    }
    Stack::gap(2)
        .children((
            (!labels.is_empty() || card.due.is_some()).then(|| {
                Row::gap(1).wrap().children((
                    labels
                        .iter()
                        .map(|label| Badge::new(label.title()).tone(label.tone()).dot())
                        .collect::<Vec<_>>(),
                    card.due.map(|due| {
                        let (text, tone) = due_label(due, today);
                        Badge::new(text).variant(BadgeVariant::Outline).tone(tone)
                    }),
                ))
            }),
            open_card(card),
            Meta::list(facts),
        ))
        .into()
}

/// The card's title, as a button that opens it in the drawer. With JavaScript off
/// it leads to the card's own page. A drag may start on it: a press that moves is
/// a drag, one that doesn't is a click.
pub fn open_card(card: &Card) -> Node {
    el("form")
        .attr("method", "get")
        .attr("action", format!("/cards/{}/peek", card.id))
        .flag("data-ui-form", true)
        .uses(Uses::CORE)
        .child(
            Button::new(card.title.clone())
                .variant(ButtonVariant::Link)
                .submit()
                .attr("aria-haspopup", "dialog")
                .attr("data-ui-drag", ""),
        )
        .into()
}

/// The filters: who, which label, when due. Each change redraws the board in place
/// and keeps the address; with JavaScript off, Apply does.
pub fn filter_bar(people: &[Person], filters: &Filters) -> Node {
    let assignee = Combobox::new("assignee")
        .aria_label("Assignee")
        .placeholder("Anyone")
        .clearable()
        .options(people.iter().map(|p| (p.id.to_string(), p.name.clone())));
    let assignee = match filters.assignee {
        Some(id) => assignee.selected(id.to_string()),
        None => assignee,
    };
    let mut label = NativeSelect::new("label")
        .attr("aria-label", "Label")
        .option("", "Any label");
    for one in Label::ALL {
        label = label.option(one.key(), one.title());
    }
    let label = label.selected(filters.label.map_or("", Label::key));
    let mut due = NativeSelect::new("due").attr("aria-label", "Due date");
    for (_, key, text) in Due::ALL {
        due = due.option(key, text);
    }
    let due = due.selected(filters.due.key());
    el("form")
        .attr("method", "get")
        .attr("action", "/")
        .attr("autocomplete", "off")
        .attr("data-ui-target", REGION)
        .attr("aria-label", "Filter the board")
        .uses(Uses::CORE)
        .child(Grid::cols(4).gap(2).children((
            assignee,
            label,
            due,
            Row::gap(2).align(Align::Center).children((
                el("noscript").child(Button::outline("Apply").submit()),
                (!filters.is_empty()).then(|| Button::ghost("Clear filters").href("/")),
            )),
        )))
        .into()
}

/// The board in its region: redrawn by the filters, and fetched again whenever
/// the event stream says the board changed, so other people's moves slide in.
pub fn board(data: &BoardData, viewer: &Person, filters: &Filters, today: NaiveDate) -> Region {
    Region::new(REGION)
        .src(format!("/fragments/board{}", filters.query()))
        .live_from("/events")
        .children(Stack::gap(2).children((live_state(), columns(data, viewer, filters, today))))
}

/// "Live", with a dot, while the board follows the stream.
fn live_state() -> Node {
    el("span")
        .slot("live-state")
        .attr("role", "status")
        .children((
            el("span").slot("live-dot").attr("aria-hidden", "true"),
            el("span")
                .attr("data-for", "connecting")
                .child("Connecting"),
            el("span")
                .attr("data-for", "on")
                .child("Live: moves by your team show up here"),
            el("span").attr("data-for", "retry").child("Reconnecting"),
            el("span").attr("data-for", "paused").child("Paused"),
            el("span").attr("data-for", "closed").child("Not live"),
        ))
        .into()
}

/// The drawer, empty until a card is opened.
pub fn empty_drawer() -> Sheet {
    Sheet::new(DRAWER, "Card").width(SheetWidth::Lg)
}

/// The palette: every card, found by its title, its column, who has it or its
/// labels. Choosing one opens it in the drawer. `Ctrl` or `Cmd` and `K` opens it.
pub fn palette(data: &BoardData) -> Command {
    let cards = data.cards.iter().map(|card| {
        let column = Column::parse(&card.list).map_or("", Column::title);
        let mut words: Vec<String> = labels_of(&card.labels)
            .iter()
            .map(|label| label.title().to_owned())
            .collect();
        if let Some(who) = person(&data.people, card.assignee) {
            words.push(who.name.clone());
        }
        CommandItem::run(
            card.title.clone(),
            vec![
                Cmd::value("peek-card", card.id.to_string()),
                Cmd::submit("peek"),
            ],
        )
        .icon(icons::SQUARE_KANBAN)
        .description(column)
        .keywords(words)
    });
    Command::new("jump")
        .title("Jump to a card")
        .placeholder("Search cards by title, person or label")
        .group(CommandGroup::new("Cards").items(cards))
        .shortcut(true)
}

/// The one form the palette opens a card with.
pub fn peek_form() -> Node {
    el("form")
        .attr("id", "peek")
        .attr("method", "get")
        .attr("action", "/cards/peek")
        .flag("data-ui-form", true)
        .flag("hidden", true)
        .uses(Uses::CORE)
        .child(
            el("input")
                .attr("type", "hidden")
                .attr("id", "peek-card")
                .attr("name", "card")
                .settable(),
        )
        .into()
}

/// Who the board is shown as: a lead may move cards to Done, a member may not.
pub fn viewer_menu(people: &[Person], viewer: &Person) -> DropdownMenu {
    let mut menu = DropdownMenu::new("viewer", format!("Viewing as {}", viewer.name))
        .variant(ButtonVariant::Outline)
        .caret()
        .header("Show the board as");
    for one in people {
        let role = if one.is_lead() { "lead" } else { "member" };
        menu = menu.item(
            MenuItem::post(format!("{} ({role})", one.name), format!("/as/{}", one.id))
                .icon(crate::views::role_icon(one)),
        );
    }
    menu
}

/// The whole board page in `page`.
pub fn board_page(
    page: AppPage,
    data: &BoardData,
    viewer: &Person,
    filters: &Filters,
    today: NaiveDate,
) -> AppPage {
    page.actions((
        Command::trigger("jump", "Jump to a card"),
        viewer_menu(&data.people, viewer),
    ))
    .body((
        Stack::gap(4).children((
            filter_bar(&data.people, filters),
            board(data, viewer, filters, today),
        )),
        empty_drawer(),
        palette(data),
        peek_form(),
    ))
}
