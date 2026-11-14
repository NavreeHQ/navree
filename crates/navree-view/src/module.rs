//! [`ViewModule`] and [`ModuleBuilderViewExt`].

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use navree_config::{Config, ModuleBuilderConfigExt};
use navree_kernel::__private::{
    imports, is_app_module, is_community_plugin, is_safe_relative_path,
};
use navree_kernel::{
    AppContext, BoxError, ConfiguredModule, Factory, Module, ModuleBuilder, ModuleId, ModuleInfo,
    ProviderKey, Resolver,
};

use navree_template::Environment;

use crate::{ViewError, ViewOptions, Views};

/// The global view module: provides and exports [`Views`].
///
/// - [`ViewModule::for_root`] takes [`ViewOptions`] (or just a directory) in code;
/// - [`ViewModule::from_config`] (and a plain `.module(ViewModule)` or an
///   import with no configured instance in the app) reads section `view` at
///   boot, so the app must include `ConfigModule`.
///
/// Templates come from the configured directories and from templates that
/// modules embed with [`ModuleBuilderViewExt::templates`] (a plugin's, with
/// [`ModuleBuilderPluginTemplatesExt::plugin_templates`](crate::ModuleBuilderPluginTemplatesExt::plugin_templates),
/// are under its namespace). They are read
/// and compiled when the app boots (off the async workers): a syntax error
/// fails the boot, naming the template.
///
/// ```no_run
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_kernel::Navree;
/// use navree_view::{ViewModule, ViewOptions};
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// Navree::app()
///     .module(ConfigModule::for_root(ConfigSources::defaults()))
///     .module(ViewModule::for_root(ViewOptions {
///         dirs: vec!["resources/views".into()],
///         reload: false,
///     }))
///     .run()
/// # }
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct ViewModule;

impl ViewModule {
    /// The module configured with `options` (configure it once, in the app
    /// root). A directory converts into the default options with that
    /// directory: `ViewModule::for_root("resources/mail")`.
    pub fn for_root(options: impl Into<ViewOptions>) -> impl Module {
        ConfiguredModule::<ViewModule, ViewOptions>::for_root(options.into(), register)
    }

    /// The module configured from section `view` of the app's `Config` at
    /// boot (see [`ViewOptions`]).
    pub fn from_config() -> impl Module {
        ConfiguredModule::<ViewModule, ViewOptions>::from_config(register)
    }
}

impl Module for ViewModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

/// Where the options come from.
#[derive(Clone)]
enum Source {
    Options(Arc<ViewOptions>),
    Config,
}

impl Source {
    async fn options(&self, r: &Resolver) -> Result<ViewOptions, BoxError> {
        Ok(match self {
            Source::Options(options) => ViewOptions::clone(options),
            Source::Config => r.get::<Config>().await?.typed::<ViewOptions>()?,
        })
    }

    /// The keys the factory resolves.
    fn deps(&self) -> Vec<ProviderKey> {
        match self {
            Source::Options(_) => Vec::new(),
            Source::Config => vec![ProviderKey::of::<Config>()],
        }
    }
}

/// Global; provides and exports [`Views`], built from the options and every
/// module's embedded templates. No options: the module reads section `view`.
fn register(m: &mut ModuleBuilder, options: Option<ViewOptions>) {
    let source = match options {
        Some(options) => Source::Options(Arc::new(options)),
        None => {
            m.config_section::<ViewOptions>();
            Source::Config
        }
    };
    let deps = source.deps();
    m.global()
        .factory(
            Factory::<Views>::new(move |r| {
                let source = source.clone();
                async move {
                    let options = source.options(&r).await?;
                    let Found {
                        templates,
                        takeovers,
                        plugin_names,
                        app_names,
                        mut errors,
                    } = embedded_templates(&r.app());
                    let names: Vec<String> =
                        takeovers.iter().map(|taken| taken.name.clone()).collect();
                    let wants_files = !plugin_names.is_empty();
                    // Reads template files: keep it off the async workers.
                    let (mut views, provided, files) = tokio::task::spawn_blocking(move || {
                        let views = Views::with_templates(&options, templates)?;
                        let provided = views.provided_by_files(&names)?;
                        let files = if wants_files {
                            views.file_names()?
                        } else {
                            Vec::new()
                        };
                        Ok::<_, ViewError>((views, provided, files))
                    })
                    .await??;
                    errors.extend(settle(takeovers, &provided));
                    errors.extend(stem_errors(&plugin_names, &app_names, &files));
                    errors.extend(configure_environment(&r.app(), &mut views));
                    if !errors.is_empty() {
                        return Err(BoxError::from(errors.join("\n")));
                    }
                    tracing::debug!(views = ?views, "views loaded");
                    Ok(Arc::new(views))
                }
            })
            .deps(deps),
        )
        .exports::<Views>();
}

