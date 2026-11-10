//! [`Combobox`]: one choice, found by typing.

use super::choice::Choice;
use super::picker::{Kind, Picker, picker_methods, typed_methods};
use crate::node::into_node;

/// One choice, found by typing. The options are in the page, or searched on the
/// server.
///
/// The input shows the chosen label and is not posted; a hidden `<select>` posts the
/// value. Typing filters the list (case and accents don't matter) and `Enter` picks
/// the highlighted option. With [`search`](Self::search) the runtime asks the server
/// instead, and nothing is filtered again in the browser.
///
/// ```
/// use navree_ui::{Combobox, render};
///
/// let owner = Combobox::new("owner_id")
///     .placeholder("Search people")
///     .search("/people/options")
///     .selected_choice(("7", "Gurpreet Singh"));
/// let html = render(&owner.into());
/// assert!(html.contains(r#"data-ui-search="/people/options""#));
/// assert!(html.contains(r#"value="Gurpreet Singh""#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Combobox {
    picker: Picker,
}

impl Combobox {
    /// A combobox posting as `name`.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            picker: Picker::new(Kind::Combobox, name),
        }
    }

    /// Sets the chosen value, which is one of the options.
    pub fn selected(mut self, value: impl Into<String>) -> Self {
        self.picker.chosen = vec![value.into()];
        self
    }

    /// Sets the chosen value when it isn't among the options in the page: the one a
    /// search found earlier, for an edit form.
    pub fn selected_choice(mut self, choice: impl Into<Choice>) -> Self {
        let choice = choice.into();
        self.picker.chosen = vec![choice.value.clone()];
        self.picker.extra.push(choice);
        self
    }
}

picker_methods!(Combobox);
typed_methods!(Combobox);
into_node!(Combobox);
