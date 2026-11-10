//! The tools: their definitions (name, description, input schema) and
//! [`Navigator::call`], which answers them. The MCP server
//! ([`crate::server`]) only adapts this to the protocol.

use std::path::PathBuf;
use std::sync::Arc;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::app::{AppError, AppProbe, ProcessRunner, Runner};
use crate::database::{self, DatabaseError, Driver, QueryResult, Target};
use crate::docs::{DocsLibrary, DocsSettings, HttpFetcher, SITE, navree_home};
use crate::lighthouse;
use crate::logs::{self, LogFile, LogFilter, NO_LOG_FILE};
use crate::project::{NotAnApp, Project};
use crate::ui::{self, Catalog};

/// How Navigator was started (`navree mcp` and its options).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NavigatorOptions {
    /// The app's directory (`--path`; default: the current directory).
    pub path: PathBuf,
    /// Never run `cargo build` (`--no-build`).
    pub no_build: bool,
    /// Allow databases that aren't on this machine (`--allow-remote-database`).
    pub allow_remote_database: bool,
    /// Never download docs (`--docs-offline`).
    pub docs_offline: bool,
}

/// One tool's definition.
#[derive(Clone, Debug)]
pub struct ToolDef {
    /// The name agents call.
    pub name: &'static str,
    /// A short human title.
    pub title: &'static str,
    /// What it answers, for the model.
    pub description: &'static str,
    /// The JSON Schema of its arguments.
    pub input_schema: Value,
    /// Whether it may reach outside this machine (`search_docs` and
    /// `read_doc`, which may download the docs once).
    pub open_world: bool,
}

/// A tool's answer.
#[derive(Clone, Debug, PartialEq)]
pub enum ToolOutput {
    /// Data, sent as JSON text (and as structured content when an object).
    Json(Value),
    /// A document, sent as Markdown text.
    Markdown(String),
}

/// Why a tool could not answer, said to the model.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ToolError(pub String);

impl ToolError {
    fn new(message: impl Into<String>) -> Self {
        ToolError(message.into())
    }
}

impl From<AppError> for ToolError {
    fn from(error: AppError) -> Self {
        ToolError(error.to_string())
    }
}

impl From<DatabaseError> for ToolError {
    fn from(error: DatabaseError) -> Self {
        ToolError(error.to_string())
    }
}

fn object(properties: Value) -> Value {
    json!({ "type": "object", "properties": properties, "additionalProperties": false })
}

