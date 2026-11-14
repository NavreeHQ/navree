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

use crate::{ViewOptions, Views};

/// The global view module: provides and exports [`Views`].
///
/// - [`ViewModule::for_root`] takes [`ViewOptions`] (or just a directory) in code;
/// - [`ViewModule::from_config`] (and a plain `.module(ViewModule)` or an
///   import with no configured instance in the app) reads section `view` at
///   boot, so the app must include `ConfigModule`.
///
/// Templates come from the configured directories and from templates that
/// modules embed with [`ModuleBuilderViewExt::templates`] (a plugin's, with
/// [`ModuleBuilderPluginViewsExt::plugin_templates`](crate::ModuleBuilderPluginViewsExt::plugin_templates),
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
                    let embedded = embedded_templates(&r.app())?;
                    // Reads template files: keep it off the async workers.
                    let mut views = tokio::task::spawn_blocking(move || {
                        Views::with_templates(&options, embedded)
                    })
                    .await??;
                    configure_environment(&r.app(), &mut views);
                    tracing::debug!(views = ?views, "views loaded");
                    Ok(Arc::new(views))
                }
            })
            .deps(deps),
        )
        .exports::<Views>();
}

/// Every embedded template with its full name (a plugin's under its
/// namespace), modules in dependency order, so that a module's template
/// replaces one of the same name from a module it imports. Two other modules
/// that embed one name with different text are reported: a warning, or an
/// error when either module belongs to a plugin the kernel does not find
/// first-party.
fn embedded_templates(ctx: &AppContext) -> Result<Vec<(String, &'static str)>, String> {
    let mut owners: HashMap<String, (ModuleInfo, &'static str)> = HashMap::new();
    let mut embedded = Vec::new();
    let mut errors = Vec::new();
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
                say_duplicate(ctx, &name, &previous, &module, &mut errors);
            }
            embedded.push((name, source));
        }
    }
    if errors.is_empty() {
        Ok(embedded)
    } else {
        Err(errors.join("\n"))
    }
}

/// What it means that `module`, after `previous`, embeds the template `name`
/// with other text: said at `info` when `module` imports `previous` (a module
/// may replace what it imports), said at `warn` instead when `module` belongs
/// to a plugin the kernel does not find first-party, an error when either
/// belongs to one, and otherwise a warning.
fn say_duplicate(
    ctx: &AppContext,
    name: &str,
    previous: &ModuleInfo,
    module: &ModuleInfo,
    errors: &mut Vec<String>,
) {
    let (first, second) = (previous.name, module.name);
    if imports(ctx, module.id, previous.id) {
        let community = ctx
            .plugin(module.id)
            .filter(|_| is_community_plugin(ctx, module.id));
        match community {
            Some(plugin) => {
                tracing::warn!(
                    target: "navree::view",
                    "embedded template `{name}` of `{first}` is replaced by `{second}`'s, a module of the plugin `{}`",
                    plugin.name
                );
            }
            None => {
                tracing::info!(
                    target: "navree::view",
                    "embedded template `{name}` of `{first}` is replaced by `{second}`'s"
                );
            }
        }
    } else if is_community_plugin(ctx, previous.id) || is_community_plugin(ctx, module.id) {
        errors.push(format!(
            "embedded template `{name}` is registered by both `{first}` and `{second}`; a plugin's templates are under its namespace"
        ));
    } else {
        tracing::warn!(
            target: "navree::view",
            "embedded template `{name}` is registered by both `{first}` and `{second}`; `{second}`'s is used, and this becomes an error in 2.0"
        );
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
    /// used; when one of the two is a plugin, the boot fails (a plugin's
    /// templates are under its namespace, see
    /// [`ModuleBuilderPluginViewsExt::plugin_templates`](crate::ModuleBuilderPluginViewsExt::plugin_templates)).
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
