//! [`MicroModule`], [`DeadLetters`] and the `micro:*` commands.

use std::sync::Arc;

use navree_config::ModuleBuilderConfigExt;
use navree_contracts::HealthCheck;
use navree_db::ModuleBuilderDbExt;
use navree_http::ModuleBuilderHttpExt;
use navree_kernel::{
    AppContext, BoxError, CommandSpec, Factory, Module, ModuleBuilder, ModuleId, ProviderKey,
    Resolver,
};
use serde_json::{Value, json};

use crate::context::http_context_layer;
use crate::listener::{self, subscriptions};
use crate::runtime::{MicroRuntime, Starter};
use crate::{DeadLetter, MicroError, MicroOptions, ServiceClient, store};

/// Messages and events between services.
///
/// Global: modules register handlers (`m.message_handlers::<T>()`) and
/// inject `Arc<ServiceClient>` without importing it. Provides and exports
/// [`ServiceClient`], the named clients declared with
/// [`client`](ConfiguredMicroModule::client), [`DeadLetters`] and
/// [`MicroOptions`]; contributes:
///
/// - the message listener, started by `serve` (with `HttpModule`, unless
///   `micro.listen_with_serve` is false), by `micro:listen`, or at boot
///   for the in-memory transport;
/// - the outbox relay and the inbox pruning, with `DatabaseModule`;
/// - the migration set `micro` (`micro_outbox`, `micro_inbox`,
///   `micro_dead_letters`), run by `migrate` when the app has
///   `DatabaseModule`;
/// - the `micro` readiness check (a `dyn HealthCheck`);
/// - the mapping of [`MicroError`]s returned with `?` to error responses;
/// - an HTTP layer that hands the current request's credentials, request
///   id, trace context and workspace to `ServiceClient`;
/// - the commands `micro:listen`, `micro:patterns [--json]`,
///   `micro:send`, `micro:emit`, `micro:setup`, `micro:failed`,
///   `micro:retry` and `micro:flush`.
///
/// ```no_run
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_kernel::Navree;
/// use navree_micro::{MicroModule, Transport};
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// Navree::app()
///     .module(ConfigModule::for_root(ConfigSources::defaults()))
///     .module(MicroModule::for_root(Transport::tcp("127.0.0.1:4000")))
///     .run()
/// # }
/// ```
///
/// Used plainly (`.module(MicroModule)`) or through
/// [`from_config`](MicroModule::from_config), it reads the `micro`
/// configuration section.
#[derive(Clone, Copy, Debug, Default)]
pub struct MicroModule;

impl MicroModule {
    /// The module with explicit options, or a transport alone
    /// (`MicroModule::for_root(Transport::nats(..))`); every other setting
    /// keeps its default.
    pub fn for_root(options: impl Into<MicroOptions>) -> ConfiguredMicroModule {
        ConfiguredMicroModule {
            options: Some(options.into()),
            clients: Vec::new(),
        }
    }

    /// The module configured from the `micro` section at boot.
    pub fn from_config() -> ConfiguredMicroModule {
        ConfiguredMicroModule {
            options: None,
            clients: Vec::new(),
        }
    }

    /// Starts the message listener and the outbox relay of a booted app in
    /// the background, as `serve` and `micro:listen` do; they stop when the
    /// app shuts down. Returns `false` when they already run. For custom
    /// commands, and tests over a network transport.
    pub async fn start_listener(ctx: &AppContext) -> Result<bool, BoxError> {
        let runtime = ctx.get::<MicroRuntime>().await?;
        runtime.start(ctx).await
    }
}

impl Module for MicroModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None, &[]);
    }
}

/// `MicroModule::for_root` / `from_config`: the same module, configured.
#[derive(Clone, Debug)]
pub struct ConfiguredMicroModule {
    options: Option<MicroOptions>,
    clients: Vec<String>,
}

impl ConfiguredMicroModule {
    /// Registers the named client `name` (`micro.clients.<name>`) as a
    /// `ServiceClient` provider named `name`, for
    /// `#[inject(name = "billing")]`. Its settings come from the options
    /// at boot; a name without a `[micro.clients.<name>]` section fails the
    /// boot. `for_root` registers every client of its options by itself;
    /// `from_config` needs the names here, since the configuration is read
    /// after the modules register.
    pub fn client(mut self, name: &str) -> Self {
        self.clients.push(name.to_owned());
        self
    }
}

impl Module for ConfiguredMicroModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<MicroModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        // `for_root` knows its clients; `from_config` names them in code.
        let mut clients = self.clients.clone();
        if let Some(options) = &self.options {
            clients.extend(options.clients.keys().cloned());
        }
        clients.sort();
        clients.dedup();
        register(m, self.options.clone(), &clients);
    }
}

