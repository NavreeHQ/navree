//! The reader of [`DataTable`](crate::DataTable): its columns with how each
//! sorts, its rows, the form that asks for other rows (search and filters),
//! and where the next rows are.
//!
//! A data table is a region that fetches itself, and one form whose controls
//! stand all over it (a search box, a menu of checkboxes for each filter, the
//! rows a page). The reader gathers that form's controls into fields, one
//! field a filter, and leaves out what only arranges the web table: which
//! columns show and in what order, their widths, the density.
//!
//! The rows of a table skip the tree when a page is drawn for a browser: they
//! are written straight to one string. For a native client the table builds
//! them as a tree instead, so they are read like everything else.

use std::borrow::Cow;

use navree_native::action::Action;
use navree_native::input as i;
use navree_native::node as n;

use super::lower::{
    Control, Cx, N, Parts, Read, align_of, attr, elements, fail, flat, has, hidden_in, hooks, id,
    one, slot, text_of,
};
use crate::node::{Element, Node};

/// The runtime hooks a data table carries that this reader answers for.
pub(crate) const HOOKS: &[&str] = &[
    "data-ui-region",
    "data-ui-table",
    "data-ui-src",
    "data-ui-live-src",
    "data-ui-live-event",
    "data-ui-live-catch-up",
    // Arrow keys through the cells: a client has its own way through a list.
    "data-ui-grid",
    // A table that holds every row sorts them in the browser, and the server's answer to
    // the same link only confirms the order: a client asks the server.
    "data-ui-local",
    "data-ui-sort-default",
];

/// The form fields that only arrange the web table: the columns shown and
/// the filters in the bar.
const ARRANGES: &[&str] = &["cols", "fb"];

