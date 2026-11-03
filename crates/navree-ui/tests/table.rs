//! The table without a browser or a database: the URL, the allow-list, what's drawn.

use navree_ui::table::{Direction, FilterValue, NoFilter, TableRow as _, ValueKind};
use navree_ui::{
    BulkAction, DataTable, FilterBar, Options, Pagination, RowAction, TablePage, TableQuery,
    TableRow, render,
};
use serde::Deserialize;

#[derive(Clone, Copy, Debug, Deserialize, Options, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Stage {
    Lead,
    Won,
}

#[derive(Clone, TableRow)]
#[table(sort = "-amount,title", per = 25, per_pages(10, 25, 50))]
struct Deal {
    #[col(skip)]
    id: i64,
    #[col(sortable, search, sticky, filter = text, link = "/deals/{slug}", label = "Deal")]
    title: String,
    #[col(skip)]
    slug: String,
    #[col(sortable, filter = multi)]
    stage: Stage,
    #[col(sortable, numeric, filter = range)]
    amount: Option<i64>,
    #[col(sortable, filter = date)]
    closed: Option<chrono::NaiveDate>,
    #[col(filter = boolean)]
    hot: bool,
    #[col(hidden, filter = relation)]
    company: Option<i64>,
    #[col(wrap, group = "More")]
    notes: Option<String>,
    #[col(locked, group = "More")]
    owner: String,
}

fn deal(id: i64) -> Deal {
    Deal {
        id,
        title: format!("Deal {id}"),
        slug: format!("deal-{id}"),
        stage: Stage::Won,
        amount: Some(id * 100),
        closed: None,
        hot: false,
        company: None,
        notes: None,
        owner: "Ada".into(),
    }
}

type Q = TableQuery<Deal, NoFilter>;

fn q(query: &str) -> Q {
    TableQuery::parse("/deals", query)
}

fn name(at: usize) -> &'static str {
    <Deal as navree_ui::table::TableRow>::COLUMNS[at].name
}

#[test]
fn defaults_come_from_the_definition() {
    let query = q("");
    assert_eq!(query.per_page(), 25);
    assert_eq!(query.page(), 1);
    let sorts: Vec<_> = query
        .sorts()
        .iter()
        .map(|term| (name(term.column), term.direction))
        .collect();
    assert_eq!(
        sorts,
        [("amount", Direction::Desc), ("title", Direction::Asc)]
    );
    assert_eq!(query.href(), "/deals", "defaults stay out of the URL");
    assert!(
        !query.visible_columns().contains(&5),
        "a hidden column starts hidden: {:?}",
        query.visible_columns()
    );
}

#[test]
fn the_url_is_read_and_written_with_one_spelling() {
    let raw = "sort=stage,-closed&q=ada+b&f.title=x&f.stage=won&f.stage=lead&f.amount.min=10&f.amount.max=20&f.closed.from=2026-01-01&f.hot=true&per=50&page=3&density=compact&cols=title,stage,owner&w=title:240&open=7";
    let query = q(raw);
    assert_eq!(query.search(), Some("ada b"));
    assert_eq!(query.page(), 3);
    assert_eq!(query.per_page(), 50);
    assert_eq!(
        query.filter_on("title"),
        Some(&FilterValue::Text("x".into()))
    );
    assert_eq!(
        query.filter_on("stage"),
        Some(&FilterValue::Values(vec!["won".into(), "lead".into()]))
    );
    assert_eq!(
        query.filter_on("amount"),
        Some(&FilterValue::Range {
            min: Some("10".into()),
            max: Some("20".into())
        })
    );
    assert_eq!(
        query.filter_on("closed"),
        Some(&FilterValue::Range {
            min: Some("2026-01-01".into()),
            max: None
        })
    );
    assert_eq!(query.filter_on("hot"), Some(&FilterValue::Bool(true)));
    assert_eq!(query.open_rows(), ["7"]);
    let once = query.href();
    let again = q(once.split_once('?').unwrap().1);
    assert_eq!(
        again.href(),
        once,
        "reading a written URL writes the same URL"
    );
}

