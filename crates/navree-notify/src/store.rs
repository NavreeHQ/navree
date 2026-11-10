//! Where the channel tables are read and written: the current unit of
//! work while it is unfinished (a request handler's), else the pool.

use std::sync::Arc;

use navree_db::{Db, Uow};
use navree_kernel::{AppContext, WeakAppContext};

use crate::NotifyError;

/// A connection to run one service call on.
pub(crate) enum Exec {
    /// The current unit of work (written to: it is marked modified).
    Tx(Uow),
    /// The pool.
    Pool(Arc<Db>),
}

impl Exec {
    /// The current unfinished unit of work, else the app's pool;
    /// [`NotifyError::Unavailable`] without `DatabaseModule`.
    pub(crate) async fn current(app: &WeakAppContext) -> Result<Exec, NotifyError> {
        let ctx = upgrade(app)?;
        let db = database(&ctx).await?;
        if let Some(uow) = Uow::current()
            && !uow.is_finished().await
        {
            return Ok(Exec::Tx(uow));
        }
        Ok(Exec::Pool(db))
    }

    /// The pool, not a unit of work of the caller's (deliveries are
    /// recorded after the channel was sent, outside any transaction).
    pub(crate) async fn pool(app: &WeakAppContext) -> Result<Exec, NotifyError> {
        let ctx = upgrade(app)?;
        Ok(Exec::Pool(database(&ctx).await?))
    }

    /// Notes a write on the unit of work.
    pub(crate) fn wrote(&self) {
        if let Exec::Tx(uow) = self {
            uow.mark_modified();
        }
    }
}

/// Runs `$body` with `$e` bound to the executor of an [`Exec`]: the unit
/// of work or the database.
macro_rules! on {
    ($exec:expr, $e:ident => $body:expr) => {
        match $exec {
            $crate::store::Exec::Tx(uow) => {
                let $e: &::navree_db::Uow = uow;
                $body
            }
            $crate::store::Exec::Pool(db) => {
                let $e: &::navree_db::Db = db;
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
