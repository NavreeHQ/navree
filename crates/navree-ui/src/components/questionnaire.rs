//! [`Questionnaire`]: one question at a time, with progress and Previous, Skip and Next.

use std::borrow::Cow;

use super::{Button, ButtonKind, ButtonVariant, Progress, root_methods};
use crate::node::{Element, Node, Uses, el, into_node};

/// What a question asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Single,
    Multiple,
    Text,
}

/// One question of a [`Questionnaire`].
#[derive(Clone, Debug)]
#[must_use]
pub struct Question {
    name: String,
    text: Node,
    help: Option<Node>,
    kind: Kind,
    choices: Vec<(String, String)>,
    required: bool,
}

impl Question {
    fn make(name: impl Into<String>, text: impl Into<Node>, kind: Kind) -> Self {
        Self {
            name: name.into(),
            text: text.into(),
            help: None,
            kind,
            choices: Vec::new(),
            required: false,
        }
    }

    /// A question with one answer out of the choices added with [`choice`](Self::choice),
    /// posted as `name`.
    pub fn single(name: impl Into<String>, text: impl Into<Node>) -> Self {
        Self::make(name, text, Kind::Single)
    }

    /// A question with any number of answers out of the choices, posted as `name` once
    /// for each.
    pub fn multiple(name: impl Into<String>, text: impl Into<Node>) -> Self {
        Self::make(name, text, Kind::Multiple)
    }

    /// A question answered in a line of text, posted as `name`.
    pub fn text(name: impl Into<String>, text: impl Into<Node>) -> Self {
        Self::make(name, text, Kind::Text)
    }

    /// Adds a choice: `value` is posted, `label` is shown.
    pub fn choice(mut self, value: impl Into<String>, label: impl Into<String>) -> Self {
        self.choices.push((value.into(), label.into()));
        self
    }

    /// Adds a line of help under the question.
    pub fn help(mut self, help: impl Into<Node>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// Needs an answer: the browser checks it before Next, and Skip is not offered.
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }
}

/// Which button of a [`Questionnaire`] was pressed: the form posts it as `_move`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum QuestionnaireMove {
    /// Go back one question, keeping what was answered.
    Back,
    /// Go on without answering.
    Skip,
    /// Save the answer and go on.
    Next,
}

impl QuestionnaireMove {
    /// Reads the posted `_move`; anything but the three words is `None`.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "back" => Some(Self::Back),
            "skip" => Some(Self::Skip),
            "next" => Some(Self::Next),
            _ => None,
        }
    }

    /// The question to show after this move from question `step` of `total`: counted from
    /// 0, never before the first, and `total` when the last was answered or skipped (the
    /// questionnaire is finished).
    pub fn target(self, step: usize, total: usize) -> usize {
        match self {
            Self::Back => step.saturating_sub(1),
            Self::Skip | Self::Next => (step + 1).min(total),
        }
    }
}

/// The words a questionnaire writes, in English by default. `{n}` and `{total}` in the
/// progress text are the question's number and how many there are.
#[derive(Clone, Debug)]
pub struct QuestionnaireLabels {
    /// The first button.
    pub previous: Cow<'static, str>,
    /// The button that goes on with no answer.
    pub skip: Cow<'static, str>,
    /// The button that saves the answer.
    pub next: Cow<'static, str>,
    /// The same button on the last question.
    pub finish: Cow<'static, str>,
    /// The progress bar's name and the line beside it.
    pub progress: Cow<'static, str>,
}

impl Default for QuestionnaireLabels {
    fn default() -> Self {
        Self {
            previous: Cow::Borrowed("Previous"),
            skip: Cow::Borrowed("Skip"),
            next: Cow::Borrowed("Next"),
            finish: Cow::Borrowed("Finish"),
            progress: Cow::Borrowed("Question {n} of {total}"),
        }
    }
}

/// A form that asks one question at a time. It is server-driven and needs no script: each
/// press of a button posts the page to `action` with the answers so far kept in hidden
/// fields, the posted `_step` (the question counted from 0) and `_move` (see
/// [`QuestionnaireMove`]), and the server draws the question to show next. The checks
/// are the app's, as for any form; the browser's own check of a required answer runs
/// before Next.
///
/// ```
/// use navree_ui::{Question, Questionnaire, render};
///
/// let form = Questionnaire::new("/welcome")
///     .question(Question::single("team", "How big is your team?").choice("small", "Up to 10").choice("large", "More than 10").required())
///     .question(Question::text("goal", "What do you want to track?"))
///     .step(1)
///     .answers([("team", "small")]);
/// let html = render(&form.into());
/// assert!(html.contains("Question 2 of 2"));
/// assert!(html.contains(r#"name="team" value="small""#), "kept as a hidden field");
/// assert!(html.contains("Finish"));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Questionnaire {
    el: Element,
    action: String,
    questions: Vec<Question>,
    step: usize,
    answers: Vec<(String, String)>,
    error: Option<String>,
    labels: QuestionnaireLabels,
}

impl Questionnaire {
    /// A questionnaire posting to `action`.
    pub fn new(action: impl Into<String>) -> Self {
        Self {
            el: el("form")
                .slot("questionnaire")
                .attr("method", "post")
                .uses(Uses::PARTS_STYLES),
            action: action.into(),
            questions: Vec::new(),
            step: 0,
            answers: Vec::new(),
            error: None,
            labels: QuestionnaireLabels::default(),
        }
    }

    /// Adds a question.
    pub fn question(mut self, question: Question) -> Self {
        self.questions.push(question);
        self
    }

