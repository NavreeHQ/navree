//! Wizard: a form in steps, with its progress kept on the server.
//!
//! Each step is a `#[derive(Form)]` struct, so its fields and its rules are written
//! once. The server checks a step before the next opens, a [`Draft`] holds what
//! was typed (it belongs to a person, and its id is in the address), and the last
//! step checks every step again. Back, forward, the stepper and Save and leave are
//! submit buttons that store what was typed without checking it, so nothing typed
//! is lost, and every move is a plain post: it works with JavaScript off.
//!
//! The wizard decides; the app stores. A handler loads the draft, asks the wizard
//! what a post means, saves the draft and answers:
//!
//! ```ignore
//! async fn post(Path((id, step)): Path<(String, String)>, Pairs(pairs): Pairs, store: Drafts) -> Response {
//!     let wizard = deal_wizard();
//!     let Some(mut draft) = store.load(&me, &id).await? else { return not_found() };
//!     let before = draft.revision;
//!     let outcome = wizard.submit(&mut draft, &step, &pairs);
//!     if outcome.changed() {
//!         store.save(&me, &draft, Some(before)).await?; // two posts at once: one wins
//!     }
//!     match outcome {
//!         WizardOutcome::Go(to) | WizardOutcome::Invalid(to) => redirect(wizard.url(&id, &to)),
//!         WizardOutcome::Left => redirect("/deals"),
//!         WizardOutcome::Done => { create_deal(&draft)?; store.discard(&me, &id).await?; redirect("/deals") }
//!         WizardOutcome::Stale => stale_page(&wizard, &draft),
//!         WizardOutcome::Refused(_) => bad_request(),
//!     }
//! }
//! ```

use std::borrow::Cow;
use std::sync::Arc;

use serde::de::DeserializeOwned;

use super::draft::{
    Draft, MAX_DRAFT_BYTES, MAX_STEP_VALUES, MAX_STEPS, MAX_VALUE_BYTES, StepRecord, is_step_id,
};
use super::stepper::{Step, StepState, Stepper, StepperLabels};
use crate::components::{Alert, AlertTone, Button, ButtonVariant, Size};
use crate::form::{self, FieldErrors, Form, FormView, Widget};
use crate::node::{Element, Node, Uses, el};
use navree_validation::Validate;

/// The name of the review step, which an app can't use for a step of its own.
pub const REVIEW: &str = "review";
/// The name of the field that says what a post is for.
pub const ACTION_FIELD: &str = "_wizard";
/// The name of the field that carries the draft's revision.
pub const REVISION_FIELD: &str = "_rev";

type Pairs = [(String, String)];
type Render = Arc<dyn Fn(&Pairs, &FieldErrors) -> Node + Send + Sync>;
type Check = Arc<dyn Fn(&Pairs) -> Result<(), FieldErrors> + Send + Sync>;
type Rows = Arc<dyn Fn(&Pairs, &str, &str) -> Vec<(String, String)> + Send + Sync>;

/// What a post to a wizard asks for: the value of its `_wizard` field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WizardAction {
    /// Check this step and open the next.
    Next,
    /// Keep what was typed and open the step before.
    Back,
    /// Keep what was typed and leave; the draft waits.
    Save,
    /// Check every step and finish.
    Finish,
    /// Keep what was typed and open the step with this id.
    Goto(String),
}

impl WizardAction {
    /// Reads a `_wizard` field. Anything else is `None`.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "next" => Some(Self::Next),
            "back" => Some(Self::Back),
            "save" => Some(Self::Save),
            "finish" => Some(Self::Finish),
            other => other
                .strip_prefix("goto:")
                .filter(|id| is_step_id(id))
                .map(|id| Self::Goto(id.to_owned())),
        }
    }

    fn text(&self) -> String {
        match self {
            Self::Next => "next".to_owned(),
            Self::Back => "back".to_owned(),
            Self::Save => "save".to_owned(),
            Self::Finish => "finish".to_owned(),
            Self::Goto(id) => format!("goto:{id}"),
        }
    }
}

