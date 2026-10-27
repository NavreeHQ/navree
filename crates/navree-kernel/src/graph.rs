//! The module graph: walking modules, visibility of providers and the
//! boot-time checks (missing or private providers, cycles, scope inversion).

use std::any::TypeId;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;

use crate::builder::{Contribution, ProviderDef};
use crate::lifecycle::LifecycleDef;
use crate::{
    BootErrors, CommandSpec, KernelError, Module, ModuleBuilder, ModuleId, ProviderKey, Scope,
};

/// The validated module graph, as printed by `check --graph`.
#[derive(Clone, Debug)]
pub struct GraphReport {
    /// Every module in dependency order (imports first); the implicit app root `App` is last.
    pub modules: Vec<ModuleReport>,
}

/// One module of a [`GraphReport`].
#[derive(Clone, Debug)]
pub struct ModuleReport {
    /// The module id.
    pub id: ModuleId,
    /// The module name.
    pub name: &'static str,
    /// Whether the module's exports are visible everywhere.
    pub global: bool,
    /// Names of the modules it imports.
    pub imports: Vec<&'static str>,
    /// Its providers, in registration order.
    pub providers: Vec<ProviderReport>,
    /// The keys it exports.
    pub exports: Vec<String>,
    /// The commands it registers.
    pub commands: Vec<CommandInfo>,
}

/// One command of a [`ModuleReport`].
#[derive(Clone, Debug)]
pub struct CommandInfo {
    /// The command name.
    pub name: &'static str,
    /// Its one-line description.
    pub about: &'static str,
}

/// One provider of a [`ModuleReport`].
#[derive(Clone, Debug)]
pub struct ProviderReport {
    /// The provider key.
    pub key: String,
    /// The provider scope.
    pub scope: Scope,
    /// The declared dependencies.
    pub dependencies: Vec<String>,
    /// Whether this is a multi-binding (see [`ModuleBuilder::multi`]).
    pub multi: bool,
}

impl GraphReport {
    /// Whether any module registers the command `name`.
    pub fn has_command(&self, name: &str) -> bool {
        self.modules
            .iter()
            .any(|m| m.commands.iter().any(|c| c.name == name))
    }
}

/// Prints the import tree from the implicit root `App`; each module's
/// providers, exports and commands are listed at its first appearance.
impl fmt::Display for GraphReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fn module_lines(
            report: &GraphReport,
            m: usize,
            prefix: &str,
            shown: &mut [bool],
            out: &mut Vec<String>,
        ) {
            let module = &report.modules[m];
            if std::mem::replace(&mut shown[m], true) {
                return;
            }
            let mut items: Vec<Result<String, usize>> = Vec::new();
            for p in &module.providers {
                let mut line = format!("provides {} ({})", p.key, p.scope);
                if p.multi {
                    line.push_str(" [multi]");
                }
                if !p.dependencies.is_empty() {
                    line.push_str(&format!(" <- {}", p.dependencies.join(", ")));
                }
                items.push(Ok(line));
            }
            items.extend(module.exports.iter().map(|e| Ok(format!("exports {e}"))));
            items.extend(
                module
                    .commands
                    .iter()
                    .map(|c| Ok(format!("command {}: {}", c.name, c.about))),
            );
            items.extend(
                module
                    .imports
                    .iter()
                    .filter_map(|name| report.modules.iter().position(|r| r.name == *name))
                    .map(Err),
            );
            let count = items.len();
            for (i, item) in items.into_iter().enumerate() {
                let last = i + 1 == count;
                let (branch, indent) = if last {
                    ("└── ", "    ")
                } else {
                    ("├── ", "│   ")
                };
                match item {
                    Ok(line) => out.push(format!("{prefix}{branch}{line}")),
                    Err(child) => {
                        let module = &report.modules[child];
                        let mut header = module.name.to_owned();
                        if module.global {
                            header.push_str(" [global]");
                        }
                        if shown[child] {
                            header.push_str(" (see above)");
                        }
                        out.push(format!("{prefix}{branch}{header}"));
                        module_lines(report, child, &format!("{prefix}{indent}"), shown, out);
                    }
                }
            }
        }

        let Some(root) = self.modules.len().checked_sub(1) else {
            return Ok(());
        };
        let mut lines = vec![self.modules[root].name.to_owned()];
        let mut shown = vec![false; self.modules.len()];
        module_lines(self, root, "", &mut shown, &mut lines);
        f.write_str(&lines.join("\n"))
    }
}

