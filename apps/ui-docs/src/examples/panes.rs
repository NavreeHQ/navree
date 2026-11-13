//! SplitPane and LazyList.

use navree::ui::*;

use crate::catalog::{Answer, Example, single};

const SOURCE: &str = include_str!("panes.rs");

pub fn all() -> Vec<Example> {
    vec![
        // Wider than a card: three panes at a card's width leave no room for a row's words.
        single!("split-pane", "thumb", "Three panes", split_pane_thumb)
            .start()
            .width(600),
        single!(
            "split-pane",
            "default",
            "Panes with separators",
            split_pane_default
        )
        .start()
        .width(1100)
        .height(420),
        single!(
            "split-pane",
            "collapsed",
            "A collapsed pane",
            split_pane_collapsed
        )
        .start()
        .width(1100)
        .height(320),
        single!(
            "split-pane",
            "vertical",
            "One above another",
            split_pane_vertical
        )
        .start()
        .height(360),
        single!(
            "split-pane",
            "stacked",
            "Stacked on a phone",
            split_pane_stacked
        )
        .start()
        .width(390)
        .height(420),
        single!("lazy-list", "thumb", "A long list", lazy_list_thumb).start(),
        // The lists' next pages come from `answers`: a preview has no server to ask.
        single!(
            "lazy-list",
            "default",
            "The first page and more",
            lazy_list_default
        )
        .start()
        .answers(lazy_rows_next)
        .height(740),
        single!(
            "lazy-list",
            "conversation",
            "Oldest first, newest in view",
            lazy_list_conversation
        )
        .start()
        .answers(lazy_messages_earlier)
        .height(600),
    ]
}

/// What `/desk/rows` answers after the last row of the first page: the last four
/// conversations, and no more.
fn lazy_rows_next() -> Vec<Answer> {
    let older = [
        ("Invoice shows the wrong GSTIN", "Rajinder Sandhu · 6 h"),
        ("Can't add a second branch", "Simranjit Dhillon · 8 h"),
        ("Refund went to the old card", "Amandeep Grewal · 1 d"),
        ("Import stopped at row 402", "Taranjit Sidhu · 1 d"),
    ];
    vec![Answer {
        src: "/desk/rows",
        dir: "end",
        items: older
            .into_iter()
            .enumerate()
            .map(|(at, (title, line))| {
                LazyItem::new(
                    (6 + at).to_string(),
                    format!("{:012}", 994 - at),
                    Node::from(Item::new(title).description(line)),
                )
            })
            .collect(),
        more: false,
    }]
}

/// What `/desk/7/messages` answers before the first message shown: how the
/// conversation began, and no more.
fn lazy_messages_earlier() -> Vec<Answer> {
    let said = [
        (
            BubbleSide::Start,
            "Jasleen Kaur",
            "Hello, our upgrade to Growth isn't going through.",
        ),
        (
            BubbleSide::End,
            "Priya Verma",
            "Sorry about that. Let me look at your account.",
        ),
    ];
    vec![Answer {
        src: "/desk/7/messages",
        dir: "start",
        items: said
            .into_iter()
            .enumerate()
            .map(|(at, (side, who, text))| {
                LazyItem::new(
                    (68 + at).to_string(),
                    format!("{:012}", 68 + at),
                    Message::new(side)
                        .header(who)
                        .content(Bubble::new(side, text)),
                )
            })
            .collect(),
        more: false,
    }]
}

/// What the panes hold in these examples.
fn rows(n: usize) -> Vec<Node> {
    [
        ("Card declined on upgrade", "Jasleen Kaur · 4 min"),
        ("Reminders 5½ hours early", "Inderjit Randhawa · 21 min"),
        ("Webhook answered 410", "Gurleen Ghuman · 1 h"),
        ("Second location", "Mehak Chawla · 2 h"),
        ("Export every client", "Karan Bhalla · 3 h"),
        ("Staff see each other's schedules", "Baljeet Virk · 5 h"),
    ]
    .iter()
    .take(n)
    .map(|(title, line)| Item::new(*title).description(*line).into())
    .collect()
}

fn split_pane_thumb() -> SplitPane {
    SplitPane::new("thumb")
        .height(SplitHeight::Sm)
        .pane(Pane::new("thumb-list", "List").size(32).body(rows(3)))
        .pane(Pane::fill("thumb-detail", "Detail").body(Skeleton::new(SkeletonShape::Line)))
        .pane(
            Pane::new("thumb-side", "Side")
                .size(24)
                .body(Skeleton::new(SkeletonShape::Short)),
        )
}

