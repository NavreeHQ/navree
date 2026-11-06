//! Select, Combobox, MultiSelect and the command palette in every state, on pages a
//! browser test drives: a gallery, one picker with ten thousand options, a form that
//! posts, and the endpoints a searched list asks.

use std::sync::{Arc, LazyLock};

use http::Method;
use http::request::Parts;
use navree::http::extract::Form;
use navree::http::response::IntoResponse;
use navree::http::{Controller, RouteDef};
use navree::kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree::ui::form::{FieldErrors, FormView, Many, Rejected};
use navree::ui::*;
use serde::Deserialize;

/// A little picture from this site.
const PICTURE: &str = navree::ui::assets::FAVICON_URL;

const FIRST: [&str; 20] = [
    "Anu",
    "Arun",
    "Gauri",
    "Esha",
    "Baljit",
    "Davinder",
    "Manpreet",
    "Laksh",
    "Rajveer",
    "Kuldeep",
    "Dilpreet",
    "Harleen",
    "Kavita",
    "Tarun",
    "Vikram",
    "Faqir",
    "Charanjit",
    "Ananya",
    "Jaspreet",
    "Simran",
];
const LAST: [&str; 20] = [
    "Lakhotia", "Tiwari", "Hundal", "Dhillon", "Bains", "Dua", "Grewal", "Toor", "Brar", "Khanna",
    "Gill", "Kaur", "Joshi", "Mann", "Chahal", "Bajwa", "Sandhu", "Sidhu", "Sharma", "Malhotra",
];
/// The people a search can find: the same on every run.
static PEOPLE: LazyLock<Vec<(String, String)>> = LazyLock::new(|| {
    let mut people: Vec<(String, String)> = (0..10_000)
        .map(|n| {
            (
                (n + 1).to_string(),
                format!("{} {} {}", FIRST[n % 20], LAST[(n / 20) % 20], n + 1),
            )
        })
        .collect();
    let odd = [
        "<img src=x onerror=alert(1)> Manjeet",
        "محمد الأحمد (Arabic)",
        "O'Brien & \"Sons\" <b>bold</b>",
        "100%_literal \\ backslash",
        "Émile Zoë Ünal",
        &"Very long name ".repeat(24),
    ];
    for (n, name) in odd.iter().enumerate() {
        people.push((format!("{}", 10_001 + n), (*name).to_owned()));
    }
    people
});

/// Every person a search can find, as choices: what a post's value is checked against.
static PEOPLE_CHOICES: LazyLock<Vec<Choice>> = LazyLock::new(|| {
    PEOPLE
        .iter()
        .map(|(id, name)| Choice::new(id.as_str(), name.as_str()))
        .collect()
});

fn stages() -> Vec<Choice> {
    vec![
        Choice::new("lead", "Lead"),
        Choice::new("qualified", "Qualified"),
        Choice::new("proposal", "Proposal"),
        Choice::new("won", "Won"),
        Choice::new("lost", "Lost"),
    ]
}

fn countries() -> Vec<Choice> {
    [
        ("ar", "Argentina"),
        ("au", "Australia"),
        ("at", "Austria"),
        ("be", "Belgium"),
        ("br", "Brazil"),
        ("ca", "Canada"),
        ("cl", "Chile"),
        ("cn", "China"),
        ("dk", "Denmark"),
        ("eg", "Egypt"),
        ("fi", "Finland"),
        ("fr", "France"),
        ("de", "Germany"),
        ("gr", "Greece"),
        ("in", "India"),
        ("id", "Indonesia"),
        ("ie", "Ireland"),
        ("it", "Italy"),
        ("jp", "Japan"),
        ("ke", "Kenya"),
        ("mx", "Mexico"),
        ("nl", "Netherlands"),
        ("nz", "New Zealand"),
        ("ng", "Nigeria"),
        ("no", "Norway"),
        ("pl", "Poland"),
        ("pt", "Portugal"),
        ("es", "Spain"),
        ("se", "Sweden"),
        ("ch", "Switzerland"),
        ("tr", "Türkiye"),
        ("gb", "United Kingdom"),
        ("us", "United States"),
    ]
    .into_iter()
    .map(|(v, l)| Choice::new(v, l).keywords([format!("code {v}")]))
    .collect()
}

