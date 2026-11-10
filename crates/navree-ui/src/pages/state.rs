//! [`State`]: the block that says why a place has no content, and the components
//! built on it: [`ErrorPage`], [`Forbidden`], [`NotFound`], [`SignInRequired`],
//! [`SessionEnded`] and [`EmptyState`].
//!
//! One markup serves them all (`data-slot="state"`), so a 404 for a page, a locked
//! row in a table and a card that needs a sign-in look like one family. Every word
//! comes from [`StateLabels`](super::StateLabels).

use std::borrow::Cow;

use http::StatusCode;
use navree_http::response::{IntoResponse, Response};

use super::labels::{StateKind, StateLabels};
use super::return_to::ReturnTo;
use crate::cmd::{Cmd, Trigger};
use crate::components::{Alert, AlertTone, Button, ButtonVariant, Size, icon};
use crate::html::Href;
use crate::icons::{self, Icon};
use crate::node::{IntoNodes, Node, el, into_node};
use crate::shell::{AppPage, AppShell};
use crate::view::View;

/// How much room a [`State`] takes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum StateSize {
    /// The page itself: centred, with room around it.
    Page,
    /// A card, a tab or a region: centred, in a dashed outline. The default.
    #[default]
    Panel,
    /// A table row or a list line: one line of icon and words, left to right.
    Compact,
}

impl StateSize {
    const fn name(self) -> &'static str {
        match self {
            Self::Page => "page",
            Self::Panel => "panel",
            Self::Compact => "compact",
        }
    }
}

/// A state: an icon, an optional status code, a headline, a plain explanation and what
/// to do next. It renders to a `div` with `data-slot="state"`.
///
/// ```
/// use navree_ui::pages::{State, StateKind};
/// use navree_ui::render;
///
/// let html = render(&State::of(StateKind::NotFound).into());
/// assert!(html.contains(r#"data-kind="not-found""#));
/// assert!(html.contains("We can&#39;t find that page"));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct State {
    name: &'static str,
    glyph: Icon,
    status: Option<StatusCode>,
    title: Cow<'static, str>,
    description: Option<Cow<'static, str>>,
    detail: Option<String>,
    wait: Option<String>,
    actions: Vec<Node>,
    reference: Option<String>,
    copy: Option<Cow<'static, str>>,
    reference_label: Cow<'static, str>,
    debug: Option<Node>,
    size: StateSize,
    heading: Option<u8>,
    id: Option<String>,
    kind: Option<StateKind>,
    custom_title: bool,
    custom_description: bool,
}

impl State {
    /// The state of `kind`, with the default words.
    pub fn of(kind: StateKind) -> Self {
        let labels = StateLabels::default();
        let words = labels.words(kind);
        Self {
            name: kind.name(),
            glyph: kind.icon(),
            status: None,
            title: words.title.clone(),
            description: Some(words.description.clone()),
            detail: None,
            wait: None,
            actions: Vec::new(),
            reference: None,
            copy: None,
            reference_label: labels.reference.clone(),
            debug: None,
            size: StateSize::Panel,
            heading: None,
            id: None,
            kind: Some(kind),
            custom_title: false,
            custom_description: false,
        }
    }

    /// A state of your own: a `name` for `data-kind` (`empty`, `locked`), an icon and a
    /// headline.
    pub fn custom(name: &'static str, glyph: Icon, title: impl Into<Cow<'static, str>>) -> Self {
        let mut state = Self::of(StateKind::Client);
        state.name = name;
        state.glyph = glyph;
        state.title = title.into();
        state.description = None;
        state.kind = None;
        state.custom_title = true;
        state.custom_description = true;
        state
    }

    /// Takes the words from `labels`: the headline and the explanation of its kind, unless
    /// you have set your own.
    pub fn labels(mut self, labels: &StateLabels) -> Self {
        if let Some(kind) = self.kind {
            let words = labels.words(kind);
            if !self.custom_title {
                self.title = words.title.clone();
            }
            if !self.custom_description {
                self.description = Some(words.description.clone());
            }
        }
        self.reference_label = labels.reference.clone();
        self
    }

    /// Shows the HTTP status as a code beside the headline.
    pub fn status(mut self, status: StatusCode) -> Self {
        self.status = Some(status);
        self
    }

    /// Sets the headline.
    pub fn title(mut self, title: impl Into<Cow<'static, str>>) -> Self {
        self.title = title.into();
        self.custom_title = true;
        self
    }

    /// Sets the explanation. It is plain text, never markup.
    pub fn description(mut self, description: impl Into<Cow<'static, str>>) -> Self {
        self.description = Some(description.into());
        self.custom_description = true;
        self
    }

    /// Takes the explanation away.
    pub fn without_description(mut self) -> Self {
        self.description = None;
        self.custom_description = true;
        self
    }

    /// Adds a line under the explanation: the error's own message, such as "Email taken".
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// Adds a line about when to try again.
    pub fn wait(mut self, text: impl Into<String>) -> Self {
        self.wait = Some(text.into());
        self
    }

    /// Adds actions. The first is the main one.
    pub fn actions(mut self, actions: impl IntoNodes) -> Self {
        actions.push_into(&mut self.actions);
        self
    }

    /// Shows a reference support can look up, such as a request id. With `copy` (the
    /// button's name) a button copies it, which loads the core runtime.
    pub fn reference(
        mut self,
        reference: impl Into<String>,
        copy: Option<Cow<'static, str>>,
    ) -> Self {
        self.reference = Some(reference.into());
        self.copy = copy;
        self
    }

    /// Adds what a debug build knows. Never call this with anything in production.
    pub fn debug(mut self, details: impl Into<Node>) -> Self {
        self.debug = Some(details.into());
        self
    }

