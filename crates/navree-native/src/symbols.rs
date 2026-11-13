//! The system symbols the toolkit's icons and the standard actions stand for:
//! one table, owned here, so every client and the server agree on it.
//!
//! A client draws a system's own symbol where one means the same as the
//! page's icon (the share sheet's arrow, a tab's filled house when it is
//! selected) and the icon's paths otherwise. The server fills
//! [`Icon::symbol`](crate::node::Icon::symbol) from the icon's name with
//! [`Icon::with_symbol`]; a standard action's symbol is [`for_standard`].
//! An Android app bundles the Material Symbols it may be sent when it is
//! built: [`table`] lists them, and the schema carries the same as
//! `x-symbols`.
//!
//! The names are SF Symbols on Apple systems and Material Symbols on Android.
//! A name a system lacks only means the client draws the paths.

use serde_json::{Map, Value, json};

use crate::node::{Icon, Standard, SystemSymbol};

/// One symbol: the toolkit's icon names that mean it, and its name in each
/// system's set.
struct Entry {
    names: &'static [&'static str],
    apple: &'static str,
    apple_filled: Option<&'static str>,
    material: &'static str,
}

const fn entry(
    names: &'static [&'static str],
    apple: &'static str,
    apple_filled: Option<&'static str>,
    material: &'static str,
) -> Entry {
    Entry {
        names,
        apple,
        apple_filled,
        material,
    }
}

/// The symbols of the standard actions, in [`Standard`]'s order.
const STANDARD: [(Standard, Entry); 8] = [
    (
        Standard::Share,
        entry(
            &[],
            "square.and.arrow.up",
            Some("square.and.arrow.up.fill"),
            "share",
        ),
    ),
    (
        Standard::Delete,
        entry(&[], "trash", Some("trash.fill"), "delete"),
    ),
    (Standard::Add, entry(&[], "plus", None, "add")),
    (Standard::Edit, entry(&[], "pencil", None, "edit")),
    (
        Standard::Search,
        entry(&[], "magnifyingglass", None, "search"),
    ),
    (Standard::Done, entry(&[], "checkmark", None, "check")),
    (Standard::Cancel, entry(&[], "xmark", None, "close")),
    (
        Standard::More,
        entry(
            &[],
            "ellipsis.circle",
            Some("ellipsis.circle.fill"),
            "more_vert",
        ),
    ),
];