#[test]
fn sort_and_filter_columns_come_only_from_the_definition() {
    let query = q(
        "sort=id;DROP TABLE deals,slug,-owner,title&f.slug=x&f.id=1&f.owner=x&f.nope=y&f.title.min=1&cols=slug,title",
    );
    let sorts: Vec<_> = query.sorts().iter().map(|term| name(term.column)).collect();
    assert_eq!(
        sorts,
        ["title"],
        "skipped, unsortable and unknown columns are ignored"
    );
    assert!(query.filters().is_empty(), "{:?}", query.filters());
    assert!(
        query.href().contains("cols=title%2Cowner") || query.href().contains("cols=title,owner"),
        "a locked column can't be hidden: {}",
        query.href()
    );
}

#[test]
fn page_sizes_are_only_those_offered() {
    assert_eq!(q("per=99999").per_page(), 25);
    assert_eq!(q("per=11").per_page(), 25);
    assert_eq!(q("per=-1").per_page(), 25);
    assert_eq!(q("per=abc").per_page(), 25);
    assert_eq!(q("per=50").per_page(), 50);
    assert_eq!(q("page=0").page(), 1);
    assert_eq!(q("page=abc").page(), 1);
    assert_eq!(q("page=99999999999999").page(), 1);
}

#[test]
fn a_value_of_the_wrong_kind_is_dropped_alone() {
    let query = q(
        "f.amount.min=abc&f.amount.max=20&f.closed.from=not-a-date&f.hot=maybe&f.stage=bogus&f.stage=won&f.company=x",
    );
    assert_eq!(
        query.filter_on("amount"),
        Some(&FilterValue::Range {
            min: None,
            max: Some("20".into())
        })
    );
    assert_eq!(query.filter_on("closed"), None);
    assert_eq!(query.filter_on("hot"), None);
    assert_eq!(
        query.filter_on("stage"),
        Some(&FilterValue::Values(vec!["won".into()]))
    );
    assert_eq!(query.filter_on("company"), None);
}

#[derive(Default, Deserialize, FilterBar)]
struct PageFilter {
    /// Search
    #[filter(search)]
    term: Option<String>,
    /// Stage
    stage: Option<Stage>,
}

#[test]
fn one_bad_value_of_a_page_filter_drops_only_that_filter() {
    let query = TableQuery::<Deal, PageFilter>::parse("/deals", "term=zzzzz&stage=bogus");
    assert_eq!(
        query.filter().term.as_deref(),
        Some("zzzzz"),
        "the good filter stays in force"
    );
    assert_eq!(query.filter().stage, None);
    let query = TableQuery::<Deal, PageFilter>::parse("/deals", "term=a&stage=won");
    assert_eq!(query.filter().stage, Some(Stage::Won));
}

#[test]
fn long_values_are_cut_at_a_character() {
    let long = "é".repeat(500);
    let query = q(&format!("q={long}"));
    assert!(query.search().unwrap().len() <= navree_ui::table::MAX_TEXT);
    assert!(query.search().unwrap().chars().all(|c| c == 'é'));
    let many: String = (0..1000)
        .map(|at| format!("f.stage=won&open={at}&"))
        .collect();
    let query = q(&many);
    assert_eq!(
        query.filter_on("stage"),
        Some(&FilterValue::Values(vec!["won".into()]))
    );
    assert!(query.open_rows().len() <= navree_ui::table::MAX_OPEN_ROWS);
}

#[test]
fn a_cursor_means_nothing_to_a_numbered_table() {
    assert_eq!(q("after=abc").after(), None);
}

fn draw(query: &Q, rows: &[Deal], total: u64) -> String {
    let page = TablePage::new(rows.to_vec(), total);
    render(
        &DataTable::new("deals", query, &page)
            .caption("Deals")
            .into(),
    )
}

