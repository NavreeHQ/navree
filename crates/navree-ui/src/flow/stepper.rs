//! Stepper: the steps of a task and where the person stands in them.
//!
//! A stepper is an ordered list. Each step says its state in words that
//! assistive technology reads (colour is never the only sign), the current step is
//! `aria-current="step"`, and a step is a link, a submit button or plain text. It
//! needs no script. At a narrow width a horizontal stepper keeps the current step's
//! name and writes "Step 2 of 5".

use std::borrow::Cow;

use super::super::components::{Orientation, icon};
use crate::html::Href;
use crate::icons;
use crate::node::{Element, Node, el, into_node};

/// Where a step stands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum StepState {
    /// Not reached yet, but it can be.
    #[default]
    Upcoming,
    /// The step the person is on.
    Current,
    /// Finished and valid.
    Done,
    /// Needs attention: something in it is wrong.
    Error,
    /// Can't be reached yet.
    Disabled,
}

impl StepState {
    /// The state as the stylesheet names it.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Upcoming => "upcoming",
            Self::Current => "current",
            Self::Done => "done",
            Self::Error => "error",
            Self::Disabled => "disabled",
        }
    }
}

/// The words a stepper writes for assistive technology and for a narrow screen.
/// English by default; give your own for another language. `{n}` and `{total}`
/// are filled in.
#[derive(Clone, Debug)]
#[allow(missing_docs)]
pub struct StepperLabels {
    pub done: Cow<'static, str>,
    pub current: Cow<'static, str>,
    pub error: Cow<'static, str>,
    pub disabled: Cow<'static, str>,
    pub optional: Cow<'static, str>,
    pub step_of: Cow<'static, str>,
}

impl Default for StepperLabels {
    fn default() -> Self {
        Self {
            done: "completed".into(),
            current: "current step".into(),
            error: "needs attention".into(),
            disabled: "not available yet".into(),
            optional: "Optional".into(),
            step_of: "Step {n} of {total}".into(),
        }
    }
}

#[derive(Clone, Debug)]
enum Nav {
    None,
    Link(Href),
    Submit { name: &'static str, value: String },
}

/// One step of a [`Stepper`].
#[derive(Clone, Debug)]
#[must_use]
pub struct Step {
    title: Node,
    description: Option<Node>,
    optional: bool,
    glyph: Option<icons::Icon>,
    state: Option<StepState>,
    nav: Nav,
}

impl Step {
    /// A step named `title`.
    pub fn new(title: impl Into<Node>) -> Self {
        Self {
            title: title.into(),
            description: None,
            optional: false,
            glyph: None,
            state: None,
            nav: Nav::None,
        }
    }

    /// A line under the title.
    pub fn description(mut self, description: impl Into<Node>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Marks it as a step that may be skipped.
    pub fn optional(mut self) -> Self {
        self.optional = true;
        self
    }

    /// Draws an icon in the indicator in place of the number.
    pub fn icon(mut self, glyph: icons::Icon) -> Self {
        self.glyph = Some(glyph);
        self
    }

    /// Sets the state. Without one the stepper works it out from
    /// [`Stepper::current`].
    pub fn state(mut self, state: StepState) -> Self {
        self.state = Some(state);
        self
    }

    /// Makes the step a link to `href`.
    pub fn link(mut self, href: impl Into<Href>) -> Self {
        self.nav = Nav::Link(href.into());
        self
    }

    /// Makes the step a submit button of the form it stands in, posting
    /// `name=value` and skipping the browser's own checks: a wizard saves what was
    /// typed and moves, whether or not it is valid yet.
    pub fn submit(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.nav = Nav::Submit {
            name,
            value: value.into(),
        };
        self
    }
}

/// The steps of a task, in order.
///
/// ```
/// use navree_ui::{Step, Stepper, render};
///
/// let stepper = Stepper::new("Setup")
///     .step(Step::new("Account").link("/setup/account"))
///     .step(Step::new("Team"))
///     .step(Step::new("Launch"))
///     .current(1);
/// let html = render(&stepper.into());
/// assert!(html.contains(r#"aria-current="step""#));
/// assert!(html.contains("completed"));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Stepper {
    el: Element,
    steps: Vec<Step>,
    current: Option<usize>,
    orientation: Orientation,
    labels: StepperLabels,
    count: bool,
}

impl Stepper {
    /// A stepper whose list is named `label` for assistive technology.
    pub fn new(label: impl Into<Cow<'static, str>>) -> Self {
        Self {
            el: el("ol")
                .slot("stepper-list")
                .attr("aria-label", label.into()),
            steps: Vec::new(),
            current: None,
            orientation: Orientation::Horizontal,
            labels: StepperLabels::default(),
            count: true,
        }
    }

    /// Adds a step.
    pub fn step(mut self, step: Step) -> Self {
        self.steps.push(step);
        self
    }

    /// Adds steps.
    pub fn steps(mut self, steps: impl IntoIterator<Item = Step>) -> Self {
        self.steps.extend(steps);
        self
    }

