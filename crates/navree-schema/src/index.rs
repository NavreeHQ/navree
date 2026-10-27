//! Indexes and foreign keys: [`IndexDefinition`], [`ForeignKeyDefinition`],
//! their builders, [`ReferentialAction`], and the [`Columns`] and
//! [`IndexRef`] argument types.

use std::fmt;

use crate::value::Expression;

/// The kind of an index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IndexKind {
    /// The primary key.
    Primary,
    /// A unique index.
    Unique,
    /// A plain index.
    Index,
    /// A full-text index.
    Fulltext,
    /// A spatial index.
    Spatial,
}

impl IndexKind {
    /// The suffix of conventional names: `deals_title_index`.
    pub fn suffix(self) -> &'static str {
        match self {
            IndexKind::Primary => "primary",
            IndexKind::Unique => "unique",
            IndexKind::Index => "index",
            IndexKind::Fulltext => "fulltext",
            IndexKind::Spatial => "spatialindex",
        }
    }
}

impl fmt::Display for IndexKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            IndexKind::Primary => "primary key",
            IndexKind::Unique => "unique index",
            IndexKind::Index => "index",
            IndexKind::Fulltext => "full-text index",
            IndexKind::Spatial => "spatial index",
        })
    }
}

/// Column names: `"email"`, `["workspace_id", "email"]`, a slice or a
/// `Vec`.
pub trait Columns {
    /// The names, in order.
    fn into_columns(self) -> Vec<String>;
}

impl Columns for &str {
    fn into_columns(self) -> Vec<String> {
        vec![self.to_owned()]
    }
}

impl Columns for String {
    fn into_columns(self) -> Vec<String> {
        vec![self]
    }
}

impl Columns for &String {
    fn into_columns(self) -> Vec<String> {
        vec![self.clone()]
    }
}

impl<const N: usize> Columns for [&str; N] {
    fn into_columns(self) -> Vec<String> {
        self.iter().map(|c| (*c).to_owned()).collect()
    }
}

impl<const N: usize> Columns for [String; N] {
    fn into_columns(self) -> Vec<String> {
        self.into_iter().collect()
    }
}

impl Columns for &[&str] {
    fn into_columns(self) -> Vec<String> {
        self.iter().map(|c| (*c).to_owned()).collect()
    }
}

impl Columns for Vec<&str> {
    fn into_columns(self) -> Vec<String> {
        self.into_iter().map(str::to_owned).collect()
    }
}

impl Columns for Vec<String> {
    fn into_columns(self) -> Vec<String> {
        self
    }
}

impl Columns for &[String] {
    fn into_columns(self) -> Vec<String> {
        self.to_vec()
    }
}

/// An index to drop: by name (`"deals_title_index"`) or by its columns
/// (`["title"]`, which means the conventional name).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IndexRef {
    /// The index name.
    Name(String),
    /// The indexed columns.
    Columns(Vec<String>),
}

impl From<&str> for IndexRef {
    fn from(name: &str) -> IndexRef {
        IndexRef::Name(name.to_owned())
    }
}

impl From<String> for IndexRef {
    fn from(name: String) -> IndexRef {
        IndexRef::Name(name)
    }
}

impl<const N: usize> From<[&str; N]> for IndexRef {
    fn from(columns: [&str; N]) -> IndexRef {
        IndexRef::Columns(columns.into_columns())
    }
}

impl From<&[&str]> for IndexRef {
    fn from(columns: &[&str]) -> IndexRef {
        IndexRef::Columns(columns.into_columns())
    }
}

impl From<Vec<&str>> for IndexRef {
    fn from(columns: Vec<&str>) -> IndexRef {
        IndexRef::Columns(columns.into_columns())
    }
}

impl From<Vec<String>> for IndexRef {
    fn from(columns: Vec<String>) -> IndexRef {
        IndexRef::Columns(columns)
    }
}

