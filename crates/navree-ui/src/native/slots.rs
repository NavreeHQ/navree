//! The slot readers: one per `data-slot`, each reading one component's parts
//! into the native part it stands for.
//!
//! A reader takes the parts it knows by name ([`Parts`]) and ends by checking
//! that nothing is left. A component that grows a part its reader never heard
//! of stops being read, and the test beside the reader, which builds the real
//! component, fails in the same crate.

use std::borrow::Cow;

use navree_native::action::{Action, Command, Pair};
use navree_native::node as n;

use super::lower::{
    Control, Cx, N, Parts, Read, align_of, attr, elements, fail, fail_on, flat, has, hooks, id,
    label_of, number, one, slot, text_of, tone, tone_of, with_id,
};
use crate::node::{Element, Node};

/// Reads `element` by the reader of `slot`. `None` when the slot has none.
pub(crate) fn read(cx: &mut Cx, slot: &str, element: &Element) -> Option<Read> {
    type Reader = fn(&mut Cx, &Element) -> Read;
    // The hooks each reader answers for, beyond the quiet ones.
    let (reader, known): (Reader, &[&str]) = match slot {
        "stack" | "row" => (stack, &[]),
        "grid" => (grid, &[]),
        "container" | "item-media" | "form-stack" | "form-grid" | "scroll-area" => (inside, &[]),
        "text" | "heading" | "code" | "kbd" => (text, &[]),
        "badge" => (badge, &[]),
        "button" => (button, &["data-ui-click", "data-ui-refresh"]),
        "icon" => (
            |_, element| icon(element).map(|icon| vec![N::Icon(icon)]),
            &[],
        ),
        "flag" => (flag, &[]),
        "separator" => (
            |_, element| Ok(vec![N::Divider(n::Divider { id: id(element) })]),
            &[],
        ),
        "avatar" => (avatar, &[]),
        "card" => (card, &[]),
        "empty" => (empty, &[]),
        "alert" => (alert, &[]),
        "stat" => (stat, &[]),
        "item" => (item, &[]),
        "dialog" => (dialog, &[]),
        "alert-dialog" => (alert_dialog, &[]),
        "sheet" | "drawer" => (sheet, &[]),
        "popover-content" => (popover, &[]),
        "form" => (form, &["data-ui-form", "data-ui-target"]),
        "form-actions" => (form_actions, &[]),
        "field" => (field, &[]),
        "choice" => (choice, &[]),
        "radio-group" => (radio_group, &[]),
        "input" | "checkbox" | "switch" | "native-select" | "textarea" => {
            (control, &["data-ui-value"])
        }
        "fact-grid" => (fact_grid, &[]),
        "timeline" => (timeline, &[]),
        "meta" => (meta, &[]),
        "table-container" => (table_container, &[]),
        "table" => (
            |cx, element| cx.table(element).map(|table| vec![N::Table(table)]),
            &[],
        ),
        "breadcrumb" => (breadcrumb, &[]),
        "region" => (
            region,
            &[
                "data-ui-region",
                "data-ui-src",
                "data-ui-lazy",
                "data-ui-live-src",
                "data-ui-live-event",
                "data-ui-live-catch-up",
            ],
        ),
        "skeleton" | "spinner" => (
            |_, element| {
                Ok(vec![N::Progress(n::Progress {
                    id: id(element),
                    value: None,
                    label: attr(element, "aria-label").map(Cow::into_owned),
                })])
            },
            &[],
        ),
        // The page's own toasts are sent as ops; the place they are drawn in is the browser's.
        "toaster" => (|_, _| Ok(Vec::new()), &["data-ui-toaster"]),
        "data-table" => (super::table::data_table, super::table::HOOKS),
        _ => return None,
    };
    Some(hooks(element, known).and_then(|()| reader(cx, element)))
}

/// A component that only wraps what it holds.
fn inside(cx: &mut Cx, element: &Element) -> Read {
    let children = cx.of(element);
    Ok(match (children.len(), id(element)) {
        (0, None) => Vec::new(),
        (1, id) => vec![with_id(one(children), id)],
        (_, id) => vec![N::Stack(n::Stack {
            id,
            children,
            ..n::Stack::default()
        })],
    })
}

/// The room between parts, from `data-gap`.
fn gap(element: &Element) -> u8 {
    number(element, "data-gap").unwrap_or(0)
}

fn justify_of(element: &Element) -> Option<n::Justify> {
    match attr(element, "data-justify").as_deref() {
        Some("start") => Some(n::Justify::Start),
        Some("center") => Some(n::Justify::Center),
        Some("end") => Some(n::Justify::End),
        Some("between") => Some(n::Justify::Between),
        _ => None,
    }
}

/// `Stack` and `Row`.
fn stack(cx: &mut Cx, element: &Element) -> Read {
    Ok(vec![N::Stack(n::Stack {
        id: id(element),
        axis: if slot(element).as_deref() == Some("row") {
            n::Axis::Horizontal
        } else {
            n::Axis::Vertical
        },
        gap: gap(element),
        align: align_of(element),
        justify: justify_of(element),
        wrap: has(element, "data-wrap"),
        hidden: has(element, "hidden"),
        children: cx.of(element),
    })])
}

/// `Grid`.
fn grid(cx: &mut Cx, element: &Element) -> Read {
    Ok(vec![N::Grid(n::Grid {
        id: id(element),
        cols: number(element, "data-cols"),
        min: None,
        gap: gap(element),
        children: cx.of(element),
    })])
}

/// `Text`, `Heading`, inline code and a key.
fn text(_: &mut Cx, element: &Element) -> Read {
    let variant = attr(element, "data-variant");
    let level = match element.tag() {
        "h1" => Some(1),
        "h2" => Some(2),
        "h3" => Some(3),
        "h4" => Some(4),
        "h5" => Some(5),
        "h6" => Some(6),
        _ => None,
    };
    let style = match (variant.as_deref(), level, element.tag()) {
        (Some("code"), ..) | (_, _, "code" | "kbd") => n::TextStyle::Mono,
        (Some("lead"), ..) => n::TextStyle::Callout,
        (_, Some(1 | 2), _) => n::TextStyle::Title,
        (_, Some(_), _) => n::TextStyle::Headline,
        (None | Some("muted"), None, _) => n::TextStyle::Body,
        (Some(other), None, _) => return fail(format!("text of the `{other}` variant")),
    };
    // Text that holds a link or more than words is read as its runs.
    if !element.child_nodes().iter().all(plain_words) {
        return fail("text that holds more than words");
    }
    Ok(vec![N::Text(n::Text {
        id: id(element),
        value: text_of(element),
        style,
        tone: tone_of(element),
        muted: variant.as_deref() == Some("muted"),
        lines: None,
        heading: level,
    })])
}

