//! Every component in its states: what the conformance suite runs against.

use std::sync::Arc;

use http::Method;
use navree::http::{Controller, RouteDef};
use navree::kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree::ui::*;

fn section(title: &'static str, content: impl IntoNodes) -> Element {
    el("section")
        .attr("data-gallery", title)
        .children((Heading::h2(title), Stack::gap(3).children(content)))
}

fn buttons() -> Element {
    let variants = [
        ButtonVariant::Primary,
        ButtonVariant::Secondary,
        ButtonVariant::Outline,
        ButtonVariant::Ghost,
        ButtonVariant::Destructive,
        ButtonVariant::Link,
    ];
    let rows: Vec<Row> = [Size::Xs, Size::Sm, Size::Md, Size::Lg]
        .into_iter()
        .map(|size| {
            Row::gap(2).wrap().children(variants.map(|variant| {
                Button::new("Save")
                    .variant(variant)
                    .size(size)
                    .icon(icons::PLUS)
            }))
        })
        .collect();
    section(
        "Buttons",
        (
            rows,
            Row::gap(2).wrap().children((
                Button::new("Disabled").disabled(true),
                Button::primary("Link").href("/plain"),
                Button::primary("Pending").pending(true),
                Button::icon_only(icons::SETTINGS, "Settings"),
                ButtonGroup::new("Text alignment").children((
                    Button::new("Left"),
                    Button::new("Centre"),
                    Button::new("Right"),
                )),
                Toggle::new("bold", "Bold").icon(icons::CHECK),
                Toggle::new("italic", "Italic").pressed(true),
                Toggle::new("off", "Off").disabled(true),
            )),
        ),
    )
}

fn inputs() -> Element {
    section(
        "Inputs",
        Grid::cols(2).gap(4).children((
            Field::new("g-text", "Text")
                .help("Help below the input.")
                .control(Input::text("text").placeholder("Placeholder")),
            Field::new("g-error", "Invalid")
                .error("This needs a value.")
                .control(Input::text("error")),
            Field::new("g-disabled", "Disabled")
                .control(Input::text("disabled").disabled(true).value("Can't change")),
            Field::new("g-readonly", "Read only")
                .control(Input::text("readonly").readonly().value("Can't change")),
            Field::new("g-required", "Required")
                .required()
                .control(Input::email("email").required()),
            Field::new("g-group", "In a group")
                .control(InputGroup::new(Input::text("amount")).start("$").end("USD")),
            Field::new("g-select", "Select").control(
                NativeSelect::new("select")
                    .prompt("Choose")
                    .option("1", "One")
                    .option("2", "Two"),
            ),
            Field::new("g-area", "Textarea")
                .control(Textarea::new("area").rows(3).value("Some text")),
            Stack::gap(2).children((
                Checkbox::new("c1").label("Unchecked"),
                Checkbox::new("c2").checked(true).label("Checked"),
                Checkbox::new("c3").disabled(true).label("Disabled"),
                Switch::new("s1").label("Off"),
                Switch::new("s2").checked(true).label("On"),
            )),
            RadioGroup::new("radio", "Radio group")
                .option("1", "One")
                .option("2", "Two")
                .selected("1"),
        )),
    )
}

fn display() -> Element {
    section(
        "Display",
        (
            (
                Row::gap(2).wrap().children((
                    Badge::new("Neutral"),
                    Badge::new("Outline").variant(BadgeVariant::Outline),
                    Badge::new("Success")
                        .variant(BadgeVariant::Success)
                        .icon(icons::CHECK),
                    Badge::new("Warning").variant(BadgeVariant::Warning),
                    Badge::new("Danger").variant(BadgeVariant::Danger),
                    Kbd::new("K"),
                    Spinner::new("Loading"),
                    Avatar::placeholder("Ada Lovelace"),
                    Avatar::placeholder("Small").size(Size::Sm),
                )),
                Alert::new(AlertTone::Neutral)
                    .title("Information")
                    .body("Something to know."),
                Alert::new(AlertTone::Success)
                    .title("Saved")
                    .body("Your changes are saved."),
                Alert::new(AlertTone::Warning)
                    .title("Careful")
                    .body("This takes effect at once."),
                Alert::new(AlertTone::Danger)
                    .title("Failed")
                    .body("That didn't work."),
                Grid::cols(2).gap(4).children((
                    Card::titled("A card")
                        .description("With a description")
                        .action(Button::outline("Act").size(Size::Sm))
                        .body("Its content.")
                        .footer(Button::new("Footer")),
                    Empty::new("No deals yet")
                        .icon(icons::INBOX)
                        .description("Create one to start.")
                        .actions(Button::primary("New deal")),
                )),
                Item::new("An item")
                    .description("With a description")
                    .media(components::icon(icons::FILE))
                    .variant(ItemVariant::Outline)
                    .actions(Button::ghost("Open")),
                Item::new("A link item")
                    .href("/plain")
                    .variant(ItemVariant::Muted),
                Breadcrumb::new()
                    .label("Example trail")
                    .link("Deals", "/deals")
                    .current("Acme"),
                Pagination::new(5, 10, |page| format!("/gallery?page={page}")),
                Progress::new(40, 100, "Upload"),
                Progress::indeterminate("Working"),
            ),
            (
                Skeleton::new(SkeletonShape::Line),
                Skeleton::new(SkeletonShape::Short),
                Separator::horizontal(),
                Row::gap(2).children(("A", Separator::vertical(), "B")),
                AspectRatio::new(Ratio::Wide, el("div").child("21:9")),
                Attachment::new("report.pdf").meta("2.4 MB").href("/plain"),
                Marker::new(MarkerVariant::Separator, "Today"),
                Marker::new(MarkerVariant::Border, "Earlier"),
                Message::new(BubbleSide::Start)
                    .avatar(Avatar::placeholder("Ada"))
                    .header("Ada")
                    .content(Bubble::new(BubbleSide::Start, "Hello"))
                    .footer("10:42"),
                Message::new(BubbleSide::End).content(Bubble::new(BubbleSide::End, "Hi there")),
            ),
            (
                Heading::h3("A heading"),
                Text::new("Body text."),
                Text::muted("Muted text."),
                Text::new("Lead text.").variant(TextVariant::Lead),
                Text::code("code()"),
                Direction::rtl(Text::new("מימין לשמאל")),
                Table::new()
                    .caption("A plain table")
                    .head((TableHead::new("Name"), TableHead::new("Amount").numeric()))
                    .rows((
                        TableRowEl::new((
                            TableCell::new("Acme"),
                            TableCell::new("1,200.00").numeric(),
                        )),
                        TableRowEl::new((
                            TableCell::new("Globex"),
                            TableCell::new("3,400.00").numeric(),
                        ))
                        .selected(true),
                    )),
            ),
        ),
    )
}

