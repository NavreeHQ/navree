//! Moves kept in an SQL database, through navree-orm: the transaction a move runs
//! in, which the toolkit begins, the lock it takes there on the lists it touches,
//! and fresh ranks for a whole list in one statement.

use std::future::Future;

use navree_db::{Backend, Db, IsolationLevel, Uow};
use navree_orm::{
    Access, Column, Dialect, Executor, Filter, OrmError, Result, Value, ValueFor, col, raw,
    scalar, table,
};

use super::moves::Move;
use super::ranked::RANK_GAP;

/// How many times [`transaction`] runs a move, at most, when the database asks for
/// it to run again.
const ATTEMPTS: usize = 3;

/// The rows a move may lock and renumber: the columns that must hold a value for a row
/// to count, such as the workspace or the board it belongs to. [`lock_lists`] and
/// [`renew_ranks`] find rows by the keys a move names, which a browser posts; a scope
/// is what keeps them to the caller's own rows, on a table that no model's scope
/// reaches. [`lock_lists_scoped`], [`renew_ranks_scoped`], [`transaction_scoped`] and
/// [`Move::transaction_scoped`] apply it to both.
///
/// ```ignore
/// let scope = ListScope::new().and(&Lane::WORKSPACE_ID, workspace.id);
/// let landed = mv
///     .transaction_scoped(&db, &Lane::KEY, &scope, |uow| async move { /* ... */ })
///     .await?;
/// ```
#[derive(Clone, Debug, Default)]
pub struct ListScope {
    terms: Vec<ScopeTerm>,
}

/// One column of a scope, and the value it must hold.
#[derive(Clone, Debug)]
struct ScopeTerm {
    table: Option<String>,
    column: String,
    /// The value as the column stores it, or why it can't be.
    value: std::result::Result<Value, String>,
}

impl ListScope {
    /// A scope with no condition: every row counts.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a condition: only the rows whose `column` holds `value`. Conditions add up.
    #[must_use]
    pub fn and<M, T, V>(mut self, column: &Column<M, T>, value: V) -> Self
    where
        V: ValueFor<T>,
    {
        let value: T = value.into_value_for();
        self.terms.push(ScopeTerm {
            table: column.table().map(str::to_owned),
            column: column.name().to_owned(),
            value: column.encode(&value),
        });
        self
    }

    /// Whether the scope has no condition.
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    /// Each condition, with its column's table checked against `table`, and its value
    /// as the column stores it.
    fn checked(&self, table: &str) -> Result<Vec<(&str, Value)>> {
        self.terms
            .iter()
            .map(|term| {
                if let Some(of) = term.table.as_deref()
                    && of != table
                {
                    return Err(OrmError::InvalidQuery(format!(
                        "the scope's column `{}` is of the table `{of}`, and the lists are of `{table}`",
                        term.column
                    )));
                }
                let value = term.value.clone().map_err(|error| {
                    OrmError::InvalidQuery(format!(
                        "cannot scope the lists by the column `{}`: {error}",
                        term.column
                    ))
                })?;
                Ok((term.column.as_str(), value))
            })
            .collect()
    }

    /// The conditions as filters for a read of `table`.
    fn filters(&self, table: &str) -> Result<Vec<Filter>> {
        Ok(self
            .checked(table)?
            .into_iter()
            .map(|(column, value)| col(column.to_owned()).eq(value))
            .collect())
    }
}