/// An index of a blueprint.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct IndexDefinition {
    /// The kind.
    pub kind: IndexKind,
    /// The index (or constraint) name.
    pub name: String,
    /// The indexed columns.
    pub columns: Vec<String>,
    /// An expression indexed instead of columns ([`raw_index`](crate::Blueprint::raw_index)).
    pub expression: Option<Expression>,
    /// The index method: `btree`, `hash`, `gin`, `gist`, `brin` ...
    pub algorithm: Option<String>,
    /// A PostgreSQL operator class applied to every column.
    pub operator_class: Option<String>,
    /// The text-search configuration of a PostgreSQL full-text index
    /// (`english` by default).
    pub language: Option<String>,
    /// `DEFERRABLE` / `NOT DEFERRABLE` (PostgreSQL unique constraints).
    pub deferrable: Option<bool>,
    /// `INITIALLY IMMEDIATE` / `INITIALLY DEFERRED` (with `deferrable`).
    pub initially_immediate: Option<bool>,
    /// `NULLS NOT DISTINCT` / `NULLS DISTINCT` (PostgreSQL 15+).
    pub nulls_not_distinct: Option<bool>,
    /// Build without locking writes (PostgreSQL `CONCURRENTLY`; the
    /// migration must then run outside a transaction).
    pub online: bool,
    /// A partial index's condition (PostgreSQL and SQLite).
    pub predicate: Option<Expression>,
    /// The DDL lock mode (MySQL): `none`, `shared`, `default`, `exclusive`.
    pub lock: Option<String>,
    /// `ALGORITHM=INPLACE` (MySQL).
    pub inplace: bool,
}

impl IndexDefinition {
    /// An index of `kind` named `name` on `columns`.
    pub fn new(kind: IndexKind, name: impl Into<String>, columns: Vec<String>) -> IndexDefinition {
        IndexDefinition {
            kind,
            name: name.into(),
            columns,
            expression: None,
            algorithm: None,
            operator_class: None,
            language: None,
            deferrable: None,
            initially_immediate: None,
            nulls_not_distinct: None,
            online: false,
            predicate: None,
            lock: None,
            inplace: false,
        }
    }
}

/// The fluent handle of an index: `t.unique(["workspace_id", "email"]).name("members_email")`.
pub struct IndexBuilder<'a> {
    index: &'a mut IndexDefinition,
}

