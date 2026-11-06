//! Select, Combobox, MultiSelect and Command: the markup a runtime and a screen
//! reader rely on, the escaping, what a page loads, and the endpoint types.

use navree_ui::{
    Choice, ChoiceList, Combobox, Command, CommandGroup, CommandItem, Field, MultiSelect,
    OptionQuery, Select, SelectLabels, Side, Size, Uses, View, icons, render,
};

fn html(node: impl Into<navree_ui::Node>) -> String {
    render(&node.into())
}

/// The value of `attr` on the first tag that has `marker` in it.
fn attr_of<'a>(html: &'a str, marker: &str, attr: &str) -> Option<&'a str> {
    let at = html.find(marker)?;
    let start = html[..at].rfind('<')?;
    let end = start + html[start..].find('>')?;
    let tag = &html[start..end];
    let key = format!(" {attr}=\"");
    let from = tag.find(&key)? + key.len();
    let to = from + tag[from..].find('"')?;
    Some(&tag[from..to])
}

fn stages() -> Select {
    Select::new("stage")
        .placeholder("Choose a stage")
        .option("lead", "Lead")
        .option("won", "Won")
        .option("lost", "Lost")
}

// ---------------------------------------------------------------- Select

#[test]
fn a_select_is_a_combobox_button_with_a_listbox_and_a_native_select_carrying_the_value() {
    let out = html(stages().selected("won"));
    assert!(out.contains(r#"data-slot="select-trigger""#), "{out}");
    assert!(out.contains(r#"role="combobox""#));
    assert!(out.contains(r#"aria-haspopup="listbox""#));
    assert!(out.contains(r#"aria-expanded="false""#));
    assert!(out.contains(r#"role="listbox""#));
    // The button controls the listbox that exists.
    let controls = attr_of(&out, "data-slot=\"select-trigger\"", "aria-controls").unwrap();
    assert!(out.contains(&format!(r#"id="{controls}""#)), "{out}");
    // The carrier posts, with an empty first choice, and holds the chosen one.
    assert!(
        out.contains(r#"<select data-slot="select-native" name="stage""#),
        "{out}"
    );
    assert!(out.contains(r#"<option value="">Choose a stage</option>"#));
    assert!(out.contains(r#"<option value="won" selected>Won</option>"#));
    assert!(!out.contains(r#"<option value="lead" selected"#));
    // The trigger shows the label of the chosen one.
    assert!(out.contains(">Won</span>"), "{out}");
    assert!(out.contains(r#"aria-selected="true""#));
}

#[test]
fn an_empty_select_shows_its_placeholder_and_selects_the_empty_choice() {
    let out = html(stages());
    assert!(out.contains(r#"data-placeholder"#), "{out}");
    assert!(
        out.contains(r#"<option value="" selected>Choose a stage</option>"#),
        "{out}"
    );
    assert!(!out.contains(r#"aria-selected="true""#));
}

#[test]
fn a_select_with_no_placeholder_still_has_an_empty_choice_so_nothing_is_posted_by_accident() {
    let out = html(Select::new("x").option("a", "A").option("b", "B"));
    assert!(
        out.contains(r#"<option value="" selected></option>"#),
        "{out}"
    );
}

#[test]
fn groups_are_groups_for_a_screen_reader_and_optgroups_for_the_browser() {
    let out = html(
        Select::new("tz")
            .group("Europe", [("london", "London"), ("paris", "Paris")])
            .separator()
            .group("Asia", [("tokyo", "Tokyo")]),
    );
    assert!(out.contains(r#"role="group" aria-label="Europe""#), "{out}");
    assert!(out.contains(r#"<optgroup label="Asia">"#));
    assert!(out.contains(r#"data-slot="option-separator""#), "{out}");
}

#[test]
fn a_disabled_choice_is_aria_disabled_and_disabled_in_the_carrier() {
    let out = html(
        Select::new("x")
            .option("a", "A")
            .choice(Choice::new("b", "B").disabled(true)),
    );
    assert!(
        out.contains(r#"data-value="b" aria-disabled="true""#),
        "{out}"
    );
    assert!(
        out.contains(r#"<option value="b" disabled>B</option>"#),
        "{out}"
    );
}

#[test]
fn rich_choices_carry_a_description_an_icon_keywords_and_a_selected_label() {
    let out = html(
        Select::new("x").choice(
            Choice::new("a", "Anu Lakhotia")
                .description("Engineer")
                .icon(icons::CHECK)
                .keywords(["analytical", "engine"])
                .selected_label("Anu"),
        ),
    );
    assert!(out.contains(r#"data-slot="option-description""#), "{out}");
    assert!(out.contains(r#"data-keywords="analytical engine""#));
    assert!(out.contains(r#"data-selected-label="Anu""#));
}

#[test]
fn text_is_escaped_everywhere_a_choice_can_put_it() {
    let hostile = r#"<img src=x onerror=alert(1)>"'&"#;
    let out = html(
        Select::new("x")
            .placeholder(hostile)
            .choice(
                Choice::new(hostile, hostile)
                    .description(hostile)
                    .keywords([hostile]),
            )
            .selected(hostile),
    );
    assert!(!out.contains("<img"), "{out}");
    assert!(out.contains("&lt;img"), "{out}");
}

#[test]
fn a_select_asks_for_the_overlay_and_select_modules_and_not_the_core_runtime() {
    let view = View::new("t").body(stages());
    assert!(view.uses().has(Uses::SELECT));
    assert!(view.uses().has(Uses::OVERLAY));
    assert!(!view.uses().has(Uses::CORE));
}

#[test]
fn a_select_in_a_field_takes_the_fields_id_on_its_trigger_and_the_label_points_at_it() {
    let out = html(
        Field::new("deal-stage", "Stage")
            .error("Choose one.")
            .control(stages()),
    );
    assert!(
        out.contains(r#"<label data-slot="label" for="deal-stage">"#),
        "{out}"
    );
    let id = attr_of(&out, "data-slot=\"select-trigger\"", "id").unwrap();
    assert_eq!(id, "deal-stage");
    assert_eq!(
        attr_of(&out, "data-slot=\"select-trigger\"", "aria-invalid"),
        Some("true")
    );
    // The hidden carrier did not take the id.
    assert!(
        attr_of(&out, "data-slot=\"select-native\"", "id").is_none(),
        "{out}"
    );
}

#[test]
fn the_flags_reach_the_trigger_and_the_carrier() {
    let out = html(
        stages()
            .required()
            .disabled(true)
            .invalid(true)
            .size(Size::Sm),
    );
    assert_eq!(
        attr_of(&out, "data-slot=\"select-trigger\"", "aria-required"),
        Some("true")
    );
    assert!(
        out.contains(r#"<select data-slot="select-native" name="stage" required disabled"#),
        "{out}"
    );
    assert!(out.contains(r#"data-size="sm""#));
    assert!(out.contains("data-invalid"));
    assert!(out.contains("data-disabled"));
}

#[test]
fn clearable_adds_a_clear_button_that_is_hidden_until_there_is_a_value() {
    let tag = |html: &str| {
        let at = html
            .find("data-slot=\"select-clear\"")
            .expect("a clear button");
        let start = html[..at].rfind('<').unwrap();
        html[start..start + html[start..].find('>').unwrap()].to_owned()
    };
    assert!(tag(&html(stages().clearable())).contains(" hidden"));
    assert!(!tag(&html(stages().clearable().selected("lead"))).contains(" hidden"));
    // A select that is not clearable has no clear button.
    assert!(!html(stages()).contains("select-clear"));
}

#[test]
fn the_labels_travel_to_the_runtime_as_json_in_the_pages_language() {
    let labels = SelectLabels {
        clear: "Effacer".into(),
        ..SelectLabels::default()
    };
    let out = html(stages().clearable().labels(&labels));
    assert!(out.contains("Effacer"), "{out}");
    assert!(out.contains("data-l="));
    // English is the runtime's own, so a page in English carries no words at all.
    assert!(!html(stages().clearable()).contains("data-l="));
}

#[test]
fn the_popup_is_placed_by_the_overlay_module() {
    let out = html(stages().side(Side::Top));
    assert!(out.contains("popover=\"manual\""), "{out}");
    assert!(out.contains("data-ui-place"));
    assert!(out.contains(r#"data-side="top""#));
    let popup = attr_of(&out, "data-slot=\"select-popup\"", "id").unwrap();
    assert_eq!(
        attr_of(&out, "data-ui-anchor", "data-ui-anchor"),
        Some(popup)
    );
}

#[test]
fn a_page_with_ten_thousand_choices_renders_fast_enough_and_small_enough() {
    let mut select = Select::new("big");
    for at in 0..10_000 {
        select = select.option(format!("v{at}"), format!("Choice number {at}"));
    }
    let out = html(select);
    // The listbox and the carrier each hold them once.
    assert_eq!(out.matches("role=\"option\"").count(), 10_000);
    assert_eq!(out.matches("<option ").count(), 10_001);
    // About 75 bytes per choice in the list and 40 in the carrier.
    assert!(out.len() < 1_400_000, "{}", out.len());
}

#[test]
fn a_long_list_people_search_draws_only_its_first_rows_and_the_chosen_one() {
    let mut combo = Combobox::new("big");
    for at in 0..10_000 {
        combo = combo.option(format!("v{at}"), format!("Choice {at}"));
    }
    let out = html(combo.selected("v5000"));
    assert_eq!(out.matches(r#"role="option""#).count(), 10_000);
    // 200 shown, the chosen one shown wherever it stands, the rest waiting.
    assert_eq!(
        out.matches(r#" hidden dir="auto""#).count(),
        10_000 - 200 - 1,
        "{}",
        out.len()
    );
    assert!(out.contains(r#"data-value="v5000" aria-selected="true" dir="auto""#));
    // A select shows every row: it is for a short list, and typeahead needs them all.
    let select =
        html(Select::new("s").options((0..300).map(|n| (format!("v{n}"), format!("C{n}")))));
    assert!(!select.contains(r#" hidden dir="auto""#));
}

// -------------------------------------------------------------- Combobox

#[test]
fn a_combobox_is_an_input_with_the_chosen_label_and_a_carrier_that_posts_the_value() {
    let out = html(
        Combobox::new("owner")
            .placeholder("Search people")
            .option("1", "Anu Lakhotia")
            .option("2", "Arun Tiwari")
            .selected("2"),
    );
    assert!(out.contains(r#"role="combobox""#), "{out}");
    assert!(out.contains(r#"aria-autocomplete="list""#));
    assert!(out.contains(r#"autocomplete="off""#));
    assert!(out.contains(r#"value="Arun Tiwari""#));
    assert!(out.contains(r#"placeholder="Search people""#));
    assert!(out.contains(r#"<option value="2" selected>Arun Tiwari</option>"#));
    // The input is not posted: it has no name.
    assert!(attr_of(&out, "data-slot=\"combobox-input\"", "name").is_none());
    assert_eq!(
        attr_of(&out, "data-slot=\"combobox-input\"", "dir"),
        Some("auto")
    );
}

#[test]
fn a_searched_combobox_names_its_endpoint_and_carries_only_the_chosen_option() {
    let out = html(
        Combobox::new("owner")
            .search("/people/options")
            .min_chars(2)
            .debounce(300)
            .selected_choice(Choice::new("7", "Gauri Hundal")),
    );
    assert_eq!(
        attr_of(&out, "data-ui-picker", "data-ui-search"),
        Some("/people/options")
    );
    assert_eq!(attr_of(&out, "data-ui-picker", "data-ui-min"), Some("2"));
    assert_eq!(
        attr_of(&out, "data-ui-picker", "data-ui-debounce"),
        Some("300")
    );
    assert!(
        out.contains(r#"<option value="7" selected>Gauri Hundal</option>"#),
        "{out}"
    );
    assert_eq!(
        out.matches("<option ").count(),
        2,
        "the empty choice and the chosen one: {out}"
    );
    assert!(!out.contains("javascript:"));
}

#[test]
fn no_wait_is_written_as_zero_and_the_default_is_left_to_the_runtime() {
    let none = html(Combobox::new("o").search("/o").debounce(0));
    assert_eq!(
        attr_of(&none, "data-ui-picker", "data-ui-debounce"),
        Some("0"),
        "a zero wait is kept, not turned into the default: {none}"
    );
    let default = html(Combobox::new("o").search("/o"));
    assert_eq!(
        attr_of(&default, "data-ui-picker", "data-ui-debounce"),
        None,
        "{default}"
    );
    let long = html(MultiSelect::new("o").search("/o").debounce(u32::MAX));
    assert_eq!(
        attr_of(&long, "data-ui-picker", "data-ui-debounce"),
        Some("5000"),
        "a wait is capped: {long}"
    );
}

#[test]
fn the_search_url_goes_through_the_href_check() {
    let out = html(Combobox::new("o").search("javascript:alert(1)"));
    assert!(!out.contains("javascript"), "{out}");
}

#[test]
fn creating_is_off_unless_asked_and_can_be_switched_off_with_a_reason() {
    assert!(!html(Combobox::new("t")).contains("data-ui-create"));
    let free = html(Combobox::new("t").creatable());
    assert_eq!(
        attr_of(&free, "data-ui-picker", "data-ui-create"),
        Some("free")
    );
    let made = html(Combobox::new("t").create_url("/tags"));
    assert_eq!(
        attr_of(&made, "data-ui-picker", "data-ui-create"),
        Some("/tags")
    );
    let denied = html(
        Combobox::new("t")
            .creatable()
            .create_denied("Only admins can add tags."),
    );
    assert!(
        denied.contains("data-ui-create-denied=\"Only admins can add tags.\""),
        "{denied}"
    );
}

#[test]
fn a_combobox_in_a_field_takes_the_id_on_its_input_not_on_the_carrier_or_the_hidden_parts() {
    let out =
        html(Field::new("deal-owner", "Owner").control(Combobox::new("owner").option("1", "Anu")));
    assert_eq!(
        attr_of(&out, "data-slot=\"combobox-input\"", "id"),
        Some("deal-owner")
    );
    assert!(attr_of(&out, "data-slot=\"select-native\"", "id").is_none());
}

#[test]
fn a_combobox_asks_for_the_select_and_overlay_modules() {
    let uses = View::new("t")
        .body(Combobox::new("o").option("1", "A"))
        .uses();
    assert!(uses.has(Uses::SELECT) && uses.has(Uses::OVERLAY));
}

// ----------------------------------------------------------- MultiSelect

#[test]
fn a_multi_select_has_a_chip_for_each_chosen_value_and_a_multiple_carrier() {
    let out = html(
        MultiSelect::new("tags")
            .option("a", "Alpha")
            .option("b", "Beta")
            .option("c", "Gamma")
            .selected_values(["a", "c"]),
    );
    let before_template = out.split("<template").next().unwrap();
    assert_eq!(
        before_template.matches(r#"data-slot="chip" "#).count(),
        2,
        "{out}"
    );
    assert!(out.contains(r#"aria-label="Remove Alpha""#), "{out}");
    assert!(out.contains(r#"<select data-slot="select-native" name="tags" multiple"#));
    assert!(out.contains(r#"<option value="a" selected>Alpha</option>"#));
    assert!(out.contains(r#"<option value="b">Beta</option>"#));
    assert!(out.contains(r#"aria-multiselectable="true""#));
    assert!(out.contains(r#"<template data-slot="chip-template">"#));
}

#[test]
fn a_chip_is_a_rectangle_with_a_text_and_a_remove_button_never_a_pill() {
    let out = html(
        MultiSelect::new("tags")
            .option("a", "Alpha")
            .selected_values(["a"]),
    );
    assert!(out.contains(r#"data-slot="chip-remove""#), "{out}");
    assert!(out.contains(r#"data-slot="chip-text""#));
}

#[test]
fn max_summary_and_select_all_reach_the_runtime() {
    let out = html(
        MultiSelect::new("tags")
            .option("a", "A")
            .max(3)
            .summary_after(2)
            .select_all(),
    );
    assert_eq!(attr_of(&out, "data-ui-picker", "data-ui-max"), Some("3"));
    assert_eq!(
        attr_of(&out, "data-ui-picker", "data-ui-summary"),
        Some("2")
    );
    assert!(out.contains("data-ui-pick-all"), "{out}");
}

#[test]
fn chosen_values_that_are_not_options_are_still_carried_so_a_form_does_not_lose_them() {
    let out = html(
        MultiSelect::new("tags")
            .option("a", "A")
            .selected_values(["a", "gone"]),
    );
    assert!(
        out.contains(r#"<option value="gone" selected>gone</option>"#),
        "{out}"
    );
}

#[test]
fn a_multi_select_posts_every_chosen_value_under_one_name_and_none_when_empty() {
    let out = html(MultiSelect::new("tags").option("a", "A"));
    assert!(out.contains(r#"<option value="a">A</option>"#));
    assert!(!out.contains(r#"<option value="a" selected"#), "{out}");
}

// --------------------------------------------------------------- Command

fn palette() -> Command {
    Command::new("palette")
        .placeholder("Type a command")
        .group(
            CommandGroup::new("Navigate")
                .item(
                    CommandItem::link("Deals", "/deals")
                        .icon(icons::SEARCH)
                        .shortcut("mod+d"),
                )
                .item(CommandItem::link("Contacts", "/contacts").keywords(["people"])),
        )
        .group(
            CommandGroup::new("Account")
                .item(CommandItem::post("Sign out", "/sign-out"))
                .item(CommandItem::disabled(
                    "Billing",
                    "Only owners can open billing.",
                )),
        )
        .recent(5)
        .source("/palette/search")
}

#[test]
fn a_palette_is_a_modal_dialog_with_a_combobox_and_a_listbox() {
    let out = html(palette());
    assert!(out.contains("<dialog"), "{out}");
    assert!(out.contains(r#"data-slot="command""#));
    assert!(out.contains(r#"role="combobox""#));
    assert!(out.contains(r#"role="listbox""#));
    assert!(out.contains(r#"aria-label="Navigate""#));
}

#[test]
fn items_are_links_forms_or_commands_and_all_of_them_are_options() {
    let out = html(palette());
    assert!(
        out.contains(r#"<a role="option" href="/deals""#)
            || out.contains(r#"role="option" href="/deals""#),
        "{out}"
    );
    assert!(out.contains(r#"formaction="/sign-out""#), "{out}");
    assert!(out.contains(r#"formmethod="post""#));
    let run = html(
        Command::new("p").group(CommandGroup::new("G").item(CommandItem::run(
            "Open filters",
            navree_ui::Cmd::open("filters"),
        ))),
    );
    assert!(run.contains("data-ui-click="), "{run}");
}

#[test]
fn a_disabled_item_says_why() {
    let out = html(palette());
    assert!(out.contains(r#"aria-disabled="true""#), "{out}");
    assert!(out.contains("Only owners can open billing."));
}

#[test]
fn a_shortcut_is_shown_for_the_platform_and_named_for_the_runtime() {
    let out = html(palette());
    assert!(out.contains(r#"data-shortcut="mod+d""#), "{out}");
    assert!(out.contains(r#"data-slot="command-shortcut""#));
}

#[test]
fn a_shortcut_with_no_modifier_is_refused() {
    let out = html(
        Command::new("p")
            .group(CommandGroup::new("G").item(CommandItem::link("Home", "/").shortcut("h"))),
    );
    assert!(
        !out.contains("data-shortcut"),
        "a bare letter would fire while typing: {out}"
    );
}

#[test]
fn recent_and_source_and_the_opening_shortcut_are_named_on_the_dialog() {
    let out = html(palette());
    assert_eq!(
        attr_of(&out, "data-ui-palette", "data-ui-recent"),
        Some("5")
    );
    assert_eq!(
        attr_of(&out, "data-ui-palette", "data-ui-search"),
        Some("/palette/search")
    );
    assert!(out.contains("data-ui-key"), "Cmd or Ctrl+K opens it: {out}");
    assert!(!html(palette().shortcut(false)).contains("data-ui-key"));
}

#[test]
fn the_trigger_opens_the_dialog_without_script() {
    let trigger = html(Command::trigger("palette", "Search"));
    assert!(trigger.contains(r#"command="show-modal""#), "{trigger}");
    assert!(trigger.contains(r#"commandfor="palette""#));
}

#[test]
fn a_palette_page_loads_the_select_module() {
    assert!(View::new("t").body(palette()).uses().has(Uses::SELECT));
}

// ------------------------------------------------------- the endpoint types

#[test]
fn a_query_clips_text_caps_the_limit_and_the_offset_and_never_fails() {
    let q = OptionQuery::parse("q=anu&limit=500&after=99999999999");
    assert_eq!(q.text(), "anu");
    assert_eq!(q.limit(), 50);
    assert_eq!(q.offset(), 1_000_000);
    let long = format!("q={}", "x".repeat(5000));
    assert_eq!(OptionQuery::parse(&long).text().len(), 256);
    let bad = OptionQuery::parse("limit=-3&after=zzz&q=%ff%fe");
    assert_eq!(bad.limit(), 20);
    assert_eq!(bad.offset(), 0);
    assert!(OptionQuery::parse("").text().is_empty());
}

#[test]
fn text_is_clipped_on_a_character_boundary() {
    let text: String = "é".repeat(200);
    let q = OptionQuery::parse(&format!("q={text}"));
    assert!(q.text().len() <= 256);
    assert!(q.text().chars().all(|c| c == 'é'));
}

#[test]
fn the_option_list_is_the_same_markup_as_an_inline_list() {
    let list = ChoiceList::new()
        .option("1", "Anu")
        .group("Others", [("2", "Arun")])
        .more("/people/options?q=a&after=2");
    let out = list.to_html();
    assert!(out.contains(r#"role="option" data-value="1""#), "{out}");
    assert!(out.contains(r#"role="group" aria-label="Others""#));
    assert!(
        out.contains(r#"data-more="/people/options?q=a&amp;after=2""#),
        "{out}"
    );
}

#[test]
fn an_option_list_escapes_what_it_holds() {
    let out = ChoiceList::new()
        .option("<b>", "<script>alert(1)</script>")
        .to_html();
    assert!(!out.contains("<script"), "{out}");
}

#[test]
fn an_option_list_answers_html_that_is_not_stored() {
    use navree_http::response::IntoResponse;
    let response = ChoiceList::new().option("1", "Anu").into_response();
    assert_eq!(
        response.headers()["content-type"],
        "text/html; charset=utf-8"
    );
    assert_eq!(response.headers()["cache-control"], "no-store");
}

#[test]
fn a_more_link_must_stay_on_this_site() {
    let out = ChoiceList::new().more("https://evil.example/x").to_html();
    assert!(!out.contains("evil.example"), "{out}");
}

#[test]
fn control_characters_are_taken_out_of_a_search_text() {
    for (query, text) in [
        ("q=a%00b", "ab"),
        ("q=%00", ""),
        ("q=%00%00x%00", "x"),
        ("q=a%0Ab%09c%1Fd%7F", "abcd"),
        ("q=%C2%85gr%C2%9Dace", "gauri"),
        ("q=+%00+anu+%00+", "anu"),
    ] {
        assert_eq!(OptionQuery::parse(query).text(), text, "{query}");
    }
    assert!(OptionQuery::parse("q=%00").is_empty());
    let next = OptionQuery::parse("q=a%00b").next_url("/people", 20);
    assert!(!next.contains("%00"), "{next}");
}

#[test]
fn a_palette_with_a_post_item_posts_through_the_form_runtime_and_one_without_loads_nothing_more() {
    let with_post = || {
        Command::new("palette")
            .group(
                CommandGroup::new("Actions")
                    .item(CommandItem::post("Sign out", "/sign-out"))
                    .item(CommandItem::link("Home", "/")),
            )
            .csrf("page-token")
    };
    let out = html(with_post());
    let form_at = out.find(r#"id="palette-post""#).expect("the hidden form");
    let form_tag =
        &out[out[..form_at].rfind('<').unwrap()..form_at + out[form_at..].find('>').unwrap()];
    assert!(
        form_tag.contains("data-ui-form"),
        "the runtime sends it: {form_tag}"
    );
    assert!(
        out.contains(r#"name="_csrf" value="page-token""#),
        "and with script off the token rides in the body: {out}"
    );
    let uses = View::new("t").body(with_post()).uses();
    assert!(
        uses.has(Uses::CORE),
        "the form runtime is loaded for a post item"
    );
    assert!(uses.has(Uses::SELECT) && uses.has(Uses::OVERLAY));

    let only_links = || {
        Command::new("palette").group(CommandGroup::new("Go").item(CommandItem::link("Home", "/")))
    };
    let out = html(only_links());
    assert!(!out.contains("data-ui-form"), "{out}");
    assert!(!View::new("t").body(only_links()).uses().has(Uses::CORE));
}

#[test]
fn the_link_to_the_next_page_goes_on_a_query_the_path_already_has() {
    let plain = OptionQuery::parse("q=anu").next_url("/people", 20);
    assert_eq!(plain, "/people?q=anu&limit=20&after=20");
    let with_query = OptionQuery::parse("q=anu").next_url("/people?team=7", 20);
    assert_eq!(with_query, "/people?team=7&q=anu&limit=20&after=20");
}
