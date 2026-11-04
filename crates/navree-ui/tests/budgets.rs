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
fn the_stylesheet_is_at_most_25_kb_gzip() {
    assert!(size("navree-ui") <= 25 * 1024, "{}", size("navree-ui"));
}

#[test]
fn a_typical_crud_page_loads_at_most_30_kb_gzip_of_script() {
    let total: usize = ["core", "sidebar", "overlay", "table", "menus"]
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
        let query = TableQuery::<Row>::parse("/deals", "per=1000");
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

    /// A page with a phone input and a saved number: the 245 countries of
    /// its select, the number read and written in its country's format.
    #[test]
    fn a_page_with_a_phone_input_renders_in_under_1_ms() {
        use navree_ui::PhoneInput;
        let saved = navree_phone::PhoneNumber::parse("+44 7400 123456", None).unwrap();
        let time = median(50, || {
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

    #[test]
    fn a_hundred_event_timeline_renders_in_under_1_ms() {
        use navree_ui::{Meta, Timeline, TimelineEvent, Tone, icons};
        let time = median(50, || {
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
    fn a_hundred_row_feed_renders_in_under_1_ms() {
        use navree_ui::{ActivityFeed, ActivityItem, Meta, Tone, icons};
        let time = median(50, || {
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
}
