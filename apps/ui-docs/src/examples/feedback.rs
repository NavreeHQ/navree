//! Alert, Empty, Progress, Skeleton, Spinner.

use navree::ui::*;

use crate::catalog::{Example, gallery, single};

const SOURCE: &str = include_str!("feedback.rs");

pub fn all() -> Vec<Example> {
    vec![
        single!("alert", "default", "An alert", alert_default).start(),
        gallery!("alert", "tones", "Tones", alert_tones).wide(),
        single!("alert", "title-only", "A title alone", alert_title_only).start(),
        single!("empty", "default", "Nothing yet", empty_default).height(240),
        single!(
            "empty",
            "search",
            "A search that found nothing",
            empty_search
        )
        .height(200),
        single!("progress", "default", "Progress", progress_default).start(),
        gallery!("progress", "states", "States", progress_states).wide(),
        single!("skeleton", "default", "A card loading", skeleton_default).start(),
        gallery!("skeleton", "shapes", "Shapes", skeleton_shapes).medium(),
        single!("spinner", "default", "A spinner", spinner_default),
        single!("spinner", "inline", "Beside its text", spinner_inline),
    ]
}

fn alert_default() -> Alert {
    Alert::new(AlertTone::Success)
        .title("Deal saved")
        .body("Acme renewal moved to Proposal.")
}

fn alert_tones() -> impl IntoIterator<Item = (&'static str, Alert)> {
    [
        (
            "Neutral",
            Alert::new(AlertTone::Neutral)
                .title("Imports run at night")
                .body("Rows added after 02:00 show up tomorrow."),
        ),
        (
            "Success",
            Alert::new(AlertTone::Success)
                .title("Saved")
                .body("Your changes are live."),
        ),
        (
            "Warning",
            Alert::new(AlertTone::Warning)
                .title("Trial ends in 3 days")
                .body("Add a card to keep your data."),
        ),
        (
            "Danger",
            Alert::new(AlertTone::Danger)
                .title("Payment failed")
                .body("The card was declined. Try another one."),
        ),
    ]
}

fn alert_title_only() -> Alert {
    Alert::new(AlertTone::Warning).title("You're viewing an archived deal.")
}

fn empty_default() -> Empty {
    Empty::new("No deals yet")
        .icon(icons::INBOX)
        .description("Create a deal to start tracking your pipeline.")
        .actions(Button::primary("New deal").icon(icons::PLUS))
}

fn empty_search() -> Empty {
    Empty::new("Nothing matches “globx”")
        .icon(icons::SEARCH)
        .description("Check the spelling, or clear the filters.")
        .actions(Button::outline("Clear filters"))
}

fn progress_default() -> Stack {
    Stack::gap(2).children((
        Row::gap(2).justify(Justify::Between).children((
            Text::new("Importing contacts").inline(),
            Text::muted("640 of 1,000").inline(),
        )),
        Progress::new(640, 1000, "Importing contacts"),
    ))
}

fn progress_states() -> impl IntoIterator<Item = (&'static str, Progress)> {
    [
        ("Started", Progress::new(10, 100, "Upload")),
        ("Halfway", Progress::new(50, 100, "Upload")),
        ("Done", Progress::new(100, 100, "Upload")),
        ("No known end", Progress::indeterminate("Working")),
    ]
}

fn skeleton_default() -> Card {
    Card::new().body(Stack::gap(2).children((
        Skeleton::new(SkeletonShape::Short),
        Skeleton::new(SkeletonShape::Line),
        Skeleton::new(SkeletonShape::Line),
    )))
}

fn skeleton_shapes() -> impl IntoIterator<Item = (&'static str, Skeleton)> {
    [
        ("Line", Skeleton::new(SkeletonShape::Line)),
        ("Short", Skeleton::new(SkeletonShape::Short)),
        ("Block", Skeleton::new(SkeletonShape::Block)),
    ]
}

fn spinner_default() -> Spinner {
    Spinner::new("Loading deals")
}

fn spinner_inline() -> Row {
    Row::gap(2).children((
        Spinner::new("Syncing"),
        Text::muted("Syncing with the bank").inline(),
    ))
}
