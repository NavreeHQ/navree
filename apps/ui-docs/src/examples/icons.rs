//! Icons and flags.

use navree::ui::flags;
use navree::ui::icons::Icon;
use navree::ui::*;

use crate::catalog::{Example, gallery, single};

const SOURCE: &str = include_str!("icons.rs");

pub fn all() -> Vec<Example> {
    vec![
        crate::icons::all_icons(),
        single!("icons", "usage", "Icons in components", icons_usage),
        single!("icons", "names", "Three ways to name an icon", icons_names),
        single!("icons", "alone", "An icon that stands alone", icons_alone),
        single!("icons", "own", "Your own icons", icons_own),
        crate::flags::all_flags(),
        single!("flags", "usage", "A flag beside its name", flags_usage),
        gallery!("flags", "sizes", "Sizes", flags_sizes).narrow(),
        gallery!("flags", "shapes", "Shapes", flags_shapes).narrow(),
        single!("flags", "places", "Regions and bodies", flags_places),
        single!(
            "flags",
            "in-components",
            "In other components",
            flags_in_components
        )
        .start(),
        single!("flags", "from-data", "A code from data", flags_from_data),
        single!(
            "flags",
            "with-icons",
            "India first, its region, and icons",
            flags_with_icons
        )
        .start(),
    ]
}

fn icons_usage() -> Row {
    Row::gap(3).children((
        Button::primary("New deal").icon(icons::PLUS),
        Button::icon_only(icons::SETTINGS, "Settings"),
        Badge::new("Synced").icon(icons::CHECK),
        components::icon(icons::INBOX),
    ))
}

fn icons_names() -> Row {
    Row::gap(3).children((
        // A constant: your editor completes it.
        Button::outline("Open").icon(icons::MAIL_OPEN),
        // The name as lucide.dev writes it, checked while your app compiles.
        Button::outline("Open").icon(lucide!("mail-open")),
        // A name that arrives at run time. This is the one way that puts every
        // icon into your binary.
        Button::outline("Open")
            .icon(Icon::lookup("mail-open").unwrap_or(icons::CIRCLE_QUESTION_MARK)),
    ))
}

fn icons_alone() -> Row {
    Row::gap(3).children((
        // Hidden from assistive technology: the text beside it says what it means.
        Badge::new("Delivered").icon(icons::CIRCLE_CHECK),
        // Alone, it needs a name.
        components::icon_labelled(icons::CIRCLE_CHECK, "Delivered"),
    ))
}

fn icons_own() -> Row {
    // Each file is read and checked while your app compiles. A script, a style,
    // a link or anything but shapes stops the build.
    const PENNANT: Icon = icon_file!("icons/pennant.svg");
    const ORBIT: Icon = icon_file!("icons/orbit.svg");
    const SEAL: Icon = icon_file!("icons/seal.svg");
    Row::gap(3).children((
        Button::primary("Launch").icon(PENNANT),
        Button::outline("Orbit").icon(ORBIT),
        Badge::new("Verified").icon(SEAL),
    ))
}

fn flags_usage() -> Row {
    Row::gap(6).children((
        Row::gap(2).children((flag!("in").decorative(), "India")),
        Row::gap(2).children((Flag::new(flags::NP).decorative(), "Nepal")),
        Row::gap(2).children((flag!("lk").decorative(), "Sri Lanka")),
    ))
}

fn flags_sizes() -> Vec<(&'static str, Vec<Flag>)> {
    vec![
        ("Xs, 20 px", vec![flag!("IN").size(FlagSize::Xs)]),
        ("Sm, 24 px (default)", vec![flag!("IN")]),
        ("Md, 32 px", vec![flag!("IN").size(FlagSize::Md)]),
        ("Lg, 40 px", vec![flag!("IN").size(FlagSize::Lg)]),
        ("Xl, 48 px", vec![flag!("IN").size(FlagSize::Xl)]),
    ]
}

fn flags_shapes() -> Vec<(&'static str, Vec<Flag>)> {
    vec![
        ("Rect, 4 by 3", vec![flag!("IN").size(FlagSize::Lg)]),
        (
            "Square",
            vec![flag!("IN").size(FlagSize::Lg).shape(FlagShape::Square)],
        ),
        ("Round", vec![flag!("IN").size(FlagSize::Lg).round()]),
    ]
}

fn flags_places() -> Row {
    Row::gap(3).children((
        flag!("EU").size(FlagSize::Lg),
        flag!("UN").size(FlagSize::Lg),
        flag!("GB-ENG").size(FlagSize::Lg),
        flag!("ES-CT").size(FlagSize::Lg),
        flag!("XK").size(FlagSize::Lg),
    ))
}

fn flags_in_components() -> Stack {
    Stack::gap(4).children((
        Row::gap(2).children((
            Badge::new(Row::gap(1).children((flag!("IN").decorative(), "India"))),
            Badge::new(Row::gap(1).children((flag!("AE").decorative(), "United Arab Emirates")))
                .variant(BadgeVariant::Outline),
        )),
        Table::new()
            .head((TableHead::new("Country"), TableHead::new("Opens").numeric()))
            .rows((
                TableRowEl::new((
                    TableCell::new(Row::gap(2).children((flag!("IN").decorative(), "India"))),
                    TableCell::new("12,480").numeric(),
                )),
                TableRowEl::new((
                    TableCell::new(
                        Row::gap(2).children((flag!("AE").decorative(), "United Arab Emirates")),
                    ),
                    TableCell::new("9,215").numeric(),
                )),
            )),
    ))
}

fn flags_with_icons() -> Stack {
    let region = [
        "NP", "BT", "BD", "LK", // neighbours
        "AE", "SG", "JP", "GB", "US", // partners
    ];
    let flags = region.map(Flag::country);
    let tags = [
        Badge::new("Store").icon(icons::STORE),
        Badge::new("Shipped").icon(icons::TRUCK),
        Badge::new("Paid").icon(icons::INDIAN_RUPEE),
        Badge::new("Ludhiana").icon(icons::MAP_PIN),
    ];
    Stack::gap(4).children((
        flag!("IN").size(FlagSize::Xl),
        Row::gap(2).children(flags),
        Row::gap(2).children(tags),
    ))
}

fn flags_from_data() -> Row {
    // A code that comes from data: any case, `gb-sct` or `GB_SCT`. One no flag
    // has draws a neutral placeholder, and never panics.
    let codes = ["in", "GB-SCT", "xx-unknown"];
    Row::gap(4).children(
        codes
            .iter()
            .map(|code| Flag::country(code).size(FlagSize::Lg))
            .collect::<Vec<_>>(),
    )
}
