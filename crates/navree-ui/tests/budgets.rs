//! The speed budgets. A regression fails the build. Sizes hold everywhere;
//! render times only mean something in a release build, so they run there
//! (`cargo test --release -p navree-ui --test budgets`), with room for a busy
//! machine: they catch a change of scale, not a bad day.

use navree_ui::assets::{self, Asset, get};
use navree_ui::{
    Button, Card, Combobox, Dialog, Grid, NavGroup, NavItem, Node, Select, Shell, Stat, Uses, View,
    icons, measure,
};

fn gzip(asset: &Asset) -> usize {
    asset.gzip.map_or(asset.bytes.len(), <[u8]>::len)
}

fn size(name: &str) -> usize {
    gzip(get(name).unwrap_or_else(|| panic!("no asset {name}")))
}

/// The core runtime is what every page with a command, a form or a boosted link needs.
/// Its ceiling leaves it room for a fix, not for a feature: what only some pages use is
/// a module of its own (see the test after this one).
#[test]
fn the_core_runtime_is_at_most_13_kb_gzip() {
    assert!(size("core") <= 13 * 1024, "{}", size("core"));
}

/// The files the core runtime is built from, and no others: a new behaviour that only
/// some pages need (a chat, a generated surface, an island) can't grow it by an import.
#[test]
fn the_core_runtime_is_built_from_its_own_files_only() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("runtime/src");
    let mut seen = std::collections::BTreeSet::new();
    let mut next = vec!["core.js".to_owned()];
    while let Some(file) = next.pop() {
        if !seen.insert(file.clone()) {
            continue;
        }
        let text =
            std::fs::read_to_string(src.join(&file)).unwrap_or_else(|e| panic!("{file}: {e}"));
        let mut rest = text.as_str();
        while let Some(at) = rest.find("from \"./") {
            rest = &rest[at + 8..];
            let end = rest.find('"').expect("an import ends");
            next.push(rest[..end].to_owned());
        }
    }
    assert_eq!(
        seen.into_iter().collect::<Vec<_>>(),
        [
            "commands.js",
            "core.js",
            "dom.js",
            "forms.js",
            "morph.js",
            "nav.js",
            "ops.js",
            "ui.js"
        ]
    );
}

#[test]
fn the_table_module_is_at_most_5_kb_gzip() {
    assert!(size("table") <= 5 * 1024, "{}", size("table"));
}

#[test]
fn the_select_module_is_at_most_10_kb_gzip() {
    assert!(size("select") <= 10 * 1024, "{}", size("select"));
}

#[test]
fn a_page_with_a_picker_loads_the_select_and_overlay_modules_and_nothing_else() {
    for page in [
        View::new("x").body(Select::new("s").option("a", "A")),
        View::new("x").body(Combobox::new("c").option("a", "A")),
    ] {
        assert_eq!(page.uses(), Uses::OVERLAY.with(Uses::SELECT));
        let html = page.to_html();
        assert!(html.contains(assets::SELECT_URL), "{html}");
        assert!(html.contains(assets::OVERLAY_URL));
        assert!(!html.contains(assets::CORE_URL));
        assert!(!html.contains(assets::TABLE_URL));
    }
}

/// The calendar, the range picker and a date picker's calendar. The box a person types a
/// date in and the time picker are modules of their own, so a page of calendars or of range
/// pickers loads this one alone.
#[test]
fn the_dates_module_is_at_most_7_kb_gzip() {
    assert!(size("dates") <= 7 * 1024, "{}", size("dates"));
}

/// The box a person types a date in: the reading of typed dates in every locale, and what
/// the box says when it can't take one.
#[test]
fn the_date_box_module_is_at_most_4_kb_gzip() {
    assert!(size("date-text") <= 4 * 1024, "{}", size("date-text"));
}

/// The time picker: its typed times, its list, and the joint date-and-time control's field.
#[test]
fn the_time_module_is_at_most_5_kb_gzip() {
    assert!(size("time") <= 5 * 1024, "{}", size("time"));
}

/// Each date and time component loads the modules it is drawn with, and no others: a
/// calendar or a range picker never brings the box's reader, a time picker never brings a
/// calendar, and a date picker drawn as a button has no box.
#[test]
fn each_date_component_loads_its_own_modules_only() {
    use navree_ui::dates::{Calendar, DatePicker, DateRangePicker, PickerStyle, TimePicker};

    let loaded = |body: Node| linked(&View::new("x").body(body).to_html());
    let dates = ["navree-ui", "dates-styles"];
    assert_eq!(
        loaded(Calendar::new("c").into()),
        (vec!["dates"], dates.to_vec())
    );
    assert_eq!(
        loaded(DateRangePicker::new("from", "to").into()),
        (vec!["overlay", "dates"], dates.to_vec())
    );
    assert_eq!(
        loaded(DatePicker::new("due").into()),
        (vec!["overlay", "dates", "date-text"], dates.to_vec())
    );
    assert_eq!(
        loaded(DatePicker::new("due").style(PickerStyle::Button).into()),
        (vec!["overlay", "dates"], dates.to_vec())
    );
    assert_eq!(
        loaded(TimePicker::new("at").into()),
        (vec!["overlay", "time"], vec!["navree-ui", "time-styles"])
    );
}

#[test]
fn the_charts_module_is_at_most_3_kb_gzip() {
    assert!(size("charts") <= 3 * 1024, "{}", size("charts"));
}

/// The chart paint sheet (gradients, glow, motion, painted sparklines, meters and bars)
/// and the count-up module stay small: every painted mark on a page links them.
#[test]
fn the_chart_paint_sheet_is_at_most_1_5_kb_and_the_count_up_module_1_kb_gzip() {
    assert!(size("chart-paint") <= 1536, "{}", size("chart-paint"));
    assert!(size("count-up") <= 1024, "{}", size("count-up"));
}

/// The radial sheet (rings, gauges and the day clock) stays small: a page with one ring
/// links it, and no script comes with a ring or a gauge.
#[test]
fn the_radial_sheet_is_at_most_2_kb_gzip_and_a_ring_loads_no_script() {
    use navree_ui::charts::{Gauge, Ring};
    assert!(size("chart-radial") <= 2048, "{}", size("chart-radial"));
    let page = View::new("x").body((Ring::new("Sleep score", 82), Gauge::new("Stress", 40)));
    assert!(page.uses().has(Uses::CHART_RADIAL), "{:?}", page.uses());
    let html = page.to_html();
    assert!(html.contains(assets::CHART_RADIAL_URL), "{html}");
    assert!(html.contains(assets::CHART_PAINT_URL), "{html}");
    assert!(!html.contains("<script"), "{html}");
}

