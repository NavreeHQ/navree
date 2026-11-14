//! What the framework's crates ask the module graph when two modules give one
//! name to two things. Not public API.

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
