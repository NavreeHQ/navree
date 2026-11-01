//! The derives, end to end: a validated struct draws its form and reads it
//! back; a row struct draws its table with sort, filter and page in the URL.

use navree_ui::form::{Form, FormView, Money, Widget, decode};
use navree_ui::table::{Direction, TableRow};
use navree_ui::test_view::Clicked;
use navree_ui::{BadgeVariant, DataTable, FilterBar, Form, InputKind, Options, TableQuery, TableRow, TestView, Validate, View, render};
use serde::Deserialize;

#[derive(Clone, Copy, Debug, Deserialize, Options, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Stage {
    /// Lead
    Lead,
    #[option(label = "Closed won")]
    ClosedWon,
}

#[derive(Debug, Deserialize, Validate, Form)]
struct NewDeal {
    /// Deal name
    #[validate(length(min = 1, max = 255))]
    #[form(placeholder = "Annual platform license", autofocus, wide, error = "Give the deal a name.")]
    title: String,
    /// Value
    #[form(addon = "currency")]
    #[validate(skip)]
    amount: Money,
    #[form(widget = select)]
    #[validate(skip)]
    currency: Currency,
    #[validate(skip)]
    stage: Stage,
    /// Expected close
    #[validate(skip)]
    close_date: Option<chrono::NaiveDate>,
    #[validate(email)]
    contact_email: Option<String>,
    #[validate(range(min = 0, max = 100))]
    probability: Option<i32>,
    #[form(widget = textarea, rows = 5, help = "Shown on the card.")]
    #[validate(length(max = 2000))]
    notes: Option<String>,
    #[validate(skip)]
    is_hot: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Options, PartialEq, Eq)]
enum Currency {
    USD,
    EUR,
}

fn html() -> String {
    render(&FormView::<NewDeal>::new().post("/deals").submit("Create deal").into())
}

#[test]
fn the_type_picks_the_widget_and_the_doc_comment_the_label() {
    let fields = NewDeal::FIELDS;
    assert_eq!(fields[0].label, "Deal name");
    assert_eq!(fields[0].widget, Widget::Input(InputKind::Text));
    assert_eq!(fields[1].widget, Widget::Money);
    assert_eq!(fields[3].widget, Widget::Select);
    assert_eq!(fields[3].options, [("lead", "Lead"), ("closed_won", "Closed won")]);
    assert_eq!(fields[4].widget, Widget::Input(InputKind::Date));
    assert!(fields[4].optional);
    assert_eq!(fields[5].label, "Contact email");
    assert_eq!(fields[2].options, [("USD", "USD"), ("EUR", "EUR")], "a name in capitals keeps them");
    assert_eq!(fields[8].widget, Widget::Checkbox);
}