/// The toolkit's icons that a system symbol means the same as: the app's
/// destinations and the actions a page names most.
const NAMED: &[Entry] = &[
    entry(&["house", "home"], "house", Some("house.fill"), "home"),
    entry(
        &["layout-dashboard", "layout-grid"],
        "square.grid.2x2",
        Some("square.grid.2x2.fill"),
        "dashboard",
    ),
    entry(
        &["shopping-cart"],
        "cart",
        Some("cart.fill"),
        "shopping_cart",
    ),
    entry(&["shopping-bag"], "bag", Some("bag.fill"), "shopping_bag"),
    entry(
        &["package", "box"],
        "shippingbox",
        Some("shippingbox.fill"),
        "inventory_2",
    ),
    entry(
        &["store"],
        "storefront",
        Some("storefront.fill"),
        "storefront",
    ),
    entry(
        &["truck"],
        "truck.box",
        Some("truck.box.fill"),
        "local_shipping",
    ),
    entry(
        &["receipt"],
        "receipt",
        Some("receipt.fill"),
        "receipt_long",
    ),
    entry(
        &["credit-card"],
        "creditcard",
        Some("creditcard.fill"),
        "credit_card",
    ),
    entry(&["tag"], "tag", Some("tag.fill"), "sell"),
    entry(
        &["user", "circle-user"],
        "person",
        Some("person.fill"),
        "person",
    ),
    entry(&["users"], "person.2", Some("person.2.fill"), "group"),
    entry(
        &["building-2", "building"],
        "building.2",
        Some("building.2.fill"),
        "apartment",
    ),
    entry(
        &["settings", "settings-2"],
        "gearshape",
        Some("gearshape.fill"),
        "settings",
    ),
    entry(&["inbox"], "tray", Some("tray.fill"), "inbox"),
    entry(&["bell"], "bell", Some("bell.fill"), "notifications"),
    entry(&["mail"], "envelope", Some("envelope.fill"), "mail"),
    entry(
        &["message-square", "message-circle"],
        "bubble.left",
        Some("bubble.left.fill"),
        "chat_bubble",
    ),
    entry(&["send"], "paperplane", Some("paperplane.fill"), "send"),
    entry(&["phone"], "phone", Some("phone.fill"), "call"),
    entry(&["calendar"], "calendar", None, "calendar_today"),
    entry(
        &["clock", "clock-4"],
        "clock",
        Some("clock.fill"),
        "schedule",
    ),
    entry(&["map"], "map", Some("map.fill"), "map"),
    entry(&["map-pin"], "mappin", None, "location_on"),
    entry(
        &["file-text", "file"],
        "doc.text",
        Some("doc.text.fill"),
        "description",
    ),
    entry(&["folder"], "folder", Some("folder.fill"), "folder"),
    entry(
        &["archive"],
        "archivebox",
        Some("archivebox.fill"),
        "archive",
    ),
    entry(&["image"], "photo", Some("photo.fill"), "image"),
    entry(&["camera"], "camera", Some("camera.fill"), "photo_camera"),
    entry(
        &["chart-bar", "chart-column", "bar-chart"],
        "chart.bar",
        Some("chart.bar.fill"),
        "bar_chart",
    ),
    entry(
        &["trending-up"],
        "chart.line.uptrend.xyaxis",
        None,
        "trending_up",
    ),
    entry(&["star"], "star", Some("star.fill"), "star"),
    entry(&["heart"], "heart", Some("heart.fill"), "favorite"),
    entry(&["lock"], "lock", Some("lock.fill"), "lock"),
    entry(&["link"], "link", None, "link"),
    entry(&["list"], "list.bullet", None, "list"),
    entry(
        &["filter", "list-filter"],
        "line.3.horizontal.decrease.circle",
        Some("line.3.horizontal.decrease.circle.fill"),
        "filter_list",
    ),
    entry(&["arrow-up-down"], "arrow.up.arrow.down", None, "swap_vert"),
    entry(&["refresh-cw"], "arrow.clockwise", None, "refresh"),
    entry(
        &["download"],
        "square.and.arrow.down",
        Some("square.and.arrow.down.fill"),
        "download",
    ),
    entry(
        &["log-out"],
        "rectangle.portrait.and.arrow.right",
        None,
        "logout",
    ),
    entry(&["info"], "info.circle", Some("info.circle.fill"), "info"),
    entry(
        &["circle-help"],
        "questionmark.circle",
        Some("questionmark.circle.fill"),
        "help",
    ),
    entry(
        &["triangle-alert"],
        "exclamationmark.triangle",
        Some("exclamationmark.triangle.fill"),
        "warning",
    ),
    entry(&["search"], "magnifyingglass", None, "search"),
    entry(&["plus"], "plus", None, "add"),
    entry(&["trash-2", "trash"], "trash", Some("trash.fill"), "delete"),
    entry(&["pencil", "square-pen"], "pencil", None, "edit"),
    entry(
        &["share", "share-2"],
        "square.and.arrow.up",
        Some("square.and.arrow.up.fill"),
        "share",
    ),
    entry(
        &["ellipsis", "ellipsis-vertical"],
        "ellipsis.circle",
        Some("ellipsis.circle.fill"),
        "more_vert",
    ),
    entry(&["check"], "checkmark", None, "check"),
    entry(&["x"], "xmark", None, "close"),
    // Health and fitness: the destinations and the figures of a day.
    entry(
        &["heart-pulse"],
        "bolt.heart",
        Some("bolt.heart.fill"),
        "monitor_heart",
    ),
    entry(&["activity"], "waveform.path.ecg", None, "vital_signs"),
    entry(&["moon"], "moon", Some("moon.fill"), "bedtime"),
    entry(&["bed"], "bed.double", Some("bed.double.fill"), "bed"),
    entry(&["sunrise"], "sunrise", Some("sunrise.fill"), "wb_twilight"),
    entry(&["sun"], "sun.max", Some("sun.max.fill"), "light_mode"),
    entry(&["brain"], "brain.head.profile", None, "psychology"),
    entry(&["footprints"], "figure.walk", None, "directions_walk"),
    entry(
        &["person-standing"],
        "figure.stand",
        None,
        "accessibility_new",
    ),
    entry(
        &["flame"],
        "flame",
        Some("flame.fill"),
        "local_fire_department",
    ),
    entry(&["timer"], "timer", None, "timer"),
    entry(&["hourglass"], "hourglass", None, "hourglass_empty"),
    entry(
        &["mountain"],
        "mountain.2",
        Some("mountain.2.fill"),
        "landscape",
    ),
    entry(
        &["dumbbell"],
        "dumbbell",
        Some("dumbbell.fill"),
        "fitness_center",
    ),
    entry(&["bike"], "bicycle", None, "directions_bike"),
    entry(&["waves", "waves-horizontal"], "water.waves", None, "waves"),
    entry(&["zap"], "bolt", Some("bolt.fill"), "bolt"),
    entry(
        &["droplet", "droplets"],
        "drop",
        Some("drop.fill"),
        "water_drop",
    ),
    entry(&["thermometer"], "thermometer.medium", None, "thermostat"),
    entry(&["wind"], "wind", None, "air"),
    entry(
        &["volume-2"],
        "speaker.wave.2",
        Some("speaker.wave.2.fill"),
        "volume_up",
    ),
    entry(
        &["shield-check"],
        "checkmark.shield",
        Some("checkmark.shield.fill"),
        "verified_user",
    ),
    entry(&["sparkles"], "sparkles", None, "auto_awesome"),
    entry(
        &["radio"],
        "antenna.radiowaves.left.and.right",
        None,
        "sensors",
    ),
];