fn register(m: &mut ModuleBuilder, options: Option<MicroOptions>, clients: &[String]) {
    m.global();
    match options {
        Some(options) => m.factory(Factory::<MicroOptions>::new(move |_| {
            let options = options.clone();
            async move {
                options
                    .check()
                    .map_err(|errors| format!("invalid micro options: {}", errors.join("; ")))?;
                Ok(Arc::new(options))
            }
        })),
        None => m.config::<MicroOptions>(),
    };
    m.exports::<MicroOptions>()
        .factory(
            Factory::<MicroRuntime>::new(|r: Resolver| async move {
                let options = r.get::<MicroOptions>().await?;
                Ok(Arc::new(
                    MicroRuntime::new(r.app(), (*options).clone()).await?,
                ))
            })
            .deps(vec![ProviderKey::of::<MicroOptions>()]),
        )
        .factory(
            Factory::<ServiceClient>::new(|r: Resolver| async move {
                Ok(r.get::<MicroRuntime>().await?.client.clone())
            })
            .deps(vec![ProviderKey::of::<MicroRuntime>()]),
        )
        .exports::<ServiceClient>()
        .factory(
            Factory::<DeadLetters>::new(|r: Resolver| async move {
                Ok(Arc::new(DeadLetters {
                    runtime: r.get::<MicroRuntime>().await?,
                }))
            })
            .deps(vec![ProviderKey::of::<MicroRuntime>()]),
        )
        .exports::<DeadLetters>()
        .factory(
            Factory::<Starter>::new(|r: Resolver| async move {
                Ok(Arc::new(Starter(r.get::<MicroRuntime>().await?)))
            })
            .deps(vec![ProviderKey::of::<MicroRuntime>()]),
        )
        .lifecycle::<Starter>()
        .multi(
            Factory::<dyn HealthCheck>::new(|r: Resolver| async move {
                Ok(r.get::<MicroRuntime>().await?.health())
            })
            .deps(vec![ProviderKey::of::<MicroRuntime>()]),
        )
        .http_layer(http_context_layer())
        .map_error::<MicroError>(|error| Some(error.to_app_error()))
        .migrations(store::migrations());
    for name in clients {
        let client_name = name.clone();
        m.factory(
            Factory::<ServiceClient>::new(move |r: Resolver| {
                let name = client_name.clone();
                async move {
                    let runtime = r.get::<MicroRuntime>().await?;
                    Ok(Arc::new(runtime.named_client(&name)?))
                }
            })
            .named(name.clone())
            .deps(vec![ProviderKey::of::<MicroRuntime>()]),
        )
        .export_key(ProviderKey::named::<ServiceClient>(name.clone()));
    }
    for (name, about, run) in COMMANDS {
        m.command(CommandSpec::new(name, about, run));
    }
}

type CommandFn =
    fn(AppContext, Vec<String>) -> navree_kernel::BoxFuture<'static, Result<(), BoxError>>;

const COMMANDS: [(&str, &str, CommandFn); 8] = [
    (
        "micro:listen",
        "Handle messages without serving HTTP, until stopped",
        |ctx, args| Box::pin(listen_command(ctx, args)),
    ),
    (
        "micro:patterns",
        "List every message and event pattern ([--json])",
        |ctx, args| Box::pin(patterns_command(ctx, args)),
    ),
    (
        "micro:send",
        "Send a request and print the reply (<pattern> <json>)",
        |ctx, args| Box::pin(send_command(ctx, args)),
    ),
    (
        "micro:emit",
        "Publish an event (<pattern> <json>)",
        |ctx, args| Box::pin(emit_command(ctx, args)),
    ),
    (
        "micro:setup",
        "Declare the streams, topics and queues the patterns need",
        |ctx, args| Box::pin(setup_command(ctx, args)),
    ),
    ("micro:failed", "List dead-lettered events", |ctx, args| {
        Box::pin(failed_command(ctx, args))
    }),
    (
        "micro:retry",
        "Retry dead-lettered events (<id|all>)",
        |ctx, args| Box::pin(retry_command(ctx, args)),
    ),
    (
        "micro:flush",
        "Delete every dead-lettered event",
        |ctx, args| Box::pin(flush_command(ctx, args)),
    ),
];

/// The dead letters of this service (`micro.events.dead_letter =
/// "database"`): what `micro:failed`, `micro:retry` and `micro:flush` use,
/// for an admin screen.
pub struct DeadLetters {
    runtime: Arc<MicroRuntime>,
}

impl DeadLetters {
    fn store(&self) -> Result<&store::Store, BoxError> {
        self.runtime.store.as_ref().ok_or_else(|| {
            "dead letters are stored in the database: add DatabaseModule to the app".into()
        })
    }