/// The linear charts' sheet (meter, mini columns, colour strip, range scale, trend) is
/// linked only by a page that draws one, and stays under 2 KB gzip.
#[test]
fn the_chart_linear_sheet_is_at_most_2_kb_gzip() {
    assert!(size("chart-linear") <= 2048, "{}", size("chart-linear"));
}

/// A page to read with a chart on it loads the charts module and no other.
#[test]
fn a_page_with_a_chart_loads_the_charts_module_alone() {
    use navree_ui::charts::{Chart, Series, Sparkline};
    let page = View::new("x").body(
        Chart::bar("Sent")
            .categories(["a"])
            .series(Series::new("Sent", [1])),
    );
    assert_eq!(page.uses(), Uses::CHARTS);
    let html = page.to_html();
    assert!(html.contains(assets::CHARTS_URL), "{html}");
    assert!(!html.contains(assets::CORE_URL));
    let picture = View::new("x").body(Stat::new("Sent", "1").chart(Sparkline::new("Sent", [1, 2])));
    assert!(picture.uses().is_none());
    assert!(!picture.to_html().contains("<script"));
}

/// The grid charts' own sheet (calendar and matrix heat maps, scatter) stays at 2 KB, and a page
/// with one loads the charts module and the grids' sheet, not the paint sheet or the core.
#[test]
fn the_chart_grids_sheet_is_at_most_2_kb_gzip_and_a_grid_chart_links_it_with_the_charts_module() {
    use navree_ui::charts::{CalendarHeatmap, MatrixHeatmap, PointSeries, Relation, Scatter};
    assert!(size("chart-grids") <= 2 * 1024, "{}", size("chart-grids"));
    let day = chrono::NaiveDate::from_ymd_opt(2026, 11, 12).unwrap();
    let pages = [
        View::new("x").body(CalendarHeatmap::new("Load").day(day, 5)),
        View::new("x").body(MatrixHeatmap::new("Links").metrics(["a", "b"]).relation(
            "a",
            "b",
            Relation::new(0.5),
        )),
        View::new("x").body(Scatter::new("Beats").series(PointSeries::new("a", [(1, 2)]))),
    ];
    for page in pages {
        assert_eq!(page.uses(), Uses::CHARTS.with(Uses::CHART_GRIDS));
        let html = page.to_html();
        assert!(html.contains(assets::CHARTS_URL), "{html}");
        assert!(html.contains(assets::CHART_GRIDS_URL), "{html}");
        assert!(!html.contains(assets::CORE_URL));
        assert!(!html.contains(assets::CHART_PAINT_URL));
    }
}

#[test]
fn the_live_module_is_at_most_3_kb_gzip() {
    assert!(size("live") <= 3 * 1024, "{}", size("live"));
}

/// The live module is for a region or a table that follows an event stream; a page
/// with neither never loads it, and a live region needs no table module.
#[test]
fn a_page_loads_the_live_module_only_for_a_live_region_or_a_live_table() {
    use navree_ui::{DataTable, Node, Region, TableQuery, TableRow};
    use std::time::Duration;

    #[derive(Clone, TableRow)]
    struct Row {
        #[col(skip)]
        id: i64,
        #[col(sortable)]
        title: String,
    }

    let rows = vec![Row {
        id: 1,
        title: "A".to_owned(),
    }];
    let query = TableQuery::<Row>::parse("/deals", "");
    let table = || DataTable::new("deals", &query, &rows);
    let page = |body: Node| View::new("x").body(body);

    // A live region, and no table.
    let region = page(
        Region::new("feed")
            .src("/fragments/feed")
            .live_from("/events")
            .into(),
    );
    assert_eq!(region.uses(), Uses::CORE.with(Uses::LIVE));
    let html = region.to_html();
    assert!(html.contains(assets::CORE_URL), "{html}");
    assert!(html.contains(assets::LIVE_URL), "{html}");
    assert!(!html.contains(assets::TABLE_URL), "{html}");

    // A live table loads the table module and the live one.
    let live = page(table().live_from("/events").into());
    for module in [Uses::CORE, Uses::TABLE, Uses::LIVE] {
        assert!(live.uses().has(module));
    }
    let html = live.to_html();
    for url in [assets::CORE_URL, assets::TABLE_URL, assets::LIVE_URL] {
        assert!(html.contains(url), "{url} in {html}");
    }

    // A table that only polls, a table that is not live, and a region that is not on a stream.
    for plain in [
        page(table().into()),
        page(table().live(Duration::from_secs(5)).into()),
        page(Region::new("feed").src("/fragments/feed").into()),
    ] {
        assert!(!plain.uses().has(Uses::LIVE));
        let html = plain.to_html();
        assert!(html.contains(assets::CORE_URL), "{html}");
        assert!(!html.contains(assets::LIVE_URL), "{html}");
    }

    // A page with neither loads no script at all.
    let none = page(navree_ui::el("p").child("Nothing live here").into());
    assert!(!none.uses().has(Uses::LIVE));
    assert!(!none.to_html().contains(assets::LIVE_URL));
}

#[test]
fn the_menus_module_is_at_most_5_kb_gzip() {
    assert!(size("menus") <= 5 * 1024, "{}", size("menus"));
}

#[test]
fn the_split_module_is_at_most_3_kb_gzip() {
    assert!(size("split") <= 3 * 1024, "{}", size("split"));
}

#[test]
fn the_lazy_module_is_at_most_4_kb_gzip() {
    assert!(size("lazy") <= 4 * 1024, "{}", size("lazy"));
}

/// A lazy list loads its own module; a live one loads the live module beside it, and
/// neither needs the core runtime.
#[test]
fn a_lazy_list_loads_the_lazy_module_and_live_only_when_live() {
    use navree_ui::{LazyEnd, LazyItem, LazyList};

    let plain = View::new("x")
        .body(LazyList::new("l", "Things", "/rows").items([LazyItem::new("1", "1", "One")]));
    assert_eq!(plain.uses(), Uses::LAZY);
    let html = plain.to_html();
    assert!(html.contains(assets::LAZY_URL), "{html}");
    assert!(!html.contains(assets::LIVE_URL) && !html.contains(assets::CORE_URL));
    let live = View::new("x").body(LazyList::new("l", "Things", "/rows").live_from(
        "/events",
        None,
        LazyEnd::Start,
    ));
    assert_eq!(live.uses(), Uses::LAZY.with(Uses::LIVE));
    assert!(live.to_html().contains(assets::LIVE_URL));
}