    /// Sets how much room it takes.
    pub fn size(mut self, size: StateSize) -> Self {
        self.size = size;
        self
    }

    /// Makes the headline a heading of `level` (1 to 6) in the page's outline. Without
    /// this it is a paragraph, so a state inside a page doesn't disturb the outline.
    pub fn heading(mut self, level: u8) -> Self {
        self.heading = Some(level.clamp(1, 6));
        self
    }

    /// Sets the `id`, which also names the headline for assistive technology.
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// The headline.
    pub fn headline(&self) -> &str {
        &self.title
    }

    fn into_node(self) -> Node {
        let title_id = self.id.as_ref().map(|id| format!("{id}-title"));
        let tag = match self.heading {
            Some(1) => "h1",
            Some(2) => "h2",
            Some(3) => "h3",
            Some(4) => "h4",
            Some(5) => "h5",
            Some(6) => "h6",
            _ => "p",
        };
        let code = self.status.map(|status| {
            el("span")
                .slot("state-code")
                .attr("dir", "ltr")
                .child(Node::Int(i64::from(status.as_u16())))
        });
        let title = el(tag)
            .slot("state-title")
            .attr_opt("id", title_id.clone())
            .child(self.title);
        let description = self
            .description
            .map(|text| el("p").slot("state-description").child(text));
        let detail = self
            .detail
            .map(|text| el("p").slot("state-detail").child(text));
        let wait = self.wait.map(|text| el("p").slot("state-wait").child(text));
        let actions =
            (!self.actions.is_empty()).then(|| el("div").slot("state-actions").child(self.actions));
        let reference = self.reference.map(|reference| {
            let copy = self.copy.map(|name| {
                Button::icon_only(icons::COPY, name.into_owned())
                    .variant(ButtonVariant::Ghost)
                    .size(Size::Xs)
                    .on(Trigger::Click, Cmd::copy(reference.clone()))
            });
            el("p").slot("state-reference").children((
                el("span").child(self.reference_label),
                " ",
                el("code").slot("code").attr("dir", "ltr").child(reference),
                copy,
            ))
        });
        let debug = self.debug.map(|details| {
            el("details")
                .slot("state-debug")
                .child(el("summary").child("Development only"))
                .child(details)
        });
        let body = el("div").slot("state-body").children((
            code,
            title,
            description,
            detail,
            wait,
            actions,
            reference,
            debug,
        ));
        // A group needs a name, and only a state with an id has one.
        let named = self.id.is_some();
        let mut root = el("div")
            .slot("state")
            .attr("data-kind", self.name)
            .attr("data-size", self.size.name())
            .attr_opt("id", self.id)
            .attr_opt("aria-labelledby", title_id)
            .attr_opt("role", named.then_some("group"));
        if let Some(status) = self.status {
            root = root.attr("data-status", i64::from(status.as_u16()));
        }
        let media = el("div").slot("state-media").child(icon(self.glyph));
        root.children((media, body)).into()
    }
}

into_node!(State);

/// A row of a table that says `state`, in one cell that spans `columns`.
fn row(state: State, columns: usize) -> Node {
    el("tr")
        .slot("state-row")
        .attr("data-kind", state.name)
        .child(
            el("td")
                .attr("colspan", i64::try_from(columns.max(1)).unwrap_or(i64::MAX))
                .child(state.size(StateSize::Compact)),
        )
        .into()
}

/// A whole page for an error status: the headline, the explanation, what to do next
/// and a reference for support.
///
/// [`UiModule`](crate::UiModule) builds one for every page route that ends in an error. Build
/// one yourself to answer from a handler:
///
/// ```
/// use navree_ui::pages::ErrorPage;
/// use http::StatusCode;
///
/// let page = ErrorPage::new(StatusCode::NOT_FOUND).home("/deals");
/// let html = page.into_view().to_html();
/// assert!(html.contains("<title>Page not found</title>"));
/// assert!(!html.contains("<script"));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct ErrorPage {
    status: StatusCode,
    kind: StateKind,
    labels: StateLabels,
    title: Option<Cow<'static, str>>,
    description: Option<Cow<'static, str>>,
    detail: Option<String>,
    reference: Option<String>,
    wait: Option<u64>,
    home: Href,
    sign_in: Href,
    retry: Option<Href>,
    no_retry: bool,
    actions: Option<Vec<Node>>,
    debug: Option<Node>,
    mark_build: bool,
}

impl ErrorPage {
    /// The page for `status`.
    pub fn new(status: StatusCode) -> Self {
        Self {
            status,
            kind: StateKind::of(status, ""),
            labels: StateLabels::default(),
            title: None,
            description: None,
            detail: None,
            reference: None,
            wait: None,
            home: Href::from("/"),
            sign_in: Href::from("/sign-in"),
            retry: None,
            no_retry: false,
            actions: None,
            debug: None,
            mark_build: true,
        }
    }

    /// The page for an error of `kind`, with the status the kind usually has.
    pub fn of(kind: StateKind) -> Self {
        let mut page = Self::new(kind.status());
        page.kind = kind;
        page
    }

    /// The page for a request that matched nothing.
    pub fn not_found() -> Self {
        Self::of(StateKind::NotFound)
    }

    /// The page for a user who may not see this.
    pub fn forbidden() -> Self {
        Self::of(StateKind::Forbidden)
    }

    /// The page for a guest.
    pub fn sign_in_required() -> Self {
        Self::of(StateKind::SignIn)
    }

    /// Sets the kind, which decides the words and the icon, when the status is not
    /// enough (a CSRF failure is a 403 that says the page expired).
    pub fn kind(mut self, kind: StateKind) -> Self {
        self.kind = kind;
        self
    }

    /// Sets the words.
    pub fn labels(mut self, labels: StateLabels) -> Self {
        self.labels = labels;
        self
    }