/// What the check of the embedded templates leaves.
struct Found {
    /// Every embedded template with its full name, in the order they win: the
    /// last one given for a name is the one rendered.
    templates: Vec<(String, &'static str)>,
    /// What fails the boot at once.
    errors: Vec<String>,
    /// The replacements that fail the boot unless a template folder settles
    /// them.
    takeovers: Vec<Takeover>,
    /// The templates of modules of plugins the kernel does not find
    /// first-party that are under their plugin's namespace: what the stem rule
    /// looks at.
    plugin_names: Vec<PluginTemplate>,
    /// The names of the templates that modules embed which are not such a
    /// plugin's: what the stem rule looks beside.
    app_names: Vec<String>,
}

/// A template of a module of a plugin the kernel does not find first-party,
/// under that plugin's namespace.
struct PluginTemplate {
    /// The template's full name.
    name: String,
    /// The plugin's name.
    plugin: &'static str,
    /// The plugin's namespace.
    namespace: &'static str,
}

/// A template of a module of the app or of the framework that a module of a
/// plugin it imports replaced, while the plugin's version is the one the app
/// renders. A file of that name in a template folder, or a module of the app
/// that registers the name after the plugin's module, makes the app's version
/// the one rendered instead, and the replacement is only said at `info`.
struct Takeover {
    /// The template's full name.
    name: String,
    /// The module whose template was replaced.
    first: &'static str,
    /// The module of the plugin that replaced it.
    second: &'static str,
    /// The plugin's name.
    plugin: &'static str,
}

/// One template a module registers.
struct Entry {
    module: ModuleInfo,
    /// The plugin's name and namespace, when the module names a plugin the
    /// kernel does not find first-party.
    plugin: Option<(&'static str, &'static str)>,
    /// The template's full name.
    name: String,
    source: &'static str,
    /// Whether `m.plugin_templates` registered it, under the namespace.
    namespaced: bool,
}

/// Every embedded template with its full name (a plugin's under its
/// namespace), modules in dependency order, so that a module's template
/// replaces one of the same name from a module it imports.
///
/// A template that a module of a plugin the kernel does not find first-party
/// registers with `m.templates` is an error unless its name starts with the
/// plugin's namespace and a slash and is a safe relative path, or is exactly
/// the name of a template that a module it imports registers (replacing that
/// one is the case below).
///
/// Two other modules that embed one name with different text are reported: a
/// warning, or an error when either module belongs to such a plugin. A module
/// of such a plugin that replaces a template of a module it imports is an error
/// too, unless the template is another such plugin's or the app's own version
/// is the one rendered: that is a [`Takeover`], settled by a template file once
/// the files are read.
fn embedded_templates(ctx: &AppContext) -> Found {
    // Every template of every module, in order.
    let mut entries: Vec<Entry> = Vec::new();
    for contributed in ctx.contributions::<EmbeddedTemplates>() {
        let module = contributed.module;
        let info = ctx.plugin(module.id);
        // A plugin's templates go under its namespace. A module that names no
        // plugin and has files to publish fails the boot before this runs, so
        // a namespaced contribution without a plugin holds no templates.
        let namespace = if contributed.value.under_namespace {
            match info {
                Some(info) => Some(info.namespace),
                None => continue,
            }
        } else {
            None
        };
        let plugin = info
            .filter(|_| is_community_plugin(ctx, module.id))
            .map(|info| (info.name, info.namespace));
        for &(path, source) in &contributed.value.templates {
            let name = match namespace {
                Some(namespace) => format!("{namespace}/{path}"),
                None => path.to_owned(),
            };
            entries.push(Entry {
                module: module.clone(),
                plugin,
                name,
                source,
                namespaced: contributed.value.under_namespace,
            });
        }
    }
    // Who registers each name, for a plugin that replaces what it imports.
    let mut registered: HashMap<&str, Vec<ModuleId>> = HashMap::new();
    for entry in &entries {
        registered
            .entry(entry.name.as_str())
            .or_default()
            .push(entry.module.id);
    }

    let mut owners: HashMap<String, (ModuleInfo, &'static str)> = HashMap::new();
    let mut found = Found {
        templates: Vec::new(),
        errors: Vec::new(),
        takeovers: Vec::new(),
        plugin_names: Vec::new(),
        app_names: Vec::new(),
    };
    for entry in &entries {
        let (module, name, source) = (&entry.module, &entry.name, entry.source);
        match entry.plugin {
            Some((plugin, namespace)) => {
                let under = under_namespace(name, namespace);
                if !entry.namespaced
                    && !(under && is_safe_relative_path(name))
                    && !registered_by_import(ctx, &registered, name, module.id)
                {
                    found.errors.push(format!(
                        "plugin `{plugin}` registers the template `{name}` in `{}`, which is not under its namespace: name it `{namespace}/..` or ship it with m.plugin_templates",
                        module.name
                    ));
                }
                if under {
                    found.plugin_names.push(PluginTemplate {
                        name: name.clone(),
                        plugin,
                        namespace,
                    });
                }
            }
            None => found.app_names.push(name.clone()),
        }
        if let Some((previous, previous_source)) =
            owners.insert(name.clone(), (module.clone(), source))
            && previous.id != module.id
            && !close_takeover(ctx, name, module, &mut found)
            && previous_source != source
        {
            say_duplicate(ctx, name, &previous, module, &mut found);
        }
        found.templates.push((name.clone(), source));
    }
    found
}

/// Whether `name` starts with `namespace` and a slash.
fn under_namespace(name: &str, namespace: &str) -> bool {
    name.strip_prefix(namespace)
        .is_some_and(|rest| rest.starts_with('/'))
}

/// Whether a module that `module` imports, directly or not, registers the
/// template `name`.
fn registered_by_import(
    ctx: &AppContext,
    registered: &HashMap<&str, Vec<ModuleId>>,
    name: &str,
    module: ModuleId,
) -> bool {
    registered.get(name).is_some_and(|modules| {
        modules
            .iter()
            .any(|other| *other != module && imports(ctx, module, *other))
    })
}

/// Whether `module`, which registers the template `name` after another module,
/// closes the [`Takeover`] of that name: a plugin's module replaced a template
/// of the app or of the framework, and now a module of the app registers the
/// name after it, so the app's version is the one rendered. It does, whatever
/// the text it registers: the plugin's own text is the app choosing the
/// plugin's version. Said at `info`; `false` when no takeover of `name` is open
/// or `module` is not a module of the app (see `is_app_module`).
fn close_takeover(ctx: &AppContext, name: &str, module: &ModuleInfo, found: &mut Found) -> bool {
    let open = found
        .takeovers
        .iter()
        .find(|taken| taken.name == name)
        .map(|taken| (taken.first, taken.second, taken.plugin));
    let Some((replaced, replacing, plugin)) = open else {
        return false;
    };
    if !is_app_module(ctx, module.id) {
        return false;
    }
    found.takeovers.retain(|taken| taken.name != name);
    let used = module.name;
    tracing::info!(
        target: "navree::view",
        "embedded template `{name}` of `{replaced}` is replaced by `{replacing}`'s, a module of the plugin `{plugin}`; `{used}`'s is used"
    );
    true
}

/// What it means that `module`, after `previous`, embeds the template `name`
/// with other text, when `module` does not close a takeover of it.
///
/// When `module` imports `previous` (a module may replace what it imports)
/// there are three outcomes: said at `info` when `module` is not a module of a
/// plugin the kernel does not find first-party; when it is, a warning if
/// `previous` belongs to such a plugin too (another plugin's template), and a
/// takeover otherwise (a template of the app or of the framework). When it does
/// not import `previous`, an error when either belongs to such a plugin, and
/// otherwise a warning.
fn say_duplicate(
    ctx: &AppContext,
    name: &str,
    previous: &ModuleInfo,
    module: &ModuleInfo,
    found: &mut Found,
) {
    let (first, second) = (previous.name, module.name);
    if imports(ctx, module.id, previous.id) {
        let community = ctx
            .plugin(module.id)
            .filter(|_| is_community_plugin(ctx, module.id));
        match community {
            None => {
                tracing::info!(
                    target: "navree::view",
                    "embedded template `{name}` of `{first}` is replaced by `{second}`'s"
                );
            }
            Some(plugin) if is_community_plugin(ctx, previous.id) => {
                tracing::warn!(
                    target: "navree::view",
                    "embedded template `{name}` of `{first}` is replaced by `{second}`'s, a module of the plugin `{}`",
                    plugin.name
                );
            }
            Some(plugin) => {
                found.takeovers.push(Takeover {
                    name: name.to_owned(),
                    first,
                    second,
                    plugin: plugin.name,
                });
            }
        }
    } else if is_community_plugin(ctx, previous.id) || is_community_plugin(ctx, module.id) {
        found.errors.push(format!(
            "embedded template `{name}` is registered by both `{first}` and `{second}`; a plugin's templates are under its namespace"
        ));
    } else {
        tracing::warn!(
            target: "navree::view",
            "embedded template `{name}` is registered by both `{first}` and `{second}`; `{second}`'s is used, and this becomes an error in 2.0"
        );
    }
}

/// The errors of the takeovers no template file settles. `provided` holds the
/// names a template file provides, which win over an embedded template: a
/// takeover of such a name is said at `info` instead.
fn settle(takeovers: Vec<Takeover>, provided: &[String]) -> Vec<String> {
    let mut errors = Vec::new();
    for Takeover {
        name,
        first,
        second,
        plugin,
    } in takeovers
    {
        if provided.contains(&name) {
            tracing::info!(
                target: "navree::view",
                "embedded template `{name}` of `{first}` is replaced by `{second}`'s, a module of the plugin `{plugin}`; the file `{name}` in a template folder is used"
            );
        } else {
            errors.push(format!(
                "embedded template `{name}` of `{first}` cannot be replaced by `{second}`, a module of the plugin `{plugin}`; to use the plugin's version, copy it to resources/views/{name} or register it from a module of the app"
            ));
        }
    }
    errors
}

/// The errors of the stem rule. A template of a plugin the kernel does not find
/// first-party, under its namespace, may not stand beside a template of the app
/// with the same name but for its extension (`reports/summary.html` and
/// `reports/summary.txt`): a lookup without an extension takes `.html` first,
/// and a mail of that name adds an HTML part when `.html` exists, so the
/// plugin's would be found where the app's was meant. The app has a template
/// that is a file in a template folder (`files`) or that a module embeds which
/// is not such a plugin's (`app_names`). The app takes the plugin's name for
/// itself by having it too, in a folder or embedded: that template wins.
fn stem_errors(
    plugin_names: &[PluginTemplate],
    app_names: &[String],
    files: &[String],
) -> Vec<String> {
    let app: BTreeSet<&str> = app_names.iter().chain(files).map(String::as_str).collect();
    let mut errors = Vec::new();
    for template in plugin_names {
        if app.contains(template.name.as_str()) {
            continue;
        }
        let stem = stem_of(&template.name);
        let other = app.iter().copied().find(|other| stem_of(other) == stem);
        if let Some(other) = other {
            errors.push(format!(
                "plugin `{}`: its template `{}` stands beside the app's `{other}`, and a lookup of `{stem}` or a mail of that name would take the plugin's; put `{}` in a template folder too (plugin:publish {} --tag views copies it) or rename the app's `{other}`",
                template.plugin, template.name, template.name, template.namespace
            ));
        }
    }
    errors
}

/// A template name without its last extension, past a trailing `.j2`, `.jinja2`
/// or `.jinja` and past the dots and spaces Windows ignores at the end of a file
/// name, as navree-template reads the extension: `reports/summary.html` and
/// `reports/summary.txt.j2` have the stem `reports/summary`.
fn stem_of(name: &str) -> &str {
    let name = name.trim_end_matches(['.', ' ']);
    let lower = name.to_ascii_lowercase();
    let mut end = name.len();
    for suffix in [".j2", ".jinja2", ".jinja"] {
        if lower.ends_with(suffix) {
            end -= suffix.len();
            break;
        }
    }
    let base = &name[..end];
    let file = base.rfind('/').map_or(0, |at| at + 1);
    match base[file..].rfind('.') {
        Some(dot) if dot > 0 => &base[..file + dot],
        _ => base,
    }
}

/// Templates a module embeds, contributed by `m.templates(..)` (names as
/// given) and by `m.plugin_templates(..)` (names under the plugin's
/// namespace).
pub(crate) struct EmbeddedTemplates {
    /// (path, source) pairs.
    pub(crate) templates: Vec<(&'static str, &'static str)>,
    /// Whether the paths are registered under the namespace of the plugin the
    /// module names, read when the app boots.
    pub(crate) under_namespace: bool,
}

/// A change to the template environment (contributed by
/// [`ModuleBuilderViewExt::view_environment`]).
type ConfigureEnvironment = dyn Fn(&mut Environment) + Send + Sync;

/// Environment changes a module contributes.
struct EnvironmentHook(Arc<ConfigureEnvironment>);

/// Applies every contributed environment change to `views`, modules in
/// dependency order (a later module's filter replaces an earlier one's).
///
/// The change of a module of a plugin the kernel does not find first-party
/// runs between two fingerprints of the environment, and is an error for each
/// difference but a filter, test, function or global added, replaced or removed
/// whose name starts with the plugin's namespace and an underscore. The
/// environment then holds what the hook did, and the boot fails. Every other
/// module's change runs unchecked.
fn configure_environment(ctx: &AppContext, views: &mut Views) -> Vec<String> {
    let mut errors = Vec::new();
    for contributed in ctx.contributions::<EnvironmentHook>() {
        let configure = contributed.value.0.clone();
        let plugin = ctx
            .plugin(contributed.module.id)
            .filter(|_| is_community_plugin(ctx, contributed.module.id));
        let Some(plugin) = plugin else {
            views.configure(|env| configure(env));
            continue;
        };
        let before = views.fingerprint();
        views.configure(|env| configure(env));
        for change in before.changes_outside(&views.fingerprint(), plugin.namespace) {
            errors.push(format!(
                "plugin `{}`: m.view_environment in `{}` {change}; a plugin's environment hook only adds filters, tests, functions and globals named `{}_..`",
                plugin.name, contributed.module.name, plugin.namespace
            ));
        }
    }
    errors
}

/// Embeds templates in a module, so it works without template files.
pub trait ModuleBuilderViewExt {
    /// Contributes templates as (name, source) pairs, typically with
    /// `include_str!`. Name them with their extension
    /// (`"mail/welcome.html"`): it selects HTML escaping and lets
    /// `views.render("mail/welcome", ..)` find them. A template file of the
    /// same name in a template directory overrides an embedded one, and a
    /// module's template overrides one of the same name from a module it
    /// imports (directly or not). Two other modules that embed one name with
    /// different text log a warning at boot and the later module's template is
    /// used; when one of the two belongs to a plugin that is not one of the
    /// framework's own, the boot fails (a plugin's templates are under its
    /// namespace, see
    /// [`ModuleBuilderPluginTemplatesExt::plugin_templates`](crate::ModuleBuilderPluginTemplatesExt::plugin_templates)).
    /// A module of such a plugin registers templates under the plugin's
    /// namespace (`announcements/list.html`), or replaces a template of a module
    /// it imports; a template of any other name fails the boot, and so does one
    /// under its namespace that stands beside a template of the app with the
    /// same name but for its extension (`announcements/list.html` beside the
    /// app's `announcements/list.txt`), unless the app has the plugin's name too,
    /// in a template folder or embedded by a module of its own.
    /// A module of such a plugin may replace a template of a module it imports
    /// when that template is another plugin's, and is then warned about.
    /// Replacing a template of the app or of the framework fails the boot while
    /// the plugin's version is the one rendered: it is not when a template
    /// folder holds a file of that name (letter for letter, with or without
    /// reloading), or when a module of the app registers the name after the
    /// plugin's module, with the plugin's text or with its own. A module of the
    /// app is one that names no plugin and is in no crate that a plugin of the
    /// app names; it is after the plugin's module when it imports it or is
    /// registered after it. The replacement is then said at `info` and the boot
    /// goes on.
    ///
    /// ```
    /// use navree_kernel::{Module, ModuleBuilder};
    /// use navree_view::ModuleBuilderViewExt;
    ///
    /// struct BillingModule;
    ///
    /// impl Module for BillingModule {
    ///     fn register(&self, m: &mut ModuleBuilder) {
    ///         m.templates(&[
    ///             ("mail/invoice.html", "<p>Total: {{ total|money(currency) }}</p>"),
    ///             ("mail/invoice.txt", "Total: {{ total|money(currency) }}"),
    ///         ]);
    ///     }
    /// }
    /// ```
    fn templates(&mut self, templates: &[(&'static str, &'static str)]) -> &mut Self;

    /// Changes the template environment when `Views` is built: adds
    /// functions, filters or globals, or replaces the built-in `money` and
    /// `date` filters. Modules apply their changes in dependency order.
    ///
    /// A module of a plugin that is not one of the framework's own may only add
    /// filters, tests, functions and globals named after the plugin's
    /// namespace (`announcements_badge`); anything else it does to the
    /// environment (adding, replacing or removing a template, replacing or
    /// removing a name it did not add, changing how templates escape, what an
    /// undefined value does, the limits, the whitespace settings or the
    /// loader) fails the boot and names the change. The app calls a plugin's
    /// function from a module of its own to let it change more.
    ///
    /// ```
    /// use navree_kernel::{Module, ModuleBuilder};
    /// use navree_view::ModuleBuilderViewExt;
    ///
    /// struct BrandModule;
    ///
    /// impl Module for BrandModule {
    ///     fn register(&self, m: &mut ModuleBuilder) {
    ///         m.view_environment(|env| {
    ///             env.add_global("brand", "Agra");
    ///             env.add_filter("shout", |text: &str| text.to_uppercase());
    ///         });
    ///     }
    /// }
    /// ```
    fn view_environment(
        &mut self,
        configure: impl Fn(&mut Environment) + Send + Sync + 'static,
    ) -> &mut Self;
}

impl ModuleBuilderViewExt for ModuleBuilder {
    fn templates(&mut self, templates: &[(&'static str, &'static str)]) -> &mut Self {
        self.contribute(EmbeddedTemplates {
            templates: templates.to_vec(),
            under_namespace: false,
        })
    }

    fn view_environment(
        &mut self,
        configure: impl Fn(&mut Environment) + Send + Sync + 'static,
    ) -> &mut Self {
        self.contribute(EnvironmentHook(Arc::new(configure)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stem_is_the_name_without_its_extension() {
        for (name, stem) in [
            ("reports/summary.html", "reports/summary"),
            ("reports/summary.txt", "reports/summary"),
            ("reports/summary.html.j2", "reports/summary"),
            ("reports/summary.TXT.JINJA2", "reports/summary"),
            ("reports/summary.html. ", "reports/summary"),
            ("reports/summary", "reports/summary"),
            ("a.b/summary", "a.b/summary"),
            ("reports/.hidden", "reports/.hidden"),
        ] {
            assert_eq!(stem_of(name), stem, "{name}");
        }
    }
}