/// The implicit root module that imports the modules passed to the app builder.
struct AppRoot;

/// Index of the implicit root module.
pub(crate) const ROOT: usize = 0;

/// A registered module.
pub(crate) struct ModuleNode {
    pub(crate) id: ModuleId,
    pub(crate) name: &'static str,
    pub(crate) global: bool,
    pub(crate) imports: Vec<usize>,
    pub(crate) providers: Vec<usize>,
    pub(crate) exports: Vec<ProviderKey>,
    pub(crate) contributions: Vec<Contribution>,
    pub(crate) lifecycles: Vec<LifecycleDef>,
    pub(crate) commands: Vec<CommandSpec>,
}

/// A registered provider and the module that owns it.
pub(crate) struct ProviderNode {
    pub(crate) def: ProviderDef,
    pub(crate) module: usize,
}

/// What a key resolves to from inside one module.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Visible {
    One(usize),
    /// Several different providers (indices, in module order) are visible.
    Ambiguous(Vec<usize>),
}

impl Visible {
    fn providers(&self) -> &[usize] {
        match self {
            Visible::One(p) => std::slice::from_ref(p),
            Visible::Ambiguous(ps) => ps,
        }
    }

    fn union(&self, other: &Visible) -> Visible {
        let mut all = self.providers().to_vec();
        for p in other.providers() {
            if !all.contains(p) {
                all.push(*p);
            }
        }
        if all.len() == 1 {
            Visible::One(all[0])
        } else {
            Visible::Ambiguous(all)
        }
    }
}

type VisibleMap = HashMap<ProviderKey, Visible>;

/// The validated module graph of an app.
pub(crate) struct Graph {
    pub(crate) modules: Vec<ModuleNode>,
    pub(crate) module_index: HashMap<ModuleId, usize>,
    pub(crate) providers: Vec<ProviderNode>,
    /// Per module: every key it can resolve.
    visible: Vec<VisibleMap>,
    /// What modules outside the graph see: exports of global modules.
    outside: VisibleMap,
    /// Module indices in dependency order (imports first); the root is last.
    pub(crate) order: Vec<usize>,
    /// Multi-binding provider indices per provided type, in dependency order.
    multi: HashMap<TypeId, Vec<usize>>,
    /// The service locator's answer for every key it resolves to exactly
    /// one provider, built once at boot (see [`Graph::locate`]).
    located: HashMap<ProviderKey, usize>,
}

impl Graph {
    /// Walks the modules from the implicit root, applies `overrides` (value
    /// definitions replacing unnamed providers) and validates the graph.
    pub(crate) fn build(
        roots: &[Box<dyn Module>],
        overrides: &[ProviderDef],
    ) -> Result<Graph, BootErrors> {
        let walked = walk(roots)?;
        let mut errors = Vec::new();
        let mut graph = assemble(walked, &mut errors);
        graph.apply_overrides(overrides);
        if let Some(cycle) = graph.find_module_cycle() {
            errors.push(cycle);
            return Err(BootErrors(errors));
        }
        graph.compute_visibility(&mut errors);
        let edges = graph.check_dependencies(&mut errors);
        graph.check_lifecycles(&mut errors);
        graph.check_commands(&mut errors);
        graph.check_scopes(&edges, &mut errors);
        graph.check_provider_cycles(&edges, &mut errors);
        if !errors.is_empty() {
            return Err(BootErrors(errors));
        }
        graph.order = graph.dependency_order(&edges);
        for &m in &graph.order {
            for &p in &graph.modules[m].providers {
                let def = &graph.providers[p].def;
                if def.multi {
                    graph.multi.entry(def.key.type_id()).or_default().push(p);
                }
            }
        }
        graph.located = graph.locator_map();
        Ok(graph)
    }

    /// The service locator's answers, decided once: every key with exactly
    /// one provider, or with several of which exactly one is exported (the
    /// rule of [`locate_by_scan`](Graph::locate_by_scan)).
    fn locator_map(&self) -> HashMap<ProviderKey, usize> {
        let mut candidates: HashMap<&ProviderKey, Vec<usize>> = HashMap::new();
        for (index, provider) in self.providers.iter().enumerate() {
            if !provider.def.multi {
                candidates.entry(&provider.def.key).or_default().push(index);
            }
        }
        candidates
            .into_iter()
            .filter_map(|(key, providers)| {
                let only = match providers.as_slice() {
                    [only] => *only,
                    _ => {
                        let mut exported = providers.iter().copied().filter(|&p| {
                            self.modules[self.providers[p].module]
                                .exports
                                .contains(key)
                        });
                        match (exported.next(), exported.next()) {
                            (Some(only), None) => only,
                            _ => return None,
                        }
                    }
                };
                Some((key.clone(), only))
            })
            .collect()
    }

