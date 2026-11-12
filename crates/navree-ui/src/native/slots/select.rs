//! `Select`, `Combobox` and `MultiSelect`: a field that chooses among
//! options.
//!
//! The rich control (a button that opens a listbox, an input that filters it,
//! chips) is for a browser with scripts. Under it the page keeps a plain
//! `<select>` that carries the name, the options and what is chosen; that is
//! what a form would send, and what is read here. The listbox only adds a
//! line of description to an option, which is taken from it.
//!
//! A combobox whose options are searched on the server holds the options the
//! page began with; the address that searches is not in the vocabulary yet, so
//! the field says so in what it leaves out.

use std::borrow::Cow;
use std::collections::HashMap;

use navree_native::input as i;
use navree_native::node as n;

use super::super::lower::{
    Control, Cx, N, Parts, Read, attr, elements, fail, flat, has, hooks, slot, text_of,
};
use crate::node::{Element, Node};

/// The hooks the root of a picker carries that this reader answers for.
pub(crate) const HOOKS: &[&str] = &[
    "data-ui-picker",
    "data-ui-clearable",
    "data-ui-search",
    "data-ui-min",
    "data-ui-debounce",
    "data-ui-max",
    "data-ui-summary",
    "data-ui-pick-all",
];

/// A picker: the field its plain `<select>` stands for.
pub(crate) fn picker(cx: &mut Cx, element: &Element) -> Read {
    field(cx, element).map(|field| vec![N::Field(field)])
}

/// The field of a picker.
pub(crate) fn field(cx: &mut Cx, element: &Element) -> Read<n::Field> {
    let kind = slot(element);
    let kind = kind.as_deref().unwrap_or_default();
    if has(element, "data-readonly") {
        return fail("a picker that can be read but not changed");
    }
    let mut parts = Parts::of(element);
    let control = [
        parts.slot("select-control"),
        parts.slot("combobox-control"),
        parts.slot("multiselect-control"),
    ]
    .into_iter()
    .flatten()
    .next();
    let Some(control) = control else {
        return fail("a picker with no control");
    };
    let carrier = parts.slot("select-native");
    let popup = parts.slot("select-popup");
    // What a screen reader is told, and the chip a script copies.
    let _status = parts.slot("select-status");
    let _template = parts.slot("chip-template");
    parts.done()?;
    let Some(carrier) = carrier else {
        return fail("a picker with no select under it");
    };
    let label = label_of(control)?;
    let described = match popup {
        Some(popup) => described(popup)?,
        None => HashMap::new(),
    };
    let Control::Field(mut field) = cx.control(carrier)? else {
        return fail("a picker whose select is not a field");
    };
    field.label = label;
    field.id = super::super::lower::id(element).or(field.id);
    match &mut field.input {
        i::Input::Choice(choice) => {
            choice.kind = if kind == "combobox" {
                i::ChoiceKind::List
            } else {
                i::ChoiceKind::Menu
            };
            describe(&mut choice.options, &described);
        }
        i::Input::Choices(choices) => describe(&mut choices.options, &described),
        _ => return fail("a picker whose select holds no choices"),
    }
    if has(element, "data-ui-search") {
        // The address that searches is not in the vocabulary yet.
        cx.dropped.push("select-search".to_owned());
    }
    Ok(*field)
}

/// The words that name the picker: the label of its button or its input.
fn label_of(control: &Element) -> Read<Option<String>> {
    let mut label = None;
    for part in elements(control)? {
        match slot(part).as_deref() {
            Some("select-trigger" | "combobox-input" | "multiselect-input") => {
                hooks(part, &["data-ui-anchor"])?;
                label = attr(part, "aria-label").map(Cow::into_owned);
            }
            Some("chips") => {
                label = label.or_else(|| attr(part, "aria-label").map(Cow::into_owned));
            }
            // The buttons that clear the choice or open the list.
            Some("select-clear" | "combobox-clear" | "combobox-toggle") => {}
            _ => return fail("a part of a picker's control no reader here knows"),
        }
    }
    control_hooks(control)?;
    Ok(label)
}

/// A control that is a box around its parts anchors the popup: that is all it
/// says of itself.
fn control_hooks(control: &Element) -> Read<()> {
    hooks(control, &["data-ui-anchor"])
}

/// The line under each option's name that the listbox shows, by option value.
/// An option that leads to a page is no choice a field can hold.
fn described(popup: &Element) -> Read<HashMap<String, String>> {
    hooks(
        popup,
        &["data-ui-fit", "data-ui-grow", "data-ui-match-width"],
    )?;
    let mut out = HashMap::new();
    let mut parts = Parts::of(popup);
    if let Some(list) = parts.slot("select-list") {
        rows(list, &mut out)?;
    }
    // The note under the list is for a script's messages.
    let _footer = parts.slot("select-footer");
    parts.done()?;
    Ok(out)
}

fn rows(list: &Element, out: &mut HashMap<String, String>) -> Read<()> {
    for row in flat(list.child_nodes()) {
        let Node::Element(row) = row else { continue };
        if row.tag() == "a" {
            return fail("an option that leads to a page");
        }
        if attr(row, "role").as_deref() == Some("group") {
            rows(row, out)?;
            continue;
        }
        let Some(value) = attr(row, "data-value") else {
            continue;
        };
        if let Some(words) = find(row, "option-description") {
            out.insert(value.into_owned(), text_of(words));
        }
    }
    Ok(())
}

fn find<'a>(element: &'a Element, name: &str) -> Option<&'a Element> {
    crate::native::page::find(element.child_nodes(), name)
}

