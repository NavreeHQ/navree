//! A form or a table that uses none of the options added after 1.7.0 draws what it drew
//! at 1.7.0, and answers the same requests the same way.
//!
//! The pins below were taken before a form could take a control of the app's own, a mono
//! field or a submit button's look, and before a table could mark the row a page shows or
//! take checkboxes, chips and defaults in its own filters: the markup of the repository's
//! example forms and tables (the docs' data tables, the proof app's deal form and deals
//! table, a form of every widget), and the links and parameters a table's own filters
//! write. A change to any of them is a change to every app that uses none of the new
//! options, so each is held here, byte for byte.

use navree_ui::form::{FormView, Many, Money};
use navree_ui::table::FilterBar as _;
use navree_ui::{
    BadgeVariant, BulkAction, DataTable, Empty, FilterBar, Form, Options, RowAction, TablePage,
    TableQuery, TableRow, Validate, icons, render,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

fn sha(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

// The docs' data table (`apps/ui-docs/src/examples/data.rs`), as it is there.

fn tone(stage: &&'static str) -> BadgeVariant {
    match *stage {
        "Won" => BadgeVariant::Success,
        "Negotiation" => BadgeVariant::Warning,
        _ => BadgeVariant::Neutral,
    }
}

#[derive(Clone, TableRow)]
#[table(sort = "-amount", per = 5, per_pages(5, 25, 100))]
struct Deal {
    #[col(skip)]
    id: i64,
    #[col(sortable, search, sticky, filter = text, link = "/deals/{id}", label = "Deal")]
    title: String,
    #[col(sortable, hide_on_mobile)]
    company: &'static str,
    #[col(badge = tone)]
    stage: &'static str,
    #[col(sortable, numeric, filter = range, label = "Amount (₹)")]
    amount: Money,
    #[col(sortable, filter = date, label = "Closes")]
    closes_on: navree_datetime::Date,
}

fn deals() -> Vec<Deal> {
    let day = |d| navree_datetime::Date::new(2026, 11, d).unwrap_or_default();
    [
        (
            1,
            "Sutlej Supply renewal",
            "Sutlej Supply",
            "Negotiation",
            4_800_000,
            day(30),
        ),
        (
            2,
            "Haveli Home expansion",
            "Haveli Home",
            "Won",
            1_250_000,
            day(4),
        ),
        (
            3,
            "Doaba Traders pilot",
            "Doaba Traders",
            "Lead",
            320_000,
            day(21),
        ),
        (
            4,
            "Phulkari Textiles platform",
            "Phulkari Textiles",
            "Proposal",
            2_100_000,
            day(18),
        ),
        (
            5,
            "Pind Pantry support",
            "Pind Pantry",
            "Qualified",
            640_000,
            day(12),
        ),
    ]
    .map(|(id, title, company, stage, amount, closes_on)| Deal {
        id,
        title: title.to_owned(),
        company,
        stage,
        amount: Money(amount),
        closes_on,
    })
    .to_vec()
}

/// Each of the docs' data tables, by its example's name.
fn docs_tables() -> Vec<(&'static str, String)> {
    let plain = TableQuery::<Deal>::parse("/deals", "");
    let rows: Vec<Deal> = deals().into_iter().take(2).collect();
    let page = TablePage::new(deals(), 128);
    let none: Vec<Deal> = Vec::new();
    let filtered = TableQuery::<Deal>::parse("/deals", "filter[amount][gte]=500000");
    let four = TablePage::new(deals(), 4);
    let compact = TableQuery::<Deal>::parse("/deals", "density=compact");
    let five = TablePage::new(deals(), 5);
    vec![
        (
            "thumb",
            render(&DataTable::new("deals", &plain, &rows).into()),
        ),
        (
            "default",
            render(
                &DataTable::new("deals", &plain, &page)
                    .caption("Deals")
                    .export("/deals/export")
                    .into(),
            ),
        ),
        (
            "bulk",
            render(
                &DataTable::new("deals", &plain, &page)
                    .caption("Deals")
                    .bulk(
                        "/deals/bulk",
                        [
                            BulkAction::new("archive", "Archive").icon(icons::ARCHIVE),
                            BulkAction::new("delete", "Delete")
                                .destructive()
                                .confirm("Delete {n} deals?"),
                        ],
                    )
                    .row_actions(|deal| {
                        vec![
                            RowAction::link("Open", format!("/deals/{}", deal.id)),
                            RowAction::post("Archive", format!("/deals/{}/archive", deal.id)),
                        ]
                    })
                    .into(),
            ),
        ),
        (
            "filter-row",
            render(
                &DataTable::new("deals", &filtered, &four)
                    .caption("Deals")
                    .filter_row()
                    .into(),
            ),
        ),
        (
            "compact",
            render(
                &DataTable::new("deals", &compact, &five)
                    .caption("Deals")
                    .into(),
            ),
        ),
        (
            "empty",
            render(
                &DataTable::new("empty", &plain, &none)
                    .caption("Deals")
                    .empty(Empty::new("No deals yet").icon(icons::INBOX))
                    .into(),
            ),
        ),
        (
            "loading",
            render(
                &DataTable::new("loading", &plain, &none)
                    .caption("Deals")
                    .loading()
                    .into(),
            ),
        ),
        (
            "failed",
            render(
                &DataTable::new("failed", &plain, &none)
                    .caption("Deals")
                    .failed("The deals couldn't be loaded. Try again in a minute.")
                    .into(),
            ),
        ),
    ]
}

// The proof app's deals table (`apps/ui-proof/src/deals.rs`): a page filter of its own.

#[derive(Clone, Copy, Debug, Deserialize, Options, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum StageFilter {
    Lead,
    Qualified,
    Proposal,
    Negotiation,
    Won,
}

#[derive(Clone, TableRow)]
#[table(per = 50, per_pages(25, 50, 100, 1000))]
struct DealRow {
    #[col(skip)]
    id: i64,
    #[col(sortable, link = "/deals/{id}", label = "Deal")]
    title: String,
    #[col(sortable, hide_on_mobile)]
    company: &'static str,
    #[col(badge = tone)]
    stage: &'static str,
    #[col(sortable, numeric)]
    amount: Money,
    #[col(hide_on_mobile, label = "Owner")]
    owner: &'static str,
    #[col(sortable, label = "Updated")]
    updated_at: navree_datetime::Date,
}

#[derive(Default, Deserialize, FilterBar)]
struct DealFilter {
    /// Search deals
    #[filter(search)]
    q: Option<String>,
    /// All stages
    stage: Option<StageFilter>,
}

fn deal_rows() -> Vec<DealRow> {
    let start = navree_datetime::Date::new(2026, 1, 1).unwrap_or_default();
    let companies = ["Agra", "Gulati", "Ilaichi", "Utsav", "Hariyali", "Sutlej"];
    let stages = ["Lead", "Qualified", "Proposal", "Negotiation", "Won"];
    let owners = [
        "Anu Lakhotia",
        "Arun Tiwari",
        "Gauri Hundal",
        "Esha Dhillon",
    ];
    (0..6)
        .map(|at| DealRow {
            id: at as i64 + 1,
            title: format!("{} renewal {}", companies[at % 6], at + 1),
            company: companies[at % 6],
            stage: stages[(at * 7) % 5],
            amount: Money(((at as i64) * 7919) % 5_000_000),
            owner: owners[at % 4],
            updated_at: start.add_days(at as i64).unwrap_or_default(),
        })
        .collect()
}

// A table whose columns filter in every way, in the bar and in the row.

#[derive(Clone, Copy, Debug, Deserialize, Options, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Stage {
    Lead,
    Won,
}

