//! [`CastContext`]: the app services casts work with (the encrypter, the
//! hasher, the clock, the configuration, the SQL dialect), and the scopes
//! that make them available while records are read and written.
//!
//! Casts run inside [`Model::from_row`](crate::Model::from_row) and
//! [`Model::values`](crate::Model::values), which are synchronous and take
//! no executor. The ORM therefore resolves the services a model's casts
//! need from the executor's app *before* it reads or writes the model's
//! records, and runs the reads and writes inside a scope that holds them:
//!
//! - queries, `save`, `delete`, `touch`, `refresh` and the increments
//!   resolve the context from the app of the `Db` or `Uow` they run on
//!   ([`CastContext::for_model`]), for models whose casts declare that they
//!   need one ([`Cast::CONTEXT`](crate::cast::Cast::CONTEXT));
//! - code that encodes or decodes outside those operations (a test, a
//!   script on a `Db` opened by hand) sets one itself with
//!   [`CastContext::scope`] or [`CastContext::sync_scope`];
//! - a process can install a fallback with [`CastContext::set_default`].
//!
//! A context set explicitly with `scope` / `sync_scope` wins over the app's
//! services, so tests can pin keys:
//!
//! ```
//! use navree_crypto::{AppKey, Encrypter};
//! use navree_orm::attributes::CastContext;
//! use navree_orm::cast::{Cast, Encrypted};
//!
//! let context = CastContext::new().with_encrypter(Encrypter::new(AppKey::generate()));
//! let payload = context.clone().sync_scope(|| <Encrypted as Cast<String>>::set(&"secret".to_owned()))?;
//! assert!(payload.starts_with("v1."));
//! let plain: String = context.sync_scope(|| Encrypted::get(payload))?;
//! assert_eq!(plain, "secret");
//! # Ok::<(), navree_orm::cast::CastError>(())
//! ```

use std::fmt;
use std::future::Future;
use std::sync::{Arc, LazyLock, RwLock};

use navree_config::Config;
use navree_contracts::Clock;
use navree_crypto::{Encrypter, Hasher};
use navree_kernel::AppContext;

use crate::cast::CastError;
use crate::error::{OrmError, Result};
use crate::exec::ExecContext;
use crate::model::Model;
use crate::sql::Dialect;

tokio::task_local! {
    /// The context of the running scope.
    static CURRENT: Arc<CastContext>;
}

/// The process-wide fallback ([`CastContext::set_default`]).
static DEFAULT: RwLock<Option<Arc<CastContext>>> = RwLock::new(None);

/// The context outside every scope when no default is installed.
static EMPTY: LazyLock<Arc<CastContext>> = LazyLock::new(|| Arc::new(CastContext::new()));

/// The services casts work with, for the model being read or written.
///
/// Built-in casts use the [`Encrypter`] (`"encrypted"`, `"encrypted_json"`),
/// the [`Hasher`] (`"hashed"`) and the dialect (`"vector"`); custom casts
/// read whatever they need in [`Cast::get_with`](crate::cast::Cast::get_with),
/// [`Cast::set_with`](crate::cast::Cast::set_with) and the asynchronous
/// [`Cast::prepare`](crate::cast::Cast::prepare), which can also resolve
/// any other service from [`app`](CastContext::app).
///
/// Cloning is cheap: the services are shared.
#[derive(Clone, Default)]
pub struct CastContext {
    /// The model being read or written.
    model: Option<&'static str>,
    /// The encrypter name the model declares (`#[model(encrypter = "..")]`).
    encrypter_name: Option<&'static str>,
    encrypter: Option<Arc<Encrypter>>,
    named_encrypters: Vec<(&'static str, Arc<Encrypter>)>,
    hasher: Option<Arc<Hasher>>,
    clock: Option<Arc<dyn Clock>>,
    config: Option<Arc<Config>>,
    app: Option<AppContext>,
    dialect: Option<Dialect>,
    /// Set by [`CastContext::scope`] / [`CastContext::sync_scope`] (called
    /// by application code, not by the ORM's own operations): such a
    /// context's services win over the app's.
    explicit: bool,
}

impl CastContext {
    /// An empty context: casts that need a service report which one is
    /// missing.
    pub fn new() -> Self {
        CastContext::default()
    }

