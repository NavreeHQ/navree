//! The translations plugins ship: the Fluent files a module contributes with
//! `m.locales(..)`, read when the app boots.

use navree_contracts::EmbeddedLocales;
use navree_kernel::__private::is_community_plugin;
use navree_kernel::AppContext;

use crate::I18nError;

/// The Fluent files of one module of a plugin.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PluginSource {
    /// The plugin's package name, which names its files in messages
    /// (`navree-plugin-announcements:locales/en/announcements.ftl`).
    pub(crate) plugin: &'static str,
    /// What the plugin's message ids start with.
    pub(crate) namespace: &'static str,
    /// Whether its message ids must start with the namespace: the plugin is
    /// not one of the framework's own.
    pub(crate) checked: bool,
    /// Each file's locale, the path it was read from and its source.
    pub(crate) files: &'static [(&'static str, &'static str, &'static str)],
}

/// The Fluent files every module of the app contributed, modules in dependency
/// order. A module that has files but names no plugin is a problem of its own:
/// its files are never loaded without saying so.
pub(crate) fn plugin_sources(ctx: &AppContext) -> Result<Vec<PluginSource>, I18nError> {
    let mut sources = Vec::new();
    let mut problems = Vec::new();
    for contributed in ctx.contributions::<EmbeddedLocales>() {
        let Some(info) = ctx.plugin(contributed.module.id) else {
            problems.push(format!(
                "module `{}` has translations but does not name itself a plugin; call m.plugin(plugin_info!()) in its register",
                contributed.module.name
            ));
            continue;
        };
        sources.push(PluginSource {
            plugin: info.name,
            namespace: info.namespace,
            checked: is_community_plugin(ctx, contributed.module.id),
            files: contributed.value.files(),
        });
    }
    if problems.is_empty() {
        Ok(sources)
    } else {
        Err(I18nError::Files(problems))
    }
}

#[cfg(test)]
mod tests {
    use navree_config::{ConfigModule, ConfigSources};
    use navree_contracts::{EmbeddedLocales, ModuleBuilderLocalesExt};
    use navree_http::HttpModule;
    use navree_kernel::{Module, ModuleBuilder, Navree};

    use crate::{I18n, I18nModule};

    /// The messages of the zones, whose ids do not start with the namespace `i18n`.
    const ZONES: &[(&str, &str, &str)] = &[(
        "en",
        "locales/en/zones.ftl",
        "zone-ist = India Standard Time\n",
    )];

    /// A module of this package that names its plugin, as the package's own modules may, with
    /// messages outside the plugin's namespace.
    struct Zones;

    impl Module for Zones {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(navree_kernel::plugin_info!())
                .locales(EmbeddedLocales::from(ZONES));
        }
    }

    #[tokio::test]
    async fn a_first_party_plugins_ids_are_not_held_to_its_namespace_in_a_booted_app() {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        let shown = dir.path().to_string_lossy().replace('\\', "/");
        let sources = ConfigSources::new().set("i18n.dir", shown.as_str());
        // `zone-ist` does not start with `i18n-`: a plugin that is not the framework's own
        // would fail the boot for it.
        let app = Navree::app()
            .module(ConfigModule::for_root(sources))
            .module(HttpModule)
            .module(I18nModule::from_config())
            .module(Zones)
            .boot()
            .await
            .expect("boots");
        let i18n = app.ctx().get::<I18n>().await.expect("I18n");
        assert_eq!(
            i18n.t("en", "zone-ist", crate::args!()).unwrap(),
            "India Standard Time"
        );
        app.shutdown().await.unwrap();
    }
}