/// Runs `f` in a transaction that holds the lists a move touches: the safe way to
/// check or place a move against a database. It
///
/// - begins a unit of work at READ COMMITTED on PostgreSQL and MySQL, whatever the
///   server's or the pool's own level, and `BEGIN IMMEDIATE` on SQLite;
/// - locks `lists` (rows found by their `key` column, see [`lock_lists`]) as its
///   first statement;
/// - only then hands the unit of work to `f`, and commits it when `f` returns
///   `Ok`, or rolls it back;
/// - runs it all again, up to three times in all, when the database asks for that:
///   a deadlock, a serialization failure, or a lock not free in time.
///
/// So nothing can read before the lock, and every read after it sees the moves
/// that held it before: at REPEATABLE READ (MySQL's default) or SERIALIZABLE a
/// read would see what was there when the transaction began, and two moves could
/// both take a list's last place. The unit of work carries the actor, request id
/// and workspace of the task's current one, as a request's does. `f` may run more
/// than once, so it does nothing outside the unit of work it is given.
///
/// ```ignore
/// let landed = sortable::transaction(&db, &Column::KEY, mv.lists(), |uow| {
///     let mv = mv.clone();
///     async move {
///         let lists = read_lists(&uow).await?; // under the lock, at READ COMMITTED
///         Ok(mv.check(&lists))
///     }
/// })
/// .await?;
/// ```
///
/// # Errors
///
/// As [`lock_lists`]; `f`'s own error; the database's, when it begins or commits.
pub async fn transaction<M, T, V, R, F, Fut>(
    db: &Db,
    key: &Column<M, T>,
    lists: Vec<V>,
    f: F,
) -> Result<R>
where
    V: ValueFor<T> + Ord + Clone,
    F: Fn(Uow) -> Fut,
    Fut: Future<Output = Result<R>>,
{
    transaction_scoped(db, key, lists, &ListScope::new(), f).await
}

/// [`transaction`], with the lists found only among the rows of `scope`: the lock
/// ([`lock_lists_scoped`]) takes a list that is not in the scope as not there. Use it
/// where the keys of the lists are the caller's to name but only their own are theirs
/// to move: lists of a workspace or a board, on a table that holds those of others.
///
/// ```ignore
/// let scope = ListScope::new().and(&Card::WORKSPACE_ID, workspace.id);
/// let landed = sortable::transaction_scoped(&db, &Column::KEY, mv.lists(), &scope, |uow| {
///     let mv = mv.clone();
///     async move { Ok(mv.check(&read_lists(&uow).await?)) }
/// })
/// .await?;
/// ```
///
/// # Errors
///
/// As [`transaction`], and [`lock_lists_scoped`]'s.
pub async fn transaction_scoped<M, T, V, R, F, Fut>(
    db: &Db,
    key: &Column<M, T>,
    lists: Vec<V>,
    scope: &ListScope,
    f: F,
) -> Result<R>
where
    V: ValueFor<T> + Ord + Clone,
    F: Fn(Uow) -> Fut,
    Fut: Future<Output = Result<R>>,
{
    let mut attempt = 1;
    loop {
        let uow = unit_of_work(db).await;
        if db.backend() != Backend::Sqlite {
            uow.set_isolation(IsolationLevel::ReadCommitted).await?;
        }
        let ran = uow
            .scope(async {
                lock_lists_scoped(&uow, key, lists.clone(), scope).await?;
                f(uow.clone()).await
            })
            .await;
        let outcome = match ran {
            Ok(value) => uow.commit().await.map(|()| value).map_err(OrmError::from),
            Err(error) => {
                // A failed rollback leaves nothing to keep: the connection is dropped.
                let _ = uow.rollback().await;
                Err(error)
            }
        };
        match outcome {
            Err(OrmError::Db(error)) if error.is_retryable() && attempt < ATTEMPTS => {
                attempt += 1;
            }
            other => return other,
        }
    }
}

impl Move {
    /// Runs `f` in a transaction that holds the lists the move touches
    /// ([`lists`](Self::lists)), each a row found by its `key` column: see
    /// [`transaction`]. Check or place the move in `f`, and write it.
    ///
    /// ```ignore
    /// let placed = mv
    ///     .transaction(&db, &Column::KEY, |uow| {
    ///         let mv = mv.clone();
    ///         async move {
    ///             let placed = mv.place(&Board { uow: &uow }).await?;
    ///             if let Ok(placement) = &placed {
    ///                 write_rank(&uow, placement).await?;
    ///             }
    ///             Ok(placed)
    ///         }
    ///     })
    ///     .await?;
    /// ```
    ///
    /// # Errors
    ///
    /// As [`transaction`].
    pub async fn transaction<M, R, F, Fut>(
        &self,
        db: &Db,
        key: &Column<M, String>,
        f: F,
    ) -> Result<R>
    where
        F: Fn(Uow) -> Fut,
        Fut: Future<Output = Result<R>>,
    {
        transaction(db, key, self.lists(), f).await
    }