#[test]
fn rules_become_native_constraints() {
    let html = html();
    assert!(html.contains(r#"name="title" value"#) || html.contains(r#"name="title""#), "{html}");
    assert!(html.contains(r#"placeholder="Annual platform license" required autofocus minlength="1" maxlength="255""#), "{html}");
    assert!(html.contains(r#"type="email" name="contact_email""#), "{html}");
    assert!(html.contains(r#"min="0" max="100""#), "{html}");
    assert!(html.contains(r#"maxlength="2000""#), "{html}");
}

#[test]
fn labels_tie_to_their_controls_and_addons_sit_in_the_group() {
    let html = html();
    assert!(html.contains(r#"for="new-deal-title""#), "{html}");
    assert!(html.contains(r#"id="new-deal-title""#), "{html}");
    assert!(html.contains(r#"<form data-slot="form" id="new-deal" method="post" action="/deals" data-ui-form>"#), "{html}");
    assert!(html.contains(r#"data-slot="input-group""#), "{html}");
    assert!(html.contains(r#"aria-label="Currency""#), "{html}");
    assert_eq!(html.matches(r#"for="new-deal-currency""#).count(), 0, "{html}");
    assert!(html.contains(r#"data-slot="button" data-variant="primary" type="submit">Create deal"#), "{html}");
}

fn good() -> Vec<(String, String)> {
    [("title", "Acme"), ("amount", "12,500.50"), ("currency", "USD"), ("stage", "closed_won"), ("close_date", ""), ("contact_email", ""), ("probability", ""), ("notes", "")]
        .iter()
        .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
        .collect()
}

#[test]
fn a_good_submission_decodes() {
    let deal: NewDeal = decode(&good()).unwrap();
    assert_eq!(deal.title, "Acme");
    assert_eq!(deal.amount, Money(1_250_050));
    assert_eq!(deal.stage, Stage::ClosedWon);
    assert_eq!(deal.close_date, None, "an empty optional is absent");
    assert_eq!(deal.contact_email, None);
    assert!(!deal.is_hot, "an unchecked box is false");
}

#[test]
fn rule_failures_land_on_their_fields() {
    let mut pairs = good();
    pairs[0].1 = String::new();
    pairs[5].1 = "nope".to_owned();
    let errors = decode::<NewDeal>(&pairs).unwrap_err();
    assert_eq!(errors["title"], ["Give the deal a name."]);
    assert!(errors["contact_email"][0].contains("Contact email"), "{errors:?}");
}

#[test]
fn a_value_of_the_wrong_type_is_a_field_error_not_a_crash() {
    let mut pairs = good();
    pairs[1].1 = "lots".to_owned();
    let errors = decode::<NewDeal>(&pairs).unwrap_err();
    assert!(errors["amount"][0].contains("enter an amount"), "{errors:?}");
    let mut pairs = good();
    pairs[3].1 = "<script>".to_owned();
    let errors = decode::<NewDeal>(&pairs).unwrap_err();
    assert!(errors["stage"][0].contains("choose one"), "{errors:?}");
}

#[test]
fn a_rejected_form_keeps_what_was_typed_and_escapes_it() {
    let view = FormView::<NewDeal>::new()
        .post("/deals")
        .with_values(vec![("title".to_owned(), "\"><script>x</script>".to_owned())])
        .with_errors([("title".to_owned(), vec!["Bad <b>".to_owned()])].into());
    let html = render(&view.into());
    assert!(!html.contains("<script"), "{html}");
    assert!(html.contains("&lt;script&gt;"), "{html}");
    assert!(html.contains("Bad &lt;b&gt;"), "{html}");
    assert!(html.contains("data-invalid"), "{html}");
    assert!(html.contains(r#"aria-invalid="true""#), "{html}");
}

#[test]
fn test_view_drives_a_form_with_no_browser() {
    let view = View::new("New").body(FormView::<NewDeal>::new().post("/deals").submit("Create"));
    let page = TestView::of(&view);
    assert!(page.sees("Deal name"));
    assert_eq!(page.click("Create"), Clicked::Submit);
    let errors = page.clone().fill("title", "").fill("amount", "1").submit_as::<NewDeal>().unwrap_err();
    assert!(errors.contains_key("title"), "{errors:?}");
    let deal = page.fill("title", "Acme").fill("amount", "1").submit_as::<NewDeal>().unwrap();
    assert_eq!(deal.title, "Acme");
    assert_eq!(deal.stage, Stage::Lead, "a select starts on its first choice");
}

fn stage_tone(stage: &'static str) -> BadgeVariant {
    if stage == "won" { BadgeVariant::Success } else { BadgeVariant::Neutral }
}

fn tone_of(stage: &&'static str) -> BadgeVariant {
    stage_tone(stage)
}

#[derive(TableRow)]
struct DealRow {
    #[col(skip)]
    id: i64,
    #[col(sortable, link = "/deals/{id}")]
    title: String,
    #[col(badge = tone_of)]
    stage: &'static str,
    #[col(sortable, numeric)]
    amount: Money,
    #[col(hide_on_mobile, label = "Owner")]
    owner_name: String,
}

#[derive(Default, Deserialize, FilterBar)]
struct DealFilter {
    /// Search deals
    #[filter(search)]
    q: Option<String>,
    stage: Option<Stage>,
}

fn rows() -> Vec<DealRow> {
    vec![DealRow { id: 7, title: "A<cme".to_owned(), stage: "won", amount: Money(1234), owner_name: "Ada".to_owned() }]
}

#[test]
fn a_query_reads_sort_filter_and_page_and_forgives_bad_values() {
    let query = TableQuery::<DealRow, DealFilter>::parse("/deals", "sort=amount&dir=desc&page=3&per=25&q=ac&stage=closed_won");
    assert_eq!(query.sort(), Some(("amount", Direction::Desc)));
    assert_eq!(query.page(), 3);
    assert_eq!(query.per_page(), 25);
    assert_eq!(query.offset(), 50);
    assert_eq!(query.filter().q.as_deref(), Some("ac"));
    assert_eq!(query.filter().stage, Some(Stage::ClosedWon));

    let query = TableQuery::<DealRow, DealFilter>::parse("/deals", "sort=owner_name&page=-1&per=999999&stage=");
    assert_eq!(query.sort(), None, "only sortable columns sort");
    assert_eq!(query.page(), 1);
    assert_eq!(query.per_page(), 1000);
    let query = TableQuery::<DealRow, DealFilter>::parse("/deals", "stage=%3Cnope%3E");
    assert_eq!(query.filter().stage, None);
}

#[test]
fn a_table_is_a_region_with_links_that_keep_the_state() {
    let query = TableQuery::<DealRow, DealFilter>::parse("/deals", "sort=amount&q=a%20b&page=2");
    let rows = rows();
    let html = render(&DataTable::new("deals", &query, &rows).total(500).into());
    assert!(html.contains(r#"id="deals" data-ui-region"#), "{html}");
    assert!(html.contains(r#"<tr data-key="7">"#), "{html}");
    assert!(html.contains(r#"<a href="/deals/7">A&lt;cme</a>"#), "{html}");
    assert!(html.contains(r#"aria-sort="ascending""#), "{html}");
    assert!(html.contains(r#"href="/deals?sort=amount&amp;dir=desc&amp;q=a%20b""#), "the next click sorts down, from page 1: {html}");
    assert!(html.contains(r#"href="/deals?sort=amount&amp;q=a%20b&amp;page=3""#), "{html}");
    assert!(html.contains(r#"data-ui-target="deals""#), "{html}");
    assert!(html.contains(r#"data-numeric>12.34</td>"#), "{html}");
    assert!(html.contains(r#"data-variant="success""#), "{html}");
    assert!(html.contains("data-hide-mobile"), "{html}");
    assert!(html.contains("51–51 of 500"), "{html}");
}

#[test]
fn test_view_reads_table_rows() {
    let query = TableQuery::<DealRow, DealFilter>::parse("/deals", "");
    let rows = rows();
    let page = TestView::of_node(DataTable::new("deals", &query, &rows));
    assert!(page.sees("A<cme"));
    assert!(page.sees("12.34"));
}

#[test]
fn an_empty_table_says_so() {
    let query = TableQuery::<DealRow, DealFilter>::parse("/deals", "");
    let html = render(&DataTable::new("deals", &query, &[]).into());
    assert!(html.contains(r#"data-slot="empty""#), "{html}");
    assert!(!html.contains("<table"), "{html}");
}

#[test]
fn row_columns_come_from_the_struct() {
    assert_eq!(DealRow::COLUMNS.len(), 4);
    assert_eq!(DealRow::COLUMNS[3].label, "Owner");
    assert!(DealRow::COLUMNS[0].sortable);
}