#[test]
fn the_keys_module_is_at_most_1_kb_gzip() {
    assert!(size("keys") <= 1024, "{}", size("keys"));
}

/// A split loads its own module and nothing else; a hotkey loads the keys module and
/// nothing else; a page with neither loads neither.
#[test]
fn a_split_and_a_hotkey_load_their_own_modules_only() {
    use navree_ui::{Pane, SplitPane, el};

    let split = View::new("x").body(
        SplitPane::new("s")
            .pane(Pane::new("a", "A"))
            .pane(Pane::fill("b", "B")),
    );
    assert_eq!(split.uses(), Uses::SPLIT);
    let html = split.to_html();
    assert!(html.contains(assets::SPLIT_URL), "{html}");
    assert!(!html.contains(assets::CORE_URL));
    assert!(!html.contains(assets::KEYS_URL));

    let keys = View::new("x").body(el("a").href("/next").hotkey("j").child("Next"));
    assert_eq!(keys.uses(), Uses::KEYS);
    let html = keys.to_html();
    assert!(html.contains(assets::KEYS_URL), "{html}");
    assert!(!html.contains(assets::SPLIT_URL));

    let plain = View::new("x").body(Button::primary("Go").href("/x"));
    let html = plain.to_html();
    assert!(!html.contains(assets::SPLIT_URL) && !html.contains(assets::KEYS_URL));
}

#[test]
fn the_sortable_module_is_at_most_6_kb_gzip() {
    assert!(size("sortable") <= 6 * 1024, "{}", size("sortable"));
}

/// A sortable that moves loads the core runtime, which posts its moves, and its own
/// module; one drawn with nowhere to post loads nothing.
#[test]
fn a_page_with_a_sortable_loads_the_sortable_module_and_only_then() {
    use navree_ui::{SortItem, SortList, Sortable};

    let list = || SortList::new("a", "Steps").item(SortItem::new("1", "Measure"));
    let moving = View::new("x").body(Sortable::new("s", "Steps").post("/moves").list(list()));
    assert_eq!(moving.uses(), Uses::CORE.with(Uses::SORTABLE));
    let html = moving.to_html();
    assert!(html.contains(assets::SORTABLE_URL), "{html}");
    assert!(!html.contains(assets::TABLE_URL));
    let still = View::new("x").body(Sortable::new("s", "Steps").list(list()));
    assert!(still.uses().is_none());
    assert!(!still.to_html().contains(assets::SORTABLE_URL));
}

#[test]
fn the_phone_module_is_at_most_5_kb_gzip() {
    assert!(size("phone") <= 5 * 1024, "{}", size("phone"));
}

/// The numbering plans the phone module formats with: fetched once, on the
/// first focus of a phone field, never with the page.
#[test]
fn the_phone_plans_are_at_most_20_kb_brotli() {
    let plans = get("phone-data").unwrap();
    let brotli = plans.brotli.map_or(plans.bytes.len(), <[u8]>::len);
    assert!(brotli <= 20 * 1024, "{brotli}");
}

#[test]
fn the_slider_module_is_at_most_3_kb_gzip() {
    assert!(size("slider") <= 3 * 1024, "{}", size("slider"));
}

#[test]
fn the_tree_module_is_at_most_5_kb_gzip() {
    assert!(size("tree") <= 5 * 1024, "{}", size("tree"));
}

#[test]
fn the_toolbar_module_is_at_most_3_kb_gzip() {
    assert!(size("toolbar") <= 3 * 1024, "{}", size("toolbar"));
}

/// Each flow part loads its own module and what it stands on, and the stepper none.
#[test]
fn a_page_with_a_slider_a_tree_or_a_toolbar_loads_its_module_and_nothing_else() {
    use navree_ui::{
        DropdownMenu, MenuItem, Slider, Step, Stepper, Toolbar, ToolbarItem, Tree, TreeNode,
    };

    let slider = View::new("x").body(Slider::range("r").label("Range"));
    assert_eq!(slider.uses(), Uses::SLIDER);
    let html = slider.to_html();
    assert!(html.contains(assets::SLIDER_URL), "{html}");
    assert!(!html.contains(assets::CORE_URL) && !html.contains(assets::TREE_URL));

    let tree = View::new("x").body(Tree::new("t", "T").node(TreeNode::leaf("a", "A")));
    assert_eq!(tree.uses(), Uses::TREE);
    let html = tree.to_html();
    assert!(html.contains(assets::TREE_URL), "{html}");
    assert!(!html.contains(assets::CORE_URL) && !html.contains(assets::SLIDER_URL));

    // A toolbar is its module and the menu it moves tools into.
    let bar = View::new("x").body(Toolbar::new("b", "B").item(ToolbarItem::button("A")).item(
        ToolbarItem::menu(
            "M",
            DropdownMenu::new("m", "M").item(MenuItem::link("L", "/l")),
        ),
    ));
    assert_eq!(
        bar.uses(),
        Uses::TOOLBAR.with(Uses::MENUS).with(Uses::OVERLAY)
    );
    assert!(!bar.to_html().contains(assets::CORE_URL));

    // The stepper is markup.
    let stepper = View::new("x").body(Stepper::new("S").step(Step::new("A")).current(0));
    assert_eq!(stepper.uses(), Uses::NONE);
    assert!(!stepper.to_html().contains("<script"));
}

#[test]
fn the_upload_module_is_at_most_3_kb_gzip() {
    assert!(size("upload") <= 3 * 1024, "{}", size("upload"));
}

#[test]
fn the_otp_module_is_at_most_1_kb_gzip() {
    assert!(size("otp") <= 1024, "{}", size("otp"));
}

#[test]
fn the_carousel_module_is_at_most_2_kb_gzip() {
    assert!(size("carousel") <= 2 * 1024, "{}", size("carousel"));
}

#[test]
fn the_menubar_module_is_at_most_3_kb_gzip() {
    assert!(size("menubar") <= 3 * 1024, "{}", size("menubar"));
}

/// What mounts islands. Each island's own script has the ceiling its app gives it, checked
/// when the app compiles (`IslandScript::new`).
#[test]
fn the_islands_module_is_at_most_1_kb_gzip() {
    assert!(size("islands") <= 1024, "{}", size("islands"));
}

