//! [`MongoDb`]: a connection to a MongoDB database, the executor models run
//! on; [`MongoSession`]: one of its sessions; [`MongoExecutor`]: what they
//! (and [`MongoUow`]) share for native commands.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use bson::{Document, doc};
use mongodb::{Client, Collection, Database};
use navree_kernel::{AppContext, WeakAppContext};
use navree_orm::{Access, DocumentTarget, Executor, OrmError, Target};
use navree_telemetry::Telemetry;

use crate::driver::{MongoDriver, SessionState, Shared};
use crate::error::{MongoError, MongoResult};
use crate::options::{DEFAULT_CONNECTION, MongoOptions};
use crate::uow::MongoUow;

/// A connection to a MongoDB database: the client (with its connection
/// pools), the database, and the executor models run on.
///
/// `MongoModule` provides one (`ctx.get::<MongoDb>()`); tests and tools
/// connect with [`MongoDb::connect`]. Connecting does not reach the
/// server: the client connects on first use, so an app boots while its
/// database is down (the `mongodb` readiness check reports it).
///
/// ```no_run
/// # use navree_mongo::{MongoDb, MongoOptions};
/// # async fn demo() -> Result<(), navree_mongo::MongoError> {
/// let mongo = MongoDb::connect(&MongoOptions::new("mongodb://127.0.0.1:27017/crm")).await?;
/// mongo.ping().await?;
/// let events = mongo.collection("events");
/// # let _ = events;
/// # Ok(()) }
/// ```
///
/// Models run on it like on navree-db's `Db`: `Event::query().get(&mongo)`,
/// `Event::create(&mongo, event)`. Inside a [`MongoUow`]'s
/// [`scope`](MongoUow::scope), `MongoDb` runs in that unit of work.
#[derive(Clone)]
pub struct MongoDb {
    inner: Arc<Inner>,
    app: Option<WeakAppContext>,
}

struct Inner {
    client: Client,
    database: Database,
    options: MongoOptions,
    shared: Arc<Shared>,
    driver: Arc<MongoDriver>,
    connections: tokio::sync::Mutex<HashMap<String, MongoDb>>,
}

impl MongoDb {
    /// A connection described by `options` (checked first). The server is
    /// reached on first use.
    pub async fn connect(options: &MongoOptions) -> MongoResult<MongoDb> {
        MongoDb::connect_with(options, Telemetry::disabled()).await
    }

    /// [`connect`](Self::connect), reporting commands to `telemetry`.
    pub(crate) async fn connect_with(
        options: &MongoOptions,
        telemetry: Telemetry,
    ) -> MongoResult<MongoDb> {
        options
            .check()
            .map_err(|problems| MongoError::Options { problems })?;
        MongoDb::open(options, &options.url, options.database.as_ref(), telemetry).await
    }

    async fn open(
        options: &MongoOptions,
        url: &str,
        database: Option<&String>,
        telemetry: Telemetry,
    ) -> MongoResult<MongoDb> {
        let client_options = options.client_options(url).await?;
        let name = MongoOptions::database_of(database, &client_options);
        let client = Client::with_options(client_options)?;
        let database = client.database(&name);
        let shared = Arc::new(Shared {
            sequences: options.sequences.clone(),
            telemetry,
            connection: format!("{} ({name})", crate::options::redact_url(url)),
            slow: (options.slow_query_ms > 0).then(|| Duration::from_millis(options.slow_query_ms)),
            log_queries: options.log_queries,
        });
        let driver = Arc::new(MongoDriver::new(database.clone(), None, shared.clone()));
        Ok(MongoDb {
            inner: Arc::new(Inner {
                client,
                database,
                options: options.clone(),
                shared,
                driver,
                connections: tokio::sync::Mutex::new(HashMap::new()),
            }),
            app: None,
        })
    }

    /// The same connection, in `app` (its clock stamps timestamps, its
    /// model hooks and MongoDB change observers run).
    pub fn with_app(mut self, app: WeakAppContext) -> MongoDb {
        self.app = Some(app);
        self
    }

    /// The app the connection belongs to, while it runs.
    pub fn app(&self) -> Option<AppContext> {
        self.app.as_ref().and_then(WeakAppContext::upgrade)
    }