fn split_pane_default() -> SplitPane {
    SplitPane::new("desk")
        .height(SplitHeight::Md)
        .stack_below(SplitWidth::Md)
        .save_to("/desk/panes")
        .grip()
        .pane(
            Pane::new("desk-list", "Conversations")
                .size(30)
                .min(22)
                .max(45)
                .header(Heading::h3("Conversations"))
                .body(rows(6)),
        )
        .pane(
            Pane::fill("desk-thread", "Conversation")
                .min(30)
                .header(Heading::h3("Card declined on upgrade"))
                .body(
                    Message::new(BubbleSide::Start)
                        .header("Jasleen Kaur")
                        .content(Bubble::new(
                            BubbleSide::Start,
                            "The card was declined twice this morning.",
                        )),
                )
                .footer(
                    Textarea::new("body")
                        .rows(2)
                        .placeholder("Write a reply")
                        .attr("aria-label", "Reply"),
                ),
        )
        .pane(
            Pane::new("desk-customer", "Customer")
                .size(26)
                .min(18)
                .max(40)
                .collapsible()
                .header(Heading::h3("Jasleen Kaur"))
                .body(FactCard::new("Plan", "Growth").detail("9 seats")),
        )
        .current("desk-thread")
}

fn split_pane_collapsed() -> SplitPane {
    SplitPane::new("folded")
        .height(SplitHeight::Sm)
        .pane(Pane::new("folded-list", "Files").size(28).body(rows(4)))
        .pane(Pane::fill("folded-main", "Editor").body(Text::muted("The side pane is collapsed. Press Enter on the separator, or drag it, to open it again.")))
        .pane(
            Pane::new("folded-side", "Outline")
                .size(24)
                .collapsible()
                .collapsed(true)
                .body("Outline"),
        )
}

fn split_pane_vertical() -> SplitPane {
    SplitPane::new("stacked-v")
        .vertical()
        .height(SplitHeight::Sm)
        .pane(
            Pane::new("stacked-v-top", "Query")
                .size(40)
                .body(Text::code("SELECT * FROM conversations")),
        )
        .pane(Pane::fill("stacked-v-bottom", "Results").body(rows(3)))
}

fn split_pane_stacked() -> SplitPane {
    SplitPane::new("phone")
        .height(SplitHeight::Md)
        .stack_below(SplitWidth::Md)
        .pane(
            Pane::new("phone-list", "Conversations")
                .size(30)
                .body(rows(5)),
        )
        .pane(
            Pane::fill("phone-thread", "Conversation")
                .header(
                    Row::gap(2).children((
                        SplitPane::stacked_only(
                            Button::ghost("Conversations")
                                .icon(icons::ARROW_LEFT)
                                .size(Size::Sm)
                                .href("/desk"),
                        ),
                        Heading::h3("Card declined on upgrade"),
                    )),
                )
                .body(Text::new(
                    "On a phone the panes show one at a time: the one the page is about.",
                )),
        )
        .current("phone-thread")
}

fn lazy_list_thumb() -> LazyList {
    LazyList::new("thumb-rows", "Conversations", "/desk/rows")
        .items(
            rows(3)
                .into_iter()
                .enumerate()
                .map(|(at, row)| LazyItem::new(at.to_string(), format!("{:012}", 100 - at), row)),
        )
        .more_after("/desk?after=000000000098")
}

fn lazy_list_default() -> LazyList {
    LazyList::new("desk-rows", "Conversations", "/desk/rows")
        .items(
            rows(6)
                .into_iter()
                .enumerate()
                .map(|(at, row)| LazyItem::new(at.to_string(), format!("{:012}", 1_000 - at), row)),
        )
        .more_after("/desk?after=000000000995")
        .since("41")
        .live_from("/desk/events", Some("rows"), LazyEnd::Start)
        .max(300)
}

fn lazy_list_conversation() -> LazyList {
    let said = [
        (
            BubbleSide::Start,
            "Jasleen Kaur",
            "We tried to upgrade and the card was declined twice.",
        ),
        (
            BubbleSide::End,
            "Priya Verma",
            "I can see two attempts at 09:12 and 09:14.",
        ),
        (
            BubbleSide::Start,
            "Jasleen Kaur",
            "That worked! We're on Growth now.",
        ),
    ];
    LazyList::new("desk-messages", "Messages", "/desk/7/messages")
        .order(LazyOrder::Ascending)
        .gap(4)
        .items(said.into_iter().enumerate().map(|(at, (side, who, text))| {
            LazyItem::new(
                (70 + at).to_string(),
                format!("{:012}", 70 + at),
                Message::new(side)
                    .header(who)
                    .content(Bubble::new(side, text)),
            )
        }))
        .more_before("/desk/7?older=000000000070")
        .labels(LazyLabels {
            start: "Show earlier messages".to_owned(),
            ..LazyLabels::default()
        })
}
