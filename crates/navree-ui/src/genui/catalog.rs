//! [`Catalog`]: the closed set of components a model may choose for an answer, and the
//! actions their buttons may take.
//!
//! One Rust type per component gives everything about it: the JSON Schema of its tool
//! (from its [`Props`]), the words the model chooses it by (the props' doc comment), the
//! check the model's arguments pass, the example the tests try, and the code that draws
//! it. They cannot drift apart, because there is nothing else to write.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use navree_http::AppError;
use navree_kernel::BoxFuture;
use serde_json::Value;

use super::props::{Checker, LinkPattern, Problem, Prop as _, Props, fits};
use crate::node::Node;

/// What a component's code may ask of the reader it draws for.
pub trait SurfaceCx: Send + Sync {
    /// Whether the reader may do `action` on some records of `subject`: what a
    /// route's `#[can(action, subject)]` checks.
    fn can(&self, action: &str, subject: &str) -> bool;
}

/// A component a model may choose: its props, who may be offered it, and how it is
/// drawn. What it shows is loaded when the page is drawn, under the reader's own access
/// at that moment, so a model never sees the numbers and a reader who lost access sees
/// nothing.
///
/// ```
/// use navree_ui::Text;
/// use navree_ui::genui::{AppError, BoxFuture, Surface, SurfaceCx, SurfaceView};
///
/// navree_ui::choices! {
///     /// The days to count.
///     pub enum Period {
///         /// Monday to now.
///         ThisWeek = "this_week",
///         /// The first of the month to now.
///         ThisMonth = "this_month",
///     }
/// }
///
/// navree_ui::props! {
///     /// How payments did over a period, split by method, as a donut.
///     pub struct PaymentsByMethod {
///         /// The period.
///         pub period: Period,
///     }
/// }
///
/// struct PaymentsDonut;
///
/// impl<Cx: SurfaceCx> Surface<Cx> for PaymentsDonut {
///     type Props = PaymentsByMethod;
///     const NAME: &'static str = "payments_by_method";
///     const CAN: Option<(&'static str, &'static str)> = Some(("read", "Payment"));
///
///     fn title(&self, _props: &PaymentsByMethod) -> String {
///         "Payments by method".to_owned()
///     }
///
///     fn render<'a>(
///         &'a self,
///         props: &'a PaymentsByMethod,
///         _cx: &'a Cx,
///     ) -> BoxFuture<'a, Result<SurfaceView, AppError>> {
///         Box::pin(async move { Ok(SurfaceView::new(Text::new(props.period.as_str()))) })
///     }
/// }
/// ```
pub trait Surface<Cx: SurfaceCx>: Send + Sync + 'static {
    /// What the model chooses: a struct written with [`props!`](crate::props).
    type Props: Props;

    /// Its name: a lower-case letter, then lower-case letters, digits and `_`, at most
    /// 40 in all. The model's tool for it is `show_<name>`.
    const NAME: &'static str;

    /// Who may be offered it: `(action, subject)`, as `#[can]` reads them. A reader
    /// who may not is never offered the tool.
    const CAN: Option<(&'static str, &'static str)> = None;

    /// The actions its buttons may take, by [`Action::NAME`]. A button for any other
    /// is never drawn and never run.
    const ACTIONS: &'static [&'static str] = &[];

    /// What it is called above what it draws: `Payments by method`.
    fn title(&self, props: &Self::Props) -> String;

    /// Draws it for the reader: load what it shows under `cx`'s access, now.
    fn render<'a>(
        &'a self,
        props: &'a Self::Props,
        cx: &'a Cx,
    ) -> BoxFuture<'a, Result<SurfaceView, AppError>>;
}

/// An action a component's button may take: its name, its input (checked as a model's
/// props are), who may take it, and whether the page asks first.
pub trait Action: Send + Sync + 'static {
    /// What the button posts: a struct written with [`props!`](crate::props). The
    /// component's code chooses it when it draws the button; the server signs it, and
    /// checks it again before the action runs.
    type Input: Props;

    /// Its name: a lower-case letter, then lower-case letters, digits and `_`.
    const NAME: &'static str;

    /// Who may take it: `(action, subject)`, checked again on every post.
    const CAN: Option<(&'static str, &'static str)> = None;

    /// Whether it changes something that can't be taken back. The page then asks
    /// first, in a dialog it draws itself, never one a component drew.
    const DESTRUCTIVE: bool = false;
}

