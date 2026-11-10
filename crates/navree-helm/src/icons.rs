//! The panel's icons: a resource's or a page's `ICON` is a [Lucide](https://lucide.dev)
//! name, drawn by Navree UI, so none is fetched from elsewhere.
//!
//! A name read at run time could be any of Lucide's two thousand icons, and a program that
//! can draw any of them holds all of them (about 640 KB more in a release build). So the
//! panel knows the common icons of `NAMED` by default, and with the `all-icons` feature
//! any Lucide name (or an old name Lucide renamed).

use navree_ui::icons::{self, Icon};

/// The icon of a resource or page whose `ICON` is empty or not one the panel knows.
pub(crate) const FALLBACK: Icon = icons::FOLDER;

/// The icons the panel knows without the `all-icons` feature, by Lucide name (and the old
/// names Lucide renamed some of them from), sorted.
#[cfg_attr(feature = "all-icons", allow(dead_code))]
pub(crate) const NAMED: &[(&str, Icon)] = &[
    ("activity", icons::ACTIVITY),
    ("alarm-clock", icons::ALARM_CLOCK),
    ("archive", icons::ARCHIVE),
    ("badge-check", icons::BADGE_CHECK),
    ("banknote", icons::BANKNOTE),
    ("bell", icons::BELL),
    ("bike", icons::BIKE),
    ("book", icons::BOOK),
    ("book-open", icons::BOOK_OPEN),
    ("bookmark", icons::BOOKMARK),
    ("bot", icons::BOT),
    ("box", icons::BOX),
    ("boxes", icons::BOXES),
    ("briefcase", icons::BRIEFCASE),
    ("bug", icons::BUG),
    ("building", icons::BUILDING),
    ("building-2", icons::BUILDING_COMPLEX),
    ("building-complex", icons::BUILDING_COMPLEX),
    ("calendar", icons::CALENDAR),
    ("calendar-days", icons::CALENDAR_DAYS),
    ("camera", icons::CAMERA),
    ("car", icons::CAR),
    ("chart-area", icons::CHART_AREA),
    ("chart-bar", icons::CHART_BAR),
    ("chart-column", icons::CHART_COLUMN),
    ("chart-line", icons::CHART_LINE),
    ("chart-pie", icons::CHART_PIE),
    ("check", icons::CHECK),
    ("chevron-down", icons::CHEVRON_DOWN),
    ("chevron-left", icons::CHEVRON_LEFT),
    ("chevron-right", icons::CHEVRON_RIGHT),
    ("chevrons-up-down", icons::CHEVRONS_UP_DOWN),
    ("circle-check", icons::CIRCLE_CHECK),
    ("circle-dashed", icons::CIRCLE_DASHED),
    ("circle-help", icons::CIRCLE_QUESTION_MARK),
    ("circle-plus", icons::CIRCLE_PLUS),
    ("circle-question-mark", icons::CIRCLE_QUESTION_MARK),
    ("clipboard", icons::CLIPBOARD),
    ("clipboard-list", icons::CLIPBOARD_LIST),
    ("clock", icons::CLOCK),
    ("cloud", icons::CLOUD),
    ("code", icons::CODE),
    ("coffee", icons::COFFEE),
    ("coins", icons::COINS),
    ("command", icons::COMMAND),
    ("contact", icons::CONTACT),
    ("cpu", icons::CPU),
    ("credit-card", icons::CREDIT_CARD),
    ("database", icons::DATABASE),
    ("dollar-sign", icons::DOLLAR_SIGN),
    ("download", icons::DOWNLOAD),
    ("eighth-notes", icons::EIGHTH_NOTES),
    ("external-link", icons::EXTERNAL_LINK),
    ("factory", icons::FACTORY),
    ("file", icons::FILE),
    ("file-text", icons::FILE_TEXT),
    ("files", icons::FILES),
    ("filter", icons::FUNNEL),
    ("flag", icons::FLAG),
    ("folder", icons::FOLDER),
    ("folder-open", icons::FOLDER_OPEN),
    ("funnel", icons::FUNNEL),
    ("gauge", icons::GAUGE),
    ("gift", icons::GIFT),
    ("git-branch", icons::GIT_BRANCH),
    ("globe", icons::GLOBE),
    ("graduation-cap", icons::GRADUATION_CAP),
    ("hammer", icons::HAMMER),
    ("handshake", icons::HANDSHAKE),
    ("heart", icons::HEART),
    ("house", icons::HOUSE),
    ("image", icons::IMAGE),
    ("inbox", icons::INBOX),
    ("info", icons::INFO),
    ("kanban", icons::KANBAN),
    ("key", icons::KEY),
    ("key-round", icons::KEY_ROUND),
    ("landmark", icons::LANDMARK),
    ("layers", icons::LAYERS),
    ("layout-dashboard", icons::LAYOUT_DASHBOARD),
    ("layout-grid", icons::LAYOUT_GRID),
    ("leaf", icons::LEAF),
    ("life-buoy", icons::LIFE_BUOY),
    ("link", icons::LINK),
    ("list", icons::LIST),
    ("list-checks", icons::LIST_CHECKS),
    ("list-todo", icons::LIST_TODO),
    ("lock", icons::LOCK),
    ("log-out", icons::LOG_OUT),
    ("mail", icons::MAIL),
    ("map", icons::MAP),
    ("map-pin", icons::MAP_PIN),
    ("megaphone", icons::MEGAPHONE),
    ("message-square", icons::MESSAGE_SQUARE),
    ("messages-square", icons::MESSAGES_SQUARE),
    ("music", icons::EIGHTH_NOTES),
    ("network", icons::NETWORK),
    ("newspaper", icons::NEWSPAPER),
    ("notebook", icons::NOTEBOOK),
    ("package", icons::PACKAGE),
    ("package-2", icons::PACKAGE_2),
    ("panel-left", icons::PANEL_LEFT),
    ("pencil", icons::PENCIL),
    ("phone", icons::PHONE),
    ("piggy-bank", icons::PIGGY_BANK),
    ("plane", icons::PLANE),
    ("plug", icons::PLUG),
    ("plus", icons::PLUS),
    ("puzzle", icons::PUZZLE),
    ("receipt", icons::RECEIPT),
    ("rocket", icons::ROCKET),
    ("school", icons::SCHOOL),
    ("search", icons::SEARCH),
    ("send", icons::SEND),
    ("server", icons::SERVER),
    ("settings", icons::SETTINGS),
    ("shield", icons::SHIELD),
    ("shield-check", icons::SHIELD_CHECK),
    ("ship-wheel", icons::SHIP_WHEEL),
    ("shopping-bag", icons::SHOPPING_BAG),
    ("shopping-cart", icons::SHOPPING_CART),
    ("sliders-horizontal", icons::SLIDERS_HORIZONTAL),
    ("sparkles", icons::SPARKLES),
    ("square-dashed", icons::SQUARE_DASHED),
    ("star", icons::STAR),
    ("store", icons::STORE),
    ("sun", icons::SUN),
    ("tag", icons::TAG),
    ("tags", icons::TAGS),
    ("terminal", icons::TERMINAL),
    ("ticket", icons::TICKET),
    ("timer", icons::TIMER),
    ("trash", icons::TRASH),
    ("trash-2", icons::TRASH),
    ("trending-down", icons::TRENDING_DOWN),
    ("trending-up", icons::TRENDING_UP),
    ("truck", icons::TRUCK),
    ("user", icons::USER),
    ("user-cog", icons::USER_COG),
    ("user-round", icons::USER_ROUND),
    ("users", icons::USERS),
    ("utensils", icons::UTENSILS),
    ("video", icons::VIDEO),
    ("wallet", icons::WALLET),
    ("warehouse", icons::WAREHOUSE),
    ("webhook", icons::WEBHOOK),
    ("workflow", icons::WORKFLOW),
    ("wrench", icons::WRENCH),
    ("x", icons::X),
    ("zap", icons::ZAP),
];