    /// The driver's client (its pools), for what this crate does not wrap.
    pub fn client(&self) -> &Client {
        &self.inner.client
    }

    /// The database.
    pub fn database(&self) -> &Database {
        &self.inner.database
    }

    /// The options the connection was made with.
    pub fn options(&self) -> &MongoOptions {
        &self.inner.options
    }

    /// The collection `name`, as documents.
    pub fn collection(&self, name: &str) -> Collection<Document> {
        self.inner.database.collection(name)
    }

    /// The driver models run on outside a session.
    pub fn driver(&self) -> &Arc<MongoDriver> {
        &self.inner.driver
    }

    /// Whether `other` is a handle to the same connection.
    pub fn same_as(&self, other: &MongoDb) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    /// The named connection `name` (`database.mongodb.connections.<name>`),
    /// opened on first use; `default` is this one.
    pub async fn connection(&self, name: &str) -> MongoResult<MongoDb> {
        if name == DEFAULT_CONNECTION {
            return Ok(self.clone());
        }
        let mut connections = self.inner.connections.lock().await;
        if let Some(connection) = connections.get(name) {
            return Ok(connection.clone());
        }
        let Some(settings) = self.inner.options.connections.get(name) else {
            let known: Vec<&str> = self
                .inner
                .options
                .connections
                .keys()
                .map(String::as_str)
                .collect();
            return Err(MongoError::Options {
                problems: vec![format!(
                    "there is no MongoDB connection `{name}` (configured: default{}{})",
                    if known.is_empty() { "" } else { ", " },
                    known.join(", ")
                )],
            });
        };
        let mut connection = MongoDb::open(
            &self.inner.options,
            &settings.url,
            settings.database.as_ref(),
            self.inner.shared.telemetry.clone(),
        )
        .await?;
        connection.app = self.app.clone();
        connections.insert(name.to_owned(), connection.clone());
        Ok(connection)
    }

    /// Asks the server whether it answers (`{"ping": 1}`).
    pub async fn ping(&self) -> MongoResult {
        self.inner.database.run_command(doc! { "ping": 1 }).await?;
        Ok(())
    }

    /// Runs a database command (`{"collStats": "events"}`, ...).
    pub async fn run_command(&self, command: Document) -> MongoResult<Document> {
        Ok(self.inner.database.run_command(command).await?)
    }

    /// Whether the deployment runs transactions: a replica set or a
    /// sharded cluster, not a standalone server.
    pub async fn supports_transactions(&self) -> MongoResult<bool> {
        let hello = self.inner.database.run_command(doc! { "hello": 1 }).await?;
        Ok(
            hello.contains_key("setName")
                || hello.get_str("msg").is_ok_and(|msg| msg == "isdbgrid"),
        )
    }

    /// The server's version, `(major, minor)`.
    pub async fn server_version(&self) -> MongoResult<(u32, u32)> {
        let info = self
            .inner
            .database
            .run_command(doc! { "buildInfo": 1 })
            .await?;
        let version = info.get_str("version").unwrap_or("0.0");
        let mut parts = version
            .split('.')
            .map(|part| part.parse::<u32>().unwrap_or(0));
        Ok((parts.next().unwrap_or(0), parts.next().unwrap_or(0)))
    }

    /// A session: reads in it see its writes (causal consistency). Run
    /// transactions with [`begin`](Self::begin) rather than by hand.
    pub async fn session(&self) -> MongoResult<MongoSession> {
        let session = self.inner.client.start_session().await?;
        let state = SessionState::new(session);
        Ok(MongoSession {
            driver: Arc::new(MongoDriver::new(
                self.inner.database.clone(),
                Some(state.clone()),
                self.inner.shared.clone(),
            )),
            db: self.clone(),
            state,
        })
    }

    /// A driver running in the session of `state`.
    pub(crate) fn session_driver(&self, state: Arc<SessionState>) -> Arc<MongoDriver> {
        Arc::new(MongoDriver::new(
            self.inner.database.clone(),
            Some(state),
            self.inner.shared.clone(),
        ))
    }