/// Whether a node is words alone: text, numbers, and inline elements that
/// only group them.
fn plain_words(node: &Node) -> bool {
    match node {
        Node::Text(_) | Node::Int(_) | Node::Empty => true,
        Node::Fragment(nodes) => nodes.iter().all(plain_words),
        Node::Element(element) => {
            matches!(
                element.tag(),
                "span" | "bdi" | "strong" | "b" | "em" | "i" | "time" | "small"
            ) && slot(element).is_none()
                && element.child_nodes().iter().all(plain_words)
        }
        Node::Raw(_) | Node::Static(_) => false,
    }
}

/// `Badge`: its words, its tone and its dot.
fn badge(_: &mut Cx, element: &Element) -> Read {
    let mut parts = Parts::of(element);
    let dot = parts.slot("badge-dot").is_some();
    let _icons = parts.slots("icon");
    if !parts
        .left()
        .iter()
        .all(|node| plain_words(node) || screen_reader_only(node))
    {
        return fail("a badge that holds more than words");
    }
    Ok(vec![N::Badge(n::Badge {
        id: id(element),
        text: text_of(element),
        tone: tone_of(element),
        dot,
    })])
}

/// Whether a node is words for a screen reader only.
fn screen_reader_only(node: &Node) -> bool {
    matches!(node, Node::Element(element) if has(element, "data-ui-sr"))
}

/// `Button`.
fn button(cx: &mut Cx, element: &Element) -> Read {
    cx.button(element).map(|button| vec![N::Button(button)])
}

/// An icon: its shapes as path data, its name, and its label when it stands alone.
pub(crate) fn icon(element: &Element) -> Read<n::Icon> {
    let [Node::Static(shapes)] = element.child_nodes() else {
        return fail("an icon whose shapes are not compile-time markup");
    };
    let view_box = attr(element, "viewBox").unwrap_or(Cow::Borrowed("0 0 24 24"));
    match super::icon::icon(shapes, &view_box) {
        Ok(mut icon) => {
            icon.id = id(element);
            icon.label = attr(element, "aria-label").map(Cow::into_owned);
            icon.tone = tone_of(element);
            Ok(icon)
        }
        Err(why) => fail(why),
    }
}

/// `Flag`: the flag of a country as the emoji every system draws for it. A
/// flag with no emoji is its name, or nothing when it is decoration.
fn flag(cx: &mut Cx, element: &Element) -> Read {
    let label = attr(element, "aria-label").map(Cow::into_owned);
    let emoji = match element.child_nodes() {
        [Node::Static(shapes)] => super::icon::flag_of(shapes).and_then(super::icon::flag_emoji),
        _ => None,
    };
    Ok(match (emoji, label) {
        (Some(emoji), _) => vec![N::Text(n::Text {
            id: id(element),
            value: emoji,
            ..n::Text::default()
        })],
        (None, Some(label)) => vec![N::text(label)],
        (None, None) => {
            cx.dropped.push("flag".to_owned());
            Vec::new()
        }
    })
}

/// `Avatar`: whose it is, with a picture or initials.
fn avatar(_: &mut Cx, element: &Element) -> Read {
    let mut src = None;
    let mut name = attr(element, "aria-label").map(Cow::into_owned);
    for child in flat(element.child_nodes()) {
        if let Node::Element(part) = child {
            match (part.tag(), slot(part).as_deref()) {
                ("img", _) => {
                    src = attr(part, "src").map(Cow::into_owned);
                    if name.is_none() {
                        name = attr(part, "alt").map(Cow::into_owned);
                    }
                }
                ("svg", Some("icon")) | ("span", _) => {}
                (tag, _) => return fail(format!("an avatar that holds a <{tag}>")),
            }
        }
    }
    let initials = Some(text_of(element)).filter(|text| !text.is_empty());
    Ok(vec![N::Avatar(n::Avatar {
        id: id(element),
        name: name.or_else(|| initials.clone()).unwrap_or_default(),
        src,
        initials,
    })])
}

/// `Card`: a section drawn as a card.
fn card(cx: &mut Cx, element: &Element) -> Read {
    let mut parts = Parts::of(element);
    let mut section = n::Section {
        id: id(element),
        style: n::SectionStyle::Card,
        ..n::Section::default()
    };
    if let Some(header) = parts.slot("card-header") {
        let mut header = Parts::of(header);
        section.title = header.slot("card-title").map(text_of);
        section.subtitle = header.slot("card-description").map(text_of);
        if let Some(action) = header.slot("card-action") {
            section.actions = cx.of(action);
        }
        header.done()?;
    }
    if let Some(content) = parts.slot("card-content") {
        section.children = cx.of(content);
    }
    if let Some(footer) = parts.slot("card-footer") {
        section.footer = cx.of(footer);
    }
    parts.done()?;
    Ok(vec![N::Section(section)])
}

/// `Empty`: what a place shows when it holds nothing.
fn empty(cx: &mut Cx, element: &Element) -> Read {
    let mut parts = Parts::of(element);
    let icon = match parts.slot("empty-media") {
        Some(media) => match elements(media)?.as_slice() {
            [glyph] if slot(glyph).as_deref() == Some("icon") => Some(icon(glyph)?),
            _ => return fail("an empty state whose picture is not an icon"),
        },
        None => None,
    };
    let Some(title) = parts.slot("empty-title") else {
        return fail("an empty state with no title");
    };
    let text = parts.slot("empty-description").map(text_of);
    let actions = match parts.slot("empty-content") {
        Some(content) => cx.of(content),
        None => Vec::new(),
    };
    parts.done()?;
    Ok(vec![N::Empty(n::Empty {
        id: id(element),
        icon,
        title: text_of(title),
        text,
        actions,
    })])
}

/// `Alert`: a message in place.
fn alert(cx: &mut Cx, element: &Element) -> Read {
    let mut parts = Parts::of(element);
    let icon = match parts.slot("icon") {
        Some(glyph) => Some(icon(glyph)?),
        None => None,
    };
    let title = parts.slot("alert-title").map(text_of);
    let text = parts.slot("alert-description").map(text_of);
    let actions = match parts.slot("alert-actions") {
        Some(actions) => cx.of(actions),
        None => Vec::new(),
    };
    // Its close button is the browser's way to dismiss it; a client has its own.
    let _close = parts.slot("alert-close");
    parts.done()?;
    Ok(vec![N::Banner(n::Banner {
        id: id(element),
        tone: tone_of(element).or_else(|| attr(element, "data-variant").and_then(|v| tone(&v))),
        title,
        text,
        icon,
        actions,
    })])
}

