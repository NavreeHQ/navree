//! A table whose columns, and a form whose fields, are given at run time: an admin
//! panel that reads them from the resources an app registers, a report builder, a
//! tool over data whose shape a struct doesn't know.
//!
//! They are the [`DataTable`](crate::DataTable) and the [`FormView`](crate::FormView)
//! themselves, drawn from a description built at run time instead of a derive, so the
//! markup, the behaviour with and without JavaScript, the words, the accessibility and
//! the budgets are the typed ones.
//!
//! - [`DescribedTable`] lists the columns ([`ColumnSpec`]) and the table's settings.
//!   [`DescribedTable::query`] reads a URL against them, as
//!   [`TableQuery::parse`](crate::TableQuery::parse) does for a derived row, and
//!   [`DescribedTable::bulk`] reads a bulk action's post. A [`DescribedRow`] holds one
//!   cell for each column, in their order.
//! - [`DescribedForm`] lists the fields ([`FieldSpec`]) and their rules in the browser;
//!   [`DescribedForm::view`] is the form's [`FormView`](crate::FormView), with every
//!   setter it has. The page reads the post back against its own description: the
//!   rules here are what the browser checks before anything is sent.
//!
//! ```
//! use navree_ui::described::{ColumnSpec, DescribedCell, DescribedRow, DescribedTable};
//! use navree_ui::table::{Direction, FilterKind};
//! use navree_ui::{DataTable, render};
//!
//! let shape = DescribedTable::new()
//!     .column(ColumnSpec::new("title", "Title").sortable().search())
//!     .column(
//!         ColumnSpec::new("stage", "Stage")
//!             .filter(FilterKind::Multi)
//!             .options([("lead", "Lead"), ("won", "Won")]),
//!     )
//!     .sort("title", Direction::Asc);
//! let query = shape.query("/deals", "filter[stage][in]=won");
//! let rows = vec![DescribedRow::new(
//!     "1",
//!     vec![DescribedCell::text("Gulati renewal"), DescribedCell::plain("Won")],
//! )];
//! let html = render(&DataTable::new("deals", &query, &rows).into());
//! assert!(html.contains("Gulati renewal"));
//! assert!(html.contains("Stage: Won"));
//! ```
//!
//! A description's names, labels and choices are kept for the life of the program,
//! each text once however often a description names it, so a description is cheap to
//! build for every request. Keep values out of it: rows, the choices a page looks up
//! and anything a user typed go to the table and the form, as for a typed one.

use std::any::{Any, TypeId};
use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::sync::{Mutex, OnceLock, PoisonError};

mod columns;
mod fields;

pub use columns::{ColumnSpec, DescribedCell, DescribedRow, DescribedTable, KeptParams};
pub use fields::{Described, DescribedForm, FieldSpec};

/// `text` as a name the table's setters take: the id of a [`DataTable`](crate::DataTable),
/// an action's id in a [`BulkAction`](crate::BulkAction), the column of
/// [`DataTable::options`](crate::DataTable::options), `cell` or `foot`. It is kept for the
/// life of the program, each text once, like the description's own names: give it names,
/// never values.
///
/// ```
/// use navree_ui::described::name;
///
/// let action = name(&String::from("mark-won"));
/// assert_eq!(action, "mark-won");
/// assert!(std::ptr::eq(action, name("mark-won")));
/// ```
pub fn name(text: &str) -> &'static str {
    keep_str(text)
}

/// `text`, kept for the life of the program: the same text is kept once.
pub(crate) fn keep_str(text: &str) -> &'static str {
    static KEPT: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
    if text.is_empty() {
        return "";
    }
    let mut kept = KEPT
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if let Some(found) = kept.get(text).copied() {
        return found;
    }
    let leaked: &'static str = Box::leak(text.to_owned().into_boxed_str());
    kept.insert(leaked);
    leaked
}

/// The list kept so far for each type, by what it holds.
type KeptLists = HashMap<(TypeId, String), &'static (dyn Any + Send + Sync)>;

/// `items`, kept for the life of the program: a list equal to one kept before is that
/// one. Two lists are equal when they print the same, which for the description's
/// types is when every field is.
pub(crate) fn keep_slice<T: Debug + Send + Sync + 'static>(items: Vec<T>) -> &'static [T] {
    static KEPT: OnceLock<Mutex<KeptLists>> = OnceLock::new();
    if items.is_empty() {
        return &[];
    }
    let key = (TypeId::of::<T>(), format!("{items:?}"));
    let mut kept = KEPT
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if let Some(found) = kept
        .get(&key)
        .copied()
        .and_then(|list| list.downcast_ref::<Vec<T>>())
    {
        return found.as_slice();
    }
    let leaked: &'static Vec<T> = Box::leak(Box::new(items));
    kept.insert(key, leaked);
    leaked.as_slice()
}

/// `(value, label)` choices, kept.
pub(crate) fn keep_options<I, V, L>(options: I) -> &'static [(&'static str, &'static str)]
where
    I: IntoIterator<Item = (V, L)>,
    V: AsRef<str>,
    L: AsRef<str>,
{
    keep_slice(
        options
            .into_iter()
            .map(|(value, label)| (keep_str(value.as_ref()), keep_str(label.as_ref())))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_text_is_kept_once() {
        let first = keep_str(&String::from("Expected close date"));
        let again = keep_str(&String::from("Expected close date"));
        assert!(std::ptr::eq(first, again));
        assert_eq!(keep_str(""), "");
    }

    #[test]
    fn an_equal_list_is_the_one_kept_before() {
        let first = keep_options([("lead", "Lead"), ("won", "Won")]);
        let again = keep_options(vec![("lead".to_owned(), "Lead"), ("won".to_owned(), "Won")]);
        assert!(std::ptr::eq(first, again));
        let other = keep_options([("lead", "Lead")]);
        assert!(!std::ptr::eq(first.as_ptr(), other.as_ptr()));
        let none: &[(&str, &str)] = keep_options(Vec::<(&str, &str)>::new());
        assert!(none.is_empty());
    }
}