fn section(id: &'static str, title: &'static str, content: Vec<Node>) -> Element {
    el("section")
        .attr("data-gallery", id)
        .children((Heading::h2(title), Grid::cols(2).gap(4).children(content)))
}

fn field(id: &'static str, label: &'static str, control: impl Into<Node>) -> Node {
    Field::new(id, label).control(control).into()
}

fn selects() -> Element {
    let long = "A very long option label that keeps going and going so that the control has to cut it with an ellipsis and the list has to wrap it ";
    section(
        "selects",
        "Select",
        vec![
            field(
                "f-select-basic",
                "Stage",
                Select::new("stage")
                    .id("select-basic")
                    .placeholder("Choose a stage")
                    .options(stages()),
            ),
            field(
                "f-select-selected",
                "Chosen",
                Select::new("chosen")
                    .id("select-selected")
                    .options(stages())
                    .selected("won"),
            ),
            field(
                "f-select-groups",
                "Time zone",
                Select::new("tz")
                    .id("select-groups")
                    .placeholder("Choose a zone")
                    .group(
                        "Europe",
                        [
                            ("london", "London"),
                            ("paris", "Paris"),
                            ("berlin", "Berlin"),
                        ],
                    )
                    .separator()
                    .group(
                        "Asia",
                        [
                            Choice::new("tokyo", "Tokyo"),
                            Choice::new("delhi", "Delhi").disabled(true),
                            Choice::new("seoul", "Seoul"),
                        ],
                    ),
            ),
            field(
                "f-select-rich",
                "Owner",
                Select::new("owner")
                    .id("select-rich")
                    .placeholder("Choose an owner")
                    .choice(
                        Choice::new("anu", "Anu Lakhotia")
                            .description("Engineering")
                            .avatar(PICTURE)
                            .selected_label("Anu"),
                    )
                    .choice(
                        Choice::new("arun", "Arun Tiwari")
                            .description("Research")
                            .icon(icons::CHECK),
                    )
                    .choice(Choice::new("gauri", "Gauri Hundal").description("Compilers")),
            ),
            field(
                "f-select-clearable",
                "Clearable",
                Select::new("clearable")
                    .id("select-clearable")
                    .placeholder("None")
                    .options(stages())
                    .selected("lead")
                    .clearable(),
            ),
            field(
                "f-select-xs",
                "Extra small",
                Select::new("xs")
                    .id("select-xs")
                    .size(Size::Xs)
                    .placeholder("Pick")
                    .options(stages()),
            ),
            field(
                "f-select-sm",
                "Small",
                Select::new("sm")
                    .id("select-sm")
                    .size(Size::Sm)
                    .placeholder("Pick")
                    .options(stages()),
            ),
            field(
                "f-select-lg",
                "Large",
                Select::new("lg")
                    .id("select-lg")
                    .size(Size::Lg)
                    .placeholder("Pick")
                    .options(stages()),
            ),
            Field::new("f-select-invalid", "Required, with an error")
                .error("Choose a stage.")
                .control(
                    Select::new("invalid")
                        .id("select-invalid")
                        .placeholder("Choose a stage")
                        .options(stages())
                        .invalid(true),
                )
                .into(),
            field(
                "f-select-disabled",
                "Disabled",
                Select::new("disabled")
                    .id("select-disabled")
                    .options(stages())
                    .selected("lead")
                    .disabled(true),
            ),
            field(
                "f-select-readonly",
                "Read only",
                Select::new("readonly")
                    .id("select-readonly")
                    .options(stages())
                    .selected("lead")
                    .readonly(),
            ),
            field(
                "f-select-empty",
                "No options",
                Select::new("none")
                    .id("select-empty")
                    .placeholder("Nothing here"),
            ),
            field(
                "f-select-one",
                "One option",
                Select::new("one")
                    .id("select-one")
                    .placeholder("Pick")
                    .option("only", "The only one"),
            ),
            field(
                "f-select-long",
                "Long and hostile text",
                Select::new("long")
                    .id("select-long")
                    .placeholder("Pick")
                    .option("a", long.repeat(3))
                    .option("b", "<img src=x onerror=alert(1)> \"quoted\" & 'single'")
                    .option("c", "محمد الأحمد — نص عربي طويل جدا لاختبار الاتجاه")
                    .option("d", "Ünïcödé ẞ 日本語 한국어 🙂")
                    .option("e", "short")
                    .selected("a"),
            ),
        ],
    )
}