/// `DataTable`.
pub(crate) fn data_table(cx: &mut Cx, element: &Element) -> Read {
    let Some(id) = id(element) else {
        return fail("a table with no id");
    };
    let Some(src) = attr(element, "data-ui-src") else {
        return fail("a table with no source");
    };
    let mut parts = Parts::of(element);
    let toolbar = parts.slot("table-toolbar");
    let filter_bar = parts.slot("filter-bar");
    // What failed to load, and the filters in force as chips to clear: the
    // fields of the query say the same.
    let _alert = parts.slot("table-alert");
    let _chips = parts.slot("table-chips");
    let bulk = parts.slot("table-bulk");
    // The question a bulk action asks first is a dialog of its own.
    let confirm = parts.slot("alert-dialog");
    let Some(container) = parts.slot("table-container") else {
        return fail("a table with no table in it");
    };
    let footer = parts.slot("table-footer");
    parts.done()?;
    if confirm.is_some() {
        return fail("bulk actions that ask a question first");
    }

    let inside = elements(container)?;
    let [markup] = inside.as_slice() else {
        return fail("a table's box that holds more or less than a table");
    };
    let mut table = n::Table::default();
    for part in elements(markup)? {
        match part.tag() {
            "caption" => table.caption = Some(text_of(part)),
            "colgroup" => {}
            "thead" => table.columns = head(part)?,
            "tbody" if has(part, "data-message") => {
                // One row across every column: empty, or failed.
                let cells: Vec<&Element> = elements(part)?
                    .into_iter()
                    .map(elements)
                    .collect::<Read<Vec<_>>>()?
                    .into_iter()
                    .flatten()
                    .collect();
                let [cell] = cells.as_slice() else {
                    return fail("a message that is not one cell");
                };
                table.empty = Some(Box::new(one(cx.of(cell))));
            }
            "tbody" => {
                // Rows the page keeps as a string are kept as a tree with the request
                // that built them; rows built any other way have none to read.
                let mut kept: Vec<Node> = Vec::new();
                for child in flat(part.child_nodes()) {
                    match child {
                        Node::Raw(html) => {
                            let tree = super::scope::native()
                                .and_then(|request| request.rows(html.as_str()));
                            match tree {
                                Some(rows) => kept.push(rows),
                                None => {
                                    return fail("rows written as markup, with no tree to read");
                                }
                            }
                        }
                        Node::Static(_) => {
                            return fail("rows written as markup, with no tree to read");
                        }
                        _ => {}
                    }
                }
                let mut seen = flat(part.child_nodes());
                seen.retain(|node| !matches!(node, Node::Raw(_)));
                seen.extend(flat(&kept));
                for row in seen {
                    let Node::Element(row) = row else { continue };
                    let detail = slot(row).as_deref() == Some("row-detail");
                    if slot(row).is_some() && !detail {
                        return fail("a row that is more than a record");
                    }
                    if table.rows.is_empty() {
                        roles(row, &mut table.columns)?;
                    }
                    let mut read = data_row(cx, row, table.columns.len())?;
                    if !detail {
                        // What the row opens in place comes before where it leads.
                        read.action = match attr(row, "data-ui-row-opens") {
                            Some(opens) => Some(Action::Request {
                                method: n::Method::Get,
                                href: opens.into_owned(),
                                fields: Vec::new(),
                            }),
                            None => attr(row, "data-ui-row-href")
                                .map(|href| Action::navigate(href.into_owned())),
                        };
                    }
                    table.rows.push(read);
                }
            }
            other => return fail(format!("a <{other}> in a data table")),
        }
    }
    for row in &table.rows {
        if row.cells.len() != table.columns.len() {
            return fail("a row with more or fewer cells than there are columns");
        }
    }
    if let Some(toolbar) = toolbar {
        table.query = query(cx, element, toolbar, filter_bar)?;
    } else if filter_bar.is_some() {
        return fail("filters with no form to send them");
    }
    if let Some(footer) = footer {
        let mut footer = Parts::of(footer);
        if let Some(summary) = footer.slot("table-summary") {
            // A link that counts the rows is not a count.
            let counts = flat(summary.child_nodes())
                .iter()
                .all(|node| !matches!(node, Node::Element(_)));
            table.summary = Some(text_of(summary)).filter(|text| counts && !text.is_empty());
        }
        if let Some(end) = footer.slot("footer-end") {
            let mut end = Parts::of(end);
            // The rows a page: one of the query's fields, read with them.
            let _size = end.slot("page-size");
            if let Some(pages) = end.slot("pagination") {
                let (prev, more) = pagination(pages)?;
                table.prev = prev;
                table.more = more;
            }
            end.done()?;
        }
        footer.done()?;
    }
    // Rows with a box to select them are fields of the form that applies an action
    // to the ones selected: the table stands inside it.
    let children = match bulk {
        Some(bulk) => {
            let mut form = bulk_form(cx, bulk)?;
            form.children.push(N::Table(table));
            vec![N::Form(form)]
        }
        None => vec![N::Table(table)],
    };
    Ok(vec![N::Region(n::Region {
        id,
        src: src.into_owned(),
        lazy: false,
        live: attr(element, "data-ui-live-src").map(|live| n::Live {
            src: live.into_owned(),
            event: attr(element, "data-ui-live-event").map(Cow::into_owned),
        }),
        children,
    })])
}

/// The form that applies an action to the rows selected: where it goes, what it sends
/// unseen, and the actions. The count and the buttons that select every row or none are
/// a script's.
fn bulk_form(cx: &mut Cx, bulk: &Element) -> Read<n::Form> {
    hooks(bulk, &["data-ui-form", "data-ui-bulk"])?;
    let mut parts = Parts::of(bulk);
    let _status = parts.slot("bulk-status");
    let actions = parts.slot("bulk-actions");
    for node in parts.left() {
        match node {
            Node::Element(input)
                if input.tag() == "input" && attr(input, "type").as_deref() == Some("hidden") => {}
            Node::Text(text) if text.trim().is_empty() => {}
            _ => return fail("a part of the bar over a table no reader here knows"),
        }
    }
    let mut hidden = Vec::new();
    hidden_in(bulk.child_nodes(), &mut hidden);
    let children = match actions {
        Some(actions) => {
            hooks(actions, &[])?;
            cx.as_form(|cx| cx.of(actions))
        }
        None => Vec::new(),
    };
    Ok(n::Form {
        id: id(bulk),
        method: n::Method::Post,
        action: attr(bulk, "action")
            .map(Cow::into_owned)
            .unwrap_or_default(),
        hidden,
        children,
    })
}

