//! Reading large results without holding them in memory: chunks, lazy
//! streams and cursors.
//!
//! | Method | Reads | Holds a connection | Writes in between | Eager loads |
//! |---|---|---|---|---|
//! | [`chunk`](crate::Builder::chunk), [`each`](crate::Builder::each), [`chunk_map`](crate::Builder::chunk_map) | a page per call (`LIMIT`/`OFFSET`), in the query's order (the key's when it has none) | only while a page is read | yes, but rows that stop matching shift later pages: use the `_by_id` forms | per page |
//! | [`chunk_by_id`](crate::Builder::chunk_by_id), [`each_by_id`](crate::Builder::each_by_id), [`chunk_by_column`](crate::Builder::chunk_by_column) | a page after the last key (`WHERE key > last ORDER BY key`) | only while a page is read | yes, safely | per page |
//! | [`lazy`](crate::Builder::lazy), [`lazy_by_id`](crate::Builder::lazy_by_id), [`lazy_by_column`](crate::Builder::lazy_by_column) | the same pages, as a [`Stream`] of records | only while a page is read | yes | per page |
//! | [`cursor`](crate::Builder::cursor), [`cursor`](crate::cursor) (raw SQL) | one query, row by row as the database sends them | for as long as the stream lives | not through the same transaction (refused with an error) | none |
//!
//! Memory stays constant: a chunk or a lazy stream keeps one page, a cursor
//! one row (the driver reads the rest from the socket as the stream is
//! polled). Retrieved hooks run for every record.
//!
//! Streams are [`RowStream`]s: `Stream<Item = Result<T, OrmError>>`, `Send`,
//! `Unpin` and fused, so every adapter of [`StreamExt`] and
//! [`TryStreamExt`] (re-exported here and in the prelude) applies: `map`,
//! `filter`, `take`, `chunks`, `try_collect`, ...
//!
//! ```no_run
//! # use navree_orm::__doc_models::*;
//! use std::ops::ControlFlow;
//!
//! use navree_orm::streams::TryStreamExt;
//!
//! # async fn demo(db: Db) -> Result<()> {
//! // Close stale deals 500 at a time; key pages stay correct while rows change.
//! let cutoff = Utc::now() - Duration::days(90);
//! let db2 = db.clone();
//! Deal::query()
//!     .where_(Deal::STAGE.eq(Stage::Open))
//!     .where_(Deal::UPDATED_AT.lt(cutoff))
//!     .chunk_by_id(&db, 500, move |deals| {
//!         let db = db2.clone();
//!         async move {
//!             Deal::query()
//!                 .where_key_in(deals.model_keys())
//!                 .update(&db, [Deal::STAGE.set(Stage::Lost)])
//!                 .await?;
//!             Ok(ControlFlow::Continue(()))
//!         }
//!     })
//!     .await?;
//!
//! // Stream every deal, one page of 1000 at a time.
//! let mut deals = Deal::query().lazy_by_id(&db, 1000);
//! while let Some(deal) = deals.try_next().await? {
//!     println!("{}", deal.title);
//! }
//! # Ok(()) }
//! ```

use std::fmt;
use std::future::Future;
use std::ops::ControlFlow;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll};

use futures::stream::FusedStream;

pub use futures::stream::{Stream, StreamExt, TryStreamExt};

use crate::collection::Collection;
use crate::column::IntoExpr;
use crate::error::{OrmError, Result};
use crate::exec::{Access, Executor, Target};
use crate::hooks::{Hooks, ModelEvent};
use crate::ir::{ColumnRef, Direction, Expr, Operator, Order, QueryIr, Where, WhereClause};
use crate::model::Model;
use crate::query::{Builder, HasQuery, TableBuilder, key_expr};
use crate::raw::Raw;
use crate::record::Record;
use crate::row::Row;
use crate::scope::group_scope_wheres;
use crate::value::Value;

// ---------------------------------------------------------------------------
// RowStream: a generator
// ---------------------------------------------------------------------------

/// Rows read from the database, decoded as `T` (a [`Record`] or a [`Row`]):
/// a `Stream<Item = Result<T, OrmError>>`.
///
/// Returned by [`lazy`](crate::Builder::lazy),
/// [`lazy_by_id`](crate::Builder::lazy_by_id),
/// [`cursor`](crate::Builder::cursor) and friends. Nothing is read until it
/// is polled, and it reads one item ahead of nothing: a record is decoded
/// (and its retrieved hooks run) when it is pulled. A failure is its last
/// item. Dropping the stream stops the read and frees the connection.
pub struct RowStream<'a, T> {
    producer: Option<Pin<Box<dyn Future<Output = ()> + Send + 'a>>>,
    slot: Arc<Mutex<Option<T>>>,
}

/// The producer's end of a [`RowStream`]: hands one item to the consumer
/// and waits until it was taken.
pub(crate) struct Yielder<T> {
    slot: Arc<Mutex<Option<T>>>,
}

