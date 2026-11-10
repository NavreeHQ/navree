//! The [`Searchable`] trait, [`SearchContext`], and the helpers the derive
//! expands to.

use std::fmt;

use navree_db::{Uow, Value};
use navree_kernel::{AppContext, BoxError, BoxFuture};
use navree_orm::{ColumnMeta, Model, PrimaryKey, Record};
use uuid::Uuid;

use crate::{Document, IndexSettings};

/// A navree-orm model whose records can be searched.
///
/// Derive it with `#[derive(Searchable)]` next to `#[derive(Model)]`, or
/// implement it by hand when a document needs data from other tables.
/// Register the model with `SearchModule::for_feature::<M>()`.
///
/// ```ignore
/// impl Searchable for Deal {
///     fn settings() -> IndexSettings {
///         IndexSettings::new("Deal", "deals").text("title", 3).filter("stage")
///     }
///
///     fn document<'a>(
///         deal: &'a Record<Deal>,
///         ctx: &'a SearchContext,
///     ) -> BoxFuture<'a, Result<Option<Document>, BoxError>> {
///         Box::pin(async move {
///             Ok(Some(Document::new(deal.id).field("title", &deal.title).field("stage", deal.stage.as_str())))
///         })
///     }
/// }
/// ```
///
/// Records are read without the model's global scopes (soft-deleted rows
/// are read, and kept out of the index by the index's `soft_delete`
/// column; workspace-scoped models are read in every workspace, each
/// document carrying its workspace).
pub trait Searchable: Model {
    /// The index: its name, fields and their uses.
    fn settings() -> IndexSettings;

    /// The document of `record`; `Ok(None)` keeps it out of the index (and
    /// removes it if it was indexed). Read related rows through
    /// [`ctx.uow()`](SearchContext::uow).
    fn document<'a>(
        record: &'a Record<Self>,
        ctx: &'a SearchContext,
    ) -> BoxFuture<'a, Result<Option<Document>, BoxError>>;
}

/// What [`Searchable::document`] may use: a unit of work reading the app's
/// database (inside the records' workspace, when they have one) and the
/// app.
pub struct SearchContext {
    uow: Uow,
    app: AppContext,
    workspace: Option<Uuid>,
    /// The unit of work was opened for this context (rolled back when the
    /// context ends); otherwise it is the caller's.
    owned: bool,
}

impl SearchContext {
    /// A context reading through `uow`, which stays the caller's to commit
    /// or roll back (to call [`Searchable::document`] in a test).
    pub fn new(uow: Uow, app: AppContext, workspace: Option<Uuid>) -> Self {
        Self {
            uow,
            app,
            workspace,
            owned: false,
        }
    }

    /// A context on a unit of work opened for it, rolled back when it ends.
    pub(crate) fn owning(uow: Uow, app: AppContext, workspace: Option<Uuid>) -> Self {
        Self {
            owned: true,
            ..Self::new(uow, app, workspace)
        }
    }

    /// The unit of work, and whether it was opened for this context.
    pub(crate) fn into_parts(self) -> (Uow, bool) {
        (self.uow, self.owned)
    }

    /// The unit of work to read related rows through
    /// (`Company::find(ctx.uow(), id)`, or navree-db statements).
    pub fn uow(&self) -> &Uow {
        &self.uow
    }

    /// The app, to resolve services.
    pub fn app(&self) -> &AppContext {
        &self.app
    }

    /// The workspace the records being indexed belong to, if any.
    pub fn workspace(&self) -> Option<Uuid> {
        self.workspace
    }
}

impl fmt::Debug for SearchContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SearchContext")
            .field("workspace", &self.workspace)
            .field("owned", &self.owned)
            .finish_non_exhaustive()
    }
}

/// The value of column `column` of `model` as a string (`None` for null or
/// an unknown column).
pub(crate) fn column_string<M: Model>(model: &M, column: &str) -> Option<String> {
    value_string(&model.value(column).ok().flatten()?)
}

/// Whether column `column` of `model` holds a value.
pub(crate) fn column_is_set<M: Model>(model: &M, column: &str) -> bool {
    column_string(model, column).is_some()
}

/// A database value as a string: UUIDs hyphenated, numbers and strings as
/// they are; `None` for null.
pub(crate) fn value_string(value: &Value) -> Option<String> {
    match value.to_json() {
        serde_json::Value::Null => None,
        serde_json::Value::String(s) => Some(s),
        json => Some(json.to_string()),
    }
}