/// One row of the body: its cells in order, a box to select it, the control that
/// opens its detail, its menu. A row of detail is one cell across the table.
fn data_row(cx: &mut Cx, row: &Element, columns: usize) -> Read<n::TableRow> {
    if row.tag() != "tr" {
        return fail(format!("a <{}> among the rows of a table", row.tag()));
    }
    let key = attr(row, "data-key")
        .or_else(|| attr(row, "id"))
        .map(Cow::into_owned);
    let cells = elements(row)?;
    if slot(row).as_deref() == Some("row-detail") {
        let [cell] = cells.as_slice() else {
            return fail("a detail that is not one cell");
        };
        let mut shown = vec![one(cx.of(cell))];
        while shown.len() < columns {
            shown.push(N::text(""));
        }
        return Ok(n::TableRow {
            id: key,
            cells: shown,
            action: None,
        });
    }
    let mut out = Vec::with_capacity(cells.len());
    for cell in cells {
        if !matches!(cell.tag(), "td" | "th") {
            return fail(format!("a <{}> among the cells of a row", cell.tag()));
        }
        if has(cell, "colspan") || has(cell, "rowspan") {
            return fail("a cell that spans rows or columns");
        }
        out.push(match slot(cell).as_deref() {
            Some("cell-select") => select_cell(cx, cell)?,
            Some("cell-toggle") => toggle_cell(cx, cell)?,
            _ => {
                let read = cx.of(cell);
                if read.is_empty() {
                    N::text("")
                } else {
                    one(read)
                }
            }
        });
    }
    Ok(n::TableRow {
        id: key,
        cells: out,
        action: None,
    })
}

/// The box that selects a row: a field of the form that applies an action to the
/// rows selected.
fn select_cell(cx: &mut Cx, cell: &Element) -> Read<N> {
    let inside = elements(cell)?;
    let [input] = inside.as_slice() else {
        return fail("a cell to select a row that is not one box");
    };
    hooks(input, &["data-ui-select-row"])?;
    let plain = super::slots::without(input, &["data-ui-select-row"]);
    match cx.as_form(|cx| cx.control(&plain))? {
        Control::Field(field) => Ok(N::Field(*field)),
        _ => fail("a cell to select a row whose box is not a field"),
    }
}

/// The control that opens a row's detail: a button that fetches the region again with
/// the row opened or closed.
fn toggle_cell(cx: &mut Cx, cell: &Element) -> Read<N> {
    let inside = elements(cell)?;
    let [link] = inside.as_slice() else {
        return fail("a cell for a detail that is not one control");
    };
    if slot(link).as_deref() != Some("row-toggle") {
        return fail("a cell for a detail that is not its control");
    }
    let action = cx.action_of(link)?;
    let mut glyph = None;
    for part in elements(link)? {
        if slot(part).as_deref() == Some("icon") && glyph.is_none() {
            glyph = Some(super::slots::icon(part)?);
        } else {
            return fail("a part of the control for a detail no reader here knows");
        }
    }
    let Some(label) = attr(link, "aria-label") else {
        return fail("a control for a detail with no name");
    };
    Ok(N::Button(n::Button {
        id: id(link),
        label: label.into_owned(),
        icon: glyph,
        icon_only: true,
        action,
        ..n::Button::default()
    }))
}