    /// Uses `encrypter` for encrypted casts.
    pub fn with_encrypter(mut self, encrypter: impl Into<Arc<Encrypter>>) -> Self {
        self.encrypter = Some(encrypter.into());
        self
    }

    /// Uses `encrypter` for the models that declare `#[model(encrypter =
    /// "name")]` with this `name`.
    pub fn with_named_encrypter(
        mut self,
        name: &'static str,
        encrypter: impl Into<Arc<Encrypter>>,
    ) -> Self {
        self.named_encrypters.retain(|(known, _)| *known != name);
        self.named_encrypters.push((name, encrypter.into()));
        self
    }

    /// Uses `hasher` for hashed casts.
    pub fn with_hasher(mut self, hasher: impl Into<Arc<Hasher>>) -> Self {
        self.hasher = Some(hasher.into());
        self
    }

    /// Uses `clock` for casts that read the time.
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = Some(clock);
        self
    }

    /// Uses `config` for casts that read configuration.
    pub fn with_config(mut self, config: impl Into<Arc<Config>>) -> Self {
        self.config = Some(config.into());
        self
    }

    /// Encodes for `dialect` (casts whose stored form differs between
    /// databases).
    pub fn with_dialect(mut self, dialect: Dialect) -> Self {
        self.dialect = Some(dialect);
        self
    }