    /// Replaces the headline.
    pub fn title(mut self, title: impl Into<Cow<'static, str>>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Replaces the explanation.
    pub fn description(mut self, description: impl Into<Cow<'static, str>>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Adds the error's own message under the explanation.
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// Shows a reference for support: the request id.
    pub fn reference(mut self, reference: impl Into<String>) -> Self {
        self.reference = Some(reference.into());
        self
    }

    /// Says how many seconds to wait (`Retry-After`).
    pub fn retry_after(mut self, seconds: u64) -> Self {
        self.wait = Some(seconds);
        self
    }

    /// Where "Go to home" leads.
    pub fn home(mut self, home: impl Into<Href>) -> Self {
        self.home = home.into();
        self
    }

    /// Where "Sign in" leads. Give it the address with `next` already on it
    /// ([`ReturnTo::sign_in_href`]).
    pub fn sign_in(mut self, sign_in: impl Into<Href>) -> Self {
        self.sign_in = sign_in.into();
        self
    }

    /// Where "Try again" and "Reload" lead: the page that failed.
    pub fn retry_to(mut self, retry: impl Into<Href>) -> Self {
        self.retry = Some(retry.into());
        self
    }

    /// Offers no "Try again" or "Reload": for a page that answers a request a link cannot
    /// repeat, such as a form posted without script. A link would be a `GET` of an address
    /// that only takes a `POST`. The page offers Home, and the way to sign in.
    pub fn without_retry(mut self) -> Self {
        self.no_retry = true;
        self
    }

    /// Replaces the actions the kind offers.
    pub fn actions(mut self, actions: impl IntoNodes) -> Self {
        let mut list = Vec::new();
        actions.push_into(&mut list);
        self.actions = Some(list);
        self
    }

    /// Adds what a debug build knows. Never call this with anything in production.
    pub fn debug(mut self, details: impl Into<Node>) -> Self {
        self.debug = Some(details.into());
        self
    }

    /// The status the page answers with.
    pub fn status_code(&self) -> StatusCode {
        self.status
    }

    /// The page's short title: "Page not found". It is the document's title and the
    /// heading in the shell.
    pub fn short_title(&self) -> String {
        match &self.title {
            Some(title) => title.to_string(),
            None => self.labels.words(self.kind).short.to_string(),
        }
    }

    fn default_actions(&self) -> Vec<Node> {
        let labels = &self.labels;
        let home = |variant: ButtonVariant| {
            Button::new(labels.home.to_string())
                .variant(variant)
                .href(self.home.clone())
                .icon(icons::HOUSE)
        };
        let again = |label: &str| -> Option<Node> {
            if self.no_retry {
                return None;
            }
            let button = Button::primary(label.to_owned()).icon(icons::REFRESH_CW);
            Some(
                match &self.retry {
                    Some(href) => button.href(href.clone()),
                    None => button.href(""),
                }
                .into(),
            )
        };
        // Try again, then Home beside it; Home alone is the main action when there is no retry.
        let retry_then_home = |label: &str, home_too: bool| -> Vec<Node> {
            match (again(label), home_too) {
                (Some(retry), true) => vec![retry, home(ButtonVariant::Outline).into()],
                (Some(retry), false) => vec![retry],
                (None, _) => vec![home(ButtonVariant::Primary).into()],
            }
        };
        match self.kind {
            StateKind::SignIn => vec![
                Button::primary(labels.sign_in.to_string())
                    .icon(icons::LOG_IN)
                    .href(self.sign_in.clone())
                    .into(),
            ],
            StateKind::Conflict | StateKind::Expired => retry_then_home(&labels.reload, true),
            StateKind::RateLimited | StateKind::Unavailable | StateKind::Offline => {
                retry_then_home(&labels.try_again, false)
            }
            StateKind::Failed => retry_then_home(&labels.try_again, true),
            _ => vec![home(ButtonVariant::Primary).into()],
        }
    }

    /// The state this page shows. `heading` is the level of its headline, if it is one.
    ///
    /// It has no `id`, so several can stand in one page; give it one with
    /// [`State::id`] when it is the page's only state.
    pub fn state(&self, heading: Option<u8>, copy: bool) -> State {
        let mut state = State::of(self.kind)
            .labels(&self.labels)
            .size(StateSize::Page);
        // No answer came, so there is no status to show.
        if self.kind != StateKind::Offline {
            state = state.status(self.status);
        }
        if let Some(level) = heading {
            state = state.heading(level);
        }
        if let Some(title) = &self.title {
            state = state.title(title.clone());
        }
        if let Some(description) = &self.description {
            state = state.description(description.clone());
        }
        if let Some(detail) = &self.detail {
            state = state.detail(detail.clone());
        }
        if let Some(seconds) = self.wait {
            state = state.wait(self.labels.wait_text(seconds));
        }
        if let Some(reference) = &self.reference {
            state = state.reference(
                reference.clone(),
                copy.then(|| Cow::Owned(self.labels.copy.to_string())),
            );
        }
        let actions = match &self.actions {
            Some(actions) => actions.clone(),
            None => self.default_actions(),
        };
        state = state.actions(actions);
        if let Some(debug) = &self.debug {
            state = state.debug(debug.clone());
        }
        state
    }

    /// The page on its own, with no shell: a document with one `h1`, the same stylesheet
    /// and no script. For a guest, and for a page whose shell can't be built.
    pub fn into_view(self) -> View {
        let title = self.short_title();
        let status = self.status;
        let state = self.state(Some(1), false).id("error");
        let view = View::new(title)
            .status(status)
            .body(el("main").slot("state-page").id("main").child(state));
        // It loads no script, but the runtime of the page it replaces reads its build: with it
        // a boosted link shows this page in place, and without it the runtime would fetch the
        // address again as a page load.
        if self.mark_build {
            view.mark_build()
        } else {
            view
        }
    }

    /// The page inside the app's shell, as `shell.page` would draw it: the shell's
    /// sidebar and top bar, the short title as the page's heading and the state below.
    pub fn in_shell(self, shell: &AppShell) -> AppPage {
        let title = self.short_title();
        let status = self.status;
        let state = self.state(None, true).id("error");
        shell.page(title).status(status).body(state)
    }

    /// The page as a fragment for a region: just the state, with no heading and no
    /// script, for the runtime to put where the region was. Its main action fetches the
    /// region again, where a page would reload.
    pub fn into_fragment(mut self) -> View {
        let title = self.short_title();
        let status = self.status;
        if self.actions.is_none() {
            let labels = &self.labels;
            let again = || {
                Button::outline(labels.try_again.to_string())
                    .size(Size::Sm)
                    .icon(icons::REFRESH_CW)
                    .attr("data-ui-refresh", "")
            };
            let actions: Vec<Node> = match self.kind {
                StateKind::SignIn => vec![
                    Button::primary(labels.sign_in.to_string())
                        .size(Size::Sm)
                        .icon(icons::LOG_IN)
                        .href(self.sign_in.clone())
                        .into(),
                ],
                StateKind::Forbidden
                | StateKind::NotFound
                | StateKind::Gone
                | StateKind::MethodNotAllowed => Vec::new(),
                _ => vec![again().into()],
            };
            self.actions = Some(actions);
        }
        let state = self.state(None, false).size(StateSize::Panel);
        View::new(title).status(status).body(state)
    }
}

impl IntoResponse for ErrorPage {
    fn into_response(self) -> Response {
        self.into_view().into_response()
    }
}

/// "You don't have access to this": the state for a place the user may not see, for a
/// card, a tab, a region or a table row. It never says whether the thing exists.
///
/// ```
/// use navree_ui::pages::Forbidden;
/// use navree_ui::render;
///
/// let html = render(&Forbidden::new().what("deal").into());
/// assert!(html.contains("You don&#39;t have access to this deal."));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Forbidden {
    state: State,
    labels: StateLabels,
    what: Option<String>,
}

impl Forbidden {
    /// The state.
    pub fn new() -> Self {
        Self {
            state: State::of(StateKind::Forbidden),
            labels: StateLabels::default(),
            what: None,
        }
    }

