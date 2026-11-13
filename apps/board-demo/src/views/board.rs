//! The board: five columns of cards that move by pointer, touch or keys, filters
//! over them, a drawer for one card, and a palette that finds any card.
//!
//! A column holds thousands of cards, so the page draws the first ones and each
//! column loads more as it is scrolled, a page at a time, by a cursor.

use chrono::NaiveDate;
use navree::ui::*;

use crate::data::{Card, Column, Label, PROJECT, Person, labels_of};
use crate::views::{due_label, person, web_only};

/// The region the board is drawn in: filters redraw it, and the stream refreshes it.
pub const REGION: &str = "board";
/// The drawer a card opens in.
pub const DRAWER: &str = "card";
/// How many cards a column draws at first, and loads at a time after that.
pub const PAGE: usize = 20;
/// The most cards a column keeps in the page. Past this, filters and the palette
/// find the rest.
pub const CAP: usize = 200;

/// One column as the page holds it: some of its cards, from `start`.
#[derive(Clone, Debug)]
pub struct ColumnPage {
    pub column: Column,
    /// The cards drawn, in order.
    pub cards: Vec<Card>,
    /// How many cards the column holds, drawn or not.
    pub total: u64,
    /// Where the cards drawn start in the column (0 for the first page).
    pub start: usize,
    /// Where each card drawn stands in the whole column, 1 being first, when a
    /// filter leaves some out; empty when the cards drawn are the column's own run.
    pub places: Vec<usize>,
    /// The cursor after the last card drawn, when the filter has more: that card's
    /// id, read as "after this card, wherever it is now".
    pub next: Option<String>,
}

/// Everything the board draws.
#[derive(Clone, Debug, Default)]
pub struct BoardData {
    pub people: Vec<Person>,
    pub columns: Vec<ColumnPage>,
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

    pub fn key(self) -> &'static str {
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

    /// The filters as query pairs.
    pub fn pairs(&self) -> Vec<(String, String)> {
        let mut pairs = Vec::new();
        if let Some(id) = self.assignee {
            pairs.push(("assignee".to_owned(), id.to_string()));
        }
        if let Some(label) = self.label {
            pairs.push(("label".to_owned(), label.key().to_owned()));
        }
        if self.due != Due::Any {
            pairs.push(("due".to_owned(), self.due.key().to_owned()));
        }
        pairs
    }
}

/// What the board's address says beyond its filters: how many cards each column
/// holds in the page, past the first page.
pub fn shown_pairs(data: &BoardData) -> Vec<(String, String)> {
    data.columns
        .iter()
        .map(|page| (page.column, page.start + page.cards.len()))
        .filter(|(_, shown)| *shown > PAGE)
        .map(|(column, shown)| (format!("show.{}", column.key()), shown.to_string()))
        .collect()
}

/// Where a move posts: `/board/moves`, with the board's filters and how much of
/// each column it holds, so the page a move with JavaScript off goes back to is
/// the one it was made on.
pub fn moves_action(pairs: &[(String, String)]) -> String {
    format!("/board/moves{}", query(pairs))
}

/// A query string, `?a=1&b=2`, or nothing. The values here are numbers and keys.
pub fn query(pairs: &[(String, String)]) -> String {
    if pairs.is_empty() {
        return String::new();
    }
    let text: Vec<String> = pairs
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect();
    format!("?{}", text.join("&"))
}

/// One column as the board draws it: its cards, its count and limit, and what
/// ends it: a button for the next cards, or a note at the cap.
pub fn column(
    page: &ColumnPage,
    people: &[Person],
    viewer: &Person,
    filters: &Filters,
    today: NaiveDate,
) -> SortList {
    let column = page.column;
    let shown = page.start + page.cards.len();
    let mut list = SortList::new(column.key(), column.title())
        .items(page.cards.iter().enumerate().map(|(at, card)| {
            let item = SortItem::new(card.id.to_string(), card.title.clone())
                .body(card_face(card, people, today));
            // Under a filter, a card's move form counts in the whole column.
            match page.places.get(at) {
                Some(place) => item.position(*place),
                None => item,
            }
        }))
        .total(usize::try_from(page.total).unwrap_or(usize::MAX))
        .from(page.start)
        .empty(if filters.is_empty() {
            "No cards"
        } else {
            "No cards match"
        });
    if let Some(cursor) = &page.next {
        list = if shown >= CAP {
            list.end_note(format!(
                "Showing the first {CAP}. Filter the board, or press Ctrl K, to find the rest."
            ))
        } else {
            let mut fields = vec![
                ("after".to_owned(), cursor.clone()),
                ("show".to_owned(), (shown + PAGE).to_string()),
            ];
            fields.extend(filters.pairs());
            list.more(format!("/columns/{}", column.key()), fields)
        };
    }
    if let Some(limit) = column.limit() {
        list = list.limit(limit);
    }
    if column == Column::Done && !viewer.is_lead() {
        list = list.refuse("Only a lead can move cards to Done.");
    }
    list
}

/// The columns. A move posts to `/board/moves`, where the server checks it
/// against the board as it is then and gives the card a rank between its new
/// neighbours'.
pub fn columns(data: &BoardData, viewer: &Person, filters: &Filters, today: NaiveDate) -> Sortable {
    let mut pairs = filters.pairs();
    pairs.extend(shown_pairs(data));
    Sortable::new("columns", format!("{PROJECT} board"))
        .board()
        .post(moves_action(&pairs))
        .lists(
            data.columns
                .iter()
                .map(|page| column(page, &data.people, viewer, filters, today)),
        )
}

/// One card as the board shows it: its labels, its title (which opens the drawer),
/// who has it, when it is due, its checklist and its comments.
pub fn card_face(card: &Card, people: &[Person], today: NaiveDate) -> Node {
    let labels = labels_of(&card.labels);
    let mut facts = Vec::new();
    if let Some(who) = person(people, card.assignee) {
        let first = who.name.split(' ').next().unwrap_or_default().to_owned();
        facts.push(Meta::new(first).start(Avatar::placeholder(who.name.clone()).size(Size::Sm)));
    }
    if card.checks_total > 0 {
        facts.push(Meta::icon(
            icons::LIST_CHECKS,
            format!("{}/{}", card.checks_done, card.checks_total),
        ));
    }
    if card.comments > 0 {
        facts.push(Meta::icon(icons::MESSAGE_SQUARE, card.comments.to_string()));
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

/// The board in its region: redrawn by the filters, and fetched again whenever the
/// event stream says the board changed, so other people's moves slide in. Its
/// source asks for as many cards in each column as the page holds.
pub fn board(data: &BoardData, viewer: &Person, filters: &Filters, today: NaiveDate) -> Region {
    let mut pairs = filters.pairs();
    pairs.extend(shown_pairs(data));
    Region::new(REGION)
        .src(format!("/fragments/board{}", query(&pairs)))
        .live_on(crate::live::stream().on(crate::live::TOPIC))
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

/// The palette: finds any card by its title, on the server, as you type, and
/// opens its page. `Ctrl` or `Cmd` and `K` opens it.
pub fn palette() -> Command {
    Command::new("jump")
        .title("Find a card")
        .placeholder("Search every card by title")
        .source("/cards/search")
        .shortcut(true)
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
        web_only(Command::trigger("jump", "Find a card")),
        viewer_menu(&data.people, viewer),
    ))
    .body((
        Stack::gap(4).children((
            filter_bar(&data.people, filters),
            board(data, viewer, filters, today),
        )),
        empty_drawer(),
        web_only(palette()),
    ))
}