/// An island loads the islands module and nothing else of the toolkit's: its own script
/// comes once the page has loaded, from the address on the island.
#[test]
fn a_page_with_an_island_loads_the_islands_module_alone() {
    use navree_ui::{Island, IslandScript};

    static DEPOTS: IslandScript = IslandScript::new("depots", b"export function mount() {}", 1024);
    let page = View::new("x").body(Island::new("depots", &DEPOTS).props(&["Ludhiana", "Patiala"]));
    assert_eq!(page.uses(), Uses::ISLAND);
    let html = page.to_html();
    assert_eq!(linked(&html).0, ["islands"], "{html}");
    assert!(
        !html.contains(&format!("<script type=\"module\" src=\"{}\"", DEPOTS.url())),
        "the island's own script is the islands module's to import: {html}"
    );
}

#[test]
fn the_parts_module_is_at_most_2_kb_gzip() {
    assert!(size("parts") <= 2 * 1024, "{}", size("parts"));
}

#[test]
fn the_conversation_module_is_at_most_2_kb_gzip() {
    assert!(size("genui") <= 2 * 1024, "{}", size("genui"));
}

/// Each of these components links a stylesheet of its own instead of growing the one every
/// page loads, so the one stays where it was. The theme builder's sheet is one of them.
#[test]
fn the_modules_styles_are_at_most_3_kb_gzip_each() {
    for name in [
        "markdown",
        "upload-styles",
        "otp-styles",
        "carousel-styles",
        "menubar-styles",
        "charts-styles",
        "chart-paint",
        "chart-radial",
        "chart-linear",
        "chart-grids",
        "app-blocks",
        "select-styles",
        "phone-styles",
        "slider-styles",
        "menus-styles",
        "split-styles",
        "tree-styles",
        "lazy-styles",
        "toolbar-styles",
        "dates-styles",
        "time-styles",
        "theme-builder",
        "parts-styles",
    ] {
        assert!(size(name) <= 3 * 1024, "{name}: {}", size(name));
    }
}

/// The data table's own styles: its toolbar, filters, columns, sticky cells and pages.
#[test]
fn the_table_styles_are_at_most_4_kb_gzip() {
    assert!(size("table-styles") <= 4 * 1024, "{}", size("table-styles"));
}

/// The asset names of the scripts and of the stylesheets `html` links, in the toolkit's order.
fn linked(html: &str) -> (Vec<&'static str>, Vec<&'static str>) {
    let mut scripts = Vec::new();
    let mut sheets = Vec::new();
    for asset in assets::FILES {
        let url = format!(
            "{}/{}-{}.{}",
            assets::MOUNT,
            asset.name,
            asset.hash,
            asset.ext
        );
        if html.contains(&url) {
            match asset.ext {
                "js" => scripts.push(asset.name),
                "css" => sheets.push(asset.name),
                _ => {}
            }
        }
    }
    (scripts, sheets)
}

/// What the files `names` weigh together, gzip.
fn weight(names: &[&str]) -> usize {
    names.iter().map(|name| size(name)).sum()
}

/// The styles that only one module's parts use are in that module's sheet, not in the one
/// every page loads: a page links a module's styles where it loads the module, and no others.
#[test]
fn a_modules_own_styles_come_with_it_and_only_with_it() {
    use navree_ui::dates::{Calendar, DatePicker, TimePicker};
    use navree_ui::{
        DropdownMenu, LazyItem, LazyList, MenuItem, Pane, PhoneInput, Slider, SplitPane, Toolbar,
        ToolbarItem, Tree, TreeNode,
    };

    let text = |name: &str| {
        std::str::from_utf8(get(name).unwrap_or_else(|| panic!("no asset {name}")).bytes)
            .expect("a stylesheet is text")
    };
    let shared = text("navree-ui");
    for (sheet, slot) in [
        ("table-styles", "[data-slot=filter-bar]"),
        ("select-styles", "[data-slot=select-popup]"),
        ("phone-styles", "[data-slot=phone-input]"),
        ("slider-styles", "[data-slot=slider-track]"),
        ("menus-styles", "[data-slot=dropdown-menu]"),
        ("split-styles", "[data-slot=split-handle]"),
        ("tree-styles", "[data-slot=tree-row]"),
        ("lazy-styles", "[data-slot=lazy-items]"),
        ("toolbar-styles", "[data-slot=toolbar-tool]"),
        ("dates-styles", "[data-slot=calendar-day]"),
        ("time-styles", "[data-slot=time-list]"),
    ] {
        assert!(text(sheet).contains(slot), "{slot} in {sheet}");
        assert!(
            !shared.contains(slot),
            "{slot} in the sheet every page loads"
        );
    }

    let sheets = |body: Node| linked(&View::new("x").body(body).to_html()).1;
    assert_eq!(
        sheets(Button::primary("Go").href("/x").into()),
        ["navree-ui"]
    );
    let parts: [(Node, &str); 10] = [
        (Select::new("s").option("a", "A").into(), "select-styles"),
        (Combobox::new("c").option("a", "A").into(), "select-styles"),
        (PhoneInput::new("mobile").into(), "phone-styles"),
        (Slider::new("v").label("Value").into(), "slider-styles"),
        (
            Tree::new("t", "T").node(TreeNode::leaf("a", "A")).into(),
            "tree-styles",
        ),
        (
            LazyList::new("l", "Things", "/rows")
                .items([LazyItem::new("1", "1", "One")])
                .into(),
            "lazy-styles",
        ),
        (
            SplitPane::new("s")
                .pane(Pane::new("a", "A"))
                .pane(Pane::fill("b", "B"))
                .into(),
            "split-styles",
        ),
        (Calendar::new("cal").into(), "dates-styles"),
        (DatePicker::new("due").into(), "dates-styles"),
        (TimePicker::new("at").into(), "time-styles"),
    ];
    for (body, own) in parts {
        let linked = sheets(body);
        assert_eq!(linked.first(), Some(&"navree-ui"), "{own}: {linked:?}");
        assert!(linked.contains(&own), "{own}: {linked:?}");
        assert!(!linked.contains(&"table-styles"), "{own}: {linked:?}");
    }
    // A menu's styles come with every menu, the one a toolbar moves its tools into too.
    let menu = sheets(
        DropdownMenu::new("m", "M")
            .item(MenuItem::link("L", "/l"))
            .into(),
    );
    assert_eq!(menu, ["navree-ui", "menus-styles"]);
    let bar = sheets(
        Toolbar::new("b", "B")
            .item(ToolbarItem::button("A"))
            .item(ToolbarItem::menu(
                "M",
                DropdownMenu::new("m", "M").item(MenuItem::link("L", "/l")),
            ))
            .into(),
    );
    assert_eq!(bar, ["navree-ui", "menus-styles", "toolbar-styles"]);
}

