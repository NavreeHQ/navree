//! The [`Searchable`] trait, [`SearchContext`], and the helpers the derive
//! expands to.

use std::fmt;
use std::sync::Arc;

use navree_db::Uow;
use navree_kernel::{AppContext, BoxError, BoxFuture};
use sea_orm::sea_query::Value as SqlValue;
use sea_orm::sea_query::value::sea_value_to_json_value;
use sea_orm::{
    ColumnTrait, ColumnType, DatabaseTransaction, EntityTrait, IdenStatic, Iterable, ModelTrait,
    PrimaryKeyToColumn,
};
use uuid::Uuid;

use crate::{Document, IndexSettings};

/// An entity whose records can be searched.
///
/// Derive it on a SeaORM model with `#[derive(Searchable)]` (it implements
/// this trait for the module's `Entity`), or implement it by hand when a
/// document needs data from other tables. Register the entity with
/// `SearchModule::for_feature::<E>()`.
///
/// ```ignore
/// impl Searchable for deal::Entity {
///     fn settings() -> IndexSettings {
///         IndexSettings::new("Deal", "deals").text("title", 3).filter("stage")
///     }
///
///     fn document<'a>(
///         deal: &'a deal::Model,
///         ctx: &'a SearchContext,
///     ) -> BoxFuture<'a, Result<Option<Document>, BoxError>> {
///         Box::pin(async move {
///             Ok(Some(Document::new(deal.id).field("title", &deal.title).field("stage", deal.stage.as_str())))
///         })
///     }
/// }
/// ```
pub trait Searchable: EntityTrait<Model: Sync> + Send + Sync + 'static {
    /// The index: its name, fields and their uses.
    fn settings() -> IndexSettings;

    /// The document of `record`; `Ok(None)` keeps it out of the index (and
    /// removes it if it was indexed). Read related rows through
    /// [`ctx.conn()`](SearchContext::conn).
    fn document<'a>(
        record: &'a Self::Model,
        ctx: &'a SearchContext,
    ) -> BoxFuture<'a, Result<Option<Document>, BoxError>>;
}

/// What [`Searchable::document`] may use: a read transaction on the app's
/// database (inside the record's workspace, when it has one) and the app.
pub struct SearchContext {
    tx: Arc<DatabaseTransaction>,
    app: AppContext,
    workspace: Option<Uuid>,
    /// The unit of work the transaction belongs to, when it was opened
    /// for a workspace.
    uow: Option<Uow>,
    /// The transaction belongs to the task's unit of work (not ours to end).
    borrowed: bool,
}

impl SearchContext {
    /// A context reading through `tx` (to call [`Searchable::document`] in
    /// a test).
    pub fn new(tx: Arc<DatabaseTransaction>, app: AppContext, workspace: Option<Uuid>) -> Self {
        Self {
            tx,
            app,
            workspace,
            uow: None,
            borrowed: false,
        }
    }

    /// The transaction is the current unit of work's.
    pub(crate) fn borrowed(mut self) -> Self {
        self.borrowed = true;
        self
    }

    /// The transaction belongs to `uow`.
    pub(crate) fn with_uow(mut self, uow: Uow) -> Self {
        self.uow = Some(uow);
        self
    }

    /// The transaction, its unit of work and whether it is borrowed.
    pub(crate) fn into_parts(self) -> (Arc<DatabaseTransaction>, Option<Uow>, bool) {
        (self.tx, self.uow, self.borrowed)
    }

    /// The connection to read related rows with (`.one(ctx.conn())`).
    pub fn conn(&self) -> &DatabaseTransaction {
        &self.tx
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
            .finish_non_exhaustive()
    }
}

/// The value of column `column` of `model` as a string (`None` for null or
/// an unknown column).
pub(crate) fn column_string<M: ModelTrait>(model: &M, column: &str) -> Option<String> {
    let column = <M::Entity as EntityTrait>::Column::iter().find(|c| c.as_str() == column)?;
    value_string(&model.get(column))
}

