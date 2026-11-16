//! What the framework's crates ask the module graph when two modules give one
//! name to two things. Not public API.

use crate::plugin::names_crate;
use crate::{AppContext, ModuleId};

/// Whether `importer` imports `imported`, directly or through the modules it
/// imports. `false` for the same module, for an id that is not in the app, and
/// for the other way round. Not public API; Navree's own crates use it.
pub fn imports(ctx: &AppContext, importer: ModuleId, imported: ModuleId) -> bool {
    let graph = &ctx.inner.graph;
    let (Some(from), Some(target)) = (graph.module_of(importer), graph.module_of(imported)) else {
        return false;
    };
    if from == target {
        return false;
    }
    let mut visited = vec![false; graph.modules.len()];
    visited[from] = true;
    let mut pending = vec![from];
    while let Some(module) = pending.pop() {
        for &next in &graph.modules[module].imports {
            if next == target {
                return true;
            }
            if !visited[next] {
                visited[next] = true;
                pending.push(next);
            }
        }
    }
    false
}

/// Whether `module` names itself a plugin and the kernel did not find that
/// plugin first-party. `false` for a module that names no plugin and for an id
/// that is not in the app. Not public API; Navree's own crates use it.
pub fn is_community_plugin(ctx: &AppContext, module: ModuleId) -> bool {
    ctx.plugin(module)
        .is_some_and(|info| !ctx.inner.graph.first_party.contains(info.name))
}

/// Whether `module` is a module of the app itself, as far as the kernel can
/// tell: it is in the app, names no plugin, and its type is in no crate that a
/// plugin of the app names. A module of a plugin's own crate that does not call
/// `m.plugin` is not the app's, and neither is a module of a first-party
/// plugin. The kernel records no crate for the app, so a module of a framework
/// package that names no plugin counts as the app's, and so does a module of a
/// plugin that sits in a crate other than the one its plugin names. Not public
/// API; Navree's own crates use it.
pub fn is_app_module(ctx: &AppContext, module: ModuleId) -> bool {
    let graph = &ctx.inner.graph;
    let Some(index) = graph.module_of(module) else {
        return false;
    };
    graph.modules[index].plugin.is_none()
        && !graph
            .modules
            .iter()
            .filter_map(|node| node.plugin.as_ref())
            .any(|plugin| names_crate(module.type_name(), plugin.crate_name))
}
