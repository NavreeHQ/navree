//! The dashboard, a resource's list and the choices of a relationship's picker.
//!
//! A list is Navree UI's `DataTable`, described at run time from the resource: one
//! column for each field the list shows, a filter in the bar over it for each of the
//! resource's filters, the search, the sort the user may use, the pages, the rows'
//! menus and the actions on the rows ticked. Its links are the list query every
//! Navree list speaks, so a list's address is the API's.

use http::HeaderMap;
use navree_access::{CREATE, READ};
use navree_http::list::{ListQuery as Grammar, Op};
use navree_http::response::{IntoResponse, Response};
use navree_http::{AppError, Path, RawQuery};
use navree_ui::described::{ColumnSpec, DescribedRow, DescribedTable, name};
use navree_ui::table::{Count, Direction, FilterKind, Total, ValueKind};
use navree_ui::{
    BulkAction, Button, ChoiceList, DataTable, Empty, FilterOption, Heading, Node, OptionQuery,
    RowAction, Stack, TablePage, Text, el, icons,
};

use super::frame::Frame;
use super::{cells, forms, metrics, page_error};
use crate::api::{parse_range, resource};
use crate::cx::Cx;
use crate::field::{Field, FieldKind};
use crate::handle::ResourceInfo;
use crate::panel::Panel;
use crate::present::{Item, Screen, present, screen_fields};
use crate::query::ListQuery;

/// The page sizes a list offers, beside the resource's own.
const PAGE_SIZES: [u32; 4] = [10, 25, 50, 100];

/// The list query of a screen: what [`ListQuery::parse`] reads, with the filters a
/// table writes in its own spelling read as Helm's filters take them: a typed-in
/// filter's `filter[name][contains]`, and a range of dates' `filter[name][gte]` and
/// `[lte]` as `from..to`.
pub(crate) fn screen_query(info: &ResourceInfo, raw: &str) -> Result<ListQuery, AppError> {
    let mut q = ListQuery::parse(raw)?;
    let (grammar, _) = Grammar::parse_lenient(raw);
    let mut ranges: Vec<(String, Option<String>, Option<String>)> = Vec::new();
    for condition in &grammar.filter {
        let Some(filter) = info.filters.iter().find(|f| f.name == condition.field) else {
            continue;
        };
        let value = condition.value().trim().to_owned();
        if value.is_empty() {
            continue;
        }
        match (filter.kind, condition.op) {
            ("custom", Op::Contains) => {
                q.filters.insert(filter.name.clone(), vec![value]);
            }
            ("date_range", Op::Gte | Op::Lte) => {
                let at = match ranges.iter().position(|(name, ..)| *name == filter.name) {
                    Some(at) => at,
                    None => {
                        ranges.push((filter.name.clone(), None, None));
                        ranges.len() - 1
                    }
                };
                let range = &mut ranges[at];
                if condition.op == Op::Gte {
                    range.1.get_or_insert(value);
                } else {
                    range.2.get_or_insert(value);
                }
            }
            _ => {}
        }
    }
    for (name, from, to) in ranges {
        let text = format!("{}..{}", from.unwrap_or_default(), to.unwrap_or_default());
        q.filters.insert(name, vec![text]);
    }
    Ok(q)
}

/// How a field's values bind in the table's URL.
fn kind_of(field: &Field) -> ValueKind {
    match field.kind {
        FieldKind::Number => ValueKind::Float,
        FieldKind::Date => ValueKind::Date,
        FieldKind::DateTime => ValueKind::DateTime,
        _ => ValueKind::Text,
    }
}

/// The table's filter for one of Helm's.
fn filter_kind(kind: &str) -> FilterKind {
    match kind {
        "select" => FilterKind::Multi,
        "belongs_to" => FilterKind::Relation,
        "boolean" => FilterKind::Boolean,
        "date_range" => FilterKind::Range,
        _ => FilterKind::Text,
    }
}