/// How an [`Action`] runs on the server, for the reader of the page.
pub trait RunAction<Cx: SurfaceCx>: Action {
    /// Runs it. Load what it changes under `cx`'s access: the record a button names may
    /// no longer be the reader's.
    fn run<'a>(
        &'a self,
        input: Self::Input,
        cx: &'a Cx,
    ) -> BoxFuture<'a, Result<ActionDone, AppError>>;
}

/// What an action did: the words the page shows, and what the assistant hears.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionDone {
    /// Shown to the reader: `Refunded ₹1,250 to Harpreet Kaur`.
    pub message: String,
    /// Told to the assistant before the reader's next message, as data.
    pub note: Option<String>,
}

impl ActionDone {
    /// Done, saying `message`.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            note: None,
        }
    }

    /// Tells the assistant `note` before the reader's next message.
    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }
}

/// What a component drew: its body, and the buttons the toolkit draws below it.
#[derive(Clone, Debug)]
#[must_use]
pub struct SurfaceView {
    pub(crate) body: Node,
    pub(crate) actions: Vec<ActionButton>,
}

impl SurfaceView {
    /// A view of `body`. Forms, fields, buttons, commands and live parts in it are taken
    /// out when it is framed: a component's buttons are [`ActionButton`]s.
    pub fn new(body: impl Into<Node>) -> Self {
        Self {
            body: body.into(),
            actions: Vec::new(),
        }
    }

    /// Adds a button.
    pub fn action(mut self, button: ActionButton) -> Self {
        self.actions.push(button);
        self
    }

    /// The body and the buttons, for the host that signs the buttons.
    pub fn into_parts(self) -> (Node, Vec<ActionButton>) {
        (self.body, self.actions)
    }
}

/// A button below a component: the action it takes, the input it posts, and its words.
/// The host signs it for the reader, the conversation and the component, so it can't be
/// posted by anyone else, for anything else, or twice.
#[derive(Clone, Debug, PartialEq)]
#[must_use]
pub struct ActionButton {
    action: &'static str,
    label: String,
    input: Value,
    destructive: bool,
    confirm: Option<Confirm>,
}

/// The question a page asks before a destructive action, in its own dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Confirm {
    /// The dialog's title: `Refund order CS-1042?`
    pub title: String,
    /// What happens: `₹1,250 goes back to Harpreet Kaur's UPI account.`
    pub body: String,
    /// The button that does it: `Refund ₹1,250`.
    pub yes: String,
}

impl ActionButton {
    /// A button that takes action `A` with `input`, saying `label`.
    pub fn new<A: Action>(label: impl Into<String>, input: &A::Input) -> Self {
        Self {
            action: A::NAME,
            label: label.into(),
            input: input.to_json(),
            destructive: A::DESTRUCTIVE,
            confirm: None,
        }
    }

    /// The question the page asks first. A destructive action without one asks
    /// `Are you sure?` with the button's words.
    pub fn confirm(
        mut self,
        title: impl Into<String>,
        body: impl Into<String>,
        yes: impl Into<String>,
    ) -> Self {
        self.confirm = Some(Confirm {
            title: title.into(),
            body: body.into(),
            yes: yes.into(),
        });
        self
    }

    /// The action's name.
    pub fn action_name(&self) -> &'static str {
        self.action
    }

    /// The words on the button.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// The input, as JSON.
    pub fn input(&self) -> &Value {
        &self.input
    }

    /// Whether the page asks first.
    pub fn is_destructive(&self) -> bool {
        self.destructive
    }

    /// The question asked first, when there is one.
    pub fn question(&self) -> Option<Confirm> {
        if let Some(confirm) = &self.confirm {
            return Some(confirm.clone());
        }
        self.destructive.then(|| Confirm {
            title: "Are you sure?".to_owned(),
            body: format!("{}: this can't be taken back.", self.label),
            yes: self.label.clone(),
        })
    }
}

/// The bounds on what a model may send for one component.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The most bytes of the arguments, as JSON (8 KB by default).
    pub max_bytes: usize,
    /// The most levels of lists and objects (6 by default).
    pub max_depth: usize,
    /// The most values in all (400 by default).
    pub max_values: usize,
    /// The most components in one answer (6 by default).
    pub max_per_answer: usize,
    /// The most refused tries of one component in one answer before the model is told
    /// to answer in words (2 by default).
    pub max_tries: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_bytes: 8 * 1024,
            max_depth: 6,
            max_values: 400,
            max_per_answer: 6,
            max_tries: 2,
        }
    }
}

