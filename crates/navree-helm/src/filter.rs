//! [`Filter`]: how the list narrows.

use std::fmt;
use std::sync::Arc;

use sea_orm::{EntityTrait, Select};

use crate::Resource;
use crate::field::{Related, sentence_case};

/// A query change taking an on/off flag.
type BooleanFn<E> = Arc<dyn Fn(Select<E>, bool) -> Select<E> + Send + Sync>;

/// A query change taking typed-in text.
type CustomFn<E> = Arc<dyn Fn(Select<E>, String) -> Select<E> + Send + Sync>;

/// The kinds of filter.
pub(crate) enum FilterKind<E: EntityTrait> {
    Select,
    BelongsTo(Related),
    Boolean(BooleanFn<E>),
    DateRange,
    Custom(CustomFn<E>),
}

impl<E: EntityTrait> Clone for FilterKind<E> {
    fn clone(&self) -> Self {
        match self {
            FilterKind::Select => FilterKind::Select,
            FilterKind::BelongsTo(related) => FilterKind::BelongsTo(*related),
            FilterKind::Boolean(f) => FilterKind::Boolean(f.clone()),
            FilterKind::DateRange => FilterKind::DateRange,
            FilterKind::Custom(f) => FilterKind::Custom(f.clone()),
        }
    }
}

impl<E: EntityTrait> FilterKind<E> {
    pub(crate) fn name(&self) -> &'static str {
        match self {
            FilterKind::Select => "select",
            FilterKind::BelongsTo(_) => "belongs_to",
            FilterKind::Boolean(_) => "boolean",
            FilterKind::DateRange => "date_range",
            FilterKind::Custom(_) => "custom",
        }
    }
}

/// One filter of a resource's list, in the side panel and the API's
/// `filters[name]` parameters.
///
/// ```ignore
/// Filter::select("stage").options([("lead", "Lead"), ("won", "Won")])
/// Filter::belongs_to::<UserResource>("owner_id").label("Owner")
/// Filter::date_range("expected_close_date")
/// Filter::boolean("open", "Open deals only", |select, on| {
///     if on { select.filter(deal::Column::ClosedAt.is_null()) } else { select }
/// })
/// ```
pub struct Filter<E: EntityTrait> {
    pub(crate) name: String,
    pub(crate) label: Option<String>,
    pub(crate) options: Vec<(String, String)>,
    pub(crate) kind: FilterKind<E>,
}

impl<E: EntityTrait> Filter<E> {
    fn new(name: &str, kind: FilterKind<E>) -> Self {
        Self {
            name: name.to_owned(),
            label: None,
            options: Vec::new(),
            kind,
        }
    }

    /// One or more values of `column`, picked from [`options`](Filter::options).
    pub fn select(column: &str) -> Self {
        Self::new(column, FilterKind::Select)
    }

    /// One or more records of resource `R` (the user may read), matched
    /// against the key in `column`.
    pub fn belongs_to<R: Resource>(column: &str) -> Self {
        Self::new(column, FilterKind::BelongsTo(Related::of::<R>()))
    }

    /// On or off: `f(select, on)` changes the query (`filters[name]=1` or
    /// `0`).
    pub fn boolean<F>(name: &str, label: &str, f: F) -> Self
    where
        F: Fn(Select<E>, bool) -> Select<E> + Send + Sync + 'static,
    {
        Self::new(name, FilterKind::Boolean(Arc::new(f))).label(label)
    }

    /// A range of dates of `column`: `from..to` (either end may be left
    /// out) or a preset: `today`, `week`, `month` or `quarter`.
    pub fn date_range(column: &str) -> Self {
        Self::new(column, FilterKind::DateRange)
    }

    /// A value typed in: `f(select, value)` changes the query.
    pub fn custom<F>(name: &str, label: &str, f: F) -> Self
    where
        F: Fn(Select<E>, String) -> Select<E> + Send + Sync + 'static,
    {
        Self::new(name, FilterKind::Custom(Arc::new(f))).label(label)
    }

    /// The label (default: the name in sentence case).
    pub fn label(mut self, label: &str) -> Self {
        self.label = Some(label.to_owned());
        self
    }

    /// The values of a select filter: `(value, label)` pairs.
    pub fn options<I, K, V>(mut self, options: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.options = options
            .into_iter()
            .map(|(value, label)| (value.into(), label.into()))
            .collect();
        self
    }

    /// The filter's name: its column, or the name given to `boolean` and
    /// `custom` filters (the key in `filters[name]`).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The label on screen.
    pub fn label_text(&self) -> String {
        self.label
            .clone()
            .unwrap_or_else(|| sentence_case(&self.name))
    }
}

impl<E: EntityTrait> Clone for Filter<E> {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            label: self.label.clone(),
            options: self.options.clone(),
            kind: self.kind.clone(),
        }
    }
}

impl<E: EntityTrait> fmt::Debug for Filter<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Filter")
            .field("name", &self.name)
            .field("kind", &self.kind.name())
            .finish_non_exhaustive()
    }
}