/// The table of `info` with the columns of `fields`: what the user may sort by,
/// search and filter. A `panel` (a record's related rows) has no sort, filter or
/// search of its own. The page sizes on offer are the usual ones and the one the
/// URL `asked` for, which Helm's lists take from 1 to 100.
pub(super) fn table_of(
    cx: &Cx,
    info: &ResourceInfo,
    fields: &[&Field],
    panel: bool,
    asked: Option<u64>,
) -> DescribedTable {
    let readable = info.readable(cx.ability());
    let search = if panel { Vec::new() } else { readable.search() };
    let per = u32::try_from(info.per_page).unwrap_or(25);
    let mut sizes: Vec<u32> = PAGE_SIZES.to_vec();
    if let Some(asked) = asked
        .and_then(|asked| u32::try_from(asked).ok())
        .filter(|asked| (1..=100).contains(asked) && !sizes.contains(asked))
    {
        sizes.push(asked);
        sizes.sort_unstable();
    }
    let mut table = DescribedTable::new()
        .key_kind(ValueKind::Text)
        .per(per)
        .per_pages(sizes)
        .count(Count::Exact)
        .keep("range");
    for field in &info.fields {
        if matches!(field.kind, FieldKind::BelongsTo(_)) {
            table = table.keep(&format!("via[{}]", field.name));
        }
    }
    let filters: Vec<_> = if panel {
        Vec::new()
    } else {
        info.filters.iter().filter(|f| readable.filter(f)).collect()
    };
    // The search box shows when a column is searched; when the resource searches
    // columns the list doesn't show, its first column stands for them.
    let shown_search = fields.iter().any(|f| search.contains(&f.name.as_str()));
    for (at, field) in fields.iter().enumerate() {
        let mut column = ColumnSpec::new(&field.name, &field.label_text()).kind(kind_of(field));
        if !panel && field.sortable && field.is_column() && readable.queryable(&field.name) {
            column = column.sortable();
        }
        if matches!(field.kind, FieldKind::Number | FieldKind::Money { .. }) {
            column = column.numeric();
        }
        let stands_for_search = at == 0 && !search.is_empty() && !shown_search;
        if search.contains(&field.name.as_str()) || stands_for_search {
            column = column.search();
        }
        if at == 0 {
            column = column.locked();
        }
        if let Some(filter) = filters.iter().find(|f| f.name == field.name) {
            column = column
                .filter(filter_kind(filter.kind))
                .options(filter.options.iter().map(|(v, l)| (v.as_str(), l.as_str())));
        }
        table = table.column(column);
    }
    // A filter on what the list doesn't show has a column of its own, off.
    for filter in filters
        .iter()
        .filter(|f| !fields.iter().any(|field| field.name == f.name))
    {
        let mut column = ColumnSpec::new(&filter.name, &filter.label)
            .hidden()
            .filter(filter_kind(filter.kind))
            .options(filter.options.iter().map(|(v, l)| (v.as_str(), l.as_str())));
        if filter.kind == "date_range" {
            column = column.kind(ValueKind::Date);
        }
        table = table.column(column);
    }
    let sort = readable.default_sort();
    let (sorted, direction) = match sort.strip_prefix('-') {
        Some(column) => (column, Direction::Desc),
        None => (sort.as_str(), Direction::Asc),
    };
    if !panel
        && table
            .columns()
            .iter()
            .any(|column| column.name == sorted && column.sortable)
    {
        table = table.sort(sorted, direction);
    }
    table
}

/// The rows of a table: each item's cells, the first linked to its page, in the
/// columns' order.
pub(super) fn rows_of(items: &[Item], skip: Option<&str>) -> Vec<DescribedRow> {
    items
        .iter()
        .map(|item| {
            let cells = item
                .cells
                .iter()
                .filter(|cell| Some(cell.name.as_str()) != skip)
                .enumerate()
                .map(|(at, cell)| {
                    cells::cell(&cell.display, (at == 0).then_some(item.url.as_str()))
                })
                .collect();
            DescribedRow::new(item.id.clone(), cells)
        })
        .collect()
}

/// The actions on the ticked rows the user may run on some record, which the
/// workspace they work in takes from them (on a record's page with `detail`).
pub(super) fn bulk_actions(cx: &Cx, info: &ResourceInfo, detail: bool) -> Vec<BulkAction> {
    info.actions
        .iter()
        .filter(|a| if detail { a.on_detail } else { a.on_index })
        .filter(|a| cx.can(a.ability, info.subject, None) && cx.offers(info, a.ability))
        .map(|a| {
            let action = BulkAction::new(name(&a.name), a.label.clone());
            if a.destructive {
                action.destructive()
            } else {
                action
            }
        })
        .collect()
}

/// A row's menu: open it, edit it, delete it, as far as the user may.
pub(super) fn row_menu(item: &Item) -> Vec<RowAction> {
    let mut actions = vec![RowAction::link("Open", item.url.clone()).icon(icons::EXTERNAL_LINK)];
    if item.can.update {
        actions.push(RowAction::link("Edit", format!("{}/edit", item.url)).icon(icons::PENCIL));
    }
    if item.can.delete {
        actions.push(
            RowAction::post("Delete", format!("{}/delete", item.url))
                .icon(icons::TRASH)
                .destructive(),
        );
    }
    actions
}

