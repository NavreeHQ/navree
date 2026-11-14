//! [`ViewModule`] and [`ModuleBuilderViewExt`].

use std::collections::HashMap;
use std::sync::Arc;

use navree_config::{Config, ModuleBuilderConfigExt};
use navree_kernel::__private::{imports, is_community_plugin};
use navree_kernel::{
    AppContext, BoxError, ConfiguredModule, Factory, Module, ModuleBuilder, ModuleInfo,
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
                        mut errors,
                    } = embedded_templates(&r.app());
                    let names: Vec<String> =
                        takeovers.iter().map(|taken| taken.name.clone()).collect();
                    // Reads template files: keep it off the async workers.
                    let (mut views, provided) = tokio::task::spawn_blocking(move || {
                        let views = Views::with_templates(&options, templates)?;
                        let provided = views.provided_by_files(&names)?;
                        Ok::<_, ViewError>((views, provided))
                    })
                    .await??;
                    errors.extend(settle(takeovers, &provided));
                    if !errors.is_empty() {
                        return Err(BoxError::from(errors.join("\n")));
                    }
                    configure_environment(&r.app(), &mut views);
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

/// Every embedded template with its full name (a plugin's under its
/// namespace), modules in dependency order, so that a module's template
/// replaces one of the same name from a module it imports. Two other modules
/// that embed one name with different text are reported: a warning, or an
/// error when either module belongs to a plugin the kernel does not find
/// first-party. A module of such a plugin that replaces a template of a module
/// it imports is an error too, unless the template is another such plugin's or
/// the app's own version is the one rendered: that is a [`Takeover`], settled
/// by a template file once the files are read.
fn embedded_templates(ctx: &AppContext) -> Found {
    let mut owners: HashMap<String, (ModuleInfo, &'static str)> = HashMap::new();
    let mut found = Found {
        templates: Vec::new(),
        errors: Vec::new(),
        takeovers: Vec::new(),
    };
    for contributed in ctx.contributions::<EmbeddedTemplates>() {
        let module = contributed.module;
        // A plugin's templates go under its namespace. A module that names no
        // plugin and has files to publish fails the boot before this runs, so
        // a namespaced contribution without a plugin holds no templates.
        let namespace = if contributed.value.under_namespace {
            match ctx.plugin(module.id) {
                Some(info) => Some(info.namespace),
                None => continue,
            }
        } else {
            None
        };
        for &(path, source) in &contributed.value.templates {
            let name = match namespace {
                Some(namespace) => format!("{namespace}/{path}"),
                None => path.to_owned(),
            };
            if let Some((previous, previous_source)) =
                owners.insert(name.clone(), (module.clone(), source))
                && previous.id != module.id
                && previous_source != source
            {
                say_duplicate(ctx, &name, &previous, &module, &mut found);
            }
            found.templates.push((name, source));
        }
    }
    found
}

/// What it means that `module`, after `previous`, embeds the template `name`
/// with other text.
///
/// When a plugin's module replaced a template of the app or of the framework
/// (a [`Takeover`] still open) and `module` belongs to no plugin, the app's
/// version is the one rendered: said at `info`, and the takeover is closed.
///
/// Otherwise, when `module` imports `previous` (a module may replace what it
/// imports) there are three outcomes: said at `info` when `module` is not a
/// module of a plugin the kernel does not find first-party; when it is, a
/// warning if `previous` belongs to such a plugin too (another plugin's
/// template), and a takeover otherwise (a template of the app or of the
/// framework). When it does not import `previous`, an error when either belongs
/// to such a plugin, and otherwise a warning.
fn say_duplicate(
    ctx: &AppContext,
    name: &str,
    previous: &ModuleInfo,
    module: &ModuleInfo,
    found: &mut Found,
) {
    let (first, second) = (previous.name, module.name);
    if ctx.plugin(module.id).is_none() {
        let open = found
            .takeovers
            .iter()
            .find(|taken| taken.name == name)
            .map(|taken| (taken.first, taken.second, taken.plugin));
        if let Some((replaced, replacing, plugin)) = open {
            found.takeovers.retain(|taken| taken.name != name);
            tracing::info!(
                target: "navree::view",
                "embedded template `{name}` of `{replaced}` is replaced by `{replacing}`'s, a module of the plugin `{plugin}`; `{second}`'s is used"
            );
            return;
        }
    }
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
fn configure_environment(ctx: &AppContext, views: &mut Views) {
    for contributed in ctx.contributions::<EnvironmentHook>() {
        let configure = contributed.value.0.clone();
        views.configure(|env| configure(env));
    }
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
    /// A module of such a plugin may replace a template of a module it imports
    /// when that template is another plugin's, and is then warned about.
    /// Replacing a template of the app or of the framework fails the boot while
    /// the plugin's version is the one rendered: it is not when a template
    /// folder holds a file of that name, or when a module that belongs to no
    /// plugin registers the name after the plugin's module. The replacement is
    /// then said at `info` and the boot goes on.
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
