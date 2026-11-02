//! [`Filter`]: how the list narrows.

use std::fmt;
use std::sync::Arc;

use navree_orm::{Builder, ColumnName, Model};

use crate::Resource;
use crate::field::{Related, sentence_case};

/// A query change taking an on/off flag.
type BooleanFn<M> = Arc<dyn Fn(Builder<M>, bool) -> Builder<M> + Send + Sync>;

/// A query change taking typed-in text.
type CustomFn<M> = Arc<dyn Fn(Builder<M>, String) -> Builder<M> + Send + Sync>;

/// The kinds of filter.
pub(crate) enum FilterKind<M: Model> {
    Select,
    BelongsTo(Related),
    Boolean(BooleanFn<M>),
    DateRange,
    Custom(CustomFn<M>),
}

impl<M: Model> Clone for FilterKind<M> {
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

impl<M: Model> FilterKind<M> {
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
/// Filter::boolean("open", "Open deals only", |query, on| {
///     if on { query.where_(Deal::CLOSED_AT.is_null()) } else { query }
/// })
/// ```
pub struct Filter<M: Model> {
    pub(crate) name: String,
    pub(crate) label: Option<String>,
    pub(crate) options: Vec<(String, String)>,
    pub(crate) kind: FilterKind<M>,
    pub(crate) reads: Option<Vec<String>>,
}

impl<M: Model> Filter<M> {
    fn new(name: &str, kind: FilterKind<M>) -> Self {
        Self {
            name: name.to_owned(),
            label: None,
            options: Vec::new(),
            kind,
            reads: None,
        }
    }

    /// The columns the code of a [`boolean`](Filter::boolean) or
    /// [`custom`](Filter::custom) filter reads. A match on a column says something
    /// about it, so the panel offers the filter only to a user who may read them.
    /// A filter that doesn't say is offered only to a user with no column hidden from
    /// them, since it could be reading any.
    pub fn reads<C: ColumnName>(mut self, columns: impl IntoIterator<Item = C>) -> Self {
        self.reads = Some(
            columns
                .into_iter()
                .map(|column| column.column_name().into_owned())
                .collect(),
        );
        self
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

    /// On or off: `f(query, on)` changes the model query (`filters[name]=1` or
    /// `0`).
    pub fn boolean<F>(name: &str, label: &str, f: F) -> Self
    where
        F: Fn(Builder<M>, bool) -> Builder<M> + Send + Sync + 'static,
    {
        Self::new(name, FilterKind::Boolean(Arc::new(f))).label(label)
    }

    /// A range of dates of `column`: `from..to` (either end may be left
    /// out) or a preset: `today`, `week`, `month` or `quarter`.
    pub fn date_range(column: &str) -> Self {
        Self::new(column, FilterKind::DateRange)
    }

    /// A value typed in: `f(query, value)` changes the model query.
    pub fn custom<F>(name: &str, label: &str, f: F) -> Self
    where
        F: Fn(Builder<M>, String) -> Builder<M> + Send + Sync + 'static,
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

impl<M: Model> Clone for Filter<M> {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            label: self.label.clone(),
            options: self.options.clone(),
            kind: self.kind.clone(),
            reads: self.reads.clone(),
        }
    }
}

impl<M: Model> fmt::Debug for Filter<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Filter")
            .field("name", &self.name)
            .field("kind", &self.kind.name())
            .finish_non_exhaustive()
    }
}