/// Why a post was refused outright.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum WizardRefusal {
    /// The step in the address is none of the wizard's.
    UnknownStep,
    /// The post asks for something the wizard doesn't do, or here.
    UnknownAction,
    /// An earlier step is not done: the step can't be reached yet.
    NotReachable,
    /// The draft is already submitted.
    AlreadySubmitted,
    /// The draft would pass the limits on its size.
    TooLarge,
}

/// What a post meant, once the wizard has changed the draft.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum WizardOutcome {
    /// Open this step.
    Go(String),
    /// A step has something to fix: open it, and its errors are there.
    Invalid(String),
    /// Saved; the person is leaving.
    Left,
    /// Every step is valid and the draft is submitted. Create what the wizard was
    /// for, then discard the draft.
    Done,
    /// The form was drawn from an older revision: another window changed the draft.
    /// Nothing was changed. Draw the draft as it is, with a notice.
    Stale,
    /// Nothing was changed.
    Refused(WizardRefusal),
}

impl WizardOutcome {
    /// Whether the draft changed, so it must be saved.
    pub fn changed(&self) -> bool {
        matches!(
            self,
            Self::Go(_) | Self::Invalid(_) | Self::Left | Self::Done
        )
    }
}

/// Why a page can't be drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum WizardError {
    /// The step in the address is none of the wizard's.
    UnknownStep,
    /// An earlier step is not done.
    NotReachable,
}

/// The words a wizard writes. English by default.
#[derive(Clone, Debug)]
#[allow(missing_docs)]
pub struct WizardLabels {
    pub next: Cow<'static, str>,
    pub back: Cow<'static, str>,
    pub finish: Cow<'static, str>,
    pub save: Cow<'static, str>,
    pub edit: Cow<'static, str>,
    pub review: Cow<'static, str>,
    pub review_hint: Cow<'static, str>,
    pub needs_attention: Cow<'static, str>,
    pub nothing: Cow<'static, str>,
    pub yes: Cow<'static, str>,
    pub no: Cow<'static, str>,
    pub stale: Cow<'static, str>,
    pub stepper: Cow<'static, str>,
}

impl Default for WizardLabels {
    fn default() -> Self {
        Self {
            next: "Next".into(),
            back: "Back".into(),
            finish: "Finish".into(),
            save: "Save and leave".into(),
            edit: "Edit".into(),
            review: "Review".into(),
            review_hint: "Check your answers. Nothing is created until you finish.".into(),
            needs_attention: "This step needs attention.".into(),
            nothing: "—".into(),
            yes: "Yes".into(),
            no: "No".into(),
            stale: "This draft was changed in another window. This is the latest.".into(),
            stepper: "Steps".into(),
        }
    }
}

/// One step of a [`Wizard`]. Make one with [`WizardStep::of`].
#[derive(Clone)]
pub struct WizardStep {
    id: String,
    title: Node,
    description: Option<Node>,
    optional: bool,
    /// The names a post may carry for this step, and are kept.
    allowed: Vec<String>,
    /// The names of passwords and files: read to check the step, never kept.
    secret: Vec<String>,
    check: Check,
    render: Render,
    rows: Rows,
}

impl std::fmt::Debug for WizardStep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WizardStep")
            .field("id", &self.id)
            .field("optional", &self.optional)
            .finish_non_exhaustive()
    }
}

/// A step being described: the form it is, and how it is shown. Turns into a
/// [`WizardStep`] where a wizard takes one.
#[must_use]
pub struct WizardStepOf<T> {
    id: String,
    title: Node,
    description: Option<Node>,
    optional: bool,
    shape: Option<Arc<dyn Fn(FormView<T>) -> FormView<T> + Send + Sync>>,
}

