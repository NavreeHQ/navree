//! Database rules: [`AsyncRules`], [`AsyncRule`] (`unique`, `exists`, or a
//! [`UowCheck`] of its own) and [`RuleError`].

use std::fmt;
use std::sync::Arc;

use navree_db::{DbError, Executor, Statement, Uow, Value};
use navree_kernel::{BoxError, BoxFuture};
use uuid::Uuid;

/// The column `unique` compares with `ignore_id`.
const ID_COLUMN: &str = "id";

/// Database rules of a DTO, run by [`ValidAsync`](crate::ValidAsync) after
/// its rules pass, inside the request's unit of work.
///
/// ```
/// use navree_validation::{AsyncRule, AsyncRules};
///
/// struct CreateUser {
///     email: String,
///     team_id: Option<uuid::Uuid>,
/// }
///
/// impl AsyncRules for CreateUser {
///     fn async_rules(&self) -> Vec<AsyncRule> {
///         vec![
///             AsyncRule::unique("email", "users", "email", self.email.clone(), None),
///             // `None` skips the rule.
///             AsyncRule::exists("team_id", "teams", "id", self.team_id),
///         ]
///     }
/// }
/// ```
pub trait AsyncRules {
    /// The rules to check, each naming the field its error goes to.
    fn async_rules(&self) -> Vec<AsyncRule>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Unique,
    Exists,
}

impl Kind {
    fn rule(self) -> &'static str {
        match self {
            Kind::Unique => "unique",
            Kind::Exists => "exists",
        }
    }
}

/// The check of a rule that runs its own queries in the unit of work:
/// what rules on ORM models build on (navree-orm's `validation::unique`
/// and `validation::exists`, whose queries keep the model's scopes, such
/// as its workspace). Wrap one with [`AsyncRule::check`].
///
/// ```
/// use navree_db::Uow;
/// use navree_kernel::{BoxError, BoxFuture};
/// use navree_validation::{AsyncRule, UowCheck};
///
/// /// Only even numbers (a check that needs no query).
/// struct Even(i64);
///
/// impl UowCheck for Even {
///     fn passes<'a>(&'a self, _uow: &'a Uow, _ignore: Option<&'a str>) -> BoxFuture<'a, Result<bool, BoxError>> {
///         Box::pin(async move { Ok(self.0 % 2 == 0) })
///     }
/// }
///
/// let rule = AsyncRule::check("count", "even", Even(4));
/// assert_eq!((rule.field(), rule.rule()), ("count", "even"));
/// ```
pub trait UowCheck: Send + Sync + 'static {
    /// Whether the rule passes, checked in `uow`. `ignore` is the value of
    /// the path parameter named by [`AsyncRule::ignore_path_param`] (under
    /// [`ValidAsync`](crate::ValidAsync)), if any: the record being
    /// updated, which a uniqueness check skips.
    fn passes<'a>(
        &'a self,
        uow: &'a Uow,
        ignore: Option<&'a str>,
    ) -> BoxFuture<'a, Result<bool, BoxError>>;
}

/// How a rule is checked.
#[derive(Clone)]
enum Check {
    /// `unique` or `exists` on a table and column.
    Table {
        kind: Kind,
        table: String,
        column: String,
        value: Value,
        ignore_id: Option<Uuid>,
        /// Rows with this column set are left out (soft-deleted rows).
        deleted: Option<String>,
    },
    /// A check of its own, in the unit of work.
    Uow {
        rule: &'static str,
        check: Arc<dyn UowCheck>,
    },
}

impl fmt::Debug for Check {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Check::Table {
                kind,
                table,
                column,
                value,
                ignore_id,
                deleted,
            } => f
                .debug_struct("Table")
                .field("kind", kind)
                .field("table", table)
                .field("column", column)
                .field("value", value)
                .field("ignore_id", ignore_id)
                .field("deleted", deleted)
                .finish(),
            Check::Uow { rule, .. } => f.debug_struct("Uow").field("rule", rule).finish(),
        }
    }
}