fn comboboxes() -> Element {
    section(
        "comboboxes",
        "Combobox",
        vec![
            field(
                "f-combo-basic",
                "Country",
                Combobox::new("country")
                    .id("combo-basic")
                    .placeholder("Search countries")
                    .options(countries()),
            ),
            field(
                "f-combo-selected",
                "Chosen and clearable",
                Combobox::new("country2")
                    .id("combo-selected")
                    .placeholder("Search countries")
                    .options(countries())
                    .selected("in")
                    .clearable(),
            ),
            field(
                "f-combo-search",
                "Person (searched on the server)",
                Combobox::new("person")
                    .id("combo-search")
                    .placeholder("Search people")
                    .search("/select/people")
                    .selected_choice(Choice::new("3", "Gauri Hundal 3")),
            ),
            field(
                "f-combo-min",
                "Person (2 characters first)",
                Combobox::new("person2")
                    .id("combo-min")
                    .placeholder("Type two letters")
                    .search("/select/people")
                    .min_chars(2),
            ),
            field(
                "f-combo-free",
                "Tag (any text)",
                Combobox::new("tag")
                    .id("combo-free")
                    .placeholder("Pick or type")
                    .options(stages())
                    .creatable(),
            ),
            field(
                "f-combo-create",
                "Team (created on the server)",
                Combobox::new("team")
                    .id("combo-create")
                    .placeholder("Pick or create")
                    .options([("red", "Red team"), ("blue", "Blue team")])
                    .create_url("/select/teams"),
            ),
            field(
                "f-combo-denied",
                "Team (not allowed to create)",
                Combobox::new("team2")
                    .id("combo-denied")
                    .placeholder("Pick")
                    .options([("red", "Red team"), ("blue", "Blue team")])
                    .create_url("/select/teams")
                    .create_denied("Only admins can add teams."),
            ),
            Field::new("f-combo-invalid", "Invalid")
                .error("Choose a country.")
                .control(
                    Combobox::new("country3")
                        .id("combo-invalid")
                        .placeholder("Search countries")
                        .options(countries())
                        .invalid(true),
                )
                .into(),
            field(
                "f-combo-disabled",
                "Disabled",
                Combobox::new("country4")
                    .id("combo-disabled")
                    .options(countries())
                    .selected("fr")
                    .disabled(true),
            ),
            field(
                "f-combo-empty",
                "No options",
                Combobox::new("none")
                    .id("combo-empty")
                    .placeholder("Nothing here"),
            ),
            field(
                "f-combo-groups",
                "Grouped",
                Combobox::new("grouped")
                    .id("combo-groups")
                    .placeholder("Search")
                    .group("Europe", [("fr", "France"), ("de", "Germany")])
                    .group("Asia", [("jp", "Japan"), ("in", "India")]),
            ),
        ],
    )
}

fn multis() -> Element {
    section(
        "multis",
        "MultiSelect",
        vec![
            field(
                "f-multi-basic",
                "Countries",
                MultiSelect::new("countries")
                    .id("multi-basic")
                    .placeholder("Add countries")
                    .options(countries())
                    .selected_values(["fr", "de"]),
            ),
            field(
                "f-multi-max",
                "At most three",
                MultiSelect::new("three")
                    .id("multi-max")
                    .placeholder("Add up to three")
                    .options(countries())
                    .max(3)
                    .selected_values(["fr", "de"]),
            ),
            field(
                "f-multi-summary",
                "Summary after two",
                MultiSelect::new("summary")
                    .id("multi-summary")
                    .placeholder("Add")
                    .options(countries())
                    .summary_after(2)
                    .clearable()
                    .selected_values(["fr", "de", "jp"]),
            ),
            field(
                "f-multi-all",
                "With select all",
                MultiSelect::new("all")
                    .id("multi-all")
                    .placeholder("Add")
                    .options(stages())
                    .select_all(),
            ),
            field(
                "f-multi-search",
                "People (searched on the server)",
                MultiSelect::new("people")
                    .id("multi-search")
                    .placeholder("Search people")
                    .search("/select/people")
                    .selected_choices([Choice::new("1", "Anu Lakhotia 1")]),
            ),
            field(
                "f-multi-cap",
                "At most two, found on the server",
                MultiSelect::new("cap")
                    .id("multi-cap")
                    .placeholder("Search people")
                    .search("/select/people-off")
                    .create_url("/select/teams")
                    .max(2)
                    .selected_choices([Choice::new("1", "Anu Lakhotia 1")]),
            ),
            field(
                "f-multi-cap-free",
                "At most two, any text",
                MultiSelect::new("capfree")
                    .id("multi-cap-free")
                    .placeholder("Pick or type")
                    .options(stages())
                    .creatable()
                    .max(2)
                    .selected_values(["lead"]),
            ),
            field(
                "f-multi-create",
                "Tags (any text)",
                MultiSelect::new("tags")
                    .id("multi-create")
                    .placeholder("Pick or type")
                    .options(stages())
                    .creatable(),
            ),
            field(
                "f-multi-disabled",
                "Disabled",
                MultiSelect::new("md")
                    .id("multi-disabled")
                    .options(countries())
                    .selected_values(["fr", "de"])
                    .disabled(true),
            ),
        ],
    )
}