    /// Names the thing: "You don't have access to this deal."
    pub fn what(mut self, what: impl Into<String>) -> Self {
        self.what = Some(what.into());
        self
    }

    /// Adds an action that asks for access. The app owns the address it leads to.
    pub fn request_access(mut self, href: impl Into<Href>) -> Self {
        let button = Button::outline(self.labels.request_access.to_string())
            .size(Size::Sm)
            .href(href);
        self.state = self.state.actions(button);
        self
    }

    /// Adds an action.
    pub fn action(mut self, action: impl Into<Node>) -> Self {
        self.state = self.state.actions(action.into());
        self
    }

    /// Takes the words from `labels`.
    pub fn labels(mut self, labels: StateLabels) -> Self {
        self.state = self.state.labels(&labels);
        self.labels = labels;
        self
    }

    /// Replaces the explanation.
    pub fn description(mut self, description: impl Into<Cow<'static, str>>) -> Self {
        self.state = self.state.description(description);
        self
    }

    /// Sets how much room it takes.
    pub fn size(mut self, size: StateSize) -> Self {
        self.state = self.state.size(size);
        self
    }

    /// One line, for a list.
    pub fn compact(self) -> Self {
        self.size(StateSize::Compact)
    }

    /// Makes the headline a heading.
    pub fn heading(mut self, level: u8) -> Self {
        self.state = self.state.heading(level);
        self
    }

    /// Sets the `id`.
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.state = self.state.id(id);
        self
    }

    fn finish(self) -> State {
        match self.what {
            Some(what) => self
                .state
                .title(StateLabels::about(&self.labels.forbidden_what, &what))
                .without_description(),
            None => self.state,
        }
    }

    /// A table row that says the user may not see this row, in one cell across `columns`.
    /// The table keeps its header, its order and its count; the row has no checkbox
    /// and no link.
    pub fn row(self, columns: usize) -> Node {
        let locked = self.labels.row_locked.to_string();
        let titled = self.what.is_none();
        let state = self.finish();
        let state = if titled {
            state.title(locked).without_description()
        } else {
            state
        };
        row(state, columns)
    }
}

impl Default for Forbidden {
    fn default() -> Self {
        Self::new()
    }
}

impl From<Forbidden> for Node {
    fn from(value: Forbidden) -> Self {
        value.finish().into()
    }
}

/// "We can't find that": the state for a record, a card or a region that isn't there.
///
/// ```
/// use navree_ui::pages::NotFound;
/// use navree_ui::render;
///
/// let html = render(&NotFound::new().what("deal").into());
/// assert!(html.contains("This deal doesn&#39;t exist, or it was removed."));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct NotFound {
    state: State,
    labels: StateLabels,
    what: Option<String>,
}

impl NotFound {
    /// The state.
    pub fn new() -> Self {
        Self {
            state: State::of(StateKind::NotFound),
            labels: StateLabels::default(),
            what: None,
        }
    }

    /// Names the thing: "This deal doesn't exist, or it was removed."
    pub fn what(mut self, what: impl Into<String>) -> Self {
        self.what = Some(what.into());
        self
    }

    /// Adds an action.
    pub fn action(mut self, action: impl Into<Node>) -> Self {
        self.state = self.state.actions(action.into());
        self
    }

