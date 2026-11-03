//! Stack, Row, Grid, Container, Accordion, Collapsible, ScrollArea, Separator,
//! AspectRatio, Direction.

use navree::ui::*;

use crate::catalog::{Example, gallery, single};

const SOURCE: &str = include_str!("layout.rs");

pub fn all() -> Vec<Example> {
    vec![
        single!("layout", "thumb", "Stack, Row and Grid", layout_thumb).start(),
        single!("layout", "stack", "A stack", layout_stack).start(),
        single!("layout", "row", "A row", layout_row).start(),
        gallery!("layout", "justify", "Sharing the space", layout_justify).wide(),
        single!("layout", "grid", "A grid", layout_grid)
            .start()
            .width(820),
        single!("layout", "span", "Spanning columns", layout_span)
            .start()
            .width(820),
        single!("layout", "container", "A container", layout_container).start(),
        single!("accordion", "default", "An accordion", accordion_default)
            .start()
            .height(200),
        single!(
            "accordion",
            "exclusive",
            "One open at a time",
            accordion_exclusive
        )
        .start()
        .height(200),
        single!(
            "collapsible",
            "default",
            "A collapsible",
            collapsible_default
        )
        .start(),
        single!(
            "scroll-area",
            "default",
            "A scroll area",
            scroll_area_default
        )
        .start(),
        single!("separator", "default", "Separators", separator_default).start(),
        single!("aspect-ratio", "default", "16:9", aspect_ratio_default).start(),
        gallery!("aspect-ratio", "ratios", "Ratios", aspect_ratio_ratios).medium(),
        single!("direction", "default", "Right to left", direction_default).start(),
    ]
}

fn layout_thumb() -> Stack {
    Stack::gap(2).children((
        Skeleton::new(SkeletonShape::Short),
        Row::gap(2).children((Badge::new("One"), Badge::new("Two"), Badge::new("Three"))),
        Skeleton::new(SkeletonShape::Line),
    ))
}

fn layout_stack() -> Stack {
    Stack::gap(3).children((
        Heading::h3("Acme renewal"),
        Text::muted("Proposal · closes 30 November"),
        Row::gap(2).children((Button::primary("Mark won"), Button::outline("Edit"))),
    ))
}

fn layout_row() -> Row {
    Row::gap(2).wrap().children((
        Badge::new("Lead"),
        Badge::new("Qualified"),
        Badge::new("Proposal"),
        Separator::vertical(),
        Button::ghost("Clear").size(Size::Sm),
    ))
}

fn layout_justify() -> impl IntoIterator<Item = (&'static str, Row)> {
    [
        Justify::Start,
        Justify::Center,
        Justify::End,
        Justify::Between,
    ]
    .map(|justify| {
        let name = match justify {
            Justify::Start => "Justify::Start",
            Justify::Center => "Justify::Center",
            Justify::End => "Justify::End",
            _ => "Justify::Between",
        };
        (
            name,
            Row::gap(2).justify(justify).children((
                Badge::new("One"),
                Badge::new("Two"),
                Badge::new("Three"),
            )),
        )
    })
}

fn layout_grid() -> Grid {
    Grid::cols(4).gap(3).children((
        Stat::new("Open deals", "312"),
        Stat::new("Pipeline", "$4.2M"),
        Stat::new("Won", "$640K"),
        Stat::new("Due today", "18"),
    ))
}

fn layout_span() -> Grid {
    Grid::cols(3).gap(3).children((
        Card::titled("Revenue").span(2).body(Bars::new(
            "Revenue by month, rising from 12 to 61",
            &[12, 18, 15, 24, 31, 28, 36, 41, 39, 48, 55, 61],
        )),
        Card::titled("Activity").body(Text::muted("Ada moved Acme renewal to Proposal")),
    ))
}

fn layout_container() -> Container {
    Container::new().size(ContainerSize::Sm).children(
        Card::titled("Sign in")
            .body(Field::new("login-email", "Email").control(Input::email("email"))),
    )
}

fn accordion_default() -> Accordion {
    Accordion::new()
        .open_item(
            "What does the plan include?",
            "Unlimited deals, 5 seats and the API.",
        )
        .item(
            "Can I change plans later?",
            "Yes, at any time. We prorate the difference.",
        )
        .item(
            "Where is my data stored?",
            "In the region you choose when you sign up.",
        )
}

fn accordion_exclusive() -> Accordion {
    Accordion::new()
        .exclusive("faq")
        .open_item("Billing", "Invoices go out on the 1st of each month.")
        .item("Seats", "Add or remove seats from Settings.")
        .item("Security", "Two-factor sign-in is on for every account.")
}

fn collapsible_default() -> Collapsible {
    Collapsible::new("Advanced settings").body(
        Stack::gap(2).children((
            Switch::new("audit")
                .label("Keep an audit log")
                .checked(true),
            Switch::new("webhooks").label("Send webhooks"),
        )),
    )
}

fn scroll_area_default() -> ScrollArea {
    ScrollArea::new("Activity").children(
        Stack::gap(2).children(
            [
                "Ada moved Acme renewal to Proposal",
                "Alan created Globex renewal",
                "Grace closed Initech expansion",
                "Ada added a note to Hooli pilot",
                "Alan changed the owner of Stark deal",
                "Grace archived Umbrella trial",
                "Ada uploaded contract-acme.pdf",
                "Alan invited Edsger to the workspace",
            ]
            .map(Text::muted),
        ),
    )
}

fn separator_default() -> Stack {
    Stack::gap(3).children((
        Text::new("Acme renewal"),
        Separator::horizontal(),
        Row::gap(3).children((
            Text::muted("Proposal").inline(),
            Separator::vertical(),
            Text::muted("$48,000").inline(),
            Separator::vertical(),
            Text::muted("Ada Lovelace").inline(),
        )),
    ))
}

fn aspect_ratio_default() -> AspectRatio {
    AspectRatio::new(
        Ratio::Video,
        Empty::new("No recording yet").icon(icons::PLAY),
    )
}

fn aspect_ratio_ratios() -> impl IntoIterator<Item = (&'static str, AspectRatio)> {
    [
        (
            "Square · 1:1",
            AspectRatio::new(Ratio::Square, Skeleton::new(SkeletonShape::Block)),
        ),
        (
            "Photo · 4:3",
            AspectRatio::new(Ratio::Photo, Skeleton::new(SkeletonShape::Block)),
        ),
        (
            "Video · 16:9",
            AspectRatio::new(Ratio::Video, Skeleton::new(SkeletonShape::Block)),
        ),
        (
            "Wide · 21:9",
            AspectRatio::new(Ratio::Wide, Skeleton::new(SkeletonShape::Block)),
        ),
    ]
}

fn direction_default() -> Direction {
    Direction::rtl(
        Field::new("rtl-name", "الاسم")
            .help("كما يظهر في الفواتير")
            .control(Input::text("name").value("شركة أكمي")),
    )
}