fn palette() -> Command {
    Command::new("palette")
        .placeholder("Type a command or search…")
        .recent(4)
        .csrf("palette-token")
        .source("/select/palette-search")
        .group(
            CommandGroup::new("Navigate")
                .item(
                    CommandItem::link("Deals", "/deals?per_page=25")
                        .icon(icons::HANDSHAKE)
                        .shortcut("alt+d")
                        .keywords(["pipeline", "sales"]),
                )
                .item(CommandItem::link("New deal", "/deals/new").icon(icons::CIRCLE_PLUS))
                .item(
                    CommandItem::link("Gallery", "/gallery")
                        .description("Every component in its states"),
                ),
        )
        .group(
            CommandGroup::new("Actions")
                .item(
                    CommandItem::run("Open the dialog", Cmd::open("pick-dialog"))
                        .shortcut("mod+shift+o"),
                )
                .item(CommandItem::post("Post an action", "/select/echo").not_recent())
                .item(CommandItem::disabled(
                    "Billing",
                    "Only owners can open billing.",
                )),
        )
}

fn palette_section() -> Element {
    section(
        "palette",
        "Command palette",
        vec![
            Command::trigger("palette", "Search…").into(),
            palette().into(),
            Dialog::new("pick-dialog", "Pickers in a dialog")
                .description("The popups sit above the dialog; Esc closes the popup first.")
                .body(
                    Stack::gap(3).children((
                        field(
                            "f-dlg-select",
                            "Stage",
                            Select::new("dlg_stage")
                                .id("dlg-select")
                                .placeholder("Choose a stage")
                                .options(stages()),
                        ),
                        field(
                            "f-dlg-combo",
                            "Country",
                            Combobox::new("dlg_country")
                                .id("dlg-combo")
                                .placeholder("Search countries")
                                .options(countries()),
                        ),
                        field(
                            "f-dlg-multi",
                            "Countries",
                            MultiSelect::new("dlg_countries")
                                .id("dlg-multi")
                                .placeholder("Add")
                                .options(countries()),
                        ),
                    )),
                )
                .footer(Button::new("Done").closes_dialog("pick-dialog"))
                .into(),
            Button::outline("Open the dialog")
                .opens_dialog("pick-dialog")
                .into(),
            ScrollArea::new("A short scrolling card")
                .children(
                    Stack::gap(8).children((
                        Text::new("Scroll down: the picker is at the bottom."),
                        field(
                            "f-scroll-combo",
                            "Country",
                            Combobox::new("scroll_country")
                                .id("scroll-combo")
                                .placeholder("Search countries")
                                .options(countries()),
                        ),
                    )),
                )
                .into(),
        ],
    )
}