/// The fourteen tools every app gets, in the order agents see them. An app
/// that includes Lighthouse also gets [`lighthouse::tool_defs`]; see
/// [`Navigator::tools`].
pub fn tools() -> Vec<ToolDef> {
    let none = || object(json!({}));
    let string = |description: &str| json!({ "type": "string", "description": description });
    vec![
        ToolDef {
            name: "app_info",
            title: "App info",
            description: "The first call in a new project: the Navree version (from Cargo.lock), the Rust \
                toolchain, the facade features the app's Cargo.toml turns on, the modules in boot order, the \
                database driver and the configuration files that exist.",
            input_schema: none(),
            open_world: false,
        },
        ToolDef {
            name: "list_routes",
            title: "List routes",
            description: "Every route of the booted app (what `navree routes --json` prints): method, path, \
                name, access rule (`public`, `auth`, `can:read Deal from id`), module and handler. A route \
                declared with `.page()` has `page: true`: its errors are pages, and `render_page` can draw it. \
                Check it before adding a route. Builds the app first; a build error is returned as is.",
            input_schema: object(json!({
                "path": string("Only routes whose path starts with this prefix (`/api/v1/deals`)."),
                "method": string("Only routes with this HTTP method (`GET`)."),
                "module": string("Only routes of this module (`DealsModule`)."),
                "pages_only": { "type": "boolean",
                                "description": "Only the routes declared with `.page()`." },
            })),
            open_world: false,
        },
        ToolDef {
            name: "module_graph",
            title: "Module graph",
            description: "The module graph `navree check --graph` prints, as data: each module (dependency \
                order, the implicit root `App` last), what it imports and exports, its providers (controllers \
                included) and commands. With `module`, that one module. Built without constructing anything.",
            input_schema: object(json!({
                "module": string("One module's name (`DealsModule`)."),
            })),
            open_world: false,
        },
        ToolDef {
            name: "list_providers",
            title: "List providers",
            description: "Providers across the app: type, scope (`singleton`, `request`, `transient`), owning \
                module, whether the module exports it, and its dependencies. Use it to wire a new service: \
                where a type lives and which module to import.",
            input_schema: object(json!({
                "module": string("Only the providers of this module."),
            })),
            open_world: false,
        },
        ToolDef {
            name: "list_commands",
            title: "List commands",
            description: "The app's commands with their one-line help, as `navree list` shows them (built-ins, \
                then `migrate`, `queue:work`, the app's own `crm:*` ...).",
            input_schema: none(),
            open_world: false,
        },
        ToolDef {
            name: "describe_schema",
            title: "Describe schema",
            description: "The database as it is: tables, columns (type, nullability, default, primary key), \
                indexes and foreign keys, plus the migrations that have run and those that haven't. With \
                `table`, one table.",
            input_schema: object(json!({
                "table": string("One table's name (`deals`)."),
            })),
            open_world: false,
        },
        ToolDef {
            name: "query_database",
            title: "Query database",
            description: "Runs one read-only SQL statement (SELECT, WITH, EXPLAIN or SHOW) against the \
                development database and returns at most 100 rows: columns, rows, truncated, duration_ms. \
                Anything else is refused before it is sent; it runs read-only with a 5 second timeout. \
                Columns whose names contain password, secret, token, api_key or private_key come back as \
                [redacted].",
            input_schema: json!({
                "type": "object",
                "properties": { "sql": string("One read-only statement.") },
                "required": ["sql"],
                "additionalProperties": false,
            }),
            open_world: false,
        },
        ToolDef {
            name: "read_logs",
            title: "Read logs",
            description: "Recent lines of the log file the app writes (`log.file`), newest last, JSON and pretty \
                formats alike. Filter by minimum `level` (`warn` returns warnings and errors), `target` prefix \
                (`navree::job`), `request_id`, `since` (RFC 3339 or `15m`, `2h`, `1d`) and `limit` (default \
                100, at most 1000).",
            input_schema: object(json!({
                "level": { "type": "string", "enum": ["error", "warn", "info", "debug", "trace"],
                           "description": "The lowest level to return." },
                "target": string("A target or target prefix (`navree::job`)."),
                "request_id": string("Only the lines of this request."),
                "since": string("An RFC 3339 time, or a duration back from now: `30s`, `15m`, `2h`, `1d`."),
                "limit": { "type": "integer", "minimum": 1, "maximum": 1000,
                           "description": "How many lines (default 100)." },
            })),
            open_world: false,
        },
        ToolDef {
            name: "last_error",
            title: "Last error",
            description: "The most recent `error` line of the app's log file, with its request id, the \
                request's method, route and status, and every other line of the same request. \"Why did that \
                request fail?\" in one call.",
            input_schema: none(),
            open_world: false,
        },
        ToolDef {
            name: "read_config",
            title: "Read configuration",
            description: "The merged configuration the app sees (`.env`, `config/app.toml`, `config/<section>.toml`, \
                `config/env/<env>.toml`, `APP_*` variables and dock.toml's settings), from the app's own \
                `config:show --json`: secrets masked by the app's sections (`Secret` fields, `#[config(secret)]`) \
                and by name (password, secret, token, key, authorization, cookie, dsn, credential...), and \
                passwords in URLs. Builds the app first; when it doesn't build, the files are read and masked by \
                name only. With `key`, one section or value (`queue`, `database.url`); with `defaults`, every \
                default too.",
            input_schema: object(json!({
                "key": string("A dotted key or section (`database`, `log.file`)."),
                "defaults": { "type": "boolean",
                              "description": "Also the defaults no source overrides (needs the app to build)." },
            })),
            open_world: false,
        },
        ToolDef {
            name: "search_docs",
            title: "Search docs",
            description: "Searches the Navree docs of the version in the app's Cargo.lock and returns the best \
                sections with an excerpt and the slug for `read_doc`.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": string("What to look for, in words (`unique validation rule`)."),
                    "limit": { "type": "integer", "minimum": 1, "maximum": 20,
                               "description": "How many sections (default 8)." },
                },
                "required": ["query"],
                "additionalProperties": false,
            }),
            open_world: true,
        },
        ToolDef {
            name: "read_doc",
            title: "Read doc",
            description: "One docs page as Markdown, from the docs of the app's Navree version: a slug from \
                `search_docs` (`basics/validation`), a `/docs/...` path or a navree.com URL. With `section`, \
                only that section (its heading or anchor).",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "slug": string("The page: `ai/navigator`, `/docs/ai/navigator/` or its URL."),
                    "section": string("One section's heading or anchor (`options`)."),
                },
                "required": ["slug"],
                "additionalProperties": false,
            }),
            open_world: true,
        },
        ToolDef {
            name: "list_components",
            title: "List components",
            description: "The Navree UI components, from the catalog built into Navigator (of its Navree \
                version, named in `version`): id, name, group, a one-line summary, the Rust types and `docs`, \
                the slug `read_doc` takes (`components/button`). Filter by `query` (words in the id, name, \
                summary or types) or `group`. Call it before building a screen, so the page uses a component \
                that exists.",
            input_schema: object(json!({
                "query": string("Words to look for (`date picker`, `ButtonVariant`)."),
                "group": string("Only the components of this group (`inputs`); the answer lists the groups."),
            })),
            open_world: false,
        },
        ToolDef {
            name: "render_page",
            title: "Render page",
            description: "What one page of the app draws, without a browser: the app draws `path` itself \
                (a GET as a guest, through its own router, changing nothing) and Navigator answers with an \
                outline: status, title, headings, links, buttons, forms with their fields, and the Navree UI \
                components the page uses. A route that redirects (to sign-in, say) answers with the redirect. \
                `list_routes` with `pages_only` lists the pages. Needs the app's `ui:render` command, which \
                Navree's `UiModule` adds. Builds the app first; a build error is returned as is.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": string("A path of the app, with its query if it has one (`/deals`, `/deals?page=2`)."),
                },
                "required": ["path"],
                "additionalProperties": false,
            }),
            open_world: false,
        },
    ]
}