#[derive(Clone, TableRow)]
struct Lead {
    #[col(skip)]
    id: i64,
    #[col(sortable, search, filter = text)]
    name: String,
    #[col(filter = multi)]
    stage: Stage,
    #[col(filter = select, options = REGIONS)]
    region: String,
    #[col(filter = boolean)]
    hot: bool,
    #[col(filter = relation, label = "Owner")]
    owner_id: i64,
    #[col(numeric, filter = range)]
    seats: Option<i64>,
}

const REGIONS: &[(&str, &str)] = &[("north", "North"), ("south", "South")];

fn leads() -> Vec<Lead> {
    vec![
        Lead {
            id: 1,
            name: "Harpreet Sandhu".to_owned(),
            stage: Stage::Lead,
            region: "north".to_owned(),
            hot: true,
            owner_id: 7,
            seats: Some(12),
        },
        Lead {
            id: 2,
            name: "Meera Sharma".to_owned(),
            stage: Stage::Won,
            region: "south".to_owned(),
            hot: false,
            owner_id: 8,
            seats: None,
        },
    ]
}

fn owners() -> Vec<navree_ui::FilterOption> {
    vec![
        navree_ui::FilterOption::new(7, "Gauri Hundal").count(3),
        navree_ui::FilterOption::new(8, "Esha Dhillon"),
    ]
}

