//! [`Trace`]: the steps of a process down a line, each with a state, a title, a line of
//! description and detail that opens.

use std::borrow::Cow;

use super::{icon, root_methods};
use crate::icons;
use crate::node::{Element, IntoNodes, Node, Uses, el, into_node};

/// Where a step of a [`Trace`] stands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum TraceState {
    /// It finished: a check.
    #[default]
    Done,
    /// It is going on now: a turning mark, and the step is the current one.
    Running,
    /// It has not started: a dashed ring.
    Pending,
    /// It stopped with an error: a cross.
    Failed,
}

impl TraceState {
    const fn value(self) -> &'static str {
        match self {
            Self::Done => "done",
            Self::Running => "running",
            Self::Pending => "pending",
            Self::Failed => "failed",
        }
    }
}

/// The words a trace says to assistive technology, in English by default: the state of a
/// step is spoken before its title, because the mark that shows it is only a picture.
#[derive(Clone, Debug)]
pub struct TraceLabels {
    /// A step that finished.
    pub done: Cow<'static, str>,
    /// A step going on now.
    pub running: Cow<'static, str>,
    /// A step that has not started.
    pub pending: Cow<'static, str>,
    /// A step that stopped with an error.
    pub failed: Cow<'static, str>,
    /// The summary of a step's detail when it has none of its own.
    pub details: Cow<'static, str>,
}

impl Default for TraceLabels {
    fn default() -> Self {
        Self {
            done: Cow::Borrowed("Done"),
            running: Cow::Borrowed("Running"),
            pending: Cow::Borrowed("Not started"),
            failed: Cow::Borrowed("Failed"),
            details: Cow::Borrowed("Details"),
        }
    }
}

/// One step of a [`Trace`].
#[derive(Clone, Debug)]
#[must_use]
pub struct TraceStep {
    title: Node,
    description: Option<Node>,
    detail: Vec<Node>,
    summary: Option<Node>,
    open: bool,
    state: TraceState,
}

impl TraceStep {
    /// A step called `title`, done unless [`state`](Self::state) says otherwise.
    pub fn new(title: impl Into<Node>) -> Self {
        Self {
            title: title.into(),
            description: None,
            detail: Vec::new(),
            summary: None,
            open: false,
            state: TraceState::Done,
        }
    }

    /// Sets where the step stands.
    pub fn state(mut self, state: TraceState) -> Self {
        self.state = state;
        self
    }

    /// Adds a line under the title: what it did, in a few words.
    pub fn description(mut self, description: impl Into<Node>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Adds detail that opens and closes under the step, with no script: the output of a
    /// command, the reasoning, a list of what was found.
    pub fn detail(mut self, detail: impl IntoNodes) -> Self {
        detail.push_into(&mut self.detail);
        self
    }

    /// Names the control that opens the detail (`Details` by default).
    pub fn summary(mut self, summary: impl Into<Node>) -> Self {
        self.summary = Some(summary.into());
        self
    }

    /// Draws the detail open.
    pub fn open(mut self) -> Self {
        self.open = true;
        self
    }
}

/// The steps of something that happened, or is happening, down a line: what an agent
/// did, how an import went, where a deploy is. Each step has a mark for its state, which
/// is spoken too, a title, a line of description and detail that opens by itself.
///
/// ```
/// use navree_ui::{Trace, TraceState, TraceStep, render};
///
/// let trace = Trace::new("Reconciling the March ledger")
///     .step(TraceStep::new("Read 4,812 entries").description("From the Pune branch"))
///     .step(TraceStep::new("Matched with bank lines").state(TraceState::Running))
///     .step(TraceStep::new("Write the report").state(TraceState::Pending));
/// let html = render(&trace.into());
/// assert!(html.contains(r#"aria-current="step""#));
/// assert!(html.contains("Not started"));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Trace {
    el: Element,
    steps: Vec<TraceStep>,
    labels: TraceLabels,
}

impl Trace {
    /// A trace named `label`, which assistive technology reads before the steps.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            el: el("ol")
                .slot("trace")
                .attr("aria-label", label.into())
                .uses(Uses::PARTS_STYLES),
            steps: Vec::new(),
            labels: TraceLabels::default(),
        }
    }

    /// Adds a step.
    pub fn step(mut self, step: TraceStep) -> Self {
        self.steps.push(step);
        self
    }

    /// Adds steps.
    pub fn steps(mut self, steps: impl IntoIterator<Item = TraceStep>) -> Self {
        self.steps.extend(steps);
        self
    }

    /// Sets the words it says.
    pub fn labels(mut self, labels: TraceLabels) -> Self {
        self.labels = labels;
        self
    }

    fn into_node(self) -> Node {
        let labels = self.labels;
        let steps: Vec<Element> = self
            .steps
            .into_iter()
            .map(|step| {
                let (glyph, word) = match step.state {
                    TraceState::Done => (icons::CIRCLE_CHECK, labels.done.clone()),
                    TraceState::Running => (icons::LOADER_CIRCLE, labels.running.clone()),
                    TraceState::Pending => (icons::CIRCLE_DASHED, labels.pending.clone()),
                    TraceState::Failed => (icons::CIRCLE_X, labels.failed.clone()),
                };
                let detail = (!step.detail.is_empty()).then(|| {
                    el("details")
                        .slot("trace-detail")
                        .flag("open", step.open)
                        .children((
                            el("summary").child(
                                step.summary
                                    .unwrap_or_else(|| Node::text(labels.details.clone())),
                            ),
                            el("div").slot("trace-detail-body").child(step.detail),
                        ))
                });
                el("li")
                    .slot("trace-step")
                    .attr("data-state", step.state.value())
                    .attr_opt(
                        "aria-current",
                        (step.state == TraceState::Running).then_some("step"),
                    )
                    .children((
                        el("span").slot("trace-marker").child(icon(glyph)),
                        el("div").slot("trace-content").children((
                            el("div").slot("trace-title").children((
                                el("span").slot("trace-state").child(word),
                                step.title,
                            )),
                            step.description
                                .map(|text| el("div").slot("trace-description").child(text)),
                            detail,
                        )),
                    ))
            })
            .collect();
        self.el.child(steps).into()
    }
}

root_methods!(Trace);
into_node!(Trace);