    /// [`transaction`](Self::transaction), with the lists found only among the rows of
    /// `scope`: a list the move names that is not in the scope is a list that isn't
    /// there ([`is_not_found`](OrmError::is_not_found)). See [`ListScope`].
    ///
    /// # Errors
    ///
    /// As [`transaction_scoped`].
    pub async fn transaction_scoped<M, R, F, Fut>(
        &self,
        db: &Db,
        key: &Column<M, String>,
        scope: &ListScope,
        f: F,
    ) -> Result<R>
    where
        F: Fn(Uow) -> Fut,
        Fut: Future<Output = Result<R>>,
    {
        transaction_scoped(db, key, self.lists(), scope, f).await
    }

    /// Locks the lists the move touches ([`lists`](Self::lists)), each a row found
    /// by its `key` column, until the transaction ends: [`lock_lists`] for this
    /// move. [`transaction`](Self::transaction) takes this lock itself.
    ///
    /// # Errors
    ///
    /// As [`lock_lists`].
    pub async fn lock<M, E>(&self, exec: &E, key: &Column<M, String>) -> Result<()>
    where
        E: Executor + ?Sized,
    {
        lock_lists(exec, key, self.lists()).await
    }

    /// [`lock`](Self::lock), with the lists found only among the rows of `scope`:
    /// [`lock_lists_scoped`] for this move.
    ///
    /// # Errors
    ///
    /// As [`lock_lists_scoped`].
    pub async fn lock_scoped<M, E>(
        &self,
        exec: &E,
        key: &Column<M, String>,
        scope: &ListScope,
    ) -> Result<()>
    where
        E: Executor + ?Sized,
    {
        lock_lists_scoped(exec, key, self.lists(), scope).await
    }
}

/// A new unit of work on `db`, for the actor, request id and workspace of the
/// task's current one.
async fn unit_of_work(db: &Db) -> Uow {
    let Some(current) = Uow::current() else {
        return db.uow(None, None).await;
    };
    let request = current.request_id().map(str::to_owned);
    let uow = match (current.actor(), current.impersonator()) {
        (Some(actor), Some(by)) => db.impersonated_uow(actor, by, request).await,
        (actor, _) => db.uow(actor, request).await,
    };
    uow.set_workspace(current.workspace());
    uow
}

/// Locks one row for each of `lists`, found by its `key` column, until the unit of
/// work ends: `SELECT ... FOR UPDATE` on PostgreSQL and MySQL, one list at a time
/// in sorted order, so that two moves never wait for each other. On SQLite, whose
/// units of work begin `BEGIN IMMEDIATE` and so already run one at a time, the
/// rows are only read. [`transaction`] calls it as its first statement: call it
/// yourself only in [`RankedLists::lock`](super::RankedLists::lock), which
/// [`Move::place`] calls first, as a second lock in the same transaction is
/// granted at once.
///
/// The key column is the list's unique, indexed key (a primary key): on MySQL a
/// `FOR UPDATE` on a column with no index locks every row it reads. And it is the
/// key of a list the caller may move in: a key is locked across the whole table, so
/// either make the keys unique to the caller's board or workspace, or lock with
/// [`lock_lists_scoped`], which finds each list only among the caller's rows.
///
/// It refuses, rather than lock nothing, when the lock would not hold:
///
/// - outside a unit of work, where a lock ends with its statement;
/// - on PostgreSQL and MySQL, in a transaction that isn't READ COMMITTED, whose
///   reads would keep a snapshot from before the lock (PostgreSQL's level is read
///   from the transaction; MySQL's from the unit of work's chosen level, or else
///   the session's);
/// - when a list has no row.
///
/// # Errors
///
/// [`OrmError::InvalidQuery`] outside a unit of work, outside READ COMMITTED, or
/// for a column with no table; [`OrmError::ModelNotFound`] (`is_not_found()`)
/// naming the first list with no row; [`OrmError::Unsupported`] on a document
/// store; the database's, when a read fails.
pub async fn lock_lists<M, T, V, E>(
    exec: &E,
    key: &Column<M, T>,
    lists: impl IntoIterator<Item = V>,
) -> Result<()>
where
    V: ValueFor<T> + Ord,
    E: Executor + ?Sized,
{
    lock_lists_scoped(exec, key, lists, &ListScope::new()).await
}