/// The primary key of `model` as a string (what change events name it).
pub fn primary_key_string<M: Model>(model: &M) -> String {
    model.key().to_key_string()
}

/// The key column of `M`, qualified with its table.
pub(crate) fn key_column<M: Model>() -> String {
    let meta = M::meta();
    format!("{}.{}", meta.table(), meta.key_column())
}

/// The value of `M`'s key for id `id`; `None` when `id` cannot be a key.
pub(crate) fn key_value<M: Model>(id: &str) -> Option<Value> {
    let meta = M::meta();
    typed_value(meta.column(meta.key_column())?, id)
}

/// Text `text` as a value of the column `meta` describes (a UUID, an
/// integer, a boolean, else text); `None` when it cannot be one.
pub(crate) fn typed_value(meta: &ColumnMeta, text: &str) -> Option<Value> {
    let rust_type = meta.rust_type();
    let inner = rust_type
        .strip_prefix("Option<")
        .and_then(|rest| rest.strip_suffix('>'))
        .unwrap_or(rust_type);
    match inner.rsplit("::").next().unwrap_or(inner).trim() {
        "Uuid" => Uuid::parse_str(text).ok().map(Value::from),
        "i8" | "i16" | "i32" | "i64" | "isize" => text.parse::<i64>().ok().map(Value::from),
        "u8" | "u16" | "u32" | "u64" | "usize" => text.parse::<u64>().ok().map(Value::from),
        "bool" => text.parse::<bool>().ok().map(Value::from),
        _ => Some(Value::from(text.to_owned())),
    }
}

/// Helpers the `Searchable` derive expands to. Not part of the API.
#[doc(hidden)]
pub mod __private {
    use std::marker::PhantomData;

    pub use navree_kernel::{BoxError, BoxFuture};
    pub use navree_orm::{Model, Record};

    /// The primary key of `record` as a string.
    pub fn primary_key_string<M: Model>(record: &Record<M>) -> String {
        super::primary_key_string(record.model())
    }

    /// The record's columns in the form access rules compare
    /// (`navree_orm::access::record_json`): the derive stores filter and
    /// sort fields this way, so engine filters agree with SQL.
    pub fn columns<M: Model>(record: &Record<M>) -> serde_json::Value {
        navree_orm::access::record_json(record)
    }

    /// Column `name` of [`columns`]' result (`null` when absent).
    pub fn column(columns: &serde_json::Value, name: &str) -> serde_json::Value {
        columns
            .get(name)
            .cloned()
            .unwrap_or(serde_json::Value::Null)
    }

    /// The table of `M`.
    pub fn table_name<M: Model>() -> String {
        M::meta().table().to_owned()
    }

    /// Finds a model's workspace column at the derive site:
    /// `(&&WorkspaceProbe::<M>::new()).workspace_column()` picks
    /// [`ViaScoped`] when `M: navree_workspace::WorkspaceScoped` and
    /// [`ViaPlain`] (the model's own `#[model(workspace)]` column, if any)
    /// otherwise.
    pub struct WorkspaceProbe<M>(PhantomData<fn() -> M>);

    impl<M> WorkspaceProbe<M> {
        /// A probe for `M`.
        #[allow(clippy::new_without_default)]
        pub fn new() -> Self {
            Self(PhantomData)
        }
    }

    /// The workspace column of a `WorkspaceScoped` model.
    pub trait ViaScoped {
        /// `Some(M::WORKSPACE_COLUMN)`.
        fn workspace_column(&self) -> Option<&'static str>;
    }

    impl<M: navree_workspace::WorkspaceScoped> ViaScoped for &WorkspaceProbe<M> {
        fn workspace_column(&self) -> Option<&'static str> {
            Some(M::WORKSPACE_COLUMN)
        }
    }

    /// The model's `#[model(workspace)]` column, if it has one.
    pub trait ViaPlain {
        /// `M::meta().workspace_column()`.
        fn workspace_column(&self) -> Option<&'static str>;
    }

    impl<M: Model> ViaPlain for WorkspaceProbe<M> {
        fn workspace_column(&self) -> Option<&'static str> {
            M::meta().workspace_column()
        }
    }
}
