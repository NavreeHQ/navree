//! [`MultiSelect`]: many choices, as chips.

use super::choice::Choice;
use super::picker::{Kind, Picker, picker_methods, typed_methods};
use crate::node::into_node;

/// Many choices, shown as chips with a 4 px corner.
///
/// Every chosen value posts under the one name (a `<select multiple>` carries them).
/// `Backspace` in the empty input moves to the last chip and again removes it;
/// `Delete` or `Backspace` on a chip removes it; `Left` and `Right` move between chips.
///
/// ```
/// use navree_ui::{MultiSelect, render};
///
/// let tags = MultiSelect::new("tags")
///     .option("a", "Alpha")
///     .option("b", "Beta")
///     .selected_values(["a"])
///     .max(3);
/// let html = render(&tags.into());
/// assert!(html.contains(r#"data-ui-max="3""#));
/// assert!(html.contains(r#"aria-label="Remove Alpha""#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct MultiSelect {
    picker: Picker,
}

impl MultiSelect {
    /// A multi-select posting as `name`.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            picker: Picker::new(Kind::Multi, name),
        }
    }

    /// Sets the chosen values.
    pub fn selected_values<S: Into<String>>(mut self, values: impl IntoIterator<Item = S>) -> Self {
        self.picker.chosen = values.into_iter().map(Into::into).collect();
        self
    }

    /// Sets the chosen choices, for values that aren't among the options in the page:
    /// the ones a search found earlier, for an edit form.
    pub fn selected_choices<C: Into<Choice>>(
        mut self,
        choices: impl IntoIterator<Item = C>,
    ) -> Self {
        for choice in choices {
            let choice = choice.into();
            self.picker.chosen.push(choice.value.clone());
            self.picker.extra.push(choice);
        }
        self
    }

    /// Allows at most `n` choices. The other options are switched off at the limit, and
    /// so is making a new one. This is the browser's help only: with JavaScript off, or
    /// from a hand-made post, more can arrive. In a derived form, `#[form(max = n)]` on
    /// the field holds the post to the limit too; otherwise check the count in the
    /// handler.
    pub fn max(mut self, n: u32) -> Self {
        self.picker.max = Some(n);
        self
    }

    /// Shows one "N selected" chip instead of the chips once more than `n` are chosen.
    pub fn summary_after(mut self, n: u32) -> Self {
        self.picker.summary_after = Some(n);
        self
    }

    /// Adds "Select all" and "Deselect all" for the options the text matches.
    /// For lists in the page: a searched list is never all there.
    pub fn select_all(mut self) -> Self {
        self.picker.pick_all = true;
        self
    }
}

picker_methods!(MultiSelect);
typed_methods!(MultiSelect);
into_node!(MultiSelect);