impl WizardStep {
    /// A step whose fields and rules are the struct `T`'s.
    ///
    /// `id` is how the step is named in addresses and posts: letters, digits, `-`
    /// and `_`. Anything else in it becomes `-`.
    pub fn of<T>(id: impl Into<String>, title: impl Into<Node>) -> WizardStepOf<T>
    where
        T: Form + DeserializeOwned + Validate + 'static,
    {
        WizardStepOf {
            id: clean_id(&id.into()),
            title: title.into(),
            description: None,
            optional: false,
            shape: None,
        }
    }
}

impl<T> WizardStepOf<T> {
    /// A line under the step's title.
    pub fn description(mut self, description: impl Into<Node>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Lets the step be skipped: it needn't be visited, but if it was, it must be
    /// valid.
    pub fn optional(mut self) -> Self {
        self.optional = true;
        self
    }

    /// Shapes the step's form: its choices from the database, the dates' locale and
    /// the like.
    ///
    /// ```ignore
    /// WizardStep::of::<Basics>("basics", "Basics").form(|v| v.options("owner", owners.clone()))
    /// ```
    pub fn form(
        mut self,
        shape: impl Fn(FormView<T>) -> FormView<T> + Send + Sync + 'static,
    ) -> Self {
        self.shape = Some(Arc::new(shape));
        self
    }
}

/// Makes a step id safe: letters, digits, `-` and `_`, at most 64.
fn clean_id(id: &str) -> String {
    let id: String = id
        .chars()
        .take(64)
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect();
    if id.is_empty() { "step".to_owned() } else { id }
}

/// The names a post may carry for the fields of `T`: the ones that are kept and
/// the ones (passwords, files) that are only read.
fn field_names<T: Form>() -> (Vec<String>, Vec<String>) {
    let (mut kept, mut secret) = (Vec::new(), Vec::new());
    for field in T::FIELDS {
        if matches!(
            field.widget,
            Widget::Input(crate::components::InputKind::Password | crate::components::InputKind::File)
        ) {
            secret.push(field.name.to_owned());
            continue;
        }
        kept.push(field.name.to_owned());
        if field.widget.is_phone() {
            kept.push(format!("{}_country", field.name));
            kept.push(format!("{}_e164", field.name));
        }
    }
    (kept, secret)
}

impl<T> From<WizardStepOf<T>> for WizardStep
where
    T: Form + DeserializeOwned + Validate + 'static,
{
    fn from(step: WizardStepOf<T>) -> Self {
        let shape = step.shape;
        let view = move |values: &Pairs, errors: &FieldErrors| {
            let view = FormView::<T>::new()
                .with_values(values.to_vec())
                .with_errors(errors.clone());
            match &shape {
                Some(shape) => shape(view),
                None => view,
            }
        };
        let view = Arc::new(view);
        let for_render = view.clone();
        let for_rows = view;
        let (allowed, secret) = field_names::<T>();
        Self {
            id: step.id,
            title: step.title,
            description: step.description,
            optional: step.optional,
            allowed,
            secret,
            check: Arc::new(|values: &Pairs| form::decode::<T>(values).map(|_| ())),
            render: Arc::new(move |values: &Pairs, errors: &FieldErrors| {
                for_render(values, errors).fields_node()
            }),
            rows: Arc::new(move |values: &Pairs, yes: &str, no: &str| {
                for_rows(values, &FieldErrors::new()).summary_rows(yes, no)
            }),
        }
    }
}

/// What a post carried for a step.
struct Picked {
    /// What is kept.
    kept: Vec<(String, String)>,
    /// Everything read: the kept values and the passwords and files.
    all: Vec<(String, String)>,
    /// Whether a password or a file came with a value.
    secret_given: bool,
}

impl WizardStep {
    fn has_secrets(&self) -> bool {
        !self.secret.is_empty()
    }