    /// Replaces every unnamed, non-multi provider of each override's key; an
    /// override nobody registers becomes a global export of the root.
    fn apply_overrides(&mut self, overrides: &[ProviderDef]) {
        for value in overrides {
            let mut replaced = false;
            for provider in self
                .providers
                .iter_mut()
                .filter(|p| !p.def.multi && p.def.key == value.key)
            {
                provider.def = value.clone();
                replaced = true;
            }
            if !replaced {
                self.modules[ROOT].providers.push(self.providers.len());
                self.modules[ROOT].exports.push(value.key.clone());
                self.providers.push(ProviderNode {
                    def: value.clone(),
                    module: ROOT,
                });
            }
        }
    }

    /// Finds the provider of `key` anywhere in the app, ignoring visibility
    /// (for the service locator). Prefers exported providers when several
    /// modules register the key. One map lookup: the answers are decided
    /// at boot; a key the map lacks is missing or ambiguous, and the scan
    /// explains which.
    pub(crate) fn locate(
        &self,
        key: &ProviderKey,
        required_by: &str,
    ) -> Result<usize, KernelError> {
        match self.located.get(key) {
            Some(&index) => Ok(index),
            None => self.locate_by_scan(key, required_by),
        }
    }

    /// [`locate`](Graph::locate) by scanning every provider.
    fn locate_by_scan(&self, key: &ProviderKey, required_by: &str) -> Result<usize, KernelError> {
        let candidates: Vec<usize> = (0..self.providers.len())
            .filter(|&p| !self.providers[p].def.multi && self.providers[p].def.key == *key)
            .collect();
        let exported: Vec<usize> = candidates
            .iter()
            .copied()
            .filter(|&p| self.modules[self.providers[p].module].exports.contains(key))
            .collect();
        match (candidates.as_slice(), exported.as_slice()) {
            ([], _) => {
                Err(self.missing(None, self.modules[ROOT].name, key, required_by.to_owned()))
            }
            ([only], _) | (_, [only]) => Ok(*only),
            _ => Err(KernelError::AmbiguousProvider {
                module: self.modules[ROOT].name,
                requested: key.to_string(),
                owners: candidates
                    .iter()
                    .map(|&p| self.modules[self.providers[p].module].name)
                    .collect(),
            }),
        }
    }

    /// The multi-bindings of type `T` (by type id), in module dependency order.
    pub(crate) fn multi_of(&self, type_id: TypeId) -> &[usize] {
        self.multi.get(&type_id).map_or(&[], Vec::as_slice)
    }

    /// Resolves `key` as seen from `module` (`None` for modules outside the graph,
    /// which only see global exports).
    pub(crate) fn lookup(&self, module: Option<usize>, key: &ProviderKey) -> Option<&Visible> {
        match module {
            Some(m) => self.visible[m].get(key),
            None => self.outside.get(key),
        }
    }

    /// The module index of `id`, if it is part of the graph.
    pub(crate) fn module_of(&self, id: ModuleId) -> Option<usize> {
        self.module_index.get(&id).copied()
    }

    /// The error for a lookup that did not find exactly one provider.
    pub(crate) fn unresolved(
        &self,
        found: Option<&Visible>,
        module: Option<usize>,
        module_name: &'static str,
        key: &ProviderKey,
        required_by: String,
    ) -> KernelError {
        match found {
            Some(Visible::Ambiguous(providers)) => KernelError::AmbiguousProvider {
                module: module_name,
                requested: key.to_string(),
                owners: providers
                    .iter()
                    .map(|&p| self.modules[self.providers[p].module].name)
                    .collect(),
            },
            _ => self.missing(module, module_name, key, required_by),
        }
    }

