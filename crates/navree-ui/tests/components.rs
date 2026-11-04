//! Every component in its states: the attribute contract holds, nothing breaks
//! the content security policy, and text is escaped.

use navree_ui::*;

/// One of everything, in every state a variant or flag can put it in.
fn all() -> Vec<(&'static str, Node)> {
    let mut out: Vec<(&'static str, Node)> = Vec::new();
    for variant in [
        ButtonVariant::Primary,
        ButtonVariant::Secondary,
        ButtonVariant::Outline,
        ButtonVariant::Ghost,
        ButtonVariant::Destructive,
        ButtonVariant::Link,
    ] {
        for size in [Size::Xs, Size::Sm, Size::Md, Size::Lg] {
            out.push((
                "button",
                Button::new("Go")
                    .variant(variant)
                    .size(size)
                    .icon(icons::PLUS)
                    .into(),
            ));
        }
    }
    out.push(("button", Button::primary("Go").href("/x").into()));
    out.push(("button", Button::new("Go").disabled(true).into()));
    out.push(("button", Button::new("Go").submit().pending(true).into()));
    out.push(("button", Button::icon_only(icons::X, "Close").into()));
    out.push((
        "button-group",
        ButtonGroup::new("Actions")
            .children((Button::new("A"), Button::new("B")))
            .into(),
    ));
    out.push((
        "toggle",
        Toggle::new("bold", "Bold")
            .pressed(true)
            .icon(icons::CHECK)
            .into(),
    ));
    out.push((
        "alert",
        Alert::new(AlertTone::Danger)
            .title("No")
            .body("Nope")
            .into(),
    ));
    out.push(("alert", Alert::new(AlertTone::Success).title("Yes").into()));
    out.push(("aspect-ratio", AspectRatio::new(Ratio::Video, "x").into()));
    out.push((
        "attachment",
        Attachment::new("a.pdf").meta("2 MB").href("/a.pdf").into(),
    ));
    out.push(("avatar", Avatar::image("/a.png", "Ada").into()));
    out.push(("avatar", Avatar::placeholder("Ada").size(Size::Sm).into()));
    out.push((
        "badge",
        Badge::new("New").variant(BadgeVariant::Success).into(),
    ));
    out.push((
        "breadcrumb",
        Breadcrumb::new()
            .link("Deals", "/deals")
            .current("Acme")
            .into(),
    ));
    out.push(("bubble", Bubble::new(BubbleSide::End, "hi").into()));
    out.push((
        "card",
        Card::titled("T")
            .description("D")
            .action(Button::new("A"))
            .body("B")
            .footer("F")
            .into(),
    ));
    out.push(("direction", Direction::rtl("x").into()));
    out.push((
        "empty",
        Empty::new("Nothing")
            .icon(icons::INBOX)
            .description("Yet")
            .actions(Button::new("Add"))
            .into(),
    ));
    out.push((
        "item",
        Item::new("T")
            .description("D")
            .media(components::icon(icons::FILE))
            .variant(ItemVariant::Outline)
            .into(),
    ));
    out.push(("kbd", Kbd::new("K").into()));
    out.push((
        "marker",
        Marker::new(MarkerVariant::Separator, "Today").into(),
    ));
    out.push((
        "message",
        Message::new(BubbleSide::Start)
            .header("Ada")
            .content(Bubble::new(BubbleSide::Start, "hi"))
            .footer("Now")
            .into(),
    ));
    out.push((
        "pagination",
        Pagination::new(5, 10, |page| format!("?page={page}")).into(),
    ));
    out.push(("progress", Progress::new(3, 10, "Upload").into()));
    out.push(("progress", Progress::indeterminate("Upload").into()));
    out.push(("separator", Separator::horizontal().into()));
    out.push(("separator", Separator::vertical().into()));
    out.push(("skeleton", Skeleton::new(SkeletonShape::Short).into()));
    out.push(("spinner", Spinner::new("Loading").into()));
    out.push((
        "table-container",
        Table::new()
            .caption("C")
            .head(TableHead::new("A"))
            .rows(TableRowEl::new(TableCell::new("a")))
            .into(),
    ));
    out.push(("heading", Heading::h2("T").into()));
    out.push(("text", Text::muted("T").into()));
    out.push((
        "input",
        Input::email("e")
            .placeholder("a@b.c")
            .required()
            .invalid(true)
            .into(),
    ));
    out.push(("textarea", Textarea::new("t").rows(3).value("x").into()));
    out.push((
        "native-select",
        NativeSelect::new("s")
            .option("1", "One")
            .selected("1")
            .into(),
    ));
    out.push(("checkbox", Checkbox::new("c").checked(true).into()));
    out.push(("choice", Checkbox::new("c").label("Yes").into()));
    out.push(("switch", Switch::new("s").checked(true).into()));
    out.push((
        "radio-group",
        RadioGroup::new("r", "Pick")
            .option("1", "One")
            .selected("1")
            .into(),
    ));
    out.push(("label", Label::new("x", "X").into()));
    out.push((
        "field",
        Field::new("x", "X")
            .help("H")
            .error("E")
            .control(Input::text("x"))
            .into(),
    ));
    out.push((
        "field-set",
        FieldSet::new("L")
            .children(Field::new("x", "X").control(Input::text("x")))
            .into(),
    ));
    out.push((
        "input-group",
        InputGroup::new(Input::text("x")).end("USD").into(),
    ));
    out.push((
        "dialog",
        Dialog::new("d", "T")
            .description("D")
            .body("B")
            .footer(Button::new("OK"))
            .into(),
    ));
    out.push((
        "alert-dialog",
        AlertDialog::new("a", "T", "D")
            .action(Button::destructive("Delete"))
            .into(),
    ));
    out.push((
        "sheet",
        Sheet::new("s", "T").side(SheetSide::Left).body("B").into(),
    ));
    out.push((
        "sheet",
        Sheet::new("w", "T").width(SheetWidth::Lg).body("B").into(),
    ));
    out.push(("drawer", Drawer::new("dr", "T").body("B").into()));
    out.push((
        "accordion",
        Accordion::new()
            .exclusive("a")
            .item("T", "B")
            .open_item("U", "C")
            .into(),
    ));
    out.push((
        "collapsible",
        Collapsible::new("T").open(true).body("B").into(),
    ));
    out.push(("scroll-area", ScrollArea::new("Log").children("x").into()));
    out.push(("stack", Stack::gap(4).children("x").into()));
    out.push((
        "row",
        Row::gap(2)
            .justify(Justify::Between)
            .wrap()
            .children("x")
            .into(),
    ));
    out.push(("grid", Grid::cols(3).gap(2).children("x").into()));
    out.push((
        "container",
        Container::new()
            .size(ContainerSize::Sm)
            .children("x")
            .into(),
    ));
    out.push((
        "tabs",
        Tabs::new("t", "Sections")
            .variant(TabsVariant::Segmented)
            .tab(Tab::new("a", "A").icon(icons::STAR).count("3").panel("x"))
            .tab(Tab::new("b", "B").lazy("/b"))
            .tab(Tab::new("c", "C").disabled(true))
            .into(),
    ));
    out.push((
        "toggle-group",
        ToggleGroup::multiple("days", "Days")
            .item(ToggleItem::new("mon", "Mon"))
            .item(ToggleItem::icon_only("tue", icons::STAR, "Tuesday"))
            .values(["mon"])
            .variant(ToggleVariant::Outline)
            .into(),
    ));
    out.push((
        "dropdown-menu",
        DropdownMenu::new("m", "Actions")
            .item(MenuItem::link("Edit", "/e").icon(icons::STAR).shortcut("E"))
            .item(MenuItem::post("Delete", "/d").destructive())
            .separator()
            .group(MenuGroup::new("View").item(MenuItem::action("Copy").disabled(true)))
            .checkbox(MenuCheckbox::new("a", "Archived").checked(true))
            .radios(MenuRadios::new("s", "Sort").option("n", "Name").value("n"))
            .submenu(MenuSub::new("Move").item(MenuItem::link("Inbox", "/i")))
            .into(),
    ));
    out.push((
        "toaster",
        Toaster::new()
            .toast(
                Toast::new(ToastTone::Danger, "No")
                    .title("T")
                    .action(ToastAction::post("Retry", "/r")),
            )
            .into(),
    ));
    out
}

#[test]
fn every_component_keeps_the_attribute_contract() {
    for (slot, node) in all() {
        let html = render(&node);
        assert!(
            html.contains(&format!("data-slot=\"{slot}\"")),
            "{slot}: {html}"
        );
        assert!(!html.contains("style="), "{slot}: {html}");
        assert!(!html.contains("class="), "{slot} uses no classes: {html}");
        assert!(!html.contains(" onclick"), "{slot}: {html}");
    }
}

#[test]
fn overlays_use_the_browser() {
    assert!(render(&Dialog::new("d", "T").into()).starts_with("<dialog "));
    let html = render(&Popover::new("p", "Open").body("x").into());
    assert!(html.contains(r#"popovertarget="p""#), "{html}");
    assert!(html.contains(r#"popover="auto""#), "{html}");
    let html = render(&Tooltip::new("t", Button::new("x"), "Tip").into());
    assert!(html.contains(r#"role="tooltip""#), "{html}");
    assert!(html.contains(r#"aria-describedby="t""#), "{html}");
    let html = render(&Accordion::new().exclusive("g").item("T", "B").into());
    assert!(
        html.contains(r#"<details data-slot="accordion-item" name="g">"#),
        "{html}"
    );
}

#[test]
fn a_sheet_is_24_rem_unless_asked_wider() {
    let narrow = render(&Sheet::new("s", "T").into());
    assert!(!narrow.contains("data-width"), "{narrow}");
    let wide = render(&Sheet::new("s", "T").width(SheetWidth::Lg).into());
    assert!(
        wide.contains(r#"data-side="right" data-width="lg""#),
        "{wide}"
    );
    let css = include_str!("../styles/30-overlays.css");
    assert!(css.contains("&[data-width=lg]{width:min(40rem,calc(100% - 3rem))}"));
}

#[test]
fn a_sheet_dims_the_page_behind_unless_asked_to_keep_it_readable() {
    let dim = render(&Sheet::new("s", "T").into());
    assert!(!dim.contains("data-backdrop"), "{dim}");
    let light = render(
        &Sheet::new("deal", "T")
            .backdrop(SheetBackdrop::Light)
            .into(),
    );
    assert!(light.contains(r#"data-backdrop="light""#), "{light}");
    // Still the browser's modal dialog: lighter, never interactive behind.
    assert!(light.starts_with("<dialog"), "{light}");
    let css = include_str!("../styles/30-overlays.css");
    assert!(
        css.contains("&::backdrop{background:var(--scrim,rgb(1 3 2/.72))}"),
        "the default backdrop is the mode's scrim"
    );
    assert!(
        css.contains(
            "&[data-backdrop=light]::backdrop{background:color-mix(in srgb,var(--scrim,rgb(1 3 2/.72)) 55%,transparent)}"
        ),
        "the light backdrop is part of the same scrim, in light and dark"
    );
}

#[test]
fn an_extra_wide_sheet_takes_sixty_percent_and_never_less_than_a_large_one() {
    let xl = render(&Sheet::new("order", "T").width(SheetWidth::Xl).into());
    assert!(xl.contains(r#"data-side="right" data-width="xl""#), "{xl}");
    // 60% of the screen, at least 40 rem, and never past the screen less 3 rem: on a
    // phone it is as wide as every other sheet.
    let css = include_str!("../styles/30-overlays.css");
    assert!(
        css.contains("&[data-width=xl]{width:min(max(40rem,60%),calc(100% - 3rem))}"),
        "the rule for an extra-wide sheet"
    );
}

#[test]
fn a_scroll_area_can_be_given_a_height_or_a_largest_height() {
    let plain = render(&ScrollArea::new("Log").into());
    assert!(!plain.contains("data-height"), "{plain}");
    assert!(!plain.contains("data-max-height"), "{plain}");
    let css = include_str!("../styles/30-overlays.css");
    for (height, value) in [
        (ScrollHeight::Sm, "sm"),
        (ScrollHeight::Md, "md"),
        (ScrollHeight::Lg, "lg"),
        (ScrollHeight::Xl, "xl"),
        (ScrollHeight::Fill, "fill"),
    ] {
        let fixed = render(&ScrollArea::new("Log").height(height).into());
        assert!(
            fixed.contains(&format!(r#"data-height="{value}""#)),
            "{fixed}"
        );
        assert!(!fixed.contains("data-max-height"), "{fixed}");
        let capped = render(&ScrollArea::new("Log").max_height(height).into());
        assert!(
            capped.contains(&format!(r#"data-max-height="{value}""#)),
            "{capped}"
        );
        assert!(!capped.contains(" data-height"), "{capped}");
        // The stylesheet knows both, and no style attribute is needed.
        assert!(css.contains(&format!("&[data-height={value}]")), "{value}");
        assert!(
            css.contains(&format!("&[data-max-height={value}]")),
            "{value}"
        );
        assert!(!fixed.contains("style="), "{fixed}");
    }
    // The last of the two wins, so a content security policy with no `style` attribute
    // never meets two heights at once.
    let last = render(
        &ScrollArea::new("Log")
            .height(ScrollHeight::Lg)
            .max_height(ScrollHeight::Sm)
            .into(),
    );
    assert!(last.contains(r#"data-max-height="sm""#), "{last}");
    assert!(!last.contains(" data-height"), "{last}");
    let last = render(
        &ScrollArea::new("Log")
            .max_height(ScrollHeight::Sm)
            .height(ScrollHeight::Lg)
            .into(),
    );
    assert!(last.contains(r#"data-height="lg""#), "{last}");
    assert!(!last.contains("data-max-height"), "{last}");
    // Auto is no choice: the area is as tall as its content.
    let auto = render(&ScrollArea::new("Log").max_height(ScrollHeight::Auto).into());
    assert!(!auto.contains("data-max-height"), "{auto}");
}

#[test]
fn overlays_that_need_placing_ask_for_the_module_and_dialogs_ask_for_nothing() {
    assert!(
        View::new("x")
            .body(Dialog::new("d", "T").body("B"))
            .uses()
            .is_none()
    );
    assert!(
        View::new("x")
            .body(Accordion::new().item("T", "B"))
            .uses()
            .is_none()
    );
    assert!(
        View::new("x")
            .body(Button::new("x").opens_dialog("d"))
            .uses()
            .is_none()
    );
    assert!(
        View::new("x")
            .body(Popover::new("p", "x"))
            .uses()
            .has(Uses::OVERLAY)
    );
    assert!(
        View::new("x")
            .body(Tooltip::new("t", Button::new("x"), "x"))
            .uses()
            .has(Uses::OVERLAY)
    );
}

#[test]
fn text_is_escaped_in_every_component_that_takes_it() {
    let evil = "<img src=x onerror=alert(1)>";
    for node in [
        Node::from(Button::new(evil.to_owned())),
        Badge::new(evil.to_owned()).into(),
        Card::titled(evil.to_owned()).into(),
        Dialog::new("d", evil.to_owned()).into(),
        Field::new("x", evil.to_owned())
            .error(evil)
            .control(Input::text("x").value(evil))
            .into(),
        Text::new(evil.to_owned()).into(),
        Breadcrumb::new().current(evil.to_owned()).into(),
    ] {
        let html = render(&node);
        assert!(!html.contains("<img"), "{html}");
    }
}

#[test]
fn a_field_ties_its_control_through_a_group() {
    let html = render(
        &Field::new("amount", "Value")
            .help("In cents")
            .error("Too much")
            .control(InputGroup::new(Input::text("amount")).end("USD"))
            .into(),
    );
    assert!(html.contains(r#"for="amount""#), "{html}");
    assert!(html.contains(r#"id="amount""#), "{html}");
    assert!(
        html.contains(r#"aria-describedby="amount-help amount-error""#),
        "{html}"
    );
    assert!(html.contains(r#"aria-invalid="true""#), "{html}");
    assert!(html.contains("data-invalid"), "{html}");
}

#[test]
fn pagination_shows_the_near_pages_and_is_safe_at_the_edges() {
    let html = render(&Pagination::new(5, 10, |page| format!("?page={page}")).into());
    for page in [1, 4, 5, 6, 10] {
        assert!(
            html.contains(&format!(r#"aria-label="Page {page}""#)),
            "{html}"
        );
    }
    assert!(!html.contains(r#"aria-label="Page 2""#), "{html}");
    assert_eq!(html.matches('…').count(), 2, "{html}");
    let html = render(&Pagination::new(99, 0, |page| format!("?page={page}")).into());
    assert!(
        html.contains(r#"aria-label="Previous page" role="link" aria-disabled="true""#),
        "{html}"
    );
    assert!(
        html.contains(r#"aria-label="Next page" role="link" aria-disabled="true""#),
        "{html}"
    );
}

#[test]
fn menus_ask_for_their_module_and_a_plain_tab_asks_for_nothing_more() {
    let uses = |node: Node| View::new("x").body(node).uses();
    assert!(uses(Tabs::new("t", "T").tab(Tab::new("a", "A")).into()).has(Uses::MENUS));
    assert!(uses(ToggleGroup::single("v", "V").into()).has(Uses::MENUS));
    let menu = uses(
        DropdownMenu::new("m", "A")
            .item(MenuItem::link("x", "/x"))
            .into(),
    );
    assert!(menu.has(Uses::MENUS) && menu.has(Uses::OVERLAY));
}

#[test]
fn a_menu_is_a_popover_of_the_menu_role_and_a_tab_list_is_a_tablist() {
    let html = render(
        &DropdownMenu::new("m", "A")
            .item(MenuItem::link("x", "/x"))
            .into(),
    );
    assert!(html.contains(r#"popover="auto" role="menu""#), "{html}");
    let html = render(&Tabs::new("t", "T").tab(Tab::new("a", "A")).into());
    assert!(
        html.contains(r#"role="tablist""#) && html.contains(r#"role="tab""#),
        "{html}"
    );
}

#[test]
fn an_icon_button_must_have_a_name() {
    assert!(render(&Button::icon_only(icons::X, "Close").into()).contains(r#"aria-label="Close""#));
}

#[test]
fn layout_gaps_snap_to_the_scale() {
    assert!(render(&Stack::gap(7).into()).contains(r#"data-gap="8""#));
    assert!(render(&Stack::gap(200).into()).contains(r#"data-gap="16""#));
    assert!(render(&Grid::cols(9).into()).contains(r#"data-cols="6""#));
}
