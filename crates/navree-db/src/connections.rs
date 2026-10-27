//! [`DbConnections`]: the default connection and the named ones.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::Arc;

use navree_kernel::{AppContext, BoxError, BoxFuture, Lifecycle, WeakAppContext};
use navree_telemetry::Telemetry;
use sea_orm::DbErr;
use tokio::sync::Mutex;

use crate::db::DEFAULT_CONNECTION;
use crate::{Db, DbOptions};

/// Every configured database connection: the default one (the app's
/// [`Db`]) and the named ones of `[database.connections.<name>]`, each
/// opened on first use and shared from then on. Provided (and exported) by
/// `DatabaseModule`.
///
/// ```no_run
/// # async fn demo(ctx: navree_kernel::AppContext) -> Result<(), navree_kernel::BoxError> {
/// use navree_db::DbConnections;
///
/// let connections = ctx.get::<DbConnections>().await?;
/// let analytics = connections.get("analytics").await?;
/// println!("{} talks to {:?}", analytics.name(), analytics.backend());
/// # Ok(()) }
/// ```
///
/// Named connections belong to the app like the default one (their units
/// of work report to the app's change observers), and close when the app
/// shuts down.
pub struct DbConnections {
    default: Arc<Db>,
    configured: BTreeMap<String, DbOptions>,
    open: Mutex<HashMap<String, Arc<Db>>>,
    app: Option<WeakAppContext>,
    telemetry: Telemetry,
}

impl DbConnections {
    /// The connections of `options` (`connections`), beside `default`.
    pub fn new(
        default: Arc<Db>,
        options: &DbOptions,
        app: Option<WeakAppContext>,
        telemetry: Telemetry,
    ) -> Self {
        DbConnections {
            default,
            configured: options.connections.clone(),
            open: Mutex::new(HashMap::new()),
            app,
            telemetry,
        }
    }

    /// The names of every configured connection, `default` included, in
    /// order.
    pub fn names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.configured.keys().map(String::as_str).collect();
        names.push(DEFAULT_CONNECTION);
        names.sort_unstable();
        names
    }

    /// The default connection (the app's [`Db`]).
    pub fn default_connection(&self) -> &Arc<Db> {
        &self.default
    }

    /// Connection `name` (`default` for the default one), opened on first
    /// use. An unknown name is an error listing the configured ones.
    pub async fn get(&self, name: &str) -> Result<Arc<Db>, DbErr> {
        if name == DEFAULT_CONNECTION {
            return Ok(self.default.clone());
        }
        let Some(options) = self.configured.get(name) else {
            return Err(DbErr::Custom(format!(
                "no database connection named `{name}` (configured: {})",
                self.names().join(", ")
            )));
        };
        let mut open = self.open.lock().await;
        if let Some(db) = open.get(name) {
            return Ok(db.clone());
        }
        let db = Db::connect_named(name, options, self.telemetry.clone())
            .await
            .map_err(|error| DbErr::Custom(format!("connection `{name}`: {error}")))?;
        let db = Arc::new(match &self.app {
            Some(app) => db.with_app(app.clone()),
            None => db,
        });
        open.insert(name.to_owned(), db.clone());
        Ok(db)
    }

    /// Closes the named connections opened so far (the default one closes
    /// with the module).
    pub async fn close(&self) -> Result<(), DbErr> {
        let open: Vec<Arc<Db>> = self.open.lock().await.drain().map(|(_, db)| db).collect();
        for db in open {
            db.close().await?;
        }
        Ok(())
    }
}

impl fmt::Debug for DbConnections {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DbConnections")
            .field("names", &self.names())
            .finish_non_exhaustive()
    }
}

/// Closes the named connections at shutdown.
impl Lifecycle for DbConnections {
    fn on_shutdown(&self, _ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move {
            self.close().await?;
            Ok(())
        })
    }
}