/// [`lock_lists`], finding each list only among the rows of `scope`: `SELECT ... FOR
/// UPDATE` on the row with that key *and* the scope's values. A key that names a row
/// outside the scope is a list with no row, which fails as [`lock_lists`] does for any
/// list that isn't there, so a browser can't have a list of another workspace locked by
/// naming it. Call it in [`RankedLists::lock`](super::RankedLists::lock) with the same
/// scope as the transaction's.
///
/// # Errors
///
/// As [`lock_lists`]; [`OrmError::InvalidQuery`] for a scope column of another table
/// than the key's, or a value that column can't hold.
pub async fn lock_lists_scoped<M, T, V, E>(
    exec: &E,
    key: &Column<M, T>,
    lists: impl IntoIterator<Item = V>,
    scope: &ListScope,
) -> Result<()>
where
    V: ValueFor<T> + Ord,
    E: Executor + ?Sized,
{
    let (dialect, uow) = {
        let target = exec.target(Access::Read).await?;
        let dialect = target.dialect().ok_or_else(|| OrmError::Unsupported {
            feature: "locking a list".into(),
            driver: "a document store".into(),
        })?;
        (dialect, target.uow().cloned())
    };
    let Some(uow) = uow else {
        return Err(OrmError::InvalidQuery(
            "a move's lists are locked in its unit of work: outside one, the lock would end with its statement. Run the move in `sortable::transaction` or `Move::transaction`".to_owned(),
        ));
    };
    if dialect != Dialect::Sqlite {
        read_committed(&uow, dialect).await?;
    }
    let name = table_of(key)?;
    let scoped = scope.filters(name)?;
    let mut lists: Vec<V> = lists.into_iter().collect();
    lists.sort();
    lists.dedup();
    for list in lists {
        let list: T = list.into_value_for();
        let named = key
            .encode(&list)
            .map_or_else(|error| error, |value| value.to_string());
        let mut query = table(name).select([key.name()]).where_(key.eq(list));
        for filter in &scoped {
            query = query.where_(filter.clone());
        }
        if dialect != Dialect::Sqlite {
            query = query.lock_for_update();
        }
        if query.first(exec).await?.is_none() {
            return Err(OrmError::not_found(name.to_owned(), vec![named]));
        }
    }
    Ok(())
}

/// Refuses a transaction that isn't READ COMMITTED.
async fn read_committed(uow: &Uow, dialect: Dialect) -> Result<()> {
    let level = match (uow.isolation(), dialect) {
        (Some(level), _) => level.as_sql().to_owned(),
        (None, Dialect::Postgres) => scalar::<String, _>(
            uow,
            raw(
                "SELECT current_setting('transaction_isolation')",
                Vec::<i64>::new(),
            ),
        )
        .await?
        .unwrap_or_default(),
        (None, _) => scalar::<String, _>(
            uow,
            raw("SELECT @@transaction_isolation", Vec::<i64>::new()),
        )
        .await?
        .unwrap_or_default(),
    };
    let level = level.to_uppercase().replace(['-', '_'], " ");
    if level == "READ COMMITTED" {
        return Ok(());
    }
    Err(OrmError::InvalidQuery(format!(
        "a move's lists are locked in a READ COMMITTED transaction, so that every read after the lock sees the moves that held it before; this one is {level}. Run the move in `sortable::transaction` or `Move::transaction`, which begin one"
    )))
}

