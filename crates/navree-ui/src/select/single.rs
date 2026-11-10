//! [`Select`]: one choice from a small, fixed list.

use super::picker::{Kind, Picker, picker_methods};
use crate::node::into_node;

/// One choice from a small, fixed list, in a styled listbox.
///
/// A real `<select>` carries the value, so the form posts as with any select, and
/// with scripting off that `<select>` is what the person sees. For a list people
/// search, use a [`Combobox`](super::Combobox); for a handful of options worth
/// seeing at once, a [`RadioGroup`](crate::RadioGroup).
///
/// ```
/// use navree_ui::{Select, render};
///
/// let select = Select::new("stage")
///     .placeholder("Choose a stage")
///     .option("lead", "Lead")
///     .option("won", "Won")
///     .selected("lead")
///     .clearable();
/// let html = render(&select.into());
/// assert!(html.contains(r#"<select data-slot="select-native" name="stage""#));
/// assert!(html.contains(r#"role="combobox""#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Select {
    picker: Picker,
}

impl Select {
    /// A select posting as `name`.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            picker: Picker::new(Kind::Select, name),
        }
    }

    /// Sets the chosen value.
    pub fn selected(mut self, value: impl Into<String>) -> Self {
        self.picker.chosen = vec![value.into()];
        self
    }

    /// Adds a clear button, and lets `Delete` and `Backspace` clear the choice.
    pub fn clearable(mut self) -> Self {
        self.picker.clearable = true;
        self
    }
}

picker_methods!(Select);
into_node!(Select);