/// Gives each option the line its listbox row has.
fn describe(options: &mut [i::ChoiceOption], described: &HashMap<String, String>) {
    for option in options {
        if let Some(words) = described.get(&option.value) {
            option.detail = Some(words.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::{misses, read, tamper};
    use super::*;
    use crate::select::Choice;
    use crate::{Combobox, MultiSelect, Select};

    fn field_of(node: N) -> n::Field {
        let N::Field(field) = node else {
            panic!("not a field: {node:?}");
        };
        field
    }

    fn choice_of(field: &n::Field) -> &i::ChoiceInput {
        let i::Input::Choice(choice) = &field.input else {
            panic!("not a choice: {:?}", field.input);
        };
        choice
    }

    #[test]
    fn a_select_is_a_menu_of_its_options_with_what_is_chosen() {
        let field = field_of(read(
            Select::new("status")
                .placeholder("Any status")
                .aria_label("Status")
                .option("open", "Open")
                .option("closed", "Closed")
                .selected("closed"),
        ));
        assert_eq!(field.name, "status");
        assert_eq!(field.label.as_deref(), Some("Status"));
        let choice = choice_of(&field);
        assert_eq!(choice.kind, i::ChoiceKind::Menu);
        assert_eq!(choice.value.as_deref(), Some("closed"));
        assert_eq!(choice.placeholder.as_deref(), Some("Any status"));
        let values: Vec<&str> = choice.options.iter().map(|o| o.value.as_str()).collect();
        assert_eq!(values, ["open", "closed"]);
        assert_eq!(choice.options[1].label, "Closed");
    }

    #[test]
    fn a_select_says_when_it_is_needed_or_switched_off_and_keeps_an_option_that_is_off() {
        let field = field_of(read(
            Select::new("state")
                .required()
                .disabled(true)
                .choice(Choice::new("pb", "Punjab"))
                .choice(Choice::new("hr", "Haryana").disabled(true)),
        ));
        assert!(field.required && field.disabled);
        assert!(choice_of(&field).options[1].disabled);
    }

    #[test]
    fn a_combobox_is_a_list_that_is_searched_and_a_description_comes_from_its_rows() {
        let field = field_of(read(
            Combobox::new("agent")
                .placeholder("Assign to")
                .choice(Choice::new("1", "Gurpreet Sandhu").description("Ludhiana"))
                .choice(Choice::new("2", "Simran Grewal"))
                .selected("2"),
        ));
        let choice = choice_of(&field);
        assert_eq!(choice.kind, i::ChoiceKind::List);
        assert_eq!(choice.value.as_deref(), Some("2"));
        assert_eq!(choice.options[0].detail.as_deref(), Some("Ludhiana"));
        assert_eq!(choice.options[1].detail, None);
    }

    #[test]
    fn a_combobox_searched_on_the_server_reads_the_options_it_began_with_and_says_what_it_left_out()
    {
        let node: Node = Combobox::new("agent")
            .choice(Choice::new("1", "Gurpreet Sandhu"))
            .search("/inbox/agents/options")
            .into();
        let lowered = crate::native::lower(&node);
        assert!(lowered.is_native(), "{:?}", lowered.misses);
        assert_eq!(lowered.dropped, ["select-search"]);
        let N::Field(field) = lowered.node else {
            panic!("not a field");
        };
        assert_eq!(choice_of(&field).kind, i::ChoiceKind::List);
    }

    #[test]
    fn a_picker_in_a_field_takes_the_fields_label_help_and_error() {
        let N::Field(field) = read(
            crate::Field::new("agent-combobox", "Assignee")
                .help("Who answers it")
                .error("Pick someone")
                .control(Combobox::new("agent").option("1", "Gurpreet Sandhu")),
        ) else {
            panic!("not a field");
        };
        assert_eq!(field.name, "agent");
        assert_eq!(field.label.as_deref(), Some("Assignee"));
        assert_eq!(field.help.as_deref(), Some("Who answers it"));
        assert_eq!(field.error.as_deref(), Some("Pick someone"));
    }

    #[test]
    fn several_choices_are_a_field_of_values() {
        let field = field_of(read(
            MultiSelect::new("labels")
                .option("a", "Billing")
                .option("b", "Bug")
                .option("c", "Idea")
                .selected_values(["a", "c"]),
        ));
        let i::Input::Choices(choices) = &field.input else {
            panic!("not choices");
        };
        assert_eq!(choices.values, ["a", "c"]);
        assert_eq!(choices.options.len(), 3);
    }

    #[test]
    fn a_picker_that_creates_options_or_leads_to_pages_or_cannot_be_changed_is_a_miss() {
        let create: Node = Combobox::new("tag").option("a", "A").creatable().into();
        assert_eq!(misses(create), ["hook:data-ui-create"]);
        let links: Node = Select::new("go")
            .choice(Choice::new("a", "A").link("/a"))
            .into();
        assert_eq!(misses(links), ["slot:select"]);
        let locked: Node = Select::new("x").option("a", "A").readonly().into();
        assert_eq!(misses(locked), ["slot:select"]);
    }

    #[test]
    fn a_hook_no_reader_knows_inside_a_picker_is_still_a_miss() {
        let node: Node = Select::new("status").option("a", "A").into();
        assert_eq!(
            misses(tamper(node, "select", "data-ui-wizard")),
            ["hook:data-ui-wizard"]
        );
        let node: Node = Combobox::new("agent").option("a", "A").into();
        assert_eq!(
            misses(tamper(node, "combobox-input", "data-ui-wizard")),
            ["hook:data-ui-wizard"]
        );
        let node: Node = Select::new("status").option("a", "A").into();
        assert_eq!(
            misses(tamper(node, "select-popup", "data-ui-wizard")),
            ["hook:data-ui-wizard"]
        );
    }
}
