//! [`Navree`], [`AppBuilder`], [`App`] and [`AppContext`].

use std::any::TypeId;
use std::fmt;
use std::sync::{Arc, Weak};
use std::time::Duration;

use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::builder::{ProviderDef, value_def};
use crate::command::help_text;
use crate::container::{AppInner, RequestScope};
use crate::graph::{Graph, GraphReport, ROOT};
use crate::{
    BootErrors, BoxError, CommandSpec, KernelError, Lifecycle, Module, ModuleId, ProviderKey,
    Resolver, Scope,
};

/// Default time allowed for each phase of a graceful shutdown.
const DEFAULT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(30);

/// Entry point: `Navree::app().module(...).run()`.
#[derive(Debug)]
pub struct Navree;

impl Navree {
    /// Starts building an app.
    pub fn app() -> AppBuilder {
        AppBuilder::default()
    }
}

/// Collects the root modules of an app, then validates, boots or runs it.
pub struct AppBuilder {
    modules: Vec<Box<dyn Module>>,
    overrides: Vec<ProviderDef>,
    shutdown: CancellationToken,
    shutdown_timeout: Duration,
}

impl Default for AppBuilder {
    fn default() -> Self {
        Self {
            modules: Vec::new(),
            overrides: Vec::new(),
            shutdown: CancellationToken::new(),
            shutdown_timeout: DEFAULT_SHUTDOWN_TIMEOUT,
        }
    }
}

impl AppBuilder {
    /// Adds a root module (imported by the implicit app root).
    pub fn module(mut self, m: impl Module) -> Self {
        self.modules.push(Box::new(m));
        self
    }

    /// Replaces the unnamed provider of `T` wherever it is registered with
    /// `value` (its factory never runs); registers `value` globally if no
    /// module provides `T`. Meant for tests and fakes.
    pub fn override_value<T: ?Sized + Send + Sync + 'static>(mut self, value: Arc<T>) -> Self {
        self.overrides
            .push(value_def(ProviderKey::of::<T>(), value));
        self
    }

    /// How long graceful shutdown waits for tracked tasks, and then for the
    /// `on_shutdown` hooks (default 30 s each).
    pub fn shutdown_timeout(mut self, d: Duration) -> Self {
        self.shutdown_timeout = d;
        self
    }

    /// Validates the module graph without constructing any provider.
    pub fn validate(&self) -> Result<GraphReport, BootErrors> {
        self.graph().map(|graph| graph.report())
    }

    /// Validates the graph, constructs every singleton (modules in dependency
    /// order), then runs `on_init` and `on_bootstrap` hooks.
    pub async fn boot(self) -> Result<App, BootErrors> {
        let graph = self.graph()?;
        self.boot_graph(graph, None).await
    }

    /// Walks and validates the modules (calls every `register` once).
    fn graph(&self) -> Result<Graph, BootErrors> {
        Graph::build(&self.modules, &self.overrides)
    }

    /// Boots from an already validated graph, so `register` is not called
    /// again; `command` is the command the app is booted to run.
    async fn boot_graph(
        self,
        graph: Graph,
        command: Option<CommandSpec>,
    ) -> Result<App, BootErrors> {
        let ctx = AppContext::from_inner(Arc::new(AppInner::new(
            graph,
            self.shutdown.clone(),
            command,
        )));
        match ctx.start().await {
            Ok(hooks) => {
                tracing::debug!(modules = ctx.inner.graph.modules.len(), "navree app booted");
                Ok(App {
                    ctx,
                    hooks,
                    shutdown_timeout: self.shutdown_timeout,
                })
            }
            Err(error) => {
                ctx.inner.shutdown.cancel();
                Err(error.into())
            }
        }
    }
}

