//! [`IndexSettings`]: the fields of an index and what each is used for.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::SearchError;

/// The reserved field holding a document's workspace.
pub const WORKSPACE_FIELD: &str = "_workspace";

/// What one field of an index is used for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FieldSettings {
    /// The field's name in the index.
    pub name: String,
    /// The database column it comes from (access rules name columns);
    /// the field name unless renamed.
    pub column: String,
    /// Matched by the query text, with this weight (1 to 10).
    pub boost: Option<u8>,
    /// Usable in `filter`, `filter_in`, `range` and `filter_null`.
    pub filter: bool,
    /// Usable in `sort_asc` and `sort_desc`.
    pub sort: bool,
    /// Part of the record's embedding (vector search).
    pub embed: bool,
}

impl FieldSettings {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            column: name.to_owned(),
            boost: None,
            filter: false,
            sort: false,
            embed: false,
        }
    }

    /// Whether the field is matched by the query text.
    pub fn is_text(&self) -> bool {
        self.boost.is_some()
    }
}

/// The settings of one index: its name, the index it lives in, and its
/// fields.
///
/// ```
/// use navree_search::IndexSettings;
///
/// let settings = IndexSettings::new("Deal", "deals")
///     .text("title", 3)
///     .text("company_name", 2)
///     .filter("stage")
///     .filter("owner_id")
///     .sort("amount_cents");
/// assert_eq!(settings.title_field(), Some("title"));
/// assert!(settings.is_filterable("stage"));
/// assert!(!settings.is_sortable("stage"));
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct IndexSettings {
    name: String,
    index: String,
    fields: Vec<FieldSettings>,
    soft_delete: Option<String>,
    workspace: Option<String>,
    synonyms: BTreeMap<String, Vec<String>>,
    stop_words: Vec<String>,
}

impl IndexSettings {
    /// Settings for the index `index` (before `search.prefix`), whose
    /// records are called `name` in commands and results (`Contact`).
    pub fn new(name: impl Into<String>, index: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            index: index.into(),
            fields: Vec::new(),
            soft_delete: None,
            workspace: None,
            synonyms: BTreeMap::new(),
            stop_words: Vec::new(),
        }
    }

    fn entry(&mut self, field: &str) -> &mut FieldSettings {
        if let Some(i) = self.fields.iter().position(|f| f.name == field) {
            &mut self.fields[i]
        } else {
            self.fields.push(FieldSettings::new(field));
            self.fields.last_mut().expect("just pushed")
        }
    }

    /// A full-text field weighing `boost` (1 to 10) times a plain one.
    pub fn text(mut self, field: &str, boost: u8) -> Self {
        self.entry(field).boost = Some(boost);
        self
    }

    /// An exact-match field for filters.
    pub fn filter(mut self, field: &str) -> Self {
        self.entry(field).filter = true;
        self
    }

    /// A field to sort on.
    pub fn sort(mut self, field: &str) -> Self {
        self.entry(field).sort = true;
        self
    }

    /// A field whose text goes into the record's embedding.
    pub fn embed(mut self, field: &str) -> Self {
        self.entry(field).embed = true;
        self
    }

    /// Field `field` comes from database column `column` (for a renamed
    /// field, so access rules on the column become filters on the field).
    pub fn column(mut self, field: &str, column: &str) -> Self {
        self.entry(field).column = column.to_owned();
        self
    }

    /// A nullable timestamp column: records where it is set are removed
    /// from the index.
    pub fn soft_delete(mut self, column: &str) -> Self {
        self.soft_delete = Some(column.to_owned());
        self
    }

    /// The column that scopes records to a workspace.
    pub fn workspace(mut self, column: &str) -> Self {
        self.workspace = Some(column.to_owned());
        self
    }

    /// [`workspace`](IndexSettings::workspace) when `column` is `Some`.
    pub fn workspace_opt(self, column: Option<&str>) -> Self {
        match column {
            Some(column) => self.workspace(column),
            None => self,
        }
    }

    /// `word` also matches each of `alternatives` (and they match it).
    pub fn synonyms<I, S>(mut self, word: &str, alternatives: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.synonyms.insert(
            word.to_owned(),
            alternatives.into_iter().map(Into::into).collect(),
        );
        self
    }

    /// Words the index ignores in queries.
    pub fn stop_words<I, S>(mut self, words: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.stop_words.extend(words.into_iter().map(Into::into));
        self
    }

    /// The name of the records (`Contact`).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The index name, before `search.prefix` (`contacts`).
    pub fn index(&self) -> &str {
        &self.index
    }

    /// Every field, in declaration order.
    pub fn fields(&self) -> &[FieldSettings] {
        &self.fields
    }

    /// Field `name`.
    pub fn field(&self, name: &str) -> Option<&FieldSettings> {
        self.fields.iter().find(|f| f.name == name)
    }

    /// The field that comes from column `column`.
    pub fn field_for_column(&self, column: &str) -> Option<&FieldSettings> {
        self.fields.iter().find(|f| f.column == column)
    }

    /// The full-text fields with their boosts, in declaration order.
    pub fn text_fields(&self) -> impl Iterator<Item = (&str, u8)> {
        self.fields
            .iter()
            .filter_map(|f| f.boost.map(|b| (f.name.as_str(), b)))
    }

    /// The first full-text field: the title of `across` results.
    pub fn title_field(&self) -> Option<&str> {
        self.text_fields().next().map(|(name, _)| name)
    }

    /// The largest boost (1 without text fields).
    pub fn max_boost(&self) -> u8 {
        self.text_fields().map(|(_, b)| b).max().unwrap_or(1)
    }

    /// Whether `field` may be filtered on (the workspace field always may).
    pub fn is_filterable(&self, field: &str) -> bool {
        field == WORKSPACE_FIELD || self.field(field).is_some_and(|f| f.filter)
    }

    /// Whether `field` may be sorted on.
    pub fn is_sortable(&self, field: &str) -> bool {
        self.field(field).is_some_and(|f| f.sort)
    }

    /// The fields whose text goes into embeddings.
    pub fn embed_fields(&self) -> impl Iterator<Item = &str> {
        self.fields
            .iter()
            .filter(|f| f.embed)
            .map(|f| f.name.as_str())
    }

    /// The soft-delete column, if any.
    pub fn soft_delete_column(&self) -> Option<&str> {
        self.soft_delete.as_deref()
    }

    /// The workspace column, if any.
    pub fn workspace_column(&self) -> Option<&str> {
        self.workspace.as_deref()
    }

    /// The synonyms.
    pub fn synonym_map(&self) -> &BTreeMap<String, Vec<String>> {
        &self.synonyms
    }

    /// The stop words.
    pub fn stop_word_list(&self) -> &[String] {
        &self.stop_words
    }

    /// Adds the synonyms and stop words configured for this index.
    pub(crate) fn with_overrides(
        mut self,
        synonyms: &BTreeMap<String, Vec<String>>,
        stop_words: &[String],
    ) -> Self {
        for (word, alternatives) in synonyms {
            self.synonyms.insert(word.clone(), alternatives.clone());
        }
        for word in stop_words {
            if !self.stop_words.contains(word) {
                self.stop_words.push(word.clone());
            }
        }
        self
    }

    /// Checks names and boosts: `[A-Za-z0-9_]` names not starting with
    /// `_`, boosts from 1 to 10, an index name.
    pub fn validate(&self) -> Result<(), SearchError> {
        let invalid = |message: String| SearchError::InvalidSettings {
            index: self.index.clone(),
            message,
        };
        if !valid_name(&self.index) {
            return Err(invalid(format!(
                "the index name `{}` must be letters, digits and `_`",
                self.index
            )));
        }
        if self.name.trim().is_empty() {
            return Err(invalid("the name is empty".to_owned()));
        }
        for field in &self.fields {
            if !valid_name(&field.name) || field.name.starts_with('_') || field.name == "id" {
                return Err(invalid(format!(
                    "`{}` is not a valid field name (letters, digits and `_`, not `id`, not starting with `_`)",
                    field.name
                )));
            }
            if let Some(boost) = field.boost
                && !(1..=10).contains(&boost)
            {
                return Err(invalid(format!(
                    "the boost of `{}` is {boost}; use 1 to 10",
                    field.name
                )));
            }
        }
        Ok(())
    }
}

