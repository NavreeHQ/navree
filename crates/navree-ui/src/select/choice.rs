//! [`Choice`]: one thing a list offers, and the entries a list is made of.

use crate::components::OptionItem;
use crate::icons::Icon;

/// One choice in a [`Select`](super::Select), [`Combobox`](super::Combobox) or
/// [`MultiSelect`](super::MultiSelect).
///
/// ```
/// use navree_ui::Choice;
///
/// let choice = Choice::new("42", "Ada Lovelace").description("Engineer").keywords(["analytical"]);
/// # let _ = choice;
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Choice {
    pub(crate) value: String,
    pub(crate) label: String,
    pub(crate) selected_label: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) icon: Option<Icon>,
    pub(crate) avatar: Option<String>,
    pub(crate) keywords: Vec<String>,
    pub(crate) disabled: bool,
    pub(crate) href: Option<String>,
}

impl Choice {
    /// A choice that posts `value` and shows `label`.
    pub fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
            selected_label: None,
            description: None,
            icon: None,
            avatar: None,
            keywords: Vec::new(),
            disabled: false,
            href: None,
        }
    }

    /// A line under the label, in the list only.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// An icon before the label, in the list and in the control.
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// A small picture before the label: a URL on this site (the content security policy
    /// allows no other).
    pub fn avatar(mut self, src: impl Into<String>) -> Self {
        self.avatar = Some(src.into());
        self
    }

    /// Words that find this choice though the label doesn't hold them.
    pub fn keywords<S: Into<String>>(mut self, words: impl IntoIterator<Item = S>) -> Self {
        self.keywords = words.into_iter().map(Into::into).collect();
        self
    }

    /// What the control shows once this is chosen, when that is shorter than the label.
    pub fn selected_label(mut self, label: impl Into<String>) -> Self {
        self.selected_label = Some(label.into());
        self
    }

    /// Makes the choice a link to a page on this site. For the results a command
    /// palette's server source answers.
    pub fn link(mut self, href: impl Into<String>) -> Self {
        self.href = Some(href.into());
        self
    }

    /// Shows the choice but refuses it.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// The value that is posted.
    pub fn value(&self) -> &str {
        &self.value
    }

    /// The label that is shown.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// What the control shows when this is chosen.
    pub(crate) fn shown(&self) -> &str {
        self.selected_label.as_deref().unwrap_or(&self.label)
    }

    /// Whether the list draws more than the label.
    pub(crate) fn is_rich(&self) -> bool {
        self.description.is_some() || self.icon.is_some() || self.avatar.is_some()
    }

    /// The words the runtime searches, besides the label.
    pub(crate) fn keyword_text(&self) -> Option<String> {
        (!self.keywords.is_empty()).then(|| self.keywords.join(" "))
    }
}

impl<V: Into<String>, L: Into<String>> From<(V, L)> for Choice {
    fn from((value, label): (V, L)) -> Self {
        Self::new(value, label)
    }
}

impl From<OptionItem> for Choice {
    fn from(item: OptionItem) -> Self {
        Self::new(item.value, item.label)
    }
}

/// One entry of a list: a choice, a labelled group, or a divider.
#[derive(Clone, Debug)]
pub(crate) enum Entry {
    Choice(Choice),
    Group { label: String, choices: Vec<Choice> },
    Separator,
}

/// A list's entries, in order.
#[derive(Clone, Debug, Default)]
pub(crate) struct Entries(pub(crate) Vec<Entry>);

impl Entries {
    pub(crate) fn choice(&mut self, choice: Choice) {
        self.0.push(Entry::Choice(choice));
    }

    pub(crate) fn group(&mut self, label: String, choices: Vec<Choice>) {
        self.0.push(Entry::Group { label, choices });
    }

    pub(crate) fn separator(&mut self) {
        self.0.push(Entry::Separator);
    }

    /// Every choice, groups opened.
    pub(crate) fn choices(&self) -> impl Iterator<Item = &Choice> {
        self.0.iter().flat_map(|entry| match entry {
            Entry::Choice(choice) => std::slice::from_ref(choice).iter(),
            Entry::Group { choices, .. } => choices.iter(),
            Entry::Separator => [].iter(),
        })
    }

    /// The choice that posts `value`.
    pub(crate) fn find(&self, value: &str) -> Option<&Choice> {
        self.choices().find(|choice| choice.value == value)
    }
}
