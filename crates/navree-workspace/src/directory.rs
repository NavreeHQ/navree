//! [`UserDirectory`]: names and email addresses of users, for member lists
//! and invitation checks.

use std::fmt;
use std::sync::Arc;

use navree_db::{Backend, Db, Executor, Row, Statement, Value};
use navree_kernel::{BoxError, BoxFuture};
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
    let reader = db.read_executor().await;
    let sql = match reader.backend() {
        Backend::Sqlite => "SELECT 1 AS found FROM sqlite_master WHERE type = 'table' AND name = ?",
        Backend::MySql => {
            "SELECT 1 AS found FROM information_schema.tables \
             WHERE table_schema = DATABASE() AND table_name = ?"
        }
        Backend::Postgres => "SELECT 1 AS found WHERE to_regclass(?) IS NOT NULL",
    };
    let statement = Statement::portable(sql, [Value::from(table)]);
    Ok(reader.fetch_optional(&statement).await?.is_some())
}

/// `SELECT id, name, email FROM <table> WHERE <condition>` for `backend`.
fn select(backend: Backend, table: &str, condition: &str) -> String {
    format!(
        "SELECT {id}, {name}, {email} FROM {table} WHERE {condition}",
        id = backend.quote_identifier("id"),
        name = backend.quote_identifier("name"),
        email = backend.quote_identifier("email"),
        table = backend.quote_identifier(table),
    )
}

fn summary(row: &Row) -> Result<UserSummary, BoxError> {
    Ok(UserSummary {
        id: row.get("id")?,
        name: row.get("name")?,
        email: row.get("email")?,
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
            let reader = self.db.read_executor().await;
            let backend = reader.backend();
            let marks = vec!["?"; ids.len()].join(", ");
            let condition = format!("{} IN ({marks})", backend.quote_identifier("id"));
            let statement = Statement::portable(
                select(backend, table, &condition),
                ids.iter().map(|id| Value::from(*id)),
            );
            let rows = reader.fetch_all(&statement).await?;
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
            let reader = self.db.read_executor().await;
            let backend = reader.backend();
            let condition = format!("{} = ? LIMIT 1", backend.quote_identifier("email"));
            let statement = Statement::portable(
                select(backend, table, &condition),
                [Value::from(email.trim().to_lowercase())],
            );
            let row = reader.fetch_optional(&statement).await?;
            row.as_ref().map(summary).transpose()
        })
    }
}