impl<'a> IndexBuilder<'a> {
    pub(crate) fn new(index: &'a mut IndexDefinition) -> IndexBuilder<'a> {
        IndexBuilder { index }
    }

    /// The index definition as built so far.
    pub fn definition(&self) -> &IndexDefinition {
        self.index
    }

    /// Names the index (instead of `{table}_{columns}_{kind}`).
    pub fn name(self, name: impl Into<String>) -> Self {
        self.index.name = name.into();
        self
    }

    /// The index method: `btree`, `hash`, `gin`, `gist`, `brin`, ...
    pub fn algorithm(self, algorithm: impl Into<String>) -> Self {
        self.index.algorithm = Some(algorithm.into());
        self
    }

    /// A PostgreSQL operator class for the indexed columns
    /// (`text_pattern_ops`, `vector_cosine_ops`, ...).
    pub fn operator_class(self, class: impl Into<String>) -> Self {
        self.index.operator_class = Some(class.into());
        self
    }

    /// The text-search configuration of a PostgreSQL full-text index.
    pub fn language(self, language: impl Into<String>) -> Self {
        self.index.language = Some(language.into());
        self
    }

    /// A deferrable unique constraint (PostgreSQL).
    pub fn deferrable(self) -> Self {
        self.index.deferrable = Some(true);
        self
    }

    /// Checked at the end of the statement even when deferrable.
    pub fn initially_immediate(self) -> Self {
        self.index.initially_immediate = Some(true);
        self
    }

    /// Checked at commit (with [`deferrable`](Self::deferrable)).
    pub fn initially_deferred(self) -> Self {
        self.index.initially_immediate = Some(false);
        self
    }

    /// Treats `NULL`s as equal, so at most one row may be `NULL`
    /// (PostgreSQL 15+).
    pub fn nulls_not_distinct(self) -> Self {
        self.index.nulls_not_distinct = Some(true);
        self
    }

    /// Builds the index without blocking writes (PostgreSQL `CONCURRENTLY`;
    /// run the migration outside a transaction).
    pub fn online(self) -> Self {
        self.index.online = true;
        self
    }

    /// A partial index over the rows matching `predicate` (PostgreSQL and
    /// SQLite): `.where_raw("deleted_at IS NULL")`.
    pub fn where_raw(self, predicate: impl Into<Expression>) -> Self {
        self.index.predicate = Some(predicate.into());
        self
    }

    /// The DDL lock mode (MySQL): `none`, `shared`, `default`, `exclusive`.
    pub fn lock(self, mode: impl Into<String>) -> Self {
        self.index.lock = Some(mode.into());
        self
    }

    /// `ALGORITHM=INPLACE` (MySQL).
    pub fn inplace(self) -> Self {
        self.index.inplace = true;
        self
    }
}

/// What happens to referencing rows when the referenced row is deleted or
/// its key updated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReferentialAction {
    /// Delete or update the referencing rows too.
    Cascade,
    /// Refuse while referencing rows exist.
    Restrict,
    /// Set the referencing columns to `NULL`.
    SetNull,
    /// Set the referencing columns to their default.
    SetDefault,
    /// Check at the end of the statement (the SQL default).
    NoAction,
}

impl ReferentialAction {
    /// The SQL keywords: `CASCADE`, `SET NULL`, ...
    pub fn sql(self) -> &'static str {
        match self {
            ReferentialAction::Cascade => "CASCADE",
            ReferentialAction::Restrict => "RESTRICT",
            ReferentialAction::SetNull => "SET NULL",
            ReferentialAction::SetDefault => "SET DEFAULT",
            ReferentialAction::NoAction => "NO ACTION",
        }
    }

    /// Parses the catalogs' spellings (`cascade`, `SET NULL`, `a`/`c`/`n`
    /// from PostgreSQL's `pg_constraint`).
    pub fn parse(text: &str) -> Option<ReferentialAction> {
        let action = match text.trim().to_ascii_lowercase().as_str() {
            "cascade" | "c" => ReferentialAction::Cascade,
            "restrict" | "r" => ReferentialAction::Restrict,
            "set null" | "n" => ReferentialAction::SetNull,
            "set default" | "d" => ReferentialAction::SetDefault,
            "no action" | "a" => ReferentialAction::NoAction,
            _ => return None,
        };
        Some(action)
    }
}

impl fmt::Display for ReferentialAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.sql())
    }
}

/// A foreign key of a blueprint.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct ForeignKeyDefinition {
    /// The constraint name.
    pub name: String,
    /// The referencing columns.
    pub columns: Vec<String>,
    /// The referenced table.
    pub on: Option<String>,
    /// The referenced columns.
    pub references: Vec<String>,
    /// The action on delete.
    pub on_delete: Option<ReferentialAction>,
    /// The action on update.
    pub on_update: Option<ReferentialAction>,
    /// `DEFERRABLE` / `NOT DEFERRABLE` (PostgreSQL).
    pub deferrable: Option<bool>,
    /// `INITIALLY IMMEDIATE` / `INITIALLY DEFERRED` (with `deferrable`).
    pub initially_immediate: Option<bool>,
    /// `NOT VALID`: skip checking existing rows (PostgreSQL).
    pub not_valid: bool,
}