    /// Every dead letter of this service, oldest first.
    pub async fn list(&self) -> Result<Vec<DeadLetter>, BoxError> {
        Ok(self.store()?.dead_list(&self.runtime.service).await?)
    }

    /// Dead letter `id`.
    pub async fn find(&self, id: &str) -> Result<Option<DeadLetter>, BoxError> {
        Ok(self.store()?.dead_find(&self.runtime.service, id).await?)
    }

    /// Hands dead letter `id` back to this service's handler, with its
    /// attempts starting over, in this process. `true` when it was handled
    /// (and deleted); `false` when it failed again (and is a new dead
    /// letter).
    pub async fn retry(&self, id: &str) -> Result<bool, BoxError> {
        let store = self.store()?;
        let letter = store
            .dead_find(&self.runtime.service, id)
            .await?
            .ok_or_else(|| format!("no dead letter `{id}`"))?;
        let ctx = self.runtime.app()?;
        let router = self.runtime.router(&ctx).await?;
        store.dead_delete(&letter.id).await?;
        match listener::retry_dead_letter(&self.runtime, &ctx, &router, &letter).await {
            Ok(handled) => Ok(handled),
            Err(error) => {
                // Keep the event: put the letter back as it was.
                store.dead_insert(&letter).await?;
                Err(error)
            }
        }
    }

    /// Retries every dead letter; returns how many were handled and how
    /// many failed again.
    pub async fn retry_all(&self) -> Result<(usize, usize), BoxError> {
        let mut handled = 0;
        let mut failed = 0;
        for letter in self.list().await? {
            if self.retry(&letter.id).await? {
                handled += 1;
            } else {
                failed += 1;
            }
        }
        Ok((handled, failed))
    }

    /// Deletes every dead letter of this service; returns how many.
    pub async fn flush(&self) -> Result<u64, BoxError> {
        Ok(self.store()?.dead_flush(&self.runtime.service).await?)
    }
}

impl std::fmt::Debug for DeadLetters {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeadLetters")
            .field("service", &self.runtime.service)
            .finish()
    }
}

async fn listen_command(ctx: AppContext, _args: Vec<String>) -> Result<(), BoxError> {
    let runtime = ctx.get::<MicroRuntime>().await?;
    runtime.start(&ctx).await?;
    println!(
        "Handling messages as `{}` over {} (Ctrl-C to stop).",
        runtime.service,
        runtime.transport.name()
    );
    ctx.shutdown_token().cancelled().await;
    Ok(())
}

/// One row of `micro:patterns`.
pub(crate) fn pattern_rows(router: &crate::pipeline::MessageRouter) -> Vec<[String; 5]> {
    router
        .patterns
        .iter()
        .map(|registered| {
            [
                registered.kind.to_string(),
                registered.pattern.clone(),
                registered.handlers.to_owned(),
                registered.access(),
                registered.module.to_owned(),
            ]
        })
        .collect()
}

/// The `micro:patterns` table.
pub(crate) fn render_patterns(rows: &[[String; 5]]) -> String {
    let header = ["KIND", "PATTERN", "HANDLER", "ACCESS", "MODULE"].map(str::to_owned);
    let mut widths = header.clone().map(|h| h.len());
    for row in rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.len());
        }
    }
    let line = |row: &[String; 5]| {
        let mut out = String::new();
        for (i, (cell, width)) in row.iter().zip(widths).enumerate() {
            if i == row.len() - 1 {
                out.push_str(cell);
            } else {
                out.push_str(&format!("{cell:<width$}  "));
            }
        }
        out.trim_end().to_owned()
    };
    let mut out = line(&header);
    out.push('\n');
    for row in rows {
        out.push_str(&line(row));
        out.push('\n');
    }
    let count = rows.len();
    out.push_str(&format!(
        "\n{count} pattern{}\n",
        if count == 1 { "" } else { "s" }
    ));
    out
}