/// Answers the tools for one app.
pub struct Navigator {
    options: NavigatorOptions,
    project: Result<Project, String>,
    probe: Option<Arc<AppProbe>>,
    docs_settings: DocsSettings,
    docs: tokio::sync::OnceCell<Arc<DocsLibrary>>,
    lighthouse: bool,
}

impl std::fmt::Debug for Navigator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Navigator")
            .field("options", &self.options)
            .field("project", &self.project)
            .field("lighthouse", &self.lighthouse)
            .finish_non_exhaustive()
    }
}

/// `limit` and friends: optional arguments of the tools.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RouteArgs {
    path: Option<String>,
    method: Option<String>,
    module: Option<String>,
    pages_only: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ComponentArgs {
    query: Option<String>,
    group: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RenderArgs {
    path: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModuleArgs {
    module: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct TableArgs {
    table: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SqlArgs {
    sql: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigArgs {
    key: Option<String>,
    defaults: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchArgs {
    query: String,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DocArgs {
    slug: String,
    section: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct NoArgs {}

fn parse<T: DeserializeOwned>(args: Value) -> Result<T, ToolError> {
    let args = if args.is_null() { json!({}) } else { args };
    serde_json::from_value(args).map_err(|e| ToolError::new(format!("invalid arguments: {e}")))
}

impl Navigator {
    /// Navigator for the app at `options.path`, with real processes and
    /// downloads.
    pub fn new(options: NavigatorOptions) -> Navigator {
        let fetcher: Arc<dyn crate::docs::DocsFetcher> = match HttpFetcher::new() {
            Ok(fetcher) => Arc::new(fetcher),
            Err(message) => Arc::new(FailingFetcher(message)),
        };
        let docs = DocsSettings {
            home: navree_home(),
            base_url: std::env::var("NAVREE_DOCS_URL").unwrap_or_else(|_| SITE.to_owned()),
            offline: options.docs_offline,
            fetcher,
        };
        Navigator::with(options, Arc::new(ProcessRunner), docs)
    }

    /// Navigator with another process runner and docs settings (tests).
    pub fn with(
        options: NavigatorOptions,
        runner: Arc<dyn Runner>,
        docs: DocsSettings,
    ) -> Navigator {
        let project = Project::find(&options.path).map_err(|e: NotAnApp| e.to_string());
        let lighthouse = project
            .as_ref()
            .is_ok_and(|project| lighthouse::included(&project.root));
        let probe = project.as_ref().ok().map(|project| {
            Arc::new(AppProbe::new(
                project.clone(),
                runner,
                options.no_build,
                project.dock_env(),
            ))
        });
        Navigator {
            options,
            project,
            probe,
            docs_settings: docs,
            docs: tokio::sync::OnceCell::new(),
            lighthouse,
        }
    }

    /// Whether the app includes Lighthouse (a file under `src/` adds
    /// `LighthouseModule`), which adds Lighthouse's five read tools.
    pub fn includes_lighthouse(&self) -> bool {
        self.lighthouse
    }

    /// The tools this app gets: [`tools`], then Lighthouse's when the app
    /// includes it.
    pub fn tools(&self) -> Vec<ToolDef> {
        let mut all = tools();
        if self.lighthouse {
            all.extend(lighthouse::tool_defs());
        }
        all
    }

    /// The app, or why there is none.
    pub fn project(&self) -> Result<&Project, ToolError> {
        self.project.as_ref().map_err(|e| ToolError::new(e.clone()))
    }

    /// Answers tool `name` with `args` (a JSON object, or null).
    pub async fn call(&self, name: &str, args: Value) -> Result<ToolOutput, ToolError> {
        match name {
            "app_info" => {
                parse::<NoArgs>(args)?;
                self.app_info().await
            }
            "list_routes" => self.list_routes(parse(args)?).await,
            "module_graph" => self.module_graph(parse(args)?).await,
            "list_providers" => self.list_providers(parse(args)?).await,
            "list_commands" => {
                parse::<NoArgs>(args)?;
                self.list_commands().await
            }
            "describe_schema" => self.describe_schema(parse(args)?).await,
            "query_database" => self.query_database(parse(args)?).await,
            "read_logs" => self.read_logs(parse(args)?),
            "last_error" => {
                parse::<NoArgs>(args)?;
                self.last_error()
            }
            "read_config" => self.read_config(parse(args)?).await,
            "search_docs" => self.search_docs(parse(args)?).await,
            "read_doc" => self.read_doc(parse(args)?).await,
            "list_components" => self.list_components(parse(args)?),
            "render_page" => self.render_page(parse(args)?).await,
            name if lighthouse::is_lighthouse_tool(name) => self.lighthouse(name, args).await,
            other => Err(ToolError::new(format!(
                "unknown tool `{other}`; the tools are {}",
                self.tools()
                    .iter()
                    .map(|t| t.name)
                    .collect::<Vec<_>>()
                    .join(", ")
            ))),
        }
    }

    /// Runs a blocking probe call off the async workers.
    async fn probe<T: Send + 'static>(
        &self,
        f: impl FnOnce(&AppProbe) -> Result<T, AppError> + Send + 'static,
    ) -> Result<T, ToolError> {
        let probe = self
            .probe
            .clone()
            .ok_or_else(|| ToolError::new(self.project().unwrap_err().0))?;
        tokio::task::spawn_blocking(move || f(&probe))
            .await
            .map_err(|e| ToolError::new(format!("the app probe stopped: {e}")))?
            .map_err(ToolError::from)
    }

    fn config(&self) -> Result<navree_config::Config, ToolError> {
        self.project()?
            .config()
            .map_err(|e| ToolError::new(format!("the app's configuration doesn't load: {e}")))
    }

    fn database(&self) -> Result<Target, ToolError> {
        let project = self.project()?;
        let url = self
            .config()?
            .get_string("database.url")
            .unwrap_or_default();
        Ok(Target::new(
            &url,
            &project.root,
            self.options.allow_remote_database,
        )?)
    }

    /// Lighthouse's database: `lighthouse.database_url`, else `database.url`.
    fn lighthouse_database(&self) -> Result<LighthouseDb, ToolError> {
        let project = self.project()?;
        let config = self.config()?;
        let own = config
            .get_string("lighthouse.database_url")
            .filter(|url| !url.trim().is_empty());
        let url = own
            .clone()
            .or_else(|| config.get_string("database.url"))
            .unwrap_or_default();
        let target = Target::new(&url, &project.root, self.options.allow_remote_database).map_err(
            |error| {
                let message = error.to_string();
                match (&error, own.is_some()) {
                    (DatabaseError::Remote { .. }, true) => ToolError::new(message.replacen(
                        "database.url",
                        "lighthouse.database_url",
                        1,
                    )),
                    _ => ToolError::new(message),
                }
            },
        )?;
        Ok(LighthouseDb { target })
    }

    async fn lighthouse(&self, name: &str, args: Value) -> Result<ToolOutput, ToolError> {
        if !self.lighthouse {
            return Err(ToolError::new(lighthouse::NOT_INCLUDED));
        }
        let now = chrono::Utc::now();
        let answer = match name {
            "lighthouse_list_issues" => {
                let args = parse(args)?;
                lighthouse::list_issues(&self.lighthouse_database()?, args).await?
            }
            "lighthouse_get_issue" => {
                let args = parse(args)?;
                lighthouse::get_issue(&self.lighthouse_database()?, args).await?
            }
            "lighthouse_slow_routes" => {
                let args = parse(args)?;
                lighthouse::slow_routes(&self.lighthouse_database()?, args, now).await?
            }
            "lighthouse_slow_queries" => {
                let args = parse(args)?;
                lighthouse::slow_queries(&self.lighthouse_database()?, args, now).await?
            }
            _ => {
                let args = parse(args)?;
                lighthouse::failed_jobs(&self.lighthouse_database()?, args, now).await?
            }
        };
        Ok(ToolOutput::Json(answer))
    }

    async fn app_info(&self) -> Result<ToolOutput, ToolError> {
        let project = self.project()?.clone();
        let navree = project.navree_version();
        let config = self.config().ok();
        let database = config
            .as_ref()
            .and_then(|c| c.get_string("database.url"))
            .filter(|url| !url.trim().is_empty());
        let rust = self
            .probe(|probe| Ok(probe.rustc_version()))
            .await
            .ok()
            .flatten();
        let modules = match self.probe(|probe| probe.introspect()).await {
            Ok(graph) => json!(module_names(&graph)),
            Err(error) => json!({ "error": error.0 }),
        };
        Ok(ToolOutput::Json(json!({
            "package": project.package,
            "path": project.root,
            "navree": navree,
            "rust": rust,
            "features": project.declared_features(),
            "modules": modules,
            "database": database.map(|url| json!({
                "driver": Driver::of(&url).map(Driver::name).unwrap_or("unsupported"),
                "url": database::redact_url(&url),
            })),
            "config_files": project.config_files(),
            "navigator": env!("CARGO_PKG_VERSION"),
        })))
    }

    async fn list_routes(&self, args: RouteArgs) -> Result<ToolOutput, ToolError> {
        let routes = self.probe(|probe| probe.routes()).await?;
        // A route table that says nothing about pages can't be filtered by it.
        let pages_only = args.pages_only == Some(true);
        let table = routes.as_array().map_or(&[][..], Vec::as_slice);
        if pages_only && !table.is_empty() && !table.iter().any(|r| r["page"].is_boolean()) {
            return Err(ToolError::new(
                "this app's route table (`routes --json`) doesn't say which routes are pages: it has no \
                 `page` field, so `pages_only` can't tell. Update Navree to a version that has it, or read \
                 the routes without `pages_only`.",
            ));
        }
        let wanted = |route: &Value| {
            args.path.as_deref().is_none_or(|p| {
                route["path"]
                    .as_str()
                    .is_some_and(|path| path.starts_with(p))
            }) && args.method.as_deref().is_none_or(|m| {
                route["method"]
                    .as_str()
                    .is_some_and(|method| method.eq_ignore_ascii_case(m))
            }) && args.module.as_deref().is_none_or(|m| route["module"] == m)
                && (!pages_only || route["page"] == true)
        };
        let list: Vec<Value> = routes
            .as_array()
            .into_iter()
            .flatten()
            .filter(|r| wanted(r))
            .cloned()
            .collect();
        Ok(ToolOutput::Json(Value::Array(list)))
    }

    fn list_components(&self, args: ComponentArgs) -> Result<ToolOutput, ToolError> {
        let app_version = self.project.as_ref().ok().and_then(Project::navree_version);
        ui::components_answer(
            Catalog::embedded(),
            args.query.as_deref(),
            args.group.as_deref(),
            app_version.as_deref(),
        )
        .map(ToolOutput::Json)
        .map_err(ToolError::new)
    }

    async fn render_page(&self, args: RenderArgs) -> Result<ToolOutput, ToolError> {
        let path = ui::check_path(&args.path).map_err(ToolError::new)?;
        let asked = path.clone();
        let printed = self
            .probe(move |probe| probe.render(&asked))
            .await
            .map_err(|error| {
                if ui::lacks_render_command(&error.0) {
                    ToolError::new(ui::NO_RENDER_COMMAND)
                } else {
                    error
                }
            })?;
        Ok(ToolOutput::Json(ui::render_answer(
            &path,
            &printed,
            Catalog::embedded(),
        )))
    }

    async fn graph(&self) -> Result<Value, ToolError> {
        self.probe(|probe| probe.introspect()).await
    }

    async fn module_graph(&self, args: ModuleArgs) -> Result<ToolOutput, ToolError> {
        let graph = self.graph().await?;
        let modules = graph["modules"].as_array().cloned().unwrap_or_default();
        match args.module {
            None => Ok(ToolOutput::Json(json!({ "modules": modules }))),
            Some(name) => modules
                .iter()
                .find(|m| m["name"] == name.as_str())
                .cloned()
                .map(ToolOutput::Json)
                .ok_or_else(|| {
                    ToolError::new(format!(
                        "no module named {name}; the modules are {}",
                        module_names(&graph).join(", ")
                    ))
                }),
        }
    }

    async fn list_providers(&self, args: ModuleArgs) -> Result<ToolOutput, ToolError> {
        let graph = self.graph().await?;
        let mut providers = Vec::new();
        for module in graph["modules"].as_array().into_iter().flatten() {
            if args.module.as_deref().is_some_and(|m| module["name"] != m) {
                continue;
            }
            for provider in module["providers"].as_array().into_iter().flatten() {
                let mut provider = provider.clone();
                provider["module"] = module["name"].clone();
                providers.push(provider);
            }
        }
        Ok(ToolOutput::Json(Value::Array(providers)))
    }

    async fn list_commands(&self) -> Result<ToolOutput, ToolError> {
        let graph = self.graph().await?;
        Ok(ToolOutput::Json(graph["commands"].clone()))
    }

    async fn describe_schema(&self, args: TableArgs) -> Result<ToolOutput, ToolError> {
        let target = self.database()?;
        let schema = database::describe_schema(&target, args.table.as_deref()).await?;
        if let Some(table) = &args.table
            && schema.tables.is_empty()
        {
            return Err(ToolError::new(format!(
                "no table named {table} in the database ({})",
                target.redacted_url()
            )));
        }
        let applied: Vec<String> = schema
            .applied_migrations
            .iter()
            .map(|m| m.label())
            .collect();
        let pending = match self.probe(|probe| probe.migration_status()).await {
            Ok(status) => json!(
                status
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|m| m["applied"] == false)
                    .map(|m| format!(
                        "{}/{}",
                        m["module"].as_str().unwrap_or_default(),
                        m["name"].as_str().unwrap_or_default()
                    ))
                    .collect::<Vec<_>>()
            ),
            Err(error) => json!({ "unknown": error.0.lines().next().unwrap_or_default() }),
        };
        let mut answer = serde_json::to_value(&schema).unwrap_or_default();
        answer["driver"] = json!(target.driver.name());
        answer["migrations"] = json!({ "applied": applied, "pending": pending });
        if let Some(object) = answer.as_object_mut() {
            object.remove("applied_migrations");
        }
        Ok(ToolOutput::Json(answer))
    }

    async fn query_database(&self, args: SqlArgs) -> Result<ToolOutput, ToolError> {
        let target = self.database()?;
        let result = database::query_database(&target, &args.sql).await?;
        Ok(ToolOutput::Json(
            serde_json::to_value(result).unwrap_or_default(),
        ))
    }

    fn log_files(&self) -> Result<Vec<PathBuf>, ToolError> {
        let project = self.project()?;
        let config = self.config()?;
        let file = LogFile::from_config(&config).ok_or_else(|| ToolError::new(NO_LOG_FILE))?;
        let files = file.files(&project.root);
        if files.is_empty() {
            return Err(ToolError::new(format!(
                "The app's log file ({}) doesn't exist yet: start the app (`navree dev`) and make a request.",
                file.path.display()
            )));
        }
        Ok(files)
    }

    fn read_logs(&self, filter: LogFilter) -> Result<ToolOutput, ToolError> {
        let files = self.log_files()?;
        let lines = logs::read_logs(&files, &filter).map_err(ToolError::new)?;
        Ok(ToolOutput::Json(
            json!({ "file": files[0], "lines": lines }),
        ))
    }

    fn last_error(&self) -> Result<ToolOutput, ToolError> {
        let files = self.log_files()?;
        match logs::last_error(&files) {
            Some(last) => Ok(ToolOutput::Json(
                serde_json::to_value(last).unwrap_or_default(),
            )),
            None => Ok(ToolOutput::Json(json!({
                "error": null,
                "message": "No error lines in the recent log.",
                "file": files[0],
            }))),
        }
    }

    /// The app's own `config:show --json`, which knows its sections and so
    /// masks every secret they declare; the files, masked by key name, when
    /// the app can't be asked (it doesn't build, `--no-build` before a build,
    /// a Navree without the command).
    async fn read_config(&self, args: ConfigArgs) -> Result<ToolOutput, ToolError> {
        let defaults = args.defaults == Some(true);
        let tree = match self.probe(move |probe| probe.config_show(defaults)).await {
            Ok(tree) => tree,
            Err(_) => self.config()?.redacted(),
        };
        let Some(key) = args.key.filter(|k| !k.trim().is_empty()) else {
            return Ok(ToolOutput::Json(tree));
        };
        let mut node = &tree;
        for segment in key.split('.') {
            node = node.get(segment).ok_or_else(|| {
                ToolError::new(format!(
                    "the configuration has no `{key}`; its sections are {}",
                    tree.as_object()
                        .map(|o| o.keys().cloned().collect::<Vec<_>>().join(", "))
                        .unwrap_or_default()
                ))
            })?;
        }
        Ok(ToolOutput::Json(node.clone()))
    }

    async fn library(&self) -> Arc<DocsLibrary> {
        self.docs
            .get_or_init(|| async {
                let version = self.project.as_ref().ok().and_then(Project::navree_version);
                Arc::new(DocsLibrary::load(version.as_deref(), &self.docs_settings).await)
            })
            .await
            .clone()
    }

    async fn search_docs(&self, args: SearchArgs) -> Result<ToolOutput, ToolError> {
        let library = self.library().await;
        let limit = args.limit.unwrap_or(8).clamp(1, 20);
        let results = library.search(&args.query, limit);
        let mut answer = json!({
            "version": library.bundle.version,
            "source": library.source,
            "results": results,
        });
        if let Some(note) = &library.note {
            answer["note"] = json!(note);
        }
        Ok(ToolOutput::Json(answer))
    }

    async fn read_doc(&self, args: DocArgs) -> Result<ToolOutput, ToolError> {
        let library = self.library().await;
        let Some(page) = library.bundle.page(&args.slug) else {
            let similar = library.bundle.similar(&args.slug);
            return Err(ToolError::new(if similar.is_empty() {
                format!("no docs page `{}`; use search_docs to find one", args.slug)
            } else {
                format!(
                    "no docs page `{}`; did you mean {}?",
                    args.slug,
                    similar.join(", ")
                )
            }));
        };
        let markdown = page.markdown(args.section.as_deref()).ok_or_else(|| {
            ToolError::new(format!(
                "the page {} has no section `{}`; its sections are {}",
                page.slug,
                args.section.as_deref().unwrap_or_default(),
                page.sections
                    .iter()
                    .filter(|s| !s.heading.is_empty())
                    .map(|s| s.anchor.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })?;
        let markdown = match &library.note {
            Some(note) => format!("> {note}\n\n{markdown}"),
            None => markdown,
        };
        Ok(ToolOutput::Markdown(markdown))
    }
}

/// The module names of an `introspect` answer, root excluded.
fn module_names(graph: &Value) -> Vec<String> {
    graph["modules"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| m["root"] != true)
        .filter_map(|m| m["name"].as_str().map(str::to_owned))
        .collect()
}

/// Lighthouse's database, read like `query_database` reads the app's.
struct LighthouseDb {
    target: Target,
}

impl lighthouse::Reader for LighthouseDb {
    fn driver(&self) -> Driver {
        self.target.driver
    }

    async fn read(&self, sql: String) -> Result<QueryResult, ToolError> {
        database::query_database(&self.target, &sql)
            .await
            .map_err(|error| lighthouse::explain(error.to_string(), &self.target.redacted_url()))
    }
}

/// A fetcher that always fails (no HTTP client could be built).
struct FailingFetcher(String);

impl crate::docs::DocsFetcher for FailingFetcher {
    fn fetch(
        &self,
        _url: &str,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<Option<String>, String>> + Send + '_>> {
        let message = self.0.clone();
        Box::pin(async move { Err(message) })
    }
}