    /// What a post carries for this step, and nothing else. `None` when it carries
    /// too much.
    fn pick(&self, pairs: &Pairs) -> Option<Picked> {
        let (mut kept, mut all, mut secret_given) = (Vec::new(), Vec::new(), false);
        for (name, value) in pairs {
            let keeps = self.allowed.iter().any(|allowed| allowed == name);
            let secret = !keeps && self.secret.iter().any(|secret| secret == name);
            if !keeps && !secret {
                continue;
            }
            if value.len() > MAX_VALUE_BYTES || all.len() >= MAX_STEP_VALUES {
                return None;
            }
            if keeps {
                kept.push((name.clone(), value.clone()));
            } else if !value.is_empty() {
                secret_given = true;
            }
            all.push((name.clone(), value.clone()));
        }
        Some(Picked { kept, all, secret_given })
    }
}

/// A form in steps.
#[derive(Clone, Debug)]
#[must_use]
pub struct Wizard {
    id: String,
    base: String,
    steps: Vec<WizardStep>,
    review: bool,
    linear: bool,
    vertical: bool,
    leave: bool,
    labels: WizardLabels,
    stepper_labels: StepperLabels,
}

impl Wizard {
    /// A wizard with no steps. `id` names it in the page and tells two wizards on
    /// one page apart.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: clean_id(&id.into()),
            base: String::new(),
            steps: Vec::new(),
            review: false,
            linear: true,
            vertical: false,
            leave: true,
            labels: WizardLabels::default(),
            stepper_labels: StepperLabels::default(),
        }
    }

    /// Sets where the wizard's pages are: a step is at `{base}/{draft id}/{step id}`.
    pub fn base(mut self, base: impl Into<String>) -> Self {
        self.base = base.into().trim_end_matches('/').to_owned();
        self
    }

    /// Adds a step. A step whose id another has gets a number added to it, and the
    /// id `review` is taken by the review step. At most 100 steps are kept.
    pub fn step(mut self, step: impl Into<WizardStep>) -> Self {
        let mut step = step.into();
        if self.steps.len() >= MAX_STEPS {
            return self;
        }
        let taken = |id: &str, steps: &[WizardStep]| id == REVIEW || steps.iter().any(|s| s.id == id);
        let base = step.id.clone();
        let mut n = 1;
        while taken(&step.id, &self.steps) {
            n += 1;
            let tail = format!("-{n}");
            step.id = format!("{}{tail}", &base[..base.len().min(64 - tail.len())]);
        }
        self.steps.push(step);
        self
    }

    /// Ends with a review: every step's answers as text, each with a button that
    /// opens that step, and the Finish button.
    pub fn review(mut self) -> Self {
        self.review = true;
        self
    }

    /// Lets the steps be done in any order. The last step still checks them all.
    pub fn free(mut self) -> Self {
        self.linear = false;
        self
    }

    /// Stacks the stepper on the side.
    pub fn vertical(mut self) -> Self {
        self.vertical = true;
        self
    }

    /// Leaves out the Save and leave button.
    pub fn no_leave(mut self) -> Self {
        self.leave = false;
        self
    }

    /// Sets the words.
    pub fn labels(mut self, labels: WizardLabels) -> Self {
        self.labels = labels;
        self
    }

    /// Sets the words of the stepper.
    pub fn stepper_labels(mut self, labels: StepperLabels) -> Self {
        self.stepper_labels = labels;
        self
    }

    /// The address of a step of a draft.
    pub fn url(&self, draft_id: &str, step_id: &str) -> String {
        format!("{}/{draft_id}/{step_id}", self.base)
    }

    /// A new draft on the first step.
    pub fn start(&self) -> Draft {
        Draft::new(self.id_at(0).unwrap_or(REVIEW))
    }

    /// How many steps there are, the review included.
    pub fn len(&self) -> usize {
        self.steps.len() + usize::from(self.review)
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn id_at(&self, at: usize) -> Option<&str> {
        match at.cmp(&self.steps.len()) {
            std::cmp::Ordering::Less => Some(&self.steps[at].id),
            std::cmp::Ordering::Equal if self.review => Some(REVIEW),
            _ => None,
        }
    }

    fn locate(&self, id: &str) -> Option<usize> {
        if self.review && id == REVIEW {
            return Some(self.steps.len());
        }
        self.steps.iter().position(|step| step.id == id)
    }

    fn values<'a>(&self, draft: &'a Draft, at: usize) -> &'a Pairs {
        self.steps
            .get(at)
            .and_then(|step| draft.steps.get(&step.id))
            .map_or(&[], |record| record.values.as_slice())
    }

    fn visited(&self, draft: &Draft, at: usize) -> bool {
        self.steps
            .get(at)
            .and_then(|step| draft.steps.get(&step.id))
            .is_some_and(|record| record.visited)
    }

    /// Whether the step has been moved on from and its rules hold. A step with a
    /// password in it can't be checked again, since the password isn't kept: it
    /// holds if it held when it was tried.
    fn complete(&self, draft: &Draft, at: usize) -> bool {
        self.steps.get(at).is_some_and(|step| {
            if !self.visited(draft, at) {
                return false;
            }
            if step.has_secrets() {
                draft.steps.get(&step.id).is_some_and(|record| record.valid)
            } else {
                (step.check)(self.values(draft, at)).is_ok()
            }
        })
    }

    /// Whether the steps before `at` allow moving past them.
    fn satisfied(&self, draft: &Draft, at: usize) -> bool {
        match self.steps.get(at) {
            Some(step) => self.complete(draft, at) || (step.optional && !self.visited(draft, at)),
            None => true,
        }
    }

    fn reachable(&self, draft: &Draft, at: usize) -> bool {
        !self.linear || (0..at).all(|before| self.satisfied(draft, before))
    }

    /// The step to open for a draft: where the person was, if it can be reached,
    /// else the last step that can.
    pub fn entry(&self, draft: &Draft) -> String {
        if let Some(at) = self.locate(&draft.step)
            && self.reachable(draft, at)
        {
            return draft.step.clone();
        }
        let last = (0..self.len())
            .take_while(|at| self.reachable(draft, *at))
            .last()
            .unwrap_or(0);
        self.id_at(last).unwrap_or(REVIEW).to_owned()
    }

    /// The state of every step, the way the stepper shows them, with `current` the
    /// open one.
    pub fn states(&self, draft: &Draft, current: &str) -> Vec<(String, StepState)> {
        let now = self.locate(current);
        (0..self.len())
            .map(|at| {
                let state = if Some(at) == now {
                    StepState::Current
                } else if self.steps.get(at).is_some_and(|_| self.visited(draft, at)) {
                    if self.complete(draft, at) {
                        StepState::Done
                    } else {
                        StepState::Error
                    }
                } else if self.reachable(draft, at) {
                    StepState::Upcoming
                } else {
                    StepState::Disabled
                };
                (self.id_at(at).unwrap_or_default().to_owned(), state)
            })
            .collect()
    }

    /// What a post meant. Changes `draft` as it says; save it when
    /// [`WizardOutcome::changed`].
    pub fn submit(&self, draft: &mut Draft, step_id: &str, pairs: &Pairs) -> WizardOutcome {
        use WizardOutcome::{Done, Go, Invalid, Left, Refused, Stale};
        use WizardRefusal::{
            AlreadySubmitted, NotReachable, TooLarge, UnknownAction, UnknownStep,
        };
        if draft.submitted {
            return Refused(AlreadySubmitted);
        }
        let Some(at) = self.locate(step_id) else {
            return Refused(UnknownStep);
        };
        if !self.reachable(draft, at) {
            return Refused(NotReachable);
        }
        let revision = pairs
            .iter()
            .find(|(name, _)| name == REVISION_FIELD)
            .and_then(|(_, value)| value.parse::<u64>().ok());
        if revision != Some(draft.revision) {
            return Stale;
        }
        let Some(action) = pairs
            .iter()
            .find(|(name, _)| name == ACTION_FIELD)
            .and_then(|(_, value)| WizardAction::parse(value))
        else {
            return Refused(UnknownAction);
        };
        let on_form_step = at < self.steps.len();
        // Next and Back are for a step with a form; Finish is for the last step.
        let last = self.len() - 1;
        match &action {
            WizardAction::Next if !on_form_step || at == last => return Refused(UnknownAction),
            WizardAction::Finish if at != last => return Refused(UnknownAction),
            WizardAction::Goto(id) if self.locate(id).is_none() => return Refused(UnknownStep),
            _ => {}
        }
        // What was typed is kept whatever comes next.
        let before = draft.steps.get(&self.steps.get(at).map(|s| s.id.clone()).unwrap_or_default()).cloned();
        if on_form_step {
            let Some(picked) = self.steps[at].pick(pairs) else {
                return Refused(TooLarge);
            };
            let id = self.steps[at].id.clone();
            let visited = before.as_ref().is_some_and(|record| record.visited);
            // Changed values, or a password just typed, are not what was last checked.
            let unchanged = before.as_ref().is_some_and(|record| record.values == picked.kept)
                && !picked.secret_given;
            let valid = if matches!(action, WizardAction::Next | WizardAction::Finish) {
                (self.steps[at].check)(&picked.all).is_ok()
            } else {
                unchanged && before.as_ref().is_some_and(|record| record.valid)
            };
            draft.steps.insert(
                id.clone(),
                StepRecord {
                    values: picked.kept,
                    visited,
                    valid,
                },
            );
            if draft.bytes() > MAX_DRAFT_BYTES {
                match before {
                    Some(record) => draft.steps.insert(id, record),
                    None => draft.steps.remove(&id),
                };
                return Refused(TooLarge);
            }
        }
        let outcome = match action {
            WizardAction::Save => {
                draft.step = step_id.to_owned();
                Left
            }
            WizardAction::Back => {
                let to = self.id_at(at.saturating_sub(1)).unwrap_or(step_id).to_owned();
                draft.step.clone_from(&to);
                Go(to)
            }
            WizardAction::Goto(id) => {
                let Some(to) = self.locate(&id) else {
                    return Refused(UnknownStep);
                };
                if !self.reachable(draft, to) {
                    // What was typed is kept; the post goes nowhere.
                    draft.step = step_id.to_owned();
                    return self.kept(draft);
                }
                draft.step.clone_from(&id);
                Go(id)
            }
            WizardAction::Next => {
                self.mark_visited(draft, at);
                if !self.complete(draft, at) {
                    draft.step = step_id.to_owned();
                    Invalid(step_id.to_owned())
                } else {
                    let to = self.id_at(at + 1).unwrap_or(step_id).to_owned();
                    draft.step.clone_from(&to);
                    Go(to)
                }
            }
            WizardAction::Finish => {
                for each in 0..self.steps.len() {
                    // A skipped optional step has nothing to check.
                    if self.steps[each].optional && !self.visited(draft, each) {
                        continue;
                    }
                    self.mark_visited(draft, each);
                }
                match (0..self.steps.len()).find(|each| {
                    let skipped = self.steps[*each].optional && !self.visited(draft, *each);
                    !skipped && !self.complete(draft, *each)
                }) {
                    Some(bad) => {
                        let id = self.steps[bad].id.clone();
                        draft.step.clone_from(&id);
                        Invalid(id)
                    }
                    None => {
                        draft.step = step_id.to_owned();
                        draft.submitted = true;
                        Done
                    }
                }
            }
        };
        draft.revision += 1;
        outcome
    }

    /// A post that kept what was typed and moved nowhere: a change to save, and the
    /// person stays where they were.
    fn kept(&self, draft: &mut Draft) -> WizardOutcome {
        draft.revision += 1;
        WizardOutcome::Go(draft.step.clone())
    }

    fn mark_visited(&self, draft: &mut Draft, at: usize) {
        if let Some(step) = self.steps.get(at) {
            draft.steps.entry(step.id.clone()).or_default().visited = true;
        }
    }

    /// A page for a step of a draft. The step must be one the draft can reach.
    pub fn page(&self, draft: &Draft, step_id: &str) -> Result<WizardPage, WizardError> {
        let at = self.locate(step_id).ok_or(WizardError::UnknownStep)?;
        if !self.reachable(draft, at) {
            return Err(WizardError::NotReachable);
        }
        Ok(WizardPage {
            wizard: self.clone(),
            draft: draft.clone(),
            at,
            csrf: None,
            notice: None,
        })
    }
}