async fn patterns_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let runtime = ctx.get::<MicroRuntime>().await?;
    let router = runtime.router(&ctx).await?;
    if args.iter().any(|a| a == "--json") {
        let list: Vec<Value> = router
            .patterns
            .iter()
            .map(|registered| {
                json!({
                    "kind": registered.kind,
                    "pattern": registered.pattern,
                    "handler": registered.handlers,
                    "method": registered.meta.handler,
                    "access": registered.access(),
                    "module": registered.module,
                    "dedupe": registered.meta.dedupe,
                    "concurrency": registered.meta.concurrency,
                    "max_attempts": registered.meta.max_attempts,
                    "ordered": registered.meta.ordered,
                    "broadcast": registered.meta.broadcast,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&list)?);
    } else {
        print!("{}", render_patterns(&pattern_rows(&router)));
    }
    Ok(())
}

/// `<pattern> <json>` arguments.
fn pattern_and_json(args: &[String], command: &str) -> Result<(String, Value), BoxError> {
    let usage = || format!("usage: {command} <pattern> <json>");
    let pattern = args.first().ok_or_else(usage)?.clone();
    let payload = match args.get(1) {
        Some(text) => serde_json::from_str(text)
            .map_err(|error| format!("the payload is not valid JSON: {error}"))?,
        None => Value::Null,
    };
    if args.len() > 2 {
        return Err(usage().into());
    }
    Ok((pattern, payload))
}

async fn send_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let (pattern, payload) = pattern_and_json(&args, "micro:send")?;
    let client = ctx.get::<ServiceClient>().await?;
    match client.send::<_, Value>(&pattern, &payload).await {
        Ok(reply) => {
            println!("{}", serde_json::to_string_pretty(&reply)?);
            Ok(())
        }
        Err(MicroError::Remote(error)) => {
            println!("{}", serde_json::to_string_pretty(error.body())?);
            Err(format!("{pattern} answered {}", error.status()).into())
        }
        Err(error) => Err(error.into()),
    }
}

async fn emit_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let (pattern, payload) = pattern_and_json(&args, "micro:emit")?;
    let client = ctx.get::<ServiceClient>().await?;
    client.emit(&pattern, &payload).immediately().await?;
    println!("Published {pattern}.");
    Ok(())
}

async fn setup_command(ctx: AppContext, _args: Vec<String>) -> Result<(), BoxError> {
    let runtime = ctx.get::<MicroRuntime>().await?;
    let router = runtime.router(&ctx).await?;
    let subscriptions = subscriptions(&runtime, &router);
    let lines = runtime.transport.setup(&subscriptions).await?;
    let name = runtime.transport.name();
    if lines.is_empty() {
        println!("{name}: nothing to set up.");
    }
    for line in lines {
        println!("{name}: {line}");
    }
    Ok(())
}

/// One line of `micro:failed`.
pub(crate) fn failed_line(letter: &DeadLetter) -> String {
    format!(
        "{}  {}  attempts={}  {}  {}",
        letter.id,
        letter.pattern,
        letter.attempts,
        letter.failed_at.format("%Y-%m-%d %H:%M:%S"),
        letter.error
    )
}

async fn failed_command(ctx: AppContext, _args: Vec<String>) -> Result<(), BoxError> {
    let letters = ctx.get::<DeadLetters>().await?.list().await?;
    if letters.is_empty() {
        println!("No dead letters.");
    }
    for letter in &letters {
        println!("{}", failed_line(letter));
    }
    Ok(())
}

async fn retry_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let target = args.first().ok_or("usage: micro:retry <id|all>")?.clone();
    let dead_letters = ctx.get::<DeadLetters>().await?;
    if target == "all" {
        let (handled, failed) = dead_letters.retry_all().await?;
        println!(
            "Retried {}: {handled} handled, {failed} failed again.",
            handled + failed
        );
    } else if dead_letters.retry(&target).await? {
        println!("Handled {target}.");
    } else {
        println!("{target} failed again; it is back in the dead letters.");
    }
    Ok(())
}

async fn flush_command(ctx: AppContext, _args: Vec<String>) -> Result<(), BoxError> {
    let deleted = ctx.get::<DeadLetters>().await?.flush().await?;
    println!(
        "Deleted {deleted} dead letter{}.",
        if deleted == 1 { "" } else { "s" }
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_patterns_table_lines_up() {
        let rows = vec![
            [
                "message".to_owned(),
                "deals.get".to_owned(),
                "DealsHandlers".to_owned(),
                "can:read Deal from id".to_owned(),
                "DealsModule".to_owned(),
            ],
            [
                "event".to_owned(),
                "company.renamed".to_owned(),
                "CompanyNames".to_owned(),
                "-".to_owned(),
                "DealsModule".to_owned(),
            ],
        ];
        assert_eq!(
            render_patterns(&rows),
            "KIND     PATTERN          HANDLER        ACCESS                 MODULE\n\
             message  deals.get        DealsHandlers  can:read Deal from id  DealsModule\n\
             event    company.renamed  CompanyNames   -                      DealsModule\n\
             \n2 patterns\n"
        );
    }

    #[test]
    fn send_and_emit_take_a_pattern_and_json() {
        let (pattern, payload) =
            pattern_and_json(&["deals.get".into(), r#"{"id":1}"#.into()], "micro:send").unwrap();
        assert_eq!(pattern, "deals.get");
        assert_eq!(payload["id"], 1);
        assert!(pattern_and_json(&[], "micro:send").is_err());
        assert!(pattern_and_json(&["a".into(), "{".into()], "micro:send").is_err());
        let (_, empty) = pattern_and_json(&["a".into()], "micro:emit").unwrap();
        assert_eq!(empty, Value::Null);
    }
}