/// Whether column `column` of `model` holds a value.
pub(crate) fn column_is_set<M: ModelTrait>(model: &M, column: &str) -> bool {
    column_string(model, column).is_some()
}

/// A database value as a string: UUIDs hyphenated, numbers and strings as
/// they are; `None` for null.
pub(crate) fn value_string(value: &SqlValue) -> Option<String> {
    match value {
        SqlValue::Uuid(Some(uuid)) => Some(uuid.hyphenated().to_string()),
        other => match sea_value_to_json_value(other) {
            serde_json::Value::Null => None,
            serde_json::Value::String(s) => Some(s),
            json => Some(json.to_string()),
        },
    }
}

/// The primary key of `model` as a string (composite keys joined with `,`).
pub fn primary_key_string<M: ModelTrait>(model: &M) -> String {
    <M::Entity as EntityTrait>::PrimaryKey::iter()
        .map(|key| value_string(&model.get(key.into_column())).unwrap_or_default())
        .collect::<Vec<_>>()
        .join(",")
}

/// The single primary key column of `E`.
pub(crate) fn key_column<E: EntityTrait>() -> Result<E::Column, crate::SearchError> {
    let mut keys = E::PrimaryKey::iter();
    match (keys.next(), keys.next()) {
        (Some(key), None) => Ok(key.into_column()),
        _ => Err(crate::SearchError::Other(format!(
            "`{}` needs a single-column primary key to be searchable",
            E::default().table_name()
        ))),
    }
}

/// The key value for id `id`, typed like `column`; `None` when `id` cannot
/// be a key of that type.
pub(crate) fn key_value<C: ColumnTrait>(column: &C, id: &str) -> Option<SqlValue> {
    match column.def().get_column_type() {
        ColumnType::Uuid => Uuid::parse_str(id).ok().map(SqlValue::from),
        ColumnType::TinyInteger
        | ColumnType::SmallInteger
        | ColumnType::Integer
        | ColumnType::BigInteger
        | ColumnType::TinyUnsigned
        | ColumnType::SmallUnsigned
        | ColumnType::Unsigned
        | ColumnType::BigUnsigned => id.parse::<i64>().ok().map(SqlValue::from),
        _ => Some(SqlValue::from(id.to_owned())),
    }
}

/// Helpers the `Searchable` derive expands to. Not part of the API.
#[doc(hidden)]
pub mod __private {
    use std::marker::PhantomData;

    pub use navree_kernel::{BoxError, BoxFuture};
    pub use sea_orm;

    pub use super::primary_key_string;

    /// The table name of `E`.
    pub fn table_name<E: sea_orm::EntityTrait>() -> String {
        sea_orm::EntityName::table_name(&E::default()).to_owned()
    }

    /// Finds an entity's `WorkspaceScoped` column at the derive site:
    /// `(&&WorkspaceProbe::<Entity>::new()).workspace_column()` picks
    /// [`ViaScoped`] when `Entity: WorkspaceScoped` and [`ViaPlain`]
    /// otherwise.
    pub struct WorkspaceProbe<E>(PhantomData<fn() -> E>);

    impl<E> WorkspaceProbe<E> {
        /// A probe for `E`.
        #[allow(clippy::new_without_default)]
        pub fn new() -> Self {
            Self(PhantomData)
        }
    }

    /// The workspace column of a `WorkspaceScoped` entity.
    pub trait ViaScoped {
        /// `Some(E::WORKSPACE_COLUMN)`.
        fn workspace_column(&self) -> Option<&'static str>;
    }

    impl<E: navree_workspace::WorkspaceScoped> ViaScoped for &WorkspaceProbe<E> {
        fn workspace_column(&self) -> Option<&'static str> {
            Some(E::WORKSPACE_COLUMN)
        }
    }

    /// No workspace column.
    pub trait ViaPlain {
        /// `None`.
        fn workspace_column(&self) -> Option<&'static str>;
    }

    impl<E> ViaPlain for WorkspaceProbe<E> {
        fn workspace_column(&self) -> Option<&'static str> {
            None
        }
    }
}