    /// The model being read or written, e.g. `Patient` (for messages).
    pub fn model(&self) -> Option<&'static str> {
        self.model
    }

    /// The encrypter of the model being read or written: the one its
    /// `#[model(encrypter = "...")]` names, else the app's.
    ///
    /// The error names what to add: navree-crypto's `CryptoModule` (with an
    /// `app.key`), a named `Encrypter` provider, or a context set with
    /// [`CastContext::scope`].
    pub fn encrypter(&self) -> Result<&Encrypter, CastError> {
        match self.encrypter_name {
            Some(name) => self
                .named_encrypters
                .iter()
                .find(|(known, _)| *known == name)
                .map(|(_, encrypter)| encrypter.as_ref())
                .ok_or_else(|| {
                    CastError::new(format!(
                        "no Encrypter named `{name}` is available: register one in the app with `m.provider_named(\"{name}\", ..)`, or set one with CastContext::with_named_encrypter"
                    ))
                }),
            None => self.encrypter.as_deref().ok_or_else(|| {
                CastError::new(
                    "no Encrypter is available: run the query on a Db or Uow of an app with navree-crypto's CryptoModule (and an `app.key`), or set one with CastContext::scope",
                )
            }),
        }
    }

    /// The hasher; the error names what to add (navree-crypto's
    /// `CryptoModule`, or a context set with [`CastContext::scope`]).
    pub fn hasher(&self) -> Result<&Hasher, CastError> {
        self.hasher.as_deref().ok_or_else(|| {
            CastError::new(
                "no Hasher is available: run the query on a Db or Uow of an app with navree-crypto's CryptoModule, or set one with CastContext::scope",
            )
        })
    }

    /// The app's clock, when it registers one.
    pub fn clock(&self) -> Option<&dyn Clock> {
        self.clock.as_deref()
    }

    /// The current time: the app's clock when it registers one, else the
    /// system clock.
    pub fn now(&self) -> chrono::DateTime<chrono::Utc> {
        self.clock
            .as_ref()
            .map_or_else(chrono::Utc::now, |clock| clock.now())
    }

    /// The app's configuration, when the context comes from an app with
    /// `ConfigModule`.
    pub fn config(&self) -> Option<&Config> {
        self.config.as_deref()
    }

    /// The app, when the context comes from one (resolve further services
    /// in [`Cast::prepare`](crate::cast::Cast::prepare), which is
    /// asynchronous).
    pub fn app(&self) -> Option<&AppContext> {
        self.app.as_ref()
    }

    /// The SQL dialect of the database being written to or read from;
    /// `None` on a document store or outside an operation.
    pub fn dialect(&self) -> Option<Dialect> {
        self.dialect
    }

    /// The context for reading and writing `M`'s records in `exec`: `None`
    /// when no cast of `M` needs one (nothing to resolve, no scope to set).
    ///
    /// Services come, in this order of precedence, from an explicit
    /// enclosing scope ([`scope`](Self::scope)), the executor's app, and
    /// then an enclosing operation's scope or the process default
    /// ([`set_default`](Self::set_default)). Fails when the model names an
    /// encrypter (`#[model(encrypter = "...")]`) that neither an explicit
    /// scope nor the app provides.
    pub async fn for_model<M: Model>(exec: &ExecContext) -> Result<Option<CastContext>> {
        let meta = M::meta();
        if !meta.uses_cast_context() {
            return Ok(None);
        }
        let outer = CastContext::current_arc();
        let mut context = CastContext {
            model: Some(meta.name()),
            encrypter_name: meta.encrypter_name(),
            dialect: exec.dialect(),
            app: exec.app().cloned(),
            explicit: outer.explicit,
            ..CastContext::default()
        };
        if outer.explicit {
            context.fill_from(&outer);
        }
        if let Some(app) = exec.app() {
            if context.encrypter.is_none() {
                context.encrypter = app.try_get::<Encrypter>().await;
            }
            if let Some(name) = meta.encrypter_name()
                && context.named_encrypter(name).is_none()
            {
                let encrypter = app.get_named::<Encrypter>(name).await.map_err(|error| {
                    OrmError::Cast {
                        model: meta.name().into(),
                        column: String::new(),
                        message: format!(
                            "{} declares #[model(encrypter = \"{name}\")], but the app provides no Encrypter named `{name}`: register one with `m.provider_named(\"{name}\", ..)` ({error})",
                            meta.name()
                        ),
                    }
                })?;
                context.named_encrypters.push((name, encrypter));
            }
            if context.hasher.is_none() {
                context.hasher = app.try_get::<Hasher>().await;
            }
            if context.clock.is_none() {
                context.clock = app.try_get::<dyn Clock>().await;
            }
            if context.config.is_none() {
                context.config = app.try_get::<Config>().await;
            }
        }
        if !outer.explicit {
            context.fill_from(&outer);
        }
        if let Some(default) = CastContext::default_context() {
            context.fill_from(&default);
        }
        Ok(Some(context))
    }

    /// Takes `other`'s services where this context lacks them.
    fn fill_from(&mut self, other: &CastContext) {
        if self.encrypter.is_none() {
            self.encrypter.clone_from(&other.encrypter);
        }
        for (name, encrypter) in &other.named_encrypters {
            if self.named_encrypter(name).is_none() {
                self.named_encrypters.push((name, Arc::clone(encrypter)));
            }
        }
        if self.hasher.is_none() {
            self.hasher.clone_from(&other.hasher);
        }
        if self.clock.is_none() {
            self.clock.clone_from(&other.clock);
        }
        if self.config.is_none() {
            self.config.clone_from(&other.config);
        }
        if self.dialect.is_none() {
            self.dialect = other.dialect;
        }
    }

    fn named_encrypter(&self, name: &str) -> Option<Arc<Encrypter>> {
        self.named_encrypters
            .iter()
            .find(|(known, _)| *known == name)
            .map(|(_, encrypter)| Arc::clone(encrypter))
    }

    /// Runs `future` with this context as the current one. Its services win
    /// over those of the app an operation inside runs in.
    pub async fn scope<F: Future>(mut self, future: F) -> F::Output {
        self.explicit = true;
        CURRENT.scope(Arc::new(self), future).await
    }

    /// Runs `f` with this context as the current one (for synchronous
    /// encoding and decoding: [`Model::values`](crate::Model::values),
    /// [`Cast::set`](crate::cast::Cast::set), ...).
    pub fn sync_scope<R>(mut self, f: impl FnOnce() -> R) -> R {
        self.explicit = true;
        CURRENT.sync_scope(Arc::new(self), f)
    }

    /// Runs `future` with this context as the current one, as the ORM does
    /// for its own operations.
    pub(crate) async fn operation_scope<F: Future>(self, future: F) -> F::Output {
        CURRENT.scope(Arc::new(self), future).await
    }

    /// Installs `context` as the process-wide fallback: what casts use
    /// outside every scope, and what operations fall back to for services
    /// their app lacks. Meant for processes with a single app or none
    /// (scripts and tools on a `Db` opened by hand); scopes are the precise
    /// tool everywhere else.
    pub fn set_default(context: CastContext) {
        let mut default = DEFAULT
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *default = Some(Arc::new(context));
    }

    /// Removes the process-wide fallback.
    pub fn clear_default() {
        let mut default = DEFAULT
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *default = None;
    }

    fn default_context() -> Option<Arc<CastContext>> {
        DEFAULT
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// The current context: the running scope's, else the process default,
    /// else an empty one.
    pub fn current() -> CastContext {
        CastContext::current_arc().as_ref().clone()
    }

    fn current_arc() -> Arc<CastContext> {
        CURRENT
            .try_with(Arc::clone)
            .ok()
            .or_else(CastContext::default_context)
            .unwrap_or_else(|| Arc::clone(&EMPTY))
    }

    /// Calls `f` with the current context (see [`current`](Self::current))
    /// without cloning its services.
    pub fn with_current<R>(f: impl FnOnce(&CastContext) -> R) -> R {
        let context = CastContext::current_arc();
        f(&context)
    }
}

