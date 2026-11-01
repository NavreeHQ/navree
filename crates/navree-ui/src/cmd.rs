//! [`Cmd`]: what an element can do in the browser with no request to the
//! server. The list is closed: commands are data in an attribute, the runtime
//! interprets them, and no expression or `eval` is involved.

use serde_json::{Value, json};

/// One browser-side command.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Cmd {
    /// Toggles `hidden` on the element with this id.
    Toggle(String),
    /// Removes `hidden`.
    Show(String),
    /// Sets `hidden`.
    Hide(String),
    /// Opens a `<dialog>`, a popover or a `<details>`.
    Open(String),
    /// Closes a `<dialog>`, a popover or a `<details>`.
    Close(String),
    /// Copies text to the clipboard.
    Copy(String),
    /// Sets an attribute. `on*`, `style` and URL attributes are refused.
    SetAttr {
        /// The target's id.
        target: String,
        /// The attribute.
        name: &'static str,
        /// Its value.
        value: String,
    },
    /// Removes an attribute.
    RemoveAttr {
        /// The target's id.
        target: String,
        /// The attribute.
        name: &'static str,
    },
    /// Toggles an attribute with no value (`data-open`).
    ToggleAttr {
        /// The target's id.
        target: String,
        /// The attribute.
        name: &'static str,
    },
    /// Sends a custom event from the element, for an island to hear.
    Dispatch(&'static str),
    /// Moves focus to the element with this id.
    Focus(String),
    /// Submits the form with this id.
    Submit(String),
}

impl Cmd {
    /// Toggles `hidden` on `id`.
    pub fn toggle(id: impl Into<String>) -> Self {
        Self::Toggle(id.into())
    }

    /// Removes `hidden` from `id`.
    pub fn show(id: impl Into<String>) -> Self {
        Self::Show(id.into())
    }

    /// Sets `hidden` on `id`.
    pub fn hide(id: impl Into<String>) -> Self {
        Self::Hide(id.into())
    }

    /// Opens the dialog, popover or details `id`.
    pub fn open(id: impl Into<String>) -> Self {
        Self::Open(id.into())
    }

    /// Closes the dialog, popover or details `id`.
    pub fn close(id: impl Into<String>) -> Self {
        Self::Close(id.into())
    }

    /// Copies `text`.
    pub fn copy(text: impl Into<String>) -> Self {
        Self::Copy(text.into())
    }

    /// Sets attribute `name` of `target`.
    pub fn set_attr(target: impl Into<String>, name: &'static str, value: impl Into<String>) -> Self {
        Self::SetAttr { target: target.into(), name, value: value.into() }
    }

    /// Removes attribute `name` of `target`.
    pub fn remove_attr(target: impl Into<String>, name: &'static str) -> Self {
        Self::RemoveAttr { target: target.into(), name }
    }

    /// Toggles attribute `name` of `target`.
    pub fn toggle_attr(target: impl Into<String>, name: &'static str) -> Self {
        Self::ToggleAttr { target: target.into(), name }
    }

    /// Sends the custom event `name`.
    pub fn dispatch(name: &'static str) -> Self {
        Self::Dispatch(name)
    }

    /// Moves focus to `id`.
    pub fn focus(id: impl Into<String>) -> Self {
        Self::Focus(id.into())
    }

    /// Submits the form `id`.
    pub fn submit(id: impl Into<String>) -> Self {
        Self::Submit(id.into())
    }

    fn to_value(&self) -> Value {
        match self {
            Self::Toggle(id) => json!(["toggle", id]),
            Self::Show(id) => json!(["show", id]),
            Self::Hide(id) => json!(["hide", id]),
            Self::Open(id) => json!(["open", id]),
            Self::Close(id) => json!(["close", id]),
            Self::Copy(text) => json!(["copy", text]),
            Self::SetAttr { target, name, value } => json!(["set_attr", target, name, value]),
            Self::RemoveAttr { target, name } => json!(["remove_attr", target, name]),
            Self::ToggleAttr { target, name } => json!(["toggle_attr", target, name]),
            Self::Dispatch(name) => json!(["dispatch", name]),
            Self::Focus(id) => json!(["focus", id]),
            Self::Submit(id) => json!(["submit", id]),
        }
    }
}

/// A list of commands, run in order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Commands(pub Vec<Cmd>);

impl Commands {
    /// The list as the JSON the runtime reads.
    pub fn to_json(&self) -> String {
        Value::Array(self.0.iter().map(Cmd::to_value).collect()).to_string()
    }
}

impl From<Cmd> for Commands {
    fn from(command: Cmd) -> Self {
        Self(vec![command])
    }
}

impl From<Vec<Cmd>> for Commands {
    fn from(commands: Vec<Cmd>) -> Self {
        Self(commands)
    }
}

/// What sets commands off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    /// A click (or Enter or Space on a button).
    Click,
    /// A value changed.
    Change,
    /// Typing, on every keystroke.
    Input,
}

impl Trigger {
    pub(crate) const fn attribute(self) -> &'static str {
        match self {
            Self::Click => "data-ui-click",
            Self::Change => "data-ui-change",
            Self::Input => "data-ui-input",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_are_compact_json() {
        let commands = Commands(vec![
            Cmd::open("d1"),
            Cmd::set_attr("x", "data-open", "1"),
            Cmd::copy("a\"b"),
        ]);
        assert_eq!(
            commands.to_json(),
            r#"[["open","d1"],["set_attr","x","data-open","1"],["copy","a\"b"]]"#
        );
    }
}
