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