    /// Opens a unit of work: a session with a transaction, for `actor` and
    /// `request_id` (its change events carry them to the observers). See
    /// [`MongoUow`].
    pub async fn unit_of_work(
        &self,
        actor: Option<uuid::Uuid>,
        request_id: Option<String>,
    ) -> MongoResult<MongoUow> {
        MongoUow::begin(self, actor, request_id, None).await
    }

    /// Opens a unit of work with no actor (jobs, commands, tests).
    pub async fn begin(&self) -> MongoResult<MongoUow> {
        self.unit_of_work(None, None).await
    }

    /// Opens a unit of work joined to navree-db's unit of work `uow`: the
    /// change events of its writes go to `uow` (its observers, the audit
    /// trail, see them when it commits, with its actor and request id), and
    /// its MongoDB transaction commits right after `uow` commits. When
    /// `uow` rolls back, the MongoDB transaction is aborted. The two
    /// commits are not atomic together: a failure of the MongoDB commit
    /// after the SQL one is logged.
    pub async fn begin_in(&self, uow: &navree_db::Uow) -> MongoResult<MongoUow> {
        MongoUow::begin(
            self,
            uow.actor(),
            uow.request_id().map(str::to_owned),
            Some(uow.clone()),
        )
        .await
    }

    /// Runs `work` in a unit of work and commits it; when the work or the
    /// commit fails with a transient transaction error (a write conflict
    /// with a concurrent transaction), runs it again, up to three times.
    /// Any other error rolls back and is returned.
    ///
    /// ```no_run
    /// # use navree_mongo::{MongoDb, MongoUow, MongoError};
    /// # async fn demo(mongo: MongoDb) -> Result<(), MongoError> {
    /// let moved = mongo
    ///     .transaction(|uow: MongoUow| async move {
    ///         // Writes through `uow` (or `mongo`, inside it) commit together.
    ///         Ok(uow.actor().is_none())
    ///     })
    ///     .await?;
    /// # let _ = moved; Ok(()) }
    /// ```
    pub async fn transaction<T, F, Fut>(&self, mut work: F) -> MongoResult<T>
    where
        F: FnMut(MongoUow) -> Fut,
        Fut: Future<Output = MongoResult<T>>,
    {
        const ATTEMPTS: u32 = 3;
        let mut attempt = 1;
        loop {
            let uow = self.begin().await?;
            let result = uow.scope(work(uow.clone())).await;
            let error = match result {
                Ok(value) => match uow.commit().await {
                    Ok(()) => return Ok(value),
                    Err(error) => error,
                },
                Err(error) => {
                    if let Err(rollback) = uow.rollback().await {
                        tracing::warn!(target: "navree_mongo::uow", error = %rollback, "rolling back the unit of work failed");
                    }
                    error
                }
            };
            if attempt < ATTEMPTS && (uow.had_transient_error() || is_transient(&error)) {
                attempt += 1;
                continue;
            }
            return Err(error);
        }
    }

    /// The schema builder on this database (see [`MongoSchema`](crate::MongoSchema)).
    pub fn schema(&self) -> crate::schema::MongoSchema {
        crate::schema::MongoSchema::new(self.clone())
    }
}

/// Whether `error` is a transient transaction error.
fn is_transient(error: &MongoError) -> bool {
    match error {
        MongoError::Driver(error) => {
            error.contains_label(mongodb::error::TRANSIENT_TRANSACTION_ERROR)
                || error.contains_label(mongodb::error::UNKNOWN_TRANSACTION_COMMIT_RESULT)
        }
        _ => false,
    }
}

impl std::fmt::Debug for MongoDb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MongoDb")
            .field("connection", &self.inner.shared.connection)
            .finish_non_exhaustive()
    }
}

/// Models on the connection: in the current [`MongoUow`] of this
/// connection when a [`scope`](MongoUow::scope) runs, else outside any
/// session.
impl Executor for MongoDb {
    async fn target(&self, _access: Access) -> Result<Target<'_>, OrmError> {
        if let Some(uow) = MongoUow::current().filter(|uow| uow.db().same_as(self))
            && !uow.is_finished()
        {
            return uow.document_target();
        }
        Ok(Target::Document(DocumentTarget::new(
            self.inner.driver.clone(),
            self.app(),
        )))
    }
}

