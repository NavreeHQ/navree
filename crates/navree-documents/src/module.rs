//! [`DocumentsModule`] and [`ModuleBuilderDocumentsExt`].

use std::collections::HashMap;
use std::sync::Arc;

use bytes::Bytes;
use navree_config::{ConfigSection, ModuleBuilderConfigExt};
use navree_contracts::{Clock, Dispatcher, ModuleBuilderJobsExt};
use navree_http::ModuleBuilderHttpExt;
use navree_kernel::__private::{imports, is_app_module, is_community_plugin};
use navree_kernel::{
    AppContext, BoxError, BoxFuture, ConfiguredModule, Contributed, Factory, Lifecycle, Module,
    ModuleBuilder, ModuleId, ModuleInfo, ProviderKey, Resolver,
};
use navree_storage::Storage;

use crate::commands::commands;
use crate::error::map_documents_error;
use crate::templates::normalize;
use crate::{Documents, DocumentsError, DocumentsOptions, RenderDocument, Templates};

/// Documents: PDF from HTML templates (native or Chrome), DOCX, merge and
/// stamps.
///
/// Global; provides and exports [`Documents`] and [`DocumentsOptions`],
/// and contributes:
///
/// - the job `documents.render` ([`RenderDocument`]), which
///   [`Documents::queue`] dispatches;
/// - the commands `documents:render` and `documents:fonts`;
/// - the mapping of [`DocumentsError`]s returned from handlers to error
///   responses (500, or 503 when an engine is unavailable or timed out);
/// - closing Chrome when the app stops.
///
/// The app must include `ConfigModule` (for
/// [`from_config`](DocumentsModule::from_config) and the plain module).
/// With `StorageModule` in the app, documents can be saved and queued to
/// its disks; with `QueueModule`, queued renders run on a worker; with a
/// `dyn Clock`, the native engine's dates (the PDF's creation date, a
/// footer's `date`) follow it. Nothing starts at boot: fonts load and Chrome starts on the
/// first render that needs them.
///
/// ```no_run
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_documents::DocumentsModule;
/// use navree_kernel::Navree;
/// use navree_storage::StorageModule;
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// Navree::app()
///     .module(ConfigModule::for_root(ConfigSources::defaults()))
///     .module(StorageModule::from_config())
///     .module(DocumentsModule::from_config()) // section `documents`
///     .run()
/// # }
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct DocumentsModule;

impl DocumentsModule {
    /// The module with explicit options (configure it once, in the app
    /// root). They are checked at boot as the `documents` section is: a
    /// value it refuses fails the boot.
    pub fn for_root(options: DocumentsOptions) -> impl Module {
        ConfiguredModule::<DocumentsModule, _>::for_root(options, register)
    }

    /// The module configured from the `documents` section of `Config` at
    /// boot.
    pub fn from_config() -> impl Module {
        ConfiguredModule::<DocumentsModule, DocumentsOptions>::from_config(register)
    }
}

impl Module for DocumentsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

/// Template files a module embeds (see
/// [`ModuleBuilderDocumentsExt::document_files`]).
#[derive(Clone, Debug)]
pub(crate) struct DocumentFiles(pub(crate) Vec<(String, Bytes)>);

/// A document file of a module of the app or of the framework that a module
/// of a plugin it imports embedded again, while the plugin's version is the one
/// the app renders. A file at that path in a template folder, or a module of
/// the app that embeds the path after the plugin's module, makes the app's
/// version the one rendered instead, and the replacement is only said at
/// `info`.
struct Takeover {
    /// The file's path.
    path: String,
    /// The module whose file was replaced.
    first: &'static str,
    /// The module of the plugin that replaced it.
    second: &'static str,
    /// The plugin's name.
    plugin: &'static str,
}