    /// Explains why `key` is not visible from `module` (`module_name` names it in the error).
    pub(crate) fn missing(
        &self,
        module: Option<usize>,
        module_name: &'static str,
        key: &ProviderKey,
        required_by: String,
    ) -> KernelError {
        let requested = key.to_string();
        let owners: Vec<usize> = self
            .providers
            .iter()
            .filter(|p| !p.def.multi && p.def.key == *key && Some(p.module) != module)
            .map(|p| p.module)
            .collect();
        let imported_owner = module.and_then(|m| {
            owners
                .iter()
                .copied()
                .find(|o| self.modules[m].imports.contains(o))
        });
        if let Some(owner) = imported_owner {
            return KernelError::NotExported {
                module: module_name,
                requested,
                owner: self.modules[owner].name,
            };
        }
        if let Some(owner) = owners
            .iter()
            .copied()
            .find(|&o| self.modules[o].exports.contains(key))
        {
            return KernelError::NotImported {
                module: module_name,
                requested,
                owner: self.modules[owner].name,
            };
        }
        if let Some(&owner) = owners.first() {
            return KernelError::NotExported {
                module: module_name,
                requested,
                owner: self.modules[owner].name,
            };
        }
        KernelError::MissingProvider {
            module: module_name,
            requested,
            required_by,
        }
    }

    /// The first module import cycle reachable from the root, if any.
    fn find_module_cycle(&self) -> Option<KernelError> {
        #[derive(Clone, Copy, PartialEq)]
        enum Mark {
            New,
            Active,
            Done,
        }
        fn visit(
            graph: &Graph,
            m: usize,
            marks: &mut [Mark],
            stack: &mut Vec<usize>,
        ) -> Option<Vec<usize>> {
            marks[m] = Mark::Active;
            stack.push(m);
            for &import in &graph.modules[m].imports {
                match marks[import] {
                    Mark::Active => {
                        let start = stack.iter().position(|&s| s == import).unwrap_or(0);
                        let mut path = stack[start..].to_vec();
                        path.push(import);
                        return Some(path);
                    }
                    Mark::New => {
                        if let Some(path) = visit(graph, import, marks, stack) {
                            return Some(path);
                        }
                    }
                    Mark::Done => {}
                }
            }
            stack.pop();
            marks[m] = Mark::Done;
            None
        }
        let mut marks = vec![Mark::New; self.modules.len()];
        visit(self, ROOT, &mut marks, &mut Vec::new()).map(|path| KernelError::ModuleCycle {
            path: path.into_iter().map(|m| self.modules[m].name).collect(),
        })
    }

    /// Modules in import post-order (imports before importers), root last.
    fn import_order(&self) -> Vec<usize> {
        fn visit(graph: &Graph, m: usize, seen: &mut [bool], order: &mut Vec<usize>) {
            if std::mem::replace(&mut seen[m], true) {
                return;
            }
            for &import in &graph.modules[m].imports {
                visit(graph, import, seen, order);
            }
            order.push(m);
        }
        let mut seen = vec![false; self.modules.len()];
        let mut order = Vec::with_capacity(self.modules.len());
        visit(self, ROOT, &mut seen, &mut order);
        order
    }

    fn own(&self, module: usize, key: &ProviderKey) -> Option<usize> {
        self.modules[module]
            .providers
            .iter()
            .copied()
            .find(|&p| !self.providers[p].def.multi && self.providers[p].def.key == *key)
    }

    /// Computes what each module exports (own providers and re-exports of
    /// imported exports) and, from that, what each module can see:
    /// own providers, then exports of direct imports, then exports of global modules.
    fn compute_visibility(&mut self, errors: &mut Vec<KernelError>) {
        let mut exported: Vec<VisibleMap> = vec![VisibleMap::new(); self.modules.len()];
        for m in self.import_order() {
            let mut map = VisibleMap::new();
            for key in &self.modules[m].exports {
                let found = match self.own(m, key) {
                    Some(p) => Some(Visible::One(p)),
                    None => self.modules[m]
                        .imports
                        .iter()
                        .filter_map(|&i| exported[i].get(key))
                        .fold(None, |acc: Option<Visible>, v| {
                            Some(acc.map_or_else(|| v.clone(), |a| a.union(v)))
                        }),
                };
                match found {
                    Some(v) => {
                        map.insert(key.clone(), v);
                    }
                    None => errors.push(KernelError::MissingProvider {
                        module: self.modules[m].name,
                        requested: key.to_string(),
                        required_by: format!("the exports of `{}`", self.modules[m].name),
                    }),
                }
            }
            exported[m] = map;
        }

        let globals: Vec<usize> = (0..self.modules.len())
            .filter(|&m| self.modules[m].global)
            .collect();
        let mut outside = VisibleMap::new();
        for &g in &globals {
            merge(&mut outside, &exported[g]);
        }
        let mut visible = Vec::with_capacity(self.modules.len());
        for m in 0..self.modules.len() {
            let mut map = VisibleMap::new();
            for &g in globals.iter().filter(|&&g| g != m) {
                merge(&mut map, &exported[g]);
            }
            let mut imported = VisibleMap::new();
            for &i in &self.modules[m].imports {
                merge(&mut imported, &exported[i]);
            }
            map.extend(imported);
            for &p in &self.modules[m].providers {
                if !self.providers[p].def.multi {
                    map.insert(self.providers[p].def.key.clone(), Visible::One(p));
                }
            }
            visible.push(map);
        }
        self.visible = visible;
        self.outside = outside;
    }