impl<T> Yielder<T> {
    /// Hands `item` over; resolves once the stream's consumer took it (the
    /// producer future is dropped instead when the stream is).
    pub(crate) fn send(&mut self, item: T) -> impl Future<Output = ()> + '_ {
        *lock(&self.slot) = Some(item);
        let mut handed_over = false;
        std::future::poll_fn(move |_| {
            // The first poll gives control back to the stream, which takes
            // the item in the same `poll_next`; the next poll is the
            // consumer asking for the following item.
            if handed_over {
                Poll::Ready(())
            } else {
                handed_over = true;
                Poll::Pending
            }
        })
    }
}

impl<'a, T: Send + 'a> RowStream<'a, T> {
    /// A stream of what `produce` sends to its [`Yielder`].
    pub(crate) fn generate<F, Fut>(produce: F) -> Self
    where
        F: FnOnce(Yielder<T>) -> Fut,
        Fut: Future<Output = ()> + Send + 'a,
    {
        let slot = Arc::new(Mutex::new(None));
        let yielder = Yielder { slot: slot.clone() };
        RowStream {
            producer: Some(Box::pin(produce(yielder))),
            slot,
        }
    }

    /// A stream whose `produce` fails or finishes: its error becomes the
    /// last item.
    pub(crate) fn try_generate<F, Fut>(produce: F) -> RowStream<'a, Result<T>>
    where
        F: FnOnce(Yielder<Result<T>>) -> Fut + Send + 'a,
        Fut: Future<Output = (Yielder<Result<T>>, Result<()>)> + Send + 'a,
    {
        RowStream::generate(move |yielder| async move {
            let (mut yielder, outcome) = produce(yielder).await;
            if let Err(error) = outcome {
                yielder.send(Err(error)).await;
            }
        })
    }
}

impl<T> Stream for RowStream<'_, T> {
    type Item = T;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
        let this = &mut *self;
        let Some(producer) = this.producer.as_mut() else {
            return Poll::Ready(None);
        };
        match producer.as_mut().poll(cx) {
            Poll::Ready(()) => {
                this.producer = None;
                Poll::Ready(lock(&this.slot).take())
            }
            Poll::Pending => match lock(&this.slot).take() {
                Some(item) => Poll::Ready(Some(item)),
                // Waiting for the database; its I/O wakes this task.
                None => Poll::Pending,
            },
        }
    }
}

impl<T> FusedStream for RowStream<'_, T> {
    fn is_terminated(&self) -> bool {
        self.producer.is_none()
    }
}

impl<T> fmt::Debug for RowStream<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RowStream")
            .field("finished", &self.producer.is_none())
            .finish_non_exhaustive()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

// ---------------------------------------------------------------------------
// Transactions busy with a cursor
// ---------------------------------------------------------------------------

/// How many cursors stream through a transaction right now (a fast path
/// for [`ensure_free`]).
static ACTIVE: AtomicUsize = AtomicUsize::new(0);

/// The transactions (by address) a cursor streams through.
static STREAMING: Mutex<Vec<usize>> = Mutex::new(Vec::new());

/// Marks a transaction busy while a cursor streams through it: its
/// connection is held, so another statement on it would wait for the
/// cursor forever.
pub(crate) struct StreamingGuard(usize);

impl StreamingGuard {
    pub(crate) fn new(transaction: usize) -> Self {
        lock(&STREAMING).push(transaction);
        ACTIVE.fetch_add(1, Ordering::SeqCst);
        StreamingGuard(transaction)
    }
}

impl Drop for StreamingGuard {
    fn drop(&mut self) {
        let mut streaming = lock(&STREAMING);
        if let Some(at) = streaming.iter().position(|key| *key == self.0) {
            streaming.swap_remove(at);
        }
        ACTIVE.fetch_sub(1, Ordering::SeqCst);
    }
}

/// An error when a cursor streams through `transaction`.
pub(crate) fn ensure_free(transaction: Option<usize>) -> Result<()> {
    let Some(transaction) = transaction else {
        return Ok(());
    };
    if ACTIVE.load(Ordering::SeqCst) == 0 || !lock(&STREAMING).contains(&transaction) {
        return Ok(());
    }
    Err(OrmError::invalid(
        "this transaction's connection is busy streaming a cursor; finish or drop the cursor first (lazy() and chunk_by_id() read in pages and allow writes in between)",
    ))
}

// ---------------------------------------------------------------------------
// Paging
// ---------------------------------------------------------------------------

/// A query read page by page: model queries (records) and table queries
/// (rows).
pub(crate) trait Paged: Clone + Send + Sync + Sized + 'static {
    /// What a page holds.
    type Item: Send + 'static;

    /// The query, for the page's window and conditions.
    fn query_mut(&mut self) -> &mut QueryIr;

    /// Runs the query on `target`.
    fn fetch_page<'t>(
        self,
        target: &'t Target<'_>,
    ) -> impl Future<Output = Result<Vec<Self::Item>>> + Send + 't;

    /// The value of result column `column` of `item`; `None` when it has no
    /// such column.
    fn column_value(item: &Self::Item, column: &str) -> Result<Option<Value>>;
}

impl<M: Model> Paged for Builder<M> {
    type Item = Record<M>;

    fn query_mut(&mut self) -> &mut QueryIr {
        &mut self.ir
    }