fn symbol(entry: &Entry) -> SystemSymbol {
    SystemSymbol {
        apple: entry.apple.to_owned(),
        apple_filled: entry.apple_filled.map(str::to_owned),
        material: entry.material.to_owned(),
    }
}

/// The system symbol the toolkit's icon `name` means the same as, when there
/// is one.
///
/// ```
/// use navree_native::symbols;
///
/// let cart = symbols::for_name("shopping-cart").unwrap();
/// assert_eq!((cart.apple.as_str(), cart.material.as_str()), ("cart", "shopping_cart"));
/// assert_eq!(cart.apple_filled.as_deref(), Some("cart.fill"));
/// assert!(symbols::for_name("brain-circuit").is_none());
/// ```
pub fn for_name(name: &str) -> Option<SystemSymbol> {
    NAMED
        .iter()
        .find(|entry| entry.names.contains(&name))
        .map(symbol)
}

/// The system symbol of a standard action.
pub fn for_standard(standard: Standard) -> SystemSymbol {
    // Every action has its row: one added to the list does not build until
    // it has its symbols.
    let at = match standard {
        Standard::Share => 0,
        Standard::Delete => 1,
        Standard::Add => 2,
        Standard::Edit => 3,
        Standard::Search => 4,
        Standard::Done => 5,
        Standard::Cancel => 6,
        Standard::More => 7,
    };
    symbol(&STANDARD[at].1)
}

/// The whole table, as the schema's `x-symbols` holds it: `standard` by the
/// action's name, `names` by the toolkit's icon name.
pub fn table() -> Value {
    let one = |entry: &Entry| serde_json::to_value(symbol(entry)).unwrap_or_default();
    let standard: Map<String, Value> = STANDARD
        .iter()
        .map(|(standard, entry)| {
            let name = serde_json::to_value(standard)
                .ok()
                .and_then(|name| name.as_str().map(str::to_owned))
                .unwrap_or_default();
            (name, one(entry))
        })
        .collect();
    let names: Map<String, Value> = NAMED
        .iter()
        .flat_map(|entry| {
            entry
                .names
                .iter()
                .map(move |name| ((*name).to_owned(), one(entry)))
        })
        .collect();
    json!({ "standard": standard, "names": names })
}

impl Icon {
    /// The same icon, with the system symbol its name means, when there is
    /// one: what a server does to an icon before it is sent.
    #[must_use]
    pub fn with_symbol(mut self) -> Self {
        if self.symbol.is_none() {
            self.symbol = self.name.as_deref().and_then(for_name);
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn every_standard_action_has_its_systems_symbols() {
        let mut seen = BTreeSet::new();
        for (standard, entry) in &STANDARD {
            assert!(seen.insert(format!("{standard:?}")), "{standard:?} twice");
            assert_eq!(for_standard(*standard).apple, entry.apple);
            assert!(!entry.apple.is_empty() && !entry.material.is_empty());
        }
        assert_eq!(seen.len(), STANDARD.len());
        assert_eq!(for_standard(Standard::Share).material, "share");
    }

    #[test]
    fn a_name_means_one_symbol_and_the_table_is_written_whole() {
        let mut names = BTreeSet::new();
        for entry in NAMED {
            for name in entry.names {
                assert!(names.insert(*name), "`{name}` is in the table twice");
            }
            assert!(
                entry
                    .material
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b == b'_' || b.is_ascii_digit())
            );
        }
        let table = table();
        assert_eq!(table["names"].as_object().unwrap().len(), names.len());
        assert_eq!(table["standard"]["more"]["material"], "more_vert");
        assert_eq!(table["names"]["house"]["apple_filled"], "house.fill");
    }

    #[test]
    fn the_destinations_of_a_health_app_have_their_systems_symbols() {
        for (name, apple, material) in [
            ("heart-pulse", "bolt.heart", "monitor_heart"),
            ("moon", "moon", "bedtime"),
            ("activity", "waveform.path.ecg", "vital_signs"),
            ("brain", "brain.head.profile", "psychology"),
            ("footprints", "figure.walk", "directions_walk"),
        ] {
            let symbol = for_name(name).unwrap_or_else(|| panic!("no symbol for `{name}`"));
            assert_eq!(
                (symbol.apple.as_str(), symbol.material.as_str()),
                (apple, material)
            );
        }
        // A destination's symbol is filled when it is the selected one, where
        // the system has a filled form.
        assert_eq!(
            for_name("moon")
                .and_then(|symbol| symbol.apple_filled)
                .as_deref(),
            Some("moon.fill")
        );
    }

    #[test]
    fn an_icon_takes_the_symbol_its_name_means() {
        let icon = Icon {
            name: Some("inbox".to_owned()),
            ..Icon::default()
        }
        .with_symbol();
        assert_eq!(
            icon.symbol.map(|symbol| symbol.apple),
            Some("tray".to_owned())
        );
        let own = Icon {
            name: Some("brain-circuit".to_owned()),
            ..Icon::default()
        };
        assert_eq!(own.clone().with_symbol(), own);
    }
}