/// A tool the model is offered for a component.
#[derive(Clone, Debug, PartialEq)]
pub struct CatalogTool {
    /// `show_<name>`.
    pub name: String,
    /// What the model chooses it by.
    pub description: String,
    /// The JSON Schema of its arguments.
    pub parameters: Value,
    /// Who may be offered it.
    pub can: Option<(&'static str, &'static str)>,
}

/// Arguments that passed the check: the component and its props as they are stored.
#[derive(Clone, Debug, PartialEq)]
pub struct Checked {
    /// The component's name.
    pub surface: &'static str,
    /// Its props, as JSON that checks again to the same value.
    pub props: Value,
}

/// Why arguments were refused, in words a model can repair its answer from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    /// What is wrong.
    pub problems: Vec<Problem>,
}

impl Refusal {
    fn one(message: impl Into<String>) -> Self {
        Self {
            problems: vec![Problem {
                path: String::new(),
                message: message.into(),
            }],
        }
    }

    /// The problems in one message.
    pub fn message(&self) -> String {
        self.problems
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ")
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message())
    }
}

/// Why an action's post was refused.
#[derive(Debug)]
#[non_exhaustive]
pub enum ActionRefusal {
    /// No such action, or the component doesn't take it.
    Unknown,
    /// The reader may not take it.
    Forbidden,
    /// Its input doesn't pass the check.
    Invalid(Refusal),
    /// It ran and failed.
    Failed(AppError),
}

/// Whether `name` can name a component or an action.
fn valid_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z'))
        && name.len() <= 40
        && bytes.all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_'))
}

/// One component, as the catalogue holds it.
trait Entry<Cx>: Send + Sync {
    fn name(&self) -> &'static str;
    fn can(&self) -> Option<(&'static str, &'static str)>;
    fn actions(&self) -> &'static [&'static str];
    fn description(&self) -> String;
    fn schema(&self) -> Value;
    fn check(&self, value: &Value, checker: &mut Checker<'_>) -> Option<Value>;
    fn example(&self) -> Value;
    fn render<'a>(
        &'a self,
        props: &'a Value,
        links: &'a [LinkPattern],
        cx: &'a Cx,
    ) -> BoxFuture<'a, Result<(String, SurfaceView), AppError>>;
}

struct SurfaceEntry<S>(S);

impl<Cx: SurfaceCx + 'static, S: Surface<Cx>> Entry<Cx> for SurfaceEntry<S> {
    fn name(&self) -> &'static str {
        S::NAME
    }

    fn can(&self) -> Option<(&'static str, &'static str)> {
        S::CAN
    }

    fn actions(&self) -> &'static [&'static str] {
        S::ACTIONS
    }

    fn description(&self) -> String {
        S::Props::description()
    }

    fn schema(&self) -> Value {
        S::Props::schema()
    }

    fn check(&self, value: &Value, checker: &mut Checker<'_>) -> Option<Value> {
        S::Props::check(value, "", checker).map(|props| props.to_json())
    }

    fn example(&self) -> Value {
        S::Props::example().to_json()
    }

    fn render<'a>(
        &'a self,
        props: &'a Value,
        links: &'a [LinkPattern],
        cx: &'a Cx,
    ) -> BoxFuture<'a, Result<(String, SurfaceView), AppError>> {
        Box::pin(async move {
            let mut checker = Checker::new(links);
            let Some(props) = S::Props::check(props, "", &mut checker) else {
                return Err(
                    AppError::from_status(http::StatusCode::UNPROCESSABLE_ENTITY)
                        .with_detail("the component's stored props no longer pass its check"),
                );
            };
            let title = self.0.title(&props);
            let view = self.0.render(&props, cx).await?;
            Ok((title, view))
        })
    }
}

/// One action, as the catalogue holds it.
trait ActionEntry<Cx>: Send + Sync {
    fn can(&self) -> Option<(&'static str, &'static str)>;
    fn destructive(&self) -> bool;
    fn check(&self, input: &Value, checker: &mut Checker<'_>) -> bool;
    fn run<'a>(
        &'a self,
        input: &'a Value,
        links: &'a [LinkPattern],
        cx: &'a Cx,
    ) -> BoxFuture<'a, Result<ActionDone, ActionRefusal>>;
}