fn gallery(shell: &AppShell) -> AppPage {
    shell
        .page("Select and Combobox")
        .body(Stack::gap(8).children((
            selects(),
            comboboxes(),
            multis(),
            palette_section(),
            Direction::rtl(section(
                "rtl",
                "Right to left",
                vec![
                field(
                    "f-rtl-select",
                    "المرحلة",
                    Select::new("rtl_stage")
                        .id("rtl-select")
                        .placeholder("اختر مرحلة")
                        .options(stages()),
                ),
                field(
                    "f-rtl-combo",
                    "البلد",
                    Combobox::new("rtl_country")
                        .id("rtl-combo")
                        .placeholder("ابحث")
                        .options(countries()),
                ),
                field(
                    "f-rtl-multi",
                    "البلدان",
                    MultiSelect::new("rtl_countries")
                        .id("rtl-multi")
                        .placeholder("أضف")
                        .options(countries())
                        .selected_values(["fr", "de"]),
                ),
            ],
            )),
        )))
}

/// One picker with `n` options, for timing.
fn big(shell: &AppShell, kind: &str, n: usize) -> AppPage {
    let choices = (0..n).map(|at| {
        Choice::new(
            format!("v{at}"),
            format!("Choice number {at} of the long list"),
        )
        .keywords([format!("k{at}")])
    });
    let control: Node = match kind {
        "select" => Select::new("big")
            .id("big")
            .placeholder("Choose")
            .options(choices)
            .into(),
        "multi" => MultiSelect::new("big")
            .id("big")
            .placeholder("Add")
            .options(choices)
            .into(),
        _ => Combobox::new("big")
            .id("big")
            .placeholder("Search")
            .options(choices)
            .into(),
    };
    shell
        .page("A long list")
        .body(Container::new().children(Field::new("f-big", "A long list").control(control)))
}

/// A form that posts, so a browser test sees what is sent.
fn form_page(shell: &AppShell) -> AppPage {
    shell.page("A form with pickers").body(
        Card::new().body(
            el("form")
                .slot("form")
                .attr("method", "post")
                .attr("action", "/select/echo")
                .attr("id", "pick-form")
                .children((
                    Field::new("f-form-stage", "Stage").required().control(
                        Select::new("stage")
                            .id("form-select")
                            .placeholder("Choose a stage")
                            .options(stages())
                            .required(),
                    ),
                    Field::new("f-form-owner", "Country").required().control(
                        Combobox::new("owner")
                            .id("form-combo")
                            .placeholder("Search countries")
                            .options(countries())
                            .required(),
                    ),
                    Field::new("f-form-person", "Person").control(
                        Combobox::new("person")
                            .id("form-person")
                            .placeholder("Search people")
                            .search("/select/people"),
                    ),
                    Field::new("f-form-tags", "Countries").control(
                        MultiSelect::new("countries")
                            .id("form-multi")
                            .placeholder("Add countries")
                            .options(countries()),
                    ),
                    Field::new("f-form-plain", "Plain text")
                        .control(Input::text("note").value("kept")),
                    Row::gap(2).children((
                        Button::primary("Send").submit(),
                        Button::outline("Reset").kind(ButtonKind::Reset),
                    )),
                )),
        ),
    )
}

/// A form drawn from a struct: the derive picks the listbox, the comboboxes and the
/// multi-select, and reads every value back.
#[derive(Debug, Deserialize, Validate, Form)]
#[form(id = "derived")]
struct Derived {
    /// Stage
    #[validate(length(min = 1))]
    #[form(widget = listbox, prompt = "Choose a stage", error = "Choose a stage.")]
    stage: String,
    /// Country
    #[validate(length(min = 1))]
    #[form(widget = combobox, placeholder = "Search countries", error = "Choose a country.")]
    owner: String,
    /// Person
    #[validate(skip)]
    #[form(widget = combobox, placeholder = "Search people")]
    person: Option<String>,
    /// Markets
    #[validate(skip)]
    #[form(max = 2)]
    countries: Many<String>,
    /// Note
    #[validate(skip)]
    note: Option<String>,
}

fn derived_page(shell: &AppShell, form: FormView<Derived>) -> AppPage {
    let pairs = |items: Vec<Choice>| -> Vec<(String, String)> {
        items
            .into_iter()
            .map(|choice| (choice.value().to_owned(), choice.label().to_owned()))
            .collect()
    };
    shell.page("A form from a struct").body(
        Card::new().body(
            form.post("/select/derive")
                .submit("Send")
                .options("stage", pairs(stages()))
                .options("owner", pairs(countries()))
                .options("countries", pairs(countries()))
                .search("person", "/select/people"),
        ),
    )
}