/// Checks the files the modules embed, in dependency order, and fails with
/// every error, one per line. Only a module of a plugin the kernel does not
/// find first-party is held to a rule, and the only thing an app of 1.2 can see
/// is the warning for a path two of its modules embed with different contents
/// (the later module's file is still the one used).
///
/// - A module of such a plugin embeds a path under its namespace, or exactly
///   the path of a module it imports.
/// - Two modules that embed one path with different contents: a warning, or an
///   error when either module belongs to such a plugin. Equal contents say
///   nothing, and one module embedding a path twice replaces silently.
/// - A module that imports the other and replaces its path is said at `info`,
///   unless it belongs to such a plugin: then a warning when the file is
///   another such plugin's, and otherwise an error while the plugin's version is
///   the one rendered. It is not when `templates` has a file at that path, or
///   when a module of the app embeds the path after it, with the same contents
///   or others.
fn check_document_files(
    ctx: &AppContext,
    contributions: &[Contributed<DocumentFiles>],
    templates: &Templates,
) -> Result<(), String> {
    // Who embeds each path, for a plugin that replaces what it imports.
    let mut embedded_by: HashMap<&str, Vec<ModuleId>> = HashMap::new();
    for contributed in contributions {
        for (path, _) in &contributed.value.0 {
            embedded_by
                .entry(path.as_str())
                .or_default()
                .push(contributed.module.id);
        }
    }
    let mut owners: HashMap<&str, (&ModuleInfo, &Bytes)> = HashMap::new();
    let mut errors = Vec::new();
    let mut takeovers = Vec::new();
    for contributed in contributions {
        let module = &contributed.module;
        let plugin = ctx
            .plugin(module.id)
            .filter(|_| is_community_plugin(ctx, module.id))
            .map(|info| (info.name, info.namespace));
        for (path, contents) in &contributed.value.0 {
            if let Some((plugin, namespace)) = plugin
                && !under_namespace(path, namespace)
                && !embedded_by_import(ctx, &embedded_by, path, module.id)
            {
                errors.push(format!(
                    "plugin `{plugin}` embeds the document file `{path}` in `{}`, which is not under its namespace: put it under `{namespace}/`",
                    module.name
                ));
            }
            if let Some((previous, previous_contents)) =
                owners.insert(path.as_str(), (module, contents))
                && previous.id != module.id
                && !close_takeover(ctx, path, module, &mut takeovers)
                && previous_contents != contents
            {
                say_duplicate(ctx, path, previous, module, &mut takeovers, &mut errors);
            }
        }
    }
    errors.extend(settle(takeovers, templates));
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n"))
    }
}

/// Whether `path` starts with `namespace` and a slash.
fn under_namespace(path: &str, namespace: &str) -> bool {
    path.strip_prefix(namespace)
        .is_some_and(|rest| rest.starts_with('/'))
}

/// Whether a module that `module` imports, directly or not, embeds `path`.
fn embedded_by_import(
    ctx: &AppContext,
    embedded_by: &HashMap<&str, Vec<ModuleId>>,
    path: &str,
    module: ModuleId,
) -> bool {
    embedded_by.get(path).is_some_and(|modules| {
        modules
            .iter()
            .any(|other| *other != module && imports(ctx, module, *other))
    })
}

/// Whether `module`, which embeds `path` after another module, closes the
/// [`Takeover`] of that path: a plugin's module replaced a file of the app or
/// of the framework, and now a module of the app embeds the path after it, so
/// the app's version is the one rendered, whatever its contents. Said at
/// `info`; `false` when no takeover of `path` is open or `module` is not a
/// module of the app.
fn close_takeover(
    ctx: &AppContext,
    path: &str,
    module: &ModuleInfo,
    takeovers: &mut Vec<Takeover>,
) -> bool {
    let open = takeovers
        .iter()
        .find(|taken| taken.path == path)
        .map(|taken| (taken.first, taken.second, taken.plugin));
    let Some((replaced, replacing, plugin)) = open else {
        return false;
    };
    if !is_app_module(ctx, module.id) {
        return false;
    }
    takeovers.retain(|taken| taken.path != path);
    let used = module.name;
    tracing::info!(
        target: "navree::documents",
        "document file `{path}` of `{replaced}` is replaced by `{replacing}`'s, a module of the plugin `{plugin}`; `{used}`'s is used"
    );
    true
}