    fn fetch_page<'t>(
        self,
        target: &'t Target<'_>,
    ) -> impl Future<Output = Result<Vec<Record<M>>>> + Send + 't {
        async move { Ok(self.fetch(target).await?.into_vec()) }
    }

    fn column_value(record: &Record<M>, column: &str) -> Result<Option<Value>> {
        if M::meta().has_column(column) {
            return record.model().value(column);
        }
        Ok(record
            .extras()
            .iter()
            .find(|(name, _)| name == column)
            .map(|(_, value)| value.clone()))
    }
}

impl Paged for TableBuilder {
    type Item = Row;

    fn query_mut(&mut self) -> &mut QueryIr {
        HasQuery::query_mut(self)
    }

    fn fetch_page<'t>(
        self,
        target: &'t Target<'_>,
    ) -> impl Future<Output = Result<Vec<Row>>> + Send + 't {
        async move { target.fetch(&self.into_ir()).await }
    }

    fn column_value(row: &Row, column: &str) -> Result<Option<Value>> {
        if row.has_column(column) {
            row.value(column).map(Some)
        } else {
            Ok(None)
        }
    }
}

/// How pages follow each other after the last row of the previous one.
struct Keyset {
    /// The column compared and ordered by.
    column: ColumnRef,
    /// The result column the last value is read from.
    alias: String,
    descending: bool,
    /// The value of the last row so far.
    last: Option<Value>,
    /// Whether the column is the model's key, which is never NULL.
    is_key: bool,
}

/// One walk through a query's pages.
struct Walk<Q: Paged> {
    query: Q,
    size: u64,
    skip: u64,
    remaining: Option<u64>,
    page: u64,
    keyset: Option<Keyset>,
    done: bool,
    /// The operation, for messages (`chunk_by_id`, `lazy_by_id`, ...).
    operation: &'static str,
}

/// The chunk size, which must be positive.
fn chunk_size(size: u64) -> Result<u64> {
    if size == 0 {
        Err(OrmError::invalid("the chunk size must be at least 1"))
    } else {
        Ok(size)
    }
}

impl<Q: Paged> Walk<Q> {
    /// Pages by `LIMIT`/`OFFSET` in the query's order.
    fn by_offset(mut query: Q, size: u64, operation: &'static str) -> Result<Self> {
        let size = chunk_size(size)?;
        let ir = query.query_mut();
        let skip = ir.offset.unwrap_or(0);
        let remaining = ir.limit;
        Ok(Walk {
            query,
            size,
            skip,
            remaining,
            page: 0,
            keyset: None,
            done: false,
            operation,
        })
    }

    /// Pages after the last value of `column` (read from result column
    /// `alias`); the query's own orders are replaced by the column's.
    fn by_column(
        mut query: Q,
        size: u64,
        column: ColumnRef,
        alias: String,
        descending: bool,
        is_key: bool,
        operation: &'static str,
    ) -> Result<Self> {
        let size = chunk_size(size)?;
        let ir = query.query_mut();
        let skip = ir.offset.unwrap_or(0);
        let remaining = ir.limit;
        Ok(Walk {
            query,
            size,
            skip,
            remaining,
            page: 0,
            keyset: Some(Keyset {
                column,
                alias,
                descending,
                last: None,
                is_key,
            }),
            done: false,
            operation,
        })
    }

    /// The query of the next page.
    fn next_query(&self, limit: u64) -> Result<Q> {
        let mut query = self.query.clone();
        let ir = query.query_mut();
        match &self.keyset {
            None => {
                let offset = self
                    .page
                    .checked_mul(self.size)
                    .and_then(|start| start.checked_add(self.skip))
                    .ok_or_else(|| OrmError::invalid("the chunk offset overflows"))?;
                ir.offset = (offset > 0).then_some(offset);
                ir.limit = Some(limit);
            }
            Some(keyset) => {
                ir.orders = vec![Order::By {
                    expr: Expr::Column(keyset.column.clone()),
                    direction: if keyset.descending {
                        Direction::Desc
                    } else {
                        Direction::Asc
                    },
                    nulls: None,
                }];
                ir.offset = (self.page == 0 && self.skip > 0).then_some(self.skip);
                ir.limit = Some(limit);
                let condition = match &keyset.last {
                    None if keyset.is_key => None,
                    None => Some(Where::Null {
                        expr: Expr::Column(keyset.column.clone()),
                        not: true,
                    }),
                    Some(last) => Some(Where::Compare {
                        left: Expr::Column(keyset.column.clone()),
                        op: if keyset.descending {
                            Operator::Lt
                        } else {
                            Operator::Gt
                        },
                        right: Expr::Value(last.clone()),
                    }),
                };
                if let Some(condition) = condition {
                    // `a OR b` and the page's condition: `(a OR b) AND key > ?`.
                    let before = ir.wheres.len();
                    ir.wheres.push(WhereClause::and(condition));
                    group_scope_wheres(ir, before);
                }
            }
        }
        Ok(query)
    }

