//! Moves kept in an SQL database, through navree-orm: the lock a move takes on the
//! lists it touches, and fresh ranks for a whole list in one statement.

use navree_orm::{Access, Column, Dialect, Executor, OrmError, Result, ValueFor, raw, table};

use super::moves::Move;
use super::ranked::RANK_GAP;

/// Locks one row for each of `lists`, found by its `key` column, until the
/// transaction ends: `SELECT ... FOR UPDATE` on PostgreSQL and MySQL, one list at a
/// time in sorted order, so that two moves never wait for each other. On SQLite it
/// does nothing: its write transactions (`BEGIN IMMEDIATE`, as every unit of work
/// begins) already run one at a time. A list with no row locks nothing.
///
/// Call it in the move's unit of work, before anything else reads there.
/// [`RankedLists::lock`](super::RankedLists::lock) is the place for it:
///
/// ```ignore
/// impl RankedLists for Board<'_> {
///     type Error = OrmError;
///
///     fn lock(&self, lists: &[&str]) -> impl Future<Output = Result<(), OrmError>> + Send {
///         let (uow, lists) = (self.uow, lists.to_vec());
///         async move { sortable::lock_lists(uow, &Column::KEY, lists).await }
///     }
///     // spot, list, beside, nth, last
/// }
/// ```
///
/// # Errors
///
/// The database's, when a read fails; [`OrmError::InvalidQuery`] for a column with
/// no table; [`OrmError::Unsupported`] on a document store.
pub async fn lock_lists<M, T, V, E>(
    exec: &E,
    key: &Column<M, T>,
    lists: impl IntoIterator<Item = V>,
) -> Result<()>
where
    V: ValueFor<T> + Ord,
    E: Executor + ?Sized,
{
    if dialect(exec, "locking a list").await? == Dialect::Sqlite {
        return Ok(());
    }
    let name = table_of(key)?;
    let mut lists: Vec<V> = lists.into_iter().collect();
    lists.sort();
    lists.dedup();
    for list in lists {
        table(name)
            .select([key.name()])
            .where_(key.eq(list))
            .lock_for_update()
            .first(exec)
            .await?;
    }
    Ok(())
}

impl Move {
    /// Locks the lists the move touches ([`lists`](Self::lists)), each a row found
    /// by its `key` column, until the transaction ends: the step before
    /// [`check`](Self::check), for lists read whole. See [`lock_lists`].
    ///
    /// ```ignore
    /// db.transaction(|uow| async move {
    ///     mv.lock(&uow, &Column::KEY).await?;
    ///     let lists = read_lists(&uow).await?;
    ///     match mv.check(&lists) { /* ... */ }
    /// })
    /// ```
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
}

/// Gives every item of `list` a fresh rank, [`RANK_GAP`] apart from 0, in the
/// order it has (by `rank`, then `id`), in one statement: for when two neighbours
/// have no room left between them. Returns how many rows it changed.
///
/// The ranks of a list pass through repeats while it runs, so the index on them
/// must not be unique. Run it in the move's unit of work, after
/// [`lock_lists`], then place the move again.
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
    // The list's order, numbered once by a window, joined back to its rows by id.
    let fresh = format!(
        "(SELECT {i} AS id, (ROW_NUMBER() OVER (ORDER BY {r}, {i}) - 1) * {RANK_GAP} AS n FROM {t} WHERE {l} = ?) AS fresh"
    );
    let sql = match dialect {
        Dialect::MySql => {
            format!("UPDATE {t} JOIN {fresh} ON {t}.{i} = fresh.id SET {t}.{r} = fresh.n")
        }
        Dialect::Postgres | Dialect::Sqlite => {
            format!("UPDATE {t} SET {r} = fresh.n FROM {fresh} WHERE {t}.{i} = fresh.id")
        }
    };
    let value = list_column
        .encode(&list.into_value_for())
        .map_err(OrmError::InvalidQuery)?;
    navree_orm::statement(exec, raw(sql, [value])).await
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