/// `Stat`: a figure under its label, as a small section.
fn stat(_: &mut Cx, element: &Element) -> Read {
    let mut parts = Parts::of(element);
    let Some(label) = parts.slot("stat-label") else {
        return fail("a figure with no label");
    };
    let Some(value) = parts.slot("stat-value") else {
        return fail("a figure with no value");
    };
    let trend = parts.slot("stat-trend");
    parts.done()?;
    let mut children = vec![N::Text(n::Text {
        value: text_of(value),
        style: n::TextStyle::Title,
        ..n::Text::default()
    })];
    if let Some(trend) = trend {
        children.push(N::Text(n::Text {
            value: text_of(trend),
            style: n::TextStyle::Caption,
            tone: match attr(trend, "data-tone").as_deref() {
                Some("up") => Some(n::Tone::Success),
                Some("down") => Some(n::Tone::Danger),
                _ => None,
            },
            ..n::Text::default()
        }));
    }
    Ok(vec![N::Section(n::Section {
        id: id(element),
        title: Some(text_of(label)),
        style: n::SectionStyle::Card,
        children,
        ..n::Section::default()
    })])
}

/// `Item`: a row of a list, as the page built it.
fn item(cx: &mut Cx, element: &Element) -> Read {
    let mut parts = Parts::of(element);
    let leading = match parts.slot("item-media") {
        Some(media) => Some(cx.of(media)).filter(|read| !read.is_empty()).map(one),
        None => None,
    };
    let mut row = n::Row {
        id: id(element),
        leading: leading.map(Box::new),
        ..n::Row::default()
    };
    let Some(content) = parts.slot("item-content") else {
        return fail("an item with no content");
    };
    let mut content = Parts::of(content);
    let mut title = content.slot("item-title");
    if let Some(head) = content.slot("item-head") {
        let mut head = Parts::of(head);
        title = head.slot("item-title");
        if let Some(aside) = head.slot("item-aside") {
            row.trailing = Some(Box::new(one(cx.of(aside))));
        }
        head.done()?;
    }
    let Some(title) = title else {
        return fail("an item with no title");
    };
    row.title = text_of(title);
    row.subtitle = content.slot("item-description").map(text_of);
    if let Some(footer) = content.slot("item-footer") {
        row.detail = cx.of(footer);
    }
    content.done()?;
    if let Some(actions) = parts.slot("item-actions") {
        let read = cx.of(actions);
        if row.trailing.is_none() {
            row.trailing = Some(Box::new(one(read)));
        } else {
            row.menu = read;
        }
    }
    parts.done()?;
    if element.tag() == "a" {
        row.action = cx.action_of(element)?;
    }
    Ok(vec![N::Row(row)])
}

/// The header every dialog and sheet starts with: its title and description.
fn dialog_header(parts: &mut Parts<'_>) -> Read<(Option<String>, Option<String>)> {
    let Some(header) = parts.slot("dialog-header") else {
        return Ok((None, None));
    };
    let mut header = Parts::of(header);
    let title = header.slot("dialog-title").map(text_of);
    let text = header.slot("dialog-description").map(text_of);
    header.done()?;
    Ok((title, text))
}

/// `Dialog`.
fn dialog(cx: &mut Cx, element: &Element) -> Read {
    let Some(id) = id(element) else {
        return fail("a dialog with no id");
    };
    let mut parts = Parts::of(element);
    let (title, text) = dialog_header(&mut parts)?;
    let children = match parts.slot("dialog-body") {
        Some(body) => cx.of(body),
        None => Vec::new(),
    };
    let actions = match parts.slot("dialog-footer") {
        Some(footer) => answers(cx, footer, &id)?,
        None => Vec::new(),
    };
    // Its close button is the browser's; every system has its own way out of a dialog.
    let _close = parts.slot("dialog-close");
    parts.done()?;
    Ok(vec![N::Dialog(n::Dialog {
        id,
        title,
        text,
        alert: false,
        open: has(element, "open"),
        children,
        actions,
    })])
}

/// `AlertDialog`: a question before something that can't be undone.
fn alert_dialog(cx: &mut Cx, element: &Element) -> Read {
    let Some(id) = id(element) else {
        return fail("a dialog with no id");
    };
    let mut parts = Parts::of(element);
    let (title, text) = dialog_header(&mut parts)?;
    let Some(footer) = parts.slot("dialog-footer") else {
        return fail("a question with no answers");
    };
    let actions = answers(cx, footer, &id)?;
    parts.done()?;
    // A system's alert holds buttons and nothing else.
    if !actions.iter().all(|node| matches!(node, N::Button(_))) {
        return fail("an answer that is not a button");
    }
    Ok(vec![N::Dialog(n::Dialog {
        id,
        title,
        text,
        alert: true,
        open: has(element, "open"),
        children: Vec::new(),
        actions,
    })])
}

/// The answers at the foot of a dialog: a button that closes the dialog it
/// stands in cancels, and a form that is one button is that button, posting.
fn answers(cx: &mut Cx, footer: &Element, dialog: &str) -> Read<Vec<N>> {
    let mut out = Vec::new();
    for node in cx.of(footer) {
        out.push(match node {
            N::Button(mut button) => {
                if button.action
                    == Some(Action::command(Command::Close {
                        id: dialog.to_owned(),
                    }))
                {
                    button.action = Some(Action::Dismiss);
                    if button.role != n::Role::Destructive {
                        button.role = n::Role::Cancel;
                    }
                }
                N::Button(button)
            }
            N::Form(form) => match button_of(form) {
                Ok(button) => N::Button(button),
                Err(form) => N::Form(*form),
            },
            other => other,
        });
    }
    Ok(out)
}

/// A form that is one submit button and nothing a reader fills, as a button
/// that sends the form's request itself.
pub(crate) fn button_of(mut form: n::Form) -> Result<n::Button, Box<n::Form>> {
    let [N::Button(button)] = form.children.as_slice() else {
        return Err(Box::new(form));
    };
    let Some(Action::Submit {
        form: None,
        href,
        with,
    }) = &button.action
    else {
        return Err(Box::new(form));
    };
    let mut fields: Vec<Pair> = form
        .hidden
        .iter()
        .map(|hidden| Pair {
            name: hidden.name.clone(),
            value: hidden.value.clone(),
        })
        .collect();
    fields.extend(with.clone());
    let action = Action::Request {
        method: form.method,
        href: href.clone().unwrap_or_else(|| form.action.clone()),
        fields,
    };
    let N::Button(mut button) = form.children.remove(0) else {
        return Err(Box::new(form));
    };
    button.action = Some(action);
    if button.id.is_none() {
        button.id = form.id;
    }
    Ok(button)
}

