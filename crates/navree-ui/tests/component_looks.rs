//! What the components that gained looks of their own draw when a page asks for none of
//! them: the segmented button group and the current button, the destructive outline and
//! solid buttons, the inverse card, stat and code block, the stepper's tiles and track, the
//! status dot, the plain timeline, the fact grid's narrower tiles, the popover's named
//! trigger and the thread list's groups.
//!
//! The pins were taken from the toolkit before any of these looks existed: the markup of
//! every variant, size and state each of these components had, and of a page holding all of
//! them (so the stylesheets it links are held too). A change to any of them is a change to
//! every app that asks for none of the new looks, so each is held here.

use navree_ui::genui::ThreadList;
use navree_ui::{
    Badge, BadgeVariant, Button, ButtonGroup, ButtonKind, ButtonVariant, Card, CardVariant,
    CodeBlock, FactCard, FactGrid, Markdown, Meta, Node, Popover, PopupAlign, Side, Size, Stat,
    Step, StepState, Stepper, StepperLabels, Tab, Tabs, TabsVariant, Timeline, TimelineEvent, Tone,
    Trend, View, el, icons, render,
};
use sha2::{Digest, Sha256};

fn sha(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// `html` with every run of exactly twelve hex digits (an asset's or a build's hash) made
/// `HASH`: what a page links changes with the toolkit's files, its markup must not.
fn unhashed(html: &str) -> String {
    let bytes = html.as_bytes();
    let hex = |b: u8| b.is_ascii_digit() || (b'a'..=b'f').contains(&b);
    let word = |b: u8| b.is_ascii_alphanumeric();
    let mut out = String::with_capacity(html.len());
    let mut at = 0;
    while at < bytes.len() {
        let run = bytes[at..].iter().take_while(|b| hex(**b)).count();
        let before = at.checked_sub(1).is_some_and(|i| word(bytes[i]));
        let after = bytes.get(at + run).is_some_and(|b| word(*b));
        if run == 12 && !before && !after {
            out.push_str("HASH");
            at += run;
        } else if run > 0 {
            out.push_str(&html[at..at + run]);
            at += run;
        } else {
            let len = html[at..].chars().next().map_or(1, char::len_utf8);
            out.push_str(&html[at..at + len]);
            at += len;
        }
    }
    out
}

fn all(nodes: Vec<Node>) -> String {
    nodes.iter().map(render).collect::<Vec<_>>().join("\n")
}

fn buttons() -> Vec<Node> {
    let mut out: Vec<Node> = [
        ButtonVariant::Primary,
        ButtonVariant::Secondary,
        ButtonVariant::Outline,
        ButtonVariant::Ghost,
        ButtonVariant::Destructive,
        ButtonVariant::Link,
    ]
    .into_iter()
    .flat_map(|variant| {
        [Size::Xs, Size::Sm, Size::Md, Size::Lg]
            .into_iter()
            .map(move |size| Button::new("Save").variant(variant).size(size).into())
    })
    .collect();
    out.extend([
        Button::primary("Create deal").icon(icons::PLUS).into(),
        Button::outline("Next").icon_end(icons::ARROW_RIGHT).into(),
        Button::ghost("Cancel").into(),
        Button::destructive("Delete").into(),
        Button::icon_only(icons::SETTINGS, "Settings").into(),
        Button::icon_only(icons::TRASH, "Delete")
            .variant(ButtonVariant::Destructive)
            .size(Size::Sm)
            .into(),
        Button::primary("Edit deal").href("/deals/42/edit").into(),
        Button::outline("Open").href("/deals").disabled(true).into(),
        Button::new("Mine").href("/deals/mine").prefetch().into(),
        Button::primary("Save").disabled(true).into(),
        Button::primary("Saving").pending(true).into(),
        Button::primary("Save")
            .submit()
            .loading_text("Saving…")
            .pending(true)
            .into(),
        Button::new("Reset").kind(ButtonKind::Reset).into(),
        Button::new("Full width").block().into(),
        Button::new("Open").opens_dialog("confirm").into(),
        Button::new("Close").closes_dialog("confirm").into(),
        Button::new("Archive")
            .named("intent", "archive")
            .submit()
            .into(),
        Button::new("Current")
            .href("/now")
            .attr("aria-current", "page")
            .into(),
    ]);
    out
}

fn button_groups() -> Vec<Node> {
    vec![
        ButtonGroup::new("View")
            .children((
                Button::outline("List").icon(icons::LIST),
                Button::outline("Board").icon(icons::KANBAN),
                Button::outline("Calendar").icon(icons::CALENDAR),
            ))
            .into(),
        ButtonGroup::new("Zoom")
            .vertical()
            .children((
                Button::icon_only(icons::PLUS, "Zoom in").variant(ButtonVariant::Outline),
                Button::icon_only(icons::MINUS, "Zoom out").variant(ButtonVariant::Outline),
            ))
            .into(),
        ButtonGroup::new("Mode")
            .id("mode")
            .children((
                Button::new("Mine")
                    .href("/deals/mine")
                    .attr("aria-current", "page"),
                Button::new("Team").href("/deals/team"),
            ))
            .into(),
    ]
}

fn tabs() -> Vec<Node> {
    let deal = || {
        Tabs::new("deal", "Deal sections")
            .tab(Tab::new("overview", "Overview").panel("A"))
            .tab(Tab::new("notes", "Notes").count("3").panel("B"))
            .tab(Tab::new("files", "Files").disabled(true).panel("C"))
    };
    vec![
        deal().into(),
        deal()
            .active("notes")
            .variant(TabsVariant::Segmented)
            .into(),
        deal()
            .variant(TabsVariant::Segmented)
            .size(Size::Sm)
            .stretch()
            .into(),
        deal().vertical().size(Size::Lg).into(),
    ]
}

fn cards() -> Vec<Node> {
    let mut out: Vec<Node> = [
        CardVariant::Default,
        CardVariant::Muted,
        CardVariant::Ghost,
        CardVariant::Dashed,
    ]
    .into_iter()
    .map(|variant| {
        Card::titled("Pipeline")
            .description("This quarter")
            .action(Button::ghost("Export"))
            .body("Twelve open deals.")
            .footer(Button::primary("Open"))
            .variant(variant)
            .into()
    })
    .collect();
    out.extend([
        Card::new().into(),
        Card::new().body("Only a body.").heading(3).into(),
        Card::titled("Seats").heading(4).id("seats").into(),
    ]);
    out
}

fn stats() -> Vec<Node> {
    vec![
        Stat::new("Open", "12").into(),
        Stat::new("Won", "3").trend("+2 this week", true).into(),
        Stat::new("Lost", "1")
            .trend("+1 this week", false)
            .caption("vs last week")
            .into(),
        Stat::new("Resting heart rate", "58")
            .icon(icons::HEART_PULSE)
            .tone(Tone::Danger)
            .unit("bpm")
            .status(Badge::new("Athletic").tone(Tone::Success))
            .change(Trend::new(-3.3).up_is_bad())
            .caption("vs last week")
            .into(),
        Stat::new("Invoices due", "₹9,12,400")
            .caption("this week")
            .href("/invoices")
            .into(),
        Stat::new("Sent", "1")
            .chart(el("span").child("chart"))
            .into(),
        Stat::new("Recovery", "82")
            .aside(el("span").child("ring"))
            .into(),
    ]
}

fn code_blocks() -> Vec<Node> {
    vec![
        CodeBlock::new("rust", "let total = 42;\n").into(),
        CodeBlock::new("sh", "navree dock up\nnavree dev")
            .copy_button()
            .into(),
        CodeBlock::new("", "plain text").into(),
        Markdown::new("Run it:\n\n```sh\nnavree dev\n```\n")
            .copy_code()
            .into(),
    ]
}

fn steppers() -> Vec<Node> {
    let three = || {
        Stepper::new("Setup")
            .step(Step::new("Account").link("/a"))
            .step(Step::new("Team").link("/t").description("Who is in"))
            .step(Step::new("Launch").link("/l").optional())
    };
    vec![
        three().current(1).into(),
        three().current(0).vertical().into(),
        three().current(2).no_count().id("steps").into(),
        three().into(),
        Stepper::new("Pay")
            .step(Step::new("Cart").state(StepState::Done))
            .step(Step::new("Card").state(StepState::Error).icon(icons::LOCK))
            .step(
                Step::new("Review")
                    .submit("_wizard", "goto:review")
                    .state(StepState::Disabled),
            )
            .step(Step::new("Paid").submit("_wizard", "goto:paid"))
            .labels(StepperLabels {
                current: "pas actuel".into(),
                ..StepperLabels::default()
            })
            .into(),
    ]
}

fn badges() -> Vec<Node> {
    let mut out: Vec<Node> = [
        BadgeVariant::Neutral,
        BadgeVariant::Outline,
        BadgeVariant::Success,
        BadgeVariant::Warning,
        BadgeVariant::Danger,
    ]
    .into_iter()
    .map(|variant| Badge::new("Won").variant(variant).into())
    .collect();
    out.extend([
        Badge::new("Delivered").tone(Tone::Success).dot().into(),
        Badge::new("Awaiting signature").dot().into(),
        Badge::new("Live").icon(icons::ACTIVITY).into(),
        Badge::outcome("Paid", true).tone(Tone::Success).into(),
        Badge::outcome("Shipped", false).into(),
    ]);
    out
}

fn timelines() -> Vec<Node> {
    vec![
        Timeline::new("steps", "What happened")
            .time_zone("Asia/Kolkata", 19_800)
            .event(
                TimelineEvent::new("Sent")
                    .at(1_793_592_667)
                    .tone(Tone::Success)
                    .detail("To 1,204 people"),
            )
            .event(
                TimelineEvent::new("Opened")
                    .at(1_793_592_727)
                    .tone(Tone::Info)
                    .icon(icons::MAIL)
                    .meta(Meta::icon(icons::MONITOR, "Safari on macOS")),
            )
            .event(TimelineEvent::new("Clicked").at(1_793_679_127).current())
            .event(TimelineEvent::new("Replied").upcoming().no_gap())
            .event(TimelineEvent::new("Indexed").muted().filtered())
            .note("Nothing else happened.")
            .into(),
        Timeline::new("deal", "Deal events")
            .day(
                "Today",
                [
                    TimelineEvent::new("Deal 4 signed")
                        .time_text("09:41")
                        .detail("Eight seats added"),
                    TimelineEvent::new("Rep assigned")
                        .time_text("09:12")
                        .gap("+29 min"),
                ],
            )
            .block("A callout")
            .newest_first()
            .heading(4)
            .into(),
        Timeline::new("empty", "Nothing yet").into(),
        Timeline::new("loading", "On its way").loading().into(),
    ]
}

fn fact_grids() -> Vec<Node> {
    let cards = || {
        [
            FactCard::new("Device", "Delivery handset")
                .icon(icons::MONITOR)
                .tone(Tone::Info)
                .detail("Chrome 130 on Android 15"),
            FactCard::new("Opens", "3"),
        ]
    };
    vec![
        FactGrid::new().cards(cards()).into(),
        FactGrid::new().cols(3).cards(cards()).into(),
        FactGrid::new()
            .id("facts")
            .card(FactCard::new("Clicks", "1"))
            .into(),
    ]
}

fn popovers() -> Vec<Node> {
    vec![
        Popover::new("filters", "Filters")
            .body("Pick a stage.")
            .into(),
        Popover::new("help", "Help")
            .variant(ButtonVariant::Ghost)
            .size(Size::Sm)
            .side(Side::Top)
            .align(PopupAlign::End)
            .label("About this page")
            .body("Read more.")
            .into(),
    ]
}

fn thread_lists() -> Vec<Node> {
    vec![
        ThreadList::new("Your conversations")
            .thread("Refunds this week", "/assistant/1", true)
            .thread("Orders from Pune", "/assistant/2", false)
            .into(),
        ThreadList::new("None yet").into(),
    ]
}

#[test]
fn every_variant_size_and_state_draws_what_it_drew_before_the_new_looks() {
    let pins: [(&str, Vec<Node>, &str); 12] = [
        (
            "buttons",
            buttons(),
            "3bb4f45c24f8bdb4b3e20b1143efef486387101506034b2a77f2c16dbf5cf61a",
        ),
        (
            "button groups",
            button_groups(),
            "68c948ddc2f2dd59e590d3c61acbd71aa469208a84253e7acba1bea9d4ece06e",
        ),
        (
            "tabs",
            tabs(),
            "8b6204c088b3fef93e87ee1609565c23ab1b791c1035aed63454a39e8d6f2c40",
        ),
        (
            "cards",
            cards(),
            "0f788436c2e235362f6ab796d7610c393c279375814004e9b9b656554fc05351",
        ),
        (
            "stats",
            stats(),
            "83e0ea554b67e6bbf7416028971e7d6549e34eda26158f316877533bf9ea38a3",
        ),
        (
            "code blocks",
            code_blocks(),
            "aad1329572860498cefb922109de225089d72dab6c73e7924302f3047c221ca1",
        ),
        (
            "steppers",
            steppers(),
            "8cd37068d0f233f605395b22204e0eb6c9b6c4ad3939e99deebf166b83f008ea",
        ),
        (
            "badges",
            badges(),
            "90dcd27457ffbc513c614da47ff61487d83d9f1440d3499a7407b51e0397bed6",
        ),
        (
            "timelines",
            timelines(),
            "adff739f43173975ab4b4b980c9e920cd2143180d4b6259f3edc836143e74243",
        ),
        (
            "fact grids",
            fact_grids(),
            "72616c8916f3b777c36bf0382112f5707fd712852eae8d55e297e225a746a45b",
        ),
        (
            "popovers",
            popovers(),
            "0a4de19844a4255f33198b4c4d306750293fa0e303f384abcb1999d61faebdf3",
        ),
        (
            "thread lists",
            thread_lists(),
            "bf2d9d2420e47c1bb5e1b045e31e15051183edac2e212ae018a8510bab19925f",
        ),
    ];
    let mut wrong = Vec::new();
    for (name, nodes, pin) in pins {
        let html = all(nodes);
        let got = sha(&html);
        if got != pin {
            wrong.push(format!("{name}: {got}\n{html}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n\n"));
}

/// A page holding every one of them: the same markup, and the same stylesheets and scripts
/// linked, so a component that asks for none of its new looks links no sheet it didn't.
#[test]
fn a_page_of_them_links_what_it_linked_before_the_new_looks() {
    let body: Vec<Node> = [
        buttons(),
        button_groups(),
        tabs(),
        cards(),
        stats(),
        code_blocks(),
        steppers(),
        badges(),
        timelines(),
        fact_grids(),
        popovers(),
        thread_lists(),
    ]
    .into_iter()
    .flatten()
    .collect();
    let html = unhashed(&View::new("Looks").body(body).to_html());
    assert_eq!(
        sha(&html),
        "e97a77f24be2fdf8635626b661df09bcdcae854ee60f21507fe1263e97ef9ea7",
        "{html}"
    );
}