/// Tables with the page's filter, the bar of filters, the row of filters and the row options.
fn other_tables() -> Vec<(&'static str, String)> {
    let rows = deal_rows();
    let leads = leads();
    let query = TableQuery::<DealRow, DealFilter>::parse("/deals", "");
    let asked = TableQuery::<DealRow, DealFilter>::parse(
        "/deals",
        "q=renewal&stage=won&sort=-amount&page=1&per_page=25",
    );
    let bar = TableQuery::<Lead>::parse(
        "/leads",
        "filter[stage][in]=won&filter[hot]=true&filter[owner_id][in]=7&fb=stage,hot,owner_id",
    );
    let row = TableQuery::<Lead>::parse(
        "/leads",
        "filter[region]=north&filter[seats][gte]=2&filter[name][contains]=har&open=1",
    );
    vec![
        (
            "page filter",
            render(
                &DataTable::new("deals", &query, &rows)
                    .caption("Deals")
                    .into(),
            ),
        ),
        (
            "page filter asked",
            render(
                &DataTable::new("deals", &asked, &rows)
                    .caption("Deals")
                    .refreshable()
                    .into(),
            ),
        ),
        (
            "filter bar",
            render(
                &DataTable::new("leads", &bar, &leads)
                    .caption("Leads")
                    .filter_bar()
                    .options("owner_id", owners())
                    .into(),
            ),
        ),
        (
            "filter row and row options",
            render(
                &DataTable::new("leads", &row, &leads)
                    .caption("Leads")
                    .filter_row()
                    .options("owner_id", owners())
                    .row_href(|lead| format!("/leads/{}", lead.id))
                    .sticky_rows(|lead| lead.hot)
                    .detail(|lead| navree_ui::Node::from(lead.name.clone()))
                    .selectable_if(|lead| !lead.hot)
                    .bulk("/leads/bulk", [BulkAction::new("archive", "Archive")])
                    .sticky_header()
                    .keyboard_grid()
                    .into(),
            ),
        ),
    ]
}

/// What a table's own filters write and read: the parameters of the page's filter, and the
/// link of the state each query names.
fn answers() -> String {
    let mut out = String::new();
    for text in [
        "",
        "q=renewal",
        "stage=won",
        "q=a&stage=bogus",
        "stage=won&stage=lead",
        "q=a&q=b",
        "stage=",
        "sort=-amount&page=3&per_page=25&q=x&stage=lead&cols=title,amount&density=compact",
    ] {
        let query = TableQuery::<DealRow, DealFilter>::parse("/deals", text);
        out.push_str(&format!(
            "{text} => {:?} {:?} {} {}\n",
            query.filter().params(),
            query.filter().q,
            query.href(),
            query.is_filtered()
        ));
    }
    out
}

// The proof app's deal form (`apps/ui-proof/src/deal_form.rs`), and a form of every widget.

#[derive(Clone, Copy, Debug, Deserialize, Options)]
#[allow(
    clippy::upper_case_acronyms,
    reason = "currency codes are written in capitals"
)]
enum Currency {
    INR,
    USD,
}

#[derive(Clone, Copy, Debug, Deserialize, Options, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum DealStage {
    #[option(label = "Lead (10%)")]
    Lead,
    #[option(label = "Won (100%)")]
    Won,
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
    /// Company
    #[validate(length(min = 1))]
    #[form(widget = select, prompt = "Choose a company", error = "Choose the company this deal is with.")]
    company_id: String,
    /// Main contact
    #[validate(skip)]
    #[form(widget = select, prompt = "Choose a contact")]
    contact_id: Option<String>,
    /// Value
    #[validate(skip)]
    #[form(addon = "currency", placeholder = "12,500")]
    amount: Money,
    #[validate(skip)]
    currency: Currency,
    /// Expected close
    #[validate(skip)]
    expected_close_date: Option<navree_datetime::Date>,
    #[validate(skip)]
    stage: DealStage,
    /// Owner
    #[validate(skip)]
    #[form(widget = select, prompt = "Unassigned")]
    owner_id: Option<String>,
}

