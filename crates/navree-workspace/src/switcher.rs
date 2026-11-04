//! [`WorkspaceSwitcher`] (feature `ui`): the Navree UI menu a browser
//! switches workspace with.

use std::borrow::Cow;

use http::request::Parts;
use navree_contracts::AuthState;
use navree_http::AppError;
use navree_http::extract::FromRequestParts;
use navree_kernel::AppContext;
use navree_session::Session;
use navree_ui::form::csrf_field;
use navree_ui::html::Href;
use navree_ui::{Node, Uses, el, icon, icons};

use crate::controller::visible_workspaces;
use crate::current::try_resolve;
use crate::{DemoWorkspaces, WorkspaceConfig, WorkspaceRole, Workspaces};

/// The id the switcher's menu has unless [`id`](WorkspaceSwitcher::id) sets one.
const DEFAULT_ID: &str = "ui-workspace-switcher";

/// One workspace in the menu.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Choice {
    /// What the form posts as `workspace`: a slug or an id.
    value: String,
    /// The name the menu shows.
    name: String,
    /// Whether it is the workspace the page is in.
    current: bool,
}

/// A button showing the current workspace, and a menu of the user's
/// workspaces. Choosing one posts a plain form to the switch route
/// (`workspace.switch_path`, default `/workspace/switch`) with the session's
/// CSRF token, the workspace and the page to come back to, so it works with
/// JavaScript off; the route stores the choice for the `session` resolver
/// and the browser loads the page again, inside the new workspace, with
/// every region and stream of the page opened again there. The current
/// workspace is marked `aria-current="true"`.
///
/// As an extractor it builds itself for the request: the signed-in user's
/// workspaces (their memberships, their organization's grants, and demo
/// workspaces when `workspace.demo.guest_role` is set), the one the request
/// resolves to, the session's CSRF token, the configured switch path and
/// this page as `next` (401 for a guest).
///
/// ```ignore
/// async fn deals(shell: AppShell, switcher: WorkspaceSwitcher, ws: CurrentWorkspace) -> AppPage {
///     shell.page("Deals").actions(switcher).body(/* ... */)
/// }
/// ```
///
/// Or by hand:
///
/// ```
/// use navree_ui::{Node, render};
/// use navree_workspace::WorkspaceSwitcher;
///
/// let switcher = WorkspaceSwitcher::new("Acme")
///     .workspace("acme", "Acme", true)
///     .workspace("globex", "Globex", false)
///     .csrf("token")
///     .next("/deals");
/// let html = render(&Node::from(switcher));
/// assert!(html.contains(r#"action="/workspace/switch""#));
/// assert!(html.contains(r#"aria-current="true""#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct WorkspaceSwitcher {
    id: String,
    current: Cow<'static, str>,
    label: Cow<'static, str>,
    choices: Vec<Choice>,
    action: String,
    csrf: Option<String>,
    next: Option<String>,
}

impl WorkspaceSwitcher {
    /// A switcher whose button shows `current`, the name of the workspace
    /// the page is in (or words such as "Choose a workspace").
    pub fn new(current: impl Into<Cow<'static, str>>) -> Self {
        Self {
            id: DEFAULT_ID.to_owned(),
            current: current.into(),
            label: Cow::Borrowed("Workspaces"),
            choices: Vec::new(),
            action: "/workspace/switch".to_owned(),
            csrf: None,
            next: None,
        }
    }

    /// Adds a workspace: `value` is what the form posts (its slug or id),
    /// `name` what the menu shows, and `current` marks the one the page is in.
    pub fn workspace(
        mut self,
        value: impl Into<String>,
        name: impl Into<String>,
        current: bool,
    ) -> Self {
        self.choices.push(Choice {
            value: value.into(),
            name: name.into(),
            current,
        });
        self
    }

    /// Sets where the forms post (`workspace.switch_path`).
    pub fn action(mut self, path: impl Into<String>) -> Self {
        self.action = path.into();
        self
    }

    /// Sets the session's CSRF token, which every form carries as `_csrf`.
    pub fn csrf(mut self, token: impl Into<String>) -> Self {
        self.csrf = Some(token.into());
        self
    }

    /// Sets the page to come back to after switching (a path on this site;
    /// the route sends anything else to `/`).
    pub fn next(mut self, path: impl Into<String>) -> Self {
        self.next = Some(path.into());
        self
    }

    /// Sets the menu's id (default `ui-workspace-switcher`), for a page with
    /// more than one.
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }

    /// Sets the menu's accessible name (default "Workspaces").
    pub fn label(mut self, label: impl Into<Cow<'static, str>>) -> Self {
        self.label = label.into();
        self
    }

    /// One workspace's form: the token, the workspace, the page to come
    /// back to and the button.
    fn form(&self, choice: &Choice) -> Node {
        let mark = choice.current.then(|| icon(icons::CHECK));
        el("form")
            .attr("method", "post")
            .attr("action", Href::from(self.action.as_str()))
            .children((
                self.csrf.clone().map(csrf_field),
                el("input")
                    .attr("type", "hidden")
                    .attr("name", "workspace")
                    .attr("value", choice.value.clone()),
                self.next.clone().map(|next| {
                    el("input")
                        .attr("type", "hidden")
                        .attr("name", "next")
                        .attr("value", next)
                }),
                el("button")
                    .slot("menu-link")
                    .attr("type", "submit")
                    .flag("data-active", choice.current)
                    .attr_opt("aria-current", choice.current.then_some("true"))
                    .children((mark, el("span").child(choice.name.clone()))),
            ))
            .into()
    }
}