/// The marketing sheet (the blocks of a public site) stays at 6 KB gzip, and a whole public
/// page of them links the toolkit's sheet and its own and loads no script: the menu, the
/// questions and the pricing switch need none. Only a part that needs one, a form, brings it.
#[test]
fn the_marketing_sheet_is_at_most_6_kb_gzip_and_a_site_page_loads_no_script() {
    use navree_ui::marketing::{
        Brand, CtaBand, Faq, Feature, FeatureGrid, Hero, LegalPage, LegalSection, Plan, Pricing,
        Section, SectionHeading, SiteFooter, SiteHeader, SitePage, ThemeToggle,
    };
    use navree_ui::theme::Mode;

    assert!(size("marketing") <= 6 * 1024, "{}", size("marketing"));
    let page = SitePage::new()
        .header(
            SiteHeader::new(Brand::new("Kesar", "/"))
                .link("Pricing", "/pricing/")
                .action(Button::primary("Start free").href("/signup")),
        )
        .section(Hero::new("Know your body").action(Button::primary("Start").href("/s")))
        .section(
            Section::new("features")
                .heading(SectionHeading::new("Features"))
                .child(FeatureGrid::new().feature(Feature::new("Sleep", "Stages"))),
        )
        .section(
            Section::new("pricing").child(
                Pricing::new("plans").plan(
                    Plan::new("Plus")
                        .monthly("₹299", "a month")
                        .yearly("₹2,990", "a year"),
                ),
            ),
        )
        .section(Section::new("faq").child(Faq::new("faq").item("Free?", "Yes.")))
        .section(LegalPage::new("Terms").section(LegalSection::new("a", "One").text("x")))
        .section(CtaBand::new("cta", "Start today"))
        .footer(
            SiteFooter::new(Brand::new("Kesar", "/")).theme_toggle(ThemeToggle::new(Mode::System)),
        );
    let view = View::new("Kesar").body(page);
    assert_eq!(view.uses(), Uses::MARKETING);
    let html = view.to_html();
    assert_eq!(linked(&html), (vec![], vec!["navree-ui", "marketing"]));
    assert!(!html.contains("<script"), "{html}");
}

/// The bits are one each, so a page asks for exactly what it names.
#[test]
fn no_two_modules_share_a_bit() {
    let all = [
        Uses::CORE,
        Uses::SIDEBAR,
        Uses::OVERLAY,
        Uses::TABLE,
        Uses::SELECT,
        Uses::MENUS,
        Uses::LIVE,
        Uses::DATES,
        Uses::DATE_TEXT,
        Uses::TIME,
        Uses::PHONE,
        Uses::THEME_BUILDER,
        Uses::CHARTS,
        Uses::SLIDER,
        Uses::TREE,
        Uses::TOOLBAR,
        Uses::SORTABLE,
        Uses::SPLIT,
        Uses::KEYS,
        Uses::LAZY,
        Uses::MARKDOWN,
        Uses::UPLOAD,
        Uses::OTP,
        Uses::CAROUSEL,
        Uses::MENUBAR,
        Uses::ISLAND,
        Uses::PARTS,
        Uses::PARTS_STYLES,
        Uses::GENUI,
        Uses::CHART_RADIAL,
        Uses::COUNT_UP,
        Uses::CHART_PAINT,
        Uses::CHART_LINEAR,
        Uses::CHART_GRIDS,
        Uses::APP_BLOCKS,
        Uses::MARKETING,
    ];
    for (at, one) in all.iter().enumerate() {
        for other in &all[at + 1..] {
            assert!(
                !one.has(*other) && !other.has(*one),
                "{one:?} and {other:?}"
            );
        }
    }
}

/// The stylesheet every page loads: the tokens, the base, controls, display, states,
/// overlays, toasts and the shell. A module's own parts are styled in its own sheet.
#[test]
fn the_stylesheet_is_at_most_16_kb_gzip() {
    assert!(size("navree-ui") <= 16 * 1024, "{}", size("navree-ui"));
}

#[test]
fn a_typical_crud_page_loads_at_most_30_kb_gzip_of_script() {
    let total: usize = ["core", "sidebar", "overlay", "table", "menus"]
        .iter()
        .map(|name| size(name))
        .sum();
    assert!(total <= 30 * 1024, "{total}");
}

/// The typical CRUD page above, built: the shell and a table whose rows have menus. It
/// loads the files the budget counts, and those styles.
#[test]
fn a_typical_crud_page_loads_its_scripts_and_at_most_22_kb_gzip_of_styles() {
    use navree_ui::{DataTable, RowAction, TableQuery, TableRow};

    #[derive(Clone, TableRow)]
    struct Deal {
        #[col(skip)]
        id: i64,
        #[col(sortable)]
        title: String,
    }

    let rows = vec![Deal {
        id: 1,
        title: "Gurdaspur depot".to_owned(),
    }];
    let query = TableQuery::<Deal>::parse("/deals", "");
    let shell = Shell::new("Agra")
        .group(NavGroup::new("Sales").item(NavItem::new("Deals", "/deals").icon(icons::HANDSHAKE)));
    let html = shell
        .page("Deals", "/deals")
        .body(
            DataTable::new("deals", &query, &rows)
                .row_actions(|row| vec![RowAction::link("View", format!("/deals/{}", row.id))]),
        )
        .to_view()
        .to_html();
    let (scripts, sheets) = linked(&html);
    assert_eq!(scripts, ["core", "sidebar", "overlay", "table", "menus"]);
    assert_eq!(sheets, ["navree-ui", "table-styles", "menus-styles"]);
    let total = weight(&sheets);
    assert!(total <= 22 * 1024, "{total}");
}

