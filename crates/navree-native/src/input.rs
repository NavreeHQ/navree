//! [`Input`]: the control of a form's field.
//!
//! A client draws each with its system's own control and sends the value under
//! the field's name, exactly as a browser's form would.

use serde::{Deserialize, Serialize};

use crate::node::{is_default, is_false};

/// The control of a field.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Input {
    /// Typed text.
    Text(TextInput),
    /// On or off.
    Toggle(ToggleInput),
    /// One of several.
    Choice(ChoiceInput),
    /// Any of several.
    Choices(ChoicesInput),
    /// A date, a time or both.
    Date(DateInput),
    /// A number between two ends, by dragging.
    Slider(SliderInput),
    /// A colour.
    Colour(ColourInput),
    /// Files, from the device or its camera.
    File(FileInput),
    /// A control from a newer server, which this build does not know. A client
    /// leaves the field out; its form still sends the rest.
    #[serde(other)]
    Unknown,
}

/// What typed text is, so a client shows the right keyboard and help.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum TextKind {
    /// Any text.
    #[default]
    Plain,
    /// An email address.
    Email,
    /// A password: hidden as it is typed.
    Password,
    /// A whole number.
    Number,
    /// A number with a fraction.
    Decimal,
    /// A phone number.
    Phone,
    /// A web address.
    Url,
    /// A search.
    Search,
    /// Several lines.
    Multiline,
    /// A code sent to the user, which a system can fill in.
    OneTimeCode,
}

/// Typed text.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextInput {
    /// What the text is.
    #[serde(default, skip_serializing_if = "is_default")]
    pub kind: TextKind,
    /// What it holds now.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub value: String,
    /// What it shows while empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    /// The most characters it takes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_length: Option<u32>,
    /// The least a number may be, as the page wrote it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<String>,
    /// The most a number may be, as the page wrote it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<String>,
    /// The step between numbers, as the page wrote it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    /// What a system may fill it with, by the web's own names (`email`,
    /// `current-password`, `one-time-code`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub autofill: Option<String>,
    /// Whether it can be read and not changed. Its value is still sent.
    #[serde(default, skip_serializing_if = "is_false")]
    pub readonly: bool,
}

/// How an on-or-off control is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToggleKind {
    /// A switch: it takes effect as it flips.
    #[default]
    Switch,
    /// A checkbox: it is sent with its form.
    Checkbox,
}

/// On or off.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToggleInput {
    /// How it is drawn.
    #[serde(default, skip_serializing_if = "is_default")]
    pub kind: ToggleKind,
    /// Whether it is on.
    #[serde(default, skip_serializing_if = "is_false")]
    pub on: bool,
    /// What is sent when it is on (`on` when the page gave none). Nothing is sent
    /// when it is off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

/// How a choice of one is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChoiceKind {
    /// A menu that opens.
    #[default]
    Menu,
    /// A few choices side by side.
    Segmented,
    /// Radio buttons.
    Radio,
    /// A list that is searched, for many choices.
    List,
}

/// One thing that can be chosen.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChoiceOption {
    /// What is sent.
    pub value: String,
    /// What the reader sees.
    pub label: String,
    /// A few words beside the label: how many records hold it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Whether it can't be chosen now.
    #[serde(default, skip_serializing_if = "is_false")]
    pub disabled: bool,
}

/// One of several.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChoiceInput {
    /// How it is drawn.
    #[serde(default, skip_serializing_if = "is_default")]
    pub kind: ChoiceKind,
    /// What can be chosen.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<ChoiceOption>,
    /// What is chosen now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// What it shows while nothing is chosen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
}

/// Any of several. Each chosen value is sent under the field's name.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChoicesInput {
    /// What can be chosen.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<ChoiceOption>,
    /// What is chosen now.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<String>,
}

/// What a date control picks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DateKind {
    /// A day, sent as `2026-11-12`.
    #[default]
    Date,
    /// A time of day, sent as `09:41`.
    Time,
    /// A day and a time, sent as `2026-11-12T09:41`.
    Datetime,
}

/// A date, a time or both, in the forms the web's own fields send.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DateInput {
    /// What it picks.
    #[serde(default, skip_serializing_if = "is_default")]
    pub kind: DateKind,
    /// What it holds now.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub value: String,
    /// The earliest it takes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<String>,
    /// The latest it takes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<String>,
}

/// A number between two ends. The numbers are text, as the page wrote them, so
/// a fraction is exact.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SliderInput {
    /// The least.
    pub min: String,
    /// The most.
    pub max: String,
    /// The step between values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    /// What it holds now.
    pub value: String,
}

/// A colour, sent as `#rrggbb`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColourInput {
    /// What it holds now.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub value: String,
}

/// Files. A field of this kind makes its form a multipart post.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileInput {
    /// The kinds of file it takes, by the web's own list (`image/*,.pdf`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accept: Option<String>,
    /// Whether it takes more than one.
    #[serde(default, skip_serializing_if = "is_false")]
    pub multiple: bool,
    /// The camera it asks for, by the web's own names (`user`, `environment`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_input_is_named_by_its_kind_with_only_what_it_holds() {
        let input = Input::Text(TextInput {
            kind: TextKind::Number,
            value: "24".to_owned(),
            min: Some("1".to_owned()),
            max: Some("500".to_owned()),
            ..TextInput::default()
        });
        let json = serde_json::to_string(&input).unwrap();
        assert_eq!(
            json,
            r#"{"t":"text","kind":"number","value":"24","min":"1","max":"500"}"#
        );
        assert_eq!(serde_json::from_str::<Input>(&json).unwrap(), input);

        let choice = Input::Choice(ChoiceInput {
            options: vec![
                ChoiceOption {
                    value: "returned".to_owned(),
                    label: "Returned by the customer".to_owned(),
                    ..ChoiceOption::default()
                },
                ChoiceOption {
                    value: "late".to_owned(),
                    label: "Arrived late".to_owned(),
                    ..ChoiceOption::default()
                },
            ],
            value: Some("returned".to_owned()),
            ..ChoiceInput::default()
        });
        let json = serde_json::to_string(&choice).unwrap();
        assert_eq!(
            json,
            concat!(
                r#"{"t":"choice","options":[{"value":"returned","label":"Returned by the customer"},"#,
                r#"{"value":"late","label":"Arrived late"}],"value":"returned"}"#
            )
        );
        assert_eq!(serde_json::from_str::<Input>(&json).unwrap(), choice);
    }

    #[test]
    fn a_control_from_a_newer_server_is_read_as_unknown() {
        assert_eq!(
            serde_json::from_str::<Input>(r#"{"t":"signature","pen":"fine"}"#).unwrap(),
            Input::Unknown
        );
    }
}
