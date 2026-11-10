//! Card, Badge, Avatar, Item, Heading and Text, Kbd, Attachment, Message.

use navree::ui::*;

use crate::catalog::{Example, gallery, single};

const SOURCE: &str = include_str!("display.rs");

pub fn all() -> Vec<Example> {
    vec![
        single!("card", "thumb", "A card", card_thumb).start(),
        single!("card", "default", "A card", card_default)
            .start()
            .height(220),
        single!("card", "stats", "Cards in a grid", card_grid)
            .start()
            .width(820),
        single!("card", "plain", "Body only", card_plain).start(),
        single!("badge", "default", "Badges", badge_default),
        gallery!("badge", "variants", "Variants", badge_variants).narrow(),
        single!("badge", "icon", "With an icon", badge_icon),
        single!("avatar", "default", "Avatars", avatar_default),
        gallery!("avatar", "sizes", "Sizes", avatar_sizes).narrow(),
        single!("item", "thumb", "An item", item_thumb).start(),
        single!("item", "default", "Items", item_default).start(),
        gallery!("item", "variants", "Variants", item_variants).wide(),
        single!("item", "link", "A row that is a link", item_link).start(),
        single!("typography", "thumb", "Heading and text", typography_thumb).start(),
        single!("typography", "headings", "Headings", typography_headings).start(),
        gallery!("typography", "text", "Text", typography_text).wide(),
        single!("kbd", "default", "Keys", kbd_default),
        single!("attachment", "default", "A file", attachment_default).start(),
        single!("attachment", "action", "With an action", attachment_action).start(),
        single!("message", "thumb", "A conversation", message_thumb).start(),
        single!("message", "default", "A conversation", message_default)
            .start()
            .height(260),
        gallery!("message", "markers", "Markers", message_markers).wide(),
    ]
}

fn card_thumb() -> Card {
    Card::titled("Sutlej Supply renewal")
        .description("Proposal · ₹48,000")
        .action(Badge::new("Hot").variant(BadgeVariant::Warning))
}

fn card_default() -> Card {
    Card::titled("Sutlej Supply renewal")
        .description("Annual platform license, 120 seats.")
        .action(Button::ghost("Edit").size(Size::Sm).icon(icons::PENCIL))
        .body(Text::new(
            "Procurement asked for a two-year term at the same price.",
        ))
        .footer((
            Button::outline("Mark lost"),
            Button::primary("Mark won").icon(icons::CHECK),
        ))
}

fn card_grid() -> Grid {
    Grid::cols(3).gap(3).children((
        Card::titled("Open deals").body(Stat::new("Count", "312")),
        Card::titled("Pipeline").body(Stat::new("Value", "₹4.2 crore")),
        Card::titled("Won").body(Stat::new("This month", "₹64 lakh")),
    ))
}

fn card_plain() -> Card {
    Card::new().body(Text::muted(
        "A card with no header holds anything: a form, a table, a chart.",
    ))
}

fn badge_default() -> Row {
    Row::gap(2).wrap().children((
        Badge::new("Draft"),
        Badge::new("Won").variant(BadgeVariant::Success),
        Badge::new("Overdue").variant(BadgeVariant::Danger),
    ))
}

fn badge_variants() -> impl IntoIterator<Item = (&'static str, Badge)> {
    [
        ("Neutral", Badge::new("Lead")),
        ("Outline", Badge::new("Lead").variant(BadgeVariant::Outline)),
        ("Success", Badge::new("Won").variant(BadgeVariant::Success)),
        (
            "Warning",
            Badge::new("At risk").variant(BadgeVariant::Warning),
        ),
        ("Danger", Badge::new("Lost").variant(BadgeVariant::Danger)),
    ]
}

fn badge_icon() -> Row {
    Row::gap(2).children((
        Badge::new("Verified")
            .variant(BadgeVariant::Success)
            .icon(icons::BADGE_CHECK),
        Badge::new("3 days").icon(icons::CLOCK),
    ))
}

fn avatar_default() -> Row {
    Row::gap(2).children((
        Avatar::placeholder("Gurpreet Singh"),
        Avatar::placeholder("Harleen Kaur"),
        Avatar::placeholder("Manpreet Gill"),
    ))
}

fn avatar_sizes() -> impl IntoIterator<Item = (&'static str, Avatar)> {
    [
        ("Xs", Avatar::placeholder("Gurpreet Singh").size(Size::Xs)),
        ("Sm", Avatar::placeholder("Gurpreet Singh").size(Size::Sm)),
        ("Md", Avatar::placeholder("Gurpreet Singh")),
        ("Lg", Avatar::placeholder("Gurpreet Singh").size(Size::Lg)),
    ]
}

