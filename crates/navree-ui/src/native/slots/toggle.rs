//! `ToggleGroup`: a few choices side by side, one on or several.
//!
//! A group of toggles is a group of radio buttons (or boxes) in labels, drawn as
//! buttons. It posts as one field, so it is read as one: a segmented choice of one,
//! or a choice of several.

use std::borrow::Cow;

use navree_native::input as i;
use navree_native::node as n;

use super::super::lower::{Cx, N, Read, attr, elements, fail, flat, has, hooks, id, slot, text_of};
use crate::node::{Element, Node};

/// The hooks the root of a toggle group carries that this reader answers for.
pub(crate) const HOOKS: &[&str] = &["data-ui-toggle-group"];

/// `ToggleGroup`.
pub(crate) fn toggle_group(_: &mut Cx, element: &Element) -> Read {
    let several = attr(element, "role").as_deref() == Some("group");
    let mut name = None;
    let mut options = Vec::new();
    let mut chosen = Vec::new();
    for item in elements(element)? {
        if slot(item).as_deref() != Some("toggle-group-item") || item.tag() != "label" {
            return fail("something that is not a toggle in a group of them");
        }
        hooks(item, &[])?;
        let inputs: Vec<&Element> = flat(item.child_nodes())
            .into_iter()
            .filter_map(|node| match node {
                Node::Element(part) if part.tag() == "input" => Some(part),
                _ => None,
            })
            .collect();
        let [input] = inputs.as_slice() else {
            return fail("a toggle with no control or with more than one");
        };
        hooks(input, &[])?;
        let kind = attr(input, "type");
        if kind.as_deref() != Some(if several { "checkbox" } else { "radio" }) {
            return fail("a toggle whose control is not the group's kind");
        }
        let this = attr(input, "name").map(Cow::into_owned).unwrap_or_default();
        if name.get_or_insert_with(|| this.clone()) != &this {
            return fail("toggles of a group that post under different names");
        }
        let value = attr(input, "value")
            .map(Cow::into_owned)
            .unwrap_or_default();
        if has(input, "checked") {
            chosen.push(value.clone());
        }
        // The words of a toggle, or the name an icon alone was given.
        let words = text_of(item);
        let label = if words.is_empty() {
            attr(item, "aria-label")
                .or_else(|| attr(input, "aria-label"))
                .map(Cow::into_owned)
                .unwrap_or_default()
        } else {
            words
        };
        if label.is_empty() {
            return fail("a toggle with no words and no name");
        }
        options.push(i::ChoiceOption {
            value,
            label,
            detail: None,
            disabled: has(input, "disabled") || has(item, "data-disabled"),
        });
    }
    let Some(name) = name else {
        return fail("a group of toggles with none in it");
    };
    let input = if several {
        i::Input::Choices(i::ChoicesInput {
            options,
            values: chosen,
            ..i::ChoicesInput::default()
        })
    } else {
        i::Input::Choice(i::ChoiceInput {
            kind: i::ChoiceKind::Segmented,
            options,
            value: chosen.into_iter().next(),
            placeholder: None,
            ..i::ChoiceInput::default()
        })
    };
    Ok(vec![N::Field(n::Field {
        id: id(element),
        name,
        label: attr(element, "aria-label").map(Cow::into_owned),
        help: None,
        error: None,
        required: false,
        disabled: has(element, "data-disabled"),
        input,
    })])
}

#[cfg(test)]
mod tests {
    use super::super::testing::{misses, read, tamper};
    use super::*;
    use crate::node::Node;
    use crate::{ToggleGroup, ToggleItem};

    fn field_of(node: N) -> n::Field {
        let N::Field(field) = node else {
            panic!("not a field: {node:?}");
        };
        field
    }

    #[test]
    fn a_group_of_one_is_a_segmented_choice() {
        let field = field_of(read(
            ToggleGroup::single("view", "Show as")
                .item(ToggleItem::new("list", "List"))
                .item(ToggleItem::new("grid", "Grid"))
                .value("grid"),
        ));
        assert_eq!(field.name, "view");
        assert_eq!(field.label.as_deref(), Some("Show as"));
        let i::Input::Choice(choice) = &field.input else {
            panic!("not a choice");
        };
        assert_eq!(choice.kind, i::ChoiceKind::Segmented);
        assert_eq!(choice.value.as_deref(), Some("grid"));
        let labels: Vec<&str> = choice
            .options
            .iter()
            .map(|option| option.label.as_str())
            .collect();
        assert_eq!(labels, ["List", "Grid"]);
    }

    #[test]
    fn a_group_of_several_is_a_choice_of_several_and_an_option_can_be_off() {
        let field = field_of(read(
            ToggleGroup::multiple("tags", "Tags")
                .item(ToggleItem::new("a", "A"))
                .item(ToggleItem::new("b", "B").disabled(true))
                .value("a"),
        ));
        let i::Input::Choices(choices) = &field.input else {
            panic!("not choices");
        };
        assert_eq!(choices.values, ["a"]);
        assert!(choices.options[1].disabled);
    }

    #[test]
    fn an_icon_alone_is_named_by_what_the_page_says_it_is() {
        let field = field_of(read(
            ToggleGroup::single("view", "Show as")
                .item(ToggleItem::icon_only(
                    "grid",
                    crate::icons::LAYOUT_GRID,
                    "Grid",
                ))
                .value("grid"),
        ));
        let i::Input::Choice(choice) = &field.input else {
            panic!("not a choice");
        };
        assert_eq!(choice.options[0].label, "Grid");
    }

    #[test]
    fn a_hook_no_reader_knows_inside_a_toggle_group_is_still_a_miss() {
        for name in ["toggle-group", "toggle-group-item"] {
            let node: Node = ToggleGroup::single("view", "Show as")
                .item(ToggleItem::new("list", "List"))
                .into();
            assert_eq!(
                misses(tamper(node, name, "data-ui-wizard")),
                ["hook:data-ui-wizard"],
                "{name}"
            );
        }
    }
}