/// A step of a wizard, ready to draw.
#[derive(Debug)]
#[must_use]
pub struct WizardPage {
    wizard: Wizard,
    draft: Draft,
    at: usize,
    csrf: Option<String>,
    notice: Option<String>,
}

impl WizardPage {
    /// Puts the session's token in the form, for a post with JavaScript off.
    pub fn csrf(mut self, token: impl Into<String>) -> Self {
        self.csrf = Some(token.into());
        self
    }

    /// Shows a notice above the step.
    pub fn notice(mut self, text: impl Into<String>) -> Self {
        self.notice = Some(text.into());
        self
    }

    /// Shows the wizard's notice for a draft changed elsewhere (a
    /// [`WizardOutcome::Stale`]).
    pub fn stale(mut self) -> Self {
        self.notice = Some(self.wizard.labels.stale.to_string());
        self
    }

    /// The step's title as text, for the page's `<title>`.
    pub fn title_text(&self) -> String {
        match self.wizard.steps.get(self.at) {
            Some(step) => step.title.text_content(),
            None => self.wizard.labels.review.to_string(),
        }
    }

    fn stepper(&self) -> Stepper {
        let wizard = &self.wizard;
        let id = &self.wizard.id;
        let current = wizard.id_at(self.at).unwrap_or_default();
        let states = wizard.states(&self.draft, current);
        let steps = (0..wizard.len()).map(|at| {
            let state = states[at].1;
            let (title, description, optional, step_id) = match wizard.steps.get(at) {
                Some(step) => (
                    step.title.clone(),
                    step.description.clone(),
                    step.optional,
                    step.id.clone(),
                ),
                None => (
                    Node::from(wizard.labels.review.to_string()),
                    None,
                    false,
                    REVIEW.to_owned(),
                ),
            };
            let mut step = Step::new(title).state(state);
            if let Some(description) = description {
                step = step.description(description);
            }
            if optional {
                step = step.optional();
            }
            step.submit(ACTION_FIELD, WizardAction::Goto(step_id).text())
        });
        Stepper::new(wizard.labels.stepper.clone())
            .id(format!("{id}-steps"))
            .labels(wizard.stepper_labels.clone())
            .steps(steps)
            .current(self.at)
    }