    /// The next page; `None` once the query has no more rows.
    async fn next_page<E: Executor + ?Sized>(&mut self, exec: &E) -> Result<Option<Vec<Q::Item>>> {
        if self.done {
            return Ok(None);
        }
        let limit = self.remaining.map_or(self.size, |left| left.min(self.size));
        if limit == 0 {
            self.done = true;
            return Ok(None);
        }
        let mut query = self.next_query(limit)?;
        let access = crate::query::read_access(query.query_mut());
        let items = {
            let target = exec.target(access).await?;
            query.fetch_page(&target).await?
        };
        let count = items.len() as u64;
        if let Some(left) = &mut self.remaining {
            *left = left.saturating_sub(count);
        }
        if count < self.size {
            self.done = true;
        }
        let Some(last) = items.last() else {
            self.done = true;
            return Ok(None);
        };
        if let Some(keyset) = &mut self.keyset {
            let value = Q::column_value(last, &keyset.alias)?.ok_or_else(|| {
                OrmError::invalid(format!(
                    "{} was stopped: the result has no `{}` column to continue from (select it, or name the column it is selected as)",
                    self.operation, keyset.alias
                ))
            })?;
            if crate::value::is_null(&value) {
                return Err(OrmError::invalid(format!(
                    "{} was stopped: the `{}` column of the last row is NULL, so the next page cannot start after it",
                    self.operation, keyset.alias
                )));
            }
            keyset.last = Some(value);
        }
        self.page += 1;
        Ok(Some(items))
    }

