//! Database rules: [`AsyncRules`], [`AsyncRule`] (`unique`, `exists`) and
//! [`RuleError`].

use sea_orm::sea_query::{Alias, Expr, ExprTrait, Query};
use sea_orm::{ConnectionTrait, DbErr, Value};
use uuid::Uuid;

/// The column `unique` compares with `ignore_id`.
const ID_COLUMN: &str = "id";

/// Database rules of a DTO, run by [`ValidAsync`](crate::ValidAsync) after
/// its garde rules pass, inside the request's unit of work.
///
/// ```
/// use navis_validation::{AsyncRule, AsyncRules};
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

/// A database rule on one field: `unique` (no row has the value) or
/// `exists` (some row has it), checked with a parameterised
/// `SELECT 1 FROM <table> WHERE <column> = ? [AND id <> ?] LIMIT 1`.
///
/// Table and column names must match `[a-z_][a-z0-9_]*`; anything else
/// fails with [`RuleError::InvalidIdentifier`] before any SQL runs. A null
/// value (`None`) passes, like Laravel's rules on a nullable field. Bind
/// values with their column's type (`Uuid` for UUID columns).
#[derive(Clone, Debug)]
pub struct AsyncRule {
    field: String,
    kind: Kind,
    table: String,
    column: String,
    value: Value,
    ignore_id: Option<Uuid>,
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
        Self {
            field: field.into(),
            kind: Kind::Unique,
            table: table.into(),
            column: column.into(),
            value: value.into(),
            ignore_id,
            ignore_path_param: None,
        }
    }

    /// `field` must exist in `table.column`.
    pub fn exists(
        field: impl Into<String>,
        table: impl Into<String>,
        column: impl Into<String>,
        value: impl Into<Value>,
    ) -> Self {
        Self {
            field: field.into(),
            kind: Kind::Exists,
            table: table.into(),
            column: column.into(),
            value: value.into(),
            ignore_id: None,
            ignore_path_param: None,
        }
    }

    /// For `unique` under [`ValidAsync`](crate::ValidAsync): ignore the row
    /// whose `id` is the route's path parameter `name` (`PATCH
    /// /users/{id}`), unless an explicit `ignore_id` was given. Has no effect
    /// on [`passes`](AsyncRule::passes) or on `exists`.
    pub fn ignore_path_param(mut self, name: impl Into<String>) -> Self {
        self.ignore_path_param = Some(name.into());
        self
    }

    /// The field the error goes to.
    pub fn field(&self) -> &str {
        &self.field
    }

    /// The rule key: `unique` or `exists` (also the message key).
    pub fn rule(&self) -> &'static str {
        self.kind.rule()
    }

    /// Whether the rule passes on `conn` (a connection or a transaction).
    pub async fn passes<C: ConnectionTrait>(&self, conn: &C) -> Result<bool, RuleError> {
        self.passes_ignoring(conn, self.ignore_id).await
    }

    /// The id to ignore given the request's path parameters.
    pub(crate) fn ignore_for(&self, path_params: &[(String, String)]) -> Option<Uuid> {
        self.ignore_id.or_else(|| {
            let name = self.ignore_path_param.as_deref()?;
            let (_, value) = path_params.iter().find(|(key, _)| key == name)?;
            Uuid::try_parse(value).ok()
        })
    }

    /// Fails when the table or column name is not a plain identifier.
    pub(crate) fn check_identifiers(&self) -> Result<(), RuleError> {
        self.identifier(&self.table)?;
        self.identifier(&self.column)?;
        Ok(())
    }

    /// Whether the value is null (the rule then passes without a query).
    pub(crate) fn is_null(&self) -> bool {
        !self.value.is_some()
    }

    pub(crate) async fn passes_ignoring<C: ConnectionTrait>(
        &self,
        conn: &C,
        ignore_id: Option<Uuid>,
    ) -> Result<bool, RuleError> {
        let table = self.identifier(&self.table)?;
        let column = self.identifier(&self.column)?;
        if self.is_null() {
            return Ok(true);
        }
        let mut select = Query::select();
        select
            .expr(Expr::val(1))
            .from(Alias::new(table))
            .and_where(Expr::col(Alias::new(column)).eq(self.value.clone()));
        if let (Kind::Unique, Some(id)) = (self.kind, ignore_id) {
            select.and_where(Expr::col(Alias::new(ID_COLUMN)).ne(id));
        }
        select.limit(1);
        let found = conn
            .query_one(&select)
            .await
            .map_err(|source| RuleError::Database {
                rule: self.rule(),
                field: self.field.clone(),
                source,
            })?
            .is_some();
        Ok(match self.kind {
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
        /// The database error.
        #[source]
        source: DbErr,
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
