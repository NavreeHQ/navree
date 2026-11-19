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

/// Whether `module` acts for a plugin ([`acts_for`]) that the kernel did not
/// find first-party: `true` for a community plugin's naming module, an unnamed
/// module of its crate, a generic module imported only by its modules, and a
/// plain module of its second crate imported only by its modules. `false` for
/// an app module, a first-party plugin's module, the app root, and an id not in
/// the app. Not public API; Navree's own crates use it.
pub fn acts_for_community_plugin(ctx: &AppContext, module: ModuleId) -> bool {
    acts_for(ctx, module).is_some_and(|info| !ctx.inner.graph.first_party.contains(info.name))
}

/// The plugin `module` acts for, first-party or not: the plugin it names;
/// else the plugin of the crate its type path is in, up to its first angle
/// bracket and read through a reference, a `Box` and a `Pin`, and a trait
/// object as its trait (`&dyn Part` is `Part`; the longest crate name wins,
/// then the plugin first in import order); else, for a module with generic
/// arguments that no module of the app imports directly, the plugin among its
/// importers' whose crate its arguments name, else the first of them in import
/// order that is not first-party, else the first; else, for a plain module of
/// no plugin's crate and of no framework crate whose direct importers all act
/// for plugins that are not first-party, the first of those plugins in import
/// order. When its type path gives a first-party plugin and its importers one
/// that is not, it acts for the one that is not. A module's own type (from the
/// module value the app or its importer passed) decides before the type its id
/// names, and a first-party plugin is matched by the module's own type only,
/// never by its id alone. `None` for a module of the app, the app root and an
/// id not in the app. `plugins` lists a plugin's modules by it. Not public
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

/// Why `module` acts for the plugin [`acts_for`] gives, as the clause the
/// kernel's own refusals carry after the module's name: empty when `module`
/// names the plugin itself or acts for no plugin, ` (a module of the plugin's
/// crate ..)` when its type path places it in the plugin's crate, or ` (only
/// the plugin's modules import it)` when its importers place it there. Not
/// public API; Navree's own crates use it.
pub fn acts_for_reason(ctx: &AppContext, module: ModuleId) -> String {
    let graph = &ctx.inner.graph;
    let Some(index) = graph.module_of(module) else {
        return String::new();
    };
    let Some(first) = graph.acts_for.get(index).copied().flatten() else {
        return String::new();
    };
    let Some(info) = graph.modules[first].plugin.as_ref() else {
        return String::new();
    };
    graph.acts_for_reason(index, info)
}

/// Whether `module` is a module of the app: it is in the app and acts for no
/// plugin (`acts_for`). Not public API; Navree's own crates use it.
pub fn is_app_module(ctx: &AppContext, module: ModuleId) -> bool {
    let graph = &ctx.inner.graph;
    graph
        .module_of(module)
        .is_some_and(|index| graph.acts_for.get(index).copied().flatten().is_none())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CommandSpec, Module, ModuleBuilder, Navree};

    mod navree_plugin_khata {
        use crate::{CommandSpec, Module, ModuleBuilder, PluginInfo};

        pub const KHATA: PluginInfo =
            PluginInfo::new("navree-plugin-khata", module_path!(), "khata", "1.0.0");

        pub struct KhataModule(pub fn(&mut ModuleBuilder));

        impl Module for KhataModule {
            fn register(&self, m: &mut ModuleBuilder) {
                m.plugin(KHATA);
                (self.0)(m);
            }
        }

        pub struct KhataHelper(pub Option<&'static str>);

        impl Module for KhataHelper {
            fn register(&self, m: &mut ModuleBuilder) {
                if let Some(name) = self.0 {
                    m.command(CommandSpec::new(name, "Helper", |_, _| async { Ok(()) }));
                }
            }
        }
    }

    mod navree_plugin_khata_core {
        use std::marker::PhantomData;

        use crate::{CommandSpec, Module, ModuleBuilder};

        pub struct KhataGeneric<T>(pub Option<&'static str>, pub PhantomData<T>);

        impl<T: Send + Sync + 'static> Module for KhataGeneric<T> {
            fn register(&self, m: &mut ModuleBuilder) {
                if let Some(name) = self.0 {
                    m.command(CommandSpec::new(name, "Core", |_, _| async { Ok(()) }));
                }
            }
        }
    }

    struct AppModule;
    impl Module for AppModule {
        fn register(&self, _: &mut ModuleBuilder) {}
    }

    struct Unwired;

    #[tokio::test]
    async fn the_reason_a_module_acts_for_a_plugin_is_the_clause_the_kernels_refusals_give() {
        use std::marker::PhantomData;

        use navree_plugin_khata::{KHATA, KhataHelper, KhataModule};
        use navree_plugin_khata_core::KhataGeneric;

        let app = Navree::app()
            .module(KhataModule(|m| {
                m.import(KhataHelper(None))
                    .import(KhataGeneric::<KhataModule>(None, PhantomData));
            }))
            .module(AppModule)
            .boot()
            .await
            .expect("boots");
        let ctx = app.ctx();

        let naming = ModuleId::of::<KhataModule>();
        let helper = ModuleId::of::<KhataHelper>();
        let second = ModuleId::of::<KhataGeneric<KhataModule>>();
        let app_mod = ModuleId::of::<AppModule>();
        let missing = ModuleId::of::<Unwired>();

        assert_eq!(acts_for_reason(ctx, naming), "");
        assert_eq!(
            acts_for_reason(ctx, helper),
            format!(" (a module of the plugin's crate `{}`)", KHATA.crate_name)
        );
        assert_eq!(
            acts_for_reason(ctx, second),
            " (only the plugin's modules import it)"
        );
        assert_eq!(acts_for_reason(ctx, app_mod), "");
        assert_eq!(acts_for_reason(ctx, missing), "");
        assert!(acts_for_community_plugin(ctx, naming));
        assert!(acts_for_community_plugin(ctx, helper));
        assert!(acts_for_community_plugin(ctx, second));
        assert!(!is_community_plugin(ctx, helper));
        assert!(!is_community_plugin(ctx, second));
        assert!(!acts_for_community_plugin(ctx, app_mod));
        assert!(!acts_for_community_plugin(ctx, missing));

        // Each non-empty or empty clause matches byte for byte what the kernel's refusals put
        // after `in \`<module>\``.
        let direct = Navree::app()
            .module(KhataModule(|m| {
                m.command(CommandSpec::new("check", "Plugin", |_, _| async { Ok(()) }));
            }))
            .validate()
            .expect_err("refused")
            .to_string();
        assert!(
            direct.contains(&format!(
                "in `KhataModule`{},",
                acts_for_reason(ctx, naming)
            )),
            "{direct}"
        );

        let in_crate = Navree::app()
            .module(KhataModule(|m| {
                m.import(KhataHelper(Some("check")));
            }))
            .validate()
            .expect_err("refused")
            .to_string();
        assert!(
            in_crate.contains(&format!(
                "in `KhataHelper`{},",
                acts_for_reason(ctx, helper)
            )),
            "{in_crate}"
        );

        let by_import = Navree::app()
            .module(KhataModule(|m| {
                m.import(KhataGeneric::<KhataModule>(Some("check"), PhantomData));
            }))
            .validate()
            .expect_err("refused")
            .to_string();
        assert!(
            by_import.contains(&format!(
                "in `{}`{},",
                second.name(),
                acts_for_reason(ctx, second)
            )),
            "{by_import}"
        );
    }
}
