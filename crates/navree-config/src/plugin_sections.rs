//! The configuration sections a community plugin reads outside its
//! namespace, which `ConfigModule` warns about at boot.

use navree_kernel::AppContext;

use crate::registry::SectionRegistration;

/// For each community plugin (one the kernel does not find first-party),
/// each section one of its modules registers whose key is neither the
/// plugin's namespace nor below it: the warning's text, in module then
/// registration order. A claimed key (`m.config_claim`, such as a billing
/// provider's) is not a section and never counts; a first-party plugin and a
/// module that names no plugin are never warned about.
pub(crate) fn outside_namespace(ctx: &AppContext) -> Vec<String> {
    let plugins = ctx.plugins().plugins;
    let mut messages = Vec::new();
    for contributed in ctx.contributions::<SectionRegistration>() {
        let module = contributed.module.id;
        let Some(plugin) = plugins.iter().find(|plugin| {
            !plugin.first_party && plugin.modules.iter().any(|member| member.id == module)
        }) else {
            continue;
        };
        let namespace = plugin.info.namespace;
        let key = contributed.value.key();
        let inside = key == namespace || key.starts_with(&format!("{namespace}."));
        if !inside {
            messages.push(format!(
                "plugin `{}` reads the configuration section `{key}`, outside its namespace `{namespace}`",
                plugin.info.name
            ));
        }
    }
    messages
}
