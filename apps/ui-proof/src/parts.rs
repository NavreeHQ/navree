//! The parts the parts module and its stylesheet serve, in the states the browser suite runs
//! against.
//!
//! | Page | Shows |
//! |---|---|
//! | `/parts` | an alert with a close button and actions, boxes with a description and an indeterminate one, a folded breadcrumb, inputs with a clear button, the colour picker, an avatar group, a trace, a questionnaire, a select with flags, flags drawn by pointer, and a sparkline in each row of a table |

use std::sync::Arc;

use http::Method;
use navree::http::{Controller, RouteDef};
use navree::kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree::ui::charts::Sparkline;
use navree::ui::flags;
use navree::ui::*;

fn section(id: &'static str, title: &'static str, content: impl IntoNodes) -> Element {
    el("section")
        .attr("id", id)
        .children((Heading::h2(title), Stack::gap(4).children(content)))
}

fn alert() -> Alert {
    Alert::new(AlertTone::Warning)
        .title("Your trial ends in 3 days")
        .body("Add a payment method to keep your workspace.")
        .actions((
            Button::primary("Add payment").href("/parts"),
            Button::ghost("Remind me later"),
        ))
        .dismissible("trial-notice", "Dismiss this notice")
}

fn boxes() -> Stack {
    Stack::gap(4).children((
        Checkbox::new("news")
            .id("parts-news")
            .label("Product news")
            .description("A message a month at most.")
            .checked(true),
        Checkbox::new("everything")
            .id("parts-all")
            .label("All notifications")
            .indeterminate(true),
        Switch::new("alerts")
            .id("parts-alerts")
            .label("Payment alerts")
            .description("An email when a payment fails."),
    ))
}

fn breadcrumb() -> Breadcrumb {
    Breadcrumb::new()
        .id("parts-trail")
        .max_items(4)
        .link("Home", "/")
        .link("Customers", "/customers")
        .link("Punjab", "/customers/punjab")
        .link("Ludhiana", "/customers/punjab/ludhiana")
        .link("Dhillon Traders", "/customers/punjab/ludhiana/dhillon")
        .current("Invoices")
}

fn inputs() -> Stack {
    Stack::gap(4).children((
        el("div").id("parts-q-group").child(
            Field::new("parts-q", "Search the catalogue").control(
                Input::search("q")
                    .clearable("Clear the search")
                    .value("kurta"),
            ),
        ),
        Field::new("parts-small", "A small field").control(
            Input::text("small").size(Size::Sm),
        ),
    ))
}

fn colour() -> Element {
    el("form")
        .id("parts-colour-form")
        .attr("method", "get")
        .attr("action", "/parts")
        .child(
            el("div").id("parts-colour").child(
                Field::new("parts-accent", "Accent colour")
                    .help("Choose one, or type a hex code.")
                    .control(
                        ColorPicker::new("accent")
                            .value("#ff9933")
                            .swatch("#ff9933", "Saffron")
                            .swatch("#138808", "Green")
                            .swatch("#000080", "Navy")
                            .swatch("#800000", "Maroon"),
                    ),
            ),
        )
}

fn people() -> Row {
    Row::gap(6).children((
        AvatarGroup::new("Members of the Ludhiana branch")
            .id("parts-people")
            .max(3)
            .total(48)
            .children((
                Avatar::placeholder("Jaspreet Dhillon"),
                Avatar::placeholder("Navdeep Grewal"),
                Avatar::placeholder("Rajveer Brar"),
                Avatar::placeholder("Harleen Kaur"),
            )),
        Avatar::image("/parts/missing.png", "Gurpreet Kaur")
            .fallback()
            .id("parts-broken"),
    ))
}

fn trace() -> Trace {
    Trace::new("Reconciling the March ledger")
        .id("parts-trace")
        .step(TraceStep::new("Read 4,812 entries").description("From the Pune branch"))
        .step(
            TraceStep::new("Matched with bank lines")
                .state(TraceState::Running)
                .description("2,904 of 4,812 so far")
                .detail("Unmatched lines wait for the next pass.")
                .open(),
        )
        .step(TraceStep::new("Write the report").state(TraceState::Pending))
        .step(TraceStep::new("Send it to the owner").state(TraceState::Failed))
}

fn questionnaire() -> Questionnaire {
    Questionnaire::new("/parts/quiz")
        .id("parts-quiz")
        .question(
            Question::single("team", "How big is your team?")
                .choice("small", "Up to 10 people")
                .choice("large", "More than 10")
                .required(),
        )
        .question(
            Question::multiple("tools", "What do you use today?")
                .help("Choose any that apply.")
                .choice("sheets", "Spreadsheets")
                .choice("tally", "Tally")
                .choice("paper", "Registers on paper"),
        )
        .question(Question::text("goal", "What do you want to keep track of?"))
        .step(1)
        .answers([("team", "small")])
}

fn country() -> Select {
    Select::new("country")
        .id("parts-country")
        .aria_label("Country")
        .choice(Choice::new("IN", "India").flag(flags::IN))
        .choice(Choice::new("NP", "Nepal").flag(flags::NP))
        .choice(Choice::new("RS", "Serbia").flag(flags::RS))
        .selected("IN")
}

fn flags_by_pointer() -> Stack {
    let row = |name: &'static str, code: flags::CountryCode| {
        el("span")
            .attr("data-flag-row", "")
            .children((Flag::new(code).sprite().decorative(), " ", name))
    };
    let rows: Vec<Element> = (0..20)
        .flat_map(|_| [row("Serbia", flags::RS), row("India", flags::IN)])
        .collect();
    Stack::gap(2).children((
        FlagSprite::new([flags::RS, flags::IN]),
        el("div").id("parts-flags").child(rows),
    ))
}

fn sparkline_table() -> Element {
    let cities = ["Ludhiana", "Amritsar", "Jalandhar", "Patiala", "Chandigarh"];
    let rows: Vec<TableRowEl> = (0..50_usize)
        .map(|at| {
            let name = format!("{} {}", cities[at % cities.len()], at / cities.len() + 1);
            let series: Vec<f64> = (0..30_usize)
                .map(|day| ((at * 7 + day * 13) % 41) as f64)
                .collect();
            TableRowEl::new((
                TableCell::new(name.clone()),
                TableCell::new(
                    Sparkline::new(format!("Orders of {name}"), series).cell(),
                ),
            ))
        })
        .collect();
    el("div").id("parts-spark").child(
        Table::new()
            .caption("Orders by store, last 30 days")
            .head((TableHead::new("Store"), TableHead::new("Orders")))
            .rows(rows),
    )
}

fn page(shell: &AppShell) -> AppPage {
    shell.page("Parts").body(Stack::gap(10).children((
        section("alert", "An alert with actions and a close button", alert()),
        section("boxes", "Boxes with a description", boxes()),
        section("breadcrumb", "A long trail folded", breadcrumb()),
        section("inputs", "Inputs", inputs()),
        section("colour", "A colour", colour()),
        section("people", "People", people()),
        section("trace", "A trace", trace()),
        section("questionnaire", "A questionnaire", questionnaire()),
        section("country", "A flag in each choice", country()),
        section("flags", "Flags by pointer", flags_by_pointer()),
        section("spark", "A sparkline in each row", sparkline_table()),
    )))
}

pub struct PartsPages;

impl Injectable for PartsPages {
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Controller for PartsPages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "!/parts", |shell: AppShell| async move {
                page(&shell)
            })
            .public(),
        ]
    }
}
