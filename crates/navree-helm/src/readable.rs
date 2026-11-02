//! [`Readable`]: what a user may read of a resource. Every route asks here, and a
//! [`RawRecord`](crate::handle::RawRecord) carries only what this let through, so a
//! new route has nothing else to show.
//!
//! A column is out of reach in two ways: the model hides it from everyone
//! (`#[model(hidden)]`, its `hidden(..)` list, or left out of its `visible(..)`
//! list), and the user's field rules hide it from them
//! (`cannot(READ, "Deal").fields(["title"])`). A hidden value reaches no page, list,
//! API answer, picker, related column, form or export, and no query is run on a
//! column that could tell it: no search, sort, filter or metric. A rule with
//! conditions hides the value only where they hold, and takes the whole column out of
//! queries, since an order or a match would still say something about those rows.
//!
//! A `#[model(hidden)]` column may still have a field: it is write-only, in forms and
//! the API's writes, shown blank.
//!
//! What the panel can't see through is the app's own code: a filter's closure
//! ([`Filter::reads`](crate::Filter::reads) says what it reads), a computed field's
//! closure (a rule names the field itself, [`Field::computed`](crate::Field::computed))
//! and a metric's [`filter`](crate::Metric::filter) conditions.

use navree_access::{Ability, READ};
use serde_json::{Map, Value};

use crate::field::{Field, FieldKind};
use crate::handle::{FilterInfo, MetricInfo, ResourceInfo, short_key};
use crate::values::key_text;

/// What the user with `ability` may read of a resource.
pub(crate) struct Readable<'a> {
    info: &'a ResourceInfo,
    ability: &'a Ability,
}

impl ResourceInfo {
    /// What the user with `ability` may read of this resource.
    pub(crate) fn readable<'a>(&'a self, ability: &'a Ability) -> Readable<'a> {
        Readable {
            info: self,
            ability,
        }
    }
}

impl Readable<'_> {
    /// Whether the user may read `column` of `record`.
    pub(crate) fn column(&self, record: &Value, column: &str) -> bool {
        !self.info.hidden.contains(column)
            && self
                .ability
                .can_field(READ, self.info.subject, Some(record), column)
    }

    /// Whether the user may read `field` of `record`: a column, and for an amount
    /// its currency, which it says; a computed or view field by its own name, which
    /// a rule can give.
    pub(crate) fn field(&self, record: &Value, field: &Field) -> bool {
        match &field.kind {
            FieldKind::HasMany(_) => true,
            FieldKind::Money { currency_column } => {
                self.column(record, &field.name) && self.column(record, currency_column)
            }
            FieldKind::Computed(_) | FieldKind::View { .. } => {
                self.ability
                    .can_field(READ, self.info.subject, Some(record), &field.name)
            }
            _ => self.column(record, &field.name),
        }
    }

    /// The columns of `record` the user may read: what a page, an answer, a form or
    /// a template sees of it.
    pub(crate) fn values(&self, record: &Value) -> Value {
        let Some(columns) = record.as_object() else {
            return Value::Object(Map::new());
        };
        Value::Object(
            columns
                .iter()
                .filter(|(column, _)| self.column(record, column))
                .map(|(column, value)| (column.clone(), value.clone()))
                .collect(),
        )
    }

    /// What names `record` on screen: its `TITLE` column, or, where there is none
    /// (the key is the default title, the column is empty, or the user may not read
    /// it), the resource's singular name and the end of its key (`Report #3f9a1c2e`),
    /// never a whole UUID.
    pub(crate) fn title(&self, record: &Value, id: &str) -> String {
        let info = self.info;
        let named = record
            .get(info.title)
            .filter(|_| info.title != info.pk && self.column(record, info.title))
            .filter(|value| !value.is_null())
            .map(|value| key_text(value).unwrap_or_else(|| value.to_string()))
            .filter(|text| !text.trim().is_empty());
        named.unwrap_or_else(|| format!("{} #{}", info.singular, short_key(id)))
    }

    /// Whether a query may use `column`: only when it says nothing the user couldn't
    /// read, on any row they may read. The key is what names a record in every URL.
    pub(crate) fn queryable(&self, column: &str) -> bool {
        column == self.info.pk
            || (!self.info.hidden.contains(column)
                && self
                    .ability
                    .can_field_on_every_record(READ, self.info.subject, column))
    }

    /// Whether every column of the model may be used in a query.
    fn every_column_queryable(&self) -> bool {
        self.info.columns.iter().all(|c| self.queryable(c))
    }

    /// The columns the search box may match.
    pub(crate) fn search(&self) -> Vec<&'static str> {
        self.info
            .search
            .iter()
            .copied()
            .filter(|column| self.queryable(column))
            .collect()
    }

    /// The columns the list may be sorted by, with its default sort's.
    pub(crate) fn sortable(&self) -> Vec<&str> {
        let mut columns: Vec<&str> = self
            .info
            .sortable()
            .into_iter()
            .filter(|column| self.queryable(column))
            .collect();
        if !self.queryable(self.info.default_sort.trim_start_matches('-')) {
            columns.push(self.info.pk);
        }
        columns
    }

    /// The list's default order, a column with a leading `-` for descending: the
    /// resource's, or newest key first when that would tell what the user can't read.
    pub(crate) fn default_sort(&self) -> String {
        let default = self.info.default_sort.as_str();
        if self.queryable(default.trim_start_matches('-')) {
            default.to_owned()
        } else {
            format!("-{}", self.info.pk)
        }
    }

    /// Whether `filter` may narrow the list: the column it names, or what its own
    /// code reads. Code that doesn't say could read anything, so it runs only for a
    /// user with nothing hidden.
    pub(crate) fn filter(&self, filter: &FilterInfo) -> bool {
        match (&filter.reads, filter.kind) {
            (Some(reads), _) => reads.iter().all(|column| self.queryable(column)),
            (None, "boolean" | "custom") => self.every_column_queryable(),
            (None, _) => self.queryable(&filter.name),
        }
    }

    /// Whether `metric` may be given: its value says something of each column it
    /// adds up, groups or dates by.
    pub(crate) fn metric(&self, metric: &MetricInfo) -> bool {
        metric.columns.iter().all(|column| self.queryable(column))
    }
}

/// The columns of a model hidden from everyone: those named by `hidden`, and with a
/// `visible` list those it doesn't name. `names` lists each column's name and its
/// field's, either of which those lists may use.
pub(crate) fn hidden_columns<'a>(
    names: impl Iterator<Item = (&'a str, &'a str)>,
    hidden: &[&str],
    visible: &[&str],
) -> Vec<&'a str> {
    names
        .filter(|(column, field)| {
            let named = |list: &[&str]| list.contains(column) || list.contains(field);
            named(hidden) || (!visible.is_empty() && !named(visible))
        })
        .map(|(column, _)| column)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_column_is_hidden_by_either_of_its_names_or_by_a_list_of_the_rest() {
        let columns = || [("id", "id"), ("mail", "email"), ("name", "name")].into_iter();
        assert_eq!(hidden_columns(columns(), &[], &[]), Vec::<&str>::new());
        assert_eq!(hidden_columns(columns(), &["email"], &[]), ["mail"]);
        assert_eq!(hidden_columns(columns(), &["mail"], &[]), ["mail"]);
        assert_eq!(hidden_columns(columns(), &[], &["id", "name"]), ["mail"]);
    }
}