/// The roles a native client's rows say their columns have: on each cell
/// of the first row, and a column the table does not show after the others.
fn roles(row: &Element, columns: &mut Vec<n::Column>) -> Read<()> {
    for (at, cell) in elements(row)?.into_iter().enumerate() {
        let Some(role) = attr(cell, "data-role") else {
            continue;
        };
        let role = match role.as_ref() {
            "title" => n::ColumnRole::Title,
            "subtitle" => n::ColumnRole::Subtitle,
            "status" => n::ColumnRole::Status,
            "trailing" => n::ColumnRole::Trailing,
            other => return fail(format!("a column whose role is `{other}`")),
        };
        if let Some(column) = columns.get_mut(at) {
            column.role = Some(role);
        } else {
            columns.push(n::Column {
                key: attr(cell, "data-col").map(Cow::into_owned),
                label: attr(cell, "data-label")
                    .map(Cow::into_owned)
                    .unwrap_or_default(),
                role: Some(role),
                secondary: true,
                ..n::Column::default()
            });
        }
    }
    // The column the page said is the title is the one: the sticky column
    // stands in for it only when none was said.
    let cells = elements(row)?;
    if let Some(said) = cells
        .iter()
        .position(|cell| attr(cell, "data-role").as_deref() == Some("title"))
    {
        for (at, column) in columns.iter_mut().enumerate() {
            if at != said && column.role == Some(n::ColumnRole::Title) {
                column.role = None;
            }
        }
    }
    Ok(())
}

/// The columns, from the one row of the head.
fn head(head: &Element) -> Read<Vec<n::Column>> {
    let rows = elements(head)?;
    let [row] = rows.as_slice() else {
        return fail("a head of grouped columns or of filters");
    };
    let mut columns = Vec::new();
    for cell in elements(row)? {
        // The cells every row has beside its columns: a box, the control for a
        // detail, a menu. They stand in the same places in the head.
        match slot(cell).as_deref() {
            Some("cell-select") => {
                let mut label = String::new();
                for part in elements(cell)? {
                    hooks(part, &["data-ui-select-page"])?;
                    if let Some(words) = attr(part, "aria-label") {
                        label = words.into_owned();
                    }
                }
                columns.push(n::Column {
                    label,
                    ..n::Column::default()
                });
                continue;
            }
            Some("cell-toggle") => {
                columns.push(n::Column::default());
                continue;
            }
            Some("cell-actions") => {
                let mut label = String::new();
                for part in elements(cell)? {
                    label = text_of(part);
                }
                columns.push(n::Column {
                    label,
                    ..n::Column::default()
                });
                continue;
            }
            _ => {}
        }
        if cell.tag() != "th" {
            return fail("a head cell that is not a column");
        }
        let mut label = None;
        let mut sort = None;
        for part in elements(cell)? {
            if slot(part).as_deref() != Some("th") {
                // The handle that resizes the column.
                continue;
            }
            let mut inner = Parts::of(part);
            if let Some(link) = inner.slot("sort-link") {
                let mut words = String::new();
                for child in flat(link.child_nodes()) {
                    if let Node::Text(text) = child {
                        words.push_str(text);
                    }
                }
                label = Some(words.trim().to_owned());
                sort = attr(link, "href").map(|href| n::Sort {
                    href: href.into_owned(),
                    dir: match attr(cell, "aria-sort").as_deref() {
                        Some("ascending") => Some(n::SortDir::Asc),
                        Some("descending") => Some(n::SortDir::Desc),
                        _ => None,
                    },
                });
            } else if let Some(words) = inner.slot("th-label") {
                label = Some(text_of(words));
            }
        }
        let Some(label) = label else {
            return fail("a column with no heading");
        };
        columns.push(n::Column {
            key: attr(cell, "data-col").map(Cow::into_owned),
            label,
            numeric: has(cell, "data-numeric"),
            align: align_of(cell),
            sort,
            // The column that stays in view as the others scroll names the record.
            role: has(cell, "data-sticky").then_some(n::ColumnRole::Title),
            secondary: has(cell, "data-hide-mobile"),
        });
    }
    Ok(columns)
}

