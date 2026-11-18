//! CLI commands contributed by modules, and the built-in `check`, `help`,
//! `introspect`, `plugins` and `plugin:publish`.

use std::fmt;
use std::future::Future;
use std::sync::Arc;

use crate::{AppContext, BoxError, BoxFuture, GraphReport};

/// The function behind a [`CommandSpec`].
pub type CommandFn =
    Arc<dyn Fn(AppContext, Vec<String>) -> BoxFuture<'static, Result<(), BoxError>> + Send + Sync>;

/// A command a module registers with [`ModuleBuilder::command`](crate::ModuleBuilder::command),
/// run as `<binary> <name> [args...]` against a booted app.
#[derive(Clone)]
pub struct CommandSpec {
    /// The command name, e.g. `serve` or `queue:work`.
    pub name: &'static str,
    /// A one-line description shown by `help`.
    pub about: &'static str,
    /// Runs the command with the arguments after its name.
    pub run: CommandFn,
    /// The command speaks a protocol on standard output (an MCP server over
    /// stdio, for example), so nothing else may write there: the logger
    /// writes to standard error instead. See [`CommandSpec::stdio`].
    pub stdio: bool,
    /// The command does not need a booted app: it runs against the module
    /// graph, and builds only the providers it asks for. See
    /// [`CommandSpec::offline`].
    pub offline: bool,
}

impl CommandSpec {
    /// A command running `f(ctx, args)`.
    pub fn new<F, Fut>(name: &'static str, about: &'static str, f: F) -> Self
    where
        F: Fn(AppContext, Vec<String>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), BoxError>> + Send + 'static,
    {
        Self {
            name,
            about,
            run: Arc::new(move |ctx, args| Box::pin(f(ctx, args))),
            stdio: false,
            offline: false,
        }
    }

    /// Marks the command as owning standard output (it writes a protocol
    /// there). The app knows the command it runs from boot on
    /// ([`AppContext::command`]), so modules that would print to stdout,
    /// navree-log's console output first among them, use stderr instead.
    pub fn stdio(mut self) -> Self {
        self.stdio = true;
        self
    }

    /// Marks the command as one that needs no booted app. The runner does
    /// not build every singleton (no database connection is opened, no
    /// server started) and runs no lifecycle hook, before or after: the
    /// command gets the [`AppContext`] of the module graph and resolves what
    /// it needs with [`AppContext::get`], which builds a provider on first
    /// use. Meant for commands that read the app's configuration or graph,
    /// such as `config:show` and `config:check`.
    pub fn offline(mut self) -> Self {
        self.offline = true;
        self
    }
}

impl fmt::Debug for CommandSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandSpec")
            .field("name", &self.name)
            .field("about", &self.about)
            .field("stdio", &self.stdio)
            .field("offline", &self.offline)
            .finish_non_exhaustive()
    }
}

/// Built-in commands handled by [`AppBuilder::run_with_args`](crate::AppBuilder::run_with_args).
pub(crate) const BUILT_INS: [(&str, &str); 5] = [
    (
        "check [--graph]",
        "Validate the module graph without constructing providers",
    ),
    ("help", "List the available commands"),
    (
        "introspect --json",
        "Print the module graph and commands as JSON without constructing providers",
    ),
    (
        "plugins [--json]",
        "List the plugins and what each adds, without constructing providers",
    ),
    (
        "plugin:publish <plugin> [options]",
        "Copy a plugin's templates, translations, config or files into the app",
    ),
];

/// Built-in commands that give way to a module command of the same name.
pub(crate) const YIELDING: [&str; 2] = ["plugins", "plugin:publish"];

