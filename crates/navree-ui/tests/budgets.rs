//! The speed budgets. A regression fails the build. Sizes hold everywhere;
//! render times only mean something in a release build, so they run there
//! (`cargo test --release -p navree-ui --test budgets`), with room for a busy
//! machine: they catch a change of scale, not a bad day.

use navree_ui::assets::{self, Asset, get};
use navree_ui::{
    Button, Card, Combobox, Dialog, Grid, NavGroup, NavItem, Select, Shell, Stat, Uses, View,
    icons, measure,
};

fn gzip(asset: &Asset) -> usize {
    asset.gzip.map_or(asset.bytes.len(), <[u8]>::len)
}

fn size(name: &str) -> usize {
    gzip(get(name).unwrap_or_else(|| panic!("no asset {name}")))
}

#[test]
fn the_core_runtime_is_at_most_12_kb_gzip() {
    assert!(size("core") <= 12 * 1024, "{}", size("core"));
}

#[test]
fn the_table_module_is_at_most_5_kb_gzip() {
    assert!(size("table") <= 5 * 1024, "{}", size("table"));
}

#[test]
fn the_select_module_is_at_most_9_kb_gzip() {
    assert!(size("select") <= 9 * 1024, "{}", size("select"));
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

#[test]
fn the_stylesheet_is_at_most_25_kb_gzip() {
    assert!(size("navree-ui") <= 25 * 1024, "{}", size("navree-ui"));
}

#[test]
fn a_typical_crud_page_loads_at_most_30_kb_gzip_of_script() {
    let total: usize = ["core", "sidebar", "overlay", "table"]
        .iter()
        .map(|name| size(name))
        .sum();
    assert!(total <= 30 * 1024, "{total}");
}

#[test]
fn all_modules_together_are_under_60_kb_gzip() {
    let total: usize = assets::FILES
        .iter()
        .filter(|asset| asset.ext == "js")
        .map(gzip)
        .sum();
    assert!(total < 60 * 1024, "{total}");
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
    let shell = Shell::new("Acme")
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

    fn median(rounds: u32, work: impl Fn() -> String) -> Duration {
        let mut times: Vec<Duration> = (0..rounds)
            .map(|_| {
                let start = Instant::now();
                black_box(work());
                start.elapsed()
            })
            .collect();
        times.sort();
        times[times.len() / 2]
    }

    #[test]
    fn ten_thousand_options_render_in_under_40_ms() {
        use navree_ui::{Choice, Combobox};
        let time = median(20, || {
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
                owner: "Ada",
            })
            .collect();
        let query = TableQuery::<Row, Filter>::parse("/deals", "per=1000");
        let time = median(50, || {
            View::new("Deals")
                .body(DataTable::new("deals", &query, &rows))
                .to_html()
        });
        assert!(time < Duration::from_millis(3), "{time:?}");
    }
}
