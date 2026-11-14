//! The boot checks of plugins, over the module graph.

use crate::KernelError;
use crate::graph::Graph;
use crate::plugin::{
    FIRST_PARTY, PluginInfo, RESERVED_NAMESPACES, is_first_party, is_namespace, names_crate,
};

/// A module that names a plugin and passed the checks of its own plugin info.
struct Member<'g> {
    /// The module's name.
    module: &'static str,
    /// The module's full type path.
    type_name: &'static str,
    /// The plugin the module names.
    info: &'g PluginInfo,
}

impl Graph {
    /// Checks what the modules say about their plugins and records which
    /// plugins are first-party. Modules are read in import order; in this
    /// order:
    ///
    /// 1. a module that calls `m.plugin(..)` twice: `PluginTwice`;
    /// 2. a namespace outside the grammar: `InvalidNamespace`, and the module
    ///    is left out below;
    /// 3. a module whose type names no path of the plugin's crate:
    ///    `PluginOfAnotherCrate`, and the module is left out below;
    /// 4. modules grouped by plugin name, the first module giving the plugin's
    ///    info: a later module with another crate is `PluginOfAnotherCrate`;
    ///    else a later one at another version is one `PluginVersions`; else a
    ///    later one with any other field is one `PluginInfoDiffers`;
    /// 5. first-party plugins (the kernel's list, its version, every module a
    ///    type of the listed crate) go into `first_party`;
    /// 6. a plugin that is not first-party in a reserved namespace:
    ///    `NamespaceReserved`;
    /// 7. a second plugin on a namespace already used: `NamespaceTaken`.
    pub(crate) fn check_plugins(&mut self, errors: &mut Vec<KernelError>) {
        let version = env!("CARGO_PKG_VERSION");
        // Each plugin's modules, plugins in first-seen order.
        let mut plugins: Vec<Vec<Member<'_>>> = Vec::new();
        for m in self.import_order() {
            let module = &self.modules[m];
            if module.plugin_twice {
                errors.push(KernelError::PluginTwice {
                    module: module.name,
                });
            }
            let Some(info) = &module.plugin else {
                continue;
            };
            if !is_namespace(info.namespace) {
                errors.push(KernelError::InvalidNamespace {
                    module: module.name,
                    plugin: info.name,
                    namespace: info.namespace,
                });
                continue;
            }
            if !names_crate(module.id.type_name(), info.crate_name) {
                errors.push(KernelError::PluginOfAnotherCrate {
                    module: module.name,
                    plugin: info.name,
                });
                continue;
            }
            let member = Member {
                module: module.name,
                type_name: module.id.type_name(),
                info,
            };
            match plugins
                .iter()
                .position(|members| members[0].info.name == info.name)
            {
                Some(index) => plugins[index].push(member),
                None => plugins.push(vec![member]),
            }
        }

        let mut first_party = Vec::new();
        // (namespace, plugin) for every namespace a plugin already uses.
        let mut namespaces: Vec<(&'static str, &'static str)> = Vec::new();
        for members in &plugins {
            let (first, others) = (&members[0], &members[1..]);
            let plugin = first.info.name;

            let mut one_crate = true;
            for member in others
                .iter()
                .filter(|member| member.info.crate_name != first.info.crate_name)
            {
                one_crate = false;
                errors.push(KernelError::PluginOfAnotherCrate {
                    module: member.module,
                    plugin,
                });
            }
            let other_version = others
                .iter()
                .find(|member| member.info.version != first.info.version);
            let described_otherwise = others.iter().find(|member| member.info != first.info);
            match (one_crate, other_version, described_otherwise) {
                (true, Some(second), _) => errors.push(KernelError::PluginVersions {
                    plugin,
                    first: first.info.version,
                    first_module: first.module,
                    second: second.info.version,
                    second_module: second.module,
                }),
                (true, None, Some(second)) => errors.push(KernelError::PluginInfoDiffers {
                    plugin,
                    first_module: first.module,
                    second_module: second.module,
                }),
                // Another crate is the error already; or every module agrees.
                _ => {}
            }

            let type_names: Vec<&str> = members.iter().map(|member| member.type_name).collect();
            if is_first_party(first.info, &type_names, FIRST_PARTY, version) {
                first_party.push(plugin);
            } else if RESERVED_NAMESPACES.contains(&first.info.namespace) {
                errors.push(KernelError::NamespaceReserved {
                    plugin,
                    namespace: first.info.namespace,
                });
            }

            let namespace = first.info.namespace;
            let taken = namespaces
                .iter()
                .find(|(used, _)| *used == namespace)
                .map(|&(_, taken_by)| taken_by);
            match taken {
                Some(taken_by) => errors.push(KernelError::NamespaceTaken {
                    namespace,
                    first: taken_by,
                    second: plugin,
                }),
                None => namespaces.push((namespace, plugin)),
            }
        }
        self.first_party.extend(first_party);
    }
}
