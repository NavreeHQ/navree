//! A data table's row the page shows, its header's look, and the page's own filters as
//! chips, a checkbox, the page's choices and defaults.

use navree_ui::form::Many;
use navree_ui::table::FilterBar as _;
use navree_ui::{
    BulkAction, DataTable, FilterBar, FilterOption, Node, Options, TablePage, TableQuery, TableRow,
    Uses, measure, render,
};
use serde::Deserialize;

#[derive(Clone, TableRow)]
struct Deal {
    #[col(skip)]
    id: i64,
    #[col(sortable, search)]
    name: String,
    #[col(numeric)]
    seats: i64,
}

fn deals() -> Vec<Deal> {
    (1..=3)
        .map(|id| Deal {
            id,
            name: format!("deal-0{id}"),
            seats: id * 8,
        })
        .collect()
}

fn styles(file: &str) -> String {
    std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("styles")
            .join(file),
    )
    .unwrap()
}

#[test]
fn the_row_the_page_shows_is_marked_current_and_nothing_is_selected() {
    let query = TableQuery::<Deal>::parse("/deals", "");
    let page = TablePage::new(deals(), 3);
    let table = || {
        DataTable::new("deals", &query, &page)
            .bulk("/deals/bulk", [BulkAction::new("archive", "Archive")])
    };
    let plain = render(&table().into());
    // Two rows say yes: the first is the one shown.
    let html = render(&table().current_row(|deal| deal.id >= 2).into());
    assert!(
        html.contains(r#"<tr data-key="2" aria-current="true">"#),
        "{html}"
    );
    let body = html.split("<tbody>").nth(1).unwrap();
    assert_eq!(body.matches("aria-current").count(), 1, "{body}");
    // A mark, not a selection: no box is ticked, nothing new posts, the rest is as it was.
    assert!(
        !html.contains("data-selected") && !body.contains(" checked"),
        "{body}"
    );
    assert_eq!(
        html.replace(
            r#"<tr data-key="2" aria-current="true">"#,
            r#"<tr data-key="2">"#
        ),
        plain
    );
    // A table no row of which says yes marks none.
    let none = render(&table().current_row(|deal| deal.id == 9).into());
    assert_eq!(none, plain);
}

#[test]
fn a_muted_header_is_a_mark_on_the_table_and_the_sheet_draws_it_in_fg_2() {
    let query = TableQuery::<Deal>::parse("/deals", "");
    let page = TablePage::new(deals(), 3);
    let plain = render(&DataTable::new("deals", &query, &page).into());
    let html = render(&DataTable::new("deals", &query, &page).muted_header().into());
    assert!(
        html.contains(r#"<table data-slot="table" data-muted-head>"#),
        "{html}"
    );
    assert_eq!(html.replace(" data-muted-head", ""), plain);
    let sheet = styles("25-table.css");
    // A sort link in a heading is muted too, until the pointer is on it.
    assert!(sheet.contains("&[data-muted-head] :is(th,th a:not(:hover)){color:var(--fg-2)}"));
    // The current row and a ticked one take the theme's colour for a chosen row.
    assert!(sheet.contains(
        "& tbody tr:is([data-selected],[aria-current]) td{background:var(--row-selected,var(--ink-3))}"
    ));
}

#[test]
fn a_data_tables_header_reads_every_value_of_the_themes_table_head_style() {
    // A data table's `<table>` is a table's slot, so the table's heading rule reaches it.
    let query = TableQuery::<Deal>::parse("/deals", "");
    let page = TablePage::new(deals(), 3);
    let html = render(&DataTable::new("deals", &query, &page).into());
    assert!(html.contains(r#"<table data-slot="table">"#), "{html}");
    let display = styles("20-display.css");
    for value in [
        "font-weight:var(--text-table-head-weight,500)",
        "line-height:var(--text-table-head-line)",
        "letter-spacing:var(--text-table-head-tracking)",
        "font-family:var(--text-table-head-face)",
    ] {
        assert!(display.contains(value), "{value}");
    }
    assert!(styles("25-table.css").contains("font-size:var(--text-table-head-size,14px)"));
}

#[derive(Clone, Copy, Debug, Deserialize, Options, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Stage {
    Lead,
    Won,
    Lost,
}

#[derive(Clone, Copy, Debug, Deserialize, Options, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Period {
    Week,
    Month,
}

const ORDERS: &[(&str, &str)] = &[("closing", "Closing first"), ("largest", "Largest first")];

#[derive(Default, Deserialize, FilterBar)]
struct Pipeline {
    /// Stage
    #[filter(multi)]
    stage: Option<Many<Stage>>,
    /// Region
    #[filter(options = page)]
    region: Option<String>,
    /// Period
    #[filter(default = "month")]
    period: Option<Period>,
    /// Hot deals only
    #[filter(boolean)]
    hot: Option<bool>,
    /// Order by
    #[filter(options = ORDERS, default = "closing")]
    order: Option<String>,
    /// Owner
    #[filter(multi, options = page)]
    owner: Option<Many<String>>,
}

fn pipeline(query: &str) -> TableQuery<Deal, Pipeline> {
    TableQuery::parse("/pipeline", query)
}

#[test]
fn a_page_filter_reads_chips_a_checkbox_the_pages_choices_and_defaults_from_the_url() {
    let query =
        pipeline("stage=lead&stage=bogus&stage=won&stage=&region=north&hot=true&owner=7&owner=8");
    let filter = query.filter();
    assert_eq!(
        filter.stage.as_deref().map(Vec::as_slice),
        Some(&[Stage::Lead, Stage::Won][..]),
        "a bad value drops that value, not the others"
    );
    assert_eq!(filter.region.as_deref(), Some("north"));
    assert_eq!(filter.hot, Some(true));
    assert_eq!(
        filter.owner.as_deref().map(Vec::as_slice),
        Some(&["7".to_owned(), "8".to_owned()][..])
    );
    // What the URL leaves empty takes its default, and a link doesn't name it.
    assert_eq!(filter.period, Some(Period::Month));
    assert_eq!(filter.order.as_deref(), Some("closing"));
    assert_eq!(
        query.href(),
        "/pipeline?stage=lead&stage=won&region=north&hot=true&owner=7&owner=8"
    );
    // A filter holding its default is no filter.
    assert!(!pipeline("").is_filtered());
    assert_eq!(pipeline("").href(), "/pipeline");
    assert_eq!(pipeline("period=month&order=closing").href(), "/pipeline");
    let other = pipeline("period=week&order=largest");
    assert_eq!(other.filter().period, Some(Period::Week));
    assert!(other.is_filtered());
    assert_eq!(other.href(), "/pipeline?period=week&order=largest");
    // A value the type can't read leaves the default in force.
    assert_eq!(pipeline("period=year").filter().period, Some(Period::Month));
    // A tick the URL turns off is no filter.
    assert!(pipeline("hot=false").filter().params().is_empty());
    // A value holding the separator a list is read with is left out, and so are values past
    // the limit.
    let joined = pipeline("owner=7%1F8&owner=9");
    assert_eq!(
        joined.filter().owner.as_deref().map(Vec::as_slice),
        Some(&["9".to_owned()][..])
    );
    let many: String = (0..200).map(|at| format!("owner={at}&")).collect();
    let capped = pipeline(&many);
    assert!(
        capped
            .filter()
            .owner
            .as_ref()
            .map_or(0, |owners| owners.len())
            <= navree_ui::table::MAX_VALUES
    );
}

#[test]
fn a_page_filter_draws_chips_a_checkbox_and_selects_with_and_without_an_empty_choice() {
    let query = pipeline("stage=lost&hot=true&region=south&owner=8");
    let page = TablePage::new(deals(), 3);
    let table = || {
        DataTable::new("pipeline", &query, &page)
            .options(
                "region",
                vec![
                    FilterOption::new("north", "North India"),
                    FilterOption::new("south", "South India"),
                ],
            )
            .options(
                "owner",
                vec![
                    FilterOption::new(7, "Gauri Hundal"),
                    FilterOption::new(8, "Esha Dhillon"),
                ],
            )
    };
    let html = render(&table().into());
    // The stage's chips: a group of checkboxes, the chosen one on.
    assert!(
        html.contains(r#"<div data-slot="toggle-group" role="group" aria-label="Stage" data-variant="outline" data-size="sm""#),
        "{html}"
    );
    assert!(
        html.contains(r#"<input type="checkbox" name="stage" value="lost" checked>"#),
        "{html}"
    );
    assert!(
        html.contains(r#"<input type="checkbox" name="stage" value="lead">"#),
        "{html}"
    );
    // The tick, with its label.
    assert!(
        html.contains(r#"<label data-slot="choice"><input data-slot="checkbox" type="checkbox" name="hot" value="true" checked><span>Hot deals only</span></label>"#),
        "{html}"
    );
    // The page's regions, with the empty choice that means any.
    assert!(
        html.contains(r#"<select data-slot="native-select" name="region" aria-label="Region"><option value="">Region</option><option value="north">North India</option><option value="south" selected>South India</option></select>"#),
        "{html}"
    );
    // A select with a default always holds a choice: no empty one.
    assert!(
        html.contains(r#"<select data-slot="native-select" name="period" aria-label="Period"><option value="week">Week</option><option value="month" selected>Month</option></select>"#),
        "{html}"
    );
    assert!(
        html.contains(r#"<option value="closing" selected>Closing first</option>"#),
        "{html}"
    );
    // Chips of the page's choices.
    assert!(
        html.contains(r#"<input type="checkbox" name="owner" value="8" checked>"#),
        "{html}"
    );
    assert!(html.contains("Gauri Hundal"), "{html}");
    // Each control is in the toolbar's form, which the table submits on a change.
    let form = html.split(r#"data-slot="toolbar-form""#).nth(1).unwrap();
    let form = form.split("</form>").next().unwrap();
    for name in ["stage", "region", "period", "hot", "order", "owner"] {
        assert!(
            form.contains(&format!(r#"name="{name}""#)),
            "{name}: {form}"
        );
    }
    // Chips need the menus module, for their keys.
    let node: Node = table().into();
    assert!(measure(&node).uses.has(Uses::MENUS));
    // Without the page's choices, a filter of the page's choices offers none of its own.
    let bare = render(&DataTable::new("pipeline", &query, &page).into());
    assert!(
        bare.contains(r#"<select data-slot="native-select" name="region" aria-label="Region"><option value="">Region</option></select>"#),
        "{bare}"
    );
}
