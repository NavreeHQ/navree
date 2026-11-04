//! Sortable lists and boards.

use navree::ui::*;

use crate::catalog::{Example, single};

const SOURCE: &str = include_str!("sortable.rs");

pub fn all() -> Vec<Example> {
    vec![
        single!("sortable", "thumb", "A board", sortable_thumb).start().height(260),
        single!("sortable", "default", "A list", sortable_default)
            .start()
            .live()
            .height(260),
        single!("sortable", "board", "A board of columns", sortable_board)
            .start()
            .live()
            .height(420),
        single!("sortable", "limits", "Limits and refusals", sortable_limits)
            .start()
            .live()
            .height(300),
        single!("sortable", "long", "A long list, a page at a time", sortable_long)
            .start()
            .live()
            .height(320),
    ]
}

fn sortable_thumb() -> Sortable {
    Sortable::new("thumb", "Release board")
        .board()
        .post("/moves")
        .list(
            SortList::new("todo", "To do")
                .item(SortItem::new("1", "Write the changelog"))
                .item(SortItem::new("2", "Tag the release")),
        )
        .list(SortList::new("doing", "Doing").item(SortItem::new("3", "Run the load test")))
}

fn sortable_default() -> Sortable {
    Sortable::new("setlist", "Set list")
        .post("/setlist/moves")
        .list(
            SortList::new("songs", "Songs")
                .hide_title()
                .item(SortItem::new("1", "Opening, slow build"))
                .item(SortItem::new("2", "The single"))
                .item(SortItem::new("3", "Acoustic, two voices"))
                .item(SortItem::new("4", "Encore")),
        )
}

fn sortable_board() -> Sortable {
    let card = |key: &str, title: &str, label: Badge| {
        SortItem::new(key, title).body(Stack::gap(2).children((label, Text::new(title.to_owned()))))
    };
    Sortable::new("launch", "Launch board")
        .board()
        .post("/launch/moves")
        .list(
            SortList::new("ready", "Ready")
                .item(card("11", "Pick the launch date", Badge::new("Plan").tone(Tone::Info)))
                .item(card("12", "Draft the press note", Badge::new("Docs").tone(Tone::named("cyan")))),
        )
        .list(
            SortList::new("doing", "In progress")
                .limit(3)
                .item(card("13", "Fix the sign-up email", Badge::new("Bug").tone(Tone::Danger))),
        )
        .list(SortList::new("done", "Done").empty("Nothing done yet"))
}

fn sortable_limits() -> Sortable {
    Sortable::new("review", "Review board")
        .board()
        .post("/review/moves")
        .list(
            SortList::new("doing", "In progress")
                .item(SortItem::new("21", "Speed up the search page"))
                .item(SortItem::new("22", "Invoice run").locked("A payment is being taken.")),
        )
        .list(
            SortList::new("review", "Review")
                .limit(1)
                .item(SortItem::new("23", "New pricing table")),
        )
        .list(SortList::new("shipped", "Shipped").refuse("Only a lead can ship."))
}

fn sortable_long() -> Sortable {
    let items = (1..=3).map(|n| SortItem::new(n.to_string(), format!("Ticket {n}")));
    Sortable::new("queue", "Support queue")
        .post("/queue/moves")
        .list(
            SortList::new("open", "Open")
                .total(4_210)
                .items(items)
                .more("/queue/open", [("after", "196608.3"), ("show", "6")]),
        )
}
