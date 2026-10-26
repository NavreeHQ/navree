//! [`ViewModule`] and [`ModuleBuilderViewExt`].

use std::collections::BTreeMap;
use std::sync::Arc;

use navree_config::Config;
use navree_kernel::{
    AppContext, BoxError, Factory, Module, ModuleBuilder, ModuleId, ProviderKey, Resolver,
};

use crate::{ViewOptions, Views};

/// The global view module: provides and exports [`Views`].
///
/// - [`ViewModule::for_root`] takes [`ViewOptions`] (or just a directory) in code;
/// - [`ViewModule::from_config`] (and a plain `.module(ViewModule)` or an
///   import with no configured instance in the app) reads section `view` at
///   boot, so the app must include `ConfigModule`.
///
/// Templates come from the configured directories and from templates that
/// modules embed with [`ModuleBuilderViewExt::templates`]. They are read
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
        ConfiguredViewModule {
            source: Source::Options(Arc::new(options.into())),
        }
    }

    /// The module configured from section `view` of the app's `Config` at
    /// boot (see [`ViewOptions`]).
    pub fn from_config() -> impl Module {
        ConfiguredViewModule {
            source: Source::Config,
        }
    }
}

impl Module for ViewModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, Source::Config);
    }
}

/// `ViewModule::for_root(...)` / `from_config()`: same module id, configured.
struct ConfiguredViewModule {
    source: Source,
}

impl Module for ConfiguredViewModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<ViewModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, self.source.clone());
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
/// module's embedded templates.
fn register(m: &mut ModuleBuilder, source: Source) {
    let deps = source.deps();
    m.global()
        .factory(
            Factory::<Views>::new(move |r| {
                let source = source.clone();
                async move {
                    let options = source.options(&r).await?;
                    let embedded = embedded_templates(&r.app());
                    // Reads template files: keep it off the async workers.
                    let views = tokio::task::spawn_blocking(move || {
                        Views::with_templates(&options, embedded)
                    })
                    .await??;
                    tracing::debug!(views = ?views, "views loaded");
                    Ok(Arc::new(views))
                }
            })
            .deps(deps),
        )
        .exports::<Views>();
}

/// Every embedded template, modules in dependency order, so that a module's
/// template replaces one of the same name from a module it imports.
fn embedded_templates(ctx: &AppContext) -> Vec<(&'static str, &'static str)> {
    let mut owners: BTreeMap<&'static str, &'static str> = BTreeMap::new();
    let mut embedded = Vec::new();
    for contributed in ctx.contributions::<EmbeddedTemplates>() {
        let module = contributed.module.name;
        for &(name, source) in &contributed.value.0 {
            if let Some(previous) = owners.insert(name, module)
                && previous != module
            {
                tracing::debug!(
                    template = name,
                    module,
                    replaces = previous,
                    "embedded template replaced"
                );
            }
            embedded.push((name, source));
        }
    }
    embedded
}

/// Templates a module embeds (contributed by [`ModuleBuilderViewExt::templates`]).
struct EmbeddedTemplates(Vec<(&'static str, &'static str)>);

/// Embeds templates in a module, so it works without template files.
pub trait ModuleBuilderViewExt {
    /// Contributes templates as (name, source) pairs, typically with
    /// `include_str!`. Name them with their extension
    /// (`"mail/welcome.html"`): it selects HTML escaping and lets
    /// `views.render("mail/welcome", ..)` find them. A template file of the
    /// same name in a template directory overrides an embedded one, and a
    /// module's template overrides one of the same name from a module it
    /// imports (directly or not).
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
}

impl ModuleBuilderViewExt for ModuleBuilder {
    fn templates(&mut self, templates: &[(&'static str, &'static str)]) -> &mut Self {
        self.contribute(EmbeddedTemplates(templates.to_vec()))
    }
}