    /// Checks that every declared dependency resolves to exactly one visible
    /// provider. Returns the resolved dependency edges per provider.
    fn check_dependencies(&self, errors: &mut Vec<KernelError>) -> Vec<Vec<usize>> {
        let mut edges = Vec::with_capacity(self.providers.len());
        for provider in &self.providers {
            let mut resolved = Vec::new();
            for dep in &provider.def.deps {
                match self.lookup(Some(provider.module), dep) {
                    Some(Visible::One(p)) => resolved.push(*p),
                    found => errors.push(self.unresolved(
                        found,
                        Some(provider.module),
                        self.modules[provider.module].name,
                        dep,
                        format!("`{}`", provider.def.key),
                    )),
                }
            }
            edges.push(resolved);
        }
        edges
    }

    /// Lifecycle hooks must belong to a provider of the registering module.
    fn check_lifecycles(&self, errors: &mut Vec<KernelError>) {
        for (m, module) in self.modules.iter().enumerate() {
            for hooks in &module.lifecycles {
                if self.own(m, &hooks.key).is_none() {
                    errors.push(KernelError::MissingProvider {
                        module: module.name,
                        requested: hooks.key.to_string(),
                        required_by: format!("a lifecycle registration of `{}`", module.name),
                    });
                }
            }
        }
    }

    /// Command names must be unique across the app.
    fn check_commands(&self, errors: &mut Vec<KernelError>) {
        let mut seen: HashMap<&str, &'static str> = HashMap::new();
        for m in self.import_order() {
            let module = &self.modules[m];
            for command in &module.commands {
                if let Some(first) = seen.insert(command.name, module.name) {
                    errors.push(KernelError::DuplicateCommand {
                        name: command.name.to_owned(),
                        first,
                        second: module.name,
                    });
                }
            }
        }
    }

    /// Whether any module registers the command `name`.
    pub(crate) fn has_command(&self, name: &str) -> bool {
        self.commands().any(|c| c.name == name)
    }

    /// Every command, modules in dependency order.
    pub(crate) fn commands(&self) -> impl Iterator<Item = &CommandSpec> {
        self.order
            .iter()
            .flat_map(|&m| self.modules[m].commands.iter())
    }

    /// Rejects singletons that depend on request-scoped providers, directly or
    /// through transient providers.
    fn check_scopes(&self, edges: &[Vec<usize>], errors: &mut Vec<KernelError>) {
        fn reaches_request(
            graph: &Graph,
            edges: &[Vec<usize>],
            p: usize,
            seen: &mut HashSet<usize>,
        ) -> Option<usize> {
            match graph.providers[p].def.scope {
                Scope::Request => Some(p),
                Scope::Singleton => None,
                Scope::Transient => {
                    if !seen.insert(p) {
                        return None;
                    }
                    edges[p]
                        .iter()
                        .find_map(|&d| reaches_request(graph, edges, d, seen))
                }
            }
        }
        for (p, provider) in self.providers.iter().enumerate() {
            if provider.def.scope != Scope::Singleton {
                continue;
            }
            let mut seen = HashSet::new();
            if let Some(r) = edges[p]
                .iter()
                .find_map(|&d| reaches_request(self, edges, d, &mut seen))
            {
                errors.push(KernelError::ScopeInversion {
                    module: self.modules[provider.module].name,
                    singleton: provider.def.key.to_string(),
                    request_scoped: self.providers[r].def.key.to_string(),
                });
            }
        }
    }