impl AppBuilder {
    /// Runs a command line (without the program name):
    ///
    /// - `check [--graph]` validates the graph without constructing anything
    ///   and prints a summary (or the module tree);
    /// - `help` / `list` prints the commands;
    /// - `introspect [--json]` validates the graph without constructing
    ///   anything and prints it as JSON ([`GraphReport::to_json`]);
    /// - `<name> [args...]` boots the app, runs the registered command, then
    ///   shuts the app down gracefully;
    /// - no command runs `serve` if a module registers it, else `help`.
    pub async fn run_with_args(self, args: Vec<String>) -> Result<(), BoxError> {
        let mut args = args.into_iter();
        let command = args.next();
        let rest: Vec<String> = args.collect();
        match command.as_deref() {
            Some("check") => {
                let report = self.validate()?;
                if rest.iter().any(|a| a == "--graph") {
                    println!("{report}");
                } else {
                    let providers: usize = report.modules.iter().map(|m| m.providers.len()).sum();
                    println!(
                        "module graph OK: {} modules, {providers} providers",
                        report.modules.len().saturating_sub(1)
                    );
                }
                Ok(())
            }
            Some("introspect") => {
                println!("{}", self.validate()?.to_json());
                Ok(())
            }
            Some("help" | "list" | "--help" | "-h") => {
                print!("{}", help_text(&self.validate()?));
                Ok(())
            }
            Some(name) => {
                let name = name.to_owned();
                let graph = self.graph()?;
                self.boot_and_run(graph, name, rest).await
            }
            None => {
                let graph = self.graph()?;
                if graph.has_command("serve") {
                    self.boot_and_run(graph, "serve".to_owned(), rest).await
                } else {
                    print!("{}", help_text(&graph.report()));
                    Ok(())
                }
            }
        }
    }

    /// Runs the app from `std::env::args` on a new multi-threaded runtime.
    /// Ctrl-C (and SIGTERM on Unix) cancels the shutdown token, so `serve`
    /// and workers stop; the app then shuts down within the shutdown timeout.
    pub fn run(self) -> Result<(), BoxError> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        let args = std::env::args_os()
            .skip(1)
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        let token = self.shutdown.clone();
        runtime.block_on(async move {
            let signals = tokio::spawn(cancel_on_signal(token));
            let result = self.run_with_args(args).await;
            signals.abort();
            result
        })
    }

    /// Checks the command exists (without constructing anything), boots from
    /// `graph`, runs the command and shuts down. The command's error wins over
    /// a shutdown error.
    async fn boot_and_run(
        self,
        graph: Graph,
        name: String,
        args: Vec<String>,
    ) -> Result<(), BoxError> {
        let Some(command) = graph.commands().find(|c| c.name == name).cloned() else {
            return Err(KernelError::UnknownCommand { name }.into());
        };
        let app = self.boot_graph(graph, Some(command)).await?;
        let result = app.run_command(&name, args).await;
        let shutdown = app.shutdown().await;
        result.and(shutdown)
    }
}

/// Waits for a shutdown signal, then cancels `token`.
async fn cancel_on_signal(token: CancellationToken) {
    shutdown_signal().await;
    tracing::info!("shutdown signal received, stopping");
    token.cancel();
}

#[cfg(unix)]
async fn shutdown_signal() {
    use tokio::signal::unix::{SignalKind, signal};
    match signal(SignalKind::terminate()) {
        Ok(mut terminate) => {
            tokio::select! {
                result = tokio::signal::ctrl_c() => if result.is_err() { std::future::pending::<()>().await },
                _ = terminate.recv() => {}
            }
        }
        Err(_) => {
            if tokio::signal::ctrl_c().await.is_err() {
                std::future::pending::<()>().await;
            }
        }
    }
}

#[cfg(not(unix))]
async fn shutdown_signal() {
    if tokio::signal::ctrl_c().await.is_err() {
        // No console to listen on: only an explicit cancel stops the app.
        std::future::pending::<()>().await;
    }
}