fn overlays() -> Element {
    section(
        "Overlays",
        (
            Row::gap(2).wrap().children((
                Button::outline("Open dialog").opens_dialog("g-dialog"),
                Button::outline("Open alert").opens_dialog("g-alert"),
                Button::outline("Open sheet").opens_dialog("g-sheet"),
                Button::outline("Open drawer").opens_dialog("g-drawer"),
                Popover::new("g-popover", "Open popover")
                    .label("More")
                    .body("Popover content."),
                HoverCard::new("g-hover", el("a").href("/plain").child("Hover card"))
                    .body("More about this link."),
                Tooltip::new("g-tooltip", Button::outline("Tooltip"), "Says more"),
            )),
            Accordion::new()
                .exclusive("g")
                .open_item("First", "First content")
                .item("Second", "Second content"),
            Collapsible::new("Collapsible").body("Collapsible content"),
            ScrollArea::new("Log").children(
                Stack::gap(1).children(["Line one", "Line two", "Line three"].map(Text::muted)),
            ),
            Dialog::new("g-dialog", "A dialog")
                .description("With a description")
                .body(Field::new("g-d-name", "Name").control(Input::text("name")))
                .footer((
                    Button::outline("Cancel").closes_dialog("g-dialog"),
                    Button::primary("Save"),
                )),
            AlertDialog::new("g-alert", "Delete this deal?", "This can't be undone.")
                .action(Button::destructive("Delete").closes_dialog("g-alert")),
            Sheet::new("g-sheet", "A sheet")
                .side(SheetSide::Right)
                .body("Sheet content")
                .footer(Button::outline("Close").closes_dialog("g-sheet")),
            Drawer::new("g-drawer", "A drawer")
                .body("Drawer content")
                .footer(Button::outline("Close").closes_dialog("g-drawer")),
        ),
    )
}

/// A row's details in a dialog, filled from the row with no request.
fn commands_fill() -> (Table, Dialog) {
    let row = |key: &'static str, title: &'static str, amount: &'static str| {
        el("tr").attr("data-key", key).children((
            el("td").child(title),
            el("td").child(amount),
            el("td").child(
                Button::ghost("Details")
                    .size(Size::Sm)
                    .attr("id", format!("g-fill-{key}"))
                    .on_click(Cmd::fill("g-fill")),
            ),
        ))
    };
    (
        Table::new()
            .caption("Deals")
            .head((
                el("th").attr("data-col", "title").child("Deal"),
                el("th").attr("data-col", "amount").child("Amount"),
                el("th").child(el("span").attr("data-ui-sr", "").child("Actions")),
            ))
            .rows((
                row("7", "Acme renewal", "12,500"),
                row("8", "Globex expansion", "48,000"),
            )),
        Dialog::new("g-fill", "Deal").body(
            el("form").attr("method", "dialog").children((
                el("p").children((
                    el("strong").attr("id", "g-fill-name").fill_cell("title"),
                    ", worth ",
                    el("span").attr("id", "g-fill-sum").fill_cell("amount"),
                    ".",
                )),
                el("input")
                    .attr("type", "hidden")
                    .attr("id", "g-fill-key")
                    .attr("name", "id")
                    .fill_key(),
            )),
        ),
    )
}

/// `GET /gallery`.
fn page(shell: &AppShell) -> AppPage {
    shell.page("Gallery").body(
        Stack::gap(8).children((
            buttons(),
            inputs(),
            display(),
            overlays(),
            section(
                "Commands",
                (
                    Row::gap(2).children((
                        Button::outline("Toggle")
                            .attr("id", "g-toggle-button")
                            .on_click(Cmd::toggle("g-toggled")),
                        el("p")
                            .attr("id", "g-toggled")
                            .child("Toggled by a command, with no request."),
                    )),
                    Row::gap(2).children((
                        Button::outline("Monthly")
                            .attr("id", "g-monthly")
                            .on_click(Cmd::text("g-price", "$12 a month")),
                        Button::outline("Yearly")
                            .attr("id", "g-yearly")
                            .on_click(Cmd::text("g-price", "$120 a year")),
                        el("strong").attr("id", "g-price").child("$12 a month"),
                    )),
                    Field::new("g-bio", "Bio").control(
                        Textarea::new("bio")
                            .attr("maxlength", 160_u32)
                            .on(Trigger::Input, Cmd::count("g-bio-count", 160)),
                    ),
                    Text::muted("0/160").id("g-bio-count"),
                    commands_fill(),
                ),
            ),
        )),
    )
}

pub struct GalleryPages;

impl Injectable for GalleryPages {
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Controller for GalleryPages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "!/gallery", |shell: AppShell| async move {
                page(&shell)
            })
            .public(),
        ]
    }
}