impl From<WorkspaceSwitcher> for Node {
    fn from(switcher: WorkspaceSwitcher) -> Self {
        let trigger = el("button")
            .slot("button")
            .attr("data-variant", "outline")
            .attr("type", "button")
            .attr("id", format!("{}-trigger", switcher.id))
            .attr("popovertarget", switcher.id.clone())
            .attr("aria-haspopup", "dialog")
            .attr("title", switcher.current.clone())
            .children((
                el("span").child(switcher.current.clone()),
                icon(icons::CHEVRONS_UP_DOWN),
            ));
        let items: Vec<Node> = if switcher.choices.is_empty() {
            vec![
                el("span")
                    .slot("menu-link")
                    .attr("aria-disabled", "true")
                    .child("No workspaces yet")
                    .into(),
            ]
        } else {
            switcher
                .choices
                .iter()
                .map(|choice| switcher.form(choice))
                .collect()
        };
        let menu = el("div")
            .slot("popover-content")
            .attr("id", switcher.id.clone())
            .attr("popover", "auto")
            .attr("role", "dialog")
            .attr("tabindex", -1_i64)
            .attr("aria-label", switcher.label.clone())
            .flag("data-ui-place", true)
            .attr("data-side", "bottom")
            .attr("data-align", "start")
            .uses(Uses::OVERLAY)
            .child(el("div").slot("menu-links").child(items));
        Node::list((trigger, menu))
    }
}

/// Builds the switcher for the signed-in user of the request (see the type).
impl FromRequestParts for WorkspaceSwitcher {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        let user = parts
            .extensions
            .get::<AuthState>()
            .and_then(|state| state.0.as_ref())
            .map(|principal| principal.user_id)
            .ok_or_else(AppError::unauthenticated)?;
        let ctx = parts
            .extensions
            .get::<AppContext>()
            .cloned()
            .ok_or_else(|| {
                AppError::internal("WorkspaceSwitcher needs the navree request pipeline")
            })?;
        let needs_module =
            |_| AppError::internal("WorkspaceSwitcher needs WorkspaceModule in the app");
        let config = ctx.get::<WorkspaceConfig>().await.map_err(needs_module)?;
        let workspaces = ctx.get::<Workspaces>().await.map_err(needs_module)?;
        let demos = ctx.get::<DemoWorkspaces>().await.map_err(needs_module)?;
        let guest_role = config.demo.guest_role.as_deref().map(WorkspaceRole::parse);
        let visible = visible_workspaces(&workspaces, &demos, guest_role.as_ref(), user).await?;
        let current = try_resolve(parts).await;
        let mut switcher = WorkspaceSwitcher::new(match &current {
            Some(current) => Cow::Owned(current.name.clone()),
            None => Cow::Borrowed("Choose a workspace"),
        })
        .action(config.switch_path.clone());
        for workspace in visible {
            let is_current = current.as_ref().is_some_and(|c| c.id() == workspace.id);
            switcher = switcher.workspace(workspace.id.to_string(), workspace.name, is_current);
        }
        if let Some(session) = parts.extensions.get::<Session>() {
            switcher = switcher.csrf(session.csrf_token());
        }
        let here = parts
            .uri
            .path_and_query()
            .map_or_else(|| parts.uri.path().to_owned(), |pq| pq.as_str().to_owned());
        Ok(switcher.next(here))
    }
}

#[cfg(test)]
mod tests {
    use navree_ui::render;

    use super::*;

    // MOD-003: every workspace is a plain form posting the token, the
    // workspace and the page to come back to; the current one is marked.
    #[test]
    fn each_workspace_is_a_form_that_works_without_script() {
        let html = render(&Node::from(
            WorkspaceSwitcher::new("Acme")
                .workspace("acme", "Acme", true)
                .workspace("globex", "Globex <Ltd>", false)
                .action("/app/workspace/switch")
                .csrf("tok")
                .next("/deals?stage=won"),
        ));
        assert_eq!(html.matches("<form").count(), 2, "{html}");
        assert_eq!(
            html.matches(r#"action="/app/workspace/switch""#).count(),
            2,
            "{html}"
        );
        assert_eq!(
            html.matches(r#"<input type="hidden" name="_csrf" value="tok">"#)
                .count(),
            2,
            "{html}"
        );
        assert!(
            html.contains(r#"<input type="hidden" name="workspace" value="globex">"#),
            "{html}"
        );
        assert!(
            html.contains(r#"<input type="hidden" name="next" value="/deals?stage=won">"#),
            "{html}"
        );
        assert_eq!(html.matches(r#"aria-current="true""#).count(), 1, "{html}");
        assert!(
            html.contains("Globex &lt;Ltd&gt;"),
            "names are escaped: {html}"
        );
        assert!(
            html.contains(r#"popovertarget="ui-workspace-switcher""#),
            "{html}"
        );
        // A plain post and a full page load: no runtime form handling, so
        // every region and stream of the next page opens in the new workspace.
        assert!(!html.contains("data-ui-form"), "{html}");
    }

    // MOD-003: no token, no field (an app on bearer tokens), and an empty
    // list says so.
    #[test]
    fn without_a_token_or_workspaces() {
        let html = render(&Node::from(
            WorkspaceSwitcher::new("Choose a workspace").id("ws"),
        ));
        assert!(!html.contains("_csrf"), "{html}");
        assert!(html.contains("No workspaces yet"), "{html}");
        assert!(html.contains(r#"id="ws""#), "{html}");
        let html = render(&Node::from(
            WorkspaceSwitcher::new("Acme").workspace("acme", "Acme", true),
        ));
        assert!(!html.contains("_csrf"), "{html}");
        assert!(!html.contains(r#"name="next""#), "{html}");
    }
}
