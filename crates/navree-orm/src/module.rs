//! [`OrmModule`] (the `[orm]` options, strict modes, runtime global
//! scopes, the model registry, `model:prune` and `model:show`) and
//! [`ModuleBuilderOrmExt`] (observers, listeners, scopes and models
//! registered by modules).

use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;

use navree_config::Config;
use navree_contracts::ModuleBuilderJobsExt;
use navree_kernel::{
    AppContext, BoxError, BoxFuture, CommandSpec, Factory, Injectable, Module, ModuleBuilder,
    ModuleId, Resolver,
};

use crate::error::Result;
use crate::events::{ClosureListener, Observer, ObserverHook};
use crate::exec::ExecContext;
use crate::global_scopes::ClosureScope;
use crate::hooks::{Flow, ModelEvent, ModelHook};
use crate::model::{Model, ModelMeta};
use crate::options::OrmOptions;
use crate::query::Builder;
use crate::record::Record;
use crate::scope::GlobalScope;
use crate::show::ModelDescription;
use crate::strict::{self, Strictness, Violation};

/// The ORM's module. Global; provides and exports [`OrmRuntime`].
///
/// - reads the `[orm]` section ([`OrmOptions`]): the strict modes, by
///   default on except in production;
/// - installs, at boot, the global scopes modules register
///   ([`ModuleBuilderOrmExt::global_scope`]);
/// - keeps the registry of the app's models
///   ([`ModuleBuilderOrmExt::model`]) for tooling;
/// - registers the commands `model:prune` (see [`prune`](crate::prune))
///   and `model:show` (see [`show`](crate::show)) and the job
///   `orm.broadcast` (queued model broadcasts, see
///   [`broadcast`](crate::broadcast)).
///
/// The ORM works without it (models need no module); add it to configure
/// strict modes and their handlers, or let a module that registers scopes
/// or models import it for you.
///
/// ```no_run
/// use navree_kernel::Navree;
/// use navree_orm::OrmModule;
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// Navree::app()
///     .module(
///         OrmModule::from_config().on_lazy_loading(|violation| {
///             tracing::warn!(%violation, "lazy loading"); // log instead of failing
///             Ok(())
///         }),
///     )
///     .run()
/// # }
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct OrmModule;

impl OrmModule {
    /// The module with explicit options.
    pub fn for_root(options: OrmOptions) -> ConfiguredOrmModule {
        ConfiguredOrmModule {
            options: Some(options),
            handlers: Handlers::default(),
        }
    }

    /// The module configured from the `[orm]` section at boot.
    pub fn from_config() -> ConfiguredOrmModule {
        ConfiguredOrmModule {
            options: None,
            handlers: Handlers::default(),
        }
    }
}

impl Module for OrmModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None, Handlers::default());
    }
}

/// `OrmModule::for_root(..)` / `OrmModule::from_config()`: the module with
/// its options and strict-mode handlers.
#[derive(Clone)]
pub struct ConfiguredOrmModule {
    options: Option<OrmOptions>,
    handlers: Handlers,
}

impl ConfiguredOrmModule {
    /// Lets `handler` decide about missing attributes in strict mode
    /// (`Ok(())` reads the field's default instead of failing).
    pub fn on_missing_attribute(
        mut self,
        handler: impl Fn(&Violation) -> Result<()> + Send + Sync + 'static,
    ) -> Self {
        self.handlers.missing_attribute = Some(Arc::new(handler));
        self
    }

    /// Lets `handler` decide about discarded attributes in strict mode
    /// (`Ok(())` skips them instead of failing).
    pub fn on_discarded_attributes(
        mut self,
        handler: impl Fn(&Violation) -> Result<()> + Send + Sync + 'static,
    ) -> Self {
        self.handlers.discarded_attributes = Some(Arc::new(handler));
        self
    }

    /// Lets `handler` decide about lazy loading in strict mode (`Ok(())`
    /// loads the relation instead of failing).
    pub fn on_lazy_loading(
        mut self,
        handler: impl Fn(&Violation) -> Result<()> + Send + Sync + 'static,
    ) -> Self {
        self.handlers.lazy_loading = Some(Arc::new(handler));
        self
    }
}

impl Module for ConfiguredOrmModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<OrmModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, self.options.clone(), self.handlers.clone());
    }
}

impl fmt::Debug for ConfiguredOrmModule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConfiguredOrmModule")
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

/// The strict-mode handlers given to the module.
#[derive(Clone, Default)]
struct Handlers {
    missing_attribute: Option<strict::ViolationHandler>,
    discarded_attributes: Option<strict::ViolationHandler>,
    lazy_loading: Option<strict::ViolationHandler>,
}

impl Handlers {
    fn apply(self, mut strictness: Strictness) -> Strictness {
        if let Some(handler) = self.missing_attribute {
            strictness = strictness.on_missing_attribute(move |violation| handler(violation));
        }
        if let Some(handler) = self.discarded_attributes {
            strictness = strictness.on_discarded_attributes(move |violation| handler(violation));
        }
        if let Some(handler) = self.lazy_loading {
            strictness = strictness.on_lazy_loading(move |violation| handler(violation));
        }
        strictness
    }
}

