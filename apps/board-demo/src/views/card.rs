//! One card: in the drawer over the board, or on a page of its own with JavaScript
//! off. Its description, a checklist whose lines move like the cards do, the
//! comments, and what happened to it.

use chrono::NaiveDate;
use navree::ui::form::FormView;
use navree::ui::*;
use serde::Deserialize;

use crate::data::{Activity, Card, Check, Column, Comment, Person, labels_of};
use crate::views::board::DRAWER;
use crate::views::{due_label, moment, person};

/// Everything the drawer draws for one card.
#[derive(Clone, Debug, Default)]
pub struct CardData {
    pub card: Card,
    /// Its checklist, in order.
    pub checks: Vec<Check>,
    /// Its comments, oldest first.
    pub comments: Vec<Comment>,
    /// What happened to it, oldest first.
    pub activity: Vec<Activity>,
    pub people: Vec<Person>,
}

/// The card in the drawer: about 60% of the width, with the board in sight behind it.
pub fn card_sheet(data: &CardData, viewer: &Person, today: NaiveDate) -> Sheet {
    let column = Column::parse(&data.card.list).unwrap_or(Column::Backlog);
    Sheet::new(DRAWER, data.card.title.clone())
        .width(SheetWidth::Lg)
        .description(
            Row::gap(1).wrap().children((
                Badge::new(column.title()).tone(column.tone()).dot(),
                labels_of(&data.card.labels)
                    .iter()
                    .map(|label| {
                        Badge::new(label.title())
                            .variant(BadgeVariant::Outline)
                            .tone(label.tone())
                    })
                    .collect::<Vec<_>>(),
            )),
        )
        .body(card_detail(data, viewer, today, None))
}

/// What the drawer and the card's page hold: the work on the left, the facts and
/// the history on the right; one column on a phone. `form` is the comment form as
/// it came back with an error, if it did.
pub fn card_detail(
    data: &CardData,
    viewer: &Person,
    today: NaiveDate,
    form: Option<FormView<CommentForm>>,
) -> Node {
    Grid::cols(3)
        .gap(6)
        .children((
            Stack::gap(6).span(2).children((
                Stack::gap(2).children((
                    Heading::h3("Description"),
                    Text::new(data.card.description.clone()),
                )),
                checklist(data),
                comments(data, form),
                history(data),
            )),
            Stack::gap(4).children((facts(data, today), move_form(&data.card, viewer))),
        ))
        .into()
}

/// Who has it, when it is due, where it is.
fn facts(data: &CardData, today: NaiveDate) -> Node {
    let card = &data.card;
    let assignee = match person(&data.people, card.assignee) {
        Some(who) => FactCard::new("Assignee", who.name.clone()).detail(who.job.clone()),
        None => FactCard::new("Assignee", "Nobody yet"),
    };
    let due = match card.due {
        Some(due) => {
            let (when, tone) = due_label(due, today);
            FactCard::new("Due", due.format("%-d %B %Y").to_string())
                .tone(tone)
                .detail(when)
        }
        None => FactCard::new("Due", "No date"),
    };
    Stack::gap(3)
        .children((
            assignee.icon(icons::CIRCLE_USER_ROUND),
            due.icon(icons::CALENDAR),
        ))
        .into()
}

/// Moving without dragging: a column to move it to, at the end.
fn move_form(card: &Card, viewer: &Person) -> Node {
    let mut to = NativeSelect::new("to_list").attr("id", "card-move-to");
    for column in Column::ALL {
        let closed = column == Column::Done && !viewer.is_lead() && card.list != column.key();
        let label = if closed {
            format!("{} (a lead closes cards)", column.title())
        } else {
            column.title().to_owned()
        };
        to = to.option(column.key(), label);
    }
    let to = to.selected(card.list.clone());
    el("form")
        .attr("method", "post")
        .attr("action", format!("/cards/{}/move", card.id))
        .flag("data-ui-form", true)
        .uses(Uses::CORE)
        .children((
            el("input")
                .attr("type", "hidden")
                .attr("name", "item")
                .attr("value", card.id.to_string()),
            el("input")
                .attr("type", "hidden")
                .attr("name", "from_list")
                .attr("value", card.list.clone()),
            Field::new("card-move-to", "Column").control(to),
            Button::outline("Move")
                .size(Size::Sm)
                .icon(icons::MOVE_RIGHT)
                .submit(),
        ))
        .into()
}

