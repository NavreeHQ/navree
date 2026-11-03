//! [`SelectLabels`]: the words of the four components.

use std::borrow::Cow;

use serde_json::{Map, Value};

/// The words of a select, combobox, multi-select or command palette. English by
/// default; pass your own for another language. `{n}`, `{total}`, `{q}` and `{label}`
/// are filled in.
#[derive(Clone, Debug)]
#[allow(missing_docs)]
pub struct SelectLabels {
    /// Said when a search or a filter finds nothing.
    pub empty: Cow<'static, str>,
    /// Said when a list has no choices at all.
    pub none: Cow<'static, str>,
    pub searching: Cow<'static, str>,
    pub failed: Cow<'static, str>,
    pub retry: Cow<'static, str>,
    pub lost: Cow<'static, str>,
    pub clear: Cow<'static, str>,
    pub remove: Cow<'static, str>,
    pub focused: Cow<'static, str>,
    pub added: Cow<'static, str>,
    pub removed: Cow<'static, str>,
    pub cleared: Cow<'static, str>,
    pub count: Cow<'static, str>,
    pub result: Cow<'static, str>,
    pub results: Cow<'static, str>,
    pub create: Cow<'static, str>,
    pub creating: Cow<'static, str>,
    pub create_failed: Cow<'static, str>,
    pub limit: Cow<'static, str>,
    pub more: Cow<'static, str>,
    pub shown: Cow<'static, str>,
    pub min: Cow<'static, str>,
    pub select_all: Cow<'static, str>,
    pub deselect_all: Cow<'static, str>,
    pub toggle: Cow<'static, str>,
    pub recent: Cow<'static, str>,
    /// The name of the list of results in a command palette.
    pub list: Cow<'static, str>,
    /// Said under a required control that was left empty.
    pub required: Cow<'static, str>,
    /// The name of a command palette's dialog.
    pub palette: Cow<'static, str>,
}

impl Default for SelectLabels {
    fn default() -> Self {
        Self {
            empty: "No results".into(),
            none: "No options".into(),
            searching: "Searching…".into(),
            failed: "Couldn't load the options. Press Enter to try again.".into(),
            retry: "Try again".into(),
            lost: "You no longer have access to these options.".into(),
            clear: "Clear".into(),
            remove: "Remove {label}".into(),
            focused: "{label} focused. Press Backspace to remove.".into(),
            added: "{label} added".into(),
            removed: "{label} removed".into(),
            cleared: "Selection cleared".into(),
            count: "{n} selected".into(),
            result: "1 result available".into(),
            results: "{n} results available".into(),
            create: "Create “{q}”".into(),
            creating: "Creating…".into(),
            create_failed: "Couldn't create it.".into(),
            limit: "You can choose at most {n}.".into(),
            more: "Show more".into(),
            shown: "Showing {n} of {total}. Keep typing to narrow the list.".into(),
            min: "Type at least {n} characters to search.".into(),
            select_all: "Select all".into(),
            deselect_all: "Deselect all".into(),
            toggle: "Show options".into(),
            recent: "Recent".into(),
            list: "Results".into(),
            required: "Choose an option.".into(),
            palette: "Command palette".into(),
        }
    }
}

impl SelectLabels {
    /// Every word, by the key the runtime knows it by.
    pub(crate) fn pairs(&self) -> [(&'static str, &str); 29] {
        [
            ("empty", &self.empty),
            ("none", &self.none),
            ("searching", &self.searching),
            ("failed", &self.failed),
            ("retry", &self.retry),
            ("lost", &self.lost),
            ("clear", &self.clear),
            ("remove", &self.remove),
            ("focused", &self.focused),
            ("added", &self.added),
            ("removed", &self.removed),
            ("cleared", &self.cleared),
            ("count", &self.count),
            ("result", &self.result),
            ("results", &self.results),
            ("create", &self.create),
            ("creating", &self.creating),
            ("createFailed", &self.create_failed),
            ("limit", &self.limit),
            ("more", &self.more),
            ("shown", &self.shown),
            ("min", &self.min),
            ("selectAll", &self.select_all),
            ("deselectAll", &self.deselect_all),
            ("toggle", &self.toggle),
            ("recent", &self.recent),
            ("list", &self.list),
            ("required", &self.required),
            ("palette", &self.palette),
        ]
    }

    /// The words that differ from English, as the JSON the runtime reads in `data-l`.
    /// The runtime holds the English itself, so a page in English carries nothing.
    pub(crate) fn changed(&self) -> Option<String> {
        let english = Self::default();
        let mut map = Map::new();
        for ((key, word), (_, was)) in self.pairs().into_iter().zip(english.pairs()) {
            if word != was {
                map.insert(key.to_owned(), Value::String(word.to_owned()));
            }
        }
        (!map.is_empty()).then(|| Value::Object(map).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_runtimes_english_is_the_same_english_the_server_defaults_to() {
        // The words live in two places so that a page in English carries none of them.
        let js = include_str!("../../runtime/src/select.js");
        for (key, word) in SelectLabels::default().pairs() {
            let line = format!("{key}: {word:?}");
            assert!(js.contains(&line), "select.js lacks {line}");
        }
    }

    #[test]
    fn only_the_words_that_differ_are_sent() {
        let mut labels = SelectLabels::default();
        assert_eq!(labels.changed(), None);
        labels.clear = "Effacer".into();
        assert_eq!(labels.changed().as_deref(), Some(r#"{"clear":"Effacer"}"#));
    }
}
