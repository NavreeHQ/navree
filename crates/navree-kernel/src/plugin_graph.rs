//! The boot checks of plugins, and the plugins report, over the module graph.

use std::any::TypeId;
use std::collections::BTreeMap;

use crate::KernelError;
use crate::graph::{Graph, ModuleNode, ROOT};
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
    /// The module's own full type path (the type of the value whose
    /// `register` ran, not its id's): what the first-party rule reads.
    type_name: &'static str,
    /// The plugin the module names.
    info: &'g PluginInfo,
}

/// A plugin as the attribution reads it: one per plugin of step 4 of
/// [`Graph::check_plugins`], in that order.
struct PluginGroup {
    /// The index of the plugin's first module, whose info is the plugin's.
    first: usize,
    /// The crate the plugin names.
    crate_name: &'static str,
    /// Whether the kernel finds the plugin first-party (step 5).
    first_party: bool,
    /// The modules that name the plugin, by index (but those left out in
    /// steps 2 and 3).
    members: Vec<usize>,
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
    ///    type of the listed crate: the module's own type, which its id need
    ///    not name) go into `first_party`;
    /// 6. a plugin that is not first-party in a reserved namespace:
    ///    `NamespaceReserved`;
    /// 7. a second plugin on a namespace already used: `NamespaceTaken`;
    /// 8. files to publish ([`Publishable`]) from a module that names no
    ///    plugin: `PublishableOutsidePlugin`; a file of a plugin whose path, or
    ///    whose place in the app, is not a safe relative path (or, under
    ///    `locales`, does not start with a locale): `UnsafePublishPath`; per
    ///    plugin and tag, the first safe path that differs from an earlier one
    ///    only in case, else the first that another path needs as a folder
    ///    (`admin` beside `admin/edit.html`): `UnsafePublishPath`;
    /// 9. which plugin each module acts for (the plugin it names; else the
    ///    plugin of its type's crate; else, for a generic module that only
    ///    modules acting for plugins import, one of their plugins, one that is
    ///    not first-party before one that is), recorded for `acts_for` and
    ///    `plugins`;
    /// 10. a configured instance (`for_root`, `from_config`) imported by a
    ///     module that acts for a plugin that is not first-party, of a module
    ///     whose type does not name the plugin's crate:
    ///     `PluginConfiguresModule` (a second configured instance of a module
    ///     reads the same way, see `configured_twice`).
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
                type_name: module.type_name,
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
        // One group per plugin, in the order of `plugins`, for step 9.
        let mut groups: Vec<PluginGroup> = Vec::with_capacity(plugins.len());
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
            let is_first = is_first_party(first.info, &type_names, FIRST_PARTY, version);
            if is_first {
                first_party.push(plugin);
            } else if RESERVED_NAMESPACES.contains(&first.info.namespace) {
                errors.push(KernelError::NamespaceReserved {
                    plugin,
                    namespace: first.info.namespace,
                });
            }
            groups.push(PluginGroup {
                first: first.index,
                crate_name: first.info.crate_name,
                first_party: is_first,
                members: members.iter().map(|member| member.index).collect(),
            });

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

        // Step 9.
        self.acts_for = self.attribution(&groups);
        self.first_party.extend(first_party);

