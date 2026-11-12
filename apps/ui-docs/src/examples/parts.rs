//! ColorPicker, AvatarGroup, Trace, Questionnaire, and the smaller parts of the base
//! components: an alert's actions, a progress bar's tone, the card variants, a folded
//! breadcrumb, a described checkbox and an input with sizes and a clear button.

use navree::ui::*;

use crate::catalog::{Example, single};

const SOURCE: &str = include_str!("parts.rs");

pub fn all() -> Vec<Example> {
    vec![
        single!("color-picker", "default", "A colour field", color_default)
            .start()
            .height(110),
        single!("color-picker", "swatches", "With swatches", color_swatches)
            .start()
            .height(170),
        single!("avatar-group", "default", "Reviewers", group_default)
            .start()
            .height(80),
        single!("avatar-group", "more", "More than fit", group_more)
            .start()
            .height(80),
        single!("trace", "default", "The steps of a run", trace_default)
            .start()
            .height(250),
        single!("trace", "detail", "With detail that opens", trace_detail)
            .start()
            .height(300),
        single!(
            "questionnaire",
            "default",
            "One question at a time",
            questionnaire_default
        )
        .start()
        .height(360),
        single!(
            "questionnaire",
            "text",
            "A question in words",
            questionnaire_text
        )
        .start()
        .height(300),
        single!(
            "alert",
            "actions",
            "Actions and a close button",
            alert_actions
        )
        .start()
        .height(150),
        single!("progress", "tone", "A tone and a figure", progress_tone)
            .start()
            .height(130),
        single!(
            "progress",
            "painted",
            "A gradient, a goal, colors by value and a glow",
            progress_painted
        )
        .start()
        .height(250),
        single!("card", "variants", "Variants", card_variants)
            .start()
            .height(420),
        single!(
            "breadcrumb",
            "folded",
            "A long trail folded",
            breadcrumb_folded
        )
        .start()
        .height(110),
        single!(
            "checkbox",
            "described",
            "With a description",
            checkbox_described
        )
        .start()
        .height(210),
        single!("input", "sizes", "Sizes and a clear button", input_sizes)
            .start()
            .height(190),
    ]
}

fn color_default() -> Field {
    Field::new("accent-colour", "Accent colour")
        .control(ColorPicker::new("accent").value("#d97706"))
}

fn color_swatches() -> Field {
    Field::new("brand-colour", "Brand colour")
        .help("Choose one, or type a hex code.")
        .control(
            ColorPicker::new("brand")
                .value("#ff9933")
                .swatch("#ff9933", "Saffron")
                .swatch("#138808", "Green")
                .swatch("#000080", "Navy")
                .swatch("#800000", "Maroon")
                .swatch("#e1ad01", "Turmeric")
                .swatch("#5b3a29", "Walnut"),
        )
}

fn group_default() -> AvatarGroup {
    AvatarGroup::new("Reviewers").children((
        Avatar::placeholder("Gurpreet Kaur"),
        Avatar::placeholder("Harjot Singh"),
        Avatar::placeholder("Simran Arora"),
    ))
}

fn group_more() -> AvatarGroup {
    AvatarGroup::new("Members of the Ludhiana branch")
        .max(3)
        .total(48)
        .children((
            Avatar::placeholder("Jaspreet Dhillon"),
            Avatar::placeholder("Navdeep Grewal"),
            Avatar::placeholder("Rajveer Brar"),
            Avatar::placeholder("Harleen Kaur"),
        ))
}

fn trace_default() -> Trace {
    Trace::new("Reconciling the March ledger")
        .step(TraceStep::new("Read 4,812 entries").description("From the Pune branch"))
        .step(
            TraceStep::new("Matched with bank lines")
                .description("2,904 of 4,812 so far")
                .state(TraceState::Running),
        )
        .step(TraceStep::new("Write the report").state(TraceState::Pending))
}

fn trace_detail() -> Trace {
    Trace::new("Importing the customer list")
        .step(
            TraceStep::new("Checked the columns")
                .description("All six were found")
                .detail(Text::code(
                    "name, phone, city, gstin, credit_limit, joined_on",
                )),
        )
        .step(
            TraceStep::new("Read the rows")
                .state(TraceState::Failed)
                .description("Row 212 was not read")
                .detail(Text::new(
                    "The phone number on row 212 has nine digits. Fix it and send the file again.",
                ))
                .open(),
        )
        .step(TraceStep::new("Saved the customers").state(TraceState::Pending))
}