/// The heaviest screen of the example apps, as its parts load: the inbox's conversation.
/// The shell, the list and the thread in panes, both lazy lists following the stream, the
/// assign box searched on the server, the status menu, the snooze date and the single-key
/// shortcuts. What every module together weighs never reaches a page; this does.
fn conversation_page() -> String {
    use navree_ui::dates::DatePicker;
    use navree_ui::{DropdownMenu, LazyEnd, LazyList, MenuItem, Pane, SplitPane, el};

    let shell = Shell::new("Support").group(
        NavGroup::new("Views").item(NavItem::new("All conversations", "/inbox").icon(icons::INBOX)),
    );
    shell
        .page("Refund for order 2041", "/inbox/1")
        .body((
            SplitPane::new("inbox")
                .pane(Pane::new("list", "Conversations"))
                .pane(Pane::fill("thread", "Conversation")),
            LazyList::new("conversations", "Conversations", "/inbox/rows").live_from(
                "/events",
                None,
                LazyEnd::Start,
            ),
            LazyList::new("messages", "Messages", "/inbox/1/messages").live_from(
                "/events",
                None,
                LazyEnd::End,
            ),
            Combobox::new("assignee").search("/inbox/team/options"),
            DropdownMenu::new("status", "Status").item(MenuItem::link("Close", "/inbox/1/close")),
            DatePicker::new("until"),
            el("a").href("/inbox/2").hotkey("j").child("Next"),
        ))
        .to_view()
        .to_html()
}

/// The script of the heaviest page. Each module has its own ceiling as well.
#[test]
fn the_heaviest_page_loads_at_most_52_kb_gzip_of_script() {
    let (scripts, _) = linked(&conversation_page());
    assert_eq!(
        scripts,
        [
            "core",
            "sidebar",
            "overlay",
            "live",
            "select",
            "dates",
            "date-text",
            "menus",
            "split",
            "keys",
            "lazy"
        ]
    );
    let total = weight(&scripts);
    assert!(total <= 52 * 1024, "{total}");
}

/// The styles of the heaviest page: the sheet every page loads and its modules' own.
#[test]
fn the_heaviest_page_loads_at_most_26_kb_gzip_of_styles() {
    let (_, sheets) = linked(&conversation_page());
    assert_eq!(
        sheets,
        [
            "navree-ui",
            "select-styles",
            "menus-styles",
            "split-styles",
            "lazy-styles",
            "dates-styles"
        ]
    );
    let total = weight(&sheets);
    assert!(total <= 26 * 1024, "{total}");
}

#[test]
fn every_text_asset_has_brotli_and_gzip_forms_that_are_smaller() {
    for asset in assets::FILES
        .iter()
        .filter(|asset| matches!(asset.ext, "js" | "css"))
    {
        assert!(
            asset
                .gzip
                .is_some_and(|bytes| bytes.len() < asset.bytes.len()),
            "{}",
            asset.name
        );
        assert!(
            asset
                .brotli
                .is_some_and(|bytes| bytes.len() < asset.bytes.len()),
            "{}",
            asset.name
        );
    }
}

#[test]
fn a_page_with_nothing_interactive_ships_no_script() {
    let page = View::new("x")
        .body(Card::titled("T").body((Button::primary("Go").href("/x"), Dialog::new("d", "T"))));
    assert_eq!(page.uses(), Uses::NONE);
    assert!(!page.to_html().contains("<script"));
}

#[test]
fn the_buffer_is_sized_ahead_of_the_write() {
    let shell = Shell::new("Agra")
        .group(NavGroup::new("Sales").item(NavItem::new("Deals", "/deals").icon(icons::HANDSHAKE)));
    let page = shell
        .page("Deals", "/deals")
        .body(Grid::cols(2).children((Stat::new("Open", "12"), Stat::new("Won", "3"))))
        .to_view();
    let html = page.to_html();
    let estimate = measure(page.body_node()).bytes;
    assert!(
        estimate * 2 > html.len() - 512,
        "the estimate should be near: {estimate} for {}",
        html.len()
    );
}

/// What the submit-capable controls of `html` hang on: how many are tied to a form by
/// a `form` attribute, and the most any one form holds. Untyped buttons, `submit` and
/// `image` inputs can submit, disabled or not; nothing else can.
fn submit_controls(html: &str) -> (usize, usize) {
    use std::collections::HashMap;
    let mut per_form: HashMap<String, usize> = HashMap::new();
    let mut tied = 0;
    let mut open: Vec<String> = Vec::new();
    let mut anonymous = 0;
    let mut rest = html;
    while let Some(at) = rest.find('<') {
        rest = &rest[at + 1..];
        let end = rest.find('>').unwrap_or(rest.len());
        let tag = &rest[..end];
        let name = tag
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '/')
            .collect::<String>()
            .to_ascii_lowercase();
        let attr = |wanted: &str| -> Option<String> {
            let needle = format!(" {wanted}=\"");
            tag.find(&needle).map(|at| {
                let value = &tag[at + needle.len()..];
                value[..value.find('"').unwrap_or(value.len())].to_owned()
            })
        };
        match name.as_str() {
            "form" => open.push(attr("id").unwrap_or_else(|| {
                anonymous += 1;
                format!("#{anonymous}")
            })),
            "/form" => {
                open.pop();
            }
            "button" | "input" => {
                let kind = attr("type")
                    .unwrap_or_else(|| if name == "button" { "submit" } else { "text" }.to_owned())
                    .to_ascii_lowercase();
                if matches!(kind.as_str(), "submit" | "image") {
                    let by_attribute = attr("form");
                    if by_attribute.is_some() {
                        tied += 1;
                    }
                    if let Some(owner) = by_attribute.or_else(|| open.last().cloned()) {
                        *per_form.entry(owner).or_default() += 1;
                    }
                }
            }
            _ => {}
        }
        rest = &rest[end..];
    }
    (tied, per_form.into_values().max().unwrap_or(0))
}