fn derived_store(
    shell: &AppShell,
    parts: &Parts,
    submit: Result<Submit<Derived>, Rejected<Derived>>,
) -> navree::http::response::Response {
    match submit {
        Ok(Submit(sent)) => {
            // A browser can send any value: each must be one of the options this page offers.
            let wrong = || vec!["Choose one of the options.".to_owned()];
            let mut errors = FieldErrors::new();
            if !Choice::all_among([sent.stage.as_str()], &stages()) {
                errors.insert("stage".to_owned(), wrong());
            }
            if !Choice::all_among([sent.owner.as_str()], &countries()) {
                errors.insert("owner".to_owned(), wrong());
            }
            if !Choice::all_among(sent.person.iter(), &PEOPLE_CHOICES) {
                errors.insert("person".to_owned(), wrong());
            }
            if !Choice::all_among(sent.countries.iter(), &countries()) {
                errors.insert("countries".to_owned(), wrong());
            }
            if !errors.is_empty() {
                return Rejected::after_decode(&sent, errors, parts)
                    .respond(|form| derived_page(shell, form));
            }
            let mut pairs = vec![
                ("stage".to_owned(), sent.stage),
                ("owner".to_owned(), sent.owner),
                ("person".to_owned(), sent.person.unwrap_or_default()),
                ("note".to_owned(), sent.note.unwrap_or_default()),
            ];
            pairs.extend(
                sent.countries
                    .0
                    .into_iter()
                    .map(|c| ("countries".to_owned(), c)),
            );
            echo(shell, &pairs).into_response()
        }
        Err(rejected) => rejected.respond(|form| derived_page(shell, form)),
    }
}

/// The answer to a search, as `ChoiceList`.
fn people(query: &OptionQuery) -> ChoiceList {
    people_list(query, "/select/people", false)
}

/// The same answer, from `path`, with the second option disabled when `off_second`: a
/// server that refuses one of its own options.
fn people_list(query: &OptionQuery, path: &str, off_second: bool) -> ChoiceList {
    let needle = query.text().to_lowercase();
    let mut found = PEOPLE
        .iter()
        .filter(|(_, name)| needle.is_empty() || name.to_lowercase().contains(&needle))
        .skip(usize::try_from(query.offset()).unwrap_or(usize::MAX));
    let page: Vec<&(String, String)> = found.by_ref().take(query.limit()).collect();
    let mut list = ChoiceList::new();
    for (at, (id, name)) in page.iter().enumerate() {
        list = list.choice(Choice::new(id.as_str(), name.as_str()).disabled(off_second && at == 1));
    }
    if found.next().is_some() {
        list = list.more(query.next_url(path, page.len()));
    }
    list
}

/// What the palette's server source answers: people, as links.
fn palette_search(query: &OptionQuery) -> ChoiceList {
    let needle = query.text().to_lowercase();
    let hits: Vec<Choice> = PEOPLE
        .iter()
        .filter(|(_, name)| !needle.is_empty() && name.to_lowercase().contains(&needle))
        .take(5)
        .map(|(id, name)| {
            Choice::new(id.as_str(), name.as_str()).link(format!("/plain?person={id}"))
        })
        .collect();
    if hits.is_empty() {
        ChoiceList::new()
    } else {
        ChoiceList::new().group("People", hits)
    }
}

/// Creates a team: answers the option.
fn create_team(pairs: &[(String, String)]) -> navree::http::response::Response {
    let name = pairs
        .iter()
        .find(|(k, _)| k == "name")
        .map_or("", |(_, v)| v.as_str())
        .trim();
    if name.is_empty() || name.eq_ignore_ascii_case("forbidden") {
        return http::StatusCode::UNPROCESSABLE_ENTITY.into_response();
    }
    ChoiceList::new()
        .option(
            format!("team-{}", name.to_lowercase().replace(' ', "-")),
            name,
        )
        .into_response()
}

