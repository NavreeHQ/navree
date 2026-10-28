//! [`LibraryColumns`]: the columns, indexes and `CHECK` constraint that make
//! a table a library, for a schema-builder migration.

use navree_db::navree_schema::{Blueprint, ColumnBuilder, Dialect};

use super::{Library, LibraryScope};

/// What `cloned_from_id` holds: the key of the library's model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeyColumn {
    Uuid,
    BigInteger,
}

/// The columns that make a table a library, added in the migration that
/// creates it:
///
/// - `scope`, a string of up to 16 characters (`global`, `organization`,
///   `workspace`; see [`LibraryScope`]);
/// - `organization_id` and `workspace_id`, nullable UUIDs;
/// - `cloned_from_id`, the nullable key of the item a clone was made from;
/// - an index on each;
/// - the `CHECK` constraint `<table>_library_owner_check`: a global row
///   has neither id, an organization's row its organization only, a
///   workspace's row its workspace only.
///
/// UUIDs are `BINARY(16)` on MySQL and MariaDB (what navree-orm writes
/// there) and native elsewhere. Models keyed by integers say
/// [`integer_keys`](Self::integer_keys).
///
/// ```
/// use navree_db::navree_schema::{Blueprint, Command, Dialect};
/// use navree_workspace::library::LibraryColumns;
///
/// let mut t = Blueprint::new("templates");
/// t.uuid("id").primary();
/// LibraryColumns::new(Dialect::Postgres).add(&mut t);
/// t.string("name");
/// let names: Vec<&str> = t.columns().iter().map(|c| c.name.as_str()).collect();
/// assert_eq!(names, ["id", "scope", "organization_id", "workspace_id", "cloned_from_id", "name"]);
/// assert!(t.commands().iter().any(|c| matches!(c, Command::Check(check) if check.name == "templates_library_owner_check")));
/// ```
///
/// In a migration (`schema.target()` tells the dialect):
///
/// ```no_run
/// use navree_db::navree_schema::{Migration, Schema, SchemaResult};
/// use navree_workspace::library::LibraryColumns;
///
/// struct CreateTemplates;
///
/// impl Migration for CreateTemplates {
///     async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
///         let library = LibraryColumns::new(schema.target().await?.dialect());
///         schema
///             .create("templates", |t| {
///                 library.uuid(t, "id").primary();
///                 library.add(t);
///                 t.string("name");
///                 t.timestamps_tz();
///             })
///             .await
///     }
///
///     async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
///         schema.drop_if_exists("templates").await
///     }
/// }
/// ```
#[derive(Clone, Debug)]
pub struct LibraryColumns {
    dialect: Dialect,
    scope: &'static str,
    organization: &'static str,
    workspace: &'static str,
    cloned_from: &'static str,
    key: KeyColumn,
}

impl LibraryColumns {
    /// The columns under their default names, for `dialect`.
    pub fn new(dialect: Dialect) -> Self {
        LibraryColumns {
            dialect,
            scope: "scope",
            organization: "organization_id",
            workspace: "workspace_id",
            cloned_from: "cloned_from_id",
            key: KeyColumn::Uuid,
        }
    }

    /// The columns under the names model `M` declares
    /// ([`Library::SCOPE_COLUMN`] and the others).
    pub fn of<M: Library>(dialect: Dialect) -> Self {
        LibraryColumns {
            scope: M::SCOPE_COLUMN,
            organization: M::ORGANIZATION_COLUMN,
            workspace: M::WORKSPACE_COLUMN,
            cloned_from: M::CLONED_FROM_COLUMN,
            ..LibraryColumns::new(dialect)
        }
    }

    /// `cloned_from_id` as a big integer, for models keyed by integers.
    pub fn integer_keys(mut self) -> Self {
        self.key = KeyColumn::BigInteger;
        self
    }

    /// A UUID column `name` as navree-orm stores one on this database
    /// (`BINARY(16)` on MySQL and MariaDB): for the table's own key and
    /// other UUID columns.
    pub fn uuid<'a>(&self, t: &'a mut Blueprint, name: &str) -> ColumnBuilder<'a> {
        if self.dialect == Dialect::MySql {
            t.binary(name).length(16).fixed()
        } else {
            t.uuid(name)
        }
    }

    /// Adds the columns, their indexes and the `CHECK` constraint to `t`.
    pub fn add(&self, t: &mut Blueprint) {
        let table = t.table().rsplit('.').next().unwrap_or_default().to_owned();
        t.string(self.scope).length(16).index();
        self.uuid(t, self.organization).nullable().index();
        self.uuid(t, self.workspace).nullable().index();
        match self.key {
            KeyColumn::Uuid => self.uuid(t, self.cloned_from).nullable().index(),
            KeyColumn::BigInteger => t.big_integer(self.cloned_from).nullable().index(),
        };
        t.check(format!("{table}_library_owner_check"), self.check_expression());
    }

    /// The `CHECK` constraint's condition.
    ///
    /// ```
    /// use navree_db::navree_schema::Dialect;
    /// use navree_workspace::library::LibraryColumns;
    ///
    /// assert_eq!(
    ///     LibraryColumns::new(Dialect::Postgres).check_expression(),
    ///     "(\"scope\" = 'global' AND \"organization_id\" IS NULL AND \"workspace_id\" IS NULL) \
    ///      OR (\"scope\" = 'organization' AND \"organization_id\" IS NOT NULL AND \"workspace_id\" IS NULL) \
    ///      OR (\"scope\" = 'workspace' AND \"workspace_id\" IS NOT NULL AND \"organization_id\" IS NULL)"
    /// );
    /// ```
    pub fn check_expression(&self) -> String {
        let scope = self.quote(self.scope);
        let organization = self.quote(self.organization);
        let workspace = self.quote(self.workspace);
        let global = LibraryScope::Global.as_str();
        let shared = LibraryScope::Organization.as_str();
        let own = LibraryScope::Workspace.as_str();
        format!(
            "({scope} = '{global}' AND {organization} IS NULL AND {workspace} IS NULL) \
             OR ({scope} = '{shared}' AND {organization} IS NOT NULL AND {workspace} IS NULL) \
             OR ({scope} = '{own}' AND {workspace} IS NOT NULL AND {organization} IS NULL)"
        )
    }

    /// `name` quoted for the dialect.
    fn quote(&self, name: &str) -> String {
        match self.dialect {
            Dialect::MySql => format!("`{}`", name.replace('`', "``")),
            Dialect::Postgres | Dialect::Sqlite => format!("\"{}\"", name.replace('"', "\"\"")),
        }
    }
}
