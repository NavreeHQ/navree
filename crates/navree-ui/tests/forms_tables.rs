//! The derives, end to end: a validated struct draws its form and reads it
//! back; a row struct draws its table with sort, filter and page in the URL.

use navree_ui::form::{Form, FormView, Many, Money, Widget, decode};
use navree_ui::table::{Direction, TableRow};
use navree_ui::test_view::Clicked;
use navree_ui::{
    BadgeVariant, Choice, DataTable, FilterBar, Form, InputKind, Options, TableQuery, TableRow,
    TestView, Validate, View, render,
};
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
    #[form(
        placeholder = "Annual platform license",
        autofocus,
        wide,
        error = "Give the deal a name."
    )]
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
#[allow(
    clippy::upper_case_acronyms,
    reason = "a name in capitals is what the test is about"
)]
enum Currency {
    USD,
    EUR,
}

fn html() -> String {
    render(
        &FormView::<NewDeal>::new()
            .post("/deals")
            .submit("Create deal")
            .into(),
    )
}

#[test]
fn the_type_picks_the_widget_and_the_doc_comment_the_label() {
    let fields = NewDeal::FIELDS;
    assert_eq!(fields[0].label, "Deal name");
    assert_eq!(fields[0].widget, Widget::Input(InputKind::Text));
    assert_eq!(fields[1].widget, Widget::Money);
    assert_eq!(fields[3].widget, Widget::Select);
    assert_eq!(
        fields[3].options,
        [("lead", "Lead"), ("closed_won", "Closed won")]
    );
    assert_eq!(fields[4].widget, Widget::Input(InputKind::Date));
    assert!(fields[4].optional);
    assert_eq!(fields[5].label, "Contact email");
    assert_eq!(
        fields[2].options,
        [("USD", "USD"), ("EUR", "EUR")],
        "a name in capitals keeps them"
    );
    assert_eq!(fields[8].widget, Widget::Checkbox);
}