#[test]
fn a_stored_slug_stays_one_path_segment() {
    let mut row = deal(1);
    row.slug = "../../logout?all=1#".into();
    let html = draw(&q(""), &[row], 1);
    assert!(
        html.contains(r#"href="/deals/..%2F..%2Flogout%3Fall%3D1%23""#),
        "{html}"
    );
    let mut row = deal(1);
    row.slug = "..".into();
    let html = draw(&q(""), &[row], 1);
    assert!(html.contains(r#"href="/deals/_""#), "{html}");
    let mut row = deal(1);
    row.slug = "//evil.example/".into();
    let html = draw(&q(""), &[row], 1);
    assert!(
        !html.contains(r#"href="/deals///"#) && html.contains("%2F%2Fevil.example%2F"),
        "{html}"
    );
}

#[derive(Clone, TableRow)]
struct WholeLink {
    #[col(link = "{site}")]
    site: String,
}

#[test]
fn a_field_can_not_be_a_whole_url() {
    let rows = vec![
        WholeLink {
            site: "//evil.example/".into(),
        },
        WholeLink {
            site: "javascript:alert(1)".into(),
        },
    ];
    let query = TableQuery::<WholeLink, NoFilter>::parse("/x", "");
    let html = render(&DataTable::new("t", &query, &rows).into());
    assert!(html.contains(r#"href="%2F%2Fevil.example%2F""#), "{html}");
    assert!(
        html.contains(r#"href="javascript%3Aalert%281%29""#),
        "{html}"
    );
}

#[derive(Clone, TableRow)]
struct Hostile {
    #[col(key)]
    title: String,
    custom: Shout,
}

#[derive(Clone)]
struct Shout(String);

impl navree_ui::table::CellValue for Shout {
    fn write_cell(&self, out: &mut navree_ui::table::CellWriter<'_>) {
        out.plain(&self.0);
    }

    fn write_text(&self, out: &mut String) {
        out.push_str(&self.0);
    }
}

impl navree_ui::table::ColumnValue for Shout {
    const KIND: ValueKind = ValueKind::Text;
}

#[test]
fn hostile_strings_are_text_everywhere() {
    let bad = r#"<img src=x onerror=alert(1)>"'&"#;
    let rows = vec![Hostile {
        title: bad.into(),
        custom: Shout(bad.into()),
    }];
    let query = TableQuery::<Hostile, NoFilter>::parse("/x", &format!("q={bad}"));
    let html = render(
        &DataTable::new("t", &query, &rows)
            .bulk("/x/bulk", [BulkAction::new("a", "A")])
            .into(),
    );
    assert!(!html.contains("<img"), "{html}");
    assert!(html.contains("&lt;img"), "{html}");
}

#[test]
fn a_cell_writer_escapes_whatever_a_custom_value_writes() {
    #[derive(Clone)]
    struct Evil;
    impl std::fmt::Display for Evil {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("<script>x</script>")
        }
    }
    impl navree_ui::table::CellValue for Evil {
        fn write_cell(&self, out: &mut navree_ui::table::CellWriter<'_>) {
            out.display(self);
            out.text("<b>");
            out.plain("<i>");
        }

        fn write_text(&self, out: &mut String) {
            out.push('x');
        }
    }
    impl navree_ui::table::ColumnValue for Evil {
        const KIND: ValueKind = ValueKind::Text;
    }
    #[derive(Clone, TableRow)]
    struct Row {
        #[col(key)]
        evil: Evil,
    }
    let query = TableQuery::<Row, NoFilter>::parse("/x", "");
    let html = render(&DataTable::new("t", &query, &vec![Row { evil: Evil }]).into());
    assert!(
        !html.contains("<script>") && !html.contains("<b>") && !html.contains("<i>"),
        "{html}"
    );
}

#[test]
fn pagination_is_instant_at_any_size_and_shows_the_window() {
    let start = std::time::Instant::now();
    let html =
        render(&Pagination::new(2_000_000_000, u32::MAX, |page| format!("/p?page={page}")).into());
    assert!(start.elapsed().as_millis() < 50, "{:?}", start.elapsed());
    let numbers: Vec<&str> = html
        .split("aria-label=\"Page ")
        .skip(1)
        .map(|part| part.split('"').next().unwrap())
        .collect();
    assert_eq!(
        numbers,
        ["1", "1999999999", "2000000000", "2000000001", "4294967295"]
    );
    assert_eq!(html.matches("pagination-ellipsis").count(), 2);
    let html = render(&Pagination::new(1, 3, |page| format!("/p?page={page}")).into());
    assert_eq!(html.matches("aria-label=\"Page ").count(), 3);
    assert_eq!(html.matches("pagination-ellipsis").count(), 0);
}

#[test]
fn every_link_keeps_the_rest_of_the_state() {
    let query = q("f.title=a&q=b&per=50&density=compact&page=2");
    let rows: Vec<Deal> = (1..=50).map(deal).collect();
    let html = draw(&query, &rows, 500);
    for needle in ["f.title=a", "q=b", "per=50", "density=compact"] {
        let links = html
            .split("data-slot=\"pagination-link\"")
            .skip(1)
            .filter(|link| link.contains("href"))
            .count();
        assert!(links > 0);
        for link in html.split("data-slot=\"pagination-link\" href=\"").skip(1) {
            let href = link.split('"').next().unwrap();
            assert!(href.contains(needle), "{needle} in {href}");
        }
    }
}

#[test]
fn sort_links_and_aria_say_what_is_sorted() {
    let html = draw(&q("sort=-stage,title"), &[deal(1)], 1);
    assert!(html.contains(r#"aria-sort="descending""#), "{html}");
    assert!(html.contains(r#"aria-sort="ascending""#), "{html}");
    assert!(
        html.contains("data-slot=\"sort-rank\""),
        "two terms show their priority"
    );
    assert!(html.contains("data-ui-add-sort"), "{html}");
}

#[test]
fn no_rows_with_no_filter_is_not_no_rows_with_a_filter() {
    let html = draw(&q(""), &[], 0);
    assert!(html.contains("Nothing here yet."), "{html}");
    assert!(!html.contains("data-slot=\"table-footer\""), "{html}");
    let html = draw(&q("f.title=zzz"), &[], 0);
    assert!(html.contains("No results match"), "{html}");
    assert!(html.contains("Clear filters"), "{html}");
    assert!(
        html.contains("data-slot=\"filter-chip\""),
        "the chips stay: {html}"
    );
    assert!(
        html.contains("<thead"),
        "the headers and their filters stay: {html}"
    );
}

#[test]
fn one_row_says_one_result_and_a_full_page_has_no_links() {
    let html = draw(&q(""), &[deal(1)], 1);
    assert!(html.contains("1 result"), "{html}");
    assert!(!html.contains("pagination-link"), "{html}");
    let rows: Vec<Deal> = (1..=25).map(deal).collect();
    let html = draw(&q(""), &rows, 25);
    assert!(html.contains("Showing 1 to 25 of 25 results"), "{html}");
    assert!(!html.contains("pagination-link"), "{html}");
    let html = draw(&q(""), &rows, 1_234_567);
    assert!(html.contains("of 1,234,567 results"), "{html}");
}

#[test]
fn the_page_shown_wins_over_the_page_asked_for() {
    let query = q("page=99");
    let mut page = TablePage::new(vec![deal(1)], 26);
    page.page = 2;
    let html = render(&DataTable::new("deals", &query, &page).into());
    assert!(html.contains(r#"data-ui-src="/deals?page=2""#), "{html}");
    assert!(html.contains("Showing 26 to 26 of 26 results"), "{html}");
}

#[test]
fn selection_is_only_drawn_for_who_can_select() {
    let query = q("");
    let rows = vec![deal(1)];
    let html = render(
        &DataTable::new("deals", &query, &rows)
            .bulk("/deals/bulk", [BulkAction::new("archive", "Archive")])
            .into(),
    );
    assert!(
        html.contains("data-ui-select-page")
            && html.contains("data-ui-select-row")
            && html.contains(r#"name="action" value="archive""#),
        "{html}"
    );
    assert!(
        html.contains(r#"method="post""#),
        "a bulk action is a post: {html}"
    );
    let html = render(
        &DataTable::new("deals", &query, &rows)
            .bulk("/deals/bulk", [BulkAction::new("archive", "Archive")])
            .can_select(false)
            .into(),
    );
    assert!(
        !html.contains("data-ui-select-") && !html.contains("Archive"),
        "{html}"
    );
}

#[test]
fn a_row_action_that_changes_something_is_a_post() {
    let query = q("");
    let rows = vec![deal(1)];
    let html = render(
        &DataTable::new("deals", &query, &rows)
            .row_actions(|row| {
                vec![
                    RowAction::link("View", format!("/deals/{}", row.id)),
                    RowAction::post("Delete", format!("/deals/{}/delete", row.id)).destructive(),
                ]
            })
            .csrf("tok")
            .into(),
    );
    assert!(
        html.contains(r#"<form method="post" action="/deals/1/delete""#),
        "{html}"
    );
    assert!(
        !html.contains(r#"<a data-slot="menu-link" href="/deals/1/delete""#),
        "{html}"
    );
    assert!(
        html.contains(r#"name="_csrf""#) || html.contains("tok"),
        "{html}"
    );
}

#[test]
fn state_without_a_control_rides_along_in_the_form() {
    let html = draw(
        &q("sort=title&cols=title,owner&density=compact&w=title:200&f.stage=won"),
        &[deal(1)],
        1,
    );
    assert!(
        html.contains(r#"type="hidden" name="sort" value="title""#),
        "{html}"
    );
    assert!(
        html.contains(r#"type="hidden" name="density" value="compact""#),
        "{html}"
    );
    assert!(
        html.contains(r#"type="hidden" name="w" value="title:200""#),
        "{html}"
    );
    assert!(
        html.contains(r#"type="hidden" name="f.stage" value="won""#),
        "a filter on a hidden column stays in force: {html}"
    );
    assert!(html.contains(r#"width="200""#), "{html}");
    assert!(!html.contains("style="), "no inline style: {html}");
}

#[test]
fn filters_are_drawn_where_they_were_asked_for() {
    let query = q("f.title=ada");
    let rows = vec![deal(1)];
    let html = render(&DataTable::new("deals", &query, &rows).filter_row().into());
    assert!(html.contains("data-slot=\"filter-row\""), "{html}");
    assert_eq!(
        html.matches(r#"name="f.title""#).count(),
        1,
        "one control per filter: {html}"
    );
    let html = render(&DataTable::new("deals", &query, &rows).into());
    assert!(!html.contains("data-slot=\"filter-row\""), "{html}");
    assert_eq!(html.matches(r#"name="f.title""#).count(), 1, "{html}");
}

#[test]
fn header_groups_span_their_columns() {
    let html = draw(&q(""), &[deal(1)], 1);
    assert!(
        html.contains(r#"scope="colgroup" data-slot="th-group" colspan="2">More"#)
            || html.contains(r#"colspan="2">More"#),
        "{html}"
    );
}

#[test]
fn a_page_of_a_table_loads_the_table_module() {
    use navree_ui::Uses;
    let query = q("");
    let rows = vec![deal(1)];
    let node: navree_ui::Node = DataTable::new("deals", &query, &rows).into();
    assert!(
        navree_ui::measure(&node).uses.has(Uses::TABLE),
        "table module"
    );
    assert!(navree_ui::measure(&node).uses.has(Uses::CORE), "core");
}

#[test]
fn csv_export_quotes_and_neutralises() {
    let mut out = String::new();
    navree_ui::table::write_csv_field(&mut out, "=1+1");
    assert_eq!(out, "\"'=1+1\"");
}

#[test]
fn deal_writes_text_cells_for_files() {
    let mut out = String::new();
    deal(1).write_text(0, &mut out);
    assert_eq!(out, "Deal 1");
}

/// A source that isn't the ORM: rows in memory, read through the query's accessors.
struct Memory(Vec<Deal>);

impl navree_ui::table::TableSource<Deal> for Memory {
    type Error = std::convert::Infallible;

    async fn page<F: navree_ui::table::FilterBar + Sync>(
        &self,
        query: &TableQuery<Deal, F>,
    ) -> Result<TablePage<Deal>, Self::Error> {
        let found: Vec<Deal> = self
            .0
            .iter()
            .filter(|deal| query.search().is_none_or(|q| deal.title.contains(q)))
            .cloned()
            .collect();
        let total = found.len() as u64;
        let skip = usize::try_from(query.offset()).unwrap_or(usize::MAX);
        Ok(TablePage::new(
            found
                .into_iter()
                .skip(skip)
                .take(query.per_page() as usize)
                .collect(),
            total,
        ))
    }
}

#[test]
fn a_source_can_be_anything() {
    use navree_ui::table::TableSource as _;
    let source = Memory((1..=30).map(deal).collect());
    let query = q("q=Deal+1&per=10");
    let page = futures_lite(source.page(&query));
    assert_eq!(page.total, navree_ui::table::Total::Exact(11));
    assert!(render(&DataTable::new("deals", &query, &page).into()).contains("of 11 results"));
}

fn futures_lite<T>(
    future: impl std::future::Future<Output = Result<T, std::convert::Infallible>>,
) -> T {
    let mut future = std::pin::pin!(future);
    let waker = std::task::Waker::noop();
    let mut context = std::task::Context::from_waker(waker);
    loop {
        if let std::task::Poll::Ready(Ok(value)) = future.as_mut().poll(&mut context) {
            return value;
        }
    }
}

#[test]
fn a_cell_drawn_from_the_row_goes_through_the_renderer() {
    let query = q("");
    let rows = vec![deal(1)];
    let html = render(
        &DataTable::new("deals", &query, &rows)
            .cell("owner", |_| navree_ui::el("b").child("<i>x</i>"))
            .into(),
    );
    assert!(html.contains("<b>&lt;i&gt;x&lt;/i&gt;</b>"), "{html}");
}

#[test]
fn a_filter_bar_is_state_in_the_url() {
    let query = q("fb=stage,amount,nope,owner");
    assert_eq!(query.href(), "/deals?fb=stage%2Camount");
    let html = render(
        &DataTable::new("deals", &query, &[deal(1)])
            .filter_bar()
            .into(),
    );
    assert!(html.contains("data-slot=\"filter-bar\""), "{html}");
    assert_eq!(
        html.matches(r#"popovertarget="deals-p-"#).count(),
        2,
        "only those chosen: {html}"
    );
    let held = q("fb=stage&f.hot=true");
    let html = render(
        &DataTable::new("deals", &held, &[deal(1)])
            .filter_bar()
            .into(),
    );
    assert_eq!(
        html.matches(r#"popovertarget="deals-p-"#).count(),
        2,
        "a filter in force stays: {html}"
    );
}

#[test]
fn typing_waits_as_long_as_the_table_says() {
    let query = q("");
    let rows = vec![deal(1)];
    let html = render(&DataTable::new("deals", &query, &rows).into());
    assert!(
        !html.contains("data-ui-debounce"),
        "the runtime's 100 ms: {html}"
    );
    let html = render(
        &DataTable::new("deals", &query, &rows)
            .debounce(std::time::Duration::from_millis(400))
            .into(),
    );
    assert!(html.contains(r#"data-ui-debounce="400""#), "{html}");
    let html = render(
        &DataTable::new("deals", &query, &rows)
            .debounce(std::time::Duration::from_secs(3600))
            .into(),
    );
    assert!(
        html.contains(r#"data-ui-debounce="5000""#),
        "capped: {html}"
    );
}
