//! The table without a browser or a database: the URL, the allow-list, what's drawn.

use navree_ui::charts::Grouping;
use navree_ui::table::{Direction, FilterValue, NoFilter, TableRow as _, ValueKind};
use navree_ui::{
    BulkAction, DataTable, FilterBar, Money, Options, Pagination, RowAction, TableLabels,
    TablePage, TableQuery, TableRow, render,
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
        owner: "Anu".into(),
    }
}

/// A table of rupees.
#[derive(Clone, TableRow)]
#[table(sort = "title", per = 25)]
struct Invoice {
    #[col(skip)]
    id: i64,
    #[col(sortable, label = "Invoice")]
    title: String,
    #[col(numeric, label = "Amount")]
    amount: Money,
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

/// A table with a select filter (`filter[stage]=won`), numbered pages.
#[derive(Clone, TableRow)]
#[table(sort = "title", per = 10, per_pages(10, 20))]
struct Prospect {
    #[col(skip)]
    id: i64,
    #[col(sortable, search)]
    title: String,
    #[col(filter = select)]
    stage: Stage,
}

/// A table that pages by cursor.
#[derive(Clone, TableRow)]
#[table(sort = "-title", per = 25, per_pages(10, 25, 50), cursor)]
struct Feed {
    #[col(skip)]
    id: i64,
    #[col(sortable, search, filter = text)]
    title: String,
}

fn prospect(query: &str) -> TableQuery<Prospect, NoFilter> {
    TableQuery::parse("/prospects", query)
}

fn feed(query: &str) -> TableQuery<Feed, NoFilter> {
    TableQuery::parse("/feed", query)
}

/// Every `href` of some HTML, with `&amp;` read back as `&`.
fn hrefs(html: &str) -> Vec<String> {
    html.split("href=\"")
        .skip(1)
        .map(|rest| rest.split('"').next().unwrap().replace("&amp;", "&"))
        .collect()
}

#[test]
fn the_url_is_read_and_written_with_one_spelling() {
    let raw = "sort=stage,-closed&search=anu+b&filter[title][contains]=x&filter[stage][in]=won,lead&filter[amount][gte]=10&filter[amount][lte]=20&filter[closed][gte]=2026-01-01&filter[hot]=true&per_page=50&page=3&density=compact&cols=title,stage,owner&w=title:240&open=7";
    let query = q(raw);
    assert_eq!(query.search(), Some("anu b"));
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
    let written = query.query_string();
    for part in [
        "filter%5Btitle%5D%5Bcontains%5D=x",
        "filter%5Bstage%5D%5Bin%5D=won%2Clead",
        "filter%5Bamount%5D%5Bgte%5D=10&filter%5Bamount%5D%5Blte%5D=20",
        "filter%5Bclosed%5D%5Bgte%5D=2026-01-01",
        "filter%5Bhot%5D=true",
        "sort=stage%2C-closed",
        "search=anu+b",
        "page=3",
        "per_page=50",
        "density=compact",
        "cols=title%2Cstage%2Cowner",
        "w=title%3A240",
        "open=7",
    ] {
        assert!(written.contains(part), "{part} in {written}");
    }
}

/// Reads `raw`, writes it, reads what was written: the same filters and the same URL.
/// Returns the query string written.
fn round_trip<R: navree_ui::table::TableRow>(raw: &str) -> String {
    let first = TableQuery::<R, NoFilter>::parse("/t", raw);
    let href = first.href();
    let written = href.split_once('?').map_or("", |(_, query)| query);
    let again = TableQuery::<R, NoFilter>::parse("/t", written);
    assert_eq!(again.filters(), first.filters(), "{raw} -> {href}");
    assert_eq!(again.search(), first.search(), "{raw} -> {href}");
    assert_eq!(again.href(), href, "{raw}");
    written.to_owned()
}

#[test]
fn every_filter_kind_round_trips_in_the_shared_grammar() {
    // Text: contains.
    assert_eq!(
        round_trip::<Deal>("filter[title][contains]=sutlej+supply"),
        "filter%5Btitle%5D%5Bcontains%5D=sutlej+supply"
    );
    // Multi: a comma list under `in`; a plain value is read as one value of the list.
    assert_eq!(
        round_trip::<Deal>("filter[stage][in]=lead,won"),
        "filter%5Bstage%5D%5Bin%5D=lead%2Cwon"
    );
    assert_eq!(
        q("filter[stage]=won").filter_on("stage"),
        Some(&FilterValue::Values(vec!["won".into()]))
    );
    assert_eq!(
        round_trip::<Deal>("filter[stage]=won"),
        "filter%5Bstage%5D%5Bin%5D=won"
    );
    assert_eq!(
        q("filter[stage][in]=lead&filter[stage][in]=won").filter_on("stage"),
        Some(&FilterValue::Values(vec!["lead".into(), "won".into()])),
        "the key may repeat"
    );
    // Range: at least and at most.
    assert_eq!(
        round_trip::<Deal>("filter[amount][gte]=10&filter[amount][lte]=20"),
        "filter%5Bamount%5D%5Bgte%5D=10&filter%5Bamount%5D%5Blte%5D=20"
    );
    assert_eq!(
        round_trip::<Deal>("filter[amount][lte]=20"),
        "filter%5Bamount%5D%5Blte%5D=20"
    );
    // Dates are a range too. The last day stays a plain date here: the end of that day
    // is added where the query runs.
    assert_eq!(
        round_trip::<Deal>("filter[closed][gte]=2026-01-01&filter[closed][lte]=2026-12-31"),
        "filter%5Bclosed%5D%5Bgte%5D=2026-01-01&filter%5Bclosed%5D%5Blte%5D=2026-12-31"
    );
    assert_eq!(
        q("filter[closed][lte]=2026-12-31").filter_on("closed"),
        Some(&FilterValue::Range {
            min: None,
            max: Some("2026-12-31".into())
        })
    );
    // Boolean.
    assert_eq!(
        round_trip::<Deal>("filter[hot]=false"),
        "filter%5Bhot%5D=false"
    );
    // Relation: a list of keys, on a column that is hidden.
    assert_eq!(
        round_trip::<Deal>("filter[company][in]=7,9"),
        "filter%5Bcompany%5D%5Bin%5D=7%2C9"
    );
    assert_eq!(
        q("filter[company][in]=7,9").filter_on("company"),
        Some(&FilterValue::Values(vec!["7".into(), "9".into()]))
    );
    // Is NULL: `true` or `1`; `false` means "is not NULL", which a table has no filter for.
    assert_eq!(
        round_trip::<Deal>("filter[amount][null]=true"),
        "filter%5Bamount%5D%5Bnull%5D=true"
    );
    assert_eq!(
        q("filter[amount][null]=1").filter_on("amount"),
        Some(&FilterValue::Empty)
    );
    assert_eq!(q("filter[amount][null]=false").filter_on("amount"), None);
    assert_eq!(
        q("filter[amount][null]=true&filter[amount][gte]=5").filter_on("amount"),
        Some(&FilterValue::Empty),
        "empty wins over a range"
    );
    // A column that cannot hold NULL has no such filter.
    assert_eq!(q("filter[hot][null]=true").filter_on("hot"), None);
    // Select: one value, `filter[stage]=won`; `in` is read too, and its first value is taken.
    assert_eq!(
        round_trip::<Prospect>("filter[stage]=won"),
        "filter%5Bstage%5D=won"
    );
    assert_eq!(
        round_trip::<Prospect>("filter[stage][in]=won,lead"),
        "filter%5Bstage%5D=won"
    );
    assert_eq!(
        prospect("filter[stage][in]=won,lead").filter_on("stage"),
        Some(&FilterValue::Values(vec!["won".into()]))
    );
    assert_eq!(
        prospect("filter[stage]=bogus").filter_on("stage"),
        None,
        "a value that is not an option is ignored"
    );
    // The page size and the order ride along.
    assert_eq!(
        round_trip::<Prospect>("filter[stage]=lead&sort=-title&per_page=20&page=2"),
        "filter%5Bstage%5D=lead&sort=-title&page=2&per_page=20"
    );
}

#[test]
fn the_list_part_is_written_as_a_list_query_is() {
    use navree_http::list::ListQuery;
    let query = q(
        "per_page=50&page=3&search=anu+b&sort=-closed&filter[hot]=true&filter[amount][lte]=20&filter[amount][gte]=10&filter[stage][in]=won,lead&filter[title][contains]=x",
    );
    let written = query.query_string();
    let (list, problems) = ListQuery::parse_lenient(&written);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(
        list.to_query_string(),
        written,
        "the same state is the same string in Rust and in the browser"
    );
    assert_eq!(list.search.as_deref(), Some("anu b"));
    assert_eq!(list.page, Some(3));
    assert_eq!(list.per_page, Some(50));
    assert_eq!(list.sort.len(), 1);
    assert_eq!(list.filter.len(), 5);
    assert_eq!(list.eq("hot"), Some("true"));
}

#[test]
fn search_and_sort_are_the_list_querys() {
    assert_eq!(q("search=%20%20anu%20").search(), Some("anu"));
    assert_eq!(q("search=").search(), None);
    assert_eq!(q("search=a%00b").search(), None, "a NUL is refused");
    let query = q("sort=-amount,title,-closed,stage,hot");
    let sorts: Vec<_> = query
        .sorts()
        .iter()
        .map(|term| (name(term.column), term.direction))
        .collect();
    assert_eq!(
        sorts,
        [
            ("amount", Direction::Desc),
            ("title", Direction::Asc),
            ("closed", Direction::Desc),
            ("stage", Direction::Asc)
        ],
        "at most four, and only sortable columns"
    );
    let query = q("sort=-amount&sort=title");
    assert_eq!(query.sorts().len(), 2, "a repeated sort adds terms");
    let none = q("sort=");
    assert!(none.sorts().is_empty(), "sort= says no order");
    assert_eq!(none.href(), "/deals?sort=");
    assert_eq!(q("sort=-amount,title").href(), "/deals");
}

#[test]
fn the_old_names_are_no_longer_read() {
    let query = q(
        "q=anu&per=50&f.title=x&f.stage=won&f.amount.min=1&f.amount.max=9&f.hot=true&f.amount.empty=1&after=abc&before=def",
    );
    assert_eq!(query.search(), None);
    assert_eq!(query.per_page(), 25);
    assert!(query.filters().is_empty(), "{:?}", query.filters());
    assert_eq!(query.after(), None);
    assert_eq!(query.before(), None);
    assert_eq!(query.href(), "/deals");
    let query = feed("q=anu&per=50&f.title=x&after=abc");
    assert_eq!(query.search(), None);
    assert_eq!(query.per_page(), 25);
    assert!(query.filters().is_empty());
    assert_eq!(query.after(), None);
    assert_eq!(query.href(), "/feed");
}

#[test]
fn sort_and_filter_columns_come_only_from_the_definition() {
    let query = q(
        "sort=id;DROP TABLE deals,slug,-owner,title&filter[slug]=x&filter[id]=1&filter[owner]=x&filter[nope]=y&filter[title][gte]=1&cols=slug,title",
    );
    let sorts: Vec<_> = query.sorts().iter().map(|term| name(term.column)).collect();
    assert_eq!(
        sorts,
        ["title"],
        "skipped, unsortable and unknown columns are ignored"
    );
    assert!(query.filters().is_empty(), "{:?}", query.filters());
    assert!(
        query.href().contains("cols=title%2Cowner"),
        "a locked column can't be hidden: {}",
        query.href()
    );
}

#[test]
fn hostile_list_parameters_are_ignored() {
    let big = "x".repeat(100 * 1024);
    let thousand_filters: String = (0..2000)
        .map(|at| format!("filter[f{at}]=1"))
        .collect::<Vec<_>>()
        .join("&");
    let thousand_sorts: String = (0..2000)
        .map(|_| "sort=nope".to_owned())
        .collect::<Vec<_>>()
        .join("&");
    for raw in [
        format!("filter[title][contains]={big}"),
        format!("search={big}&filter[hot]=maybe"),
        "filter[__proto__]=x".to_owned(),
        "filter[constructor][prototype]=x".to_owned(),
        "filter[title][regex]=x".to_owned(),
        "filter[title][contains][deeper]=x".to_owned(),
        "filter[title][$ne]=x".to_owned(),
        "filter[=x&filter[]=y&filter=z&filter]=w".to_owned(),
        "filter[title]=a&filter[title]=b".to_owned(),
        format!("filter[title][contains]={}", "é".repeat(200)),
        thousand_filters,
        thousand_sorts,
    ] {
        let query = q(&raw);
        assert!(query.filters().is_empty(), "{:?}", query.filters());
        assert!(query.href().len() < 4096, "{}", query.href().len());
    }
    // The good part of a hostile query stays.
    let query = q(&format!(
        "filter[title][regex]=x&filter[hot]=true&filter[title][contains]={big}"
    ));
    assert_eq!(query.filter_on("hot"), Some(&FilterValue::Bool(true)));
    assert_eq!(query.filter_on("title"), None);
}

#[test]
fn page_sizes_are_only_those_offered() {
    assert_eq!(q("per_page=99999").per_page(), 25);
    assert_eq!(q("per_page=11").per_page(), 25);
    assert_eq!(q("per_page=-1").per_page(), 25);
    assert_eq!(q("per_page=abc").per_page(), 25);
    assert_eq!(q("per_page=0").per_page(), 25);
    assert_eq!(q("per_page=50").per_page(), 50);
    assert_eq!(q("per_page=10").per_page(), 10);
    assert_eq!(
        q("limit=50").per_page(),
        25,
        "a numbered table has no limit"
    );
    assert_eq!(q("per=50").per_page(), 25, "per was the old name");
    assert_eq!(q("page=0").page(), 1);
    assert_eq!(q("page=abc").page(), 1);
    assert_eq!(q("page=99999999999999").page(), 1);
    assert_eq!(q("page=10000000").page(), 10_000_000);
    assert_eq!(q("page=10000001").page(), 1);
}

#[test]
fn a_value_of_the_wrong_kind_is_dropped_alone() {
    let query = q(
        "filter[amount][gte]=abc&filter[amount][lte]=20&filter[closed][gte]=not-a-date&filter[hot]=maybe&filter[stage][in]=bogus,won&filter[company]=x",
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
    assert_eq!(
        q("filter[amount][null]=maybe").filter_on("amount"),
        None,
        "NULL is true or false"
    );
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
    let query = q(&format!("search={long}"));
    assert!(query.search().unwrap().len() <= navree_ui::table::MAX_TEXT);
    assert!(query.search().unwrap().chars().all(|c| c == 'é'));
    let many: String = (0..1000)
        .map(|at| format!("filter[stage][in]=won&open={at}&"))
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
    assert_eq!(q("cursor=abc").after(), None);
    assert_eq!(q("before=abc").before(), None);
    assert_eq!(q("limit=50&cursor=abc").per_page(), 25);
    assert_eq!(q("after=abc").after(), None, "after was the old name");
    let query = q("page=2&cursor=abc&before=def&limit=10");
    assert_eq!(query.page(), 2);
    assert_eq!(query.after(), None);
    assert_eq!(query.before(), None);
    assert_eq!(query.per_page(), 25);
    assert_eq!(query.href(), "/deals?page=2");
}

#[test]
fn a_cursor_table_reads_cursor_before_and_limit() {
    let query = feed("cursor=abc&limit=50");
    assert_eq!(query.after(), Some("abc"));
    assert_eq!(query.before(), None);
    assert_eq!(query.per_page(), 50);
    assert_eq!(feed("before=xyz").before(), Some("xyz"));
    assert_eq!(feed("limit=11").per_page(), 25, "only the sizes on offer");
    // The cursor is the position: a page number beside it means nothing, and `cursor`
    // wins over `before`.
    let query = feed("cursor=abc&before=xyz&page=4&per_page=50");
    assert_eq!(query.after(), Some("abc"));
    assert_eq!(query.before(), None);
    assert_eq!(query.page(), 1);
    assert_eq!(query.per_page(), 25, "per_page is the numbered pages' size");
    // A cursor of 1,024 bytes is whole; one past the limit is ignored.
    let whole = "c".repeat(1024);
    assert_eq!(
        feed(&format!("cursor={whole}")).after(),
        Some(whole.as_str())
    );
    assert_eq!(feed(&format!("cursor={whole}c")).after(), None);
    // Written the way a list query is: the page size after the cursor.
    assert_eq!(
        feed("limit=50&before=xyz&filter[title][contains]=a").href(),
        "/feed?filter%5Btitle%5D%5Bcontains%5D=a&before=xyz&limit=50"
    );
    assert_eq!(
        feed("cursor=abc&limit=50").href(),
        "/feed?cursor=abc&limit=50"
    );
    let again = feed(&feed("cursor=abc&limit=50").query_string());
    assert_eq!(again.after(), Some("abc"));
    assert_eq!(again.per_page(), 50);
}

#[test]
fn a_cursor_tables_links_and_page_size_use_the_same_names() {
    let query = feed("limit=50&search=anu");
    let rows = vec![Feed {
        id: 1,
        title: "Anu".into(),
    }];
    let mut page = TablePage::new(rows, 0);
    page.next = Some("n1".into());
    page.prev = Some("p1".into());
    let html = render(&DataTable::new("feed", &query, &page).into());
    let links = hrefs(&html);
    assert!(
        links
            .iter()
            .any(|href| href == "/feed?search=anu&cursor=n1&limit=50"),
        "next: {links:?}"
    );
    assert!(
        links
            .iter()
            .any(|href| href == "/feed?search=anu&before=p1&limit=50"),
        "previous: {links:?}"
    );
    assert!(
        links.iter().any(|href| href == "/feed?search=anu&limit=50"),
        "first: {links:?}"
    );
    assert!(html.contains(r#"name="limit""#), "the page size: {html}");
    assert!(!html.contains(r#"name="per_page""#), "{html}");
    assert!(!html.contains(r#"name="per""#), "{html}");
    assert!(
        !links.iter().any(|href| href.contains("after=")),
        "{links:?}"
    );
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
    let query = TableQuery::<Hostile, NoFilter>::parse("/x", &format!("search={bad}"));
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
    let query = q("filter[title][contains]=a&search=b&per_page=50&density=compact&page=2");
    let rows: Vec<Deal> = (1..=50).map(deal).collect();
    let html = draw(&query, &rows, 500);
    for needle in [
        "filter%5Btitle%5D%5Bcontains%5D=a",
        "search=b",
        "per_page=50",
        "density=compact",
    ] {
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
    let html = draw(&q("filter[title][contains]=zzz"), &[], 0);
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
fn chips_and_clear_links_write_the_shared_names() {
    let html = draw(
        &q("filter[stage][in]=won&search=anu&filter[amount][gte]=5&page=2&per_page=50"),
        &[deal(1)],
        1,
    );
    let links = hrefs(&html);
    let stage = "filter%5Bstage%5D%5Bin%5D=won";
    let amount = "filter%5Bamount%5D%5Bgte%5D=5";
    // Each chip's link leaves that one thing out, goes back to page 1 and keeps the rest.
    for want in [
        format!("/deals?{stage}&{amount}&per_page=50"),
        format!("/deals?{amount}&search=anu&per_page=50"),
        format!("/deals?{stage}&search=anu&per_page=50"),
        "/deals?per_page=50".to_owned(),
    ] {
        assert!(links.contains(&want), "{want} in {links:?}");
    }
    assert!(
        links
            .iter()
            .all(|href| !href.contains("q=") && !href.contains("f.") && !href.contains("per=")),
        "{links:?}"
    );
}

#[test]
fn the_export_link_carries_the_state_in_the_shared_names() {
    let query = q("search=anu&filter[hot]=true&per_page=50&page=2&sort=-closed");
    let rows = vec![deal(1)];
    let html = render(
        &DataTable::new("deals", &query, &rows)
            .export("/deals/export")
            .into(),
    );
    let want = "/deals/export?filter%5Bhot%5D=true&sort=-closed&search=anu&per_page=50";
    assert!(hrefs(&html).iter().any(|href| href == want), "{html}");
}

#[test]
fn the_toolbar_controls_are_named_in_the_shared_grammar() {
    let html = draw(
        &q("filter[title][contains]=anu&search=x&per_page=50"),
        &[deal(1)],
        1,
    );
    for name in [
        "search",
        "filter[title][contains]",
        "filter[stage][in]",
        "filter[amount][gte]",
        "filter[amount][lte]",
        "filter[amount][null]",
        "filter[closed][gte]",
        "filter[closed][lte]",
        "filter[hot]",
        "per_page",
    ] {
        assert!(
            html.contains(&format!(r#"name="{name}""#)),
            "{name} in {html}"
        );
    }
    for old in [r#"name="q""#, r#"name="per""#, r#"name="f."#] {
        assert!(!html.contains(old), "{old} in {html}");
    }
    // The empty tick says `true`.
    assert!(
        html.contains(r#"name="filter[amount][null]" value="true""#),
        "{html}"
    );
    // The page size and the values in force are what the controls show.
    assert!(html.contains(r#"value="anu""#), "{html}");
    assert!(html.contains(r#"value="x""#), "{html}");
}

#[test]
fn a_select_filter_is_a_select_named_by_its_column() {
    let page = TablePage::new(
        vec![Prospect {
            id: 1,
            title: "Arjun Sethi".into(),
            stage: Stage::Won,
        }],
        1,
    );
    let query = prospect("filter[stage]=won");
    let html = render(&DataTable::new("prospects", &query, &page).into());
    assert!(html.contains(r#"name="filter[stage]""#), "{html}");
    assert!(!html.contains(r#"name="filter[stage][in]""#), "{html}");
    assert!(!html.contains(r#"name="f.stage""#), "{html}");
    assert!(html.contains(r#"<option value="won" selected>"#), "{html}");
    assert!(
        hrefs(&html)
            .iter()
            .any(|href| href == "/prospects?filter%5Bstage%5D=won&sort=-title"),
        "the title's sort link keeps the filter: {html}"
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
fn counts_and_amounts_are_grouped_as_the_labels_group_them() {
    let query: TableQuery<Invoice, NoFilter> = TableQuery::parse("/invoices", "");
    let rows = vec![Invoice {
        id: 1,
        title: "INV-1".into(),
        amount: Money(123_456_750),
    }];
    let page = TablePage::new(rows, 1_234_567);

    let thousands = render(&DataTable::new("invoices", &query, &page).into());
    assert!(thousands.contains("of 1,234,567 results"), "{thousands}");
    assert!(thousands.contains("1,234,567.50"), "{thousands}");

    // A table of rupees: lakhs and crores, in the count and in the amount.
    let labels = TableLabels {
        grouping: Grouping::Indian,
        ..TableLabels::default()
    };
    let lakhs = render(
        &DataTable::new("invoices", &query, &page)
            .labels(&labels)
            .into(),
    );
    assert!(lakhs.contains("of 12,34,567 results"), "{lakhs}");
    assert!(lakhs.contains("12,34,567.50"), "{lakhs}");
    assert!(!lakhs.contains("1,234,567"), "{lakhs}");
    // The browser groups the selection's count the same way.
    assert!(
        lakhs.contains("&quot;grouping&quot;:&quot;indian&quot;"),
        "{lakhs}"
    );
    assert!(!thousands.contains("grouping&quot;"), "{thousands}");
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
        &q("sort=title&cols=title,owner&density=compact&w=title:200&filter[stage][in]=won"),
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
        html.contains(r#"type="hidden" name="filter[stage][in]" value="won""#),
        "a filter on a hidden column stays in force: {html}"
    );
    assert!(html.contains(r#"width="200""#), "{html}");
    assert!(!html.contains("style="), "no inline style: {html}");
}

#[test]
fn filters_are_drawn_where_they_were_asked_for() {
    let query = q("filter[title][contains]=anu");
    let rows = vec![deal(1)];
    let html = render(&DataTable::new("deals", &query, &rows).filter_row().into());
    assert!(html.contains("data-slot=\"filter-row\""), "{html}");
    assert_eq!(
        html.matches(r#"name="filter[title][contains]""#).count(),
        1,
        "one control per filter: {html}"
    );
    let html = render(&DataTable::new("deals", &query, &rows).into());
    assert!(!html.contains("data-slot=\"filter-row\""), "{html}");
    assert_eq!(
        html.matches(r#"name="filter[title][contains]""#).count(),
        1,
        "{html}"
    );
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
    let query = q("search=Deal+1&per_page=10");
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
    let held = q("fb=stage&filter[hot]=true");
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

#[test]
fn a_page_that_holds_every_row_can_sort_them_in_the_browser() {
    let rows: Vec<Deal> = (1..=3).map(deal).collect();
    let html = draw(&q(""), &rows, 3);
    assert!(html.contains("data-ui-local"), "{html}");
    assert!(
        html.contains(r#"data-ui-sort-default="-amount,title""#),
        "the order with no sort in the URL: {html}"
    );
    assert!(
        html.contains(r#"data-col="amount" data-numeric data-kind="n""#),
        "{html}"
    );
    assert!(
        html.contains(r#"data-col="closed" data-kind="d""#),
        "{html}"
    );
    assert!(
        !html.contains(r#"data-col="title" data-kind"#),
        "text compares as text: {html}"
    );
    // A cell the page draws itself carries its value.
    let page = TablePage::new(rows.clone(), 3);
    let html = render(
        &DataTable::new("deals", &q(""), &page)
            .cell("amount", |row| {
                navree_ui::el("strong").child(format!("about {}", row.id))
            })
            .into(),
    );
    assert!(html.contains(r#"data-v="100""#), "{html}");
    assert!(
        !html.contains(r#"data-v="Deal"#),
        "only drawn cells: {html}"
    );
}

#[test]
fn a_page_that_holds_part_of_the_result_asks_the_server() {
    let rows: Vec<Deal> = (1..=3).map(deal).collect();
    for (query, total) in [("", 30), ("page=2", 28), ("per_page=10", 11)] {
        let html = draw(&q(query), &rows, total);
        assert!(!html.contains("data-ui-local"), "{query}: {html}");
        assert!(!html.contains("data-kind"), "{query}: {html}");
    }
    let html = draw(&q(""), &[], 0);
    assert!(!html.contains("data-ui-local"), "no rows: {html}");
}

/// Only numbers and dates whose cells show the value the store sorts: text follows the
/// database's collation, a relation sorts by a key the cell doesn't show, and a number
/// column whose cells show words can't be read back. Those ask the server.
#[test]
fn the_browser_sorts_only_values_it_can_read_as_the_store_does() {
    #[derive(Clone, TableRow)]
    #[table(per = 25)]
    struct Local {
        #[col(skip)]
        id: i64,
        #[col(sortable)]
        name: String,
        #[col(sortable, kind = int, source = "company_id")]
        company: String,
        #[col(sortable, kind = int)]
        code: String,
        #[col(sortable, numeric)]
        amount: Option<i64>,
        #[col(sortable)]
        closed: Option<chrono::NaiveDate>,
    }
    let row = |id: i64, code: &str, amount: Option<i64>| Local {
        id,
        name: format!("Deal {id}"),
        company: "Agra".into(),
        code: code.into(),
        amount,
        closed: chrono::NaiveDate::from_ymd_opt(2026, 1, u32::try_from(id).unwrap()),
    };
    let rows = vec![row(1, "12", Some(5)), row(2, "A1", None)];
    let page = TablePage::new(rows, 2);
    let query = TableQuery::<Local>::parse("/deals", "");
    let html = render(&DataTable::new("deals", &query, &page).into());
    assert!(html.contains("data-ui-local"), "{html}");
    for column in ["name", "company", "code"] {
        assert!(
            !html.contains(&format!(r#"data-col="{column}" data-kind"#)),
            "{column} asks the server: {html}"
        );
    }
    assert!(
        html.contains(r#"data-col="amount" data-numeric data-kind="n""#),
        "{html}"
    );
    assert!(
        html.contains(r#"data-col="closed" data-kind="d""#),
        "{html}"
    );
    // Every cell of those columns says its value, NULL as empty.
    assert!(html.contains(r#"data-v="5""#), "{html}");
    assert!(html.contains(r#"data-v="""#), "{html}");
    assert!(html.contains(r#"data-v="2026-01-01""#), "{html}");
    assert!(!html.contains(r#"data-v="Deal"#), "{html}");
    assert!(!html.contains(r#"data-v="A1""#), "{html}");
    assert!(!html.contains(r#"data-v="Agra""#), "{html}");
}

/// The names of the columns shown, in order.
fn shown(query: &Q) -> Vec<&'static str> {
    query.visible_columns().iter().map(|at| name(*at)).collect()
}

#[test]
fn hidden_is_how_a_column_starts_and_not_who_may_use_it() {
    let query = q("cols=title,company&filter[company][in]=7");
    assert_eq!(shown(&query), ["title", "company", "owner"]);
    assert!(query.filter_on("company").is_some());
}

#[test]
fn a_column_taken_away_is_in_no_part_of_the_state() {
    let url = "cols=title,amount,company&sort=-amount,title&filter[amount][gte]=500\
               &filter[stage][in]=won&filter[company][in]=7&w=amount:200,title:300\
               &fb=amount,stage&search=sutlej";
    assert_eq!(
        shown(&q(url)),
        ["title", "amount", "company", "owner"],
        "all there until one is taken away"
    );
    let query = q(url).without_columns(["amount", "company"]);
    assert_eq!(shown(&query), ["title", "owner"]);
    assert_eq!(query.sorts().len(), 1);
    assert_eq!(query.sort(), Some(("title", Direction::Asc)));
    assert_eq!(query.filters().len(), 1);
    assert!(query.filter_on("stage").is_some());
    assert!(query.filter_on("amount").is_none());
    assert!(query.filter_on("company").is_none());
    assert_eq!(query.search(), Some("sutlej"), "a search column is left");
    for text in [query.href(), query.filter_key()] {
        assert!(
            !text.contains("amount") && !text.contains("company"),
            "{text}"
        );
    }
    assert!(query.href().contains("fb=stage"), "{}", query.href());
    // What the table writes never names them, so reading it back is the same.
    let again = TableQuery::<Deal, NoFilter>::parse("/deals", &query.query_string());
    assert_eq!(shown(&again), ["title", "owner"]);
    assert_eq!(again.filters(), query.filters());
}

#[test]
fn taking_columns_away_adds_up_and_unknown_names_are_ignored() {
    let query = q("cols=title,amount,hot")
        .without_columns(["amount", "no such column"])
        .without_columns(vec![String::from("hot")]);
    assert_eq!(shown(&query), ["title", "owner"]);
}

#[test]
fn only_columns_lets_in_what_it_names_and_nothing_else() {
    let query = q("cols=title,amount,hot&sort=-amount&filter[hot]=true&filter[stage][in]=won")
        .only_columns(["title", "hot", "no such column"]);
    assert_eq!(shown(&query), ["title", "hot"]);
    assert!(query.sorts().is_empty());
    assert_eq!(query.filters().len(), 1);
    assert!(query.filter_on("hot").is_some());
    // A hidden column can be the one that is left.
    assert_eq!(shown(&q("").only_columns(["company"])), ["company"]);
}

#[test]
fn a_default_order_on_a_column_taken_away_is_not_in_the_urls() {
    let query = q("").without_columns(["amount"]);
    assert_eq!(query.sort(), Some(("title", Direction::Asc)));
    assert_eq!(query.sorts().len(), 1);
    assert_eq!(query.href(), "/deals", "what is left is the default");
}

#[test]
fn a_search_with_no_column_left_to_search_is_ignored() {
    let query = q("search=sutlej&filter[title][contains]=anu").without_columns(["title"]);
    assert_eq!(query.search(), None);
    assert!(query.filters().is_empty());
    assert!(!query.is_filtered());
    let rows = [deal(1)];
    assert!(draw(&q(""), &rows, 1).contains(r#"name="search""#));
    let html = draw(&query, &rows, 1);
    assert!(!html.contains(r#"name="search""#), "no search box: {html}");
}

#[test]
fn a_column_taken_away_is_not_drawn_offered_or_filterable() {
    let rows = [deal(1)];
    let tokens = |column: &str| {
        [
            format!(r#"data-col="{column}""#),
            format!(r#"value="{column}""#),
            format!(r#"name="filter[{column}][null]""#),
        ]
    };
    let before = draw(&q(""), &rows, 1);
    for token in tokens("amount") {
        assert!(before.contains(&token), "{token} in {before}");
    }
    assert!(before.contains(r#"value="company""#), "{before}");
    let query = q("cols=title,amount,company&filter[amount][gte]=5&sort=-amount")
        .without_columns(["amount", "company"]);
    let html = draw(&query, &rows, 1);
    for column in ["amount", "company"] {
        for token in tokens(column) {
            assert!(!html.contains(&token), "{token} in {html}");
        }
        assert!(!html.contains(&format!("filter[{column}]")), "{html}");
        assert!(!html.contains(&format!("filter%5B{column}")), "{html}");
    }
    assert!(html.contains(r#"data-col="title""#), "{html}");
    assert!(html.contains(r#"name="cols" value="stage""#), "{html}");
}

#[test]
fn a_saved_view_applied_by_default_leaves_the_columns_taken_away_out() {
    use navree_ui::table::SavedView;
    let views = [SavedView {
        id: "v1".to_owned(),
        name: "Big deals".to_owned(),
        query: "sort=-amount&filter[amount][gte]=500&cols=title,amount".to_owned(),
        is_default: true,
    }];
    let opened = q("").or_default_view(&views);
    assert!(opened.filter_on("amount").is_some(), "the view applies");
    assert!(shown(&opened).contains(&"amount"));
    for query in [
        q("").without_columns(["amount"]).or_default_view(&views),
        q("").or_default_view(&views).without_columns(["amount"]),
    ] {
        assert!(query.filter_on("amount").is_none());
        assert!(
            query
                .sorts()
                .iter()
                .all(|term| name(term.column) != "amount")
        );
        assert!(!shown(&query).contains(&"amount"));
    }
}

#[test]
fn a_bulk_request_holds_the_same_columns_back() {
    let body = b"action=archive&_mode=all&_expected=3&_q=filter%5Bamount%5D%5Bgte%5D%3D500%26filter%5Bstage%5D%5Bin%5D%3Dwon";
    let request = navree_ui::BulkRequest::<Deal, NoFilter>::parse(body).unwrap();
    assert_eq!(request.query().filters().len(), 2);
    let request = request.without_columns(["amount"]);
    assert_eq!(request.query().filters().len(), 1);
    assert!(request.query().filter_on("stage").is_some());
}

#[test]
fn a_columns_alignment_is_for_its_cells_not_for_a_row_drawn_inside_one() {
    // A cell may hold a `Row` centred on its cross axis, which writes the same
    // `data-align` attribute: the column's text alignment must not reach into it.
    let css = include_str!("../styles/25-table.css");
    assert!(
        !css.contains("& [data-align=center]{text-align:center}"),
        "a centred row inside a cell would centre its text"
    );
    assert!(css.contains("& :is(th,td)[data-align=center]{text-align:center}"));
    assert!(css.contains("& :is(th,td)[data-align=end]{text-align:end}"));
}