/// The checklist: a bar for how much is done, and its lines, which tick and move.
pub fn checklist(data: &CardData) -> Node {
    let done = data.checks.iter().filter(|check| check.done == 1).count();
    let total = data.checks.len();
    let id = data.card.id;
    let lines = data.checks.iter().map(|check| {
        SortItem::new(check.id.to_string(), check.text.clone()).body(
            el("form")
                .attr("method", "post")
                .attr("action", format!("/checks/{}/toggle", check.id))
                .attr("id", format!("check-{}", check.id))
                .flag("data-ui-form", true)
                .uses(Uses::CORE)
                .child(
                    Row::gap(2).align(Align::Center).children((
                        Checkbox::new("done")
                            .checked(check.done == 1)
                            .id(format!("check-{}-box", check.id))
                            .on(Trigger::Change, Cmd::submit(format!("check-{}", check.id))),
                        el("label")
                            .attr("for", format!("check-{}-box", check.id))
                            .child(check.text.clone()),
                        el("noscript").child(Button::ghost("Save").size(Size::Xs).submit()),
                    )),
                ),
        )
    });
    Stack::gap(3)
        .id("checklist")
        .children((
            Row::gap(2).align(Align::Center).children((
                Heading::h3("Checklist"),
                (total > 0).then(|| Text::muted(format!("{done} of {total} done"))),
            )),
            (total > 0).then(|| {
                Progress::new(
                    u32::try_from(done).unwrap_or(0),
                    u32::try_from(total).unwrap_or(1),
                    "Checklist done",
                )
            }),
            Sortable::new(format!("checks-{id}"), "Checklist")
                .post(format!("/cards/{id}/checks/moves"))
                .heading(4)
                .list(
                    SortList::new("checks", "Checklist")
                        .hide_title()
                        .empty("No checklist yet")
                        .items(lines),
                ),
        ))
        .into()
}

/// A comment, as its form posts it.
#[derive(Debug, Deserialize, Validate, Form)]
#[form(id = "comment-form")]
pub struct CommentForm {
    /// Add a comment
    #[validate(length(min = 1, max = 2000))]
    #[form(
        widget = textarea,
        rows = 3,
        placeholder = "Write a note for the team",
        error = "Write something first, at most 2,000 characters."
    )]
    pub body: String,
}

/// The comments, oldest first, and a box to add one. `form` is that box as it came
/// back with an error; a fresh one otherwise.
pub fn comments(data: &CardData, form: Option<FormView<CommentForm>>) -> Node {
    let id = data.card.id;
    let thread: Vec<Node> = data
        .comments
        .iter()
        .map(|comment| {
            let who = person(&data.people, Some(comment.person_id));
            let name = who.map_or_else(|| "Someone".to_owned(), |p| p.name.clone());
            Message::new(BubbleSide::Start)
                .avatar(Avatar::placeholder(name.clone()).size(Size::Sm))
                .header(Row::gap(2).children((Text::new(name), Text::muted(moment(comment.at)))))
                .content(Bubble::new(BubbleSide::Start, comment.body.clone()))
                .into()
        })
        .collect();
    let form = form
        .unwrap_or_default()
        .post(format!("/cards/{id}/comments"))
        .one_column()
        .submit("Comment");
    Stack::gap(3)
        .id("comments")
        .children((
            Heading::h3(format!("Comments ({})", data.comments.len())),
            if thread.is_empty() {
                Text::muted("No comments yet. Ask a question or leave a note.").into()
            } else {
                Node::from(Stack::gap(3).children(thread))
            },
            form,
        ))
        .into()
}

/// What happened to the card, newest last.
fn history(data: &CardData) -> Node {
    let events = data.activity.iter().map(|event| {
        let who = person(&data.people, Some(event.person_id))
            .map_or_else(|| "Someone".to_owned(), |p| p.name.clone());
        let (title, tone, glyph) = match event.kind.as_str() {
            "created" => (
                "Created".to_owned(),
                Tone::Neutral,
                icons::SQUARE_KANBAN_PLUS,
            ),
            "moved" => (
                format!("Moved {}", event.detail.replace(" to ", " → ")),
                Tone::Info,
                icons::ARROW_RIGHT_LEFT,
            ),
            "commented" => (
                "Commented".to_owned(),
                Tone::named("violet"),
                icons::MESSAGE_SQUARE,
            ),
            "checked" => (event.detail.clone(), Tone::Success, icons::SQUARE_CHECK_BIG),
            _ => (event.detail.clone(), Tone::Neutral, icons::DOT),
        };
        TimelineEvent::new(title)
            .at(event.at)
            .tone(tone)
            .icon(glyph)
            .meta(Meta::new(who))
    });
    Stack::gap(2)
        .id("history")
        .children((
            Heading::h3("Activity"),
            Timeline::new("card-history", "What happened to this card").events(events),
        ))
        .into()
}

/// An empty drawer for a card that is gone.
pub fn missing_card() -> Sheet {
    Sheet::new(DRAWER, "Card").width(SheetWidth::Lg).body(
        Empty::new("That card isn't on the board")
            .icon(icons::SQUARE_KANBAN)
            .description("Someone may have deleted it."),
    )
}