/// The form that asks for other rows: the toolbar's form, with every control
/// in the table that names it.
fn query(
    cx: &mut Cx,
    table: &Element,
    toolbar: &Element,
    filter_bar: Option<&Element>,
) -> Read<Option<n::Form>> {
    let mut toolbar = Parts::of(toolbar);
    let Some(form) = toolbar.slot("toolbar-form") else {
        return Ok(None);
    };
    // The rest of the toolbar arranges the web table or says what a client
    // says its own way: the columns, the density, "live", refresh.
    let Some(form_id) = id(form) else {
        return fail("a table's form with no id");
    };
    let mut out = n::Form {
        id: Some(form_id.clone()),
        method: n::Method::Get,
        action: attr(form, "action")
            .map(Cow::into_owned)
            .unwrap_or_default(),
        ..n::Form::default()
    };
    // The search box, and whatever else stands in the form itself.
    let mut inside = Vec::new();
    controls(form, None, &mut inside);
    for control in inside {
        push_control(cx, control, None, &mut out)?;
    }
    // The filters: each menu of the bar is one field, named by its button.
    if let Some(bar) = filter_bar {
        for popover in elements(bar)? {
            if slot(popover).as_deref() != Some("popover-content") {
                continue;
            }
            let label = attr(popover, "aria-label");
            let mut boxes = Vec::new();
            filter(
                cx,
                popover,
                &form_id,
                label.as_deref(),
                &mut boxes,
                &mut out,
            )?;
            out.children.extend(boxes.into_iter().map(N::Field));
        }
    }
    // The rows a page, at the table's foot.
    let mut elsewhere = Vec::new();
    controls(table, Some(&form_id), &mut elsewhere);
    for control in elsewhere {
        let name = attr(control, "name").unwrap_or_default();
        let known = ARRANGES.contains(&name.as_ref())
            || out.hidden.iter().any(|hidden| hidden.name == name)
            || out
                .children
                .iter()
                .any(|node| matches!(node, N::Field(field) if field.name == name));
        if !known {
            push_control(cx, control, None, &mut out)?;
        }
    }
    Ok(Some(out))
}

/// The controls under `element` that belong to the form `form`: every control
/// when `form` is `None`.
fn controls<'a>(element: &'a Element, form: Option<&str>, out: &mut Vec<&'a Element>) {
    for child in flat(element.child_nodes()) {
        let Node::Element(child) = child else {
            continue;
        };
        if matches!(child.tag(), "input" | "select" | "textarea") {
            let named = attr(child, "form");
            if form.is_none() || named.as_deref() == form {
                out.push(child);
            }
        } else if child.tag() != "noscript" {
            controls(child, form, out);
        }
    }
}

/// Adds one control to the query as a field or a hidden value.
fn push_control(
    cx: &mut Cx,
    control: &Element,
    label: Option<String>,
    out: &mut n::Form,
) -> Read<()> {
    if ARRANGES.contains(&attr(control, "name").unwrap_or_default().as_ref()) {
        return Ok(());
    }
    match in_form(cx, control)? {
        Control::Field(mut field) => {
            if let Some(label) = label {
                field.label = Some(match field.label.take() {
                    // "From" under "Placed" is "Placed From" already.
                    Some(own) if own.contains(&label) => own,
                    Some(own) => format!("{label} {own}"),
                    None => label,
                });
            }
            out.children.push(N::Field(*field));
        }
        Control::Hidden(hidden) => out.hidden.push(hidden),
        Control::Button(_) => {}
    }
    Ok(())
}

/// Reads a control that names its form from elsewhere, which the generic
/// reader refuses: here the form is known.
fn in_form(cx: &mut Cx, control: &Element) -> Read<Control> {
    cx.as_form(|cx| cx.control(control))
}