/// `Sheet` and `Drawer`.
fn sheet(cx: &mut Cx, element: &Element) -> Read {
    let Some(id) = id(element) else {
        return fail("a sheet with no id");
    };
    let mut parts = Parts::of(element);
    let (title, subtitle) = dialog_header(&mut parts)?;
    let children = match parts.slot("dialog-body") {
        Some(body) => cx.of(body),
        None => Vec::new(),
    };
    let actions = match parts.slot("dialog-footer") {
        Some(footer) => answers(cx, footer, &id)?,
        None => Vec::new(),
    };
    let _close = parts.slot("dialog-close");
    parts.done()?;
    Ok(vec![N::Sheet(n::Sheet {
        id,
        title,
        subtitle,
        size: match attr(element, "data-width").as_deref() {
            Some("sm" | "md") => n::SheetSize::Medium,
            Some("full") => n::SheetSize::Full,
            _ => n::SheetSize::Large,
        },
        open: has(element, "open"),
        children,
        actions,
    })])
}

/// `Popover`'s content.
fn popover(cx: &mut Cx, element: &Element) -> Read {
    let Some(id) = id(element) else {
        return fail("a popover with no id");
    };
    Ok(vec![N::Popover(n::Popover {
        id,
        children: cx.of(element),
    })])
}

/// `FormView`'s form.
fn form(cx: &mut Cx, element: &Element) -> Read {
    cx.form(element).map(|form| vec![N::Form(form)])
}

/// The buttons at the foot of a form.
fn form_actions(cx: &mut Cx, element: &Element) -> Read {
    Ok(vec![N::Stack(n::Stack {
        id: id(element),
        axis: n::Axis::Horizontal,
        gap: 2,
        justify: Some(n::Justify::End),
        children: cx.of(element),
        ..n::Stack::default()
    })])
}

/// A control with a slot of its own.
fn control(cx: &mut Cx, element: &Element) -> Read {
    match cx.control(element)? {
        Control::Field(field) => Ok(vec![N::Field(*field)]),
        Control::Button(button) => Ok(vec![N::Button(*button)]),
        Control::Hidden(_) if cx.in_form() => Ok(Vec::new()),
        Control::Hidden(_) => fail("a hidden value outside any form"),
    }
}

/// `Field`: a label, one control, a line of help and what is wrong with it.
fn field(cx: &mut Cx, element: &Element) -> Read {
    let mut parts = Parts::of(element);
    let label = parts.slot("label").map(text_of);
    let help = parts.slot("field-description").map(text_of);
    let errors = parts.slots("field-error");
    let error = Some(
        errors
            .into_iter()
            .map(text_of)
            .collect::<Vec<_>>()
            .join(" "),
    )
    .filter(|text| !text.is_empty());
    let [Node::Element(inner)] = parts.left() else {
        return fail("a field that holds more or less than one control");
    };
    // A box or a switch carries its own label; a group of radios, its legend.
    if matches!(slot(inner).as_deref(), Some("choice" | "radio-group")) {
        let mut read = if slot(inner).as_deref() == Some("choice") {
            choice(cx, inner)?
        } else {
            radio_group(cx, inner)?
        };
        if let Some(N::Field(field)) = read.first_mut() {
            if field.help.is_none() {
                field.help = help;
            }
            field.error = error;
        }
        return Ok(read);
    }
    if !matches!(inner.tag(), "input" | "select" | "textarea") {
        return match slot(inner) {
            Some(slot) => fail_on(format!("slot:{slot}"), "a control no reader here knows"),
            None => fail(format!("a field around a <{}>", inner.tag())),
        };
    }
    hooks(inner, &["data-ui-value"])?;
    let Control::Field(mut field) = cx.control(inner)? else {
        return fail("a field around a control that is not filled");
    };
    if label.is_some() {
        field.label = label.map(|label| label.trim_end_matches('*').trim().to_owned());
    }
    field.help = help;
    field.error = error;
    Ok(vec![N::Field(*field)])
}

/// A box or a switch in its label, with a line under the label when there is one.
fn choice(cx: &mut Cx, element: &Element) -> Read {
    let mut inner = None;
    let mut label = None;
    let mut help = None;
    for part in elements(element)? {
        if part.tag() == "input" {
            if inner.replace(part).is_some() {
                return fail("a choice with more than one control");
            }
        } else if let Some(words) = super::page::find(part.child_nodes(), "choice-label") {
            label = Some(text_of(words));
            help = super::page::find(part.child_nodes(), "choice-description").map(text_of);
        } else {
            label = Some(text_of(part));
        }
    }
    let Some(inner) = inner else {
        return fail("a choice with no control");
    };
    hooks(inner, &["data-ui-value"])?;
    let Control::Field(mut field) = cx.control(inner)? else {
        return fail("a choice around a control that is not filled");
    };
    field.label = label.or(field.label);
    field.help = help;
    Ok(vec![N::Field(*field)])
}

/// `RadioGroup`: one of several, each a radio button in its label.
fn radio_group(_: &mut Cx, element: &Element) -> Read {
    let mut parts = Parts::of(element);
    let label = parts.slot("field-legend").map(text_of);
    let mut input = navree_native::input::ChoiceInput {
        kind: navree_native::input::ChoiceKind::Radio,
        ..navree_native::input::ChoiceInput::default()
    };
    let mut name = String::new();
    let mut required = false;
    for option in parts.slots("choice") {
        let inside = elements(option)?;
        let [radio, words] = inside.as_slice() else {
            return fail("a choice of a group that is not a radio button and its words");
        };
        if attr(radio, "type").as_deref() != Some("radio") {
            return fail("a choice of a group that is not a radio button");
        }
        name = attr(radio, "name").map(Cow::into_owned).unwrap_or_default();
        required |= has(radio, "required");
        let value = attr(radio, "value")
            .map(Cow::into_owned)
            .unwrap_or_default();
        if has(radio, "checked") {
            input.value = Some(value.clone());
        }
        input.options.push(navree_native::input::ChoiceOption {
            value,
            label: text_of(words),
            detail: None,
            disabled: has(radio, "disabled"),
        });
    }
    parts.done()?;
    Ok(vec![N::Field(n::Field {
        id: id(element),
        name,
        label,
        help: None,
        error: None,
        required,
        disabled: has(element, "disabled"),
        input: navree_native::input::Input::Choice(input),
    })])
}