/// What it means that `module`, after `previous`, embeds the file `path` with
/// other contents, when `module` does not close a takeover of it: said at
/// `info` when `module` imports `previous` and is not a module of a plugin the
/// kernel does not find first-party; when it is, a warning if `previous`
/// belongs to such a plugin too, and a takeover otherwise. When it does not
/// import `previous`, an error when either belongs to such a plugin, and
/// otherwise a warning.
fn say_duplicate(
    ctx: &AppContext,
    path: &str,
    previous: &ModuleInfo,
    module: &ModuleInfo,
    takeovers: &mut Vec<Takeover>,
    errors: &mut Vec<String>,
) {
    let (first, second) = (previous.name, module.name);
    if imports(ctx, module.id, previous.id) {
        let community = ctx
            .plugin(module.id)
            .filter(|_| is_community_plugin(ctx, module.id));
        match community {
            None => {
                tracing::info!(
                    target: "navree::documents",
                    "document file `{path}` of `{first}` is replaced by `{second}`'s"
                );
            }
            Some(plugin) if is_community_plugin(ctx, previous.id) => {
                tracing::warn!(
                    target: "navree::documents",
                    "document file `{path}` of `{first}` is replaced by `{second}`'s, a module of the plugin `{}`",
                    plugin.name
                );
            }
            Some(plugin) => takeovers.push(Takeover {
                path: path.to_owned(),
                first,
                second,
                plugin: plugin.name,
            }),
        }
    } else if is_community_plugin(ctx, previous.id) || is_community_plugin(ctx, module.id) {
        errors.push(format!(
            "document file `{path}` is embedded by both `{first}` and `{second}`; a plugin's document files are under its namespace"
        ));
    } else {
        tracing::warn!(
            target: "navree::documents",
            "document file `{path}` is embedded by both `{first}` and `{second}`; `{second}`'s is used, and this becomes an error in 2.0"
        );
    }
}

/// The errors of the takeovers no file in a template folder settles; one that
/// a file settles is said at `info`.
fn settle(takeovers: Vec<Takeover>, templates: &Templates) -> Vec<String> {
    let mut errors = Vec::new();
    for Takeover {
        path,
        first,
        second,
        plugin,
    } in takeovers
    {
        if templates.has_file(&path) {
            tracing::info!(
                target: "navree::documents",
                "document file `{path}` of `{first}` is replaced by `{second}`'s, a module of the plugin `{plugin}`; the file `{path}` in a documents.templates folder is used"
            );
        } else {
            let folder = match templates.dirs().first() {
                Some(dir) => {
                    let dir = dir.to_string_lossy().replace('\\', "/");
                    format!(" ({}/{path})", dir.trim_end_matches('/'))
                }
                None => String::new(),
            };
            errors.push(format!(
                "document file `{path}` of `{first}` cannot be replaced by `{second}`, a module of the plugin `{plugin}`; to settle it, put the file you want, the plugin's or your own, in a documents.templates folder{folder}, or embed it from a module of the app that comes after `{second}`"
            ));
        }
    }
    errors
}

/// Closes Chrome when the app stops.
struct CloseEngines(Arc<Documents>);

impl Lifecycle for CloseEngines {
    fn on_shutdown(&self, _ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move {
            self.0.shutdown().await;
            Ok(())
        })
    }
}