        // Step 10.
        for m in self.import_order() {
            let module = &self.modules[m];
            let Some(importer) = module.configured_by else {
                continue;
            };
            let Some(info) = self.community_plugin(importer) else {
                continue;
            };
            if !names_crate(module.id.type_name(), info.crate_name) {
                errors.push(KernelError::PluginConfiguresModule {
                    plugin: info.name,
                    module: module.name,
                    importer: self.modules[importer].name,
                });
            }
        }
    }

    /// The plugin module `m` acts for (step 9 of
    /// [`check_plugins`](Graph::check_plugins)), when the kernel does not find
    /// it first-party.
    pub(crate) fn community_plugin(&self, m: usize) -> Option<&PluginInfo> {
        let first = self.acts_for.get(m).copied().flatten()?;
        let info = self.modules[first].plugin.as_ref()?;
        (!self.first_party.contains(info.name)).then_some(info)
    }

    /// Which plugin each module acts for, by module index: the `first` of its
    /// plugin's group, or `None`.
    ///
    /// A module acts for the plugin it names. Else, for the plugin of the
    /// crate its type path is in, up to its first `<`: the type of the module
    /// value the app or its importer passed, and the type its id names, either
    /// one (the longest crate name, then the plugin first in import order; a
    /// plugin that is not first-party before one that is). Else, for a module
    /// with generic arguments whose direct importers all act for plugins: the
    /// one of theirs whose crate its arguments name, else the first of them in
    /// import order that is not first-party, else the first. When its type
    /// path gives a first-party plugin and its importers one that is not, it
    /// acts for the one that is not. The root acts for nothing.
    fn attribution(&self, groups: &[PluginGroup]) -> Vec<Option<usize>> {
        let mut acts_for: Vec<Option<usize>> = vec![None; self.modules.len()];
        if groups.is_empty() {
            return acts_for;
        }
        let order = self.import_order();
        // The modules that import each module directly, in import order.
        let mut importers: Vec<Vec<usize>> = vec![Vec::new(); self.modules.len()];
        for &m in &order {
            for &import in &self.modules[m].imports {
                importers[import].push(m);
            }
        }
        // The plugins by crate: the longest crate name first, then in the
        // order of `groups` (the sort is stable).
        let mut by_crate: Vec<&PluginGroup> = groups.iter().collect();
        by_crate.sort_by_key(|group| std::cmp::Reverse(group.crate_name.len()));
        let group = |first: usize| groups.iter().find(|group| group.first == first);
        let first_party = |first: usize| group(first).is_some_and(|group| group.first_party);
        // The plugin of the crate `type_name` is in, up to its first `<`.
        let of_crate = |type_name: &str| {
            let outer = type_name
                .split_once('<')
                .map_or(type_name, |(outer, _)| outer);
            by_crate
                .iter()
                .find(|group| {
                    outer
                        .strip_prefix(group.crate_name)
                        .is_some_and(|rest| rest.starts_with("::"))
                })
                .map(|group| group.first)
        };
        // Reverse import order is a topological order (the graph has no
        // import cycle by now): every importer is decided before the modules
        // it imports.
        for &m in order.iter().rev() {
            if m == ROOT {
                continue;
            }
            if let Some(named) = groups.iter().find(|group| group.members.contains(&m)) {
                acts_for[m] = Some(named.first);
                continue;
            }
            let own = self.modules[m].type_name;
            let id = self.modules[m].id.type_name();
            let by_type = match (of_crate(own), of_crate(id)) {
                (Some(by_own), Some(by_id)) if first_party(by_own) && !first_party(by_id) => {
                    Some(by_id)
                }
                (Some(by_own), _) => Some(by_own),
                (None, by_id) => by_id,
            };
            // Some only when every direct importer acts for a plugin.
            let generic = own.contains('<') || id.contains('<');
            let importing: Option<Vec<usize>> = if generic && !importers[m].is_empty() {
                importers[m].iter().map(|&i| acts_for[i]).collect()
            } else {
                None
            };
            let by_importers = importing.and_then(|firsts| {
                firsts
                    .iter()
                    .copied()
                    .find(|&f| {
                        group(f).is_some_and(|group| {
                            names_crate(own, group.crate_name) || names_crate(id, group.crate_name)
                        })
                    })
                    .or_else(|| firsts.iter().copied().find(|&f| !first_party(f)))
                    .or_else(|| firsts.first().copied())
            });
            acts_for[m] = match (by_type, by_importers) {
                (Some(by_type), Some(by_importers))
                    if first_party(by_type) && !first_party(by_importers) =>
                {
                    Some(by_importers)
                }
                (Some(by_type), _) => Some(by_type),
                (None, by_importers) => by_importers,
            };
        }
        acts_for
    }

    /// Every plugin of the graph and what its modules add: every module that
    /// acts for a plugin (step 9 of [`check_plugins`](Graph::check_plugins)),
    /// in dependency order, under that plugin, whose info is its first
    /// module's; plugins by name. Nothing is built.
    pub(crate) fn plugins_report(&self) -> PluginsReport {
        let mut plugins: Vec<PluginReport> = Vec::new();
        for &m in &self.order {
            let Some(first) = self.acts_for.get(m).copied().flatten() else {
                continue;
            };
            let Some(info) = &self.modules[first].plugin else {
                continue;
            };
            let module = &self.modules[m];
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

#[cfg(test)]
mod tests {
    use std::marker::PhantomData;

    use super::*;
    use crate::{Module, ModuleBuilder, ModuleId};

    /// A module of the first-party plugin of these tests, whose crate is this module.
    struct FirstPartyModule;
    impl Module for FirstPartyModule {
        fn register(&self, _: &mut ModuleBuilder) {}
    }

    /// A generic module of this module's crate, standing for one of a first-party package.
    struct Wrap<T>(PhantomData<fn() -> T>);
    impl<T: 'static> Module for Wrap<T> {
        fn register(&self, _: &mut ModuleBuilder) {}
    }

    /// A type of no plugin's crate.
    struct Shared;

    /// The community plugin's module: it imports `Wrap<Shared>`, which no module of the app
    /// imports.
    struct Community;
    impl Module for Community {
        fn register(&self, m: &mut ModuleBuilder) {
            m.import(Wrap::<Shared>(PhantomData));
        }
    }

    #[test]
    fn a_generic_module_only_plugins_import_is_never_the_apps() {
        let roots: Vec<Box<dyn Module>> = vec![Box::new(FirstPartyModule), Box::new(Community)];
        let graph = Graph::build(&roots, &[]).expect("valid");
        let index = |id: ModuleId| graph.module_of(id).expect("in the graph");
        let first_party = index(ModuleId::of::<FirstPartyModule>());
        let community = index(ModuleId::of::<Community>());
        let wrap = index(ModuleId::of::<Wrap<Shared>>());
        // `Wrap`'s type is of the first-party plugin's crate, and its one importer acts for
        // a plugin that is not first-party: it acts for that one.
        let groups = |community_is_first_party: bool| {
            [
                PluginGroup {
                    first: first_party,
                    crate_name: module_path!(),
                    first_party: true,
                    members: vec![first_party],
                },
                PluginGroup {
                    first: community,
                    crate_name: "navree_plugin_khata",
                    first_party: community_is_first_party,
                    members: vec![community],
                },
            ]
        };
        let acts_for = graph.attribution(&groups(false));
        assert_eq!(acts_for[wrap], Some(community));
        assert_eq!(acts_for[first_party], Some(first_party));
        assert_eq!(acts_for[community], Some(community));
        assert_eq!(acts_for[ROOT], None, "the root acts for nothing");
        // With both plugins first-party, the plugin of its crate.
        assert_eq!(graph.attribution(&groups(true))[wrap], Some(first_party));
    }

    /// A module that names the plugin `.0`.
    struct Names(PluginInfo);
    impl Module for Names {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(self.0.clone());
        }
    }

    #[test]
    fn a_plugin_is_first_party_by_its_modules_own_types_and_keeps_its_reserved_namespace() {
        let version = env!("CARGO_PKG_VERSION");
        let notes = PluginInfo::new("navree-plugin-flags", "navree_kernel", "flag_notes", version);
        let roots: Vec<Box<dyn Module>> = vec![Box::new(Names(notes))];
        let mut graph = Graph::build(&roots, &[]).expect("a community plugin in its own namespace");
        let m = graph
            .module_of(ModuleId::of::<Names>())
            .expect("in the graph");
        // The module names navree-flags, in its reserved namespace. Its own type is not of
        // navree-flags' crate, whatever crate its id is in: not first-party, and refused.
        graph.modules[m].plugin = Some(PluginInfo::new(
            "navree-flags",
            "navree_kernel",
            "flags",
            version,
        ));
        let mut errors = Vec::new();
        graph.check_plugins(&mut errors);
        assert!(
            matches!(
                errors.as_slice(),
                [KernelError::NamespaceReserved {
                    plugin: "navree-flags",
                    namespace: "flags"
                }]
            ),
            "{errors:?}"
        );
        // Read as a module of navree-flags' own crate: first-party, and the namespace is its.
        graph.modules[m].type_name = "navree_flags::module::FlagsModule";
        let mut errors = Vec::new();
        graph.check_plugins(&mut errors);
        assert!(errors.is_empty(), "{errors:?}");
        assert!(graph.first_party.contains("navree-flags"));
        assert_eq!(graph.acts_for[m], Some(m));
    }
}