/// `FactGrid`: cards of a label, a value and lines of detail.
fn fact_grid(cx: &mut Cx, element: &Element) -> Read {
    let mut items = Vec::new();
    for card in elements(element)? {
        if slot(card).as_deref() != Some("fact-card") {
            return fail("something that is not a fact in a grid of facts");
        }
        let mut parts = Parts::of(card);
        let glyph = match parts.slot("fact-icon") {
            Some(holder) => match elements(holder)?.as_slice() {
                [glyph] if slot(glyph).as_deref() == Some("icon") => Some(icon(glyph)?),
                _ => return fail("a fact whose picture is not an icon"),
            },
            None => None,
        };
        let Some(body) = parts.slot("fact-body") else {
            return fail("a fact with no body");
        };
        parts.done()?;
        let mut fact = n::Fact {
            icon: glyph,
            tone: tone_of(card),
            ..n::Fact::default()
        };
        for group in elements(body)? {
            for part in elements(group)? {
                match slot(part).as_deref() {
                    Some("fact-label") => fact.label = text_of(part),
                    Some("fact-value") => fact.value = cx.of(part),
                    Some("fact-detail") => fact.details.push(text_of(part)),
                    _ => return fail("a part of a fact no reader here knows"),
                }
            }
        }
        items.push(fact);
    }
    Ok(vec![N::Facts(n::Facts {
        id: id(element),
        items,
    })])
}

/// `Timeline`: its days as sections, each event a row with its time at the end.
fn timeline(cx: &mut Cx, element: &Element) -> Read {
    let mut parts = Parts::of(element);
    let mut children = Vec::new();
    if let Some(zone) = parts.slot("timeline-zone") {
        children.push(N::Text(n::Text {
            value: text_of(zone),
            style: n::TextStyle::Caption,
            muted: true,
            ..n::Text::default()
        }));
    }
    let days = parts.slots("timeline-day");
    let lists = parts.slots("timeline-list");
    parts.done()?;
    for day in days {
        let mut day_parts = Parts::of(day);
        let title = day_parts.slot("timeline-day-title").map(text_of);
        let Some(list) = day_parts.slot("timeline-list") else {
            return fail("a day with no events");
        };
        day_parts.done()?;
        children.push(N::Section(n::Section {
            title,
            children: vec![N::List(n::List {
                children: events(cx, list)?,
                ..n::List::default()
            })],
            ..n::Section::default()
        }));
    }
    for list in lists {
        children.push(N::List(n::List {
            children: events(cx, list)?,
            ..n::List::default()
        }));
    }
    Ok(vec![N::Stack(n::Stack {
        id: id(element),
        gap: 3,
        children,
        ..n::Stack::default()
    })])
}

/// The events of one list of a timeline.
fn events(cx: &mut Cx, list: &Element) -> Read<Vec<N>> {
    let mut rows = Vec::new();
    for event in elements(list)? {
        if slot(event).as_deref() != Some("timeline-event") {
            return fail("something that is not an event in a timeline");
        }
        let mut parts = Parts::of(event);
        let time = parts.slot("timeline-time").map(text_of);
        let glyph = match parts.slot("timeline-dot") {
            Some(dot) => match elements(dot)?.as_slice() {
                [] => None,
                [glyph] if slot(glyph).as_deref() == Some("icon") => {
                    let mut glyph = icon(glyph)?;
                    glyph.tone = tone_of(event);
                    Some(glyph)
                }
                _ => return fail("an event whose mark is not an icon"),
            },
            None => None,
        };
        let Some(body) = parts.slot("timeline-body") else {
            return fail("an event with no body");
        };
        parts.done()?;
        let mut body = Parts::of(body);
        let Some(head) = body.slot("timeline-head") else {
            return fail("an event with no title");
        };
        let mut head = Parts::of(head);
        let Some(title) = head.slot("timeline-title") else {
            return fail("an event with no title");
        };
        let gap = head.slot("timeline-gap").map(text_of);
        head.done()?;
        let details = body.slots("timeline-detail");
        let mut detail = Vec::new();
        if let Some(meta) = body.slot("meta") {
            detail.extend(self::meta(cx, meta)?);
        }
        if let Some(gap) = gap {
            detail.push(N::Text(n::Text {
                value: gap,
                style: n::TextStyle::Caption,
                muted: true,
                ..n::Text::default()
            }));
        }
        body.done()?;
        rows.push(N::Row(n::Row {
            id: id(event),
            leading: glyph.map(|glyph| Box::new(N::Icon(glyph))),
            title: text_of(title),
            subtitle: Some(
                details
                    .into_iter()
                    .map(text_of)
                    .collect::<Vec<_>>()
                    .join(" "),
            )
            .filter(|text| !text.is_empty()),
            detail,
            trailing: time.map(|time| {
                Box::new(N::Text(n::Text {
                    value: time,
                    style: n::TextStyle::Caption,
                    muted: true,
                    ..n::Text::default()
                }))
            }),
            state: match attr(event, "data-state").as_deref() {
                Some("current") => Some(n::RowState::Current),
                Some("upcoming") => Some(n::RowState::Upcoming),
                Some("done") => Some(n::RowState::Done),
                _ => None,
            },
            ..n::Row::default()
        }));
    }
    Ok(rows)
}

/// `Meta`: small facts in a line, each with what stands before it.
fn meta(cx: &mut Cx, element: &Element) -> Read {
    let mut children = Vec::new();
    for item in elements(element)? {
        if slot(item).as_deref() != Some("meta-item") {
            return fail("something that is not an item in a line of facts");
        }
        let mut parts = Parts::of(item);
        let mut pair = Vec::new();
        if let Some(start) = parts.slot("meta-start") {
            pair.extend(cx.of(start));
        }
        if let Some(words) = parts.slot("meta-text") {
            pair.push(N::Text(n::Text {
                value: text_of(words),
                style: n::TextStyle::Caption,
                muted: true,
                ..n::Text::default()
            }));
        }
        parts.done()?;
        children.push(if pair.len() == 1 {
            pair.remove(0)
        } else {
            N::Stack(n::Stack {
                axis: n::Axis::Horizontal,
                gap: 1,
                align: Some(n::Align::Center),
                children: pair,
                ..n::Stack::default()
            })
        });
    }
    Ok(vec![N::Stack(n::Stack {
        id: id(element),
        axis: n::Axis::Horizontal,
        gap: 3,
        wrap: true,
        children,
        ..n::Stack::default()
    })])
}

