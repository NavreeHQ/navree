//! [`OptionSource`]: the answer to a search, from a `navree-orm` query.
//!
//! Which columns are searched, which one the order follows and which one the value
//! is come from lists written in the code (`&'static`), never from the request: the
//! request carries only the text, and the text is bound. The builder you pass is
//! the app's scoped query, so workspace scoping and row-level security hold, as
//! for the data table.

use navree_orm::{Builder, Executor, Filter, Model, Record, Result, col_as};

use super::choice::Choice;
use super::query::{ChoiceList, OptionQuery};

/// Where a searched list's options come from.
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
                col_as::<String>(column).contains_ignore_case(query.text().to_owned())
            })));
        }
        let limit = query.limit();
        let rows = builder
            .reorder()
            .order_by(self.order)
            .order_by(self.value)
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