    /// Calls `f` with every page until it breaks; `false` when it did.
    async fn chunks<E, F, Fut>(mut self, exec: &E, mut f: F) -> Result<bool>
    where
        E: Executor + ?Sized,
        F: FnMut(Vec<Q::Item>) -> Fut,
        Fut: Future<Output = Result<ControlFlow<()>>>,
    {
        while let Some(page) = self.next_page(exec).await? {
            if f(page).await?.is_break() {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Calls `f` with every item until it breaks; `false` when it did.
    async fn items<E, F, Fut>(mut self, exec: &E, mut f: F) -> Result<bool>
    where
        E: Executor + ?Sized,
        F: FnMut(Q::Item) -> Fut,
        Fut: Future<Output = Result<ControlFlow<()>>>,
    {
        while let Some(page) = self.next_page(exec).await? {
            for item in page {
                if f(item).await?.is_break() {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }

    /// Every item, page by page, as a stream.
    fn stream<'e, E: Executor + ?Sized>(walk: Result<Self>, exec: &'e E) -> RowStream<'e, Result<Q::Item>> {
        RowStream::try_generate(move |mut out| async move {
            let outcome = async {
                let mut walk = walk?;
                while let Some(page) = walk.next_page(exec).await? {
                    for item in page {
                        out.send(Ok(item)).await;
                    }
                }
                Ok(())
            }
            .await;
            (out, outcome)
        })
    }
}

/// The column a keyset walk follows.
fn keyset_column(column: impl IntoExpr) -> Result<ColumnRef> {
    match column.into_expr() {
        Expr::Column(column) if column.path.is_empty() => Ok(column),
        other => Err(OrmError::invalid(format!(
            "chunking by a column needs a plain column, not {other:?}"
        ))),
    }
}

// ---------------------------------------------------------------------------
// Model queries
// ---------------------------------------------------------------------------

impl<M: Model> Builder<M> {
    /// The query ready for offset pages: global scopes applied, ordered by
    /// the key when it has no order (pages need a stable order).
    fn for_offset_pages(self) -> Self {
        let mut query = self.apply_global_scopes();
        if query.ir.orders.is_empty() {
            query.ir.orders.push(Order::asc(key_expr::<M>()));
        }
        query
    }

    /// A keyset walk on `column` (the key when `None`).
    fn key_walk(
        self,
        size: u64,
        column: Option<(ColumnRef, String)>,
        descending: bool,
        operation: &'static str,
    ) -> Result<Walk<Self>> {
        let meta = M::meta();
        let (column, alias) = column.unwrap_or_else(|| {
            (
                ColumnRef::new(Some(meta.table()), meta.key_column()),
                meta.key_column().to_owned(),
            )
        });
        let is_key = column.name == meta.key_column()
            && column.table.as_deref().is_none_or(|table| table == meta.table());
        Walk::by_column(
            self.apply_global_scopes(),
            size,
            column,
            alias,
            descending,
            is_key,
            operation,
        )
    }

    /// Calls `f` with the records `size` at a time (one query per page, in
    /// the query's order, the key's when it has none); stops when `f`
    /// returns [`ControlFlow::Break`]. Returns `false` when `f` stopped it.
    ///
    /// The query's offset and limit are honoured. Pages are cut with
    /// `LIMIT`/`OFFSET`, so when `f` changes rows so they no longer match,
    /// later pages shift and skip rows: use
    /// [`chunk_by_id`](Self::chunk_by_id) then.
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// use std::ops::ControlFlow;
    ///
    /// # async fn demo(db: Db) -> Result<()> {
    /// let mut total = 0;
    /// Deal::query()
    ///     .chunk(&db, 200, |deals| {
    ///         total += deals.iter().map(|deal| deal.amount_cents).sum::<i64>();
    ///         async { Ok(ControlFlow::Continue(())) }
    ///     })
    ///     .await?;
    /// # Ok(()) }
    /// ```
    pub async fn chunk<E, F, Fut>(self, exec: &E, size: u64, mut f: F) -> Result<bool>
    where
        E: Executor + ?Sized,
        F: FnMut(Collection<M>) -> Fut,
        Fut: Future<Output = Result<ControlFlow<()>>>,
    {
        Walk::by_offset(self.for_offset_pages(), size, "chunk")?
            .chunks(exec, |page| f(Collection::from(page)))
            .await
    }

    /// The values `f` maps every record to, read `size` records at a time.
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # async fn demo(db: Db) -> Result<()> {
    /// let titles: Vec<String> = Deal::query().chunk_map(&db, 500, |deal| deal.title.clone()).await?;
    /// # Ok(()) }
    /// ```
    pub async fn chunk_map<E, T, F>(self, exec: &E, size: u64, mut f: F) -> Result<Vec<T>>
    where
        E: Executor + ?Sized,
        F: FnMut(Record<M>) -> T,
    {
        let mut mapped = Vec::new();
        let mut walk = Walk::by_offset(self.for_offset_pages(), size, "chunk_map")?;
        while let Some(page) = walk.next_page(exec).await? {
            mapped.extend(page.into_iter().map(&mut f));
        }
        Ok(mapped)
    }

    /// Calls `f` with every record, read `size` at a time like
    /// [`chunk`](Self::chunk); stops when `f` returns
    /// [`ControlFlow::Break`]. Returns `false` when `f` stopped it.
    pub async fn each<E, F, Fut>(self, exec: &E, size: u64, f: F) -> Result<bool>
    where
        E: Executor + ?Sized,
        F: FnMut(Record<M>) -> Fut,
        Fut: Future<Output = Result<ControlFlow<()>>>,
    {
        Walk::by_offset(self.for_offset_pages(), size, "each")?
            .items(exec, f)
            .await
    }

    /// Calls `f` with the records `size` at a time, by ascending key: each
    /// page is `WHERE key > last ORDER BY key LIMIT size`, so pages stay
    /// correct while `f` changes or deletes rows. Returns `false` when `f`
    /// stopped it.
    ///
    /// The query's orders are replaced by the key's (key pages must follow
    /// the key); its offset applies to the first page and its limit to the
    /// whole walk.
    pub async fn chunk_by_id<E, F, Fut>(self, exec: &E, size: u64, mut f: F) -> Result<bool>
    where
        E: Executor + ?Sized,
        F: FnMut(Collection<M>) -> Fut,
        Fut: Future<Output = Result<ControlFlow<()>>>,
    {
        self.key_walk(size, None, false, "chunk_by_id")?
            .chunks(exec, |page| f(Collection::from(page)))
            .await
    }

    /// [`chunk_by_id`](Self::chunk_by_id) by descending key (newest first
    /// for time-ordered keys).
    pub async fn chunk_by_id_desc<E, F, Fut>(self, exec: &E, size: u64, mut f: F) -> Result<bool>
    where
        E: Executor + ?Sized,
        F: FnMut(Collection<M>) -> Fut,
        Fut: Future<Output = Result<ControlFlow<()>>>,
    {
        self.key_walk(size, None, true, "chunk_by_id_desc")?
            .chunks(exec, |page| f(Collection::from(page)))
            .await
    }

    /// [`chunk_by_id`](Self::chunk_by_id) on another unique column
    /// (ascending), whose value is read back from result column `alias` (the
    /// column's own name, or the name it is selected as). Rows where the
    /// column is NULL are left out.
    pub async fn chunk_by_column<E, F, Fut>(
        self,
        exec: &E,
        size: u64,
        column: impl IntoExpr,
        alias: impl Into<String>,
        mut f: F,
    ) -> Result<bool>
    where
        E: Executor + ?Sized,
        F: FnMut(Collection<M>) -> Fut,
        Fut: Future<Output = Result<ControlFlow<()>>>,
    {
        let column = keyset_column(column)?;
        self.key_walk(size, Some((column, alias.into())), false, "chunk_by_id")?
            .chunks(exec, |page| f(Collection::from(page)))
            .await
    }

    /// [`chunk_by_column`](Self::chunk_by_column), descending.
    pub async fn chunk_by_column_desc<E, F, Fut>(
        self,
        exec: &E,
        size: u64,
        column: impl IntoExpr,
        alias: impl Into<String>,
        mut f: F,
    ) -> Result<bool>
    where
        E: Executor + ?Sized,
        F: FnMut(Collection<M>) -> Fut,
        Fut: Future<Output = Result<ControlFlow<()>>>,
    {
        let column = keyset_column(column)?;
        self.key_walk(size, Some((column, alias.into())), true, "chunk_by_id_desc")?
            .chunks(exec, |page| f(Collection::from(page)))
            .await
    }

    /// Calls `f` with every record, read `size` at a time by key like
    /// [`chunk_by_id`](Self::chunk_by_id). Returns `false` when `f` stopped
    /// it.
    pub async fn each_by_id<E, F, Fut>(self, exec: &E, size: u64, f: F) -> Result<bool>
    where
        E: Executor + ?Sized,
        F: FnMut(Record<M>) -> Fut,
        Fut: Future<Output = Result<ControlFlow<()>>>,
    {
        self.key_walk(size, None, false, "each_by_id")?
            .items(exec, f)
            .await
    }

    /// The records as a stream, read `size` at a time like
    /// [`chunk`](Self::chunk) (each page is one query; no connection is
    /// held between pages).
    pub fn lazy<'e, E: Executor + ?Sized>(
        self,
        exec: &'e E,
        size: u64,
    ) -> RowStream<'e, Result<Record<M>>> {
        Walk::stream(Walk::by_offset(self.for_offset_pages(), size, "lazy"), exec)
    }

    /// The records as a stream, read `size` at a time by ascending key like
    /// [`chunk_by_id`](Self::chunk_by_id).
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// use navree_orm::streams::TryStreamExt;
    ///
    /// # async fn demo(db: Db) -> Result<()> {
    /// let mut deals = Deal::query().lazy_by_id(&db, 1000);
    /// while let Some(mut deal) = deals.try_next().await? {
    ///     deal.views += 1;
    ///     deal.save(&db).await?; // fine between pages
    /// }
    /// # Ok(()) }
    /// ```
    pub fn lazy_by_id<'e, E: Executor + ?Sized>(
        self,
        exec: &'e E,
        size: u64,
    ) -> RowStream<'e, Result<Record<M>>> {
        Walk::stream(self.key_walk(size, None, false, "lazy_by_id"), exec)
    }

