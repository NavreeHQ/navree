//! The boot checks of plugins, and the plugins report, over the module graph.

use std::any::TypeId;
use std::collections::BTreeMap;

use crate::KernelError;
use crate::graph::{Graph, ModuleNode};
use crate::plugin::{
    FIRST_PARTY, PluginInfo, PublishTag, Publishable, RESERVED_NAMESPACES, destination,
    is_first_party, is_namespace, names_crate,
};
use crate::publish::{
    first_case_collision, first_file_and_folder, is_locale_path, is_safe_relative_path,
};
use crate::report::{PluginModuleReport, PluginReport, PluginsReport};

/// A module that names a plugin and passed the checks of its own plugin info.
struct Member<'g> {
    /// The module's index in the graph.
    index: usize,
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
    /// 7. a second plugin on a namespace already used: `NamespaceTaken`;
    /// 8. files to publish ([`Publishable`]) from a module that names no
    ///    plugin: `PublishableOutsidePlugin`; a file of a plugin whose path, or
    ///    whose place in the app, is not a safe relative path (or, under
    ///    `locales`, does not start with a locale): `UnsafePublishPath`; per
    ///    plugin and tag, the first safe path that differs from an earlier one
    ///    only in case, else the first that another path needs as a folder
    ///    (`admin` beside `admin/edit.html`): `UnsafePublishPath`.
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
                index: m,
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

        // The safe paths of each plugin's files, with their tags, in module
        // then registration order.
        let mut safe_paths: Vec<Vec<(PublishTag, &str)>> = vec![Vec::new(); plugins.len()];
        for m in self.import_order() {
            let module = &self.modules[m];
            let files: Vec<&Publishable> = shipped(module).collect();
            if files.is_empty() {
                continue;
            }
            if module.plugin.is_none() {
                errors.push(KernelError::PublishableOutsidePlugin {
                    module: module.name,
                });
                continue;
            }
            // A module left out in steps 2 and 3 is left out here too.
            let Some(group) = plugins
                .iter()
                .position(|members| members.iter().any(|member| member.index == m))
            else {
                continue;
            };
            // The plugin's info is its first module's (step 4).
            let info = plugins[group][0].info;
            for publishable in files {
                let tag = publishable.tag();
                for (path, _) in publishable.files() {
                    let safe = is_safe_relative_path(path)
                        && (tag != PublishTag::Locales || is_locale_path(path))
                        && is_safe_relative_path(&destination(tag, info.namespace, path));
                    if safe {
                        safe_paths[group].push((tag, path));
                    } else {
                        errors.push(KernelError::UnsafePublishPath {
                            plugin: info.name,
                            tag: tag.as_str(),
                            path: path.to_owned(),
                        });
                    }
                }
            }
        }
        for (members, paths) in plugins.iter().zip(&safe_paths) {
            for &tag in PublishTag::ALL {
                let of_tag: Vec<&str> = paths
                    .iter()
                    .filter(|(of, _)| *of == tag)
                    .map(|&(_, path)| path)
                    .collect();
                let conflict = first_case_collision(of_tag.iter().copied())
                    .or_else(|| first_file_and_folder(&of_tag).map(|(file, _)| file));
                if let Some(path) = conflict {
                    errors.push(KernelError::UnsafePublishPath {
                        plugin: members[0].info.name,
                        tag: tag.as_str(),
                        path: path.to_owned(),
                    });
                }
            }
        }

        self.first_party.extend(first_party);
    }

    /// Every plugin of the graph and what its modules add: modules in
    /// dependency order, grouped by plugin name, a plugin's info taken from
    /// its first module; plugins by name. Nothing is built.
    pub(crate) fn plugins_report(&self) -> PluginsReport {
        let mut plugins: Vec<PluginReport> = Vec::new();
        for &m in &self.order {
            let module = &self.modules[m];
            let Some(info) = &module.plugin else {
                continue;
            };
            let report = PluginModuleReport {
                id: module.id,
                commands: module.commands.iter().map(|command| command.name).collect(),
                contributions: contribution_counts(module),
                publishable: publishable_counts(module),
            };
            match plugins
                .iter_mut()
                .find(|plugin| plugin.info.name == info.name)
            {
                Some(plugin) => plugin.modules.push(report),
                None => plugins.push(PluginReport {
                    info: info.clone(),
                    first_party: self.first_party.contains(info.name),
                    modules: vec![report],
                }),
            }
        }
        plugins.sort_by_key(|plugin| plugin.info.name);
        PluginsReport { plugins }
    }
}

/// The files `module` ships to publish, in registration order.
pub(crate) fn shipped(module: &ModuleNode) -> impl Iterator<Item = &Publishable> {
    module
        .contributions
        .iter()
        .filter(|contribution| contribution.type_id == TypeId::of::<Publishable>())
        .filter_map(|contribution| contribution.value.downcast_ref::<Publishable>())
}

/// How many values of each type `module` contributes, by type name, in name
/// order.
fn contribution_counts(module: &ModuleNode) -> Vec<(&'static str, usize)> {
    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    for contribution in &module.contributions {
        *counts.entry(contribution.type_name).or_default() += 1;
    }
    counts.into_iter().collect()
}

/// How many files `module` ships per tag, in [`PublishTag::ALL`] order; a tag
/// without files is left out.
fn publishable_counts(module: &ModuleNode) -> Vec<(PublishTag, usize)> {
    PublishTag::ALL
        .iter()
        .filter_map(|&tag| {
            let count: usize = shipped(module)
                .filter(|files| files.tag() == tag)
                .map(|files| files.files().count())
                .sum();
            (count > 0).then_some((tag, count))
        })
        .collect()
}