/// The names of the framework's own commands, sorted: the built-ins, the other
/// names [`AppBuilder::run_with_args`](crate::AppBuilder::run_with_args) takes
/// for them, and every command the framework's modules register. A module that
/// acts for a plugin which is not first-party may not register a command of
/// one of these names (the app's own module may, and runs in place of a
/// built-in that yields). Not public API; Navree's own crates and tests use
/// it.
pub const FRAMEWORK_COMMANDS: &[&str] = &[
    "--help",
    "-h",
    "access:explain",
    "access:grant",
    "access:role",
    "ai:prune",
    "audit:prune",
    "automations:catalog",
    "automations:list",
    "billing:events",
    "billing:plans",
    "billing:prune-events",
    "billing:report-usage",
    "billing:sync",
    "billing:webhook",
    "cache:clear",
    "check",
    "config:check",
    "config:publish",
    "config:schema",
    "config:show",
    "conformity:check",
    "db",
    "db:monitor",
    "db:seed",
    "directory:connections",
    "directory:prune",
    "directory:runs",
    "directory:sync",
    "directory:token",
    "documents:fonts",
    "documents:render",
    "domains:certificates",
    "domains:list",
    "domains:transfer",
    "domains:verify",
    "down",
    "flags:forget",
    "flags:kill",
    "flags:list",
    "flags:off",
    "flags:on",
    "flags:purge",
    "flags:revive",
    "flags:rollout",
    "fleet",
    "fleet:continue",
    "fleet:forget",
    "fleet:pause",
    "fleet:status",
    "fleet:terminate",
    "fleet:work",
    "graphql:schema",
    "help",
    "i18n:check",
    "i18n:missing",
    "i18n:publish",
    "integrations:check",
    "integrations:list",
    "integrations:mailboxes",
    "introspect",
    "key:generate",
    "lighthouse:prune",
    "lighthouse:test-alerts",
    "list",
    "logbook:clear",
    "mail:domains",
    "mail:suppressions",
    "mail:test",
    "mcp:serve",
    "media:regenerate",
    "micro:emit",
    "micro:failed",
    "micro:flush",
    "micro:listen",
    "micro:patterns",
    "micro:retry",
    "micro:send",
    "micro:setup",
    "migrate",
    "migrate:fresh",
    "migrate:install",
    "migrate:refresh",
    "migrate:reset",
    "migrate:rollback",
    "migrate:status",
    "model:prune",
    "model:show",
    "mongo:migrate",
    "mongo:migrate:fresh",
    "mongo:migrate:refresh",
    "mongo:migrate:reset",
    "mongo:migrate:rollback",
    "mongo:migrate:status",
    "native:coverage",
    "notify:release",
    "notify:vapid",
    "oauth:client",
    "oauth:client:revoke",
    "oauth:client:secret",
    "oauth:clients",
    "oauth:keys",
    "oauth:prune",
    "openapi:export",
    "plugin:publish",
    "plugins",
    "queue:batches",
    "queue:failed",
    "queue:flush",
    "queue:prune",
    "queue:retry",
    "queue:watchdog",
    "queue:work",
    "routes",
    "schedule:list",
    "schedule:run",
    "schedule:work",
    "schema:dump",
    "search:flush",
    "search:import",
    "search:settings",
    "search:status",
    "serve",
    "settings:inherit",
    "settings:list",
    "settings:set",
    "settings:show",
    "sheets:prune",
    "storage:sync",
    "time:zones",
    "ui:render",
    "up",
    "webhooks:endpoints",
    "webhooks:prune",
    "webhooks:retry",
    "webhooks:test",
    "workflows:cancel",
    "workflows:list",
    "workflows:prune",
    "workflows:retry",
    "workflows:show",
    "workflows:tick",
    "workspace:demo-create",
    "workspace:demo-reset",
    "workspace:migrate",
    "workspace:prune-invitations",
];

/// The built-in commands of an app with `report`'s modules, as (usage,
/// about): [`BUILT_INS`] without one that yields to a module command of its
/// name.
pub(crate) fn built_ins(
    report: &GraphReport,
) -> impl Iterator<Item = (&'static str, &'static str)> + '_ {
    BUILT_INS.into_iter().filter(move |&(usage, _)| {
        let name = usage.split_whitespace().next().unwrap_or(usage);
        !(YIELDING.contains(&name) && report.has_command(name))
    })
}

/// The text printed by `help`.
pub(crate) fn help_text(report: &GraphReport) -> String {
    let listed: Vec<(&str, &str)> = built_ins(report).collect();
    let commands: Vec<(&str, &str)> = report
        .modules
        .iter()
        .flat_map(|m| m.commands.iter().map(|c| (c.name, c.about)))
        .collect();
    let width = listed
        .iter()
        .chain(commands.iter())
        .map(|(name, _)| name.len())
        .max()
        .unwrap_or(0);
    let mut out = String::from("Usage: <command> [args...]\n\nBuilt-in commands:\n");
    for (name, about) in listed {
        out.push_str(&format!("  {name:width$}  {about}\n"));
    }
    if !commands.is_empty() {
        out.push_str("\nCommands:\n");
        for (name, about) in commands {
            out.push_str(&format!("  {name:width$}  {about}\n"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CommandInfo, ModuleReport};

    #[test]
    fn help_leaves_out_a_built_in_that_yields() {
        struct Mine;
        let own = GraphReport {
            modules: vec![ModuleReport {
                id: crate::ModuleId::of::<Mine>(),
                name: "Mine",
                global: false,
                imports: Vec::new(),
                providers: Vec::new(),
                exports: Vec::new(),
                commands: vec![CommandInfo {
                    name: "plugins",
                    about: "The app's own list",
                }],
            }],
        };
        let text = help_text(&own);
        assert!(!text.contains("plugins [--json]"), "{text}");
        assert!(
            text.lines().any(
                |l| l.trim_start().starts_with("plugins ") && l.ends_with("The app's own list")
            ),
            "{text}"
        );
        assert!(
            help_text(&GraphReport {
                modules: Vec::new()
            })
            .contains("plugins [--json]")
        );
        assert!(FRAMEWORK_COMMANDS.windows(2).all(|w| w[0] < w[1]));
        for (usage, _) in BUILT_INS {
            let name = usage.split_whitespace().next().unwrap_or(usage);
            assert!(FRAMEWORK_COMMANDS.binary_search(&name).is_ok(), "{name}");
        }
    }
}