struct RunEntry<A>(A);

impl<Cx: SurfaceCx + 'static, A: RunAction<Cx>> ActionEntry<Cx> for RunEntry<A> {
    fn can(&self) -> Option<(&'static str, &'static str)> {
        A::CAN
    }

    fn destructive(&self) -> bool {
        A::DESTRUCTIVE
    }

    fn check(&self, input: &Value, checker: &mut Checker<'_>) -> bool {
        A::Input::check(input, "", checker).is_some()
    }

    fn run<'a>(
        &'a self,
        input: &'a Value,
        links: &'a [LinkPattern],
        cx: &'a Cx,
    ) -> BoxFuture<'a, Result<ActionDone, ActionRefusal>> {
        Box::pin(async move {
            let mut checker = Checker::new(links);
            let Some(input) = A::Input::check(input, "", &mut checker) else {
                return Err(ActionRefusal::Invalid(Refusal {
                    problems: checker.into_problems(),
                }));
            };
            self.0.run(input, cx).await.map_err(ActionRefusal::Failed)
        })
    }
}

/// The components a model may choose for an answer, and the actions their buttons may
/// take. Build it once, at boot, and give it to the assistant.
///
/// ```
/// # use navree_ui::Text;
/// # use navree_ui::genui::{AppError, BoxFuture, Surface, SurfaceCx, SurfaceView};
/// # navree_ui::props! {
/// #     /// Today's sales.
/// #     pub struct Sales {}
/// # }
/// # struct SalesToday;
/// # impl<Cx: SurfaceCx> Surface<Cx> for SalesToday {
/// #     type Props = Sales;
/// #     const NAME: &'static str = "sales_today";
/// #     fn title(&self, _: &Sales) -> String { "Sales today".into() }
/// #     fn render<'a>(&'a self, _: &'a Sales, _: &'a Cx) -> BoxFuture<'a, Result<SurfaceView, AppError>> {
/// #         Box::pin(async { Ok(SurfaceView::new(Text::new("₹1,00,000"))) })
/// #     }
/// # }
/// # struct Reader;
/// # impl SurfaceCx for Reader { fn can(&self, _: &str, _: &str) -> bool { true } }
/// use navree_ui::genui::Catalog;
/// use serde_json::json;
///
/// let catalog = Catalog::<Reader>::new().surface(SalesToday).link("/orders/{key}");
/// assert_eq!(catalog.tools(&Reader)[0].name, "show_sales_today");
/// assert!(catalog.check("show_sales_today", &json!({})).is_ok());
/// assert!(catalog.check("show_sales_today", &json!({ "html": "<b>" })).is_err());
/// assert!(catalog.check("show_approval", &json!({})).is_err());
/// ```
pub struct Catalog<Cx> {
    entries: BTreeMap<String, Arc<dyn Entry<Cx>>>,
    actions: BTreeMap<&'static str, Arc<dyn ActionEntry<Cx>>>,
    links: Vec<LinkPattern>,
    limits: Limits,
}

impl<Cx> Clone for Catalog<Cx> {
    fn clone(&self) -> Self {
        Self {
            entries: self.entries.clone(),
            actions: self.actions.clone(),
            links: self.links.clone(),
            limits: self.limits,
        }
    }
}

impl<Cx> fmt::Debug for Catalog<Cx> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Catalog")
            .field("surfaces", &self.entries.keys().collect::<Vec<_>>())
            .field("actions", &self.actions.keys().collect::<Vec<_>>())
            .field("links", &self.links)
            .field("limits", &self.limits)
            .finish()
    }
}

impl<Cx: SurfaceCx + 'static> Default for Catalog<Cx> {
    fn default() -> Self {
        Self::new()
    }
}

/// The prefix of a component's tool.
pub const TOOL_PREFIX: &str = "show_";

