//! [`UserDirectory`]: names and email addresses of users, for member lists
//! and invitation checks.

use std::fmt;
use std::sync::Arc;

use navree_db::Db;
use navree_kernel::{BoxError, BoxFuture};
use sea_orm::sea_query::{Alias, Expr, ExprTrait, Query};
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::Serialize;
use tokio::sync::OnceCell;
use uuid::Uuid;

/// A user as member lists show them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct UserSummary {
    /// The user id.
    pub id: Uuid,
    /// The display name.
    pub name: String,
    /// The email address.
    pub email: String,
}

/// Looks users up by id and by email address. The workspace module uses it
/// to show members' names and emails, to refuse invitations to existing
/// members and to check that an invitation is accepted by the account it
/// was sent to.
///
/// The module registers a [`TableUserDirectory`] reading
/// `workspace.users_table` (navree-auth's `users` by default); replace it
/// with `override_value::<dyn UserDirectory>(..)` when users live
/// elsewhere.
pub trait UserDirectory: Send + Sync + 'static {
    /// The users with these ids that exist (in any order).
    fn users<'a>(&'a self, ids: &'a [Uuid]) -> BoxFuture<'a, Result<Vec<UserSummary>, BoxError>>;

    /// The user with this email address (trimmed and lower-cased), if any.
    fn user_by_email<'a>(
        &'a self,
        email: &'a str,
    ) -> BoxFuture<'a, Result<Option<UserSummary>, BoxError>>;
}

/// A [`UserDirectory`] over a table with `id`, `name` and `email` columns
/// (navree-auth's `users`). Without the table (or with no table name) it
/// knows no one.
pub struct TableUserDirectory {
    db: Arc<Db>,
    table: Option<String>,
    exists: OnceCell<bool>,
}

impl TableUserDirectory {
    /// A directory reading `table` (`None`: knows no one).
    pub fn new(db: Arc<Db>, table: Option<String>) -> Self {
        Self {
            db,
            table,
            exists: OnceCell::new(),
        }
    }

    /// The table, when it is configured and exists (checked once).
    async fn table(&self) -> Result<Option<&str>, BoxError> {
        let Some(table) = self.table.as_deref() else {
            return Ok(None);
        };
        let exists = self
            .exists
            .get_or_try_init(|| table_exists(&self.db, table))
            .await?;
        Ok(exists.then_some(table))
    }
}

impl fmt::Debug for TableUserDirectory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TableUserDirectory")
            .field("table", &self.table)
            .finish_non_exhaustive()
    }
}

async fn table_exists(db: &Db, table: &str) -> Result<bool, BoxError> {
    let backend = db.backend();
    let statement = match backend {
        DbBackend::Sqlite => Statement::from_sql_and_values(
            backend,
            "SELECT 1 AS found FROM sqlite_master WHERE type = 'table' AND name = ?",
            [table.into()],
        ),
        _ => Statement::from_sql_and_values(
            backend,
            "SELECT 1 AS found WHERE to_regclass($1) IS NOT NULL",
            [table.into()],
        ),
    };
    Ok(db.reader().await.query_one_raw(statement).await?.is_some())
}

fn summary(row: &sea_orm::QueryResult) -> Result<UserSummary, BoxError> {
    Ok(UserSummary {
        id: row.try_get("", "id")?,
        name: row.try_get("", "name")?,
        email: row.try_get("", "email")?,
    })
}

impl UserDirectory for TableUserDirectory {
    fn users<'a>(&'a self, ids: &'a [Uuid]) -> BoxFuture<'a, Result<Vec<UserSummary>, BoxError>> {
        Box::pin(async move {
            let Some(table) = self.table().await? else {
                return Ok(Vec::new());
            };
            if ids.is_empty() {
                return Ok(Vec::new());
            }
            let select = Query::select()
                .columns([Alias::new("id"), Alias::new("name"), Alias::new("email")])
                .from(Alias::new(table))
                .and_where(Expr::col(Alias::new("id")).is_in(ids.iter().copied()))
                .to_owned();
            let rows = self.db.reader().await.query_all(&select).await?;
            rows.iter().map(summary).collect()
        })
    }

    fn user_by_email<'a>(
        &'a self,
        email: &'a str,
    ) -> BoxFuture<'a, Result<Option<UserSummary>, BoxError>> {
        Box::pin(async move {
            let Some(table) = self.table().await? else {
                return Ok(None);
            };
            let select = Query::select()
                .columns([Alias::new("id"), Alias::new("name"), Alias::new("email")])
                .from(Alias::new(table))
                .and_where(Expr::col(Alias::new("email")).eq(email.trim().to_lowercase()))
                .limit(1)
                .to_owned();
            let row = self.db.reader().await.query_one(&select).await?;
            row.as_ref().map(summary).transpose()
        })
    }
}
