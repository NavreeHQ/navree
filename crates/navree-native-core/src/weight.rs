//! How much of a size limit a node, a row or an op takes.
//!
//! A limit on the nodes of an answer or of a screen would mean little if the
//! weight could be held in what is not a node: a table's rows, the items of a
//! list of facts, the points of a chart. Those take a client as much to read,
//! hold and draw as a node does, so each counts as one, beside the nodes. What
//! is not counted is what takes no more to hold than the words it was written
//! in: a list of names or of days, the pairs and commands of an action.

use navree_native::node::{Node, TableRow};
use navree_native::{Input, Op, Screen};

use crate::tree::roots;

/// What a node holds that is not a node, and takes as much to hold as one: the
/// parts of a kind with a list of its own.
fn parts(node: &Node) -> usize {
    match node {
        Node::Table(table) => {
            table.rows.len()
                + table.columns.len()
                + table.query.as_ref().map_or(0, |form| form.hidden.len())
        }
        Node::Form(form) => form.hidden.len(),
        Node::Facts(facts) => facts.items.len(),
        Node::Tabs(tabs) => tabs.items.len(),
        Node::Timeline(timeline) => timeline.items.len(),
        Node::Toolbar(toolbar) => toolbar.items.len(),
        Node::Rich(rich) => {
            rich.runs.len()
                + rich.blocks.len()
                + rich
                    .blocks
                    .iter()
                    .map(|block| block.runs.len())
                    .sum::<usize>()
        }
        Node::FeedRow(row) => row.runs.len(),
        Node::Row(row) => row.swipe.len(),
        Node::Chart(chart) => {
            chart.series.len()
                + chart
                    .series
                    .iter()
                    .map(|series| series.values.len())
                    .sum::<usize>()
                + chart.references.len()
                + chart.zones.len()
        }
        Node::Gauge(gauge) => gauge.zones.len(),
        Node::Rings(rings) => rings.rings.len(),
        Node::DayClock(clock) => clock.hours.len() + clock.key.len() + clock.moments.len(),
        Node::Meter(meter) => meter.segments.len(),
        Node::Columns(columns) => {
            columns.parts.len()
                + columns.columns.len()
                + columns
                    .columns
                    .iter()
                    .map(|column| column.values.len())
                    .sum::<usize>()
        }
        Node::Strip(strip) => strip.key.len() + strip.cells.len(),
        Node::Range(range) => range.zones.len(),
        Node::CalendarHeatmap(map) => map.days.len(),
        Node::MatrixHeatmap(map) => map.relations.len(),
        Node::Scatter(scatter) => {
            scatter.series.len()
                + scatter
                    .series
                    .iter()
                    .map(|series| series.points.len())
                    .sum::<usize>()
                + scatter.references.len()
        }
        Node::Field(field) => match &field.input {
            Input::Choice(choice) => choice.options.len(),
            Input::Choices(choices) => choices.options.len(),
            _ => 0,
        },
        _ => 0,
    }
}

/// What a node takes by itself: one, and one for each of its parts.
pub(crate) fn own(node: &Node) -> usize {
    1 + parts(node)
}

/// What `nodes` and the nodes under them take.
pub(crate) fn of_nodes<'a>(nodes: impl IntoIterator<Item = &'a Node>) -> usize {
    let mut tally = Tally::new(usize::MAX);
    tally.nodes(nodes);
    tally.total
}

/// What `rows` take: each, and the nodes in it.
pub(crate) fn of_rows<'a>(rows: impl IntoIterator<Item = &'a TableRow>) -> usize {
    let mut tally = Tally::new(usize::MAX);
    for row in rows {
        tally.row(row);
    }
    tally.total
}

/// What an op brings.
pub(crate) fn of_op(op: &Op) -> usize {
    let mut tally = Tally::new(usize::MAX);
    tally.op(op);
    tally.total
}

/// What a whole screen takes.
pub(crate) fn of_screen(screen: &Screen) -> usize {
    let mut tally = Tally::new(usize::MAX);
    tally.screen(screen);
    tally.total
}

/// A running count against a limit. It stops looking once the count is past
/// the limit: what it holds then is more than the limit, not the whole.
pub(crate) struct Tally {
    total: usize,
    cap: usize,
}

impl Tally {
    pub(crate) fn new(cap: usize) -> Self {
        Self { total: 0, cap }
    }

    /// Whether the count is still within the limit.
    pub(crate) fn within(&self) -> bool {
        self.total <= self.cap
    }

    fn add(&mut self, count: usize) {
        self.total = self.total.saturating_add(count);
    }

