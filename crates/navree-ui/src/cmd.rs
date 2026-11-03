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
    /// Sets a state attribute: `data-*` other than the runtime's own `data-ui-*`,
    /// `aria-*`, `hidden`, `open` or `disabled`. Any other name is refused, here and in
    /// the runtime, so a command can't change where a thing points or what it runs.
    SetAttr {
        /// The target's id.
        target: String,
        /// The attribute.
        name: &'static str,
        /// Its value.
        value: String,
    },
    /// Removes a state attribute (see [`Cmd::SetAttr`]).
    RemoveAttr {
        /// The target's id.
        target: String,
        /// The attribute.
        name: &'static str,
    },
    /// Toggles a state attribute with no value, such as `data-open` (see [`Cmd::SetAttr`]).
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
    /// Sets the text of the element with this id: a price when a plan changes, a
    /// label when a choice is made.
    ///
    /// It is text. The runtime sets `textContent`, so nothing in it is read as markup,
    /// and it refuses a `<script>` or `<style>` target, whose text would be code.
    Text {
        /// The target's id.
        target: String,
        /// The text.
        text: String,
    },
    /// Writes how many characters the element's own value holds into the element with
    /// this id, as `12`, or `12/160` with a most. For a field's counter, on
    /// [`Trigger::Input`].
    ///
    /// Characters are counted as the server's `length` rule counts them, by Unicode
    /// scalar value. The runtime writes a number, as text, and nothing else.
    Count {
        /// The target's id.
        target: String,
        /// The most the field takes, written after a slash.
        max: Option<u32>,
    },
    /// Sets the value of a hidden field: the state a form carries, such as the column
    /// a set of buttons sorts by.
    ///
    /// Only `<input type="hidden">` takes it. A field the user types in keeps what was
    /// typed, and a hidden field points at nothing and runs nothing. The value is posted
    /// like any other, so the handler checks it like any other.
    Value {
        /// The field's id.
        target: String,
        /// The value.
        value: String,
    },
    /// Fills the dialog, popover or details with this id from the row the element sits
    /// in, and opens it: a row's details with no request.
    ///
    /// The row is the nearest element with `data-key`. In the dialog, an element marked
    /// with [`Element::fill_cell`](crate::Element::fill_cell) gets the text of that
    /// column's cell, and one marked with [`Element::fill_key`](crate::Element::fill_key)
    /// the row's key; a hidden field gets it as its value, so a form in the dialog can
    /// post it. Everything is copied as text. A key posted this way is a request like
    /// any other: the handler checks the user may act on that row.
    Fill(String),
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

    /// Sets attribute `name` of `target`. A name a command may not touch is a mistake in
    /// the page's code: a debug build stops on it here, and a release build sends nothing.
    pub fn set_attr(
        target: impl Into<String>,
        name: &'static str,
        value: impl Into<String>,
    ) -> Self {
        refused(name);
        Self::SetAttr {
            target: target.into(),
            name,
            value: value.into(),
        }
    }

    /// Removes attribute `name` of `target`, under the same rule as [`Cmd::set_attr`].
    pub fn remove_attr(target: impl Into<String>, name: &'static str) -> Self {
        refused(name);
        Self::RemoveAttr {
            target: target.into(),
            name,
        }
    }

    /// Toggles attribute `name` of `target`, under the same rule as [`Cmd::set_attr`].
    pub fn toggle_attr(target: impl Into<String>, name: &'static str) -> Self {
        refused(name);
        Self::ToggleAttr {
            target: target.into(),
            name,
        }
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

    /// Sets the text of `id` to `text`, as text.
    pub fn text(id: impl Into<String>, text: impl Into<String>) -> Self {
        Self::Text {
            target: id.into(),
            text: text.into(),
        }
    }

    /// Writes how many characters this field holds into `id`, after a slash `max`
    /// when there is one: `Cmd::count("bio-count", 160)`, `Cmd::count("words", None)`.
    pub fn count(id: impl Into<String>, max: impl Into<Option<u32>>) -> Self {
        Self::Count {
            target: id.into(),
            max: max.into(),
        }
    }

    /// Sets the hidden field `id` to `value`.
    pub fn value(id: impl Into<String>, value: impl Into<String>) -> Self {
        Self::Value {
            target: id.into(),
            value: value.into(),
        }
    }

    /// Fills the dialog `id` from the row this element sits in, and opens it.
    pub fn fill(id: impl Into<String>) -> Self {
        Self::Fill(id.into())
    }

    /// The command as data, or nothing when it names a refused attribute.
    fn to_value(&self) -> Option<Value> {
        if let Self::SetAttr { name, .. }
        | Self::RemoveAttr { name, .. }
        | Self::ToggleAttr { name, .. } = self
            && !is_state_attr(name)
        {
            return None;
        }
        Some(match self {
            Self::Toggle(id) => json!(["toggle", id]),
            Self::Show(id) => json!(["show", id]),
            Self::Hide(id) => json!(["hide", id]),
            Self::Open(id) => json!(["open", id]),
            Self::Close(id) => json!(["close", id]),
            Self::Copy(text) => json!(["copy", text]),
            Self::SetAttr {
                target,
                name,
                value,
            } => json!(["set_attr", target, name, value]),
            Self::RemoveAttr { target, name } => json!(["remove_attr", target, name]),
            Self::ToggleAttr { target, name } => json!(["toggle_attr", target, name]),
            Self::Dispatch(name) => json!(["dispatch", name]),
            Self::Focus(id) => json!(["focus", id]),
            Self::Submit(id) => json!(["submit", id]),
            Self::Text { target, text } => json!(["text", target, text]),
            Self::Count { target, max } => json!(["count", target, max]),
            Self::Value { target, value } => json!(["value", target, value]),
            Self::Fill(id) => json!(["fill", id]),
        })
    }
}