    /// Reports every cycle in the declared provider dependencies.
    fn check_provider_cycles(&self, edges: &[Vec<usize>], errors: &mut Vec<KernelError>) {
        #[derive(Clone, Copy, PartialEq)]
        enum Mark {
            New,
            Active,
            Done,
        }
        fn visit(
            graph: &Graph,
            edges: &[Vec<usize>],
            p: usize,
            marks: &mut [Mark],
            stack: &mut Vec<usize>,
            errors: &mut Vec<KernelError>,
        ) {
            marks[p] = Mark::Active;
            stack.push(p);
            for &d in &edges[p] {
                match marks[d] {
                    Mark::Active => {
                        let start = stack.iter().position(|&s| s == d).unwrap_or(0);
                        let path = stack[start..]
                            .iter()
                            .chain(std::iter::once(&d))
                            .map(|&q| graph.providers[q].def.key.to_string())
                            .collect();
                        errors.push(KernelError::ProviderCycle { path });
                    }
                    Mark::New => visit(graph, edges, d, marks, stack, errors),
                    Mark::Done => {}
                }
            }
            stack.pop();
            marks[p] = Mark::Done;
        }
        let mut marks = vec![Mark::New; self.providers.len()];
        for p in 0..self.providers.len() {
            if marks[p] == Mark::New {
                visit(self, edges, p, &mut marks, &mut Vec::new(), errors);
            }
        }
    }

    /// Module order for construction, contributions and lifecycle hooks: a
    /// module comes after the modules it imports and the modules whose
    /// providers it uses; the root is last.
    fn dependency_order(&self, edges: &[Vec<usize>]) -> Vec<usize> {
        fn visit(
            graph: &Graph,
            edges: &[Vec<usize>],
            m: usize,
            seen: &mut [bool],
            order: &mut Vec<usize>,
        ) {
            if std::mem::replace(&mut seen[m], true) {
                return;
            }
            for &import in &graph.modules[m].imports {
                visit(graph, edges, import, seen, order);
            }
            for &p in &graph.modules[m].providers {
                for &d in &edges[p] {
                    visit(graph, edges, graph.providers[d].module, seen, order);
                }
            }
            order.push(m);
        }
        let mut seen = vec![false; self.modules.len()];
        let mut order = Vec::with_capacity(self.modules.len());
        visit(self, edges, ROOT, &mut seen, &mut order);
        order
    }

    /// A description of the graph for `check --graph`, modules in dependency order.
    pub(crate) fn report(&self) -> GraphReport {
        let modules = self
            .order
            .iter()
            .map(|&m| {
                let module = &self.modules[m];
                ModuleReport {
                    id: module.id,
                    name: module.name,
                    global: module.global,
                    imports: module
                        .imports
                        .iter()
                        .map(|&i| self.modules[i].name)
                        .collect(),
                    providers: module
                        .providers
                        .iter()
                        .map(|&p| {
                            let def = &self.providers[p].def;
                            ProviderReport {
                                key: def.key.to_string(),
                                scope: def.scope,
                                dependencies: def.deps.iter().map(ToString::to_string).collect(),
                                multi: def.multi,
                            }
                        })
                        .collect(),
                    exports: module.exports.iter().map(ToString::to_string).collect(),
                    commands: module
                        .commands
                        .iter()
                        .map(|c| CommandInfo {
                            name: c.name,
                            about: c.about,
                        })
                        .collect(),
                }
            })
            .collect();
        GraphReport { modules }
    }
}

/// Adds `from` into `into`; a key mapped to different providers becomes ambiguous.
fn merge(into: &mut VisibleMap, from: &VisibleMap) {
    for (key, v) in from {
        match into.get_mut(key) {
            Some(existing) => *existing = existing.union(v),
            None => {
                into.insert(key.clone(), v.clone());
            }
        }
    }
}

/// The output of `Module::register` for one module.
struct Registered {
    builder: ModuleBuilder,
    import_ids: Vec<ModuleId>,
}

/// A module value to register: a root module borrowed from the app builder or
/// an import owned by the walk.
enum Source<'a> {
    Root(&'a dyn Module),
    Import(Box<dyn Module>),
}

impl Source<'_> {
    fn module(&self) -> &dyn Module {
        match self {
            Source::Root(m) => *m,
            Source::Import(m) => m.as_ref(),
        }
    }
}