    /// [`lazy_by_id`](Self::lazy_by_id) by descending key.
    pub fn lazy_by_id_desc<'e, E: Executor + ?Sized>(
        self,
        exec: &'e E,
        size: u64,
    ) -> RowStream<'e, Result<Record<M>>> {
        Walk::stream(self.key_walk(size, None, true, "lazy_by_id_desc"), exec)
    }

    /// [`lazy_by_id`](Self::lazy_by_id) on another unique column, read back
    /// from result column `alias`.
    pub fn lazy_by_column<'e, E: Executor + ?Sized>(
        self,
        exec: &'e E,
        size: u64,
        column: impl IntoExpr,
        alias: impl Into<String>,
    ) -> RowStream<'e, Result<Record<M>>> {
        let walk = keyset_column(column).and_then(|column| {
            self.key_walk(size, Some((column, alias.into())), false, "lazy_by_id")
        });
        Walk::stream(walk, exec)
    }

    /// [`lazy_by_column`](Self::lazy_by_column), descending.
    pub fn lazy_by_column_desc<'e, E: Executor + ?Sized>(
        self,
        exec: &'e E,
        size: u64,
        column: impl IntoExpr,
        alias: impl Into<String>,
    ) -> RowStream<'e, Result<Record<M>>> {
        let walk = keyset_column(column).and_then(|column| {
            self.key_walk(size, Some((column, alias.into())), true, "lazy_by_id_desc")
        });
        Walk::stream(walk, exec)
    }

    /// The records of one query as a stream, decoded row by row as the
    /// database sends them: constant memory however many rows match.
    ///
    /// The stream holds a connection until it ends or is dropped: inside a
    /// unit of work that is the transaction's, and statements through the
    /// same unit of work are refused with an error until the cursor is done
    /// (they would wait for it forever). Use [`lazy_by_id`](Self::lazy_by_id)
    /// to write while iterating. Retrieved hooks run per record; eager loads
    /// do not apply (use [`lazy`](Self::lazy)).
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// use navree_orm::streams::TryStreamExt;
    ///
    /// # async fn demo(db: Db) -> Result<()> {
    /// let mut deals = Deal::query().where_(Deal::STAGE.eq(Stage::Won)).cursor(&db);
    /// let mut total = 0;
    /// while let Some(deal) = deals.try_next().await? {
    ///     total += deal.amount_cents;
    /// }
    /// # Ok(()) }
    /// ```
    pub fn cursor<'e, E: Executor + ?Sized>(self, exec: &'e E) -> RowStream<'e, Result<Record<M>>> {
        RowStream::try_generate(move |mut out| async move {
            let outcome = async {
                let target = exec.target(crate::query::read_access(&self.ir)).await?;
                let ir = self.into_ir();
                let mut rows = target.stream(&ir).await?;
                let ctx = target.context();
                let hooks = Hooks::<M>::resolve(&ctx).await?;
                while let Some(row) = rows.next().await {
                    let mut record = Record::<M>::from_row(&row?)?;
                    if !hooks.is_empty() {
                        hooks.after(ModelEvent::Retrieved, &ctx, &mut record).await?;
                    }
                    out.send(Ok(record)).await;
                }
                Ok(())
            }
            .await;
            (out, outcome)
        })
    }
}

// ---------------------------------------------------------------------------
// Table queries
// ---------------------------------------------------------------------------

impl TableBuilder {
    /// The query ready for offset pages: it must be ordered.
    fn for_offset_pages(mut self, operation: &str) -> Result<Self> {
        let ir = HasQuery::query_mut(&mut self);
        if ir.orders.is_empty() {
            return Err(OrmError::invalid(format!(
                "{operation} needs an order on a table query (add order_by(..) so pages neither overlap nor skip rows)"
            )));
        }
        Ok(self)
    }

