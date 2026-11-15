//! What the framework's crates ask the module graph when two modules give one
//! name to two things. Not public API.

use crate::{AppContext, ModuleId, PluginInfo};

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

/// The plugin `module` acts for, first-party or not: the plugin it names;
/// else the plugin of the crate its type path is in, up to its first `<`
/// (the longest crate name, then the one first in import order); else, for a
/// module with generic arguments that no module of the app imports directly,
/// the plugin among its importers' whose crate its arguments name, else the
/// first of them in import order that is not first-party, else the first.
/// When its type path gives a first-party plugin and its importers one that is
/// not, the one that is not. A module's type path is read from the module
/// value the app or its importer passed and from the type its id names: either
/// one may name the plugin. `None` for a module of the app, the app root and
/// an id not in the app. `plugins` lists a plugin's modules by it. Not public
/// API; Navree's own crates use it.
pub fn acts_for(ctx: &AppContext, module: ModuleId) -> Option<&PluginInfo> {
    let graph = &ctx.inner.graph;
    let first = graph
        .acts_for
        .get(graph.module_of(module)?)
        .copied()
        .flatten()?;
    graph.modules[first].plugin.as_ref()
}

/// Whether `module` is a module of the app: it is in the app and acts for no
/// plugin (`acts_for`). Not public API; Navree's own crates use it.
pub fn is_app_module(ctx: &AppContext, module: ModuleId) -> bool {
    let graph = &ctx.inner.graph;
    graph
        .module_of(module)
        .is_some_and(|index| graph.acts_for.get(index).copied().flatten().is_none())
}
