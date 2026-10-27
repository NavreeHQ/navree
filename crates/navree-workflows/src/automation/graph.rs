//! [`AutomationGraph`]: an automation's definition, and its validation
//! against the [`Catalog`].

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

use super::blocks::Catalog;
use crate::DefinitionIssue;

/// The most nodes one automation may have.
pub const MAX_NODES: usize = 200;

/// The longest node id.
const MAX_NODE_ID_LEN: usize = 100;

/// An automation's definition: the trigger that enrolls, and the nodes an
/// enrollment walks through, starting at the first one.
///
/// ```json
/// {
///   "trigger": {"type": "deal.won", "config": {"min_amount_cents": 100000}},
///   "nodes": [
///     {"id": "thank-you", "kind": "action", "type": "mail.send",
///      "config": {"template": "deal-won"}, "next": "three-days"},
///     {"id": "three-days", "kind": "delay", "seconds": 259200, "next": "big-deal"},
///     {"id": "big-deal", "kind": "condition", "type": "field",
///      "config": {"path": "event.amount_cents", "op": "gte", "value": 1000000},
///      "then": "call", "else": null},
///     {"id": "call", "kind": "action", "type": "task.create",
///      "config": {"title": "Call the customer"}}
///   ]
/// }
/// ```
///
/// Nodes form a graph without loops: each runs at most once per
/// enrollment. A node without `next` (or a branch without a target) ends
/// the enrollment. `ui` fields are kept as they are, for an editor's own
/// data (positions, notes).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AutomationGraph {
    /// The trigger that enrolls.
    pub trigger: BlockRef,
    /// The nodes; the first one runs first.
    #[serde(default)]
    pub nodes: Vec<Node>,
    /// Editor data, kept as is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<Object>)]
    pub ui: Option<Value>,
}

/// A registered block and its configuration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct BlockRef {
    /// The block's type name (`deal.won`, `mail.send`).
    #[serde(rename = "type")]
    pub block_type: String,
    /// Its configuration: the block type's JSON form.
    #[serde(default)]
    #[schema(value_type = Object)]
    pub config: Value,
}

impl BlockRef {
    /// Block `block_type` with `config`.
    pub fn new(block_type: impl Into<String>, config: Value) -> Self {
        Self {
            block_type: block_type.into(),
            config,
        }
    }
}

/// One node of an [`AutomationGraph`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Node {
    /// Unique within the definition; later nodes read what this one
    /// returned as `nodes.<id>`. 1 to 100 characters, no `#`, not starting
    /// with `$`.
    pub id: String,
    /// What the node does.
    #[serde(flatten)]
    pub step: NodeStep,
    /// Editor data, kept as is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<Object>)]
    pub ui: Option<Value>,
}

/// What a [`Node`] does (its `kind`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NodeStep {
    /// Runs an [`Action`](crate::Action).
    Action {
        /// The action's type name.
        #[serde(rename = "type")]
        block_type: String,
        /// Its configuration.
        #[serde(default)]
        #[schema(value_type = Object)]
        config: Value,
        /// The node after it.
        #[serde(default)]
        next: Option<String>,
    },
    /// Evaluates a [`Condition`](crate::Condition) and branches.
    Condition {
        /// The condition's type name (`field` is built in).
        #[serde(rename = "type")]
        block_type: String,
        /// Its configuration.
        #[serde(default)]
        #[schema(value_type = Object)]
        config: Value,
        /// The node when it holds.
        #[serde(default)]
        then: Option<String>,
        /// The node when it does not.
        #[serde(default, rename = "else")]
        otherwise: Option<String>,
    },
    /// Waits, durably.
    Delay {
        /// How long, in seconds (at least 1).
        seconds: u64,
        /// The node after it.
        #[serde(default)]
        next: Option<String>,
    },
    /// Waits for a trigger's event (for the same subject, by default),
    /// at most `timeout_seconds`.
    Wait {
        /// The trigger whose event ends the wait, and its configuration.
        trigger: BlockRef,
        /// Only the enrolled subject's events count (default true; needs a
        /// trigger with a `SUBJECT`).
        #[serde(default = "yes")]
        same_subject: bool,
        /// Give up after this many seconds (none: wait for good).
        #[serde(default)]
        timeout_seconds: Option<u64>,
        /// The node when the event came; its payload is the node's output.
        #[serde(default)]
        next: Option<String>,
        /// The node when the wait timed out.
        #[serde(default)]
        on_timeout: Option<String>,
    },
}

fn yes() -> bool {
    true
}