#[test]
fn rules_become_native_constraints() {
    let html = html();
    assert!(
        html.contains(r#"name="title" value"#) || html.contains(r#"name="title""#),
        "{html}"
    );
    assert!(html.contains(r#"placeholder="Annual platform license" required autofocus minlength="1" maxlength="255""#), "{html}");
    assert!(
        html.contains(r#"type="email" name="contact_email""#),
        "{html}"
    );
    assert!(html.contains(r#"min="0" max="100""#), "{html}");
    assert!(html.contains(r#"maxlength="2000""#), "{html}");
}

#[test]
fn labels_tie_to_their_controls_and_addons_sit_in_the_group() {
    let html = html();
    assert!(html.contains(r#"for="new-deal-title""#), "{html}");
    assert!(html.contains(r#"id="new-deal-title""#), "{html}");
    assert!(
        html.contains(
            r#"<form data-slot="form" id="new-deal" method="post" action="/deals" data-ui-form>"#
        ),
        "{html}"
    );
    assert!(html.contains(r#"data-slot="input-group""#), "{html}");
    assert!(html.contains(r#"aria-label="Currency""#), "{html}");
    assert_eq!(
        html.matches(r#"for="new-deal-currency""#).count(),
        0,
        "{html}"
    );
    assert!(
        html.contains(r#"data-slot="button" data-variant="primary" type="submit">Create deal"#),
        "{html}"
    );
}

fn good() -> Vec<(String, String)> {
    [
        ("title", "Agra"),
        ("amount", "12,500.50"),
        ("currency", "USD"),
        ("stage", "closed_won"),
        ("close_date", ""),
        ("contact_email", ""),
        ("probability", ""),
        ("notes", ""),
    ]
    .iter()
    .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
    .collect()
}

#[test]
fn a_good_submission_decodes() {
    let deal: NewDeal = decode(&good()).unwrap();
    assert_eq!(deal.title, "Agra");
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
    assert!(
        errors["contact_email"][0].contains("Contact email"),
        "{errors:?}"
    );
}

#[test]
fn a_value_of_the_wrong_type_is_a_field_error_not_a_crash() {
    let mut pairs = good();
    pairs[1].1 = "lots".to_owned();
    let errors = decode::<NewDeal>(&pairs).unwrap_err();
    assert!(
        errors["amount"][0].contains("enter an amount"),
        "{errors:?}"
    );
    let mut pairs = good();
    pairs[3].1 = "<script>".to_owned();
    let errors = decode::<NewDeal>(&pairs).unwrap_err();
    assert!(errors["stage"][0].contains("choose one"), "{errors:?}");
}

#[test]
fn every_value_of_the_wrong_type_is_named_at_once() {
    let mut pairs = good();
    pairs[1].1 = "lots".to_owned();
    pairs[3].1 = "<script>".to_owned();
    pairs.push(("close_date".to_owned(), "someday".to_owned()));
    pairs.push(("probability".to_owned(), "high".to_owned()));
    let errors = decode::<NewDeal>(&pairs).unwrap_err();
    let names: Vec<&str> = errors.keys().map(String::as_str).collect();
    assert_eq!(
        names,
        ["amount", "close_date", "probability", "stage"],
        "{errors:?}"
    );
    assert!(
        errors["amount"][0].contains("enter an amount"),
        "{errors:?}"
    );
    assert!(
        errors["close_date"][0].contains("enter a date"),
        "{errors:?}"
    );
    assert!(
        errors["probability"][0].contains("enter a number"),
        "{errors:?}"
    );
    assert!(errors["stage"][0].contains("choose one"), "{errors:?}");
    // One bad value alone still reports one field, and a second pass is the same.
    let mut one = good();
    one[1].1 = "lots".to_owned();
    assert_eq!(decode::<NewDeal>(&one).unwrap_err().len(), 1);
    // Repeating a field cannot loop or hide the others.
    let mut twice = good();
    twice[1].1 = "lots".to_owned();
    twice.push(("amount".to_owned(), "more".to_owned()));
    twice[3].1 = "x".to_owned();
    let errors = decode::<NewDeal>(&twice).unwrap_err();
    assert_eq!(errors.len(), 2, "{errors:?}");
}

#[test]
fn a_rejected_form_keeps_what_was_typed_and_escapes_it() {
    let view = FormView::<NewDeal>::new()
        .post("/deals")
        .with_values(vec![(
            "title".to_owned(),
            "\"><script>x</script>".to_owned(),
        )])
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
    let errors = page
        .clone()
        .fill("title", "")
        .fill("amount", "1")
        .submit_as::<NewDeal>()
        .unwrap_err();
    assert!(errors.contains_key("title"), "{errors:?}");
    let deal = page
        .fill("title", "Agra")
        .fill("amount", "1")
        .submit_as::<NewDeal>()
        .unwrap();
    assert_eq!(deal.title, "Agra");
    assert_eq!(
        deal.stage,
        Stage::Lead,
        "a select starts on its first choice"
    );
}

fn stage_tone(stage: &'static str) -> BadgeVariant {
    if stage == "won" {
        BadgeVariant::Success
    } else {
        BadgeVariant::Neutral
    }
}

fn tone_of(stage: &&'static str) -> BadgeVariant {
    stage_tone(stage)
}

#[derive(TableRow)]
#[table(per = 50, per_pages(25, 50, 1000))]
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
    vec![DealRow {
        id: 7,
        title: "A<cme".to_owned(),
        stage: "won",
        amount: Money(1234),
        owner_name: "Anu".to_owned(),
    }]
}

#[test]
fn a_query_reads_sort_filter_and_page_and_forgives_bad_values() {
    let query = TableQuery::<DealRow, DealFilter>::parse(
        "/deals",
        "sort=-amount&page=3&per_page=25&q=ac&stage=closed_won",
    );
    assert_eq!(query.sort(), Some(("amount", Direction::Desc)));
    assert_eq!(query.page(), 3);
    assert_eq!(query.per_page(), 25);
    assert_eq!(query.offset(), 50);
    assert_eq!(query.filter().q.as_deref(), Some("ac"));
    assert_eq!(query.filter().stage, Some(Stage::ClosedWon));

    let query = TableQuery::<DealRow, DealFilter>::parse(
        "/deals",
        "sort=owner_name&page=-1&per_page=999999&stage=",
    );
    assert_eq!(query.sort(), None, "only sortable columns sort");
    assert_eq!(query.page(), 1);
    assert_eq!(query.per_page(), 50, "a page size not on offer is ignored");
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
    assert!(
        html.contains(r#"<a href="/deals/7"><bdi>A&lt;cme</bdi></a>"#),
        "{html}"
    );
    assert!(html.contains(r#"aria-sort="ascending""#), "{html}");
    assert!(
        html.contains(r#"href="/deals?q=a+b&amp;sort=-amount""#),
        "the next click sorts down, from page 1: {html}"
    );
    assert!(
        html.contains(r#"href="/deals?q=a+b&amp;sort=amount&amp;page=3""#),
        "{html}"
    );
    assert!(html.contains(r#"data-ui-target="deals""#), "{html}");
    assert!(html.contains(r#"data-numeric>12.34</td>"#), "{html}");
    assert!(html.contains(r#"data-variant="success""#), "{html}");
    assert!(html.contains("data-hide-mobile"), "{html}");
    assert!(html.contains("Showing 51 to 51 of 500 results"), "{html}");
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
    assert!(
        html.contains("<thead"),
        "the headers stay, so nothing jumps: {html}"
    );
}

#[test]
fn row_columns_come_from_the_struct() {
    assert_eq!(DealRow::COLUMNS.len(), 4);
    assert_eq!(DealRow::COLUMNS[3].label, "Owner");
    assert!(DealRow::COLUMNS[0].sortable);
}

#[test]
fn a_form_carries_the_csrf_token_first_and_escaped() {
    let html = render(
        &FormView::<NewDeal>::new()
            .post("/deals")
            .csrf("tok\"><script>")
            .into(),
    );
    let at = html
        .find(r#"<input type="hidden" name="_csrf" value="tok&quot;&gt;&lt;script&gt;">"#)
        .expect(&html);
    assert!(
        at < html
            .find("<div data-slot=\"form-grid\"")
            .unwrap_or(usize::MAX),
        "{html}"
    );
    assert!(!html.contains("<script>"), "{html}");
    // Without a token there is no field.
    assert!(!render(&FormView::<NewDeal>::new().post("/deals").into()).contains("_csrf"));
}

#[derive(Debug, Deserialize, Validate, Form)]
struct Pick {
    #[validate(length(min = 1))]
    #[form(widget = listbox, prompt = "Choose a stage", error = "Choose a stage.")]
    stage: String,
    #[validate(skip)]
    #[form(widget = combobox, placeholder = "Search people")]
    owner: Option<String>,
    #[validate(skip)]
    tags: Many<String>,
    #[validate(skip)]
    stages: Many<Stage>,
}

#[test]
fn a_form_can_pick_with_a_listbox_a_combobox_and_a_multi_select() {
    assert_eq!(Pick::FIELDS[0].widget, Widget::Listbox);
    assert_eq!(Pick::FIELDS[1].widget, Widget::Combobox);
    assert_eq!(
        Pick::FIELDS[2].widget,
        Widget::Multi,
        "a Many is a multi-select"
    );
    assert_eq!(
        Pick::FIELDS[3].options.len(),
        2,
        "a list of an enum takes the enum's options"
    );
    let value = Pick {
        stage: "won".to_owned(),
        owner: Some("7".to_owned()),
        tags: vec!["a".to_owned(), "b".to_owned()].into(),
        stages: vec![Stage::ClosedWon].into(),
    };
    let out = render(
        &FormView::<Pick>::new()
            .post("/pick")
            .of(&value)
            .options("stage", [("won".to_owned(), "Won".to_owned())])
            .options("owner", [("7".to_owned(), "Gauri Hundal".to_owned())])
            .options(
                "tags",
                [
                    ("a".to_owned(), "Alpha".to_owned()),
                    ("b".to_owned(), "Beta".to_owned()),
                ],
            )
            .search("owner", "/people/options")
            .into(),
    );
    assert!(out.contains(r#"data-slot="select-trigger""#), "{out}");
    assert!(
        out.contains(">Won</span>"),
        "the listbox shows the chosen label"
    );
    assert!(
        out.contains(r#"value="Gauri Hundal""#),
        "the combobox shows the label of the chosen one"
    );
    assert!(out.contains(r#"data-ui-search="/people/options""#));
    // Three chips in all (two tags, one stage) and a template in each of the two lists.
    assert_eq!(out.matches(r#"aria-label="Remove "#).count(), 5, "{out}");
    assert!(out.contains(r#"<option value="a" selected>Alpha</option>"#));
    assert!(
        out.contains(r#"<option value="closed_won" selected>Closed won</option>"#),
        "{out}"
    );
    assert!(
        out.contains(r#"for="pick-stage""#),
        "the label points at the trigger"
    );
    assert!(out.contains(r#"id="pick-stage""#));
}

#[test]
fn a_picker_field_can_take_rich_choices_with_a_picture_and_words_to_find_it_by() {
    let value = Pick {
        stage: "won".to_owned(),
        owner: Some("7".to_owned()),
        tags: Many::default(),
        stages: Many::default(),
    };
    let out = render(
        &FormView::<Pick>::new()
            .post("/pick")
            .of(&value)
            .options("owner", [("1".to_owned(), "Plain".to_owned())])
            .choices(
                "owner",
                [
                    Choice::new("7", "Gauri Hundal")
                        .avatar("/people/7.svg")
                        .keywords(["navy"]),
                    Choice::new("8", "Anu Lakhotia").description("Analyst"),
                ],
            )
            .into(),
    );
    assert!(
        out.contains(r#"src="/people/7.svg""#),
        "the picture is drawn: {out}"
    );
    assert!(out.contains(r#"data-keywords="navy""#), "{out}");
    assert!(
        out.contains(r#"value="Gauri Hundal""#),
        "the chosen label shows"
    );
    assert!(
        !out.contains(">Plain<"),
        "rich choices stand in for plain options"
    );
}

#[test]
fn a_form_reads_back_what_the_pickers_post_and_a_multi_select_with_nothing_posts_nothing() {
    let pairs = |list: &[(&str, &str)]| -> Vec<(String, String)> {
        list.iter()
            .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
            .collect()
    };
    let got: Pick = decode(&pairs(&[
        ("stage", "won"),
        ("tags", "x"),
        ("tags", "y"),
        ("stages", "lead"),
        ("owner", ""),
    ]))
    .unwrap();
    assert_eq!(got.tags.0, ["x", "y"]);
    assert_eq!(got.stages.0, [Stage::Lead]);
    assert_eq!(got.owner, None, "an empty optional is none");
    let none: Pick = decode(&pairs(&[("stage", "won")])).unwrap();
    assert!(none.tags.is_empty() && none.stages.is_empty());
    let errors = decode::<Pick>(&pairs(&[("stage", "")])).unwrap_err();
    assert_eq!(errors["stage"], ["Choose a stage."]);
    let wrong = decode::<Pick>(&pairs(&[("stage", "won"), ("stages", "nonsense")])).unwrap_err();
    assert!(wrong.contains_key("stages"), "{wrong:?}");
}

#[derive(Debug, Deserialize, Validate, Form)]
struct Capped {
    #[validate(skip)]
    #[form(max = 2)]
    tags: Many<String>,
}

#[test]
fn a_post_with_more_values_than_a_multi_select_allows_is_refused_with_script_off_too() {
    let pairs = |list: &[(&str, &str)]| -> Vec<(String, String)> {
        list.iter()
            .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
            .collect()
    };
    assert_eq!(Capped::FIELDS[0].max, Some(2));
    let two: Capped = decode(&pairs(&[("tags", "a"), ("tags", "b")])).unwrap();
    assert_eq!(two.tags.0, ["a", "b"]);
    let errors =
        decode::<Capped>(&pairs(&[("tags", "a"), ("tags", "b"), ("tags", "c")])).unwrap_err();
    assert_eq!(errors["tags"], ["Choose at most 2."], "{errors:?}");
    // Empty values and values with the separator are not counted: they are not read.
    let counted: Capped = decode(&pairs(&[
        ("tags", "a"),
        ("tags", ""),
        ("tags", "b"),
        ("tags", "x\u{1f}y"),
    ]))
    .unwrap();
    assert_eq!(counted.tags.0, ["a", "b"]);
    // The browser is told too.
    let out = render(&FormView::<Capped>::new().post("/t").into());
    assert!(out.contains(r#"data-ui-max="2""#), "{out}");
}

#[test]
fn a_refusal_after_decoding_keeps_what_was_sent_and_shows_the_errors() {
    use navree_ui::form::{FieldErrors, Rejected};
    let value = Pick {
        stage: "won".to_owned(),
        owner: Some("7".to_owned()),
        tags: vec!["a".to_owned()].into(),
        stages: Vec::new().into(),
    };
    let errors =
        FieldErrors::from([("owner".to_owned(), vec!["Choose from the list.".to_owned()])]);
    let (parts, ()) = http::Request::builder()
        .method("POST")
        .uri("/pick?x=1")
        .body(())
        .unwrap()
        .into_parts();
    let refused = Rejected::<Pick>::after_decode(&value, errors.clone(), &parts);
    assert_eq!(refused.errors()["owner"], ["Choose from the list."]);
    // Drawn as a page it is a 422 holding what was sent and the message.
    let page = refused.respond(|form| {
        View::new("Pick").body(navree_ui::Node::from(
            form.options("owner", [("7".to_owned(), "Gauri".to_owned())]),
        ))
    });
    assert_eq!(page.status(), http::StatusCode::UNPROCESSABLE_ENTITY);
    // From the runtime it is ops, also a 422.
    let (parts, ()) = http::Request::builder()
        .method("POST")
        .uri("/pick")
        .header("x-navree-ui", "1")
        .body(())
        .unwrap()
        .into_parts();
    let ops = Rejected::<Pick>::after_decode(&value, errors, &parts)
        .respond(|form| View::new("Pick").body(navree_ui::Node::from(form)));
    assert_eq!(ops.status(), http::StatusCode::UNPROCESSABLE_ENTITY);
    let kind = ops
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    assert!(kind.contains("ops+json"), "{kind}");
}

#[derive(Debug, Deserialize, Validate, Form)]
struct Handles {
    /// Handle
    #[validate(pattern("^[a-z_]+$", browser))]
    handle: String,
    /// Secret code
    #[validate(pattern("^SECRET-[0-9]{4}$"))]
    code: String,
}

/// A pattern reaches the page only when its rule says `browser`: the browser checks
/// that one before anything is sent, and the other stays on the server.
#[test]
fn only_a_pattern_marked_browser_reaches_the_page() {
    let html = render(&FormView::<Handles>::new().post("/handles").into());
    assert!(html.contains(r#"pattern="^[a-z_]+$""#), "{html}");
    assert!(!html.contains("SECRET"), "{html}");
    assert_eq!(html.matches("pattern=").count(), 1, "{html}");
}

/// A refusal from a real post, as `Result<Submit<T>, Rejected<T>>` hands it to a handler.
async fn refused<T>(body: &str) -> navree_ui::form::Rejected<T>
where
    T: Form + serde::de::DeserializeOwned + Validate + Send + 'static,
{
    use navree_http::extract::FromRequest;
    let request = http::Request::builder()
        .method("POST")
        .uri("/pick")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(navree_http::body::Body::from(body.to_owned()))
        .unwrap();
    match navree_ui::Submit::<T>::from_request(request).await {
        Err(rejected) => rejected,
        Ok(_) => panic!("the post was meant to be refused"),
    }
}

#[tokio::test]
async fn a_handler_reads_what_was_posted_from_a_refusal() {
    // No stage was chosen, so the form is refused; the person did pick an owner.
    let rejected = refused::<Pick>("stage=&owner=7&tags=a&tags=b%20c").await;
    assert!(rejected.errors().contains_key("stage"));
    assert_eq!(rejected.value("owner"), Some("7"));
    assert_eq!(
        rejected.value("stage"),
        Some(""),
        "an empty field was posted"
    );
    assert_eq!(rejected.value("nothing"), None);
    assert_eq!(rejected.values_of("tags"), ["a", "b c"]);
    assert!(rejected.values_of("nothing").is_empty());
    let names: Vec<&str> = rejected.values().map(|(name, _)| name).collect();
    assert_eq!(names, ["stage", "owner", "tags", "tags"]);
    // A field the form does not have is not there to read.
    let rejected = refused::<Pick>("stage=&owner=7&admin=1").await;
    assert_eq!(rejected.value("admin"), None);
}

#[tokio::test]
async fn a_refusal_after_decoding_has_the_same_values_to_read() {
    use navree_ui::form::{FieldErrors, Rejected};
    let value = Pick {
        stage: "won".to_owned(),
        owner: Some("7".to_owned()),
        tags: vec!["a".to_owned(), "b".to_owned()].into(),
        stages: Vec::new().into(),
    };
    let (parts, ()) = http::Request::builder()
        .method("POST")
        .uri("/pick")
        .body(())
        .unwrap()
        .into_parts();
    let refused = Rejected::<Pick>::after_decode(&value, FieldErrors::new(), &parts);
    assert_eq!(refused.value("owner"), Some("7"));
    assert_eq!(refused.values_of("tags"), ["a", "b"]);
}

#[derive(Debug, Deserialize, Validate, Form)]
struct SignUp {
    #[validate(length(min = 1))]
    email: String,
    #[validate(length(min = 8))]
    password: String,
}

#[tokio::test]
async fn a_password_is_never_among_the_values_a_refusal_gives_back() {
    let rejected = refused::<SignUp>("email=a%40b.example&password=hunter2").await;
    assert_eq!(rejected.value("email"), Some("a@b.example"));
    assert_eq!(rejected.value("password"), None);
    assert!(rejected.values().all(|(name, _)| name != "password"));
}

#[tokio::test]
async fn a_refused_form_shows_the_label_of_the_option_that_was_picked() {
    // Without the label the combobox can only show what it has: the posted id.
    let rejected = refused::<Pick>("stage=&owner=7").await;
    let bare = response_text(rejected).await;
    assert!(!bare.contains("Gauri Hundal"), "{bare}");
    assert!(
        bare.contains(r#"value="7" placeholder="Search people""#),
        "the input shows the id: {bare}"
    );
    // The handler looks the person up from the posted id and hands the label over.
    let rejected = refused::<Pick>("stage=&owner=7").await;
    let owner = rejected.value("owner").unwrap().to_owned();
    let rejected = rejected.options("owner", [(owner, "Gauri Hundal".to_owned())]);
    let html = response_text(rejected).await;
    assert!(
        html.contains(r#"value="Gauri Hundal" placeholder="Search people""#),
        "the input shows the label: {html}"
    );
    assert!(
        html.contains(r#"<option value="7" selected>Gauri Hundal</option>"#),
        "{html}"
    );
    // The error is still there, and so is the status.
    assert!(html.contains("Choose a stage."), "{html}");
}

#[tokio::test]
async fn a_refused_form_takes_rich_choices_too_and_a_custom_page_still_gets_the_form() {
    use navree_ui::Choice;
    let rejected = refused::<Pick>("stage=&owner=7").await.choices(
        "owner",
        [Choice::new("7", "Gauri Hundal").description("Admiral")],
    );
    let page =
        rejected.respond(|form| View::new("Pick").body(navree_ui::Node::from(form.one_column())));
    assert_eq!(page.status(), http::StatusCode::UNPROCESSABLE_ENTITY);
    let html = body_text(page).await;
    assert!(html.contains("Gauri Hundal"), "{html}");
    assert!(html.contains("Admiral"), "{html}");
}

async fn body_text(response: navree_http::response::Response) -> String {
    use http_body_util::BodyExt;
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

async fn response_text(rejected: navree_ui::form::Rejected<Pick>) -> String {
    use navree_http::response::IntoResponse;
    let response = rejected.into_response();
    assert_eq!(response.status(), http::StatusCode::UNPROCESSABLE_ENTITY);
    body_text(response).await
}