fn register(m: &mut ModuleBuilder, options: Option<OrmOptions>, handlers: Handlers) {
    m.global()
        .factory(Factory::<OrmRuntime>::new(move |r: Resolver| {
            let options = options.clone();
            let handlers = handlers.clone();
            async move {
                let app = r.app();
                let config = app.try_get::<Config>().await;
                let options = match (options, &config) {
                    (Some(options), _) => options,
                    (None, Some(config)) => config.typed::<OrmOptions>()?,
                    (None, None) => OrmOptions::default(),
                };
                let environment = config.as_deref().and_then(environment);
                let strictness =
                    handlers.apply(Strictness::from_options(&options, environment.as_deref()));
                strict::install(strictness.clone());
                for registration in app.contributions::<ScopeRegistration>() {
                    (registration.value.install)();
                }
                Ok(Arc::new(OrmRuntime {
                    options,
                    strictness,
                    environment,
                }))
            }
        }))
        .exports::<OrmRuntime>()
        .job::<crate::broadcast::BroadcastModelEvent>()
        .command(CommandSpec::new(
            "model:prune",
            "Prune the registered prunable models ([--model M]... [--except M]... [--chunk N] [--pretend])",
            crate::prune::prune_command,
        ))
        .command(crate::show::command());
}

/// The app's environment (`APP_ENV` or `app.env`), if it names one.
fn environment(config: &Config) -> Option<String> {
    config.environment().map(|env| env.to_string())
}

/// The ORM of a booted app: its options and resolved strict modes.
/// Provided by [`OrmModule`].
pub struct OrmRuntime {
    options: OrmOptions,
    strictness: Strictness,
    environment: Option<String>,
}

impl OrmRuntime {
    /// The `[orm]` options.
    pub fn options(&self) -> &OrmOptions {
        &self.options
    }

    /// The strict modes and their handlers.
    pub fn strictness(&self) -> &Strictness {
        &self.strictness
    }

    /// The app's environment (`env` / `app.env`), if it names one.
    pub fn environment(&self) -> Option<&str> {
        self.environment.as_deref()
    }
}

impl fmt::Debug for OrmRuntime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OrmRuntime")
            .field("options", &self.options)
            .field("strictness", &self.strictness)
            .field("environment", &self.environment)
            .finish()
    }
}

/// A global scope a module registered, installed when the app boots.
struct ScopeRegistration {
    install: Box<dyn Fn() + Send + Sync>,
}

/// Describes model `M` with the hooks the app registers for it.
type Describe =
    fn(AppContext) -> BoxFuture<'static, std::result::Result<ModelDescription, BoxError>>;

/// A model a module registered (see [`ModuleBuilderOrmExt::model`]).
#[derive(Clone, Copy)]
pub struct RegisteredModel {
    meta: fn() -> &'static ModelMeta,
    describe: Describe,
}

impl RegisteredModel {
    /// The registration of model `M`.
    pub fn of<M: Model>() -> Self {
        RegisteredModel {
            meta: M::meta,
            describe: describe::<M>,
        }
    }

    /// The model's metadata.
    pub fn meta(&self) -> &'static ModelMeta {
        (self.meta)()
    }

    /// The model's name, e.g. `Deal`.
    pub fn name(&self) -> &'static str {
        self.meta().name()
    }

    /// The model as `model:show` shows it, with the hooks `app` registers
    /// for it.
    pub async fn describe(
        &self,
        app: &AppContext,
    ) -> std::result::Result<ModelDescription, BoxError> {
        (self.describe)(app.clone()).await
    }
}

impl fmt::Debug for RegisteredModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("RegisteredModel")
            .field(&self.name())
            .finish()
    }
}

/// `M`'s description, with the hooks `app` registers for it.
fn describe<M: Model>(
    app: AppContext,
) -> BoxFuture<'static, std::result::Result<ModelDescription, BoxError>> {
    Box::pin(async move {
        let registered = app.get_all::<dyn ModelHook<M>>().await?;
        let app_hooks = registered.iter().map(|hook| hook.name()).collect();
        Ok(ModelDescription::of::<M>(app_hooks))
    })
}

/// The models the app's modules registered, in module dependency order
/// (each once).
pub fn registered_models(app: &AppContext) -> Vec<RegisteredModel> {
    let mut models: Vec<RegisteredModel> = Vec::new();
    for registration in app.contributions::<RegisteredModel>() {
        let model = *registration.value;
        if !models
            .iter()
            .any(|known| std::ptr::eq(known.meta(), model.meta()))
        {
            models.push(model);
        }
    }
    models
}