/// Letters, digits and `_`, not empty.
pub(crate) fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_on_one_field_combine() {
        let settings = IndexSettings::new("Contact", "contacts")
            .text("email_address", 2)
            .column("email_address", "email")
            .filter("status")
            .sort("status")
            .embed("email_address");
        let email = settings.field("email_address").unwrap();
        assert_eq!(email.column, "email");
        assert!(email.is_text() && email.embed);
        assert_eq!(
            settings.field_for_column("email").map(|f| f.name.as_str()),
            Some("email_address")
        );
        assert!(settings.is_filterable("status") && settings.is_sortable("status"));
        assert!(settings.is_filterable(WORKSPACE_FIELD));
        assert_eq!(settings.fields().len(), 2);
        assert_eq!(settings.max_boost(), 2);
        assert_eq!(settings.embed_fields().collect::<Vec<_>>(), ["email_address"]);
    }

    #[test]
    fn validation_names_the_problem() {
        let bad = IndexSettings::new("Deal", "deals").text("title", 11);
        assert!(bad.validate().unwrap_err().to_string().contains("1 to 10"));
        let bad = IndexSettings::new("Deal", "deals").filter("_hidden");
        assert!(bad.validate().is_err());
        let bad = IndexSettings::new("Deal", "deals").filter("id");
        assert!(bad.validate().is_err());
        let bad = IndexSettings::new("Deal", "de-als");
        assert!(bad.validate().is_err());
        assert!(
            IndexSettings::new("Deal", "deals")
                .text("title", 10)
                .validate()
                .is_ok()
        );
    }

    #[test]
    fn overrides_add_synonyms_and_stop_words() {
        let settings = IndexSettings::new("Company", "companies")
            .stop_words(["inc"])
            .with_overrides(
                &BTreeMap::from([("acme".to_owned(), vec!["acme corp".to_owned()])]),
                &["inc".to_owned(), "ltd".to_owned()],
            );
        assert_eq!(settings.stop_word_list(), ["inc", "ltd"]);
        assert_eq!(settings.synonym_map()["acme"], ["acme corp"]);
    }
}