fn register(m: &mut ModuleBuilder, options: Option<DocumentsOptions>) {
    m.global();
    match options {
        // Options given in code are checked at boot, as configuration is.
        Some(options) => m.factory(Factory::<DocumentsOptions>::new(move |_| {
            let options = options.clone();
            async move {
                options
                    .validate()
                    .map_err(|message| navree_config::ConfigError::Validation {
                        key: DocumentsOptions::KEY.to_owned(),
                        message,
                    })?;
                Ok(Arc::new(options))
            }
        })),
        None => m.config::<DocumentsOptions>(),
    };
    m.exports::<DocumentsOptions>()
        .factory(
            Factory::<Documents>::new(|r: Resolver| async move {
                let options = r.get::<DocumentsOptions>().await?;
                let app = r.app();
                let contributions = app.contributions::<DocumentFiles>();
                let embedded = contributions
                    .iter()
                    .flat_map(|contributed| contributed.value.0.clone());
                let templates = Templates::new(options.templates.clone(), embedded);
                check_document_files(&app, &contributions, &templates)?;
                let clock = app.try_get::<dyn Clock>().await;
                let mut documents = Documents::build(&options, templates, clock);
                if let Some(storage) = app.try_get::<Storage>().await {
                    if let Some(disk) = &options.disk {
                        storage.disk(disk).map_err(|_| {
                            format!(
                                "documents.disk = \"{disk}\" is not a configured storage disk \
                                 (storage.disks)"
                            )
                        })?;
                    }
                    documents = documents.storage(storage, None);
                }
                if let Some(dispatcher) = app.try_get::<dyn Dispatcher>().await {
                    documents = documents.dispatcher(dispatcher);
                }
                Ok(Arc::new(documents))
            })
            .deps(vec![ProviderKey::of::<DocumentsOptions>()]),
        )
        .exports::<Documents>()
        .factory(
            Factory::<CloseEngines>::new(|r: Resolver| async move {
                Ok(Arc::new(CloseEngines(r.get::<Documents>().await?)))
            })
            .deps(vec![ProviderKey::of::<Documents>()]),
        )
        .lifecycle::<CloseEngines>()
        .job::<RenderDocument>()
        .map_error::<DocumentsError>(map_documents_error);
    for command in commands() {
        m.command(command);
    }
}

/// Document registrations on a [`ModuleBuilder`].
pub trait ModuleBuilderDocumentsExt {
    /// Embeds template files (templates, layouts, images, fonts' CSS) in
    /// the app, as `(path, contents)` pairs: `certificates/classic.html`,
    /// `images/seal.png`. A file at the same path in a template directory
    /// wins, so an app can override what a module ships. Paths that would
    /// leave the template tree are ignored.
    ///
    /// A module of a plugin that is not one of the framework's own embeds paths
    /// under the plugin's namespace (`announcements/certificates/course.html`),
    /// or exactly the path of a module it imports; any other path fails the
    /// boot. Two modules that embed one path with different contents log a
    /// warning at boot and the later module's file is used; when one of them
    /// belongs to such a plugin, the boot fails. A module of such a plugin may
    /// replace a path of a module it imports when the file is another plugin's,
    /// with a warning; replacing a file of the app or of the framework fails the
    /// boot while the plugin's version is the one used. It is not when a
    /// `documents.templates` folder has a file at that path, or when a module of
    /// the app, one that names no plugin and is in no crate that a plugin of the
    /// app names, embeds the path after the plugin's module, with the plugin's
    /// contents or its own. The replacement is then said at `info`.
    ///
    /// ```ignore
    /// m.document_files([
    ///     ("certificates/course.html", include_bytes!("../documents/certificates/course.html").as_slice()),
    ///     ("images/seal.png", include_bytes!("../documents/images/seal.png").as_slice()),
    /// ]);
    /// ```
    fn document_files<I, P, B>(&mut self, files: I) -> &mut Self
    where
        I: IntoIterator<Item = (P, B)>,
        P: Into<String>,
        B: Into<Bytes>;
}

impl ModuleBuilderDocumentsExt for ModuleBuilder {
    fn document_files<I, P, B>(&mut self, files: I) -> &mut Self
    where
        I: IntoIterator<Item = (P, B)>,
        P: Into<String>,
        B: Into<Bytes>,
    {
        let files = files
            .into_iter()
            .filter_map(|(path, contents)| {
                let path = path.into();
                match normalize(&path) {
                    Some(path) => Some((path, contents.into())),
                    None => {
                        tracing::warn!(target: "navree_documents", path, "ignored an embedded document file with an invalid path");
                        None
                    }
                }
            })
            .collect();
        self.contribute(DocumentFiles(files))
    }
}