impl fmt::Debug for CastContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CastContext")
            .field("model", &self.model)
            .field("encrypter_name", &self.encrypter_name)
            .field("encrypter", &self.encrypter.is_some())
            .field(
                "named_encrypters",
                &self
                    .named_encrypters
                    .iter()
                    .map(|(name, _)| *name)
                    .collect::<Vec<_>>(),
            )
            .field("hasher", &self.hasher.is_some())
            .field("clock", &self.clock.is_some())
            .field("config", &self.config.is_some())
            .field("app", &self.app.is_some())
            .field("dialect", &self.dialect)
            .finish()
    }
}

/// Runs `future` in `M`'s cast context for `exec`, when `M`'s casts need
/// one: how the ORM's operations make the app's services available to the
/// casts they encode and decode with.
pub(crate) async fn in_model_scope<M, F, T>(exec: &ExecContext, future: F) -> Result<T>
where
    M: Model,
    F: Future<Output = Result<T>>,
{
    match CastContext::for_model::<M>(exec).await? {
        Some(context) => context.operation_scope(future).await,
        None => future.await,
    }
}

#[cfg(test)]
mod tests {
    use navree_crypto::AppKey;

    use super::*;

    #[test]
    fn scopes_nest_and_restore() {
        assert_eq!(CastContext::current().dialect(), None);
        let outer = CastContext::new().with_dialect(Dialect::Postgres);
        outer.sync_scope(|| {
            assert_eq!(CastContext::current().dialect(), Some(Dialect::Postgres));
            CastContext::new().with_dialect(Dialect::MySql).sync_scope(|| {
                assert_eq!(CastContext::current().dialect(), Some(Dialect::MySql));
            });
            assert_eq!(CastContext::current().dialect(), Some(Dialect::Postgres));
        });
        assert_eq!(CastContext::current().dialect(), None);
    }

    #[test]
    fn missing_services_say_what_to_add() {
        let context = CastContext::new();
        let error = context.encrypter().unwrap_err().to_string();
        assert!(error.contains("CryptoModule"), "{error}");
        assert!(error.contains("CastContext::scope"), "{error}");
        assert!(context.hasher().unwrap_err().to_string().contains("Hasher"));
        let named = CastContext {
            encrypter_name: Some("pii"),
            ..CastContext::new()
        };
        let error = named.encrypter().unwrap_err().to_string();
        assert!(error.contains("no Encrypter named `pii`"), "{error}");
    }

    #[test]
    fn a_model_picks_its_named_encrypter() {
        let default = Encrypter::new(AppKey::generate());
        let pii = Encrypter::new(AppKey::generate());
        let pii_kid = pii.current_kid().map(str::to_owned);
        let context = CastContext::new()
            .with_encrypter(default)
            .with_named_encrypter("pii", Encrypter::new(AppKey::generate()))
            .with_named_encrypter("pii", pii);
        assert_eq!(context.named_encrypters.len(), 1, "the later one replaces");
        let for_model = CastContext {
            encrypter_name: Some("pii"),
            ..context.clone()
        };
        assert_eq!(
            for_model
                .encrypter()
                .unwrap()
                .current_kid()
                .map(str::to_owned),
            pii_kid
        );
        assert_ne!(
            context.encrypter().unwrap().current_kid().map(str::to_owned),
            pii_kid
        );
        assert_eq!(
            format!("{context:?}"),
            r#"CastContext { model: None, encrypter_name: None, encrypter: true, named_encrypters: ["pii"], hasher: false, clock: false, config: false, app: false, dialect: None }"#
        );
    }
}