/// `Table`'s scrolling box: the table inside it.
fn table_container(cx: &mut Cx, element: &Element) -> Read {
    let inside = elements(element)?;
    let [table] = inside.as_slice() else {
        return fail("a table's box that holds more or less than a table");
    };
    if table.tag() != "table" {
        return fail("a table's box that holds no table");
    }
    cx.table(table).map(|table| vec![N::Table(table)])
}

/// `Breadcrumb` in a page's content: its steps as links in a line.
fn breadcrumb(_: &mut Cx, element: &Element) -> Read {
    let trail = trail(element)?;
    let mut children: Vec<N> = trail
        .steps
        .into_iter()
        .map(|step| {
            N::Link(n::Link {
                id: None,
                label: step.title,
                action: Action::navigate(step.href),
                children: Vec::new(),
            })
        })
        .collect();
    if let Some(current) = trail.current {
        children.push(N::text(current));
    }
    Ok(vec![N::Stack(n::Stack {
        id: id(element),
        axis: n::Axis::Horizontal,
        gap: 2,
        wrap: true,
        children,
        ..n::Stack::default()
    })])
}

/// A breadcrumb's steps and the page it ends on.
pub(crate) struct Trail {
    pub(crate) steps: Vec<navree_native::Back>,
    pub(crate) current: Option<String>,
}

/// Reads a `Breadcrumb`: every step with a link, shown or folded away, in order.
pub(crate) fn trail(element: &Element) -> Read<Trail> {
    let mut trail = Trail {
        steps: Vec::new(),
        current: None,
    };
    fn walk(element: &Element, trail: &mut Trail) {
        for child in flat(element.child_nodes()) {
            let Node::Element(child) = child else {
                continue;
            };
            if child.tag() == "a"
                && let Some(href) = attr(child, "href")
            {
                trail.steps.push(navree_native::Back {
                    title: label_of(child),
                    href: href.into_owned(),
                });
            } else if attr(child, "aria-current").as_deref() == Some("page") {
                trail.current = Some(text_of(child));
            } else if !matches!(child.tag(), "svg" | "button") {
                walk(child, trail);
            }
        }
    }
    walk(element, &mut trail);
    Ok(trail)
}