    /// Takes the words from `labels`.
    pub fn labels(mut self, labels: StateLabels) -> Self {
        self.state = self.state.labels(&labels);
        self.labels = labels;
        self
    }

    /// Replaces the explanation.
    pub fn description(mut self, description: impl Into<Cow<'static, str>>) -> Self {
        self.state = self.state.description(description);
        self
    }

    /// Sets how much room it takes.
    pub fn size(mut self, size: StateSize) -> Self {
        self.state = self.state.size(size);
        self
    }

    /// One line, for a list.
    pub fn compact(self) -> Self {
        self.size(StateSize::Compact)
    }

    /// Makes the headline a heading.
    pub fn heading(mut self, level: u8) -> Self {
        self.state = self.state.heading(level);
        self
    }

    /// Sets the `id`.
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.state = self.state.id(id);
        self
    }

    fn finish(self) -> State {
        match self.what {
            Some(what) => self
                .state
                .title(StateLabels::about(&self.labels.not_found_what, &what))
                .without_description(),
            None => self.state,
        }
    }

    /// A table row that says this row is gone, in one cell across `columns`.
    pub fn row(self, columns: usize) -> Node {
        let missing = self.labels.row_missing.to_string();
        let titled = self.what.is_none();
        let state = self.finish();
        let state = if titled {
            state.title(missing).without_description()
        } else {
            state
        };
        row(state, columns)
    }
}

impl Default for NotFound {
    fn default() -> Self {
        Self::new()
    }
}

impl From<NotFound> for Node {
    fn from(value: NotFound) -> Self {
        value.finish().into()
    }
}

/// The state for a card, tab or region that needs a signed-in user: a button to the
/// sign-in page that brings the user back to where they were.
///
/// ```
/// use navree_ui::pages::{ReturnTo, SignInRequired};
/// use navree_ui::render;
///
/// let here = ReturnTo::parse("/deals/42").unwrap();
/// let html = render(&SignInRequired::new("/sign-in").return_to(&here).into());
/// assert!(html.contains(r#"href="/sign-in?next=%2Fdeals%2F42""#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct SignInRequired {
    state: State,
    labels: StateLabels,
    href: Href,
    what: Option<String>,
}

impl SignInRequired {
    /// The state, with a button to `sign_in`.
    pub fn new(sign_in: impl Into<Href>) -> Self {
        Self {
            state: State::of(StateKind::SignIn),
            labels: StateLabels::default(),
            href: sign_in.into(),
            what: None,
        }
    }

    /// Brings the user back to `here` after they sign in. An address that is not safe to
    /// return to is left out, and the user lands where the sign-in page sends everyone.
    pub fn return_to(mut self, here: &ReturnTo) -> Self {
        self.href = here.sign_in_href(self.href.as_str());
        self
    }

    /// Names what needs a user: "Sign in to see your deals."
    pub fn what(mut self, what: impl Into<String>) -> Self {
        self.what = Some(what.into());
        self
    }

    /// Takes the words from `labels`.
    pub fn labels(mut self, labels: StateLabels) -> Self {
        self.state = self.state.labels(&labels);
        self.labels = labels;
        self
    }

    /// Sets how much room it takes.
    pub fn size(mut self, size: StateSize) -> Self {
        self.state = self.state.size(size);
        self
    }

    /// One line, for a list.
    pub fn compact(self) -> Self {
        self.size(StateSize::Compact)
    }

    /// Makes the headline a heading.
    pub fn heading(mut self, level: u8) -> Self {
        self.state = self.state.heading(level);
        self
    }

    /// Sets the `id`.
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.state = self.state.id(id);
        self
    }

    fn finish(self) -> State {
        let button = Button::primary(self.labels.sign_in.to_string())
            .size(Size::Sm)
            .icon(icons::LOG_IN)
            .href(self.href);
        let state = self.state.actions(button);
        match self.what {
            Some(what) => state
                .title(StateLabels::about(&self.labels.sign_in_what, &what))
                .without_description(),
            None => state,
        }
    }
}

impl From<SignInRequired> for Node {
    fn from(value: SignInRequired) -> Self {
        value.finish().into()
    }
}

/// The notice on the sign-in page for a user who was sent there: "You've been signed
/// out". It takes no picture and little room, so the form stays on the screen.
///
/// ```
/// use navree_ui::pages::SessionEnded;
/// use navree_ui::render;
///
/// let html = render(&SessionEnded::new().into());
/// assert!(html.contains("You&#39;ve been signed out"));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct SessionEnded {
    labels: StateLabels,
}

impl SessionEnded {
    /// The notice.
    pub fn new() -> Self {
        Self {
            labels: StateLabels::default(),
        }
    }

    /// Takes the words from `labels`.
    pub fn labels(mut self, labels: StateLabels) -> Self {
        self.labels = labels;
        self
    }

    fn into_node(self) -> Node {
        Alert::new(AlertTone::Neutral)
            .icon(icons::LOG_IN)
            .title(self.labels.signed_out.to_string())
            .body(self.labels.signed_out_help.to_string())
            .into()
    }
}

impl Default for SessionEnded {
    fn default() -> Self {
        Self::new()
    }
}

into_node!(SessionEnded);

/// What an [`EmptyState`] is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum EmptyKind {
    /// There is nothing yet, and the user can make something.
    NothingYet,
    /// A search or a filter matched nothing.
    NoResults,
    /// There was work, and it is done.
    CaughtUp,
}

impl EmptyKind {
    const fn name(self) -> &'static str {
        match self {
            Self::NothingYet => "nothing-yet",
            Self::NoResults => "no-results",
            Self::CaughtUp => "caught-up",
        }
    }

    const fn icon(self) -> Icon {
        match self {
            Self::NothingYet => icons::INBOX,
            Self::NoResults => icons::SEARCH_X,
            Self::CaughtUp => icons::CIRCLE_CHECK,
        }
    }
}

