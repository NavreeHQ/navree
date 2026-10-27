//! Where the channel tables are read and written: the transaction of the
//! current unit of work while it is unfinished (a request handler's), else
//! the pool.

use std::sync::Arc;

use navree_db::{Db, Uow};
use navree_kernel::{AppContext, WeakAppContext};
use sea_orm::DatabaseTransaction;

use crate::NotifyError;

/// A connection to run one service call on.
pub(crate) enum Exec {
    /// The current unit of work's transaction (written to: it is marked
    /// modified).
    Tx(Uow, Arc<DatabaseTransaction>),
    /// The pool.
    Pool(Arc<Db>),
}

impl Exec {
    /// The current unfinished unit of work's transaction, else the app's
    /// pool; [`NotifyError::Unavailable`] without `DatabaseModule`.
    pub(crate) async fn current(app: &WeakAppContext) -> Result<Exec, NotifyError> {
        let ctx = upgrade(app)?;
        let db = database(&ctx).await?;
        if let Some(uow) = Uow::current()
            && !uow.is_finished().await
        {
            let tx = uow.tx().await?;
            return Ok(Exec::Tx(uow, tx));
        }
        Ok(Exec::Pool(db))
    }

    /// The pool, never a unit of work (deliveries are recorded after the
    /// channel was sent, outside any transaction).
    pub(crate) async fn pool(app: &WeakAppContext) -> Result<Exec, NotifyError> {
        let ctx = upgrade(app)?;
        Ok(Exec::Pool(database(&ctx).await?))
    }

    /// Notes a write on the unit of work.
    pub(crate) fn wrote(&self) {
        if let Exec::Tx(uow, _) = self {
            uow.mark_modified();
        }
    }
}

/// Runs `$body` with `$e` bound to the executor of an [`Exec`]: the
/// transaction or the pool connection.
macro_rules! on {
    ($exec:expr, $e:ident => $body:expr) => {
        match $exec {
            $crate::store::Exec::Tx(_, tx) => {
                let $e: &::sea_orm::DatabaseTransaction = tx;
                $body
            }
            $crate::store::Exec::Pool(db) => {
                let $e: &::sea_orm::DatabaseConnection = db.conn();
                $body
            }
        }
    };
}
pub(crate) use on;

/// The app, unless it has shut down.
pub(crate) fn upgrade(app: &WeakAppContext) -> Result<AppContext, NotifyError> {
    app.upgrade().ok_or_else(|| NotifyError::Unavailable {
        channel: None,
        reason: "the app has shut down".to_owned(),
    })
}

/// The app's database.
pub(crate) async fn database(ctx: &AppContext) -> Result<Arc<Db>, NotifyError> {
    ctx.try_get::<Db>()
        .await
        .ok_or_else(|| NotifyError::Unavailable {
            channel: None,
            reason: "the notification tables need DatabaseModule".to_owned(),
        })
}
