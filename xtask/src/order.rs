//! The order crates are published in: a crate goes after every workspace
//! crate the registry must already have when it arrives.
//!
//! That is every normal and build dependency (optional ones too, and those
//! of any target) and every dev-dependency that names a version: Cargo keeps
//! those in the published manifest, and a registry refuses a manifest that
//! names a crate it does not have. A path-only dev-dependency is dropped when
//! the crate is packaged, so it puts no constraint on the order, and it is
//! how a crate's tests may use a crate that depends on it.

use std::collections::{BTreeMap, BTreeSet};

use crate::metadata::{DepKind, Workspace};

/// The workspace crates `name` must wait for, among `within`.
pub fn waits_for(workspace: &Workspace, name: &str, within: &BTreeSet<String>) -> BTreeSet<String> {
    let Some(package) = workspace.member(name) else {
        return BTreeSet::new();
    };
    workspace
        .internal_deps(package)
        .filter(|dep| dep.kind != DepKind::Dev || dep.is_versioned())
        .filter(|dep| within.contains(&dep.name))
        .map(|dep| dep.name.clone())
        .collect()
}

/// The publish order of `selected`, in layers: each crate of a layer
/// depends only on crates of earlier layers, and each layer is sorted by
/// name. `Err` holds every cycle (a crate that waits for itself, directly
/// or through others), each sorted by name.
pub fn layers(
    workspace: &Workspace,
    selected: &[String],
) -> Result<Vec<Vec<String>>, Vec<Vec<String>>> {
    let within: BTreeSet<String> = selected.iter().cloned().collect();
    let edges: BTreeMap<String, BTreeSet<String>> = within
        .iter()
        .map(|name| (name.clone(), waits_for(workspace, name, &within)))
        .collect();
    let cycles = cycles(&edges);
    if !cycles.is_empty() {
        return Err(cycles);
    }
    let mut done: BTreeSet<String> = BTreeSet::new();
    let mut layers = Vec::new();
    while done.len() < edges.len() {
        let layer: Vec<String> = edges
            .iter()
            .filter(|(name, deps)| !done.contains(*name) && deps.iter().all(|d| done.contains(d)))
            .map(|(name, _)| name.clone())
            .collect();
        // Without a cycle there is always a crate whose dependencies are done.
        assert!(!layer.is_empty(), "no progress without a cycle");
        done.extend(layer.iter().cloned());
        layers.push(layer);
    }
    Ok(layers)
}

/// The publish order of `selected` as one list.
pub fn publish_order(workspace: &Workspace, selected: &[String]) -> Result<Vec<String>, String> {
    layers(workspace, selected)
        .map(|layers| layers.into_iter().flatten().collect())
        .map_err(|cycles| describe_cycles(&cycles))
}

/// A message naming each cycle.
pub fn describe_cycles(cycles: &[Vec<String>]) -> String {
    let mut message = String::from(
        "these crates wait for each other, so no publish order exists (write the dev-dependency that closes each cycle as a path-only dependency, without a version or `workspace = true`, so Cargo drops it when it packages the crate):",
    );
    for cycle in cycles {
        message.push_str("\n  ");
        message.push_str(&cycle.join(" <-> "));
    }
    message
}

/// The strongly connected components with more than one crate, and the
/// crates that wait for themselves (Tarjan's algorithm).
fn cycles(edges: &BTreeMap<String, BTreeSet<String>>) -> Vec<Vec<String>> {
    struct State<'a> {
        edges: &'a BTreeMap<String, BTreeSet<String>>,
        next: usize,
        index: BTreeMap<&'a str, usize>,
        low: BTreeMap<&'a str, usize>,
        stack: Vec<&'a str>,
        on_stack: BTreeSet<&'a str>,
        found: Vec<Vec<String>>,
    }

    fn visit<'a>(state: &mut State<'a>, node: &'a str) {
        state.index.insert(node, state.next);
        state.low.insert(node, state.next);
        state.next += 1;
        state.stack.push(node);
        state.on_stack.insert(node);
        let edges = state.edges;
        for dep in &edges[node] {
            let dep = dep.as_str();
            if !state.index.contains_key(dep) {
                visit(state, dep);
                let low = state.low[node].min(state.low[dep]);
                state.low.insert(node, low);
            } else if state.on_stack.contains(dep) {
                let low = state.low[node].min(state.index[dep]);
                state.low.insert(node, low);
            }
        }
        if state.low[node] == state.index[node] {
            let mut component = Vec::new();
            while let Some(top) = state.stack.pop() {
                state.on_stack.remove(top);
                component.push(top.to_owned());
                if top == node {
                    break;
                }
            }
            let waits_for_itself = state.edges[node].contains(node);
            if component.len() > 1 || waits_for_itself {
                component.sort();
                state.found.push(component);
            }
        }
    }

    let mut state = State {
        edges,
        next: 0,
        index: BTreeMap::new(),
        low: BTreeMap::new(),
        stack: Vec::new(),
        on_stack: BTreeSet::new(),
        found: Vec::new(),
    };
    for node in edges.keys() {
        if !state.index.contains_key(node.as_str()) {
            visit(&mut state, node);
        }
    }
    state.found.sort();
    state.found
}