    /// The step the person is on, counting from 0. A step with no state of its
    /// own before it is done, after it is upcoming. An index past the end is
    /// the last step.
    pub fn current(mut self, index: usize) -> Self {
        self.current = Some(index);
        self
    }

    /// Stacks the steps, with their descriptions beside the indicators.
    pub fn vertical(mut self) -> Self {
        self.orientation = Orientation::Vertical;
        self
    }

    /// Leaves out the "Step 2 of 5" line a horizontal stepper writes at a narrow
    /// width.
    pub fn no_count(mut self) -> Self {
        self.count = false;
        self
    }

    /// Sets the words.
    pub fn labels(mut self, labels: StepperLabels) -> Self {
        self.labels = labels;
        self
    }

    /// Sets the list's `id`.
    pub fn id(mut self, id: impl Into<crate::node::AttrValue>) -> Self {
        self.el = self.el.id(id);
        self
    }

    fn into_node(self) -> Node {
        let last = self.steps.len().saturating_sub(1);
        let current = self.current.map(|at| at.min(last));
        let states: Vec<StepState> = self
            .steps
            .iter()
            .enumerate()
            .map(|(at, step)| {
                step.state.unwrap_or(match current {
                    Some(now) if at < now => StepState::Done,
                    Some(now) if at == now => StepState::Current,
                    _ => StepState::Upcoming,
                })
            })
            .collect();
        let total = self.steps.len();
        let now = states.iter().position(|state| *state == StepState::Current);
        let items: Vec<Element> = self
            .steps
            .into_iter()
            .zip(states.iter().copied())
            .enumerate()
            .map(|(at, (step, state))| item(at, step, state, &self.labels))
            .collect();
        let count = (self.count && self.orientation == Orientation::Horizontal)
            .then_some(now)
            .flatten()
            .map(|at| {
                el("p")
                    .slot("stepper-count")
                    .attr("aria-hidden", "true")
                    .child(
                        self.labels
                            .step_of
                            .replace("{n}", &(at + 1).to_string())
                            .replace("{total}", &total.to_string()),
                    )
            });
        el("div")
            .slot("stepper")
            .attr("data-orientation", self.orientation.as_str())
            .children((self.el.children(items), count))
            .into()
    }
}

into_node!(Stepper);

/// The indicator's content: the number, a check, an alert or the step's icon.
fn indicator(at: usize, step_icon: Option<icons::Icon>, state: StepState) -> Node {
    match (state, step_icon) {
        (StepState::Done, _) => icon(icons::CHECK),
        (StepState::Error, _) => icon(icons::TRIANGLE_ALERT),
        (_, Some(glyph)) => icon(glyph),
        _ => Node::Text(Cow::Owned((at + 1).to_string())),
    }
}

fn item(at: usize, step: Step, state: StepState, labels: &StepperLabels) -> Element {
    let words = match state {
        StepState::Done => Some(labels.done.clone()),
        StepState::Current => Some(labels.current.clone()),
        StepState::Error => Some(labels.error.clone()),
        StepState::Disabled => Some(labels.disabled.clone()),
        StepState::Upcoming => None,
    };
    let body = (
        el("span")
            .slot("step-indicator")
            .attr("aria-hidden", "true")
            .child(indicator(at, step.glyph, state)),
        el("span").slot("step-text").children((
            el("span").slot("step-title").child(step.title),
            words.map(|words| {
                el("span")
                    .attr("data-ui-sr", "")
                    .child(Node::from(format!(" ({words})")))
            }),
            step.description
                .map(|description| el("span").slot("step-description").child(description)),
            step.optional.then(|| {
                el("span")
                    .slot("step-note")
                    .child(Node::from(labels.optional.clone()))
            }),
        )),
    );
    let can_move = state != StepState::Disabled && state != StepState::Current;
    let trigger = match (&step.nav, can_move) {
        (Nav::Link(href), true) => el("a").href(href.clone()),
        (Nav::Submit { name, value }, true) => el("button")
            .attr("type", "submit")
            .attr("name", *name)
            .attr("value", value.clone())
            .flag("formnovalidate", true),
        (Nav::Submit { .. }, false) if state == StepState::Disabled => {
            // A button that can't be pressed says so itself.
            el("button").attr("type", "button").flag("disabled", true)
        }
        _ => el("div"),
    };
    el("li")
        .slot("step")
        .attr("data-state", state.as_str())
        .child(
            trigger
                .slot("step-trigger")
                .attr_opt("aria-current", (state == StepState::Current).then_some("step"))
                .flag("data-disabled", state == StepState::Disabled)
                .children(body),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::render;

    fn html(stepper: Stepper) -> String {
        render(&stepper.into())
    }

    fn three() -> Stepper {
        Stepper::new("Setup")
            .step(Step::new("Account").link("/a"))
            .step(Step::new("Team").link("/t"))
            .step(Step::new("Launch").link("/l"))
    }

    #[test]
    fn states_follow_the_current_step() {
        let out = html(three().current(1));
        assert!(out.contains(r#"data-state="done""#), "{out}");
        assert!(out.contains(r#"data-state="current""#), "{out}");
        assert!(out.contains(r#"data-state="upcoming""#), "{out}");
        assert_eq!(out.matches(r#"aria-current="step""#).count(), 1, "{out}");
    }

    #[test]
    fn a_state_is_in_words_not_only_in_colour() {
        let out = html(
            three()
                .current(0)
                .step(Step::new("Pay").state(StepState::Error))
                .step(Step::new("Done").state(StepState::Disabled)),
        );
        assert!(out.contains("(current step)"), "{out}");
        assert!(out.contains("(needs attention)"), "{out}");
        assert!(out.contains("(not available yet)"), "{out}");
    }

    #[test]
    fn the_current_step_is_not_a_link_but_the_others_are() {
        let out = html(three().current(1));
        assert_eq!(out.matches("<a ").count(), 2, "{out}");
        assert!(out.contains(r#"href="/a""#), "{out}");
        assert!(!out.contains(r#"href="/t""#), "{out}");
    }

    #[test]
    fn a_disabled_step_is_not_a_link_and_a_disabled_button_is_disabled() {
        let out = html(
            Stepper::new("S")
                .step(Step::new("A").link("/a").state(StepState::Disabled))
                .step(Step::new("B").submit("_wizard", "goto:b").state(StepState::Disabled)),
        );
        assert!(!out.contains("href="), "{out}");
        assert!(out.contains("disabled"), "{out}");
    }

    #[test]
    fn a_submit_step_posts_and_skips_the_browsers_checks() {
        let out = html(
            Stepper::new("S")
                .step(Step::new("A").submit("_wizard", "goto:a"))
                .step(Step::new("B"))
                .current(1),
        );
        assert!(out.contains(r#"type="submit""#), "{out}");
        assert!(out.contains(r#"name="_wizard""#), "{out}");
        assert!(out.contains(r#"value="goto:a""#), "{out}");
        assert!(out.contains("formnovalidate"), "{out}");
    }

    #[test]
    fn done_shows_a_check_and_error_an_alert_and_others_their_number() {
        let out = html(
            Stepper::new("S")
                .step(Step::new("A").state(StepState::Done))
                .step(Step::new("B").state(StepState::Error))
                .step(Step::new("C")),
        );
        assert_eq!(out.matches("<svg").count(), 2, "{out}");
        assert!(out.contains(">3</span>"), "{out}");
    }

    #[test]
    fn a_step_can_carry_an_icon_a_description_and_an_optional_note() {
        let out = html(
            Stepper::new("S").step(
                Step::new("Team")
                    .icon(icons::LOCK)
                    .description("Who is in")
                    .optional(),
            ),
        );
        assert!(out.contains("Who is in"), "{out}");
        assert!(out.contains("Optional"), "{out}");
        assert!(out.contains("<svg"), "{out}");
    }

    #[test]
    fn a_horizontal_stepper_writes_the_count_and_a_vertical_one_does_not() {
        let out = html(three().current(1));
        assert!(out.contains("Step 2 of 3"), "{out}");
        assert!(out.contains(r#"data-slot="stepper-count" aria-hidden="true""#), "{out}");
        assert!(!html(three().current(1).vertical()).contains("stepper-count"));
        assert!(!html(three().current(1).no_count()).contains("stepper-count"));
        assert!(!html(three()).contains("stepper-count"), "no current step");
    }

    #[test]
    fn an_index_past_the_end_is_the_last_step_and_no_steps_is_fine() {
        let out = html(three().current(99));
        assert!(out.contains("Step 3 of 3"), "{out}");
        let empty = html(Stepper::new("none").current(4));
        assert!(empty.contains("<ol"), "{empty}");
        assert!(!empty.contains("<li"), "{empty}");
    }

    #[test]
    fn one_step_and_many_steps_render() {
        assert!(html(Stepper::new("one").step(Step::new("Only")).current(0)).contains("Step 1 of 1"));
        let many = Stepper::new("many").steps((0..50).map(|n| Step::new(format!("Step {n}"))));
        let out = html(many.current(25));
        assert_eq!(out.matches("<li").count(), 50);
        assert!(out.contains("Step 26 of 50"));
    }

    #[test]
    fn text_is_escaped_and_the_words_are_changeable() {
        let out = html(
            Stepper::new("<s>")
                .step(Step::new("<script>").description("\"x\""))
                .labels(StepperLabels {
                    current: "pas actuel".into(),
                    ..StepperLabels::default()
                })
                .current(0),
        );
        assert!(!out.contains("<script>"), "{out}");
        assert!(!out.contains("<s>"), "{out}");
        assert!(out.contains("(pas actuel)"), "{out}");
    }

    #[test]
    fn it_ships_no_script() {
        let node: Node = three().current(0).into();
        assert!(crate::render::measure(&node).uses.is_none());
    }
}