impl<Cx: SurfaceCx + 'static> Catalog<Cx> {
    /// An empty catalogue: the model may choose nothing.
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            actions: BTreeMap::new(),
            links: Vec::new(),
            limits: Limits::default(),
        }
    }

    /// Adds a component.
    ///
    /// # Panics
    ///
    /// When its name is not a lower-case name of at most 40 characters, or the
    /// catalogue has one of that name: both are mistakes in the app's code, found at
    /// boot.
    pub fn surface<S: Surface<Cx>>(mut self, surface: S) -> Self {
        assert!(
            valid_name(S::NAME),
            "a component's name is a lower-case letter, then lower-case letters, digits and `_`, at most 40: `{}`",
            S::NAME
        );
        let tool = format!("{TOOL_PREFIX}{}", S::NAME);
        assert!(
            !self.entries.contains_key(&tool),
            "the catalogue has a component `{}` already",
            S::NAME
        );
        self.entries.insert(tool, Arc::new(SurfaceEntry(surface)));
        self
    }

    /// Adds an action components' buttons may take.
    ///
    /// # Panics
    ///
    /// When its name is not a lower-case name, or the catalogue has one of that name.
    pub fn action<A: RunAction<Cx>>(mut self, action: A) -> Self {
        assert!(
            valid_name(A::NAME),
            "an action's name is a lower-case letter, then lower-case letters, digits and `_`, at most 40: `{}`",
            A::NAME
        );
        assert!(
            !self.actions.contains_key(A::NAME),
            "the catalogue has an action `{}` already",
            A::NAME
        );
        self.actions.insert(A::NAME, Arc::new(RunEntry(action)));
        self
    }

    /// Lets components link to the pages of `pattern`: `/orders/{key}`.
    ///
    /// # Panics
    ///
    /// When the pattern is not plain segments and `{key}`.
    pub fn link(mut self, pattern: &str) -> Self {
        match LinkPattern::parse(pattern) {
            Ok(pattern) => self.links.push(pattern),
            Err(error) => panic!("{error}"),
        }
        self
    }

    /// Replaces the bounds on what a model may send.
    pub fn limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    /// The bounds.
    pub fn bounds(&self) -> Limits {
        self.limits
    }

    /// The pages components may link to.
    pub fn links(&self) -> &[LinkPattern] {
        &self.links
    }

    /// Whether `tool` is one of the catalogue's tools.
    pub fn is_tool(&self, tool: &str) -> bool {
        self.entries.contains_key(tool)
    }

    /// The names of the components.
    pub fn names(&self) -> Vec<&'static str> {
        self.entries.values().map(|entry| entry.name()).collect()
    }

    /// Every component's tool, whoever reads.
    pub fn all_tools(&self) -> Vec<CatalogTool> {
        self.entries
            .iter()
            .map(|(tool, entry)| CatalogTool {
                name: tool.clone(),
                description: tool_description(&entry.description()),
                parameters: entry.schema(),
                can: entry.can(),
            })
            .collect()
    }

    /// The tools offered to `cx`'s reader: the components they may be offered.
    pub fn tools(&self, cx: &Cx) -> Vec<CatalogTool> {
        self.all_tools()
            .into_iter()
            .filter(|tool| {
                tool.can
                    .is_none_or(|(action, subject)| cx.can(action, subject))
            })
            .collect()
    }

    /// What the system prompt says about the components: that they exist, that the
    /// page draws them from the reader's own data, that actions come only from the
    /// components' own buttons, and the pages they may link to.
    pub fn prompt(&self) -> String {
        if self.entries.is_empty() {
            return String::new();
        }
        let mut text = String::from(
            "You can show components on the page. Call a `show_` tool with its props: the page \
             draws the component from the user's own data, which you don't see, so don't repeat \
             its numbers in words. Prefer a component to a table or a list in text. Never write \
             text that looks like a button, a form or an approval: the only actions are the \
             components' own buttons and the approvals the page asks for.",
        );
        if !self.links.is_empty() {
            let pages: Vec<&str> = self.links.iter().map(LinkPattern::as_str).collect();
            text.push_str("\nPages a component may link to, each {key} a record's key: ");
            text.push_str(&pages.join(", "));
            text.push('.');
        }
        text
    }

    /// About how many tokens the components cost in every request: their tools and the
    /// prompt, at four characters a token.
    pub fn prompt_tokens(&self) -> usize {
        let tools: usize = self
            .all_tools()
            .iter()
            .map(|tool| {
                tool.name.len() + tool.description.len() + tool.parameters.to_string().len()
            })
            .sum();
        (tools + self.prompt().len()).div_ceil(4)
    }

    /// Checks a model's call of `tool` with `arguments`: the bounds, then the props.
    /// What passes is the component and its props as stored; what doesn't says why.
    pub fn check(&self, tool: &str, arguments: &Value) -> Result<Checked, Refusal> {
        let Some(entry) = self.entries.get(tool) else {
            return Err(Refusal::one(format!(
                "there is no component `{}`; the components are: {}",
                tool.chars().take(60).collect::<String>(),
                self.entries.keys().cloned().collect::<Vec<_>>().join(", ")
            )));
        };
        let arguments = if arguments.is_null() {
            &Value::Object(serde_json::Map::new())
        } else {
            arguments
        };
        fits(arguments, self.limits.max_depth, self.limits.max_values).map_err(Refusal::one)?;
        let bytes = serde_json::to_string(arguments).map_or(usize::MAX, |text| text.len());
        if bytes > self.limits.max_bytes {
            return Err(Refusal::one(format!(
                "the arguments must be at most {} bytes",
                self.limits.max_bytes
            )));
        }
        let mut checker = Checker::new(&self.links);
        match entry.check(arguments, &mut checker) {
            Some(props) if checker.is_clean() => Ok(Checked {
                surface: entry.name(),
                props,
            }),
            _ => Err(Refusal {
                problems: checker.into_problems(),
            }),
        }
    }

    /// Checks props stored for component `surface` again, as a page is drawn: a
    /// catalogue that changed since may refuse what it once took.
    pub fn recheck(&self, surface: &str, props: &Value) -> Result<Checked, Refusal> {
        self.check(&format!("{TOOL_PREFIX}{surface}"), props)
    }

    /// Whether `cx`'s reader may still be shown component `surface`.
    pub fn allows(&self, surface: &str, cx: &Cx) -> bool {
        self.entries
            .get(&format!("{TOOL_PREFIX}{surface}"))
            .is_some_and(|entry| {
                entry
                    .can()
                    .is_none_or(|(action, subject)| cx.can(action, subject))
            })
    }

    /// Draws component `surface` with stored `props` for `cx`'s reader: its title and
    /// what it drew. Props that no longer pass are a 422; a reader who may no longer be
    /// shown it is a 403, and nothing is loaded.
    pub async fn render(
        &self,
        surface: &str,
        props: &Value,
        cx: &Cx,
    ) -> Result<(String, SurfaceView), AppError> {
        let Some(entry) = self.entries.get(&format!("{TOOL_PREFIX}{surface}")) else {
            return Err(AppError::not_found().with_detail("no such component"));
        };
        if !self.allows(surface, cx) {
            return Err(AppError::forbidden());
        }
        let (title, mut view) = entry.render(props, &self.links, cx).await?;
        let declared = entry.actions();
        view.actions.retain(|button| {
            declared.contains(&button.action) && self.actions.contains_key(button.action)
        });
        Ok((title, view))
    }

    /// Whether component `surface` declares action `action` and the catalogue has it.
    pub fn takes(&self, surface: &str, action: &str) -> bool {
        self.actions.contains_key(action)
            && self
                .entries
                .get(&format!("{TOOL_PREFIX}{surface}"))
                .is_some_and(|entry| entry.actions().contains(&action))
    }

    /// Whether action `action` asks first.
    pub fn is_destructive(&self, action: &str) -> bool {
        self.actions
            .get(action)
            .is_some_and(|entry| entry.destructive())
    }

    /// Checks an action's input without running it.
    pub fn check_input(&self, action: &str, input: &Value) -> Result<(), ActionRefusal> {
        let Some(entry) = self.actions.get(action) else {
            return Err(ActionRefusal::Unknown);
        };
        fits(input, self.limits.max_depth, self.limits.max_values)
            .map_err(|message| ActionRefusal::Invalid(Refusal::one(message)))?;
        let mut checker = Checker::new(&self.links);
        if entry.check(input, &mut checker) && checker.is_clean() {
            Ok(())
        } else {
            Err(ActionRefusal::Invalid(Refusal {
                problems: checker.into_problems(),
            }))
        }
    }

    /// Runs action `action` of a button on component `surface` for `cx`'s reader: the
    /// component must declare it, the reader may take it now, and its input passes the
    /// check again.
    pub async fn run_action(
        &self,
        surface: &str,
        action: &str,
        input: &Value,
        cx: &Cx,
    ) -> Result<ActionDone, ActionRefusal> {
        if !self.takes(surface, action) {
            return Err(ActionRefusal::Unknown);
        }
        let Some(entry) = self.actions.get(action) else {
            return Err(ActionRefusal::Unknown);
        };
        if let Some((verb, subject)) = entry.can()
            && !cx.can(verb, subject)
        {
            return Err(ActionRefusal::Forbidden);
        }
        self.check_input(action, input)?;
        entry.run(input, &self.links, cx).await
    }

    /// Every component's example arguments, by tool: each passes [`check`](Self::check)
    /// with the home page allowed as a link.
    pub fn examples(&self) -> Vec<(String, Value)> {
        self.entries
            .iter()
            .map(|(tool, entry)| (tool.clone(), entry.example()))
            .collect()
    }

    /// Checks `arguments` for `tool` as [`check`](Self::check) does, with the home
    /// page also allowed as a link: for the examples, whose links are `/`.
    pub fn check_example(&self, tool: &str, arguments: &Value) -> Result<Checked, Refusal> {
        let mut with_root = self.clone();
        with_root.links.push(LinkPattern::root());
        with_root.check(tool, arguments)
    }
}