    fn body(&self) -> Node {
        let wizard = &self.wizard;
        match wizard.steps.get(self.at) {
            Some(step) => {
                let values = wizard.values(&self.draft, self.at);
                // A step the person has moved on from shows what is wrong with it.
                let errors = if wizard.visited(&self.draft, self.at) {
                    (step.check)(values).err().unwrap_or_default()
                } else {
                    FieldErrors::new()
                };
                (step.render)(values, &errors)
            }
            None => self.review(),
        }
    }

    fn review(&self) -> Node {
        let wizard = &self.wizard;
        let words = &wizard.labels;
        let sections: Vec<Element> = wizard
            .steps
            .iter()
            .enumerate()
            .map(|(at, step)| {
                let values = wizard.values(&self.draft, at);
                let rows = (step.rows)(values, &words.yes, &words.no);
                let broken = wizard.visited(&self.draft, at)
                    && (step.check)(values).is_err()
                    || (!wizard.visited(&self.draft, at) && !step.optional);
                let id = format!("{}-review-{}", wizard.id, step.id);
                el("section")
                    .slot("wizard-review-step")
                    .attr("aria-labelledby", id.clone())
                    .flag("data-invalid", broken)
                    .children((
                        el("header").children((
                            el("h3").attr("id", id).child(step.title.clone()),
                            Button::ghost(words.edit.to_string())
                                .size(Size::Sm)
                                .named(ACTION_FIELD, WizardAction::Goto(step.id.clone()).text())
                                .submit()
                                .attr("formnovalidate", "")
                                .attr(
                                    "aria-label",
                                    format!("{} {}", words.edit, step.title.text_content()),
                                ),
                        )),
                        broken.then(|| {
                            el("p")
                                .slot("field-error")
                                .child(words.needs_attention.to_string())
                        }),
                        el("dl").slot("wizard-summary").child(
                            rows.into_iter()
                                .map(|(label, text)| {
                                    el("div").children((
                                        el("dt").child(label),
                                        el("dd").child(if text.is_empty() {
                                            words.nothing.to_string()
                                        } else {
                                            text
                                        }),
                                    ))
                                })
                                .collect::<Vec<_>>(),
                        ),
                    ))
            })
            .collect();
        el("div")
            .slot("wizard-review")
            .child(sections)
            .into()
    }