/// Gives every item of `list` a fresh rank, [`RANK_GAP`] apart from 0, in the
/// order it has (by `rank`, then `id`), in one statement: for when two neighbours
/// have no room left between them. Returns how many rows it changed.
///
/// The ranks of a list pass through repeats while it runs, so the index on them
/// must not be unique. Run it in the move's [`transaction`], under its lock, then
/// place the move again.
///
/// ```ignore
/// sortable::renew_ranks(&uow, &Card::LIST, &Card::RANK, &Card::ID, "backlog").await?;
/// ```
///
/// # Errors
///
/// The database's; [`OrmError::InvalidQuery`] when the columns name no table or
/// different tables; [`OrmError::Unsupported`] on a document store.
pub async fn renew_ranks<M, L, I, V, E>(
    exec: &E,
    list_column: &Column<M, L>,
    rank_column: &Column<M, i64>,
    id_column: &Column<M, I>,
    list: V,
) -> Result<u64>
where
    V: ValueFor<L>,
    E: Executor + ?Sized,
{
    renew_ranks_scoped(
        exec,
        list_column,
        rank_column,
        id_column,
        list,
        &ListScope::new(),
    )
    .await
}

/// [`renew_ranks`], for the items of `list` among the rows of `scope` only: where lists
/// of several workspaces or boards share a name, the rows of the others keep their ranks.
/// Without a scope, a list named `backlog` is every row of the table whose list column
/// holds `backlog`, whoever's it is. Use the same scope as the move's transaction.
///
/// ```ignore
/// let scope = ListScope::new().and(&Card::WORKSPACE_ID, workspace.id);
/// sortable::renew_ranks_scoped(&uow, &Card::LIST, &Card::RANK, &Card::ID, "backlog", &scope)
///     .await?;
/// ```
///
/// # Errors
///
/// As [`renew_ranks`]; [`OrmError::InvalidQuery`] for a scope column of another table
/// than the list's, or a value that column can't hold.
pub async fn renew_ranks_scoped<M, L, I, V, E>(
    exec: &E,
    list_column: &Column<M, L>,
    rank_column: &Column<M, i64>,
    id_column: &Column<M, I>,
    list: V,
    scope: &ListScope,
) -> Result<u64>
where
    V: ValueFor<L>,
    E: Executor + ?Sized,
{
    let dialect = dialect(exec, "renewing ranks").await?;
    let name = table_of(list_column)?;
    if table_of(rank_column)? != name || table_of(id_column)? != name {
        return Err(OrmError::InvalidQuery(
            "the list, rank and id columns are of different tables".to_owned(),
        ));
    }
    let q = |name: &str| dialect.quote_identifier(name);
    let (t, l, r, i) = (
        q(name),
        q(list_column.name()),
        q(rank_column.name()),
        q(id_column.name()),
    );
    let value = list_column
        .encode(&list.into_value_for())
        .map_err(OrmError::InvalidQuery)?;
    // The rows that count: the list's, and the scope's.
    let mut conditions = format!("{l} = ?");
    let mut values = vec![value];
    for (column, wanted) in scope.checked(name)? {
        let column = q(column);
        if matches!(wanted, Value::Null(_)) {
            conditions = format!("{conditions} AND {column} IS NULL");
        } else {
            conditions = format!("{conditions} AND {column} = ?");
            values.push(wanted);
        }
    }
    // The list's order, numbered once by a window, joined back to its rows by id.
    let fresh = format!(
        "(SELECT {i} AS id, (ROW_NUMBER() OVER (ORDER BY {r}, {i}) - 1) * {RANK_GAP} AS n FROM {t} WHERE {conditions}) AS fresh"
    );
    let sql = match dialect {
        Dialect::MySql => {
            format!("UPDATE {t} JOIN {fresh} ON {t}.{i} = fresh.id SET {t}.{r} = fresh.n")
        }
        Dialect::Postgres | Dialect::Sqlite => {
            format!("UPDATE {t} SET {r} = fresh.n FROM {fresh} WHERE {t}.{i} = fresh.id")
        }
    };
    navree_orm::statement(exec, raw(sql, values)).await
}

/// The SQL dialect `exec` speaks.
async fn dialect<E: Executor + ?Sized>(exec: &E, feature: &'static str) -> Result<Dialect> {
    let target = exec.target(Access::Read).await?;
    target.dialect().ok_or_else(|| OrmError::Unsupported {
        feature: feature.into(),
        driver: "a document store".into(),
    })
}

/// The table `column` belongs to.
fn table_of<M, T>(column: &Column<M, T>) -> Result<&str> {
    column.table().ok_or_else(|| {
        OrmError::InvalidQuery(format!("the column `{}` names no table", column.name()))
    })
}
