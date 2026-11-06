//! The page: the shell with the entry types down the side, the list in a pane,
//! and the open entry in the pane beside it.

use std::collections::BTreeMap;

use http::StatusCode;
use navree_ui::{
    AppPage, AppShell, Button, ButtonVariant, Grid, NavGroup, NavItem, Shell, Size, Uses, View, el,
    icons,
};

use super::{detail_page, list_href, list_page, shorten, tight};
use crate::EntryKind;

/// What is open beside the list.
pub(super) enum Beside {
    /// Nothing is chosen.
    Nothing,
    /// An entry.
    Entry(Box<detail_page::Detail>),
    /// The address names an entry that isn't there.
    Missing,
}

/// Everything one page of the tool shows.
pub(super) struct Screen {
    pub(super) base: String,
    pub(super) q: String,
    pub(super) kind: Option<EntryKind>,
    pub(super) counts: BTreeMap<EntryKind, u64>,
    /// Where the entries are kept, for the line under the name.
    pub(super) store: String,
    /// The page's own path, which the filter submits to: an open entry stays open.
    pub(super) path: String,
    pub(super) list: list_page::List,
    pub(super) beside: Beside,
}

/// The icon of an entry type.
fn icon_of(kind: EntryKind) -> navree_ui::icons::Icon {
    match kind {
        EntryKind::Request => icons::GLOBE,
        EntryKind::Query => icons::DATABASE,
        EntryKind::Job => icons::LIST_CHECKS,
        EntryKind::Mail => icons::MAIL,
        EntryKind::Event => icons::ZAP,
        EntryKind::Cache => icons::LAYERS,
        EntryKind::Log => icons::SCROLL_TEXT,
        EntryKind::Exception => icons::TRIANGLE_ALERT,
        EntryKind::HttpClient => icons::ARROW_UP_RIGHT,
        EntryKind::Notification => icons::BELL,
        EntryKind::Model => icons::PENCIL,
        EntryKind::Schedule => icons::CLOCK,
        EntryKind::Dump => icons::BUG,
        EntryKind::Ai => icons::SPARKLES,
        EntryKind::Broadcast => icons::RADIO_TOWER,
        EntryKind::Storage => icons::HARD_DRIVE,
        EntryKind::Webhook => icons::WEBHOOK,
        EntryKind::Search => icons::SEARCH,
        EntryKind::SignIn => icons::LOG_IN,
        EntryKind::Ui => icons::MOUSE_POINTER_CLICK,
    }
}

/// `842`, `1.2k`, `15k`, `1.3M`: a count that fits beside a name.
pub(super) fn short_count(count: u64) -> String {
    match count {
        0..=999 => count.to_string(),
        1_000..=9_999 => {
            let tenths = count / 100;
            if tenths.is_multiple_of(10) {
                format!("{}k", tenths / 10)
            } else {
                format!("{}.{}k", tenths / 10, tenths % 10)
            }
        }
        10_000..=999_999 => format!("{}k", count / 1_000),
        _ => {
            let tenths = count / 100_000;
            if tenths.is_multiple_of(10) {
                format!("{}M", tenths / 10)
            } else {
                format!("{}.{}M", tenths / 10, tenths % 10)
            }
        }
    }
}

/// The shell of the tool: the entry types, each with how many there are, and the
/// filter in the top bar. It is Logbook's own, applied to the request, so an app
/// with a shell of its own is not asked for another.
pub(super) fn shell(screen: &Screen) -> Shell {
    let q = screen.q.as_str();
    let mut entries = NavGroup::new("Entries").item(
        NavItem::new("All", list_href(&screen.base, q, None))
            .icon(icons::LIST)
            .current(screen.kind.is_none()),
    );
    for kind in EntryKind::ALL {
        let mut item = NavItem::new(
            kind.label(),
            list_href(&screen.base, q, Some(kind.as_str())),
        )
        .icon(icon_of(kind))
        .current(screen.kind == Some(kind));
        if let Some(count) = screen.counts.get(&kind) {
            item = item.count(short_count(*count));
        }
        entries = entries.item(item);
    }
    Shell::new("Logbook")
        .context(screen.store.clone())
        .home(screen.base.clone())
        .group(entries)
        .search(list_page::search(
            &screen.path,
            q,
            screen.kind.map(EntryKind::as_str),
        ))
}

/// The page.
pub(super) fn page(app: &AppShell, mut screen: Screen) -> View {
    let list_url = list_href(&screen.base, &screen.q, screen.kind.map(EntryKind::as_str));
    let (title, mut actions, status) = match (&screen.beside, screen.kind) {
        (Beside::Entry(detail), _) => {
            let mut actions = Vec::new();
            if detail.replay.is_some() {
                actions.push(Button::outline("Replay").size(Size::Sm).href("#replay"));
            }
            (tight(&shorten(&detail.title, 90)), actions, StatusCode::OK)
        }
        (Beside::Missing, _) => (
            "No such entry".to_owned(),
            Vec::new(),
            StatusCode::NOT_FOUND,
        ),
        (Beside::Nothing, Some(kind)) => (kind.label().to_owned(), Vec::new(), StatusCode::OK),
        (Beside::Nothing, None) => ("Logbook".to_owned(), Vec::new(), StatusCode::OK),
    };
    let mut page: AppPage = app.page(title);
    if !matches!(screen.beside, Beside::Nothing) || screen.kind.is_some() {
        page = page.crumb("Logbook", list_url);
    }
    let clear = el("form")
        .attr("method", "post")
        .attr("action", format!("{}/clear", screen.base))
        .flag("data-ui-form", true)
        .uses(Uses::CORE)
        .child(
            Button::new("Clear")
                .variant(ButtonVariant::Outline)
                .size(Size::Sm)
                .submit(),
        );
    let beside = match std::mem::replace(&mut screen.beside, Beside::Nothing) {
        Beside::Entry(detail) => {
            screen.list.entry_open = true;
            detail_page::pane(*detail)
        }
        Beside::Missing => {
            screen.list.entry_open = true;
            detail_page::missing(&screen.base)
        }
        Beside::Nothing => detail_page::nothing_chosen(),
    };
    for action in actions.drain(..) {
        page = page.actions(action);
    }
    page.actions(clear)
        .body(
            Grid::cols(3)
                .gap(4)
                .children((list_page::pane(screen.list), beside)),
        )
        .to_view()
        .status(status)
}

#[cfg(test)]
mod tests {
    use super::short_count;

    #[test]
    fn counts_fit_beside_a_name() {
        let shown: Vec<String> = [
            0, 7, 999, 1_000, 1_234, 9_999, 10_000, 15_400, 999_999, 1_000_000, 1_250_000,
        ]
        .into_iter()
        .map(short_count)
        .collect();
        assert_eq!(
            shown,
            [
                "0", "7", "999", "1k", "1.2k", "9.9k", "10k", "15k", "999k", "1M", "1.2M"
            ]
        );
    }
}