/// The choices of the relationship filters of `info`: the first records of each
/// related resource the user may read, and those the query names.
async fn relation_filters(
    cx: &Cx,
    info: &ResourceInfo,
    q: &ListQuery,
) -> Result<Vec<(&'static str, Vec<FilterOption>)>, AppError> {
    let readable = info.readable(cx.ability());
    let mut out = Vec::new();
    for filter in info.filters.iter().filter(|f| f.kind == "belongs_to") {
        if !readable.filter(filter) {
            continue;
        }
        let Some(related) = filter
            .related
            .and_then(|r| cx.core.registry.by_type(r.type_id))
        else {
            continue;
        };
        let selected = q.filters.get(&filter.name).cloned().unwrap_or_default();
        let options = forms::relation_options(cx, related.as_ref(), &selected)
            .await?
            .into_iter()
            .map(|(id, title)| FilterOption::new(id, title))
            .collect();
        out.push((name(&filter.name), options));
    }
    Ok(out)
}

/// `href` with `name` set to `value`, in place of what it held.
fn with_param(href: &str, name: &str, value: &str) -> String {
    let (path, query) = href.split_once('?').unwrap_or((href, ""));
    let mut out = form_urlencoded::Serializer::new(String::new());
    for (key, kept) in form_urlencoded::parse(query.as_bytes()) {
        if key != name {
            out.append_pair(&key, &kept);
        }
    }
    out.append_pair(name, value);
    format!("{path}?{}", out.finish())
}

// ---------------------------------------------------------------------------
// The dashboard
// ---------------------------------------------------------------------------

/// `GET {path}`: the dashboard.
pub(crate) async fn dashboard(panel: Panel, headers: HeaderMap) -> Response {
    let cx = &panel.cx;
    let core = &cx.core;
    let base = core.options.base();
    let frame = Frame::new(&panel, &headers, base);
    let mut sections: Vec<Node> = Vec::new();
    if core.options.dashboard {
        for resource in core.registry.resources() {
            let info = resource.info();
            let readable = info.readable(cx.ability());
            if !cx.can(READ, info.subject, None)
                || !info
                    .metrics
                    .iter()
                    .any(|m| m.on_dashboard && readable.metric(m))
            {
                continue;
            }
            let cards = metrics::cards(cx, resource.as_ref(), Default::default(), true, None).await;
            if cards.is_empty() {
                continue;
            }
            sections.push(
                el("section")
                    .attr("aria-label", info.label)
                    .child(
                        Stack::gap(3).children((
                            Heading::h2(
                                el("a")
                                    .href(format!("{base}/resources/{}", info.slug))
                                    .child(info.label),
                            ),
                            metrics::grid(cards),
                        )),
                    )
                    .into(),
            );
        }
    }
    let body: Node = if core.registry.resources().is_empty() {
        Empty::new("No resources yet")
            .icon(icons::INBOX)
            .description(Stack::gap(2).children((
                Text::new(
                    "Describe a model once and it appears here with a list, filters, a detail page and forms:",
                ),
                Text::code("navree make:resource deals Deal"),
                Text::muted("Then grant helm.view to the roles that should see the panel."),
            )))
            .into()
    } else if sections.is_empty() {
        Empty::new("Welcome")
            .icon(icons::LAYOUT_DASHBOARD)
            .description(
                "Pick a resource from the menu. Metrics marked on_dashboard() appear here.",
            )
            .into()
    } else {
        Stack::gap(6).children(sections).into()
    };
    frame.page("Dashboard").await.body(body).into_response()
}

// ---------------------------------------------------------------------------
// Lists
// ---------------------------------------------------------------------------