#[derive(Debug, Deserialize, Validate, Form)]
struct EveryWidget {
    /// Contact email
    #[validate(email)]
    #[form(help = "We write to it once.")]
    email: String,
    #[validate(length(min = 8))]
    password: String,
    #[validate(range(min = 0, max = 100))]
    probability: Option<i32>,
    #[form(widget = textarea, rows = 4)]
    #[validate(length(max = 2000))]
    notes: Option<String>,
    #[form(widget = radio)]
    #[validate(skip)]
    stage: DealStage,
    #[form(widget = listbox)]
    #[validate(skip)]
    region: DealStage,
    #[form(widget = combobox)]
    #[validate(skip)]
    owner: Option<String>,
    #[validate(skip)]
    stages: Many<DealStage>,
    #[validate(skip)]
    hot: bool,
    #[form(widget = switch)]
    #[validate(skip)]
    alerts: bool,
    #[form(widget = date_picker)]
    #[validate(skip)]
    due: Option<navree_datetime::Date>,
    #[form(widget = time_picker)]
    #[validate(skip)]
    at: Option<navree_datetime::Time>,
    #[form(widget = slider)]
    #[validate(range(min = 0, max = 10))]
    score: i32,
    #[form(widget = hidden)]
    #[validate(skip)]
    step: Option<String>,
}

fn choices(names: &[&str]) -> Vec<(String, String)> {
    names
        .iter()
        .enumerate()
        .map(|(id, name)| (id.to_string(), (*name).to_owned()))
        .collect()
}

fn deal_form(form: FormView<NewDeal>) -> String {
    render(
        &form
            .post("/deals")
            .submit("Create deal")
            .cancel("Cancel", "/deals?per_page=1000")
            .options("company_id", choices(&["Agra", "Gulati", "Ilaichi"]))
            .options("contact_id", choices(&["Anu Lakhotia", "Arun Tiwari"]))
            .options("owner_id", choices(&["Gauri Hundal", "Esha Dhillon"]))
            .into(),
    )
}

/// Each form, by name.
fn forms() -> Vec<(&'static str, String)> {
    let mut errors = BTreeMap::new();
    errors.insert("title".to_owned(), vec!["Give the deal a name.".to_owned()]);
    errors.insert(
        "_form".to_owned(),
        vec!["The deal could not be saved.".to_owned()],
    );
    let sent = vec![
        ("title".to_owned(), "<b>Renewal</b>".to_owned()),
        ("company_id".to_owned(), "9".to_owned()),
        ("amount".to_owned(), "12,500".to_owned()),
        ("currency".to_owned(), "USD".to_owned()),
        ("stage".to_owned(), "won".to_owned()),
    ];
    let every = vec![
        ("email".to_owned(), "anu@example.com".to_owned()),
        ("probability".to_owned(), "40".to_owned()),
        ("notes".to_owned(), "Call first".to_owned()),
        ("stage".to_owned(), "won".to_owned()),
        ("region".to_owned(), "lead".to_owned()),
        ("owner".to_owned(), "7".to_owned()),
        ("stages".to_owned(), "lead".to_owned()),
        ("stages".to_owned(), "won".to_owned()),
        ("hot".to_owned(), "true".to_owned()),
        ("due".to_owned(), "2026-11-30".to_owned()),
        ("at".to_owned(), "14:30".to_owned()),
        ("score".to_owned(), "4".to_owned()),
        ("step".to_owned(), "two".to_owned()),
    ];
    let mut every_errors = BTreeMap::new();
    every_errors.insert("email".to_owned(), vec!["Enter an email.".to_owned()]);
    every_errors.insert("hot".to_owned(), vec!["Say yes or no.".to_owned()]);
    every_errors.insert("stage".to_owned(), vec!["Choose a stage.".to_owned()]);
    vec![
        ("deal form", deal_form(FormView::new())),
        (
            "deal form refused",
            deal_form(FormView::new().with_values(sent).with_errors(errors)),
        ),
        (
            "every widget",
            render(&FormView::<EveryWidget>::new().post("/every").into()),
        ),
        (
            "every widget filled",
            render(
                &FormView::<EveryWidget>::new()
                    .post("/every")
                    .with_values(every)
                    .with_errors(every_errors)
                    .options("owner", choices(&["Gauri Hundal"]))
                    .search("owner", "/people/options")
                    .submit("Save it")
                    .one_column()
                    .id_prefix("edit")
                    .hidden("return", "/every")
                    .into(),
            ),
        ),
    ]
}