    /// A keyset walk on `column`, read back from result column `alias`.
    fn key_walk(
        self,
        size: u64,
        column: ColumnRef,
        alias: String,
        descending: bool,
        operation: &'static str,
    ) -> Result<Walk<Self>> {
        Walk::by_column(self, size, column, alias, descending, false, operation)
    }

    /// Calls `f` with the rows `size` at a time, in the query's order (which
    /// it must have); stops when `f` returns [`ControlFlow::Break`]. Returns
    /// `false` when `f` stopped it. See [`Builder::chunk`].
    pub async fn chunk<E, F, Fut>(self, exec: &E, size: u64, f: F) -> Result<bool>
    where
        E: Executor + ?Sized,
        F: FnMut(Vec<Row>) -> Fut,
        Fut: Future<Output = Result<ControlFlow<()>>>,
    {
        Walk::by_offset(self.for_offset_pages("chunk")?, size, "chunk")?
            .chunks(exec, f)
            .await
    }

    /// The values `f` maps every row to, read `size` rows at a time.
    pub async fn chunk_map<E, T, F>(self, exec: &E, size: u64, mut f: F) -> Result<Vec<T>>
    where
        E: Executor + ?Sized,
        F: FnMut(Row) -> T,
    {
        let mut mapped = Vec::new();
        let mut walk = Walk::by_offset(self.for_offset_pages("chunk_map")?, size, "chunk_map")?;
        while let Some(page) = walk.next_page(exec).await? {
            mapped.extend(page.into_iter().map(&mut f));
        }
        Ok(mapped)
    }

    /// Calls `f` with every row, read `size` at a time; stops when `f`
    /// returns [`ControlFlow::Break`].
    pub async fn each<E, F, Fut>(self, exec: &E, size: u64, f: F) -> Result<bool>
    where
        E: Executor + ?Sized,
        F: FnMut(Row) -> Fut,
        Fut: Future<Output = Result<ControlFlow<()>>>,
    {
        Walk::by_offset(self.for_offset_pages("each")?, size, "each")?
            .items(exec, f)
            .await
    }

    /// Calls `f` with the rows `size` at a time by ascending `id` (see
    /// [`Builder::chunk_by_id`]).
    pub async fn chunk_by_id<E, F, Fut>(self, exec: &E, size: u64, f: F) -> Result<bool>
    where
        E: Executor + ?Sized,
        F: FnMut(Vec<Row>) -> Fut,
        Fut: Future<Output = Result<ControlFlow<()>>>,
    {
        let column = id_column(&self);
        self.key_walk(size, column, "id".to_owned(), false, "chunk_by_id")?
            .chunks(exec, f)
            .await
    }

    /// [`chunk_by_id`](Self::chunk_by_id) by descending `id`.
    pub async fn chunk_by_id_desc<E, F, Fut>(self, exec: &E, size: u64, f: F) -> Result<bool>
    where
        E: Executor + ?Sized,
        F: FnMut(Vec<Row>) -> Fut,
        Fut: Future<Output = Result<ControlFlow<()>>>,
    {
        let column = id_column(&self);
        self.key_walk(size, column, "id".to_owned(), true, "chunk_by_id_desc")?
            .chunks(exec, f)
            .await
    }

    /// Calls `f` with the rows `size` at a time by ascending `column`, read
    /// back from result column `alias`.
    pub async fn chunk_by_column<E, F, Fut>(
        self,
        exec: &E,
        size: u64,
        column: impl IntoExpr,
        alias: impl Into<String>,
        f: F,
    ) -> Result<bool>
    where
        E: Executor + ?Sized,
        F: FnMut(Vec<Row>) -> Fut,
        Fut: Future<Output = Result<ControlFlow<()>>>,
    {
        let column = keyset_column(column)?;
        self.key_walk(size, column, alias.into(), false, "chunk_by_id")?
            .chunks(exec, f)
            .await
    }

    /// Calls `f` with every row, read `size` at a time by ascending `id`.
    pub async fn each_by_id<E, F, Fut>(self, exec: &E, size: u64, f: F) -> Result<bool>
    where
        E: Executor + ?Sized,
        F: FnMut(Row) -> Fut,
        Fut: Future<Output = Result<ControlFlow<()>>>,
    {
        let column = id_column(&self);
        self.key_walk(size, column, "id".to_owned(), false, "each_by_id")?
            .items(exec, f)
            .await
    }

