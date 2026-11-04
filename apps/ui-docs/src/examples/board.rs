//! The board example app's own views, drawn from its seed: what `/ui/` shows under
//! "Build anything". The code beside each preview is the app's source, read from its
//! files, so the website shows what the app runs.

use board_demo::data::{self, Column, PROJECT, TEAM};
use board_demo::views::board::{BoardData, ColumnPage, Filters, PAGE, board_page, columns};
use board_demo::views::card::{CardData, card_detail};
use navree::ui::*;

use crate::catalog::{self, Example};

/// The moment the previews are drawn at, fixed so the files don't change:
/// 4 November 2026, 12:00 UTC.
const NOW: i64 = 1_793_793_600;

const BOARD_RS: &str = include_str!("../../../board-demo/src/views/board.rs");
const CARD_RS: &str = include_str!("../../../board-demo/src/views/card.rs");

pub fn all() -> Vec<Example> {
    vec![
        Example::new(
            "build-anything",
            "board-columns",
            "A board of five columns",
            catalog::Kind::Single(board),
            items(BOARD_RS, &["pub fn column", "pub fn card_face"]),
        )
        .fill()
        .width(1440)
        .height(900)
        .streaming("on"),
        Example::new(
            "build-anything",
            "board-card",
            "A card: its checklist, comments and history",
            catalog::Kind::Single(card),
            items(CARD_RS, &["pub fn card_detail", "pub fn checklist"]),
        )
        .start()
        .height(900),
        Example::new(
            "build-anything",
            "board-drag",
            "A card part way through a move",
            catalog::Kind::Single(drag),
            items(BOARD_RS, &["pub fn columns"]),
        )
        .fill()
        .width(1440)
        .height(900),
        Example::new(
            "build-anything",
            "board-sync",
            "Two people on one board",
            catalog::Kind::Single(sync),
            items(BOARD_RS, &["pub fn board"]),
        )
        .start()
        .height(520)
        .streaming("on"),
    ]
}

/// Top-level items of `source` as written, each from its attributes to its
/// closing brace, one after another.
fn items(source: &str, starts: &[&str]) -> String {
    let source = source.replace("\r\n", "\n");
    let lines: Vec<&str> = source.lines().collect();
    let mut out = Vec::new();
    for start in starts {
        let at = lines
            .iter()
            .position(|line| {
                line.starts_with(&format!("{start}(")) || line.starts_with(&format!("{start}<"))
            })
            .unwrap_or_else(|| panic!("no `{start}` at the start of a line"));
        let mut first = at;
        while first > 0
            && (lines[first - 1].starts_with("#[") || lines[first - 1].starts_with("///"))
        {
            first -= 1;
        }
        let end = (at..lines.len())
            .find(|line| lines[*line] == "}")
            .unwrap_or_else(|| panic!("`{start}` doesn't end"));
        out.push(lines[first..=end].join("\n"));
    }
    out.join("\n\n")
}

/// The seed, as the app's first screen holds it: each column's first page.
fn seeded(moves: &[(i64, Column)]) -> BoardData {
    let mut seed = data::seed(NOW);
    for (id, to) in moves {
        let last = seed
            .cards
            .iter()
            .filter(|card| card.list == to.key())
            .map(|card| card.rank)
            .max()
            .unwrap_or(0);
        if let Some(card) = seed.cards.iter_mut().find(|card| card.id == *id) {
            card.list = to.key().to_owned();
            card.rank = last + 1;
        }
    }
    let columns = Column::ALL
        .iter()
        .map(|column| {
            let mut held: Vec<_> = seed
                .cards
                .iter()
                .filter(|card| card.list == column.key())
                .cloned()
                .collect();
            held.sort_by_key(|card| (card.rank, card.id));
            let total = held.len() as u64;
            let next = (held.len() > PAGE).then(|| held[PAGE - 1].id.to_string());
            held.truncate(PAGE);
            ColumnPage {
                column: *column,
                cards: held,
                total,
                start: 0,
                places: Vec::new(),
                next,
            }
        })
        .collect();
    BoardData {
        people: seed.people,
        columns,
    }
}

fn today() -> chrono::NaiveDate {
    data::day_of(NOW)
}

/// The board page, and what a teammate's two moves bring, replayed in the preview:
/// each card as the app draws it in its new column.
fn board() -> Node {
    let data = seeded(&[]);
    let viewer = data.people[0].clone();
    let page = board_page(
        board_demo::views::shell()
            .page(PROJECT, "/")
            .crumb(TEAM, "/"),
        &data,
        &viewer,
        &Filters::default(),
        today(),
    );
    let moves = [(11, Column::Review), (6, Column::Doing)];
    let after = seeded(&moves);
    let drawn = render(&columns(&after, &viewer, &Filters::default(), today()).into());
    let mut replay = String::new();
    for (id, to) in moves {
        if let Some(card) = item_markup(&drawn, id) {
            let into = format!("columns-list-{}-items", to.key());
            replay.push_str(&card.replacen(
                "<li ",
                &format!("<li data-preview-into=\"{into}\" data-preview-replaces=\"columns-item-{id}\" data-preview-at=\"end\" "),
                1,
            ));
        }
    }
    Node::list(vec![
        page.to_view().body_node().clone(),
        el("template")
            .flag("data-preview-replay", true)
            .child(SafeHtml::trusted(replay))
            .into(),
    ])
}

