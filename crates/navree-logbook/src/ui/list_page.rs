//! The list page, drawn with navree-ui: the type buttons, the filter, the
//! table, and live arrivals as a region that polls and morphs.

use std::time::Duration;

use navree_ui::{
    Alert, AlertTone, Badge, BadgeVariant, Button, ButtonVariant, Empty, Input, Node, Region, Row as Layout, Size, Table, TableCell,
    TableHead, TableRowEl, View, el, icons,
};

use super::Row;

/// How often the list asks for new entries.
const LIVE: Duration = Duration::from_secs(2);

/// One type button: a link, marked when it's the current filter.
pub(super) struct TypeButton {
    pub(super) label: &'static str,
    pub(super) href: String,
    pub(super) on: bool,
}

/// What the list shows.
pub(super) struct List {
    pub(super) base: String,
    pub(super) q: String,
    pub(super) kind: Option<String>,
    pub(super) all_href: String,
    pub(super) buttons: Vec<TypeButton>,
    pub(super) error: String,
    pub(super) rows: Vec<Row>,
    /// The URL to poll; `None` on older pages and after a bad filter.
    pub(super) live_href: Option<String>,
    pub(super) next_href: Option<String>,
}

fn row(row: Row) -> TableRowEl {
    let mut title = vec![Node::from(el("a").href(row.href).child(row.title))];
    for flag in row.flags {
        title.push(" ".into());
        title.push(Badge::new(flag).variant(if row.amber { BadgeVariant::Warning } else { BadgeVariant::Outline }).into());
    }
    let user: Node = if row.user.is_empty() {
        Node::Empty
    } else {
        el("a").href(row.user_href).attr("title", row.user).child(row.user_short).into()
    };
    let counters: Vec<Node> = row
        .counters
        .into_iter()
        .map(|counter| el("span").attr("title", counter.label).child(format!("{} {} ", counter.count, counter.short)).into())
        .collect();
    TableRowEl::new((
        TableCell::new(Badge::new(row.kind_label).variant(BadgeVariant::Outline)),
        TableCell::new(title),
        TableCell::new(row.status),
        TableCell::new(row.duration).numeric(),
        TableCell::new(user).hide_on_mobile(),
        TableCell::new(counters).hide_on_mobile(),
        TableCell::new(el("span").attr("title", row.at).child(row.time)),
    ))
    .key(row.seq.to_string())
}

fn table(rows: Vec<Row>) -> Node {
    Table::new()
        .caption("Recorded entries, newest first")
        .head((
            TableHead::new("Type"),
            TableHead::new("Entry"),
            TableHead::new("Status"),
            TableHead::new("Duration").numeric(),
            TableHead::new("User").hide_on_mobile(),
            TableHead::new("Inside").hide_on_mobile(),
            TableHead::new("Time"),
        ))
        .rows(rows.into_iter().map(row).collect::<Vec<_>>())
        .into()
}

/// The page.
pub(super) fn page(list: List) -> View {
    let filtered = !list.q.is_empty() || list.kind.is_some();
    let search = el("form")
        .attr("role", "search")
        .attr("method", "get")
        .attr("action", list.base.clone())
        .attr("data-ui-target", "rows")
        .children((
            list.kind.map(|kind| el("input").attr("type", "hidden").attr("name", "type").attr("value", kind)),
            Input::search("q")
                .value(list.q)
                .placeholder("Filter: user:… tag:… status:4xx route:… slow failed duration>200")
                .attr("aria-label", "Filter")
                .autocomplete("off"),
        ));
    let top = el("header").slot("topbar").children((
        el("a").href(list.base.clone()).child("Logbook"),
        el("div").slot("topbar-spacer").child(search),
        el("form").attr("method", "post").attr("action", format!("{}/clear", list.base)).flag("data-ui-form", true).child(Button::outline("Clear").size(Size::Sm).submit()),
    ));
    let mut types = vec![type_button("All", list.all_href, !filtered || list.buttons.iter().all(|button| !button.on))];
    types.extend(list.buttons.into_iter().map(|button| type_button(button.label, button.href, button.on)));
    let content = if list.rows.is_empty() {
        let message = if filtered { "Nothing recorded yet for this filter." } else { "Nothing recorded yet." };
        Node::from(Empty::new(message).icon(icons::INBOX))
    } else {
        table(list.rows)
    };
    let mut region = Region::new("rows").children((
        (!list.error.is_empty()).then(|| Alert::new(AlertTone::Danger).title(list.error)),
        content,
    ));
    if let Some(live) = list.live_href {
        region = region.src(live).poll(LIVE);
    }
    View::new("Logbook").body((
        top,
        el("main").slot("page").attr("id", "main").attr("tabindex", -1_i64).children((
            el("h1").slot("page-title").child("Logbook"),
            el("nav").attr("aria-label", "Entry types").child(Layout::gap(1).wrap().children(types)),
            region,
            list.next_href.map(|href| Button::ghost("Older entries").href(href)),
        )),
    ))
}

fn type_button(label: &'static str, href: String, on: bool) -> Button {
    let button = Button::new(label).variant(ButtonVariant::Ghost).size(Size::Sm).href(href);
    if on { button.attr("aria-current", "true") } else { button }
}