/// What a list, a table or a region shows when it has nothing in it, with the action
/// each case needs. Built on the same markup as the error states.
///
/// ```
/// use navree_ui::pages::EmptyState;
/// use navree_ui::{Button, render};
///
/// let html = render(&EmptyState::nothing_yet("deals").action(Button::primary("New deal")).into());
/// assert!(html.contains("No deals yet"));
/// assert!(html.contains("New deal"));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct EmptyState {
    kind: EmptyKind,
    labels: StateLabels,
    title: Option<Cow<'static, str>>,
    description: Option<Cow<'static, str>>,
    actions: Vec<Node>,
    clear: Option<Href>,
    size: StateSize,
    heading: Option<u8>,
    what: Option<String>,
    id: Option<String>,
}

impl EmptyState {
    fn new(kind: EmptyKind) -> Self {
        Self {
            kind,
            labels: StateLabels::default(),
            title: None,
            description: None,
            actions: Vec::new(),
            clear: None,
            size: StateSize::Panel,
            heading: None,
            what: None,
            id: None,
        }
    }

    /// There is nothing yet: "No deals yet". Add the action that makes the first one.
    pub fn nothing_yet(what: impl Into<String>) -> Self {
        let mut state = Self::new(EmptyKind::NothingYet);
        state.what = Some(what.into());
        state
    }

    /// A search or filter found nothing. Add [`clear_filters`](Self::clear_filters) to
    /// offer the way back.
    pub fn no_results() -> Self {
        Self::new(EmptyKind::NoResults)
    }

    /// The work is done: "You're all caught up".
    pub fn caught_up() -> Self {
        Self::new(EmptyKind::CaughtUp)
    }

    /// Offers a link that clears the filters (a region link when it is inside a region).
    pub fn clear_filters(mut self, href: impl Into<Href>) -> Self {
        self.clear = Some(href.into());
        self
    }

