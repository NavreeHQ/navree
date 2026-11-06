//! The parts that the parts module and its stylesheet serve: an avatar group and an
//! avatar's fallback, an alert's close button and actions, the description under a
//! checkbox or switch, an indeterminate box, a progress bar with a tone and a figure, the
//! card variants, the smaller input sizes and the clear button, a breadcrumb that folds,
//! the two smallest headings, the colour picker, a trace, a questionnaire and a sparkline
//! in a table cell.

use navree_ui::charts::Sparkline;
use navree_ui::{
    Alert, AlertTone, Avatar, AvatarGroup, Breadcrumb, Button, Card, CardVariant, Checkbox,
    ColorPicker, Field, Heading, Input, Node, Progress, Question, Questionnaire, QuestionnaireMove,
    Size, Switch, Tone, Trace, TraceState, TraceStep, Uses, View, render,
};

fn html(node: impl Into<Node>) -> String {
    render(&node.into())
}

fn uses(node: impl Into<Node>) -> Uses {
    View::new("x").body(node.into()).uses()
}

fn count(text: &str, needle: &str) -> usize {
    text.matches(needle).count()
}

#[test]
fn an_alert_with_a_close_button_hides_itself_by_command_and_only_where_a_script_runs() {
    let out = html(
        Alert::new(AlertTone::Warning)
            .title("Trial ends soon")
            .body("Three days left.")
            .dismissible("trial-alert", "Dismiss this notice"),
    );
    assert!(out.contains(r#"id="trial-alert""#), "{out}");
    assert!(out.contains("data-closable"), "{out}");
    assert!(out.contains(r#"aria-label="Dismiss this notice""#), "{out}");
    assert!(
        out.contains(r#"<span data-script-only>"#),
        "no script, no button: {out}"
    );
    assert!(out.contains("trial-alert"), "the command names the alert");
    let page = uses(Alert::new(AlertTone::Neutral).dismissible("a", "Dismiss"));
    assert!(page.has(Uses::PARTS_STYLES));
    assert!(page.has(Uses::CORE), "a command needs the core runtime");
    let plain = uses(Alert::new(AlertTone::Neutral).title("Hello"));
    assert!(plain.is_none(), "an alert as before loads nothing");
}

#[test]
fn an_alert_draws_its_actions_under_the_description() {
    let out = html(
        Alert::new(AlertTone::Danger)
            .title("Payment failed")
            .body("The card was declined.")
            .actions(Button::outline("Update card").href("/billing")),
    );
    let body = out.find("alert-description").expect("description");
    let actions = out.find("alert-actions").expect("actions");
    assert!(body < actions, "{out}");
    assert!(out.contains("Update card"), "{out}");
    assert!(!out.contains("data-closable"), "{out}");
}

#[test]
fn an_avatar_group_shows_the_most_it_may_and_counts_the_rest() {
    let people = || {
        (
            Avatar::placeholder("Gurpreet Kaur"),
            Avatar::placeholder("Harjot Singh"),
            Avatar::placeholder("Simran Arora"),
            Avatar::placeholder("Jaspreet Dhillon"),
        )
    };
    let out = html(AvatarGroup::new("Reviewers").max(2).children(people()));
    assert!(out.contains(r#"role="group" aria-label="Reviewers""#), "{out}");
    assert_eq!(count(&out, "Gurpreet Kaur") + count(&out, "Harjot Singh"), 2);
    assert!(!out.contains("Simran Arora"), "{out}");
    assert!(out.contains("+2"), "{out}");
    assert!(out.contains(r#"aria-label="2 more""#), "{out}");

    // All shown: no tile.
    let all = html(AvatarGroup::new("Reviewers").max(9).children(people()));
    assert!(!all.contains("data-count"), "{all}");

    // The first of a longer list: the tile counts from the total.
    let first = html(
        AvatarGroup::new("Members")
            .max(3)
            .total(40)
            .children(people()),
    );
    assert!(first.contains("+37"), "{first}");

    // A total smaller than what was given changes nothing; at least one avatar shows.
    let low = html(AvatarGroup::new("Members").max(0).total(1).children(people()));
    assert_eq!(count(&low, r#"data-slot="avatar""#), 2, "{low}");
    assert!(low.contains("+3"), "{low}");

    assert!(uses(AvatarGroup::new("x").children(Avatar::placeholder("A"))).has(Uses::PARTS_STYLES));
}

#[test]
fn an_avatar_asks_for_its_fallback_and_loads_the_script_only_then() {
    let plain = Avatar::image("/a.png", "Ada");
    assert!(uses(plain.clone()).is_none());
    assert!(!html(plain).contains("data-ui-fallback"));
    let with = Avatar::image("/a.png", "Ada").fallback();
    assert!(html(with.clone()).contains("data-ui-fallback"));
    let needs = uses(with);
    assert!(needs.has(Uses::PARTS) && needs.has(Uses::PARTS_STYLES));
}

#[test]
fn a_long_breadcrumb_folds_its_middle_into_a_list_that_opens_without_script() {
    let trail = Breadcrumb::new()
        .link("Home", "/")
        .link("Customers", "/customers")
        .link("Punjab", "/customers/punjab")
        .link("Ludhiana", "/customers/punjab/ludhiana")
        .link("Dhillon Traders", "/customers/punjab/ludhiana/dhillon")
        .current("Invoices");
    let out = html(trail.clone().max_items(4));
    assert_eq!(count(&out, "<li"), 1 + 1 + 3 + 2, "{out}");
    assert!(out.contains("<details"), "{out}");
    assert!(out.contains(r#"aria-label="Show hidden pages""#), "{out}");
    let folded = out.find("breadcrumb-hidden").expect("folded list");
    let ludhiana = out.find("Ludhiana").expect("Ludhiana");
    let invoices = out.find("Invoices").expect("current");
    assert!(folded < ludhiana, "the ones left out are in the folded list");
    assert!(ludhiana < invoices, "{out}");
    assert!(out.find("Home").unwrap() < folded, "the first stays in view");
    assert_eq!(count(&out, r#"aria-current="page""#), 1);

    // Short enough: nothing folds.
    let whole = html(trail.clone().max_items(9));
    assert!(!whole.contains("<details"), "{whole}");
    // Three is the least: the first, the ellipsis and the last.
    let tiny = html(trail.max_items(1));
    assert_eq!(count(&tiny, r#"data-slot="breadcrumb-ellipsis""#), 1, "{tiny}");
    assert!(tiny.contains("Invoices") && tiny.contains("Home"));
}

#[test]
fn a_card_variant_is_an_attribute_and_the_default_loads_nothing() {
    assert!(uses(Card::titled("Plain")).is_none());
    for (variant, name) in [
        (CardVariant::Muted, "muted"),
        (CardVariant::Ghost, "ghost"),
        (CardVariant::Dashed, "dashed"),
    ] {
        let card = Card::titled("Add a team").variant(variant);
        assert!(html(card.clone()).contains(&format!(r#"data-variant="{name}""#)));
        assert!(uses(card).has(Uses::PARTS_STYLES));
    }
    assert!(!html(Card::titled("A").variant(CardVariant::Default)).contains("data-variant"));
}

#[test]
fn a_progress_bar_takes_a_tone_and_can_write_its_figure() {
    let out = html(
        Progress::new(3, 8, "Profile complete")
            .tone(Tone::Success)
            .show_value(),
    );
    assert!(out.contains(r#"data-tone="success""#), "{out}");
    assert!(out.contains("37%"), "3 of 8 is 37 percent, rounded down: {out}");
    assert!(
        out.contains(r#"<span data-slot="progress-value" aria-hidden="true">37%"#),
        "the figure is hidden from a reader, who has the bar: {out}"
    );
    assert!(
        out.contains(r#"aria-label="Profile complete""#),
        "the bar still has its name: {out}"
    );
    // Past the end, and nothing to measure.
    assert!(html(Progress::new(20, 10, "x").show_value()).contains("100%"));
    assert!(html(Progress::new(1, 0, "x").show_value()).contains("0%"));
    assert!(!html(Progress::indeterminate("Working").show_value()).contains("%"));
    // A neutral tone changes nothing.
    assert!(!html(Progress::new(1, 2, "x").tone(Tone::Neutral)).contains("data-tone"));
    assert!(uses(Progress::new(1, 2, "x")).is_none());
}

#[test]
fn the_two_smallest_headings_are_headings() {
    assert!(html(Heading::h5("Notes")).starts_with("<h5 data-slot=\"heading\">"));
    assert!(html(Heading::h6("Fine print")).starts_with("<h6 data-slot=\"heading\">"));
    assert!(uses(Heading::h5("Notes")).has(Uses::PARTS_STYLES));
    assert!(uses(Heading::h4("Notes")).is_none());
}

#[test]
fn a_description_is_tied_to_its_box_and_a_box_can_be_indeterminate() {
    let out = html(
        Checkbox::new("marketing")
            .label("Product news")
            .description("A message a month at most."),
    );
    assert!(out.contains(r#"aria-describedby="marketing-description""#), "{out}");
    assert!(out.contains(r#"id="marketing-description""#), "{out}");
    let label = out.find("Product news").unwrap();
    let description = out.find("A message a month").unwrap();
    assert!(label < description, "{out}");
    assert!(uses(Checkbox::new("a").label("A").description("B")).has(Uses::PARTS_STYLES));

    // The id follows the box's own id when it has one, and is kept to safe characters.
    let own = html(
        Checkbox::new("roles[]")
            .id("role admin")
            .label("Admin")
            .description("Everything"),
    );
    assert!(own.contains(r#"aria-describedby="role-admin-description""#), "{own}");

    // With no label there is nowhere for a description; the box is alone.
    let bare = html(Checkbox::new("x").description("Nothing to hang it on"));
    assert!(!bare.contains("Nothing to hang"), "{bare}");

    let mixed = Checkbox::new("all").label("Everything").indeterminate(true);
    assert!(html(mixed.clone()).contains("data-indeterminate"));
    let needs = uses(mixed);
    assert!(needs.has(Uses::PARTS) && needs.has(Uses::PARTS_STYLES));
    assert!(!html(Checkbox::new("all").indeterminate(false)).contains("data-indeterminate"));
    assert!(html(Checkbox::new("x").invalid(true)).contains(r#"aria-invalid="true""#));

    let switch = html(
        Switch::new("alerts")
            .label("Email alerts")
            .description("When a payment fails."),
    );
    assert!(switch.contains(r#"aria-describedby="alerts-description""#), "{switch}");
    assert!(switch.contains(r#"role="switch""#), "{switch}");
}

#[test]
fn an_input_has_sizes_and_a_clear_button_that_needs_a_script() {
    let plain = html(Input::text("q"));
    assert_eq!(plain, r#"<input data-slot="input" type="text" name="q">"#);
    assert!(uses(Input::text("q")).is_none());

    let small = html(Input::text("q").size(Size::Sm));
    assert!(small.contains(r#"data-size="sm""#), "{small}");
    assert!(uses(Input::text("q").size(Size::Sm)).has(Uses::PARTS_STYLES));
    assert!(!html(Input::text("q").size(Size::Md)).contains("data-size"));

    let clear = html(Input::search("q").clearable("Clear the search").value("saffron"));
    assert!(clear.starts_with(r#"<div data-slot="input-group""#), "{clear}");
    assert!(clear.contains("data-ui-clearable"), "{clear}");
    assert!(clear.contains(r#"aria-label="Clear the search""#), "{clear}");
    assert!(clear.contains("data-script-only"), "{clear}");
    assert!(!clear.contains(r#" hidden="""#), "a field with a value shows it: {clear}");
    let empty = html(Input::search("q").clearable("Clear the search"));
    assert!(empty.contains(r#" hidden="""#), "an empty field hides it: {empty}");
    let needs = uses(Input::search("q").clearable("Clear"));
    assert!(needs.has(Uses::PARTS) && needs.has(Uses::PARTS_STYLES));

    // In a field, the input takes the field's id, and the clear button finds it by place.
    let field = html(
        Field::new("search-box", "Search")
            .control(Input::search("q").clearable("Clear the search")),
    );
    assert!(field.contains(r#"id="search-box""#), "{field}");
}

#[test]
fn the_colour_picker_posts_a_native_colour_and_reads_a_colour_the_way_the_field_does() {
    for (text, want) in [
        ("#D97706", Some("#d97706")),
        ("d97706", Some("#d97706")),
        ("#abc", Some("#aabbcc")),
        ("  #ABC ", Some("#aabbcc")),
        ("#d977", None),
        ("#d9770600", None),
        ("#ggg", None),
        ("red", None),
        ("rgb(1,2,3)", None),
        ("", None),
        ("#", None),
        ("\u{ff03}abc", None),
    ] {
        assert_eq!(ColorPicker::parse(text).as_deref(), want, "{text:?}");
    }

    let out = html(
        ColorPicker::new("accent")
            .value("#D97706")
            .swatches(["#ef4444", "d97706", "not a colour", "#2563eb"])
            .swatch("#16a34a", "Green"),
    );
    assert!(
        out.contains(r##"type="color" name="accent" value="#d97706""##),
        "{out}"
    );
    assert_eq!(count(&out, r#"data-slot="color-swatch""#), 4, "{out}");
    assert!(out.contains(r#"aria-label="Green""#), "{out}");
    assert!(out.contains(r##"aria-label="#ef4444""##), "{out}");
    assert_eq!(count(&out, r#"aria-pressed="true""#), 1, "{out}");
    assert!(out.contains(r##"fill="#2563eb""##), "{out}");
    assert!(!out.contains("style="), "{out}");
    // The hex field and the swatches are for a script; the native field posts alone.
    assert_eq!(count(&out, r#"name=""#), 1, "only one field posts: {out}");
    assert!(out.contains("data-script-only"), "{out}");
    let needs = uses(ColorPicker::new("accent"));
    assert!(needs.has(Uses::PARTS) && needs.has(Uses::PARTS_STYLES));

    // Hostile text never reaches an attribute as itself.
    let hostile = html(
        ColorPicker::new("a")
            .value(r##"#fff" onfocus="x"##)
            .swatches([r##"#fff"><script>"##]),
    );
    assert!(!hostile.contains("onfocus") && !hostile.contains("<script"), "{hostile}");
    assert!(hostile.contains(r##"value="#000000""##), "an unreadable value keeps the default");

    let off = html(ColorPicker::new("a").disabled(true));
    assert!(count(&off, "disabled") >= 2, "{off}");
}

#[test]
fn a_trace_names_the_state_of_each_step_to_a_reader_and_marks_the_current_one() {
    let out = html(
        Trace::new("Reconciling the March ledger")
            .step(TraceStep::new("Read 4,812 entries").description("From the Pune branch"))
            .step(
                TraceStep::new("Matched with bank lines")
                    .state(TraceState::Running)
                    .detail("2,904 of 4,812 matched so far.")
                    .open(),
            )
            .step(TraceStep::new("Wrote the report").state(TraceState::Pending))
            .step(TraceStep::new("Sent to the owner").state(TraceState::Failed)),
    );
    assert!(out.starts_with("<ol data-slot=\"trace\""), "{out}");
    assert!(out.contains(r#"aria-label="Reconciling the March ledger""#), "{out}");
    assert_eq!(count(&out, r#"data-slot="trace-step""#), 4, "{out}");
    for word in ["Done", "Running", "Not started", "Failed"] {
        assert_eq!(count(&out, &format!(">{word}<")), 1, "{word}: {out}");
    }
    assert_eq!(count(&out, r#"aria-current="step""#), 1, "{out}");
    assert!(out.contains("<details data-slot=\"trace-detail\" open>"), "{out}");
    assert!(out.contains("<summary>Details</summary>"), "{out}");
    assert!(!out.contains("style="), "{out}");
    // A step with no detail has no details element.
    let flat = html(Trace::new("x").step(TraceStep::new("One")));
    assert!(!flat.contains("<details"), "{flat}");
    assert!(uses(Trace::new("x")).has(Uses::PARTS_STYLES));
}

#[test]
fn a_questionnaire_shows_one_question_keeps_the_rest_and_offers_the_moves() {
    let form = || {
        Questionnaire::new("/welcome")
            .question(
                Question::single("team", "How big is your team?")
                    .choice("small", "Up to 10")
                    .choice("large", "More than 10")
                    .required(),
            )
            .question(
                Question::multiple("tools", "What do you use today?")
                    .choice("sheets", "Spreadsheets")
                    .choice("tally", "Tally")
                    .help("Choose any that apply."),
            )
            .question(Question::text("goal", "What do you want to track?"))
    };
    let first = html(form());
    assert!(first.starts_with("<form data-slot=\"questionnaire\" method=\"post\""), "{first}");
    assert!(first.contains(r#"action="/welcome""#), "{first}");
    assert!(first.contains("Question 1 of 3"), "{first}");
    assert!(first.contains("How big is your team?"), "{first}");
    assert!(!first.contains("What do you use today?"), "one question at a time: {first}");
    assert!(first.contains(r#"name="_step" value="0""#), "{first}");
    assert!(first.contains(r#"type="radio" name="team" value="small" required"#), "{first}");
    assert!(
        first.contains("disabled"),
        "Previous is off on the first question: {first}"
    );
    assert!(!first.contains(r#"value="skip""#), "a required question is not skipped: {first}");
    assert!(first.contains(r#"value="next""#), "{first}");

    // The second: its answer shows checked, the first answer travels hidden.
    let second = html(
        form()
            .step(1)
            .answers([("team", "small"), ("tools", "tally")]),
    );
    assert!(second.contains("Question 2 of 3"), "{second}");
    assert!(second.contains(r#"type="hidden" name="team" value="small""#), "{second}");
    assert!(!second.contains(r#"type="hidden" name="tools""#), "the shown question is not hidden: {second}");
    assert!(second.contains(r#"type="checkbox" name="tools" value="tally" checked"#), "{second}");
    assert!(second.contains(r#"value="skip""#), "{second}");
    assert!(second.contains(r#"value="back""#), "{second}");
    assert!(second.contains("formnovalidate"), "Previous and Skip ignore the checks: {second}");
    assert!(second.contains("Choose any that apply."), "{second}");

    // The last question finishes; a number past the end shows the last; an error is tied.
    let last = html(form().step(99).error("Say what you want to track."));
    assert!(last.contains("Question 3 of 3"), "{last}");
    assert!(last.contains("Finish"), "{last}");
    assert!(last.contains(r#"role="alert""#) && last.contains("Say what you want to track."), "{last}");
    assert!(last.contains(r#"aria-invalid="true""#), "{last}");

    // Nothing to ask draws nothing.
    assert_eq!(html(Questionnaire::new("/x")), "");

    // Hostile text stays text.
    let hostile = html(
        Questionnaire::new("/x").question(
            Question::single("q", "<img src=x onerror=alert(1)>").choice("a\"><script>", "<b>A</b>"),
        ),
    );
    assert!(!hostile.contains("<img") && !hostile.contains("<script") && !hostile.contains("<b>"), "{hostile}");
}

#[test]
fn the_buttons_of_a_questionnaire_say_where_to_go_next() {
    use QuestionnaireMove::{Back, Next, Skip};
    assert_eq!(QuestionnaireMove::parse("back"), Some(Back));
    assert_eq!(QuestionnaireMove::parse("skip"), Some(Skip));
    assert_eq!(QuestionnaireMove::parse("next"), Some(Next));
    assert_eq!(QuestionnaireMove::parse("Next"), None);
    assert_eq!(QuestionnaireMove::parse(""), None);
    assert_eq!(Back.target(0, 5), 0, "never before the first");
    assert_eq!(Back.target(3, 5), 2);
    assert_eq!(Next.target(0, 5), 1);
    assert_eq!(Skip.target(3, 5), 4);
    assert_eq!(Next.target(4, 5), 5, "past the last is finished");
    assert_eq!(Next.target(9, 5), 5, "and never beyond");
}

#[test]
fn a_sparkline_in_a_cell_is_one_named_picture_with_no_table() {
    let cell = html(Sparkline::new("Opens, last 12 hours", [3, 5, 4, 9, 12, 10, 14, 13, 18, 16, 21, 24]).cell());
    assert!(cell.starts_with(r#"<span data-slot="sparkline-cell" role="img" aria-label=""#), "{cell}");
    assert!(!cell.contains("<table") && !cell.contains("<figure"), "{cell}");
    assert!(cell.contains("<svg"), "{cell}");
    assert!(cell.contains("Opens, last 12 hours"), "the summary names the series: {cell}");
    assert!(uses(Sparkline::new("x", [1, 2]).cell()).has(Uses::PARTS_STYLES));
    assert!(!uses(Sparkline::new("x", [1, 2]).cell()).has(Uses::CHARTS), "no script");

    // Nothing to draw: a dash, still named.
    let none = html(Sparkline::new("Opens", Vec::<f64>::new()).cell());
    assert!(none.contains("data-empty"), "{none}");
    assert!(none.contains("Opens"), "{none}");
    assert!(!none.contains("<svg"), "{none}");

    // The full form is unchanged.
    let figure = html(Sparkline::new("Trend", [1, 3, 2, 5]));
    assert!(figure.starts_with("<figure"), "{figure}");
}

/// Fifty rows with a sparkline each stay small, and hold no nested table.
#[test]
fn fifty_rows_of_cell_sparklines_have_a_budget() {
    let rows: Vec<String> = (0..50)
        .map(|row| {
            let series: Vec<f64> = (0..30)
                .map(|day| f64::from((row * 7 + day * 13) % 41))
                .collect();
            html(Sparkline::new("Orders, last 30 days", series).cell())
        })
        .collect();
    let bytes: usize = rows.iter().map(String::len).sum();
    assert!(rows.iter().all(|row| !row.contains("<table")));
    assert!(
        bytes < 50 * 1200,
        "{bytes} bytes for 50 cells, {} each",
        bytes / 50
    );
    let figure = html(Sparkline::new("Orders, last 30 days", (0..30).map(f64::from).collect::<Vec<_>>()));
    assert!(
        rows[0].len() * 2 < figure.len(),
        "a cell is far smaller than the figure: {} against {}",
        rows[0].len(),
        figure.len()
    );
}

#[derive(Clone, navree_ui::TableRow)]
#[table(per = 50)]
struct TrendRow {
    #[col(skip)]
    id: i64,
    #[col(sortable, label = "Store")]
    store: String,
    #[col(cell = trend_cell, label = "Orders, last 30 days")]
    trend: navree_ui::table::Computed,
}

fn trend_cell(row: &TrendRow) -> Node {
    let series: Vec<f64> = (0..30_i64)
        .map(|day| ((row.id * 7 + day * 13) % 41) as f64)
        .collect();
    Sparkline::new(format!("Orders of {}", row.store), series)
        .cell()
        .into()
}

/// A page of a table with a sparkline in each row holds no nested table and stays small.
#[test]
fn a_table_column_can_hold_a_cell_sparkline() {
    let rows: Vec<TrendRow> = (1..=50)
        .map(|id| TrendRow {
            id,
            store: format!("Store {id}"),
            trend: navree_ui::table::Computed,
        })
        .collect();
    let query = navree_ui::TableQuery::<TrendRow>::parse("/stores", "");
    let out = html(navree_ui::DataTable::new("stores", &query, &rows));
    assert_eq!(count(&out, r#"data-slot="sparkline-cell""#), 50, "one picture in each row");
    assert_eq!(count(&out, "<table"), 1, "the table, and no table in any cell");
    assert!(out.contains("Orders of Store 7"), "each picture is named for its row");
}

#[test]
fn a_choice_leads_with_a_flag_in_the_list_and_in_the_control() {
    use navree_ui::{Choice, Select, flags};
    let country = |chosen: &str| {
        Select::new("country")
            .choice(Choice::new("IN", "India").flag(flags::IN))
            .choice(Choice::new("NP", "Nepal").flag(flags::NP))
            .choice(Choice::new("LK", "Sri Lanka"))
            .selected(chosen)
    };
    let out = html(country("NP"));
    assert_eq!(
        count(&out, r#"data-slot="flag""#),
        3,
        "two in the list, one in the control: {out}"
    );
    assert_eq!(count(&out, "data-lead"), 3, "{out}");
    let trigger = out.find("select-trigger").expect("trigger");
    let first_flag = out.find(r#"data-slot="flag""#).expect("flag");
    let value = out.find("select-value").expect("value");
    assert!(
        trigger < first_flag && first_flag < value,
        "the flag leads the control's text: {out}"
    );
    assert!(uses(country("IN")).has(Uses::PARTS_STYLES));

    // A choice with no lead, chosen: the control has none.
    let plain = html(country("LK"));
    assert_eq!(
        count(&plain, r#"data-slot="flag""#),
        2,
        "only the two in the list: {plain}"
    );
}

#[test]
fn a_choice_takes_any_node_as_its_lead() {
    use navree_ui::{Choice, Select, el};
    let out = html(
        Select::new("tone")
            .choice(Choice::new("red", "Red").lead(el("span").attr("data-dot", "red")))
            .selected("red"),
    );
    assert!(out.contains(r#"data-dot="red""#), "{out}");
    assert!(out.contains(r#"aria-hidden="true""#), "{out}");
}

#[test]
fn a_flag_in_a_timeline_meta_is_a_picture_beside_the_words_and_read_once() {
    use navree_ui::{Flag, Meta, Timeline, TimelineEvent, flags};
    let out = html(
        Timeline::new("steps", "What happened").event(
            TimelineEvent::new("Signed in")
                .at(1_793_592_667)
                .meta(Meta::flag(Flag::new(flags::IN).decorative(), "Mumbai"))
                .meta(Meta::flag(Flag::new(flags::NP), "Kathmandu")),
        ),
    );
    assert_eq!(
        count(&out, r#"data-slot="meta-start" aria-hidden="true""#),
        2,
        "{out}"
    );
    assert_eq!(count(&out, r#"data-slot="flag""#), 2, "{out}");
    assert!(out.contains("Mumbai") && out.contains("Kathmandu"), "{out}");
    // The slot hides the flag from a reader, so its name is not read twice.
    assert!(
        out.contains(r#"<span data-slot="meta-start" aria-hidden="true"><svg data-slot="flag""#),
        "{out}"
    );
}

#[test]
fn a_flag_by_pointer_is_small_and_the_sprite_holds_each_flag_once() {
    use navree_ui::{Flag, FlagShape, FlagSprite, flags};
    let inline = html(Flag::new(flags::RS));
    let pointer = html(Flag::new(flags::RS).sprite());
    assert!(pointer.contains(r##"<use href="#flag-rs">"##), "{pointer}");
    assert!(
        pointer.len() * 20 < inline.len(),
        "{} against {}",
        pointer.len(),
        inline.len()
    );
    assert!(
        pointer.contains(r#"aria-label="Serbia""#),
        "the name is kept: {pointer}"
    );

    let rows: String = (0..100).map(|_| pointer.clone()).collect();
    let sprite = html(FlagSprite::new([flags::RS, flags::IN, flags::RS]));
    assert_eq!(count(&sprite, "<symbol"), 2);
    assert!(sprite.contains(r#"id="flag-rs""#) && sprite.contains(r#"id="flag-in""#));
    assert!(sprite.contains(r#"aria-hidden="true""#));
    let whole_page = rows.len() + sprite.len();
    assert!(
        whole_page < inline.len() * 2,
        "a hundred rows and the sprite: {whole_page} bytes, one flag inline is {}",
        inline.len()
    );
    assert!(uses(FlagSprite::new([flags::IN])).has(Uses::PARTS_STYLES));

    // A square flag points at its own art.
    let square = html(Flag::new(flags::IN).shape(FlagShape::Round).sprite());
    assert!(square.contains("#flag-in-square"), "{square}");
    let both = html(FlagSprite::new([flags::IN]).with_squares());
    assert_eq!(count(&both, "<symbol"), 2);
    // Without the sprite option the flag is as before.
    assert!(!html(Flag::new(flags::IN)).contains("<use"));
    // A code nobody knows is drawn in full, whatever is asked.
    assert!(!html(Flag::country("??").sprite()).contains("<use"));
}

#[derive(Clone, navree_ui::TableRow)]
#[table(per = 10)]
struct Ledger {
    #[col(skip)]
    id: i64,
    #[col(sortable, sticky, label = "Account")]
    account: String,
    #[col(numeric, label = "Debit")]
    debit: i64,
    #[col(numeric, sticky_end, label = "Balance")]
    balance: i64,
    #[col(numeric, sticky_end, label = "Limit")]
    limit: i64,
}

#[derive(Clone, navree_ui::TableRow)]
#[table(per = 10)]
struct OneEnd {
    #[col(skip)]
    id: i64,
    #[col(sortable, label = "Account")]
    account: String,
    #[col(numeric, sticky_end, label = "Balance")]
    balance: i64,
}

#[test]
fn a_table_can_keep_several_columns_at_its_end_and_numbers_them_from_the_end() {
    use navree_ui::{DataTable, RowAction, TableQuery};
    let rows: Vec<Ledger> = (1..=3)
        .map(|id| Ledger {
            id,
            account: format!("Gurdwara Road {id}"),
            debit: id * 1_00_000,
            balance: id * 2_50_000,
            limit: id * 10_00_000,
        })
        .collect();
    let query = TableQuery::<Ledger>::parse("/ledger", "");

    // No row menu: the last column is `e`, the one before it `e1`.
    let plain = html(DataTable::new("ledger", &query, &rows));
    let heads = plain.split("<thead>").nth(1).and_then(|part| part.split("</thead>").next()).expect("a head");
    assert_eq!(count(heads, r#"data-sticky="e1""#), 1, "{heads}");
    assert_eq!(count(heads, r#"data-sticky="e""#), 1, "{heads}");
    assert!(heads.find(r#"data-sticky="e1""#) < heads.find(r#"data-sticky="e""#), "balance comes before limit: {heads}");
    assert_eq!(count(&plain, r#"data-sticky="e1""#), 1 + 3, "the head and a cell in each row: {plain}");
    assert!(uses(DataTable::new("ledger", &query, &rows)).has(Uses::PARTS_STYLES));

    // With a row menu, which is the last of them: the two columns are `e2` and `e1`.
    let menu = html(
        DataTable::new("ledger", &query, &rows)
            .row_actions(|row: &Ledger| vec![RowAction::link("Open", format!("/ledger/{}", row.id))])
            .csrf("tok"),
    );
    let heads = menu.split("<thead>").nth(1).and_then(|part| part.split("</thead>").next()).expect("a head");
    for name in ["e2", "e1", "e"] {
        assert_eq!(count(heads, &format!(r#"data-sticky="{name}""#)), 1, "{name}: {heads}");
    }
    assert!(
        heads.find(r#"data-sticky="e2""#) < heads.find(r#"data-sticky="e1""#)
            && heads.find(r#"data-sticky="e1""#) < heads.find(r#"data-sticky="e""#),
        "{heads}"
    );

    // One column at the end needs nothing more than the sheet every page has.
    let one: Vec<OneEnd> = vec![OneEnd { id: 1, account: "Sector 17".to_owned(), balance: 5_00_000 }];
    let one_query = TableQuery::<OneEnd>::parse("/ledger", "");
    assert!(!uses(DataTable::new("one", &one_query, &one)).has(Uses::PARTS_STYLES));
}