impl NodeStep {
    /// The `kind` name.
    pub fn kind(&self) -> &'static str {
        match self {
            NodeStep::Action { .. } => "action",
            NodeStep::Condition { .. } => "condition",
            NodeStep::Delay { .. } => "delay",
            NodeStep::Wait { .. } => "wait",
        }
    }

    /// The block type name, for actions, conditions and waits.
    pub fn block_type(&self) -> Option<&str> {
        match self {
            NodeStep::Action { block_type, .. } | NodeStep::Condition { block_type, .. } => {
                Some(block_type)
            }
            NodeStep::Wait { trigger, .. } => Some(&trigger.block_type),
            NodeStep::Delay { .. } => None,
        }
    }

    /// The nodes it may go to, with the field naming each.
    pub fn targets(&self) -> Vec<(&'static str, &str)> {
        let pairs: Vec<(&'static str, &Option<String>)> = match self {
            NodeStep::Action { next, .. } | NodeStep::Delay { next, .. } => vec![("next", next)],
            NodeStep::Condition {
                then, otherwise, ..
            } => vec![("then", then), ("else", otherwise)],
            NodeStep::Wait {
                next, on_timeout, ..
            } => vec![("next", next), ("on_timeout", on_timeout)],
        };
        pairs
            .into_iter()
            .filter_map(|(field, target)| target.as_deref().map(|t| (field, t)))
            .collect()
    }
}

impl AutomationGraph {
    /// Reads a definition from JSON; the error is one issue at the root.
    pub fn from_json(value: &Value) -> Result<Self, Vec<DefinitionIssue>> {
        AutomationGraph::deserialize(value)
            .map_err(|error| vec![DefinitionIssue::new("", error.to_string())])
    }

    /// The node `id`.
    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|node| node.id == id)
    }

    /// Every problem against `catalog`: unknown blocks, configurations
    /// their types do not accept, duplicate or invalid ids, targets that
    /// do not exist, loops. Empty when the definition can be published.
    pub fn issues(&self, catalog: &Catalog) -> Vec<DefinitionIssue> {
        let mut issues = Vec::new();
        match catalog.trigger(&self.trigger.block_type) {
            None => issues.push(DefinitionIssue::new(
                "/trigger/type",
                format!("unknown trigger `{}`", self.trigger.block_type),
            )),
            Some(entry) => {
                if let Err(error) = (entry.check)(&self.trigger.config) {
                    issues.push(DefinitionIssue::new("/trigger/config", error));
                }
            }
        }
        if self.nodes.is_empty() {
            issues.push(DefinitionIssue::new(
                "/nodes",
                "add at least one node after the trigger",
            ));
        }
        if self.nodes.len() > MAX_NODES {
            issues.push(DefinitionIssue::new(
                "/nodes",
                format!("an automation has at most {MAX_NODES} nodes"),
            ));
        }
        let mut positions: BTreeMap<&str, usize> = BTreeMap::new();
        for (index, node) in self.nodes.iter().enumerate() {
            let at = format!("/nodes/{index}");
            if let Err(message) = check_node_id(&node.id) {
                issues.push(DefinitionIssue::new(format!("{at}/id"), message));
            } else if positions.insert(&node.id, index).is_some() {
                issues.push(DefinitionIssue::new(
                    format!("{at}/id"),
                    format!("two nodes have the id `{}`", node.id),
                ));
            }
            check_step(&node.step, &at, catalog, &mut issues);
        }
        for (index, node) in self.nodes.iter().enumerate() {
            for (field, target) in node.step.targets() {
                if !positions.contains_key(target) {
                    issues.push(DefinitionIssue::new(
                        format!("/nodes/{index}/{field}"),
                        format!("no node has the id `{target}`"),
                    ));
                }
            }
        }
        if let Some((index, field, target)) = self.find_loop(&positions) {
            issues.push(DefinitionIssue::new(
                format!("/nodes/{index}/{field}"),
                format!("going to `{target}` makes a loop: each node runs at most once"),
            ));
        }
        issues
    }

    /// The first edge that closes a loop: (node index, field, target).
    fn find_loop(&self, positions: &BTreeMap<&str, usize>) -> Option<(usize, &'static str, String)> {
        // 0 = unvisited, 1 = on the current path, 2 = done.
        let mut state = vec![0u8; self.nodes.len()];
        for start in 0..self.nodes.len() {
            if state[start] != 0 {
                continue;
            }
            // Depth-first, with an explicit stack of (node, next edge).
            let mut stack: Vec<(usize, usize)> = vec![(start, 0)];
            state[start] = 1;
            while let Some((index, edge)) = stack.pop() {
                let targets = self.nodes[index].step.targets();
                if let Some((field, target)) = targets.get(edge) {
                    stack.push((index, edge + 1));
                    let Some(&next) = positions.get(target) else {
                        continue;
                    };
                    match state[next] {
                        1 => return Some((index, field, (*target).to_owned())),
                        0 => {
                            state[next] = 1;
                            stack.push((next, 0));
                        }
                        _ => {}
                    }
                } else {
                    state[index] = 2;
                }
            }
        }
        None
    }
}

fn check_node_id(id: &str) -> Result<(), String> {
    if id.is_empty() || id.len() > MAX_NODE_ID_LEN || id.starts_with('$') || id.contains('#') {
        return Err(format!(
            "invalid node id `{id}`: use 1 to {MAX_NODE_ID_LEN} characters without `#`, not \
             starting with `$`"
        ));
    }
    Ok(())
}