/// A database rule on one field: `unique` (no row has the value) or
/// `exists` (some row has it), checked with a parameterised
/// `SELECT 1 FROM <table> WHERE <column> = ? [AND id <> ?] [AND <deleted>
/// IS NULL] LIMIT 1`; or a [`UowCheck`] of its own ([`AsyncRule::check`]).
///
/// Table and column names must match `[a-z_][a-z0-9_]*`; anything else
/// fails with [`RuleError::InvalidIdentifier`] before any SQL runs. A null
/// value (`None`) passes, so optional fields stay optional. Bind
/// values with their column's type (`Uuid` for UUID columns).
#[derive(Clone, Debug)]
pub struct AsyncRule {
    field: String,
    check: Check,
    null: bool,
    ignore_path_param: Option<String>,
}

impl AsyncRule {
    /// `field` must be unique in `table.column`, ignoring the row whose `id`
    /// is `ignore_id` (the record being updated).
    pub fn unique(
        field: impl Into<String>,
        table: impl Into<String>,
        column: impl Into<String>,
        value: impl Into<Value>,
        ignore_id: Option<Uuid>,
    ) -> Self {
        Self::table(Kind::Unique, field, table, column, value.into(), ignore_id)
    }

    /// `field` must exist in `table.column`.
    pub fn exists(
        field: impl Into<String>,
        table: impl Into<String>,
        column: impl Into<String>,
        value: impl Into<Value>,
    ) -> Self {
        Self::table(Kind::Exists, field, table, column, value.into(), None)
    }

    fn table(
        kind: Kind,
        field: impl Into<String>,
        table: impl Into<String>,
        column: impl Into<String>,
        value: Value,
        ignore_id: Option<Uuid>,
    ) -> Self {
        Self {
            field: field.into(),
            null: value.is_null(),
            check: Check::Table {
                kind,
                table: table.into(),
                column: column.into(),
                value,
                ignore_id,
                deleted: None,
            },
            ignore_path_param: None,
        }
    }

    /// Leaves out the rows whose `column` is set, the soft-deleted rows of a
    /// table that marks deletions there (`deleted_at`): `exists` does not
    /// find a deleted row, and `unique` does not count one. The column name
    /// is held to the same rules as the table's. Has no effect on a
    /// [`check`](AsyncRule::check) rule, whose own query decides (an ORM
    /// model's keeps its soft-delete scope).
    ///
    /// ```
    /// use navree_validation::AsyncRule;
    ///
    /// let company_id = uuid::Uuid::now_v7();
    /// let rule = AsyncRule::exists("company_id", "companies", "id", company_id)
    ///     .without_deleted("deleted_at");
    /// assert_eq!(rule.rule(), "exists");
    /// ```
    pub fn without_deleted(mut self, column: impl Into<String>) -> Self {
        if let Check::Table { deleted, .. } = &mut self.check {
            *deleted = Some(column.into());
        }
        self
    }

    /// `field` passes when `check` does. `rule` is the rule key, which
    /// names the error message (`"unique"`: "has already been taken").
    pub fn check(field: impl Into<String>, rule: &'static str, check: impl UowCheck) -> Self {
        Self {
            field: field.into(),
            check: Check::Uow {
                rule,
                check: Arc::new(check),
            },
            null: false,
            ignore_path_param: None,
        }
    }

    /// Marks the rule's value as null: the rule passes without running, so
    /// optional fields stay optional (for [`check`](AsyncRule::check)
    /// rules, whose value the rule cannot see).
    pub fn skipped_when(mut self, null: bool) -> Self {
        self.null = null;
        self
    }

    /// For `unique` under [`ValidAsync`](crate::ValidAsync): ignore the row
    /// whose `id` is the route's path parameter `name` (`PATCH
    /// /users/{id}`), unless an explicit `ignore_id` was given. Has no effect
    /// on [`passes`](AsyncRule::passes) or on `exists`. A
    /// [`check`](AsyncRule::check) rule receives the parameter's value.
    pub fn ignore_path_param(mut self, name: impl Into<String>) -> Self {
        self.ignore_path_param = Some(name.into());
        self
    }

    /// The field the error goes to.
    pub fn field(&self) -> &str {
        &self.field
    }