/// In Chromium, each submit-capable control that gains or loses a form owner makes the
/// form rebuild its list of controls by walking the document, so moving a thousand rows
/// that each hold one costs time in the square of the rows: about 700 ms of every sort
/// on a phone. One small form inside each row is fine; a control tied by `form=`, or a
/// form around the rows, is not. No table may gain either kind with its rows.
#[test]
fn a_table_ties_no_submit_control_to_a_form_per_row() {
    use navree_ui::table::EditCell;
    use navree_ui::{BulkAction, DataTable, RowAction, TableQuery, TableRow, render};

    #[derive(Clone, TableRow)]
    #[table(per = 1000, per_pages(10, 1000))]
    struct Row {
        #[col(skip)]
        id: i64,
        #[col(sortable, search, filter = text)]
        title: String,
        #[col(sortable, numeric, filter = range)]
        amount: i64,
    }

    let measure = |count: i64| {
        let rows: Vec<Row> = (0..count)
            .map(|id| Row {
                id,
                title: format!("Deal {id}"),
                amount: id,
            })
            .collect();
        let query = TableQuery::<Row>::parse("/deals", "per_page=1000");
        submit_controls(&render(
            &DataTable::new("deals", &query, &rows)
                .bulk(
                    "/deals/bulk",
                    [
                        BulkAction::new("archive", "Archive"),
                        BulkAction::new("delete", "Delete").destructive(),
                    ],
                )
                .row_actions(|row| {
                    vec![
                        RowAction::link("View", format!("/deals/{}", row.id)),
                        RowAction::post("Archive", format!("/deals/{}/archive", row.id)),
                        RowAction::post("Delete", format!("/deals/{}/delete", row.id)),
                    ]
                })
                .detail(|row| navree_ui::el("p").child(row.title.clone()).into())
                .cell("title", |row| {
                    EditCell::text(row.title.clone(), format!("/deals/{}/title", row.id))
                })
                .into(),
        ))
    };
    let few = measure(3);
    let many = measure(1000);
    assert_eq!(
        many.0, few.0,
        "submit controls tied by `form=` grew with the rows"
    );
    assert_eq!(many.1, few.1, "a form's submit controls grew with the rows");
    assert!(many.1 <= 8, "{many:?}");
}

#[test]
fn submit_controls_are_found_by_form_attribute_and_by_nesting() {
    let html = concat!(
        r#"<form id="a"><button>x</button><button type="button">y</button></form>"#,
        r#"<table><tr><td><button form="a">z</button><input type="IMAGE" form="b" disabled></td></tr></table>"#,
        r#"<form><input type="submit"><input type="checkbox" form="a"></form>"#,
    );
    assert_eq!(submit_controls(html), (2, 2));
}

#[cfg(not(debug_assertions))]
mod times {
    use std::hint::black_box;
    use std::time::{Duration, Instant};

    use navree_ui::form::Money;
    use navree_ui::{DataTable, FilterBar, Options, TableQuery, TableRow, View};
    use serde::Deserialize;

    #[derive(Clone, TableRow)]
    #[table(per = 50, per_pages(50, 1000))]
    struct Row {
        #[col(skip)]
        id: i64,
        #[col(sortable, link = "/deals/{id}")]
        title: String,
        #[col(sortable, numeric)]
        amount: Money,
        owner: &'static str,
    }

    #[derive(Clone, Copy, Deserialize, Options)]
    enum Stage {
        Lead,
    }

    #[derive(Default, Deserialize, FilterBar)]
    struct Filter {
        #[filter(search)]
        q: Option<String>,
        stage: Option<Stage>,
    }

    /// The fastest of `rounds` timed runs, after one run that is not timed. The bound is
    /// on what the code costs, not on a busy machine's worst moments: a real slowdown
    /// slows every run, while noise slows only some.
    fn best(rounds: u32, work: impl Fn() -> String) -> Duration {
        black_box(work());
        (0..rounds)
            .map(|_| {
                let start = Instant::now();
                black_box(work());
                start.elapsed()
            })
            .min()
            .expect("at least one round")
    }

    /// A line of a million points: cleaned, summarised over every value, reduced to
    /// 640 with largest-triangle-three-buckets, and written.
    #[test]
    fn a_million_point_line_renders_in_under_100_ms() {
        use navree_ui::charts::{Chart, Series};
        let n = 1_000_000;
        let xs: Vec<f64> = (0..n).map(|i| i as f64).collect();
        let ys: Vec<f64> = (0..n)
            .map(|i| ((i as f64) / 977.0).sin() * 50.0 + (i % 7) as f64)
            .collect();
        let time = best(5, || {
            navree_ui::render(
                &Chart::line("A million points")
                    .numbers(xs.iter().copied())
                    .series(Series::new("Walk", ys.iter().copied()))
                    .into(),
            )
        });
        println!("a million-point line renders in {time:?}");
        assert!(time < Duration::from_millis(100), "{time:?}");
    }