/// ORM registrations on a [`ModuleBuilder`]: observers and closure
/// listeners of model events, global scopes, and the app's models.
///
/// Listeners belong to the app being built, so each app (each test) has
/// its own. Global scopes apply process-wide once the app boots (scopes
/// apply where no app is at hand, such as `to_sql`).
///
/// ```no_run
/// # use navree_orm::__doc_models::*;
/// use navree_kernel::{Module, ModuleBuilder};
/// use navree_orm::ModuleBuilderOrmExt;
///
/// struct DealsModule;
///
/// impl Module for DealsModule {
///     fn register(&self, m: &mut ModuleBuilder) {
///         m.model::<Deal>()
///             .global_scope::<Deal>("open", |q| q.where_null(Deal::CLOSED_AT))
///             .on_model::<Deal, _>(ModelEvent::Saving, |deal, _ctx| {
///                 Box::pin(async move {
///                     deal.title = deal.title.trim().to_owned();
///                     Ok(())
///                 })
///             });
///     }
/// }
/// ```
pub trait ModuleBuilderOrmExt {
    /// Registers model `M` with the app, so commands and tooling find it
    /// (`model:prune`, `model:show`, ...). Rust cannot discover types at
    /// run time: a module lists its models once. Imports [`OrmModule`].
    fn model<M: Model>(&mut self) -> &mut Self;

    /// Registers `observer` for model `M`'s events (a `dyn ModelHook<M>`
    /// multi-binding).
    fn observe<M: Model>(&mut self, observer: impl Observer<M>) -> &mut Self;

    /// Registers an observer of model `M` that the container builds (with
    /// the visibility of this module), for observers with dependencies.
    fn observer<M: Model, O: Observer<M> + Injectable>(&mut self) -> &mut Self;

    /// Registers `listener` for `event` of model `M`. It may change the
    /// record; for a cancellable event it may return [`Flow::Cancel`]
    /// (`Ok(())` goes ahead). Inside a unit of work it hears "-ed" write
    /// events after the commit, on a snapshot of the record.
    ///
    /// The second type parameter is the listener's output (`()` or
    /// [`Flow`]): write `m.on_model::<Deal, _>(..)`.
    fn on_model<M, O>(
        &mut self,
        event: ModelEvent,
        listener: impl for<'a> Fn(&'a mut Record<M>, &'a ExecContext) -> BoxFuture<'a, Result<O>>
        + Send
        + Sync
        + 'static,
    ) -> &mut Self
    where
        M: Model,
        O: Into<Flow> + 'static;

    /// Registers a global scope named `name` for every query of `M`,
    /// installed when the app boots (it imports [`OrmModule`]). Queries
    /// leave it out with `without_global_scope_named(name)`.
    fn global_scope<M: Model>(
        &mut self,
        name: impl Into<Cow<'static, str>>,
        scope: impl Fn(Builder<M>) -> Builder<M> + Send + Sync + 'static,
    ) -> &mut Self;

    /// Registers `scope` (a [`GlobalScope`] value) for every query of `M`,
    /// installed when the app boots (it imports [`OrmModule`]).
    fn global_scope_of<M: Model>(&mut self, scope: impl GlobalScope<M>) -> &mut Self;
}

impl ModuleBuilderOrmExt for ModuleBuilder {
    fn model<M: Model>(&mut self) -> &mut Self {
        self.import(OrmModule)
            .contribute(RegisteredModel::of::<M>())
    }

    fn observe<M: Model>(&mut self, observer: impl Observer<M>) -> &mut Self {
        let hook: Arc<dyn ModelHook<M>> = Arc::new(ObserverHook::new(observer));
        self.multi(Factory::<dyn ModelHook<M>>::new(move |_| {
            let hook = hook.clone();
            async move { Ok(hook) }
        }))
    }

    fn observer<M: Model, O: Observer<M> + Injectable>(&mut self) -> &mut Self {
        self.multi(
            Factory::<dyn ModelHook<M>>::new(|r: Resolver| async move {
                let observer = O::construct(r).await?;
                Ok(Arc::new(ObserverHook::new(observer)) as Arc<dyn ModelHook<M>>)
            })
            .deps(O::dependencies()),
        )
    }

    fn on_model<M, O>(
        &mut self,
        event: ModelEvent,
        listener: impl for<'a> Fn(&'a mut Record<M>, &'a ExecContext) -> BoxFuture<'a, Result<O>>
        + Send
        + Sync
        + 'static,
    ) -> &mut Self
    where
        M: Model,
        O: Into<Flow> + 'static,
    {
        let hook: Arc<dyn ModelHook<M>> = Arc::new(ClosureListener::new(event, listener));
        self.multi(Factory::<dyn ModelHook<M>>::new(move |_| {
            let hook = hook.clone();
            async move { Ok(hook) }
        }))
    }

    fn global_scope<M: Model>(
        &mut self,
        name: impl Into<Cow<'static, str>>,
        scope: impl Fn(Builder<M>) -> Builder<M> + Send + Sync + 'static,
    ) -> &mut Self {
        self.global_scope_of::<M>(ClosureScope::new(name, scope))
    }

    fn global_scope_of<M: Model>(&mut self, scope: impl GlobalScope<M>) -> &mut Self {
        let scope: Arc<dyn GlobalScope<M>> = Arc::new(scope);
        self.import(OrmModule).contribute(ScopeRegistration {
            install: Box::new(move || crate::global_scopes::register::<M>(scope.clone())),
        })
    }
}