    /// The rule key: `unique`, `exists` or a check's own (also the message
    /// key).
    pub fn rule(&self) -> &'static str {
        match &self.check {
            Check::Table { kind, .. } => kind.rule(),
            Check::Uow { rule, .. } => rule,
        }
    }

    /// Whether the rule passes on `executor` (a pool, a transaction, a
    /// unit of work or a `Db`). A [`check`](AsyncRule::check) rule needs a
    /// unit of work ([`passes_in`](AsyncRule::passes_in)): here it fails with
    /// [`RuleError::NeedsUnitOfWork`].
    pub async fn passes<E: Executor + ?Sized>(&self, executor: &E) -> Result<bool, RuleError> {
        let ignore_id = match &self.check {
            Check::Table { ignore_id, .. } => *ignore_id,
            Check::Uow { .. } => None,
        };
        self.passes_ignoring(executor, ignore_id).await
    }

    /// Whether the rule passes in `uow` (any rule).
    pub async fn passes_in(&self, uow: &Uow) -> Result<bool, RuleError> {
        match &self.check {
            Check::Table { ignore_id, .. } => self.passes_ignoring(uow, *ignore_id).await,
            Check::Uow { check, .. } => self.run_check(check.as_ref(), uow, None).await,
        }
    }

    /// Runs the rule in `uow` with the request's path parameters.
    pub(crate) async fn passes_for(
        &self,
        uow: &Uow,
        path_params: &[(String, String)],
    ) -> Result<bool, RuleError> {
        match &self.check {
            Check::Table { .. } => {
                self.passes_ignoring(uow, self.ignore_for(path_params))
                    .await
            }
            Check::Uow { check, .. } => {
                let ignore = self.ignore_path_param.as_deref().and_then(|name| {
                    path_params
                        .iter()
                        .find(|(key, _)| key == name)
                        .map(|(_, value)| value.as_str())
                });
                self.run_check(check.as_ref(), uow, ignore).await
            }
        }
    }

    async fn run_check(
        &self,
        check: &dyn UowCheck,
        uow: &Uow,
        ignore: Option<&str>,
    ) -> Result<bool, RuleError> {
        if self.null {
            return Ok(true);
        }
        check
            .passes(uow, ignore)
            .await
            .map_err(|source| RuleError::Check {
                rule: self.rule(),
                field: self.field.clone(),
                source,
            })
    }

    /// The id to ignore given the request's path parameters.
    pub(crate) fn ignore_for(&self, path_params: &[(String, String)]) -> Option<Uuid> {
        let explicit = match &self.check {
            Check::Table { ignore_id, .. } => *ignore_id,
            Check::Uow { .. } => None,
        };
        explicit.or_else(|| {
            let name = self.ignore_path_param.as_deref()?;
            let (_, value) = path_params.iter().find(|(key, _)| key == name)?;
            Uuid::try_parse(value).ok()
        })
    }

    /// Fails when the table or column name is not a plain identifier.
    pub(crate) fn check_identifiers(&self) -> Result<(), RuleError> {
        if let Check::Table {
            table,
            column,
            deleted,
            ..
        } = &self.check
        {
            self.identifier(table)?;
            self.identifier(column)?;
            if let Some(deleted) = deleted {
                self.identifier(deleted)?;
            }
        }
        Ok(())
    }

    /// Whether the value is null (the rule then passes without a query).
    pub(crate) fn is_null(&self) -> bool {
        self.null
    }

    fn database(&self, source: DbError) -> RuleError {
        RuleError::Database {
            rule: self.rule(),
            field: self.field.clone(),
            source: Box::new(source),
        }
    }

    pub(crate) async fn passes_ignoring<E: Executor + ?Sized>(
        &self,
        executor: &E,
        ignore_id: Option<Uuid>,
    ) -> Result<bool, RuleError> {
        let Check::Table {
            kind,
            table,
            column,
            value,
            deleted,
            ..
        } = &self.check
        else {
            return Err(RuleError::NeedsUnitOfWork {
                rule: self.rule(),
                field: self.field.clone(),
            });
        };
        let table = self.identifier(table)?;
        let column = self.identifier(column)?;
        let deleted = deleted
            .as_deref()
            .map(|deleted| self.identifier(deleted))
            .transpose()?;
        if self.is_null() {
            return Ok(true);
        }
        let backend = executor.backend();
        let (table, column) = (
            backend.quote_identifier(table),
            backend.quote_identifier(column),
        );
        let mut sql = format!("SELECT 1 FROM {table} WHERE {column} = ?");
        let mut values = vec![value.clone()];
        if let (Kind::Unique, Some(id)) = (kind, ignore_id) {
            sql.push_str(&format!(
                " AND {} <> ?",
                backend.quote_identifier(ID_COLUMN)
            ));
            values.push(id.into());
        }
        if let Some(deleted) = deleted {
            sql.push_str(&format!(
                " AND {} IS NULL",
                backend.quote_identifier(deleted)
            ));
        }
        sql.push_str(" LIMIT 1");
        // The identifiers are checked names, quoted for the backend; the
        // values are bound.
        // nosemgrep: navree-sql-built-from-strings
        let found = executor
            .fetch_optional(&Statement::portable(sql, values))
            .await
            .map_err(|source| self.database(source))?
            .is_some();
        Ok(match kind {
            Kind::Unique => !found,
            Kind::Exists => found,
        })
    }

    fn identifier<'a>(&self, name: &'a str) -> Result<&'a str, RuleError> {
        if is_identifier(name) {
            Ok(name)
        } else {
            Err(RuleError::InvalidIdentifier {
                rule: self.rule(),
                field: self.field.clone(),
                identifier: name.to_owned(),
            })
        }
    }
}