/// `Region`: a part that fetches itself. What stands in it while it loads, or
/// when it failed, is the browser's.
fn region(cx: &mut Cx, element: &Element) -> Read {
    let Some(id) = id(element) else {
        return fail("a region with no id");
    };
    let Some(src) = attr(element, "data-ui-src") else {
        return fail("a region with no source");
    };
    let mut parts = Parts::of(element);
    let _waiting = parts.slots("skeleton");
    let _failed = parts.slots("region-failed");
    let children = cx.flat_sequence(parts.left());
    Ok(vec![N::Region(n::Region {
        id,
        src: src.into_owned(),
        lazy: has(element, "data-ui-lazy"),
        live: attr(element, "data-ui-live-src").map(|live| n::Live {
            src: live.into_owned(),
            event: attr(element, "data-ui-live-event").map(Cow::into_owned),
        }),
        children,
    })])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::lower;
    use crate::{
        AlertDialog, Align, Avatar, Badge, Button, ButtonVariant, Card, Dialog,
        Empty as EmptyState, FactCard, FactGrid, Flag, Grid, Justify, Meta, Region, Row, Separator,
        Sheet, SheetWidth, Stack, Stat, Table, TableCell, TableHead, TableRowEl, Text, Timeline,
        TimelineEvent, Tone, el, icons,
    };

    fn read(node: impl Into<Node>) -> N {
        let lowered = lower(&node.into());
        assert!(lowered.is_native(), "{:?}", lowered.misses);
        lowered.node
    }

    fn text_node(value: &str) -> N {
        N::text(value)
    }

    fn muted(value: &str) -> N {
        N::Text(n::Text {
            value: value.to_owned(),
            muted: true,
            ..n::Text::default()
        })
    }

    #[test]
    fn a_stack_and_a_row_keep_their_gap_and_where_their_parts_sit() {
        let node = read(
            Row::gap(3)
                .align(Align::Center)
                .justify(Justify::Between)
                .wrap()
                .children((
                    Stack::gap(0).children((Text::new("a"), Text::muted("b"))),
                    "c",
                )),
        );
        assert_eq!(
            node,
            N::Stack(n::Stack {
                axis: n::Axis::Horizontal,
                gap: 3,
                align: Some(n::Align::Center),
                justify: Some(n::Justify::Between),
                wrap: true,
                children: vec![
                    N::Stack(n::Stack {
                        children: vec![text_node("a"), muted("b")],
                        ..n::Stack::default()
                    }),
                    text_node("c"),
                ],
                ..n::Stack::default()
            })
        );
        let N::Grid(grid) = read(
            Grid::cols(4)
                .gap(3)
                .children((Text::new("a"), Text::new("b"))),
        ) else {
            panic!("not a grid");
        };
        assert_eq!((grid.cols, grid.gap, grid.children.len()), (Some(4), 3, 2));
    }

    #[test]
    fn a_badge_reads_its_words_tone_and_dot() {
        assert_eq!(
            read(Badge::new("Delivered").tone(Tone::Success).dot()),
            N::Badge(n::Badge {
                text: "Delivered".into(),
                tone: Some(n::Tone::Success),
                dot: true,
                ..n::Badge::default()
            })
        );
        // A colour with no meaning has no tone.
        let N::Badge(violet) = read(Badge::new("Beta").tone(Tone::named("violet"))) else {
            panic!("not a badge");
        };
        assert_eq!(violet.tone, None);
    }

    #[test]
    fn a_button_reads_its_look_as_a_role_and_what_it_does() {
        let N::Button(open) = read(
            Button::destructive("Cancel order")
                .icon(icons::BAN)
                .disabled(true)
                .opens_dialog("cancel"),
        ) else {
            panic!("not a button");
        };
        assert_eq!(open.label, "Cancel order");
        assert_eq!(open.role, n::Role::Destructive);
        assert_eq!(open.prominence, n::Prominence::High);
        assert!(open.disabled);
        assert_eq!(open.icon.as_ref().unwrap().name.as_deref(), Some("ban"));
        assert_eq!(
            open.action,
            Some(Action::command(Command::Open {
                id: "cancel".into()
            }))
        );

        let N::Button(only) = read(Button::icon_only(icons::REFRESH_CW, "Refresh now")) else {
            panic!("not a button");
        };
        assert_eq!(only.label, "Refresh now");
        assert!(only.icon_only);
        assert_eq!(only.action, None);

        let N::Button(link) = read(
            Button::new("CS-20481")
                .variant(ButtonVariant::Link)
                .href("/orders/1"),
        ) else {
            panic!("not a button");
        };
        assert_eq!(link.prominence, n::Prominence::Low);
        assert_eq!(link.action, Some(Action::navigate("/orders/1")));

        let N::Button(save) = read(Button::primary("Save").submit().loading_text("Saving…"))
        else {
            panic!("not a button");
        };
        assert_eq!(save.label, "Save");
        assert_eq!(save.action, Some(Action::submit()));
    }

    #[test]
    fn a_flag_is_its_emoji_and_an_avatar_names_its_person() {
        assert_eq!(read(Flag::country("IN").decorative()), text_node("🇮🇳"));
        let N::Avatar(avatar) = read(Avatar::placeholder("Ravneet Kaur")) else {
            panic!("not an avatar");
        };
        assert_eq!(avatar.name, "Ravneet Kaur");
        assert_eq!(
            read(Separator::horizontal()),
            N::Divider(n::Divider::default())
        );
    }

    #[test]
    fn a_card_is_a_section_with_its_title_and_what_it_holds() {
        let node = read(
            Card::titled("Items")
                .description("2 units, from the Bengaluru warehouse")
                .action(Button::ghost("See all").href("/items"))
                .body(Text::new("Pashmina shawl"))
                .footer(Text::muted("Free shipping")),
        );
        let N::Section(section) = node else {
            panic!("not a section");
        };
        assert_eq!(section.style, n::SectionStyle::Card);
        assert_eq!(section.title.as_deref(), Some("Items"));
        assert_eq!(
            section.subtitle.as_deref(),
            Some("2 units, from the Bengaluru warehouse")
        );
        assert!(matches!(section.actions.as_slice(), [N::Button(_)]));
        assert_eq!(section.children, [text_node("Pashmina shawl")]);
        assert_eq!(section.footer, [muted("Free shipping")]);
    }

    #[test]
    fn an_empty_state_reads_its_icon_words_and_ways_on() {
        let node = read(
            EmptyState::new("That order isn't in the shop")
                .icon(icons::SEARCH_X)
                .description("Search the orders for its number.")
                .actions(Button::outline("Back to orders").href("/orders")),
        );
        let N::Empty(empty) = node else {
            panic!("not an empty state");
        };
        assert_eq!(empty.title, "That order isn't in the shop");
        assert_eq!(
            empty.text.as_deref(),
            Some("Search the orders for its number.")
        );
        assert_eq!(empty.icon.unwrap().name.as_deref(), Some("search-x"));
        let [N::Button(back)] = empty.actions.as_slice() else {
            panic!("not one button");
        };
        assert_eq!(back.role, n::Role::Secondary);
        assert_eq!(back.action, Some(Action::navigate("/orders")));
    }

    #[test]
    fn a_figure_is_a_small_section() {
        let node = read(Stat::new("Orders today", "952").trend("43 in the last hour", true));
        let N::Section(section) = node else {
            panic!("not a section");
        };
        assert_eq!(section.title.as_deref(), Some("Orders today"));
        let [N::Text(value), N::Text(trend)] = section.children.as_slice() else {
            panic!("not a value and a trend");
        };
        assert_eq!(value.value, "952");
        assert_eq!(trend.tone, Some(n::Tone::Success));
    }

    #[test]
    fn a_dialog_reads_its_words_and_what_it_holds() {
        let node = read(
            Dialog::new("refund", "Refund CS-20481")
                .description("Up to ₹7,848 can go back.")
                .body(Text::new("Inside")),
        );
        assert_eq!(
            node,
            N::Dialog(n::Dialog {
                id: "refund".into(),
                title: Some("Refund CS-20481".into()),
                text: Some("Up to ₹7,848 can go back.".into()),
                children: vec![text_node("Inside")],
                ..n::Dialog::default()
            })
        );
    }

    #[test]
    fn a_question_is_an_alert_whose_answers_are_buttons_by_role() {
        let node = read(
            AlertDialog::new("cancel", "Cancel CS-20481?", "This can't be undone.")
                .cancel("Keep the order")
                .action(
                    el("form")
                        .attr("method", "post")
                        .attr("action", "/orders/1/cancel")
                        .flag("data-ui-form", true)
                        .children((
                            crate::form::csrf_field("tok".to_owned()),
                            Button::destructive("Cancel order").submit(),
                        )),
                ),
        );
        assert_eq!(
            node,
            N::Dialog(n::Dialog {
                id: "cancel".into(),
                title: Some("Cancel CS-20481?".into()),
                text: Some("This can't be undone.".into()),
                alert: true,
                actions: vec![
                    N::Button(n::Button {
                        label: "Keep the order".into(),
                        role: n::Role::Cancel,
                        action: Some(Action::Dismiss),
                        ..n::Button::default()
                    }),
                    N::Button(n::Button {
                        label: "Cancel order".into(),
                        role: n::Role::Destructive,
                        prominence: n::Prominence::High,
                        action: Some(Action::Request {
                            method: n::Method::Post,
                            href: "/orders/1/cancel".into(),
                            fields: vec![Pair {
                                name: "_csrf".into(),
                                value: "tok".into()
                            }],
                        }),
                        ..n::Button::default()
                    }),
                ],
                ..n::Dialog::default()
            })
        );
    }

    #[test]
    fn a_sheet_reads_its_title_size_and_what_it_holds() {
        let node = read(
            Sheet::new("order", "Order CS-20481")
                .width(SheetWidth::Xl)
                .description("2 units for Tanvi Sharma in Pune, Maharashtra")
                .body(Text::new("Inside")),
        );
        assert_eq!(
            node,
            N::Sheet(n::Sheet {
                id: "order".into(),
                title: Some("Order CS-20481".into()),
                subtitle: Some("2 units for Tanvi Sharma in Pune, Maharashtra".into()),
                size: n::SheetSize::Large,
                children: vec![text_node("Inside")],
                ..n::Sheet::default()
            })
        );
    }

    #[test]
    fn a_box_in_its_label_and_a_group_of_radios_are_fields() {
        use navree_native::input as i;
        let node = read(
            crate::Checkbox::new("notify")
                .checked(true)
                .label("Tell the customer")
                .description("By email, once."),
        );
        let N::Field(notify) = node else {
            panic!("not a field");
        };
        assert_eq!(notify.name, "notify");
        assert_eq!(notify.label.as_deref(), Some("Tell the customer"));
        assert_eq!(notify.help.as_deref(), Some("By email, once."));
        assert!(matches!(&notify.input, i::Input::Toggle(toggle) if toggle.on));

        let node = read(
            crate::RadioGroup::new("pays", "Pays by")
                .options([
                    crate::OptionItem::new("upi", "UPI"),
                    crate::OptionItem::new("cod", "Cash on delivery"),
                ])
                .selected("cod"),
        );
        let N::Field(pays) = node else {
            panic!("not a field");
        };
        assert_eq!(pays.name, "pays");
        assert_eq!(pays.label.as_deref(), Some("Pays by"));
        let i::Input::Choice(choice) = &pays.input else {
            panic!("not a choice");
        };
        assert_eq!(choice.kind, i::ChoiceKind::Radio);
        assert_eq!(choice.value.as_deref(), Some("cod"));
        assert_eq!(choice.options.len(), 2);
        assert_eq!(choice.options[1].label, "Cash on delivery");
    }

    #[test]
    fn facts_read_their_label_value_icon_and_lines() {
        let node = read(
            FactGrid::new().cols(2).cards([
                FactCard::new("Customer", "Tanvi Sharma")
                    .icon(icons::USER)
                    .tone(Tone::Info)
                    .detail("tanvi.sharma@example.com")
                    .detail("+91 98765 36760"),
                FactCard::new(
                    "Ships to",
                    Row::gap(2)
                        .children((Flag::country("IN").decorative(), Text::new("Pune").inline())),
                ),
            ]),
        );
        let N::Facts(facts) = node else {
            panic!("not facts");
        };
        let [customer, ships] = facts.items.as_slice() else {
            panic!("not two facts");
        };
        assert_eq!(customer.label, "Customer");
        assert_eq!(customer.value, [text_node("Tanvi Sharma")]);
        assert_eq!(customer.tone, Some(n::Tone::Info));
        assert_eq!(
            customer.icon.as_ref().unwrap().name.as_deref(),
            Some("user")
        );
        assert_eq!(
            customer.details,
            ["tanvi.sharma@example.com", "+91 98765 36760"]
        );
        let [N::Stack(place)] = ships.value.as_slice() else {
            panic!("not a row: {:?}", ships.value);
        };
        assert_eq!(place.children, [text_node("🇮🇳"), text_node("Pune")]);
    }

    #[test]
    fn a_timeline_is_days_of_rows_with_what_is_still_to_come() {
        let node = read(
            Timeline::new("order-steps", "The steps of CS-20481")
                .time_zone("Asia/Kolkata", 19_800)
                .events([
                    TimelineEvent::new("Placed")
                        .detail("2 units for ₹7,848")
                        .at(1_793_773_800)
                        .tone(Tone::Info)
                        .icon(icons::SHOPPING_BAG),
                    TimelineEvent::new("Delivered")
                        .at(1_793_860_200)
                        .meta(Meta::icon(icons::TRUCK, "KL 6392")),
                    TimelineEvent::new("Packing")
                        .detail("Picked and packed at the warehouse")
                        .time_text("Next")
                        .current(),
                    TimelineEvent::new("Shipping").time_text("Later").upcoming(),
                ]),
        );
        let N::Stack(stack) = node else {
            panic!("not a stack");
        };
        assert_eq!(stack.id.as_deref(), Some("order-steps"));
        let rows: Vec<&n::Row> = stack
            .children
            .iter()
            .flat_map(|node| match node {
                N::Section(day) => day.children.iter().collect::<Vec<_>>(),
                other => vec![other],
            })
            .flat_map(|node| match node {
                N::List(list) => list.children.iter().collect::<Vec<_>>(),
                _ => Vec::new(),
            })
            .filter_map(|node| match node {
                N::Row(row) => Some(row),
                _ => None,
            })
            .collect();
        let titles: Vec<&str> = rows.iter().map(|row| row.title.as_str()).collect();
        assert_eq!(titles, ["Placed", "Delivered", "Packing", "Shipping"]);
        assert_eq!(rows[0].subtitle.as_deref(), Some("2 units for ₹7,848"));
        let Some(N::Icon(mark)) = rows[0].leading.as_deref() else {
            panic!("no mark");
        };
        assert_eq!(mark.name.as_deref(), Some("shopping-bag"));
        assert_eq!(mark.tone, Some(n::Tone::Info));
        assert!(rows[0].trailing.is_some());
        assert!(!rows[1].detail.is_empty(), "the delivery's line of facts");
        assert_eq!(rows[2].state, Some(n::RowState::Current));
        assert_eq!(rows[3].state, Some(n::RowState::Upcoming));
    }

    #[test]
    fn a_plain_table_reads_through_its_scrolling_box() {
        let node = read(
            Table::new()
                .head((TableHead::new("Product"), TableHead::new("Qty").numeric()))
                .rows(vec![
                    TableRowEl::new((
                        TableCell::new("Pashmina shawl"),
                        TableCell::new("1").numeric(),
                    ))
                    .key(7_i64),
                ]),
        );
        let N::Table(table) = node else {
            panic!("not a table");
        };
        assert_eq!(table.columns.len(), 2);
        assert!(table.columns[1].numeric);
        assert_eq!(table.rows[0].id.as_deref(), Some("7"));
        assert_eq!(
            table.rows[0].cells,
            [text_node("Pashmina shawl"), text_node("1")]
        );
    }

    #[test]
    fn a_region_reads_where_it_fetches_itself_and_leaves_its_placeholder_out() {
        let node = read(Region::new("order-others").src("/orders/1/others").lazy());
        assert_eq!(
            node,
            N::Region(n::Region {
                id: "order-others".into(),
                src: "/orders/1/others".into(),
                lazy: true,
                live: None,
                children: Vec::new(),
            })
        );
    }

    #[test]
    fn a_component_that_grows_a_part_its_reader_never_heard_of_stops_being_read() {
        let card: Node = el("div")
            .slot("card")
            .children((
                el("div").slot("card-content").child("x"),
                el("div").slot("card-ribbon").child("New"),
            ))
            .into();
        let lowered = lower(&card);
        assert_eq!(lowered.why(), ["slot:card"]);
        assert_eq!(lowered.misses[0].why, "an unexpected `card-ribbon`");
    }
}