fn item_thumb() -> Item {
    Item::new("Gurpreet Singh")
        .description("gurpreet@example.com")
        .media(Avatar::placeholder("Gurpreet Singh"))
        .variant(ItemVariant::Outline)
}

fn item_default() -> Stack {
    Stack::gap(2).children((
        Item::new("Gurpreet Singh")
            .description("Owner · gurpreet@example.com")
            .media(Avatar::placeholder("Gurpreet Singh"))
            .variant(ItemVariant::Outline)
            .actions(Button::ghost("Remove").size(Size::Sm)),
        Item::new("Harleen Kaur")
            .description("Member · harleen@example.com")
            .media(Avatar::placeholder("Harleen Kaur"))
            .variant(ItemVariant::Outline)
            .actions(Button::ghost("Remove").size(Size::Sm)),
    ))
}

fn item_variants() -> impl IntoIterator<Item = (&'static str, Item)> {
    [
        (
            "Plain",
            Item::new("Two-factor sign-in")
                .description("Ask for a code at every sign-in.")
                .media(components::icon(icons::SHIELD_CHECK)),
        ),
        (
            "Outline",
            Item::new("Two-factor sign-in")
                .description("Ask for a code at every sign-in.")
                .media(components::icon(icons::SHIELD_CHECK))
                .variant(ItemVariant::Outline),
        ),
        (
            "Muted",
            Item::new("Two-factor sign-in")
                .description("Ask for a code at every sign-in.")
                .media(components::icon(icons::SHIELD_CHECK))
                .variant(ItemVariant::Muted),
        ),
    ]
}

fn item_link() -> Item {
    Item::new("Sutlej Supply renewal")
        .description("Proposal · closes 30 November")
        .media(components::icon(icons::HANDSHAKE))
        .variant(ItemVariant::Outline)
        .href("/deals/42")
}

fn typography_thumb() -> Stack {
    Stack::gap(1).children((
        Heading::h3("Quarterly review"),
        Text::muted("Pipeline, wins and losses."),
    ))
}

fn typography_headings() -> Stack {
    Stack::gap(3).children((
        Heading::h1("Deals"),
        Heading::h2("Open this quarter"),
        Heading::h3("Sutlej Supply renewal"),
        Heading::h4("Notes"),
    ))
}

fn typography_text() -> impl IntoIterator<Item = (&'static str, Text)> {
    [
        (
            "Lead",
            Text::new("Track every opportunity from first call to signature.")
                .variant(TextVariant::Lead),
        ),
        ("Body", Text::new("Procurement asked for a two-year term.")),
        ("Muted", Text::muted("Updated 3 minutes ago")),
        ("Code", Text::code("deal.amount")),
    ]
}

fn kbd_default() -> Row {
    Row::gap(1).children((Kbd::new("Ctrl"), "+", Kbd::new("K")))
}

fn attachment_default() -> Attachment {
    Attachment::new("proposal-sutlej.pdf")
        .meta("PDF · 2.4 MB")
        .href("/files/proposal-sutlej.pdf")
}

fn attachment_action() -> Attachment {
    Attachment::new("contract-draft.docx")
        .meta("Word · 86 KB")
        .icon(icons::FILE_TEXT)
        .action(Button::icon_only(icons::X, "Remove").variant(ButtonVariant::Ghost))
}

fn message_thumb() -> Stack {
    Stack::gap(2).children((
        Bubble::new(BubbleSide::Start, "Call at 10?"),
        Bubble::new(BubbleSide::End, "Works for me."),
    ))
}

fn message_default() -> Stack {
    Stack::gap(3).children((
        Marker::new(MarkerVariant::Separator, "Today"),
        Message::new(BubbleSide::Start)
            .avatar(Avatar::placeholder("Simran Kaur"))
            .header("Simran Kaur")
            .content(Bubble::new(
                BubbleSide::Start,
                "Procurement signed off. Can we send the contract?",
            ))
            .footer("10:42"),
        Message::new(BubbleSide::End)
            .content((
                Bubble::new(BubbleSide::End, "Sending it now."),
                Attachment::new("contract-sutlej.pdf").meta("PDF · 310 KB"),
            ))
            .footer("10:44"),
    ))
}

fn message_markers() -> impl IntoIterator<Item = (&'static str, Marker)> {
    [
        (
            "Plain",
            Marker::new(MarkerVariant::Plain, "Harleen joined the chat"),
        ),
        (
            "Separator",
            Marker::new(MarkerVariant::Separator, "Yesterday"),
        ),
        ("Border", Marker::new(MarkerVariant::Border, "Unread")),
    ]
}