    /// Counts each of `start` and every node under it.
    pub(crate) fn nodes<'a>(&mut self, start: impl IntoIterator<Item = &'a Node>) {
        let mut stack: Vec<&Node> = start.into_iter().collect();
        while let Some(node) = stack.pop() {
            self.add(own(node));
            if !self.within() {
                return;
            }
            stack.extend(node.children());
        }
    }

    /// Counts a table row, and the nodes in it.
    pub(crate) fn row(&mut self, row: &TableRow) {
        self.add(1);
        self.nodes(row.nodes());
    }

    /// Counts what a screen holds: its nodes, and the steps of its trail and
    /// the pairs of its search.
    pub(crate) fn screen(&mut self, screen: &Screen) {
        self.add(screen.trail.len());
        self.add(
            screen
                .search
                .as_ref()
                .map_or(0, |search| search.hidden.len()),
        );
        self.nodes(roots(screen));
    }

    /// Counts what an op brings.
    pub(crate) fn op(&mut self, op: &Op) {
        match op {
            Op::Morph { node, .. } | Op::Replace { node, .. } => self.nodes([node]),
            Op::Append { nodes, .. } | Op::Prepend { nodes, .. } | Op::Merge { nodes, .. } => {
                self.nodes(nodes);
            }
            Op::Rows { rows, .. } => {
                for row in rows {
                    self.row(row);
                    if !self.within() {
                        return;
                    }
                }
            }
            Op::Row { row, .. } => self.row(row),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    use serde_json::Value;

    /// The lists of nodes, which the walk down the tree counts node by node.
    const NODES: &[&str] = &[
        "Adaptive.compact",
        "Adaptive.wide",
        "Banner.actions",
        "Card.children",
        "Card.menu",
        "Carousel.children",
        "Dialog.actions",
        "Dialog.children",
        "Disclosure.children",
        "Empty.actions",
        "Fact.value",
        "FeedRow.detail",
        "Form.children",
        "Grid.children",
        "Layers.children",
        "Link.children",
        "List.children",
        "Menu.items",
        "Message.body",
        "Moment.detail",
        "Op.Append.nodes",
        "Op.Merge.nodes",
        "Op.Prepend.nodes",
        "Pane.children",
        "Popover.children",
        "Region.children",
        "Row.detail",
        "Row.menu",
        "Screen.actions",
        "Section.actions",
        "Section.children",
        "Section.footer",
        "Sheet.actions",
        "Sheet.children",
        "Stack.children",
        "Tab.children",
        "TableRow.actions",
        "TableRow.cells",
        "TableRow.detail",
        "ToolbarItem.menu",
    ];

    /// The lists of parts that are not nodes and take as much to hold as one:
    /// each element counts as one, in `parts`, `Tally::row` and
    /// `Tally::screen`.
    const COUNTED: &[&str] = &[
        "Block.runs",
        "CalendarHeatmap.days",
        "Chart.references",
        "Chart.series",
        "Chart.zones",
        "ChoiceInput.options",
        "ChoicesInput.options",
        "ColorStrip.cells",
        "ColorStrip.key",
        "DayClock.hours",
        "DayClock.key",
        "DayClock.moments",
        "FeedRow.runs",
        "Form.hidden",
        "Gauge.zones",
        "Facts.items",
        "MatrixHeatmap.relations",
        "Meter.segments",
        "MiniColumn.values",
        "MiniColumns.columns",
        "MiniColumns.parts",
        "Op.Rows.rows",
        "PointSeries.points",
        "RangeScale.zones",
        "Rich.blocks",
        "Rich.runs",
        "Rings.rings",
        "Row.swipe",
        "Scatter.references",
        "Scatter.series",
        "Screen.trail",
        "Search.hidden",
        "Series.values",
        "Tabs.items",
        "Table.columns",
        "Table.rows",
        "Timeline.items",
        "Toolbar.items",
    ];

    /// What is held to another limit: the ops of an answer, by `MAX_OPS`.
    const CAPPED: &[&str] = &["Envelope.ops"];

    /// Lists left out of the count, because each element is no more than a
    /// short word, a number, or a pair or a command of an action, which a
    /// client holds in about as many bytes as the answer wrote it in; the
    /// answer's own size limit is what bounds them. The icon shapes' strings
    /// are counted by their bytes, once for each icon they are written into
    /// (`Envelope::inline_icons_within`).
    const LEFT: &[&str] = &[
        "Action.Commands.list",
        "Action.Request.fields",
        "CalendarHeatmap.steps",
        "Chart.labels",
        "Chart.levels",
        "ChoicesInput.values",
        "DateInput.off_days",
        "DateInput.off_weekdays",
        "Fact.details",
        "Icon.fills",
        "Icon.paths",
        "IconShape.fills",
        "IconShape.paths",
        "MatrixHeatmap.measures",
        "Op.Merge.gone",
        "RangeInput.off_days",
        "RangeInput.off_weekdays",
        "RegisterPush.categories",
        "Reorder.fields",
        "ScanCode.formats",
        "Series.shown",
        "ToastAction.fields",
        "Web.why",
    ];

    /// Every definition the schema reaches from `name`.
    fn reach(defs: &Value, name: &str, seen: &mut BTreeSet<String>) {
        fn refs(value: &Value, out: &mut Vec<String>) {
            match value {
                Value::Object(map) => {
                    if let Some(Value::String(path)) = map.get("$ref") {
                        out.extend(path.rsplit('/').next().map(str::to_owned));
                    }
                    map.values().for_each(|inner| refs(inner, out));
                }
                Value::Array(list) => list.iter().for_each(|inner| refs(inner, out)),
                _ => {}
            }
        }
        if !seen.insert(name.to_owned()) {
            return;
        }
        let mut found = Vec::new();
        refs(&defs[name], &mut found);
        for next in found {
            reach(defs, &next, seen);
        }
    }

    /// A list the protocol gains, in a kind or anywhere in an answer, fails
    /// here until it is decided: counted with the nodes, or left out for a
    /// reason. So a limit on an answer's nodes can't be got round by a list
    /// nobody thought of.
    #[test]
    fn every_list_in_an_answer_is_counted_or_left_out_on_purpose() {
        let schema = navree_native::schema::document();
        let defs = &schema["$defs"];
        let mut reached = BTreeSet::new();
        reach(defs, "Envelope", &mut reached);
        let mut found = BTreeSet::new();
        for name in &reached {
            for (field, shape) in defs[name]["properties"].as_object().into_iter().flatten() {
                if shape["type"] == "array" {
                    found.insert(format!("{name}.{field}"));
                }
            }
        }
        let decided: BTreeSet<String> = [NODES, COUNTED, CAPPED, LEFT]
            .into_iter()
            .flatten()
            .map(|list| (*list).to_owned())
            .collect();
        let undecided: Vec<_> = found.difference(&decided).collect();
        assert!(
            undecided.is_empty(),
            "lists nobody decided to count or leave out: {undecided:?}"
        );
        let gone: Vec<_> = decided.difference(&found).collect();
        assert!(
            gone.is_empty(),
            "lists decided on that the protocol no longer has: {gone:?}"
        );
    }

    fn node(json: &str) -> Node {
        serde_json::from_str(json).unwrap_or_else(|error| panic!("{json}: {error}"))
    }

    /// What a node takes, for a node of each kind with a list of its own.
    #[test]
    fn a_part_that_is_not_a_node_counts_as_one() {
        let weigh = |json: &str| {
            let mut tally = Tally::new(usize::MAX);
            tally.nodes([&node(json)]);
            tally.total
        };
        assert_eq!(weigh(r#"{"t":"text","value":"Ludhiana"}"#), 1);
        // The table, its three rows, its two columns and the node in a cell.
        assert_eq!(
            weigh(
                r#"{"t":"table","columns":[{"label":"Order"},{"label":"Total"}],"rows":[{},{},{"cells":[{"t":"text","value":"₹507"}]}]}"#
            ),
            1 + 3 + 2 + 1
        );
        assert_eq!(
            weigh(
                r#"{"t":"facts","items":[{"label":"City","value":[{"t":"text","value":"Patiala"}]},{"label":"Pin"}]}"#
            ),
            1 + 2 + 1
        );
        assert_eq!(
            weigh(r#"{"t":"tabs","items":[{"label":"A"},{"label":"B"}]}"#),
            3
        );
        assert_eq!(
            weigh(r#"{"t":"timeline","items":[{"title":"Packed"},{"title":"Sent"}]}"#),
            3
        );
        assert_eq!(
            weigh(r#"{"t":"toolbar","items":[{"title":"Share","icon":{}}]}"#),
            2
        );
        // A chart's series and the points in them.
        assert_eq!(
            weigh(
                r#"{"t":"chart","series":[{"label":"Orders","values":[1,2,null,4]}],"references":[{"value":3}]}"#
            ),
            1 + 1 + 4 + 1
        );
        assert_eq!(
            weigh(
                r#"{"t":"field","name":"city","input":{"t":"choice","options":[{"value":"a","label":"A"},{"value":"b","label":"B"}]}}"#
            ),
            3
        );
    }

    #[test]
    fn a_tally_stops_counting_past_its_limit() {
        let rows = vec!["{}"; 1_000].join(",");
        let table = node(&format!(r#"{{"t":"table","rows":[{rows}]}}"#));
        let mut tally = Tally::new(100);
        tally.nodes([&table]);
        assert!(!tally.within());
        // A table's own parts are counted in one step: the tally says it is
        // over, and what it holds is the whole of them.
        assert_eq!(tally.total, 1_001);
    }
}