fn check_step(step: &NodeStep, at: &str, catalog: &Catalog, issues: &mut Vec<DefinitionIssue>) {
    match step {
        NodeStep::Action {
            block_type, config, ..
        } => match catalog.action(block_type) {
            None => issues.push(DefinitionIssue::new(
                format!("{at}/type"),
                format!("unknown action `{block_type}`"),
            )),
            Some(entry) => {
                if let Err(error) = (entry.check)(config) {
                    issues.push(DefinitionIssue::new(format!("{at}/config"), error));
                }
            }
        },
        NodeStep::Condition {
            block_type, config, ..
        } => match catalog.condition(block_type) {
            None => issues.push(DefinitionIssue::new(
                format!("{at}/type"),
                format!("unknown condition `{block_type}`"),
            )),
            Some(entry) => {
                if let Err(error) = (entry.check)(config) {
                    issues.push(DefinitionIssue::new(format!("{at}/config"), error));
                }
            }
        },
        NodeStep::Delay { seconds, .. } => {
            if *seconds == 0 {
                issues.push(DefinitionIssue::new(
                    format!("{at}/seconds"),
                    "a delay lasts at least 1 second",
                ));
            }
        }
        NodeStep::Wait {
            trigger,
            same_subject,
            timeout_seconds,
            ..
        } => {
            match catalog.trigger(&trigger.block_type) {
                None => issues.push(DefinitionIssue::new(
                    format!("{at}/trigger/type"),
                    format!("unknown trigger `{}`", trigger.block_type),
                )),
                Some(entry) => {
                    if let Err(error) = (entry.check)(&trigger.config) {
                        issues.push(DefinitionIssue::new(format!("{at}/trigger/config"), error));
                    }
                    if *same_subject && entry.subject.is_none() {
                        issues.push(DefinitionIssue::new(
                            format!("{at}/same_subject"),
                            format!(
                                "trigger `{}` has no subject: set same_subject to false",
                                trigger.block_type
                            ),
                        ));
                    }
                }
            }
            if *timeout_seconds == Some(0) {
                issues.push(DefinitionIssue::new(
                    format!("{at}/timeout_seconds"),
                    "a timeout lasts at least 1 second",
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn graphs_round_trip_with_editor_data() {
        let value = json!({
            "trigger": {"type": "deal.won", "config": {}},
            "nodes": [
                {"id": "a", "kind": "action", "type": "mail.send", "config": {"x": 1},
                 "next": "b", "ui": {"x": 10, "y": 20}},
                {"id": "b", "kind": "delay", "seconds": 60},
                {"id": "c", "kind": "condition", "type": "field", "config": {},
                 "then": "a", "else": null},
                {"id": "d", "kind": "wait", "trigger": {"type": "x"}, "timeout_seconds": 5,
                 "on_timeout": "b"}
            ],
            "ui": {"zoom": 1.5}
        });
        let graph = AutomationGraph::from_json(&value).unwrap();
        assert_eq!(graph.nodes.len(), 4);
        assert_eq!(graph.nodes[0].ui, Some(json!({"x": 10, "y": 20})));
        assert_eq!(graph.nodes[2].step.targets(), vec![("then", "a")]);
        assert_eq!(graph.nodes[3].step.targets(), vec![("on_timeout", "b")]);
        assert!(matches!(
            graph.nodes[3].step,
            NodeStep::Wait {
                same_subject: true,
                ..
            }
        ));
        let back = serde_json::to_value(&graph).unwrap();
        assert_eq!(AutomationGraph::from_json(&back).unwrap(), graph);
        assert_eq!(back["ui"], json!({"zoom": 1.5}));
    }

    #[test]
    fn a_bad_shape_is_one_issue_at_the_root() {
        let issues = AutomationGraph::from_json(&json!({"nodes": []})).unwrap_err();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].path, "");
        assert!(issues[0].message.contains("trigger"), "{issues:?}");
    }

    #[test]
    fn loops_are_found() {
        let graph = AutomationGraph::from_json(&json!({
            "trigger": {"type": "t"},
            "nodes": [
                {"id": "a", "kind": "delay", "seconds": 1, "next": "b"},
                {"id": "b", "kind": "delay", "seconds": 1, "next": "c"},
                {"id": "c", "kind": "delay", "seconds": 1, "next": "a"}
            ]
        }))
        .unwrap();
        let positions = graph
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id.as_str(), i))
            .collect();
        assert_eq!(
            graph.find_loop(&positions),
            Some((2, "next", "a".to_owned()))
        );
        let diamond = AutomationGraph::from_json(&json!({
            "trigger": {"type": "t"},
            "nodes": [
                {"id": "a", "kind": "condition", "type": "field", "then": "b", "else": "c"},
                {"id": "b", "kind": "delay", "seconds": 1, "next": "d"},
                {"id": "c", "kind": "delay", "seconds": 1, "next": "d"},
                {"id": "d", "kind": "delay", "seconds": 1}
            ]
        }))
        .unwrap();
        let positions = diamond
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id.as_str(), i))
            .collect();
        assert_eq!(diamond.find_loop(&positions), None);
    }
}