/// Stops a debug build on an attribute a command may not touch, saying what to use.
fn refused(name: &str) {
    debug_assert!(
        is_state_attr(name),
        "a command may not touch `{name}`: only `data-*` (not `data-ui-*`), `aria-*`, \
         `hidden`, `open` and `disabled`.{}",
        if name.eq_ignore_ascii_case("value") {
            " A hidden field's value is set with Cmd::value."
        } else {
            ""
        }
    );
}

/// The attributes a command may touch. The runtime holds the same list.
fn is_state_attr(name: &str) -> bool {
    let starts = |prefix: &str| {
        name.get(..prefix.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
    };
    (starts("data-") && !starts("data-ui-"))
        || starts("aria-")
        || ["hidden", "open", "disabled"]
            .iter()
            .any(|state| state.eq_ignore_ascii_case(name))
}

/// A list of commands, run in order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Commands(pub Vec<Cmd>);

impl Commands {
    /// The list as the JSON the runtime reads.
    pub fn to_json(&self) -> String {
        Value::Array(self.0.iter().filter_map(Cmd::to_value).collect()).to_string()
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
    fn the_commands_that_write_text_are_data_on_the_wire() {
        let commands = Commands(vec![
            Cmd::text("price", "$31"),
            Cmd::count("bio-count", 160),
            Cmd::count("words", None),
            Cmd::value("sort", "amount"),
            Cmd::fill("deal-dialog"),
        ]);
        assert_eq!(
            commands.to_json(),
            r#"[["text","price","$31"],["count","bio-count",160],["count","words",null],["value","sort","amount"],["fill","deal-dialog"]]"#
        );
    }

    #[test]
    fn text_that_looks_like_markup_or_more_commands_stays_one_string() {
        let hostile = r#"<img src=x onerror=alert(1)>"],["set_attr","a","href","javascript:1"#;
        let json = Commands::from(Cmd::text("t", hostile)).to_json();
        let parsed: Vec<Vec<Value>> = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.len(), 1, "{json}");
        assert_eq!(parsed[0][2], Value::String(hostile.to_owned()));
        let json = Commands::from(Cmd::value("t", hostile)).to_json();
        let parsed: Vec<Vec<Value>> = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.len(), 1, "{json}");
    }

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