impl fmt::Debug for AppBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppBuilder")
            .field(
                "modules",
                &self.modules.iter().map(|m| m.name()).collect::<Vec<_>>(),
            )
            .field(
                "overrides",
                &self
                    .overrides
                    .iter()
                    .map(|o| o.key.to_string())
                    .collect::<Vec<_>>(),
            )
            .field("shutdown_timeout", &self.shutdown_timeout)
            .finish()
    }
}

/// A registered lifecycle provider and the name of its module.
type Hook = (&'static str, Arc<dyn Lifecycle>);

/// A booted app. Dropping it without [`shutdown`](App::shutdown) cancels the
/// shutdown token but runs no hooks.
pub struct App {
    ctx: AppContext,
    hooks: Vec<Hook>,
    shutdown_timeout: Duration,
}

impl App {
    /// The app context.
    pub fn ctx(&self) -> &AppContext {
        &self.ctx
    }

    /// Runs the registered command `name` against this app.
    pub async fn run_command(&self, name: &str, args: Vec<String>) -> Result<(), BoxError> {
        let command = self
            .ctx
            .inner
            .graph
            .commands()
            .find(|c| c.name == name)
            .cloned()
            .ok_or_else(|| KernelError::UnknownCommand {
                name: name.to_owned(),
            })?;
        tracing::debug!(command = name, "running command");
        (command.run)(self.ctx.clone(), args).await
    }

    /// Graceful shutdown: cancels the shutdown token, waits for tasks spawned
    /// on the [tracker](AppContext::tracker), then runs `on_shutdown` hooks in
    /// reverse dependency order. Each phase is bounded by the shutdown timeout.
    pub async fn shutdown(mut self) -> Result<(), BoxError> {
        let hooks = std::mem::take(&mut self.hooks);
        let ctx = self.ctx.clone();
        let timeout = self.shutdown_timeout;
        ctx.inner.shutdown.cancel();
        ctx.inner.tracker.close();

        let drained = tokio::time::timeout(timeout, ctx.inner.tracker.wait())
            .await
            .is_ok();
        let run_hooks = async {
            let mut errors = Vec::new();
            for (module, hooks) in hooks.iter().rev() {
                if let Err(source) = hooks.on_shutdown(ctx.clone()).await {
                    errors.push(KernelError::Lifecycle {
                        module,
                        hook: "on_shutdown",
                        source,
                    });
                }
            }
            errors
        };
        let errors = tokio::time::timeout(timeout, run_hooks).await;

        if !drained {
            return Err(format!(
                "graceful shutdown timed out after {timeout:?} waiting for tracked tasks"
            )
            .into());
        }
        match errors {
            Err(_) => Err(format!(
                "graceful shutdown timed out after {timeout:?} in on_shutdown hooks"
            )
            .into()),
            Ok(errors) if errors.is_empty() => Ok(()),
            Ok(errors) => Err(Box::new(BootErrors(errors))),
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.ctx.inner.shutdown.cancel();
    }
}

impl fmt::Debug for App {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("App")
            .field("ctx", &self.ctx)
            .finish_non_exhaustive()
    }
}

/// A value contributed with [`ModuleBuilder::contribute`](crate::ModuleBuilder::contribute)
/// and the module that contributed it.
pub struct Contributed<C> {
    /// The contributing module.
    pub module: ModuleInfo,
    /// The contributed value.
    pub value: Arc<C>,
}

impl<C> Clone for Contributed<C> {
    fn clone(&self) -> Self {
        Self {
            module: self.module.clone(),
            value: self.value.clone(),
        }
    }
}

impl<C: fmt::Debug> fmt::Debug for Contributed<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Contributed")
            .field("module", &self.module.name)
            .field("value", &self.value)
            .finish()
    }
}

/// A module of a booted app.
#[derive(Clone, Debug)]
pub struct ModuleInfo {
    /// The module id.
    pub id: ModuleId,
    /// The module name.
    pub name: &'static str,
    /// Whether the module's exports are visible everywhere.
    pub global: bool,
}

