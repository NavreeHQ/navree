//! The boot checks of plugins, and the plugins report, over the module graph.

use std::any::TypeId;
use std::collections::BTreeMap;

use crate::command::FRAMEWORK_COMMANDS;
use crate::graph::{Graph, ModuleNode, ROOT};
use crate::plugin::{
    APP_ONLY_PROVIDERS, FIRST_PARTY, MODEL_HOOK_KEY, PluginInfo, PublishTag, Publishable,
    RESERVED_NAMESPACES, app_only_role, destination, is_first_party, is_framework_crate,
    is_namespace, module_path, names_crate,
};
use crate::publish::{
    first_case_collision, first_file_and_folder, is_locale_path, is_safe_relative_path,
};
use crate::report::{PluginModuleReport, PluginReport, PluginsReport};
use crate::{KernelError, ProviderKey};

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
    /// 3. a module whose type names no path of the plugin's crate (neither its
    ///    own type nor the type its id names): `PluginOfAnotherCrate`, and the
    ///    module is left out below;
    /// 4. modules grouped by plugin name, the first module giving the plugin's
    ///    info: a later module with another crate is `PluginOfAnotherCrate`;
    ///    else a later one at another version is one `PluginVersions`; else a
    ///    later one with any other field is one `PluginInfoDiffers`;
    /// 5. first-party plugins (the kernel's list, the listed crate named as
    ///    the plugin's, its version, every module a type of the listed crate:
    ///    the module's own type, which its id need not name) go into
    ///    `first_party`;
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
    ///    not first-party before one that is; else, for a plain module of no
    ///    plugin's crate and of no framework crate that only modules acting for
    ///    plugins that are not first-party import, the first of their plugins),
    ///    recorded for `acts_for` and `plugins`;
    /// 10. a configured instance (`for_root`, `from_config`) imported by a
    ///     module that acts for a plugin that is not first-party, of a module
    ///     whose type does not name the plugin's crate:
    ///     `PluginConfiguresModule` (a second configured instance of a module
    ///     reads the same way, see `configured_twice`), and that module is left
    ///     out of steps 11 to 14;
    /// 11. a module that acts for a plugin that is not first-party and
    ///     registers a provider (value, factory or multi-binding) whose key is
    ///     on [`APP_ONLY_PROVIDERS`]: one `PluginProvidesAppOnly` per module and
    ///     type name;
    /// 12. such a module registering a provider keyed `dyn ModelHook<M>` of
    ///     navree-orm ([`MODEL_HOOK_KEY`]) whose model `M` is not a type of the
    ///     plugin's crate: one `PluginHooksModel` per module and model;
    /// 13. such a module registering a command with the name of one of the
    ///     framework's own ([`FRAMEWORK_COMMANDS`]): `PluginCommand`;
    /// 14. two modules each registering and exporting one non-multi provider
    ///     key (not on [`APP_ONLY_PROVIDERS`]), when at least one of them acts
    ///     for a plugin that is not first-party: `PluginExportsProvider`.
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
            if !names_crate(module.type_name, info.crate_name)
                && !names_crate(module.id.type_name(), info.crate_name)
            {
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

        // Step 10. A module whose configured instance it refuses is left out of
        // steps 11 to 14: what it registers is the configured module's, and the
        // refusal already names it.
        let mut configured_refused: Vec<usize> = Vec::new();
        for m in self.import_order() {
            let module = &self.modules[m];
            let Some(importer) = module.configured_by else {
                continue;
            };
            let Some(info) = self.community_plugin(importer) else {
                continue;
            };
            if !names_crate(module.id.type_name(), info.crate_name) {
                configured_refused.push(m);
                errors.push(KernelError::PluginConfiguresModule {
                    plugin: info.name,
                    module: module.name,
                    importer: self.modules[importer].name,
                    reason: self.acts_for_reason(importer, info),
                });
            }
        }

        // Step 11.
        for m in self.import_order() {
            let Some(info) = self.community_plugin(m) else {
                continue;
            };
            if configured_refused.contains(&m) {
                continue;
            }
            let module = &self.modules[m];
            let mut refused: Vec<&'static str> = Vec::new();
            for &p in &module.providers {
                let provider = self.providers[p].def.key.type_name();
                if APP_ONLY_PROVIDERS.contains(&provider) && !refused.contains(&provider) {
                    refused.push(provider);
                    errors.push(KernelError::PluginProvidesAppOnly {
                        plugin: info.name,
                        provider,
                        role: app_only_role(provider),
                        module: module.name,
                        reason: self.acts_for_reason(m, info),
                    });
                }
            }
        }

        // Step 12.
        for m in self.import_order() {
            let Some(info) = self.community_plugin(m) else {
                continue;
            };
            if configured_refused.contains(&m) {
                continue;
            }
            let module = &self.modules[m];
            let mut refused: Vec<&'static str> = Vec::new();
            for &p in &module.providers {
                let Some(model) = self.providers[p]
                    .def
                    .key
                    .type_name()
                    .strip_prefix(MODEL_HOOK_KEY)
                    .and_then(|rest| rest.strip_suffix('>'))
                else {
                    continue;
                };
                let own = model
                    .strip_prefix(info.crate_name)
                    .is_some_and(|rest| rest.starts_with("::"));
                if !own && !refused.contains(&model) {
                    refused.push(model);
                    errors.push(KernelError::PluginHooksModel {
                        plugin: info.name,
                        model,
                        module: module.name,
                        reason: self.acts_for_reason(m, info),
                    });
                }
            }
        }

        // Step 13.
        for m in self.import_order() {
            let Some(info) = self.community_plugin(m) else {
                continue;
            };
            if configured_refused.contains(&m) {
                continue;
            }
            let module = &self.modules[m];
            for command in &module.commands {
                if FRAMEWORK_COMMANDS.binary_search(&command.name).is_ok() {
                    errors.push(KernelError::PluginCommand {
                        plugin: info.name,
                        command: command.name,
                        module: module.name,
                        reason: self.acts_for_reason(m, info),
                    });
                }
            }
        }

        // Step 14. Two modules each registering and exporting one non-multi
        // provider key leave that key out of `locator_map`, so `get` fails and
        // `try_get` answers `None`.
        let mut exported_by: Vec<(ProviderKey, Vec<usize>)> = Vec::new();
        for m in self.import_order() {
            if configured_refused.contains(&m) {
                continue;
            }
            let module = &self.modules[m];
            for &p in &module.providers {
                let def = &self.providers[p].def;
                if def.multi || APP_ONLY_PROVIDERS.contains(&def.key.type_name()) {
                    continue;
                }
                if !module.exports.contains(&def.key) {
                    continue;
                }
                match exported_by.iter_mut().find(|(key, _)| *key == def.key) {
                    Some((_, owners)) => {
                        if !owners.contains(&m) {
                            owners.push(m);
                        }
                    }
                    None => exported_by.push((def.key.clone(), vec![m])),
                }
            }
        }
        for (key, owners) in exported_by {
            let Some((&first, rest)) = owners.split_first() else {
                continue;
            };
            let mut first_reported = false;
            for &later in rest {
                match (self.community_plugin(later), self.community_plugin(first)) {
                    (Some(info), _) => errors.push(KernelError::PluginExportsProvider {
                        plugin: info.name,
                        provider: key.to_string(),
                        module: self.modules[later].name,
                        reason: self.acts_for_reason(later, info),
                        other: self.modules[first].name,
                    }),
                    (None, Some(info)) if !first_reported => {
                        first_reported = true;
                        errors.push(KernelError::PluginExportsProvider {
                            plugin: info.name,
                            provider: key.to_string(),
                            module: self.modules[first].name,
                            reason: self.acts_for_reason(first, info),
                            other: self.modules[later].name,
                        });
                    }
                    _ => {}
                }
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

    /// Why module `m` acts for the plugin `info`, for the text that refuses
    /// what it registers: nothing when it names the plugin; else that it is a
    /// module of the plugin's crate; else that only the plugin's modules
    /// import it. In parentheses after a space.
    pub(crate) fn acts_for_reason(&self, m: usize, info: &PluginInfo) -> String {
        let module = &self.modules[m];
        if module
            .plugin
            .as_ref()
            .is_some_and(|named| named.name == info.name)
        {
            return String::new();
        }
        let in_crate = |type_name: &str| {
            let read = module_path(type_name);
            let outer = read.split_once('<').map_or(read, |(outer, _)| outer);
            outer
                .strip_prefix(info.crate_name)
                .is_some_and(|rest| rest.starts_with("::"))
        };
        if in_crate(module.type_name)
            || (!self.first_party.contains(info.name) && in_crate(module.id.type_name()))
        {
            format!(" (a module of the plugin's crate `{}`)", info.crate_name)
        } else {
            String::from(" (only the plugin's modules import it)")
        }
    }

    /// Which plugin each module acts for, by module index: the `first` of its
    /// plugin's group, or `None`.
    ///
    /// A module acts for the plugin it names. Else, for the plugin of the
    /// crate its type path is in, up to its first `<` and read through a
    /// reference, a `Box` or a `Pin` (`&'static Helper` is `Helper`), a trait
    /// object as its trait (`&'static dyn Part` is `Part`): the type of the
    /// module value the app or its importer passed, else, for a plugin that is
    /// not first-party, the type its id names (a first-party plugin is never
    /// matched by an id alone). For each type, among plugins whose crates hold
    /// it, the longest crate name wins, then the plugin first in import order.
    /// When its own type gives a first-party plugin and its id one that is not,
    /// it is the one its id gives. Else, for a module with generic arguments
    /// whose direct importers all act for plugins: the one of theirs whose
    /// crate its arguments name, else the first of them in import order that is
    /// not first-party, else the first. Else, for a module that names no
    /// plugin, has no generic arguments, whose own type is in no plugin's crate
    /// and in no framework crate, and whose direct importers all act for
    /// plugins that are not first-party: the first of those plugins in import
    /// order. When its type path gives a first-party plugin and its importers
    /// one that is not, it acts for the one that is not. The root acts for
    /// nothing.
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
        let plugins: Vec<(&str, bool)> = groups
            .iter()
            .map(|group| (group.crate_name, group.first_party))
            .collect();
        let group = |first: usize| groups.iter().find(|group| group.first == first);
        let first_party = |first: usize| group(first).is_some_and(|group| group.first_party);
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
            let by_type = plugin_by_type(own, id, &plugins).map(|index| groups[index].first);
            let generic = module_path(own).contains('<') || module_path(id).contains('<');
            let by_importers = if generic && !importers[m].is_empty() {
                // Some only when every direct importer acts for a plugin.
                let importing: Option<Vec<usize>> =
                    importers[m].iter().map(|&i| acts_for[i]).collect();
                importing.and_then(|firsts| {
                    firsts
                        .iter()
                        .copied()
                        .find(|&f| {
                            group(f).is_some_and(|group| {
                                names_crate(own, group.crate_name)
                                    || (!group.first_party && names_crate(id, group.crate_name))
                            })
                        })
                        .or_else(|| firsts.iter().copied().find(|&f| !first_party(f)))
                        .or_else(|| firsts.first().copied())
                })
            } else if !generic
                && self.modules[m].plugin.is_none()
                && by_type.is_none()
                && !is_framework_crate(own)
                && !importers[m].is_empty()
                && importers[m]
                    .iter()
                    .all(|&i| acts_for[i].is_some_and(|f| !first_party(f)))
            {
                acts_for[importers[m][0]]
            } else {
                None
            };
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

/// Which plugin of `plugins` a module's type paths place it in (step 9 of
/// [`Graph::check_plugins`]): `plugins` lists `(crate_name, first_party)` in
/// first-seen plugin order, and the index returned is into `plugins`.
///
/// For each type path (read through wrappers and trait objects by
/// [`module_path`], up to its first `<`), among plugins whose crates hold it,
/// the longest crate name wins, then the plugin first in `plugins`. The
/// module's own type `own` decides before the type its `id` names, unless
/// `own` gives a first-party plugin and `id` one that is not. When `own` is in
/// no plugin's crate, `id` may place the module in a plugin that is not
/// first-party, never in a first-party one.
fn plugin_by_type(own: &str, id: &str, plugins: &[(&str, bool)]) -> Option<usize> {
    let mut order: Vec<usize> = (0..plugins.len()).collect();
    order.sort_by_key(|&index| std::cmp::Reverse(plugins[index].0.len()));
    let of_crate = |type_name: &str| {
        let read = module_path(type_name);
        let outer = read.split_once('<').map_or(read, |(outer, _)| outer);
        order.iter().copied().find(|&index| {
            outer
                .strip_prefix(plugins[index].0)
                .is_some_and(|rest| rest.starts_with("::"))
        })
    };
    let first_party = |index: usize| plugins[index].1;
    match (of_crate(own), of_crate(id)) {
        (Some(by_own), Some(by_id)) if first_party(by_own) && !first_party(by_id) => Some(by_id),
        (Some(by_own), _) => Some(by_own),
        (None, Some(by_id)) if !first_party(by_id) => Some(by_id),
        (None, _) => None,
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

    /// Whether the plugin checks of `graph` refuse only navree-flags' use of its namespace.
    fn only_the_namespace_refused(graph: &mut Graph) -> bool {
        let mut errors = Vec::new();
        graph.check_plugins(&mut errors);
        matches!(
            errors.as_slice(),
            [KernelError::NamespaceReserved {
                plugin: "navree-flags",
                namespace: "flags"
            }]
        )
    }

    #[test]
    fn a_plugin_is_first_party_by_its_crate_and_its_modules_own_types_and_keeps_its_namespace() {
        let version = env!("CARGO_PKG_VERSION");
        let notes = PluginInfo::new(
            "navree-plugin-flags",
            "navree_kernel",
            "flag_notes",
            version,
        );
        let roots: Vec<Box<dyn Module>> = vec![Box::new(Names(notes))];
        let mut graph = Graph::build(&roots, &[]).expect("a community plugin in its own namespace");
        let m = graph
            .module_of(ModuleId::of::<Names>())
            .expect("in the graph");
        // The module names navree-flags, in its reserved namespace, with this crate as the
        // plugin's. Its own type is not of navree-flags' crate, whatever crate its id is in:
        // not first-party, and refused.
        graph.modules[m].plugin = Some(PluginInfo::new(
            "navree-flags",
            "navree_kernel",
            "flags",
            version,
        ));
        assert!(only_the_namespace_refused(&mut graph));
        // Its own type is of navree-flags' crate, but the crate it names as the plugin's is
        // another: not first-party either.
        graph.modules[m].type_name = "navree_flags::module::FlagsModule";
        assert!(only_the_namespace_refused(&mut graph));
        assert!(!graph.first_party.contains("navree-flags"));
        // It names navree-flags' crate, and its own type is of that crate (its id need not
        // be): first-party, and the namespace is its.
        graph.modules[m].plugin = Some(PluginInfo::new(
            "navree-flags",
            "navree_flags",
            "flags",
            version,
        ));
        let mut errors = Vec::new();
        graph.check_plugins(&mut errors);
        assert!(errors.is_empty(), "{errors:?}");
        assert!(graph.first_party.contains("navree-flags"));
        assert_eq!(graph.acts_for[m], Some(m));
    }

    #[test]
    fn a_module_acts_for_a_first_party_plugin_by_its_own_type_and_never_by_its_id_alone() {
        use crate::builder::PassedModule;

        let plugins = [
            ("navree_flags", true),
            ("navree_plugin_khata", false),
            ("navree_plugin_khata::ledger", false),
        ];

        // A module whose own type is in the first-party plugin's crate acts for it (plain,
        // configured in its crate, or behind a reference or box).
        for own in [
            "navree_flags::module::FlagsModule",
            "navree_flags::module::ConfiguredFlagsModule",
            "&navree_flags::module::FlagsModule",
            "alloc::boxed::Box<navree_flags::module::FlagsModule>",
        ] {
            assert_eq!(
                plugin_by_type(own, "alloc::string::String", &plugins),
                Some(0),
                "{own}"
            );
        }

        // When its own type is in no plugin's crate, an id in the first-party plugin's crate
        // never gives it to the first-party plugin.
        for own in [
            "khata_extra::Sneak",
            "alloc::boxed::Box<dyn navree_kernel::module::Module>",
            "navree_kernel::module::ConfiguredModule<navree_flags::module::FlagsModule, ()>",
        ] {
            assert_eq!(
                plugin_by_type(own, "navree_flags::module::FlagsModule", &plugins),
                None,
                "{own}"
            );
        }

        // For a community plugin, an id still places a boxed or kernel-configured module in
        // that plugin, and the longest crate name wins.
        assert_eq!(
            plugin_by_type(
                "navree_kernel::module::ConfiguredModule<navree_plugin_khata::Store, ()>",
                "navree_plugin_khata::Store",
                &plugins
            ),
            Some(1)
        );
        assert_eq!(
            plugin_by_type(
                "alloc::boxed::Box<dyn navree_kernel::module::Module>",
                "navree_plugin_khata::ledger::Books",
                &plugins
            ),
            Some(2)
        );

        // When its own type is first-party and its id is a community plugin's, the community
        // plugin wins; when its own type is a community plugin's, a first-party id changes
        // nothing.
        assert_eq!(
            plugin_by_type("navree_flags::Wrap", "navree_plugin_khata::Store", &plugins),
            Some(1)
        );
        assert_eq!(
            plugin_by_type(
                "navree_plugin_khata::Sneak",
                "navree_flags::module::FlagsModule",
                &plugins
            ),
            Some(1)
        );

        // Two plugins of one crate break ties by first-seen plugin order.
        let same_crate = [
            ("navree_plugin_khata", false),
            ("navree_plugin_khata", false),
        ];
        assert_eq!(
            plugin_by_type(
                "navree_plugin_khata::Store",
                "navree_plugin_khata::Store",
                &same_crate
            ),
            Some(0)
        );

        // In a module graph: a boxed module with a first-party module's id recovers its own
        // crate's type and is never given to the first-party plugin; imported by the app it
        // stays the app's, and imported only by a community plugin from a non-framework crate
        // it acts for that community plugin.
        struct FirstPartyProbe;
        impl Module for FirstPartyProbe {
            fn register(&self, _: &mut ModuleBuilder) {}
        }

        mod other_crate {
            use crate::{Module, ModuleBuilder, ModuleId};

            pub struct TakesFirstPartyId<M>(pub std::marker::PhantomData<M>);

            impl<M: Send + Sync + 'static> Module for TakesFirstPartyId<M> {
                fn id(&self) -> ModuleId {
                    ModuleId::named::<M>("sneak")
                }

                fn register(&self, _: &mut ModuleBuilder) {}
            }
        }

        struct CommunityBoxed;
        impl Module for CommunityBoxed {
            fn register(&self, m: &mut ModuleBuilder) {
                m.imports(vec![
                    Box::new(other_crate::TakesFirstPartyId::<FirstPartyProbe>(
                        std::marker::PhantomData,
                    )) as Box<dyn Module>,
                ]);
            }
        }

        let first_party_crate_name = "navree_flags";
        let sneak_id = ModuleId::named::<FirstPartyProbe>("sneak");
        let mut graph = Graph::build_passed(
            &[
                PassedModule::of(FirstPartyProbe),
                PassedModule::of(CommunityBoxed),
            ],
            &[],
        )
        .expect("valid");
        let first_party = graph
            .module_of(ModuleId::of::<FirstPartyProbe>())
            .expect("in the graph");
        let community = graph
            .module_of(ModuleId::of::<CommunityBoxed>())
            .expect("in the graph");
        let sneak = graph.module_of(sneak_id).expect("in the graph");
        assert_eq!(
            graph.modules[sneak].type_name,
            std::any::type_name::<other_crate::TakesFirstPartyId<FirstPartyProbe>>()
        );
        graph.modules[sneak].type_name = "khata_extra::TakesFirstPartyId";
        let groups = [
            PluginGroup {
                first: first_party,
                crate_name: first_party_crate_name,
                first_party: true,
                members: vec![first_party],
            },
            PluginGroup {
                first: community,
                crate_name: "navree_plugin_khata",
                first_party: false,
                members: vec![community],
            },
        ];
        let acts_for = graph.attribution(&groups);
        assert_eq!(acts_for[sneak], Some(community));

        let app_graph = Graph::build_passed(
            &[
                PassedModule::of(FirstPartyProbe),
                PassedModule::boxed(Box::new(other_crate::TakesFirstPartyId::<FirstPartyProbe>(
                    std::marker::PhantomData,
                ))),
            ],
            &[],
        )
        .expect("valid");
        let first_party = app_graph
            .module_of(ModuleId::of::<FirstPartyProbe>())
            .expect("in the graph");
        let sneak = app_graph.module_of(sneak_id).expect("in the graph");
        let only_first = [PluginGroup {
            first: first_party,
            crate_name: first_party_crate_name,
            first_party: true,
            members: vec![first_party],
        }];
        assert_eq!(app_graph.attribution(&only_first)[sneak], None);
    }
}