    /// The rows as a stream, read `size` at a time in the query's order.
    pub fn lazy<'e, E: Executor + ?Sized>(self, exec: &'e E, size: u64) -> RowStream<'e, Result<Row>> {
        let walk = self
            .for_offset_pages("lazy")
            .and_then(|query| Walk::by_offset(query, size, "lazy"));
        Walk::stream(walk, exec)
    }

    /// The rows as a stream, read `size` at a time by ascending `id`.
    pub fn lazy_by_id<'e, E: Executor + ?Sized>(
        self,
        exec: &'e E,
        size: u64,
    ) -> RowStream<'e, Result<Row>> {
        let column = id_column(&self);
        Walk::stream(
            self.key_walk(size, column, "id".to_owned(), false, "lazy_by_id"),
            exec,
        )
    }

    /// The rows as a stream, read `size` at a time by descending `id`.
    pub fn lazy_by_id_desc<'e, E: Executor + ?Sized>(
        self,
        exec: &'e E,
        size: u64,
    ) -> RowStream<'e, Result<Row>> {
        let column = id_column(&self);
        Walk::stream(
            self.key_walk(size, column, "id".to_owned(), true, "lazy_by_id_desc"),
            exec,
        )
    }

    /// The rows as a stream, read `size` at a time by ascending `column`,
    /// read back from result column `alias`.
    pub fn lazy_by_column<'e, E: Executor + ?Sized>(
        self,
        exec: &'e E,
        size: u64,
        column: impl IntoExpr,
        alias: impl Into<String>,
    ) -> RowStream<'e, Result<Row>> {
        let walk = keyset_column(column).and_then(|column| {
            self.key_walk(size, column, alias.into(), false, "lazy_by_id")
        });
        Walk::stream(walk, exec)
    }

    /// The rows of one query as a stream, as the database sends them (see
    /// [`Builder::cursor`]).
    pub fn cursor<'e, E: Executor + ?Sized>(self, exec: &'e E) -> RowStream<'e, Result<Row>> {
        RowStream::try_generate(move |mut out| async move {
            let outcome = async {
                let target = exec.target(crate::query::read_access(self.ir())).await?;
                let ir = self.into_ir();
                let mut rows = target.stream(&ir).await?;
                while let Some(row) = rows.next().await {
                    out.send(Ok(row?)).await;
                }
                Ok(())
            }
            .await;
            (out, outcome)
        })
    }
}

/// The `id` column of a table query, qualified by its table (or alias).
fn id_column(query: &TableBuilder) -> ColumnRef {
    ColumnRef::new(Some(query.ir().table.qualifier().to_owned()), "id")
}

// ---------------------------------------------------------------------------
// Raw statements
// ---------------------------------------------------------------------------

/// The rows of raw query `query` as a stream, read one by one as the
/// database sends them (see [`Builder::cursor`]: the stream holds a
/// connection while it lives).
///
/// ```no_run
/// # use navree_orm::__doc_models::*;
/// use navree_orm::streams::TryStreamExt;
/// use navree_orm::{cursor, raw};
///
/// # async fn demo(db: Db) -> Result<()> {
/// let mut rows = cursor(&db, raw("SELECT id, payload FROM events WHERE name = ?", ["signup"]));
/// while let Some(row) = rows.try_next().await? {
///     let payload: serde_json::Value = row.get("payload")?;
///     # let _ = payload;
/// }
/// # Ok(()) }
/// ```
pub fn cursor<E: Executor + ?Sized>(exec: &E, query: Raw) -> RowStream<'_, Result<Row>> {
    RowStream::try_generate(move |mut out| async move {
        let outcome = async {
            let target = exec.target(Access::Read).await?;
            let sql = crate::raw::sql_target(&target)?;
            let statement = query.to_statement(sql.dialect())?;
            let mut rows = sql.stream(&statement).await?;
            while let Some(row) = rows.next().await {
                out.send(Ok(row?)).await;
            }
            Ok(())
        }
        .await;
        (out, outcome)
    })
}

#[cfg(test)]
mod tests {
    use futures::StreamExt;

    use super::*;

    #[tokio::test]
    async fn generators_hand_over_one_item_at_a_time() {
        let produced = Arc::new(AtomicUsize::new(0));
        let counter = produced.clone();
        let mut stream = RowStream::generate(move |mut out| async move {
            for i in 0..3 {
                counter.fetch_add(1, Ordering::SeqCst);
                out.send(i).await;
            }
        });
        assert_eq!(produced.load(Ordering::SeqCst), 0, "nothing runs before a poll");
        assert_eq!(stream.next().await, Some(0));
        assert_eq!(produced.load(Ordering::SeqCst), 1, "no read-ahead");
        assert_eq!(stream.next().await, Some(1));
        assert_eq!(stream.next().await, Some(2));
        assert!(!stream.is_terminated());
        assert_eq!(stream.next().await, None);
        assert!(stream.is_terminated());
        assert_eq!(stream.next().await, None);
    }

    #[tokio::test]
    async fn failures_end_the_stream() {
        let mut stream = RowStream::<i32>::try_generate(|mut out| async move {
            out.send(Ok(1)).await;
            (out, Err(OrmError::invalid("boom")))
        });
        assert_eq!(stream.next().await.unwrap().unwrap(), 1);
        assert_eq!(
            stream.next().await.unwrap().unwrap_err().to_string(),
            "invalid query: boom"
        );
        assert!(stream.next().await.is_none());
    }

    #[test]
    fn busy_transactions_are_refused_until_the_guard_drops() {
        assert!(ensure_free(Some(41)).is_ok());
        let guard = StreamingGuard::new(41);
        assert!(ensure_free(Some(41)).is_err());
        assert!(ensure_free(Some(42)).is_ok());
        assert!(ensure_free(None).is_ok());
        drop(guard);
        assert!(ensure_free(Some(41)).is_ok());
    }
}