/// A session of a [`MongoDb`]: its reads see its writes. Models run in it
/// like on the connection; transactions are started and ended by hand
/// (prefer [`MongoDb::begin`] and [`MongoDb::transaction`], which also
/// record change events).
///
/// ```no_run
/// # use navree_mongo::{MongoDb, MongoResult};
/// # use navree_orm::Model;
/// # #[derive(Model, Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
/// # #[model(collection = "events", driver = "mongodb")]
/// # struct Event { id: navree_mongo::ObjectId, kind: String }
/// # async fn demo(mongo: MongoDb) -> MongoResult {
/// let session = mongo.session().await?;
/// session.start_transaction().await?;
/// Event::create(&session, Event { kind: "signup".into(), ..Event::default() }).await?;
/// session.commit_transaction().await?;
/// # Ok(()) }
/// ```
pub struct MongoSession {
    db: MongoDb,
    state: Arc<SessionState>,
    driver: Arc<MongoDriver>,
}

impl MongoSession {
    /// The connection.
    pub fn db(&self) -> &MongoDb {
        &self.db
    }

    /// Starts a transaction: until it commits or aborts, operations in the
    /// session are in it.
    pub async fn start_transaction(&self) -> MongoResult {
        self.state.session.lock().await.start_transaction().await?;
        self.state.transaction.store(true, Ordering::Relaxed);
        Ok(())
    }

    /// Commits the transaction.
    pub async fn commit_transaction(&self) -> MongoResult {
        let result = self.state.session.lock().await.commit_transaction().await;
        self.state.transaction.store(false, Ordering::Relaxed);
        Ok(result?)
    }

    /// Aborts the transaction.
    pub async fn abort_transaction(&self) -> MongoResult {
        let result = self.state.session.lock().await.abort_transaction().await;
        self.state.transaction.store(false, Ordering::Relaxed);
        Ok(result?)
    }

    /// Whether a transaction is open.
    pub fn in_transaction(&self) -> bool {
        self.state.transaction.load(Ordering::Relaxed)
    }
}

impl Executor for MongoSession {
    async fn target(&self, _access: Access) -> Result<Target<'_>, OrmError> {
        Ok(Target::Document(DocumentTarget::new(
            self.driver.clone(),
            self.db.app(),
        )))
    }
}

impl std::fmt::Debug for MongoSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MongoSession")
            .field("db", &self.db)
            .field("in_transaction", &self.in_transaction())
            .finish()
    }
}

/// An executor on MongoDB ([`MongoDb`], [`MongoSession`], [`MongoUow`]):
/// native commands ([`pipeline`](crate::MongoQuery::pipeline) queries,
/// bulk writes, cursor streams) run on its driver, in its session when it
/// has one.
///
/// ```no_run
/// use navree_mongo::{MongoExecutor, MongoResult};
///
/// /// A collection's statistics, wherever `exec` runs: on the connection,
/// /// or in a unit of work's session.
/// async fn stats(exec: &impl MongoExecutor) -> MongoResult<bson::Document> {
///     exec.mongo_driver()?
///         .run_command(bson::doc! { "collStats": "deals" })
///         .await
/// }
/// ```
pub trait MongoExecutor: Executor {
    /// The driver the next native command runs on.
    fn mongo_driver(&self) -> Result<Arc<MongoDriver>, OrmError>;
}

impl MongoExecutor for MongoDb {
    fn mongo_driver(&self) -> Result<Arc<MongoDriver>, OrmError> {
        if let Some(uow) = MongoUow::current().filter(|uow| uow.db().same_as(self))
            && !uow.is_finished()
        {
            return uow.mongo_driver();
        }
        Ok(self.inner.driver.clone())
    }
}

impl MongoExecutor for MongoSession {
    fn mongo_driver(&self) -> Result<Arc<MongoDriver>, OrmError> {
        Ok(self.driver.clone())
    }
}

impl<E: MongoExecutor + ?Sized> MongoExecutor for Arc<E> {
    fn mongo_driver(&self) -> Result<Arc<MongoDriver>, OrmError> {
        (**self).mongo_driver()
    }
}
