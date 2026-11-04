//! CLI commands contributed by modules, and the built-in `check`, `help`
//! and `introspect`.

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
pub(crate) const BUILT_INS: [(&str, &str); 3] = [
    (
        "check [--graph]",
        "Validate the module graph without constructing providers",
    ),
    ("help", "List the available commands"),
    (
        "introspect --json",
        "Print the module graph and commands as JSON without constructing providers",
    ),
];

/// The text printed by `help`.
pub(crate) fn help_text(report: &GraphReport) -> String {
    let commands: Vec<(&str, &str)> = report
        .modules
        .iter()
        .flat_map(|m| m.commands.iter().map(|c| (c.name, c.about)))
        .collect();
    let width = BUILT_INS
        .iter()
        .chain(commands.iter())
        .map(|(name, _)| name.len())
        .max()
        .unwrap_or(0);
    let mut out = String::from("Usage: <command> [args...]\n\nBuilt-in commands:\n");
    for (name, about) in BUILT_INS {
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
