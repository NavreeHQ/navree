//! [`OptionSource`]: the answer to a search, from a `navree-orm` query.
//!
//! Which columns are searched, which one the order follows and which one the value
//! is come from lists written in the code (`&'static`), never from the request: the
//! request carries only the text, and the text is bound. The builder you pass is
//! the app's scoped query, so workspace scoping and row-level security hold, as
//! for the data table.

use navree_orm::{Builder, Executor, Filter, Model, Record, Result, Value, col, col_as};

use super::choice::Choice;
use super::query::{ChoiceList, OptionQuery};

/// Where a searched list's options come from.
///
/// Columns are named as the model has them. A field the store keeps under another
/// name can be named by the field instead: `id` on a MongoDB model, whose column is
/// `_id`, means that column, so a list is ordered by its key on every store.
///
/// ```ignore
/// const PEOPLE: OptionSource = OptionSource::new("id", "name").search_in(&["name", "email"]);
///
/// async fn people(query: OptionQuery, uow: Uow) -> Result<ChoiceList> {
///     PEOPLE
///         .find(Person::query(), &query, &uow, "/people/options", |record| {
///             let person = record.into_inner();
///             Choice::new(person.id.to_string(), person.name)
///         })
///         .await
/// }
/// ```
#[derive(Clone, Copy, Debug)]
pub struct OptionSource {
    value: &'static str,
    label: &'static str,
    search: &'static [&'static str],
    order: &'static str,
}

impl OptionSource {
    /// Options whose value is the column `value` and whose label is the column `label`.
    /// The label column is searched and ordered by until you say otherwise.
    pub const fn new(value: &'static str, label: &'static str) -> Self {
        Self {
            value,
            label,
            search: &[],
            order: label,
        }
    }

    /// The columns the typed text is looked for in (any of them), instead of the
    /// label column alone.
    pub const fn search_in(mut self, columns: &'static [&'static str]) -> Self {
        self.search = columns;
        self
    }

    /// The column the options are ordered by. The value column breaks ties, so a page
    /// boundary never repeats or skips a row.
    pub const fn order_by(mut self, column: &'static str) -> Self {
        self.order = column;
        self
    }

    /// The options `query` asks for, from `builder`: the rows whose searched columns
    /// contain the text, at most the limit, with a "Show more" link when there are more.
    /// `path` is this endpoint's own path, for that link. `map` makes a choice of a row.
    ///
    /// # Errors
    /// Whatever the database reports.
    pub async fn find<M, E>(
        &self,
        builder: Builder<M>,
        query: &OptionQuery,
        exec: &E,
        path: &str,
        map: impl Fn(Record<M>) -> Choice,
    ) -> Result<ChoiceList>
    where
        M: Model,
        E: Executor + ?Sized,
    {
        let mut builder = builder;
        if !query.is_empty() {
            let columns = if self.search.is_empty() {
                std::slice::from_ref(&self.label)
            } else {
                self.search
            };
            builder = builder.where_(Filter::any(columns.iter().map(|column| {
                col_as::<String>(column_of::<M>(column))
                    .contains_ignore_case(query.text().to_owned())
            })));
        }
        let limit = query.limit();
        let rows = builder
            .reorder()
            .order_by(column_of::<M>(self.order))
            .order_by(column_of::<M>(self.value))
            .offset(query.offset())
            .limit(limit as u64 + 1)
            .get(exec)
            .await?
            .into_vec();
        let more = rows.len() > limit;
        let mut list = ChoiceList::new();
        for record in rows.into_iter().take(limit) {
            list = list.choice(map(record));
        }
        Ok(if more {
            list.more(query.next_url(path, limit))
        } else {
            list
        })
    }
}

impl OptionSource {
    /// Whether every one of `values` is an option this caller may pick: a row of
    /// `builder`, which is the caller's own scoped query, with the value column
    /// equal to it. Nothing a browser sends is known to be one of the options it
    /// was given, so a handler asks this after reading a form, with the same
    /// builder it answers searches with: a value from another workspace, one the
    /// caller may not see, or one that never existed is not pickable.
    ///
    /// The count is of different values, so a query that repeats a row (a join) or a value
    /// column that repeats a value does not change the answer.
    ///
    /// An empty value is no choice and is passed over: whether one is needed is the
    /// field's own rule. `as_value` turns the text of a value into what the column
    /// holds; [`as_int`](Self::as_int), [`as_uuid`](Self::as_uuid) and
    /// [`as_text`](Self::as_text) are the usual ones, and a value that doesn't turn
    /// into one is not pickable.
    ///
    /// ```ignore
    /// let picked = deal.owner_id.iter();
    /// if !PEOPLE.all_pickable(Person::query(), picked, &uow, OptionSource::as_int).await? {
    ///     // Refuse it as a failed rule: see `Rejected::after_decode`.
    /// }
    /// ```
    ///
    /// # Errors
    /// Whatever the database reports.
    pub async fn all_pickable<M, E, V>(
        &self,
        builder: Builder<M>,
        values: impl IntoIterator<Item = V>,
        exec: &E,
        as_value: impl Fn(&str) -> Option<Value>,
    ) -> Result<bool>
    where
        M: Model,
        E: Executor + ?Sized,
        V: AsRef<str>,
    {
        let mut wanted: Vec<Value> = Vec::new();
        for value in values {
            let text = value.as_ref();
            if text.is_empty() {
                continue;
            }
            let Some(parsed) = as_value(text) else {
                return Ok(false);
            };
            if !wanted.contains(&parsed) {
                wanted.push(parsed);
            }
        }
        if wanted.is_empty() {
            return Ok(true);
        }
        // How many different values the query has among those posted: not how many rows,
        // which a join repeats and a value column that isn't unique repeats.
        let column = column_of::<M>(self.value);
        let distinct = wanted.len() as u64;
        let found = builder
            .reorder()
            .where_(col(column).is_in(wanted))
            .count_distinct(exec, column)
            .await?;
        Ok(found == distinct)
    }

    /// A value read as a whole number, for a column of `i64`.
    pub fn as_int(text: &str) -> Option<Value> {
        text.parse::<i64>().ok().map(Value::I64)
    }

    /// A value read as a UUID, for a column of `Uuid`.
    pub fn as_uuid(text: &str) -> Option<Value> {
        text.parse::<uuid::Uuid>().ok().map(Value::Uuid)
    }

    /// A value read as the text it is, for a column of text.
    pub fn as_text(text: &str) -> Option<Value> {
        Some(Value::String(text.to_owned()))
    }
}

/// The model's column for `name`: `name` itself when the model has such a column, the
/// column of the field called `name` when there is one (`id` is `_id` on MongoDB), and
/// `name` as written otherwise (a column of a joined table, say).
fn column_of<M: Model>(name: &'static str) -> &'static str {
    let meta = M::meta();
    if meta.has_column(name) {
        return name;
    }
    meta.all_columns()
        .iter()
        .find(|column| column.field() == name)
        .map_or(name, |column| column.name())
}