    /// Shows question `step`, counted from 0. A number past the last shows the last.
    pub fn step(mut self, step: usize) -> Self {
        self.step = step;
        self
    }

    /// The answers so far, as `(name, value)`: the ones to the question shown fill it in,
    /// and the rest are kept in hidden fields.
    pub fn answers<N: Into<String>, V: Into<String>>(
        mut self,
        answers: impl IntoIterator<Item = (N, V)>,
    ) -> Self {
        self.answers
            .extend(answers.into_iter().map(|(n, v)| (n.into(), v.into())));
        self
    }

    /// Says what is wrong with the answer to the question shown.
    pub fn error(mut self, error: impl Into<String>) -> Self {
        self.error = Some(error.into());
        self
    }

    /// Sets the words it writes.
    pub fn labels(mut self, labels: QuestionnaireLabels) -> Self {
        self.labels = labels;
        self
    }

    fn into_node(self) -> Node {
        if self.questions.is_empty() {
            return Node::Empty;
        }
        let total = self.questions.len();
        let step = self.step.min(total - 1);
        let labels = self.labels;
        let progress = labels
            .progress
            .replace("{n}", &(step + 1).to_string())
            .replace("{total}", &total.to_string());
        let current = &self.questions[step];
        let last = step + 1 == total;
        let mine: Vec<&str> = self
            .answers
            .iter()
            .filter(|(name, _)| *name == current.name)
            .map(|(_, value)| value.as_str())
            .collect();
        let error_id = format!("question-{}-error", step);
        let hint = current
            .help
            .as_ref()
            .map(|_| format!("question-{}-help", step));
        let described = match (&hint, &self.error) {
            (Some(help), Some(_)) => Some(format!("{help} {error_id}")),
            (Some(help), None) => Some(help.clone()),
            (None, Some(_)) => Some(error_id.clone()),
            (None, None) => None,
        };
        let field: Vec<Element> = match current.kind {
            Kind::Text => vec![
                el("input")
                    .slot("input")
                    .attr("type", "text")
                    .attr("name", current.name.clone())
                    .attr_opt("value", mine.first().map(|v| (*v).to_owned()))
                    .attr_opt("aria-describedby", described.clone())
                    .attr_opt("aria-invalid", self.error.as_ref().map(|_| "true"))
                    .flag("required", current.required),
            ],
            Kind::Single | Kind::Multiple => current
                .choices
                .iter()
                .map(|(value, label)| {
                    let single = current.kind == Kind::Single;
                    el("label").slot("questionnaire-choice").children((
                        el("input")
                            .slot(if single { "radio" } else { "checkbox" })
                            .attr("type", if single { "radio" } else { "checkbox" })
                            .attr("name", current.name.clone())
                            .attr("value", value.clone())
                            .flag("checked", mine.contains(&value.as_str()))
                            .flag("required", current.required && single),
                        el("span").child(label.clone()),
                    ))
                })
                .collect(),
        };
        // The answers to the other questions travel with the form.
        let kept: Vec<Element> = self
            .answers
            .iter()
            .filter(|(name, _)| *name != current.name)
            .map(|(name, value)| {
                el("input")
                    .attr("type", "hidden")
                    .attr("name", name.clone())
                    .attr("value", value.clone())
            })
            .collect();
        let question = el("fieldset")
            .slot("questionnaire-question")
            .attr_opt("aria-describedby", described)
            .children((
                el("legend")
                    .slot("questionnaire-text")
                    .child(current.text.clone()),
                current.help.clone().map(|help| {
                    el("p")
                        .slot("questionnaire-help")
                        .id(hint.clone().unwrap_or_default())
                        .child(help)
                }),
                el("div")
                    .slot("questionnaire-answers")
                    .attr(
                        "data-kind",
                        match current.kind {
                            Kind::Text => "text",
                            Kind::Single | Kind::Multiple => "choices",
                        },
                    )
                    .child(field),
                self.error.clone().map(|error| {
                    el("p")
                        .slot("questionnaire-error")
                        .id(error_id.clone())
                        .attr("role", "alert")
                        .child(error)
                }),
            ));
        let step_field = el("input")
            .attr("type", "hidden")
            .attr("name", "_step")
            .attr("value", step.to_string());
        let button = |label: Cow<'static, str>, action: &'static str, variant: ButtonVariant| {
            Button::new(label.into_owned())
                .kind(ButtonKind::Submit)
                .variant(variant)
                .named("_move", action)
        };
        let actions = el("div").slot("questionnaire-actions").children((
            button(labels.previous.clone(), "back", ButtonVariant::Outline)
                .disabled(step == 0)
                .attr("formnovalidate", ""),
            (!current.required).then(|| {
                button(labels.skip.clone(), "skip", ButtonVariant::Ghost).attr("formnovalidate", "")
            }),
            button(
                if last { labels.finish } else { labels.next },
                "next",
                ButtonVariant::Primary,
            ),
        ));
        self.el
            .attr("action", self.action)
            .children((
                el("div").slot("questionnaire-progress").children((
                    Progress::new(
                        u32::try_from(step + 1).unwrap_or(u32::MAX),
                        u32::try_from(total).unwrap_or(u32::MAX),
                        progress.clone(),
                    ),
                    el("span")
                        .slot("questionnaire-count")
                        .attr("aria-hidden", "true")
                        .child(progress),
                )),
                step_field,
                kept,
                question,
                actions,
            ))
            .into()
    }
}

root_methods!(Questionnaire);
into_node!(Questionnaire);