/// A cheap, cloneable handle to a booted app.
#[derive(Clone)]
pub struct AppContext {
    pub(crate) inner: Arc<AppInner>,
}

impl AppContext {
    pub(crate) fn from_inner(inner: Arc<AppInner>) -> Self {
        Self { inner }
    }

    /// A handle that does not keep the app alive. Providers that need the
    /// app after boot (an event bus, a scheduler) hold this instead of an
    /// `AppContext`, so an app dropped without `shutdown` is still freed.
    pub fn downgrade(&self) -> WeakAppContext {
        WeakAppContext {
            inner: Arc::downgrade(&self.inner),
        }
    }

    /// Service locator: resolves the unnamed provider of `T` from any module,
    /// ignoring visibility. Meant for lifecycle hooks, commands and glue code;
    /// providers use constructor injection instead.
    pub async fn get<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Arc<T>, KernelError> {
        self.locate::<T>(ProviderKey::of::<T>()).await
    }

    /// Like [`get`](AppContext::get), but `None` when no provider of `T` exists
    /// (or it cannot be built; that failure is logged).
    pub async fn try_get<T: ?Sized + Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        match self.get::<T>().await {
            Ok(value) => Some(value),
            Err(KernelError::MissingProvider { .. }) => None,
            Err(error) => {
                tracing::warn!(error = %error, "optional provider could not be resolved");
                None
            }
        }
    }

    /// Service locator for the provider of `T` registered under `name`.
    pub async fn get_named<T: ?Sized + Send + Sync + 'static>(
        &self,
        name: &str,
    ) -> Result<Arc<T>, KernelError> {
        self.locate::<T>(ProviderKey::named::<T>(name.to_owned()))
            .await
    }

    /// Resolves every multi-binding of `T` (see [`ModuleBuilder::multi`](crate::ModuleBuilder::multi)),
    /// in module dependency order.
    pub async fn get_all<T: ?Sized + Send + Sync + 'static>(
        &self,
    ) -> Result<Vec<Arc<T>>, KernelError> {
        self.root_resolver().get_all::<T>().await
    }

    /// How many multi-bindings of `T` the app registers, read from the
    /// module graph without resolving any (so a provider's factory can call
    /// it even when those multi-bindings depend on the provider). A module
    /// decides with it, once at boot, whether anybody listens: navree-db
    /// skips preparing change events in an app with no `dyn ChangeObserver`.
    pub fn multi_count<T: ?Sized + 'static>(&self) -> usize {
        self.inner.graph.multi_of(std::any::TypeId::of::<T>()).len()
    }

    /// Every value of type `C` contributed by any module, modules in dependency
    /// order, then in registration order.
    pub fn contributions<C: Send + Sync + 'static>(&self) -> Vec<Contributed<C>> {
        let graph = &self.inner.graph;
        let mut collected = Vec::new();
        for &m in &graph.order {
            for (type_id, value) in &graph.modules[m].contributions {
                if *type_id == TypeId::of::<C>()
                    && let Ok(value) = value.clone().downcast::<C>()
                {
                    collected.push(Contributed {
                        module: self.module_info(m),
                        value,
                    });
                }
            }
        }
        collected
    }

    /// A resolver with the visibility of `module` (no request scope).
    pub fn resolver_for(&self, module: ModuleId) -> Resolver {
        Resolver::new(self.inner.clone(), module, None)
    }

    /// A resolver with the visibility of `module` that memoizes request-scoped
    /// providers in `scope` (one scope per HTTP request, job run, command, ...).
    pub fn request_resolver(&self, module: ModuleId, scope: RequestScope) -> Resolver {
        Resolver::new(self.inner.clone(), module, Some(scope))
    }

    /// Every registered command, modules in dependency order.
    pub fn commands(&self) -> Vec<CommandSpec> {
        self.inner.graph.commands().cloned().collect()
    }

    /// The command this app was booted to run by
    /// [`AppBuilder::run_with_args`] (or [`AppBuilder::run`]), known from
    /// the first provider built on; `None` after a plain
    /// [`boot`](AppBuilder::boot). Providers check
    /// [`CommandSpec::stdio`] here before writing to standard output.
    pub fn command(&self) -> Option<&CommandSpec> {
        self.inner.command.as_ref()
    }

    /// Every module of the app in dependency order (imports first).
    pub fn modules(&self) -> Vec<ModuleInfo> {
        self.inner
            .graph
            .order
            .iter()
            .filter(|&&m| m != ROOT)
            .map(|&m| self.module_info(m))
            .collect()
    }

    /// Cancelled when the app shuts down; long-running tasks should watch it.
    pub fn shutdown_token(&self) -> CancellationToken {
        self.inner.shutdown.clone()
    }

    /// Spawn background tasks here; graceful shutdown waits for them.
    pub fn tracker(&self) -> TaskTracker {
        self.inner.tracker.clone()
    }

    fn module_info(&self, m: usize) -> ModuleInfo {
        let module = &self.inner.graph.modules[m];
        ModuleInfo {
            id: module.id,
            name: module.name,
            global: module.global,
        }
    }

    async fn locate<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: ProviderKey,
    ) -> Result<Arc<T>, KernelError> {
        let index = self.inner.graph.locate(&key, "`AppContext`")?;
        self.root_resolver().resolve_as::<T>(index, &key).await
    }

    /// A resolver with the visibility of the implicit app root.
    fn root_resolver(&self) -> Resolver {
        Resolver::new(self.inner.clone(), self.inner.graph.modules[ROOT].id, None)
    }

    /// Constructs every singleton (modules in dependency order), then runs
    /// `on_init` and `on_bootstrap`. Returns the lifecycle providers.
    async fn start(&self) -> Result<Vec<Hook>, KernelError> {
        let graph = &self.inner.graph;
        let mut hooks = Vec::new();
        for &m in &graph.order {
            let resolver = self.resolver_for(graph.modules[m].id);
            for &p in &graph.modules[m].providers {
                if graph.providers[p].def.scope == Scope::Singleton {
                    resolver.resolve_index(p).await?;
                }
            }
            for registration in &graph.modules[m].lifecycles {
                hooks.push((
                    graph.modules[m].name,
                    (registration.resolve)(resolver.clone()).await?,
                ));
            }
        }
        for (module, hooks) in &hooks {
            hooks
                .on_init(self.clone())
                .await
                .map_err(|source| KernelError::Lifecycle {
                    module,
                    hook: "on_init",
                    source,
                })?;
        }
        for (module, hooks) in &hooks {
            hooks
                .on_bootstrap(self.clone())
                .await
                .map_err(|source| KernelError::Lifecycle {
                    module,
                    hook: "on_bootstrap",
                    source,
                })?;
        }
        Ok(hooks)
    }
}

impl fmt::Debug for AppContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppContext")
            .field("modules", &self.inner.graph.modules.len())
            .field("providers", &self.inner.graph.providers.len())
            .finish()
    }
}

/// A weak handle to a booted app ([`AppContext::downgrade`]): it does not
/// keep the app alive, and [`upgrade`](WeakAppContext::upgrade) returns
/// `None` once the app is dropped.
#[derive(Clone)]
pub struct WeakAppContext {
    inner: Weak<AppInner>,
}

impl WeakAppContext {
    /// The app, if it is still alive.
    pub fn upgrade(&self) -> Option<AppContext> {
        self.inner.upgrade().map(AppContext::from_inner)
    }
}

impl fmt::Debug for WeakAppContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WeakAppContext")
            .field("alive", &(self.inner.strong_count() > 0))
            .finish()
    }
}