fn questionnaire() -> Questionnaire {
    Questionnaire::new("/welcome")
        .question(
            Question::single("team", "How big is your team?")
                .choice("small", "Up to 10 people")
                .choice("medium", "11 to 50 people")
                .choice("large", "More than 50")
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
}

fn questionnaire_default() -> Questionnaire {
    questionnaire().step(1).answers([("team", "small")])
}

fn questionnaire_text() -> Questionnaire {
    questionnaire()
        .step(2)
        .answers([("team", "small"), ("tools", "tally")])
}

fn alert_actions() -> Alert {
    Alert::new(AlertTone::Warning)
        .title("Your trial ends in 3 days")
        .body("Add a payment method to keep your workspace.")
        .actions((
            Button::primary("Add payment").href("/billing"),
            Button::ghost("Remind me later"),
        ))
        .dismissible("trial-notice", "Dismiss this notice")
}

fn progress_tone() -> Stack {
    Stack::gap(3).children((
        Progress::new(72, 100, "Storage used")
            .tone(Tone::Warning)
            .show_value(),
        Progress::new(100, 100, "Import finished")
            .tone(Tone::Success)
            .show_value(),
        Progress::new(18, 100, "Deliveries failed")
            .tone(Tone::Danger)
            .show_value(),
    ))
}

fn progress_painted() -> Stack {
    use navree::ui::charts::{ColorScale, Gradient, Ink, Motion};
    let quality = |value: u32, label: &str, slot: u8, step: u32| {
        Progress::new(value, 100, label.to_owned())
            .ink(Ink::slot(slot))
            .height(8)
            .show_value()
            .motion(Motion::new().delay(300 + step * 100))
    };
    Stack::gap(4).children((
        Progress::new(402, 480, "Sleep, 6 h 42 min against a goal of 7 h")
            .fill(
                Gradient::horizontal()
                    .stop(0.0, Ink::slot(7))
                    .stop_with_opacity(1.0, Ink::slot(7), 0.6),
            )
            .goal(420)
            .height(12)
            .motion(Motion::new().delay(200)),
        Progress::new(68, 100, "Recovery 68%, fair")
            .colors(
                ColorScale::new(Ink::tone(Tone::Danger))
                    .from(50, Ink::tone(Tone::Warning))
                    .from(75, Ink::tone(Tone::Success)),
            )
            .glow()
            .height(8)
            .show_value(),
        quality(91, "Efficiency", 7, 0),
        quality(74, "Depth", 1, 1),
        quality(83, "Continuity", 3, 2),
    ))
}

fn card_variants() -> Stack {
    Stack::gap(3).children((
        Card::titled("Default").body("A line around a surface."),
        Card::titled("Muted")
            .variant(CardVariant::Muted)
            .body("A filled surface with no line."),
        Card::titled("Ghost")
            .variant(CardVariant::Ghost)
            .body("Only the spacing."),
        Card::titled("Add a branch")
            .variant(CardVariant::Dashed)
            .body("A place to add something."),
    ))
}

fn breadcrumb_folded() -> Breadcrumb {
    Breadcrumb::new()
        .max_items(4)
        .link("Home", "/")
        .link("Customers", "/customers")
        .link("Punjab", "/customers/punjab")
        .link("Ludhiana", "/customers/punjab/ludhiana")
        .link("Dhillon Traders", "/customers/punjab/ludhiana/dhillon")
        .current("Invoices")
}

fn checkbox_described() -> Stack {
    Stack::gap(4).children((
        Checkbox::new("news")
            .label("Product news")
            .description("A message a month at most.")
            .checked(true),
        Checkbox::new("everything")
            .label("All notifications")
            .indeterminate(true),
        Switch::new("alerts")
            .label("Payment alerts")
            .description("An email when a payment fails."),
    ))
}

fn input_sizes() -> Stack {
    Stack::gap(3).children((
        Input::text("small").size(Size::Sm).placeholder("Small"),
        Input::text("default").placeholder("Default"),
        Input::search("q")
            .clearable("Clear the search")
            .value("kurta")
            .placeholder("Search the catalogue"),
    ))
}