/// The SHA-256 of each table and form's markup, and of the table's answers, at 1.7.0.
const HELD: &[(&str, &str)] = &[
    (
        "thumb",
        "8c42f7d265adae9f794432c3b360dd5b844acf387a818df11955b73918e0b411",
    ),
    (
        "default",
        "397a9aacb5fb5dd8e1d8a1f9825e0b07eaafb295f9410b5ec180e1a391a2b173",
    ),
    (
        "bulk",
        "145a2588d920962cec1ac5a37da67f9b7cdebefbd0bdd9a830ac8284f4fe8e4d",
    ),
    (
        "filter-row",
        "94563c6e28b8873579e01ce4915d867e142518138035b00e7cba38aef5ec00a5",
    ),
    (
        "compact",
        "ae18a0aac51749ee062f716c9a7d85bbc8361c4f97d2c698830aa89bb321adf6",
    ),
    (
        "empty",
        "6d74c12ff73110262542ed09a473fd82d209e983752e42e5f35b90fd17f848b1",
    ),
    (
        "loading",
        "b09e311142f2703bd471c078a616711f0d92bda3cec56d953c8ef1fb26917dad",
    ),
    (
        "failed",
        "e2ff34ee382a939c376e1a6dba8549af784f31cb34ec0b196ce6ca69fa663403",
    ),
    (
        "page filter",
        "e2bbcf0aa05ddd2ab714412f4eaa3d0c1e395b37d37d6a6e48f4d3567288ab67",
    ),
    (
        "page filter asked",
        "f14f3cbceacd7149ea7a6c257f09cf821793b54ea7285f674e81b5eab5e82345",
    ),
    (
        "filter bar",
        "3b3f3d265e9c4e880bce71b8b385270ceaba15acd20b9c950f5c132fac5bb5af",
    ),
    (
        "filter row and row options",
        "5fcf99408b54ecb4e4e0fbdd52793c9466dc8c2585f3d7725a4384274889f07b",
    ),
    (
        "answers",
        "d50992de3b676f98d37f6bb1622b6f796806f3373e8cbf64285ed2ea36de23bb",
    ),
    (
        "deal form",
        "07209c1c318ef5994eaac3e9fccef79c367efa7906326bb31771b97f41c062ec",
    ),
    (
        "deal form refused",
        "e769fa601a12e953a321b68b57cc8af6de887c62eff99369c377e9cd2e0163b5",
    ),
    (
        "every widget",
        "7581559c52136bb16ff3d565424cf3afce98b41f2290e748e359b33b80024e56",
    ),
    (
        "every widget filled",
        "4a607783060bbf84820503e76965ac0cf3d0339264a955c3b6b7d6ccb5439dd7",
    ),
];

#[test]
fn a_form_or_a_table_with_none_of_the_new_options_writes_what_it_wrote_at_1_7_0() {
    let mut drawn = docs_tables();
    drawn.extend(other_tables());
    drawn.push(("answers", answers()));
    drawn.extend(forms());
    let got: Vec<(&str, String)> = drawn
        .iter()
        .map(|(name, html)| (*name, sha(html)))
        .collect();
    let want: Vec<(&str, String)> = HELD
        .iter()
        .map(|(name, hash)| (*name, (*hash).to_owned()))
        .collect();
    for ((name, html), ((_, got), (_, want))) in drawn.iter().zip(got.iter().zip(&want)) {
        assert_eq!(got, want, "{name}:\n{html}");
    }
    assert_eq!(got, want);
}