/// `GET {path}/resources/{resource}`: the list.
pub(crate) async fn list(
    panel: Panel,
    Path(slug): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    let cx = &panel.cx;
    let result = async {
        let resource = resource(cx, &slug)?;
        let info = resource.info();
        let raw = query.as_deref().unwrap_or("");
        let q = screen_query(info, raw)?;
        let range = parse_range(query.as_deref())?;
        let base = cx.core.options.base();
        let list_url = format!("{base}/resources/{}", info.slug);
        let fields = screen_fields(info, Screen::Index);
        let shape = table_of(cx, info, &fields, false, q.per_page);
        let table_query = shape.query(list_url.clone(), raw);
        // The rows are counted up to the resource's cap, unless the user asks for all.
        let page = resource
            .list(
                cx,
                &ListQuery {
                    capped: !table_query.count_asked(),
                    ..q.clone()
                },
            )
            .await?;
        let items = cells::in_zone(
            present(cx, resource.as_ref(), &page.records, Screen::Index).await?,
            cx.zone,
        );
        let here = if raw.is_empty() {
            list_url.clone()
        } else {
            format!("{list_url}?{raw}")
        };
        let frame = Frame::new(&panel, &headers, here);
        let meta = page.meta;
        let rows = TablePage {
            rows: rows_of(&items, None),
            total: if page.capped {
                Total::AtLeast(meta.total)
            } else {
                Total::Exact(meta.total)
            },
            page: u32::try_from(meta.current_page).unwrap_or(u32::MAX),
            has_more: meta.current_page < meta.last_page,
            next: None,
            prev: None,
            version: None,
        };
        let can_create = cx.can(CREATE, info.subject, None) && cx.offers(info, CREATE);
        let new_url = format!("{list_url}/new");
        let new_label = format!("New {}", info.singular.to_lowercase());
        // What the page waits for comes first: the table is drawn at once after.
        let relations = relation_filters(cx, info, &q).await?;
        let export = crate::export::offered(cx, info).await;
        let href = table_query.href();
        let range_link: metrics::RangeLink<'_> = &|range: &str| with_param(&href, "range", range);
        let cards = metrics::cards(cx, resource.as_ref(), range, false, Some(range_link)).await;
        let mut page = frame
            .page(info.label)
            .await
            .crumb(cx.core.options.title.clone(), base.to_owned());
        if can_create {
            page = page.actions(
                Button::primary(new_label.clone())
                    .icon(icons::PLUS)
                    .href(new_url.clone()),
            );
        }
        let actions = bulk_actions(cx, info, false);
        let mut empty =
            Empty::new(format!("No {} yet.", info.label.to_lowercase())).icon(icons::INBOX);
        if can_create {
            empty = empty.actions(Button::primary(new_label).href(new_url));
        }
        let by_key =
            |row: &DescribedRow| items.iter().find(|item| Some(item.id.as_str()) == row.id());
        let mut table = DataTable::new("records", &table_query, &rows)
            .caption(info.label)
            .filter_bar()
            .without_select_all_matching()
            .empty(empty)
            .row_href(|row: &DescribedRow| {
                by_key(row).map(|item| item.url.clone()).unwrap_or_default()
            })
            .row_actions(|row: &DescribedRow| by_key(row).map(row_menu).unwrap_or_default());
        for (column, options) in relations {
            table = table.options(column, options);
        }
        if !actions.is_empty() {
            table = table.bulk(format!("{list_url}/actions"), actions);
        } else {
            table = table.can_select(false);
        }
        if export {
            table = table.export(format!("{list_url}/export.csv"));
        }
        if let Some(token) = frame.csrf() {
            table = table.csrf(token.to_owned());
        }
        let body: Node = table.into();
        Ok::<_, AppError>(
            page.body(((!cards.is_empty()).then(|| metrics::grid(cards)), body))
                .into_response(),
        )
    }
    .await;
    result.unwrap_or_else(|error| page_error(&cx.core, error))
}

/// `GET {path}/resources/{resource}/choices?q=..`: the records of a resource the
/// user may read whose title holds `q`, as the choices of a relationship's picker.
pub(crate) async fn choices(
    panel: Panel,
    Path(slug): Path<String>,
    RawQuery(query): RawQuery,
) -> Response {
    let cx = &panel.cx;
    let result = async {
        let resource = resource(cx, &slug)?;
        if !cx.can(READ, resource.info().subject, None) {
            return Err(AppError::forbidden());
        }
        let ask = OptionQuery::parse(query.as_deref().unwrap_or(""));
        let search = (!ask.is_empty()).then(|| ask.text());
        let found = resource
            .lookup(cx, search, None, ask.limit() as u64)
            .await?;
        Ok::<_, AppError>(ChoiceList::new().choices(found).into_response())
    }
    .await;
    result.unwrap_or_else(|error| page_error(&cx.core, error))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_parameter_is_set_in_place_of_what_it_held() {
        assert_eq!(
            with_param("/admin/resources/deals?range=7&page=2", "range", "30"),
            "/admin/resources/deals?page=2&range=30"
        );
        assert_eq!(
            with_param("/admin/resources/deals", "range", "month"),
            "/admin/resources/deals?range=month"
        );
    }
}