/// The markup of card `id`'s item in `html`, from its `<li>` to the `</li>` that
/// closes it.
fn item_markup(html: &str, id: i64) -> Option<String> {
    let start = html.find(&format!(
        "<li data-slot=\"sortable-item\" id=\"columns-item-{id}\""
    ))?;
    let mut depth = 0;
    let mut at = start;
    while at < html.len() {
        let rest = &html[at..];
        if rest.starts_with("<li") {
            depth += 1;
            at += 3;
        } else if rest.starts_with("</li>") {
            depth -= 1;
            at += 5;
            if depth == 0 {
                return Some(html[start..at].to_owned());
            }
        } else {
            at += rest.chars().next().map_or(1, char::len_utf8);
        }
    }
    None
}

/// The card the drawer opens, with its checklist, comments and history.
fn card() -> Node {
    let seed = data::seed(NOW);
    let card = seed.cards[9].clone();
    let mut comments: Vec<_> = seed
        .comments
        .iter()
        .filter(|c| c.card_id == card.id)
        .cloned()
        .collect();
    comments.sort_by_key(|c| std::cmp::Reverse((c.at, c.id)));
    let mut activity: Vec<_> = seed
        .activity
        .iter()
        .filter(|a| a.card_id == card.id)
        .cloned()
        .collect();
    activity.sort_by_key(|a| std::cmp::Reverse((a.at, a.id)));
    let data = CardData {
        checks: seed
            .checks
            .iter()
            .filter(|c| c.card_id == card.id)
            .cloned()
            .collect(),
        card,
        comments,
        older_comments: None,
        activity,
        older_history: None,
        people: seed.people.clone(),
    };
    Card::titled(data.card.title.clone())
        .heading(2)
        .body(card_detail(&data, &seed.people[0], today(), None))
        .into()
}

/// The board with In progress's second card lifted by the keys and carried into
/// Review, the board saying where it is: what a move looks like part way through.
fn drag() -> Node {
    let data = seeded(&[(11, Column::Review)]);
    let viewer = data.people[0].clone();
    let page = board_page(
        board_demo::views::shell()
            .page(PROJECT, "/")
            .crumb(TEAM, "/"),
        &data,
        &viewer,
        &Filters::default(),
        today(),
    );
    let html = render(page.to_view().body_node())
        .replacen(
            "<li data-slot=\"sortable-item\" id=\"columns-item-11\"",
            "<li data-slot=\"sortable-item\" id=\"columns-item-11\" data-lifted",
            1,
        )
        .replacen(
            "aria-atomic=\"true\" data-ui-sr></div>",
            "aria-atomic=\"true\" data-ui-sr>Position 2 of 2 in Review.</div>",
            1,
        );
    Node::from(SafeHtml::trusted(html))
}

/// Two screens of one board, In progress and Review: Gurpreet has just moved a
/// card to Review, and on Harleen's screen it slides in, replayed here.
fn sync() -> Node {
    let before = seeded(&[]);
    let after = seeded(&[(11, Column::Review)]);
    let lead = before.people[0].clone();
    let pair = |id: &str, data: &BoardData| {
        let lists = data
            .columns
            .iter()
            .filter(|page| matches!(page.column, Column::Doing | Column::Review))
            .map(|page| {
                board_demo::views::board::column(
                    page,
                    &data.people,
                    &lead,
                    &Filters::default(),
                    today(),
                )
            });
        Sortable::new(id, "In progress and Review")
            .board()
            .post("/board/moves")
            .lists(lists)
    };
    let theirs = render(&pair("harleen", &after).into());
    let card = item_markup(&theirs.replace("harleen-item-", "columns-item-"), 11)
        .map(|card| card.replace("columns-item-", "harleen-item-"))
        .unwrap_or_default()
        .replacen(
            "<li ",
            "<li data-preview-into=\"harleen-list-review-items\" data-preview-replaces=\"harleen-item-11\" data-preview-at=\"end\" ",
            1,
        );
    Grid::cols(2)
        .gap(4)
        .children((
            Card::titled("Gurpreet's screen")
                .heading(2)
                .body(pair("gurpreet", &after)),
            Card::titled("Harleen's screen").heading(2).body((
                pair("harleen", &before),
                Node::from(
                    el("template")
                        .flag("data-preview-replay", true)
                        .child(SafeHtml::trusted(card)),
                ),
            )),
        ))
        .into()
}