/// One filter's menu as fields: its checkboxes of one name are one field of
/// several choices, each named by the label around it; any other control is
/// a field of its own.
fn filter(
    cx: &mut Cx,
    menu: &Element,
    form: &str,
    label: Option<&str>,
    boxes: &mut Vec<n::Field>,
    out: &mut n::Form,
) -> Read<()> {
    for child in flat(menu.child_nodes()) {
        let Node::Element(child) = child else {
            continue;
        };
        if slot(child).as_deref() == Some("filter-option") {
            let mut inside = Vec::new();
            controls(child, Some(form), &mut inside);
            let [checkbox] = inside.as_slice() else {
                if inside.is_empty() {
                    continue;
                }
                return fail("a choice of a filter with more than one control");
            };
            let name = attr(checkbox, "name")
                .map(Cow::into_owned)
                .unwrap_or_default();
            if ARRANGES.contains(&name.as_str()) {
                continue;
            }
            if attr(checkbox, "type").as_deref() != Some("checkbox") {
                return fail("a choice of a filter that is not a checkbox");
            }
            let mut words = String::new();
            let mut detail = None;
            for part in flat(child.child_nodes()) {
                match part {
                    Node::Text(text) => words.push_str(text),
                    Node::Element(part) if slot(part).as_deref() == Some("filter-count") => {
                        detail = Some(text_of(part));
                    }
                    Node::Element(part) if !matches!(part.tag(), "input" | "svg") => {
                        words.push_str(&text_of(part));
                    }
                    _ => {}
                }
            }
            let value = attr(checkbox, "value").map_or_else(|| "on".to_owned(), Cow::into_owned);
            let at = match boxes.iter().position(|field| field.name == name) {
                Some(at) => at,
                None => {
                    boxes.push(n::Field {
                        id: None,
                        name,
                        label: label.map(str::to_owned),
                        help: None,
                        error: None,
                        required: false,
                        disabled: false,
                        input: i::Input::Choices(i::ChoicesInput::default()),
                    });
                    boxes.len() - 1
                }
            };
            if let i::Input::Choices(choices) = &mut boxes[at].input {
                if has(checkbox, "checked") {
                    choices.values.push(value.clone());
                }
                choices.options.push(i::ChoiceOption {
                    value,
                    label: words.trim().to_owned(),
                    detail,
                    disabled: has(checkbox, "disabled"),
                });
            }
        } else if matches!(child.tag(), "input" | "select" | "textarea") {
            if attr(child, "form").as_deref() == Some(form) {
                push_control(cx, child, label.map(str::to_owned), out)?;
            }
        } else if child.tag() != "noscript" {
            filter(cx, child, form, label, boxes, out)?;
        }
    }
    Ok(())
}

/// Where the rows before and after these are, from the links at the foot:
/// cursors (first, previous, next) or numbered pages (previous … next).
fn pagination(pages: &Element) -> Read<(Option<String>, Option<String>)> {
    let mut links: Vec<&Element> = Vec::new();
    fn walk<'a>(element: &'a Element, links: &mut Vec<&'a Element>) {
        for child in flat(element.child_nodes()) {
            let Node::Element(child) = child else {
                continue;
            };
            if child.tag() == "a" {
                links.push(child);
            } else {
                walk(child, links);
            }
        }
    }
    walk(pages, &mut links);
    let href = |link: Option<&&Element>| {
        link.and_then(|link| attr(link, "href"))
            .map(Cow::into_owned)
    };
    Ok(match links.as_slice() {
        [] => (None, None),
        [only] => (None, href(Some(only))),
        [.., prev, next]
            if links
                .iter()
                .all(|link| slot(link).as_deref() == Some("pagination-text")) =>
        {
            (href(Some(prev)), href(Some(next)))
        }
        [prev, .., next] => (href(Some(prev)), href(Some(next))),
    })
}