impl ForeignKeyDefinition {
    /// A foreign key named `name` on `columns`, not yet pointing anywhere.
    pub fn new(name: impl Into<String>, columns: Vec<String>) -> ForeignKeyDefinition {
        ForeignKeyDefinition {
            name: name.into(),
            columns,
            on: None,
            references: Vec::new(),
            on_delete: None,
            on_update: None,
            deferrable: None,
            initially_immediate: None,
            not_valid: false,
        }
    }
}

/// The fluent handle of a foreign key:
/// `t.foreign("company_id").references("id").on("companies").cascade_on_delete()`.
pub struct ForeignKeyBuilder<'a> {
    foreign: &'a mut ForeignKeyDefinition,
}

impl<'a> ForeignKeyBuilder<'a> {
    pub(crate) fn new(foreign: &'a mut ForeignKeyDefinition) -> ForeignKeyBuilder<'a> {
        ForeignKeyBuilder { foreign }
    }

    /// The foreign key as built so far.
    pub fn definition(&self) -> &ForeignKeyDefinition {
        self.foreign
    }

    /// The referenced columns.
    pub fn references(self, columns: impl Columns) -> Self {
        self.foreign.references = columns.into_columns();
        self
    }

    /// The referenced table.
    pub fn on(self, table: impl Into<String>) -> Self {
        self.foreign.on = Some(table.into());
        self
    }

    /// Names the constraint (instead of `{table}_{columns}_foreign`).
    pub fn name(self, name: impl Into<String>) -> Self {
        self.foreign.name = name.into();
        self
    }

    /// The action when the referenced row is deleted.
    pub fn on_delete(self, action: ReferentialAction) -> Self {
        self.foreign.on_delete = Some(action);
        self
    }

    /// The action when the referenced key is updated.
    pub fn on_update(self, action: ReferentialAction) -> Self {
        self.foreign.on_update = Some(action);
        self
    }

    /// Deletes referencing rows with the referenced row.
    pub fn cascade_on_delete(self) -> Self {
        self.on_delete(ReferentialAction::Cascade)
    }

    /// Refuses to delete a referenced row.
    pub fn restrict_on_delete(self) -> Self {
        self.on_delete(ReferentialAction::Restrict)
    }

    /// Sets the referencing columns to `NULL` when the referenced row is
    /// deleted (make the column nullable).
    pub fn null_on_delete(self) -> Self {
        self.on_delete(ReferentialAction::SetNull)
    }

    /// `ON DELETE NO ACTION`.
    pub fn no_action_on_delete(self) -> Self {
        self.on_delete(ReferentialAction::NoAction)
    }

    /// Updates referencing columns with the referenced key.
    pub fn cascade_on_update(self) -> Self {
        self.on_update(ReferentialAction::Cascade)
    }

    /// Refuses to change a referenced key.
    pub fn restrict_on_update(self) -> Self {
        self.on_update(ReferentialAction::Restrict)
    }

    /// Sets the referencing columns to `NULL` when the referenced key
    /// changes.
    pub fn null_on_update(self) -> Self {
        self.on_update(ReferentialAction::SetNull)
    }

    /// `ON UPDATE NO ACTION`.
    pub fn no_action_on_update(self) -> Self {
        self.on_update(ReferentialAction::NoAction)
    }

    /// A deferrable constraint (PostgreSQL).
    pub fn deferrable(self) -> Self {
        self.foreign.deferrable = Some(true);
        self
    }

    /// Checked at the end of each statement even when deferrable.
    pub fn initially_immediate(self) -> Self {
        self.foreign.initially_immediate = Some(true);
        self
    }

    /// Checked at commit (with [`deferrable`](Self::deferrable)).
    pub fn initially_deferred(self) -> Self {
        self.foreign.initially_immediate = Some(false);
        self
    }

    /// Adds the constraint without checking existing rows (PostgreSQL
    /// `NOT VALID`; validate later with `ALTER TABLE ... VALIDATE
    /// CONSTRAINT`).
    pub fn not_valid(self) -> Self {
        self.foreign.not_valid = true;
        self
    }
}