/// A component's tool description: what it shows, and that the page draws it.
fn tool_description(description: &str) -> String {
    let description = description.trim();
    if description.is_empty() {
        "Shows a component on the page, drawn from the user's own data.".to_owned()
    } else {
        format!("{description} Shown on the page, drawn from the user's own data.")
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::genui::props::{Int, Key, UserText};

    struct Reader {
        manager: bool,
    }

    impl SurfaceCx for Reader {
        fn can(&self, action: &str, _subject: &str) -> bool {
            action == "read" || self.manager
        }
    }

    crate::props! {
        /// Recent orders.
        pub struct Recent {
            /// How many.
            pub count: Int<1, 20>,
            /// A heading.
            pub heading: Option<UserText<40>>,
        }
    }

    crate::props! {
        /// An order to refund.
        pub struct OrderKey {
            /// The order.
            pub order: Key,
        }
    }

    struct RecentOrders;

    impl Surface<Reader> for RecentOrders {
        type Props = Recent;
        const NAME: &'static str = "recent_orders";
        const CAN: Option<(&'static str, &'static str)> = Some(("read", "Order"));
        const ACTIONS: &'static [&'static str] = &["refund"];

        fn title(&self, _props: &Recent) -> String {
            "Recent orders".to_owned()
        }

        fn render<'a>(
            &'a self,
            props: &'a Recent,
            _cx: &'a Reader,
        ) -> BoxFuture<'a, Result<SurfaceView, AppError>> {
            Box::pin(async move {
                let key = Key::new("1042").unwrap();
                Ok(SurfaceView::new(props.count.get())
                    .action(ActionButton::new::<Refund>(
                        "Refund",
                        &OrderKey { order: key.clone() },
                    ))
                    .action(ActionButton::new::<Delete>(
                        "Delete",
                        &OrderKey { order: key },
                    )))
            })
        }
    }

    struct Revenue;

    impl Surface<Reader> for Revenue {
        type Props = Recent;
        const NAME: &'static str = "revenue";
        const CAN: Option<(&'static str, &'static str)> = Some(("manage", "Report"));

        fn title(&self, _props: &Recent) -> String {
            "Revenue".to_owned()
        }

        fn render<'a>(
            &'a self,
            _props: &'a Recent,
            _cx: &'a Reader,
        ) -> BoxFuture<'a, Result<SurfaceView, AppError>> {
            Box::pin(async { Ok(SurfaceView::new("₹1,00,000")) })
        }
    }

    struct Refund;

    impl Action for Refund {
        type Input = OrderKey;
        const NAME: &'static str = "refund";
        const CAN: Option<(&'static str, &'static str)> = Some(("refund", "Order"));
        const DESTRUCTIVE: bool = true;
    }

    impl RunAction<Reader> for Refund {
        fn run<'a>(
            &'a self,
            input: OrderKey,
            _cx: &'a Reader,
        ) -> BoxFuture<'a, Result<ActionDone, AppError>> {
            Box::pin(async move { Ok(ActionDone::new(format!("Refunded {}", input.order))) })
        }
    }

    struct Delete;

    impl Action for Delete {
        type Input = OrderKey;
        const NAME: &'static str = "delete";
    }

    impl RunAction<Reader> for Delete {
        fn run<'a>(
            &'a self,
            _input: OrderKey,
            _cx: &'a Reader,
        ) -> BoxFuture<'a, Result<ActionDone, AppError>> {
            Box::pin(async { Ok(ActionDone::new("Deleted")) })
        }
    }

    fn catalog() -> Catalog<Reader> {
        Catalog::new()
            .surface(RecentOrders)
            .surface(Revenue)
            .action(Refund)
            .action(Delete)
    }

    #[test]
    fn a_reader_is_offered_only_what_they_may_see() {
        let catalog = catalog();
        let staff: Vec<String> = catalog
            .tools(&Reader { manager: false })
            .into_iter()
            .map(|t| t.name)
            .collect();
        assert_eq!(staff, ["show_recent_orders"]);
        assert_eq!(catalog.tools(&Reader { manager: true }).len(), 2);
    }

    #[tokio::test]
    async fn only_declared_buttons_are_drawn_and_run() {
        let catalog = catalog();
        let reader = Reader { manager: true };
        let (_, view) = catalog
            .render("recent_orders", &json!({ "count": 3 }), &reader)
            .await
            .unwrap();
        let names: Vec<&str> = view.actions.iter().map(|b| b.action).collect();
        assert_eq!(names, ["refund"], "delete is not declared by the component");
        assert!(matches!(
            catalog
                .run_action("recent_orders", "delete", &json!({ "order": "1" }), &reader)
                .await,
            Err(ActionRefusal::Unknown)
        ));
        let staff = Reader { manager: false };
        assert!(matches!(
            catalog
                .run_action("recent_orders", "refund", &json!({ "order": "1" }), &staff)
                .await,
            Err(ActionRefusal::Forbidden)
        ));
        assert!(matches!(
            catalog
                .run_action(
                    "recent_orders",
                    "refund",
                    &json!({ "order": "1 OR 1=1" }),
                    &reader
                )
                .await,
            Err(ActionRefusal::Invalid(_))
        ));
        let done = catalog
            .run_action(
                "recent_orders",
                "refund",
                &json!({ "order": "1042" }),
                &reader,
            )
            .await
            .unwrap();
        assert_eq!(done.message, "Refunded 1042");
    }

    #[tokio::test]
    async fn a_reader_who_lost_access_is_refused_before_anything_loads() {
        let catalog = catalog();
        let error = catalog
            .render(
                "revenue",
                &json!({ "count": 1 }),
                &Reader { manager: false },
            )
            .await
            .unwrap_err();
        assert_eq!(error.status.as_u16(), 403);
    }

    #[test]
    fn bounds_are_checked_before_the_props() {
        let catalog = catalog().limits(Limits {
            max_bytes: 64,
            ..Limits::default()
        });
        let long = json!({ "count": 1, "heading": "x".repeat(100) });
        let refusal = catalog.check("show_recent_orders", &long).unwrap_err();
        assert!(refusal.message().contains("bytes"), "{refusal}");
        let mut deep = json!(1);
        for _ in 0..10 {
            deep = json!({ "a": deep });
        }
        let refusal = catalog.check("show_recent_orders", &deep).unwrap_err();
        assert!(refusal.message().contains("levels"), "{refusal}");
    }

    #[test]
    fn examples_pass_their_own_check() {
        let catalog = catalog();
        for (tool, example) in catalog.examples() {
            catalog
                .check_example(&tool, &example)
                .unwrap_or_else(|refusal| panic!("{tool}: {refusal}"));
        }
    }

    #[test]
    #[should_panic(expected = "a component's name")]
    fn a_bad_name_fails_at_boot() {
        struct Bad;
        impl Surface<Reader> for Bad {
            type Props = Recent;
            const NAME: &'static str = "Show Approval";
            fn title(&self, _props: &Recent) -> String {
                String::new()
            }
            fn render<'a>(
                &'a self,
                _props: &'a Recent,
                _cx: &'a Reader,
            ) -> BoxFuture<'a, Result<SurfaceView, AppError>> {
                Box::pin(async { Ok(SurfaceView::new("")) })
            }
        }
        let _ = Catalog::new().surface(Bad);
    }
}