    /// A random walk of a million points: every bucket has its own peak, the shape that
    /// costs downsampling most.
    #[test]
    fn a_million_point_random_walk_renders_in_under_100_ms() {
        use navree_ui::charts::{Chart, Series};
        let n = 1_000_000;
        let mut seed: u64 = 7;
        let mut level = 0.0;
        let ys: Vec<f64> = (0..n)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                level += (seed >> 11) as f64 / (1u64 << 53) as f64 - 0.5;
                level
            })
            .collect();
        let xs: Vec<f64> = (0..n).map(|i| i as f64).collect();
        let time = best(5, || {
            navree_ui::render(
                &Chart::line("A random walk")
                    .numbers(xs.iter().copied())
                    .y_zero(false)
                    .series(Series::new("Walk", ys.iter().copied()))
                    .into(),
            )
        });
        println!("a million-point random walk renders in {time:?}");
        assert!(time < Duration::from_millis(100), "{time:?}");
    }

    /// A page with a phone input and a saved number: the 245 countries of
    /// its select, the number read and written in its country's format.
    #[test]
    fn a_page_with_a_phone_input_renders_in_under_1_ms() {
        use navree_ui::PhoneInput;
        let saved = navree_phone::PhoneNumber::parse("+44 7400 123456", None).unwrap();
        let time = best(50, || {
            View::new("Contact")
                .body(PhoneInput::new("mobile").number(&saved))
                .to_html()
        });
        println!("a page with a phone input renders in {time:?}");
        assert!(time < Duration::from_millis(1), "{time:?}");
    }

    #[test]
    fn ten_thousand_options_render_in_under_40_ms() {
        use navree_ui::{Choice, Combobox};
        let time = best(20, || {
            View::new("Options")
                .body(Combobox::new("c").options((0..10_000).map(|at| {
                    Choice::new(
                        format!("v{at}"),
                        format!("Choice number {at} of the long list"),
                    )
                })))
                .to_html()
        });
        println!("ten thousand options render in {time:?}");
        assert!(time < Duration::from_millis(40), "{time:?}");
    }

    #[test]
    fn a_thousand_rows_render_in_under_3_ms() {
        let rows: Vec<Row> = (0..1000)
            .map(|id| Row {
                id,
                title: format!("Deal {id}"),
                amount: Money(id * 100),
                owner: "Anu",
            })
            .collect();
        let query = TableQuery::<Row, Filter>::parse("/deals", "per_page=1000");
        let time = best(50, || {
            View::new("Deals")
                .body(DataTable::new("deals", &query, &rows))
                .to_html()
        });
        assert!(time < Duration::from_millis(3), "{time:?}");
    }

    #[test]
    fn a_hundred_event_timeline_renders_in_under_1_ms() {
        use navree_ui::{Meta, Timeline, TimelineEvent, Tone, icons};
        let time = best(50, || {
            let timeline = Timeline::new("t", "Steps")
                .time_zone("Asia/Kolkata", 19_800)
                .events((0..100).map(|n| {
                    TimelineEvent::new(format!("Step {n}"))
                        .at(1_793_592_667 + n * 97)
                        .tone(Tone::Info)
                        .meta(Meta::icon(icons::MONITOR, "Safari on macOS"))
                }));
            View::new("Steps").body(timeline).to_html()
        });
        assert!(time < Duration::from_millis(1), "{time:?}");
    }

    #[test]
    fn a_tree_of_ten_thousand_nodes_renders_in_under_30_ms() {
        use navree_ui::{Tree, TreeNode};
        // The best of thirty: a machine with other builds on it can make a few runs three times as slow.
        let time = best(30, || {
            let tree = Tree::new("t", "People")
                .nodes((0..10_000).map(|n| TreeNode::leaf(format!("n{n}"), format!("Person {n}"))));
            View::new("Tree").body(tree).to_html()
        });
        println!("a tree of ten thousand nodes renders in {time:?}");
        assert!(time < Duration::from_millis(30), "{time:?}");
    }

    #[test]
    fn a_choosing_tree_of_a_thousand_nodes_renders_in_under_5_ms() {
        use navree_ui::{Tree, TreeNode, TreeSelect};
        let time = best(30, || {
            let tree = Tree::new("t", "People")
                .select(TreeSelect::Multiple)
                .cascade()
                .nodes((0..10).map(|group| {
                    TreeNode::branch(
                        format!("g{group}"),
                        format!("Group {group}"),
                        (0..99).map(move |n| {
                            TreeNode::leaf(format!("g{group}-{n}"), format!("Person {n}"))
                        }),
                    )
                    .open(true)
                }));
            View::new("Tree").body(tree).to_html()
        });
        println!("a choosing tree of a thousand nodes renders in {time:?}");
        assert!(time < Duration::from_millis(5), "{time:?}");
    }

    #[test]
    fn a_wizard_step_renders_in_under_1_ms() {
        use navree_ui::{Form, Validate, Wizard, WizardStep};
        #[derive(serde::Deserialize, Validate, Form)]
        struct One {
            /// Name
            #[validate(length(min = 1))]
            name: String,
        }
        #[derive(serde::Deserialize, Validate, Form)]
        struct Two {
            /// Email
            #[validate(email)]
            email: String,
        }
        let wizard = Wizard::new("w")
            .base("/w")
            .step(WizardStep::of::<One>("one", "One"))
            .step(WizardStep::of::<Two>("two", "Two"))
            .review();
        let draft = wizard.draft("a-draft-id-1");
        let time = best(50, || {
            View::new("W")
                .body(wizard.page(&draft, "one").expect("the first step opens"))
                .to_html()
        });
        println!("a wizard step renders in {time:?}");
        assert!(time < Duration::from_millis(1), "{time:?}");
    }

    #[test]
    fn sliders_and_a_toolbar_of_many_tools_render_in_under_1_ms() {
        use navree_ui::{Slider, Toolbar, ToolbarItem};
        let time = best(50, || {
            let sliders = navree_ui::Stack::gap(3).children((
                Slider::new("a").max(100).step(5).ticks_every(1).value(40),
                Slider::range("b")
                    .max(1000)
                    .step(10)
                    .values(200, 800)
                    .mark(0, "0")
                    .mark(1000, "1000"),
                Slider::new("c").max(2).names(["Low", "Mid", "High"]),
            ));
            let bar = Toolbar::new("t", "Tools")
                .items((0..40).map(|n| ToolbarItem::button(format!("Tool {n}"))));
            View::new("S").body((sliders, bar)).to_html()
        });
        println!("sliders and a toolbar render in {time:?}");
        assert!(time < Duration::from_millis(1), "{time:?}");
    }

    #[test]
    fn a_hundred_row_feed_renders_in_under_1_ms() {
        use navree_ui::{ActivityFeed, ActivityItem, Meta, Tone, icons};
        let time = best(50, || {
            let feed = ActivityFeed::new("a", "Recent activity")
                .live()
                .items((0..100).map(|n| {
                    ActivityItem::new("Maya Patel", "opened")
                        .object("Autumn launch")
                        .tone(Tone::Info)
                        .icon(icons::MAIL)
                        .at(1_793_592_667 + n * 97)
                        .meta(Meta::icon(icons::MONITOR, "Safari on macOS"))
                        .href("/people/7")
                }));
            View::new("Feed").body(feed).to_html()
        });
        assert!(time < Duration::from_millis(1), "{time:?}");
    }

    /// An error page is drawn on the error path of a request: it has to cost next to nothing.
    #[test]
    fn an_error_page_renders_in_under_100_us() {
        use http::StatusCode;
        use navree_ui::pages::ErrorPage;
        let time = best(300, || {
            ErrorPage::new(StatusCode::NOT_FOUND)
                .reference("01a1a48c27697468b09935ddd4286017")
                .into_view()
                .to_html()
        });
        println!("an error page renders in {time:?}");
        assert!(time < Duration::from_micros(100), "{time:?}");
    }

    #[test]
    fn an_error_page_in_the_shell_renders_in_under_300_us() {
        use http::{HeaderMap, StatusCode};
        use navree_ui::pages::ErrorPage;
        use navree_ui::{NavGroup, NavItem, Shell, icons};
        let shell = Shell::new("Agra").group(
            NavGroup::new("Sales")
                .item(NavItem::new("Deals", "/deals").icon(icons::HANDSHAKE))
                .item(NavItem::new("Contacts", "/contacts").icon(icons::USERS)),
        );
        let time = best(300, || {
            let app = shell.for_request("/missing", &HeaderMap::new());
            ErrorPage::new(StatusCode::FORBIDDEN)
                .reference("01a1a48c27697468b09935ddd4286017")
                .in_shell(&app)
                .to_view()
                .to_html()
        });
        println!("an error page in the shell renders in {time:?}");
        assert!(time < Duration::from_micros(300), "{time:?}");
    }
}