/// `[a-z_][a-z0-9_]*`.
fn is_identifier(name: &str) -> bool {
    let mut bytes = name.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z' | b'_'))
        && bytes.all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_'))
}

/// A database rule that could not be checked (a server error, not a
/// validation failure).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RuleError {
    /// A table or column name is not a plain SQL identifier.
    #[error(
        "the `{rule}` rule on `{field}` names `{identifier}`, which is not a plain SQL identifier ([a-z_][a-z0-9_]*)"
    )]
    InvalidIdentifier {
        /// `unique` or `exists`.
        rule: &'static str,
        /// The validated field.
        field: String,
        /// The rejected table or column name.
        identifier: String,
    },
    /// The query failed.
    #[error("the `{rule}` rule on `{field}` could not be checked: {source}")]
    Database {
        /// `unique` or `exists`.
        rule: &'static str,
        /// The validated field.
        field: String,
        /// The database error (boxed: it is large).
        #[source]
        source: Box<DbError>,
    },
    /// A [`UowCheck`] failed to run.
    #[error("the `{rule}` rule on `{field}` could not be checked: {source}")]
    Check {
        /// The rule key.
        rule: &'static str,
        /// The validated field.
        field: String,
        /// What went wrong.
        #[source]
        source: BoxError,
    },
    /// A [`UowCheck`] rule was checked on a bare connection
    /// ([`AsyncRule::passes`]); it runs in a unit of work
    /// ([`AsyncRule::passes_in`]).
    #[error("the `{rule}` rule on `{field}` runs in a unit of work: use `passes_in`")]
    NeedsUnitOfWork {
        /// The rule key.
        rule: &'static str,
        /// The validated field.
        field: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_are_lower_snake_case() {
        for ok in ["users", "_tmp", "user_roles2"] {
            assert!(is_identifier(ok), "{ok}");
        }
        for bad in [
            "",
            "Users",
            "2users",
            "users;",
            "public.users",
            "us ers",
            "\"users\"",
        ] {
            assert!(!is_identifier(bad), "{bad}");
        }
    }

    #[test]
    fn path_param_ignore_needs_a_uuid_and_yields_to_an_explicit_id() {
        let id = Uuid::now_v7();
        let params = vec![("id".to_owned(), id.to_string())];
        let rule = AsyncRule::unique("email", "users", "email", "a@b.c", None);
        assert_eq!(rule.ignore_for(&params), None);
        let rule = rule.ignore_path_param("id");
        assert_eq!(rule.ignore_for(&params), Some(id));
        assert_eq!(rule.ignore_for(&[("id".into(), "nope".into())]), None);
        let explicit = Uuid::now_v7();
        let rule = AsyncRule::unique("email", "users", "email", "a@b.c", Some(explicit))
            .ignore_path_param("id");
        assert_eq!(rule.ignore_for(&params), Some(explicit));
    }
}