/// A filter form: a select that changes what a region shows. With script the choice
/// fetches the region; without it the button submits.
fn filter_page(shell: &AppShell, pairs: &[(String, String)]) -> AppPage {
    let stage = pairs
        .iter()
        .find(|(name, _)| name == "stage")
        .map_or("", |(_, value)| value.as_str());
    shell.page("A filter").body(
        Card::new().body(
            Stack::gap(3).children((
                el("form")
                    .attr("method", "get")
                    .attr("action", "/select/filter")
                    .attr("data-ui-target", "filter-result")
                    .attr("id", "filter-form")
                    .children(
                        Row::gap(2).children((
                            Select::new("stage")
                                .id("filter-select")
                                .aria_label("Stage")
                                .placeholder("Any stage")
                                .options(stages())
                                .selected(stage.to_owned()),
                            Button::outline("Apply").submit(),
                        )),
                    ),
                el("p")
                    .attr("id", "filter-result")
                    .child(format!("Stage: {stage}")),
            )),
        ),
    )
}

/// The words of a page in French, for the runtime to use.
fn french() -> SelectLabels {
    SelectLabels {
        empty: "Aucun résultat".into(),
        clear: "Effacer".into(),
        results: "{n} résultats disponibles".into(),
        result: "1 résultat disponible".into(),
        create: "Créer « {q} »".into(),
        ..SelectLabels::default()
    }
}

fn french_page(shell: &AppShell) -> AppPage {
    shell.page("En français").body(
        Card::new().body(
            Stack::gap(3).children((Field::new("f-fr-combo", "Pays").control(
                Combobox::new("pays")
                    .id("fr-combo")
                    .placeholder("Chercher un pays")
                    .options(countries())
                    .creatable()
                    .clearable()
                    .selected("fr")
                    .labels(&french()),
            ),)),
        ),
    )
}

/// Shows what a form sent.
fn echo(shell: &AppShell, pairs: &[(String, String)]) -> AppPage {
    let items: Vec<Element> = pairs
        .iter()
        .map(|(name, value)| {
            el("li").children((
                el("code").child(name.clone()),
                " = ",
                el("code").child(value.clone()),
            ))
        })
        .collect();
    shell
        .page("What was sent")
        .body(Card::new().body(el("ul").attr("data-echo", "").child(items)))
}

pub struct SelectPages;

impl Injectable for SelectPages {
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Controller for SelectPages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "!/select", |shell: AppShell| async move {
                gallery(&shell)
            })
            .public(),
            RouteDef::on(
                Method::GET,
                "!/select/big-select",
                |shell: AppShell| async move { big(&shell, "select", 10_000) },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "!/select/big-combobox",
                |shell: AppShell| async move { big(&shell, "combobox", 10_000) },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "!/select/big-multi",
                |shell: AppShell| async move { big(&shell, "multi", 10_000) },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "!/select/filter",
                |shell: AppShell, query: navree::http::extract::Query<Vec<(String, String)>>| async move {
                    filter_page(&shell, &query.0)
                },
            )
            .public(),
            RouteDef::on(Method::GET, "!/select/derive", |shell: AppShell| async move {
                derived_page(&shell, FormView::new())
            })
            .public(),
            RouteDef::on(
                Method::POST,
                "!/select/derive",
                |shell: AppShell,
                 parts: Parts,
                 sent: Result<Submit<Derived>, Rejected<Derived>>| async move {
                    derived_store(&shell, &parts, sent)
                },
            )
            .public(),
            RouteDef::on(Method::GET, "!/select/fr", |shell: AppShell| async move {
                french_page(&shell)
            })
            .public(),
            RouteDef::on(Method::GET, "!/select/form", |shell: AppShell| async move {
                form_page(&shell)
            })
            .public(),
            RouteDef::on(
                Method::POST,
                "!/select/echo",
                |shell: AppShell, form: Form<Vec<(String, String)>>| async move {
                    echo(&shell, &form.0)
                },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "!/select/people",
                |query: OptionQuery| async move {
                    // What an ended session does to a search: it goes to the sign-in page.
                    if query.text() == "session-ended" {
                        return (
                            http::StatusCode::SEE_OTHER,
                            [(http::header::LOCATION, "/plain")],
                        )
                            .into_response();
                    }
                    people(&query).into_response()
                },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "!/select/people-off",
                |query: OptionQuery| async move {
                    people_list(&query, "/select/people-off", true).into_response()
                },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "!/select/palette-search",
                |query: OptionQuery| async move { palette_search(&query) },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "!/select/teams",
                |form: Form<Vec<(String, String)>>| async move { create_team(&form.0) },
            )
            .public(),
        ]
    }
}