    /// Replaces the headline.
    pub fn title(mut self, title: impl Into<Cow<'static, str>>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Replaces the explanation.
    pub fn description(mut self, description: impl Into<Cow<'static, str>>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Adds an action. The first is the main one.
    pub fn action(mut self, action: impl Into<Node>) -> Self {
        self.actions.push(action.into());
        self
    }

    /// Takes the words from `labels`.
    pub fn labels(mut self, labels: StateLabels) -> Self {
        self.labels = labels;
        self
    }

    /// Sets how much room it takes.
    pub fn size(mut self, size: StateSize) -> Self {
        self.size = size;
        self
    }

    /// One line, for a list.
    pub fn compact(self) -> Self {
        self.size(StateSize::Compact)
    }

    /// Makes the headline a heading.
    pub fn heading(mut self, level: u8) -> Self {
        self.heading = Some(level);
        self
    }

    /// Sets the `id`.
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    fn state(self) -> State {
        let labels = &self.labels;
        let (title, description): (Cow<'static, str>, Option<Cow<'static, str>>) = match self.kind {
            EmptyKind::NothingYet => (
                match &self.what {
                    Some(what) => labels.nothing_yet_what.replace("{what}", what).into(),
                    None => labels.nothing_yet.clone(),
                },
                None,
            ),
            EmptyKind::NoResults => (
                labels.no_results.clone(),
                Some(labels.no_results_help.clone()),
            ),
            EmptyKind::CaughtUp => (
                labels.caught_up.clone(),
                Some(labels.caught_up_help.clone()),
            ),
        };
        let mut state = State::custom(
            self.kind.name(),
            self.kind.icon(),
            self.title.unwrap_or(title),
        )
        .size(self.size);
        if let Some(text) = self.description.or(description) {
            state = state.description(text);
        }
        if let Some(level) = self.heading {
            state = state.heading(level);
        }
        if let Some(id) = self.id {
            state = state.id(id);
        }
        let clear = self.clear.map(|href| {
            Button::outline(labels.clear_filters.to_string())
                .size(Size::Sm)
                .href(href)
                .attr("data-ui-in-region", "")
        });
        state.actions(self.actions).actions(clear)
    }

    /// A table row that says the table is empty, in one cell across `columns`.
    pub fn row(self, columns: usize) -> Node {
        row(self.state(), columns)
    }
}

impl From<EmptyState> for Node {
    fn from(value: EmptyState) -> Self {
        value.state().into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::render;

    fn html(node: impl Into<Node>) -> String {
        render(&node.into())
    }

    #[test]
    fn a_state_has_a_code_a_headline_a_sentence_and_actions() {
        let out = html(
            State::of(StateKind::Forbidden)
                .status(StatusCode::FORBIDDEN)
                .actions(Button::primary("Home").href("/")),
        );
        assert!(out.starts_with(r#"<div data-slot="state" data-kind="forbidden" data-size="panel" data-status="403">"#), "{out}");
        assert!(
            out.contains(r#"<span data-slot="state-code" dir="ltr">403</span>"#),
            "{out}"
        );
        assert!(
            out.contains(r#"<p data-slot="state-title">You don&#39;t have access to this</p>"#),
            "{out}"
        );
        assert!(out.contains(r#"data-slot="state-description""#), "{out}");
        assert!(
            out.contains(r#"<div data-slot="state-actions"><a data-slot="button""#),
            "{out}"
        );
        assert!(out.contains(r#"data-slot="state-media""#), "{out}");
    }

    #[test]
    fn the_headline_is_a_paragraph_unless_made_a_heading() {
        let plain = html(State::of(StateKind::NotFound));
        assert!(plain.contains("<p data-slot=\"state-title\">"), "{plain}");
        assert!(!plain.contains("<h"), "{plain}");
        for level in 1..=6u8 {
            let out = html(State::of(StateKind::NotFound).heading(level));
            assert!(
                out.contains(&format!("<h{level} data-slot=\"state-title\"")),
                "{out}"
            );
        }
        let out = html(State::of(StateKind::NotFound).heading(9));
        assert!(out.contains("<h6 data-slot=\"state-title\""), "{out}");
        let out = html(State::of(StateKind::NotFound).heading(0));
        assert!(out.contains("<h1 data-slot=\"state-title\""), "{out}");
    }

    #[test]
    fn a_state_with_an_id_is_named_by_its_headline() {
        let out = html(State::of(StateKind::NotFound).id("x").heading(2));
        assert!(out.contains(r#"id="x""#), "{out}");
        assert!(out.contains(r#"aria-labelledby="x-title""#), "{out}");
        assert!(out.contains(r#"role="group""#), "{out}");
        assert!(
            out.contains(r#"<h2 data-slot="state-title" id="x-title">"#),
            "{out}"
        );
        let bare = html(State::of(StateKind::NotFound));
        assert!(
            !bare.contains("aria-labelledby") && !bare.contains("role="),
            "{bare}"
        );
    }

    #[test]
    fn what_a_state_is_told_is_escaped() {
        let hostile = "<img src=x onerror=alert(1)>\"'&";
        let out = html(
            State::of(StateKind::Failed)
                .title(hostile.to_owned())
                .description(hostile.to_owned())
                .detail(hostile)
                .wait(hostile)
                .reference(hostile, Some("Copy".into()))
                .id(hostile),
        );
        assert!(!out.contains("<img"), "{out}");
        assert!(!out.contains("onerror=alert(1)>"), "{out}");
        assert!(out.contains("&lt;img src=x onerror=alert(1)&gt;"), "{out}");
    }

    #[test]
    fn the_reference_is_selectable_and_copyable_only_when_asked() {
        let plain = html(State::of(StateKind::Failed).reference("abc123", None));
        assert!(
            plain.contains(r#"<code data-slot="code" dir="ltr">abc123</code>"#),
            "{plain}"
        );
        assert!(!plain.contains("data-ui-click"), "{plain}");
        let copy = State::of(StateKind::Failed).reference("abc123", Some("Copy reference".into()));
        let node: Node = copy.into();
        let out = render(&node);
        assert!(out.contains("data-ui-click"), "{out}");
        assert!(out.contains(r#"aria-label="Copy reference""#), "{out}");
        assert!(crate::render::measure(&node).uses.has(crate::Uses::CORE));
    }

    #[test]
    fn a_state_with_no_copy_button_needs_no_script() {
        let node: Node = State::of(StateKind::Failed)
            .reference("abc", None)
            .actions(Button::primary("Go").href("/"))
            .into();
        assert!(crate::render::measure(&node).uses.is_none());
    }

    #[test]
    fn labels_change_the_words_but_not_what_was_set_by_hand() {
        let mut labels = StateLabels::default();
        labels = labels.with_words(
            StateKind::NotFound,
            super::super::labels::KindWords {
                short: "Introuvable".into(),
                title: "Page introuvable".into(),
                description: "Cette page n'existe pas.".into(),
                toast: "Introuvable".into(),
            },
        );
        let out = html(State::of(StateKind::NotFound).labels(&labels));
        assert!(
            out.contains("Page introuvable") && out.contains("Cette page n&#39;existe pas."),
            "{out}"
        );
        let out = html(State::of(StateKind::NotFound).title("Mine").labels(&labels));
        assert!(
            out.contains("Mine") && !out.contains("Page introuvable"),
            "{out}"
        );
    }

    #[test]
    fn debug_details_are_marked() {
        let out = html(State::of(StateKind::Failed).debug(el("pre").child("boom")));
        assert!(out.contains(r#"<details data-slot="state-debug"><summary>Development only</summary><pre>boom</pre></details>"#), "{out}");
        assert!(!html(State::of(StateKind::Failed)).contains("Development only"));
    }

    #[test]
    fn the_error_page_stands_alone_with_one_h1_and_no_script() {
        let view = ErrorPage::new(StatusCode::NOT_FOUND)
            .reference("req-1")
            .into_view();
        let page = view.to_html();
        assert!(page.contains("<title>Page not found</title>"), "{page}");
        assert_eq!(page.matches("<h1").count(), 1, "{page}");
        assert!(
            page.contains(r#"<main data-slot="state-page" id="main">"#),
            "{page}"
        );
        assert!(!page.contains("<script"), "{page}");
        assert!(page.contains("req-1"), "{page}");
        assert_eq!(view.uses(), crate::Uses::NONE);
        let response = ErrorPage::new(StatusCode::NOT_FOUND).into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn each_kind_offers_what_the_user_can_do() {
        let actions = |status: u16| {
            ErrorPage::new(StatusCode::from_u16(status).unwrap())
                .sign_in("/sign-in?next=%2Fx")
                .retry_to("/x")
                .into_view()
                .to_html()
        };
        assert!(actions(401).contains(r#"href="/sign-in?next=%2Fx""#));
        assert!(actions(401).contains("Sign in"));
        assert!(actions(403).contains("Go to home"));
        assert!(actions(404).contains("Go to home"));
        assert!(actions(409).contains("Reload") && actions(409).contains(r#"href="/x""#));
        assert!(actions(419).contains("Reload"));
        assert!(actions(429).contains("Try again") && !actions(429).contains("Go to home"));
        assert!(actions(500).contains("Try again") && actions(500).contains("Go to home"));
        assert!(actions(503).contains("Try again"));
    }

    #[test]
    fn a_page_for_a_request_a_link_cannot_repeat_offers_home_and_no_retry() {
        for status in [409, 419, 429, 500, 503] {
            let page = ErrorPage::new(StatusCode::from_u16(status).unwrap())
                .without_retry()
                .retry_to("/post-address")
                .into_view()
                .to_html();
            assert!(!page.contains("post-address"), "{status}: {page}");
            // The explanation may still say "try again"; the actions are what is checked.
            let actions = page
                .split(r#"data-slot="state-actions""#)
                .nth(1)
                .and_then(|rest| rest.split("</div>").next())
                .expect("actions");
            assert!(
                !actions.contains("Try again") && !actions.contains("Reload"),
                "{status}: {actions}"
            );
            assert!(actions.contains("Go to home"), "{status}: {actions}");
            assert!(
                actions.contains(r#"data-variant="primary" href="/""#),
                "Home is the main action: {actions}"
            );
        }
        // The sign-in link is not a retry.
        let page = ErrorPage::new(StatusCode::UNAUTHORIZED)
            .without_retry()
            .into_view()
            .to_html();
        assert!(page.contains(r#"href="/sign-in""#), "{page}");
    }

    #[test]
    fn a_rate_limit_says_how_long() {
        let page = ErrorPage::new(StatusCode::TOO_MANY_REQUESTS)
            .retry_after(12)
            .into_view()
            .to_html();
        assert!(page.contains("Try again in 12 seconds."), "{page}");
    }

    #[test]
    fn the_shell_page_has_the_short_title_as_its_heading_and_the_state_below() {
        let shell = crate::Shell::new("Agra");
        let app = shell.for_request("/missing", &http::HeaderMap::new());
        let page = ErrorPage::not_found()
            .reference("req-2")
            .in_shell(&app)
            .to_view()
            .to_html();
        assert!(
            page.contains(r#"<h1 data-slot="page-title">Page not found</h1>"#),
            "{page}"
        );
        assert!(
            page.contains(
                r#"<p data-slot="state-title" id="error-title">We can&#39;t find that page</p>"#
            ),
            "{page}"
        );
        assert_eq!(page.matches("<h1").count(), 1, "{page}");
        assert!(page.contains("data-slot=\"sidebar\""), "{page}");
        // The copy button asks for the core runtime, which the shell loads anyway.
        assert!(page.contains("aria-label=\"Copy reference\""), "{page}");
    }

    #[test]
    fn forbidden_and_not_found_name_the_thing_without_saying_whether_it_exists() {
        let forbidden = html(Forbidden::new().what("deal"));
        assert!(
            forbidden.contains("You don&#39;t have access to this deal."),
            "{forbidden}"
        );
        assert!(!forbidden.contains("state-description"), "{forbidden}");
        assert!(!forbidden.contains("exist"), "{forbidden}");
        let missing = html(NotFound::new().what("deal"));
        assert!(
            missing.contains("This deal doesn&#39;t exist, or it was removed."),
            "{missing}"
        );
        let plain = html(Forbidden::new().request_access("/access?deal=1"));
        assert!(
            plain.contains("Request access") && plain.contains(r#"href="/access?deal=1""#),
            "{plain}"
        );
    }

    #[test]
    fn a_locked_row_spans_the_table_and_is_not_a_record() {
        let out = html(Forbidden::new().row(5));
        assert!(
            out.starts_with(r#"<tr data-slot="state-row" data-kind="forbidden"><td colspan="5">"#),
            "{out}"
        );
        assert!(
            out.contains("You no longer have access to this row."),
            "{out}"
        );
        assert!(out.contains(r#"data-size="compact""#), "{out}");
        assert!(
            !out.contains("data-key") && !out.contains("checkbox"),
            "{out}"
        );
        assert!(html(NotFound::new().row(0)).contains(r#"colspan="1""#));
        assert!(html(NotFound::new().row(3)).contains("This row was removed."));
        assert!(html(EmptyState::no_results().row(4)).contains(r#"colspan="4""#));
    }

    #[test]
    fn the_sign_in_state_carries_the_way_back() {
        let here = ReturnTo::parse("/deals?per=50").unwrap();
        let out = html(
            SignInRequired::new("/sign-in")
                .return_to(&here)
                .what("your deals"),
        );
        assert!(out.contains("Sign in to see your deals."), "{out}");
        assert!(
            out.contains(r#"href="/sign-in?next=%2Fdeals%3Fper%3D50""#),
            "{out}"
        );
        let out = html(SignInRequired::new("https://sso.example/login"));
        assert!(out.contains(r#"href="https://sso.example/login""#), "{out}");
    }

    #[test]
    fn the_signed_out_notice_is_an_alert_with_no_picture() {
        let out = html(SessionEnded::new());
        assert!(out.contains("You&#39;ve been signed out"), "{out}");
        assert!(
            out.contains(r#"data-slot="alert""#) && !out.contains("<img"),
            "{out}"
        );
    }

    #[test]
    fn empty_states_say_what_to_do() {
        let first = html(EmptyState::nothing_yet("deals").action(Button::primary("New deal")));
        assert!(
            first.contains("No deals yet") && first.contains("New deal"),
            "{first}"
        );
        assert!(first.contains(r#"data-kind="nothing-yet""#), "{first}");
        let none = html(EmptyState::no_results().clear_filters("/deals"));
        assert!(
            none.contains("No results") && none.contains("Clear filters"),
            "{none}"
        );
        assert!(
            none.contains(r#"href="/deals""#) && none.contains("data-ui-in-region"),
            "{none}"
        );
        let done = html(EmptyState::caught_up().title("Inbox zero").compact());
        assert!(
            done.contains("Inbox zero") && done.contains(r#"data-size="compact""#),
            "{done}"
        );
        // An empty state is not an error: it carries no code.
        assert!(
            !none.contains("state-code") && !none.contains("data-status"),
            "{none}"
        );
    }
}