/// A configured instance of `ModuleId` turned up after a plain instance of it
/// was registered: walk again, registering that plain import last.
struct Restart(ModuleId, &'static str);

/// Calls `register` once per module id, starting from the implicit root.
/// Configured (`for_root`) instances win over plain imports of the same id.
fn walk(roots: &[Box<dyn Module>]) -> Result<Vec<Registered>, BootErrors> {
    let mut deferred = HashSet::new();
    loop {
        match Walker::new(&deferred).run(roots) {
            Ok(Ok(registered)) => return Ok(registered),
            Ok(Err(errors)) => return Err(BootErrors(errors)),
            Err(Restart(id, name)) => {
                if !deferred.insert(id) {
                    // Only the plain instance's own imports lead to the configured
                    // one: the module (transitively) imports itself.
                    return Err(KernelError::ModuleCycle {
                        path: vec![name, name],
                    }
                    .into());
                }
            }
        }
    }
}

struct Walker<'a, 'd> {
    deferred: &'d HashSet<ModuleId>,
    registered: HashSet<ModuleId>,
    configured: HashSet<ModuleId>,
    configured_queue: VecDeque<Source<'a>>,
    plain: Vec<(ModuleId, Source<'a>)>,
    out: Vec<Registered>,
    errors: Vec<KernelError>,
}

impl<'a, 'd> Walker<'a, 'd> {
    fn new(deferred: &'d HashSet<ModuleId>) -> Self {
        Self {
            deferred,
            registered: HashSet::new(),
            configured: HashSet::new(),
            configured_queue: VecDeque::new(),
            plain: Vec::new(),
            out: Vec::new(),
            errors: Vec::new(),
        }
    }

    fn run(
        mut self,
        roots: &'a [Box<dyn Module>],
    ) -> Result<Result<Vec<Registered>, Vec<KernelError>>, Restart> {
        let root_id = ModuleId::with_name::<AppRoot>("App");
        let mut root = ModuleBuilder::new(root_id, root_id.name());
        root.global = true;
        self.registered.insert(root_id);
        self.out.push(Registered {
            builder: root,
            import_ids: roots.iter().map(|m| m.id()).collect(),
        });
        for module in roots {
            self.enqueue(Source::Root(module.as_ref()))?;
        }
        loop {
            if let Some(source) = self.configured_queue.pop_front() {
                self.process(source)?;
            } else if let Some(source) = self.next_plain() {
                self.process(source)?;
            } else {
                break;
            }
        }
        Ok(if self.errors.is_empty() {
            Ok(self.out)
        } else {
            Err(self.errors)
        })
    }

    fn enqueue(&mut self, source: Source<'a>) -> Result<(), Restart> {
        let module = source.module();
        let id = module.id();
        if module.is_configured() {
            if !self.configured.insert(id) {
                self.errors.push(KernelError::ConfiguredTwice {
                    module: module.name(),
                });
                return Ok(());
            }
            if self.registered.contains(&id) {
                return Err(Restart(id, module.name()));
            }
            self.configured_queue.push_back(source);
        } else if !self.registered.contains(&id)
            && !self.configured.contains(&id)
            && !self.plain.iter().any(|(queued, _)| *queued == id)
        {
            self.plain.push((id, source));
        }
        Ok(())
    }

    /// The next plain module to register; deferred ids go last.
    fn next_plain(&mut self) -> Option<Source<'a>> {
        let (registered, configured) = (&self.registered, &self.configured);
        self.plain
            .retain(|(id, _)| !registered.contains(id) && !configured.contains(id));
        let position = self
            .plain
            .iter()
            .position(|(id, _)| !self.deferred.contains(id))
            .or(if self.plain.is_empty() { None } else { Some(0) })?;
        Some(self.plain.remove(position).1)
    }

    fn process(&mut self, source: Source<'a>) -> Result<(), Restart> {
        let module = source.module();
        let id = module.id();
        let mut builder = ModuleBuilder::new(id, module.name());
        module.register(&mut builder);
        let imports = std::mem::take(&mut builder.imports);
        self.registered.insert(id);
        self.out.push(Registered {
            builder,
            import_ids: imports.iter().map(|m| m.id()).collect(),
        });
        for import in imports {
            self.enqueue(Source::Import(import))?;
        }
        Ok(())
    }
}

/// Turns registered builders into graph nodes; reports duplicate keys.
fn assemble(walked: Vec<Registered>, errors: &mut Vec<KernelError>) -> Graph {
    let module_index: HashMap<ModuleId, usize> = walked
        .iter()
        .enumerate()
        .map(|(i, r)| (r.builder.id, i))
        .collect();
    let mut modules = Vec::with_capacity(walked.len());
    let mut providers: Vec<ProviderNode> = Vec::new();
    for (index, registered) in walked.into_iter().enumerate() {
        let builder = registered.builder;
        let mut imports: Vec<usize> = Vec::new();
        for id in &registered.import_ids {
            let import = module_index[id];
            if !imports.contains(&import) {
                imports.push(import);
            }
        }
        let mut provider_indices: Vec<usize> = Vec::new();
        for def in builder.providers {
            if !def.multi
                && provider_indices
                    .iter()
                    .any(|&p| !providers[p].def.multi && providers[p].def.key == def.key)
            {
                errors.push(KernelError::DuplicateProvider {
                    module: builder.name,
                    key: def.key.to_string(),
                });
                continue;
            }
            provider_indices.push(providers.len());
            providers.push(ProviderNode { def, module: index });
        }
        modules.push(ModuleNode {
            id: builder.id,
            name: builder.name,
            global: builder.global,
            imports,
            providers: provider_indices,
            exports: builder.exports,
            contributions: builder.contributions,
            lifecycles: builder.lifecycles,
            commands: builder.commands,
        });
    }
    Graph {
        modules,
        module_index,
        providers,
        visible: Vec::new(),
        outside: VisibleMap::new(),
        order: Vec::new(),
        multi: HashMap::new(),
        located: HashMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{Factory, Resolver};

    /// Provides a private `u32` and an unexported `String`.
    struct Private;

    impl Module for Private {
        fn register(&self, m: &mut ModuleBuilder) {
            m.value(1_u32).value(String::from("private"));
        }
    }

    /// Exports a `String`, an `i64` and a `u8` named `eight`.
    struct Exporting;

    impl Module for Exporting {
        fn register(&self, m: &mut ModuleBuilder) {
            m.value(String::from("exported"))
                .exports::<String>()
                .value(1_i64)
                .exports::<i64>()
                .factory(
                    Factory::<u8>::new(|_: Resolver| async { Ok(Arc::new(8_u8)) }).named("eight"),
                )
                .export_key(ProviderKey::named::<u8>("eight"));
        }
    }

    /// Exports another `i64`: two modules export one, so the locator cannot pick.
    struct AlsoExporting;

    impl Module for AlsoExporting {
        fn register(&self, m: &mut ModuleBuilder) {
            m.value(2_i64).exports::<i64>();
        }
    }

    fn graph() -> Graph {
        let roots: Vec<Box<dyn Module>> = vec![
            Box::new(Private),
            Box::new(Exporting),
            Box::new(AlsoExporting),
        ];
        Graph::build(&roots, &[]).expect("graph builds")
    }

    #[test]
    fn the_locator_map_answers_as_a_scan_of_every_provider_does() {
        let graph = graph();
        let mut keys: Vec<ProviderKey> = graph
            .providers
            .iter()
            .map(|p| p.def.key.clone())
            .collect();
        keys.push(ProviderKey::of::<bool>());
        keys.push(ProviderKey::named::<u8>("nine"));
        for key in &keys {
            let mapped = graph.locate(key, "a test").map_err(|e| e.to_string());
            let scanned = graph
                .locate_by_scan(key, "a test")
                .map_err(|e| e.to_string());
            assert_eq!(mapped, scanned, "{key}");
        }
    }

    #[test]
    fn only_keys_with_one_answer_are_in_the_locator_map() {
        let graph = graph();
        // One provider, private: the locator ignores visibility.
        assert!(graph.located.contains_key(&ProviderKey::of::<u32>()));
        // Two providers, one exported: the exported one.
        let string = graph.located[&ProviderKey::of::<String>()];
        assert_eq!(
            graph.modules[graph.providers[string].module].name,
            "Exporting"
        );
        assert!(
            graph
                .located
                .contains_key(&ProviderKey::named::<u8>("eight"))
        );
        // Two exported providers: ambiguous, answered by the scan's error.
        assert!(!graph.located.contains_key(&ProviderKey::of::<i64>()));
        assert!(matches!(
            graph.locate(&ProviderKey::of::<i64>(), "a test"),
            Err(KernelError::AmbiguousProvider { .. })
        ));
        assert!(matches!(
            graph.locate(&ProviderKey::of::<bool>(), "a test"),
            Err(KernelError::MissingProvider { .. })
        ));
    }
}