    fn into_node(self) -> Node {
        let wizard = &self.wizard;
        let words = &wizard.labels;
        let id = &wizard.id;
        let step_id = wizard.id_at(self.at).unwrap_or(REVIEW).to_owned();
        let last = self.at + 1 == wizard.len();
        let on_review = self.at >= wizard.steps.len();
        let heading_id = format!("{id}-heading");
        let (title, description) = match wizard.steps.get(self.at) {
            Some(step) => (step.title.clone(), step.description.clone()),
            None => (
                Node::from(words.review.to_string()),
                Some(Node::from(words.review_hint.to_string())),
            ),
        };
        let primary = if last {
            Button::primary(words.finish.to_string())
                .named(ACTION_FIELD, "finish")
                .submit()
        } else {
            Button::primary(words.next.to_string())
                .named(ACTION_FIELD, "next")
                .submit()
        };
        // The first submit button is the one Enter presses in a field: it must be the
        // primary action, not the first step of the stepper.
        let default_press = el("button")
            .attr("type", "submit")
            .attr("name", ACTION_FIELD)
            .attr("value", if last { "finish" } else { "next" })
            .attr("tabindex", -1_i64)
            .attr("aria-hidden", "true")
            .attr("data-slot", "wizard-default")
            .flag("hidden", true);
        let actions = el("div").slot("wizard-actions").children((
            (self.at > 0).then(|| {
                Button::new(words.back.to_string())
                    .variant(ButtonVariant::Outline)
                    .named(ACTION_FIELD, "back")
                    .submit()
                    .attr("formnovalidate", "")
            }),
            (wizard.leave && !on_review).then(|| {
                Button::ghost(words.save.to_string())
                    .named(ACTION_FIELD, "save")
                    .submit()
                    .attr("formnovalidate", "")
            }),
            primary,
        ));
        let form = el("form")
            .slot("wizard-form")
            .attr("id", id.clone())
            .attr("method", "post")
            .attr("action", wizard.url(&self.draft.id, &step_id))
            .flag("data-ui-form", true)
            .uses(Uses::CORE)
            .children((
                self.csrf.clone().map(form::csrf_field),
                el("input")
                    .attr("type", "hidden")
                    .attr("name", REVISION_FIELD)
                    .attr("value", self.draft.revision.to_string()),
                default_press,
                self.stepper(),
                el("section")
                    .slot("wizard-step")
                    .attr("aria-labelledby", heading_id.clone())
                    .children((
                        self.notice.clone().map(|text| {
                            Alert::new(AlertTone::Warning).title(text)
                        }),
                        el("h2")
                            .slot("wizard-heading")
                            .attr("id", heading_id)
                            .attr("tabindex", -1_i64)
                            .child(title),
                        description
                            .map(|text| el("p").slot("wizard-description").child(text)),
                        self.body(),
                    )),
                actions,
            ));
        el("div")
            .slot("wizard")
            .attr("data-orientation", if wizard.vertical { "vertical" } else { "horizontal" })
            .attr("data-step", step_id)
            .child(form)
            .into()
    }
}

impl From<WizardPage> for Node {
    fn from(page: WizardPage) -> Self {
        page.into_node()
    }
}