/// The icon called `name`, when the panel knows it.
fn lookup(name: &str) -> Option<Icon> {
    #[cfg(feature = "all-icons")]
    {
        Icon::lookup(name)
    }
    #[cfg(not(feature = "all-icons"))]
    {
        NAMED
            .binary_search_by(|(known, _)| known.cmp(&name))
            .ok()
            .map(|at| NAMED[at].1)
    }
}

/// Whether the panel knows the icon `name`.
pub(crate) fn exists(name: &str) -> bool {
    lookup(name).is_some()
}

/// The icon `name`, or the fallback when the panel knows none by that name.
pub(crate) fn icon(name: &str) -> Icon {
    lookup(name).unwrap_or(FALLBACK)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_are_lucide_names() {
        assert!(exists("handshake"));
        assert!(exists("folder"));
        assert!(!exists("no-such-icon"));
        assert_eq!(icon("handshake"), icons::HANDSHAKE);
        // Unknown names fall back to a generic icon.
        assert_eq!(icon("no-such-icon"), FALLBACK);
        assert_eq!(icon(""), FALLBACK);
    }

    #[test]
    fn the_icons_the_panel_knows_are_sorted_and_each_is_lucides() {
        for pair in NAMED.windows(2) {
            assert!(pair[0].0 < pair[1].0, "{} before {}", pair[0].0, pair[1].0);
        }
        for (name, icon) in NAMED {
            assert_eq!(Icon::lookup(name), Some(*icon), "{name}");
        }
    }

    #[test]
    fn a_lucide_icon_outside_the_common_set_needs_the_all_icons_feature() {
        let rare = "a-arrow-down";
        assert!(NAMED.iter().all(|(name, _)| *name != rare));
        assert_eq!(exists(rare), cfg!(feature = "all-icons"));
        if cfg!(feature = "all-icons") {
            assert_eq!(icon(rare), icons::A_ARROW_DOWN);
        } else {
            assert_eq!(icon(rare), FALLBACK);
        }
    }
}
