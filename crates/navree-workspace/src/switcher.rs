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
#[derive(Clone, Debug)]
struct Choice {
    /// What the form posts as `workspace`: a slug or an id.
    value: String,
    /// The name the menu shows.
    name: String,
    /// Whether it is the workspace the page is in.
    current: bool,
    /// Its picture before its name, when the app gives one.
    media: Option<Node>,
}

/// How a [`WorkspaceSwitcher`]'s button is drawn. What it opens and what the
/// menu posts are the same in each.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum SwitcherVariant {
    /// An outline button with the workspace's name and a chevron: the
    /// default.
    #[default]
    Button,
    /// A card as wide as its place: the workspace's picture
    /// ([`media`](WorkspaceSwitcher::media)), its name over a second line
    /// ([`description`](WorkspaceSwitcher::description)) and a chevron at
    /// the end, in the theme's `card` colour and `field` corner. Made for
    /// the sidebar's header slot, whose rows it lines up with.
    Card,
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
/// let switcher = WorkspaceSwitcher::new("Agra")
///     .workspace("agra", "Agra", true)
///     .workspace("gulati", "Gulati", false)
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
    variant: SwitcherVariant,
    media: Option<Node>,
    description: Option<Cow<'static, str>>,
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
            variant: SwitcherVariant::Button,
            media: None,
            description: None,
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
            media: None,
        });
        self
    }

    /// Adds a workspace with its picture before its name in the menu, such
    /// as its initials (`Avatar::initials(name).size(Size::Sm)`, which fits
    /// the menu's rows). The picture is decorative: the name says which
    /// workspace it is. The current one's tick moves to the end of its row.
    pub fn workspace_with(
        mut self,
        value: impl Into<String>,
        name: impl Into<String>,
        current: bool,
        media: impl Into<Node>,
    ) -> Self {
        self.choices.push(Choice {
            value: value.into(),
            name: name.into(),
            current,
            media: Some(media.into()),
        });
        self
    }

    /// Sets how the button is drawn: [`SwitcherVariant::Button`] by default,
    /// or [`SwitcherVariant::Card`].
    pub fn variant(mut self, variant: SwitcherVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Sets the current workspace's picture, which the card draws before its
    /// name: its initials (`Avatar::initials(name).solid()`) or its logo. It
    /// is decorative, as the name beside it says which workspace it is. The
    /// outline button draws no picture.
    pub fn media(mut self, media: impl Into<Node>) -> Self {
        self.media = Some(media.into());
        self
    }

    /// Sets the card's second line, under the workspace's name: a place, a
    /// role (`Ludhiana · Admin`). The outline button draws no second
    /// line.
    pub fn description(mut self, description: impl Into<Cow<'static, str>>) -> Self {
        self.description = Some(description.into());
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
        let label = el("span").child(choice.name.clone());
        let row = match &choice.media {
            // With a picture, the tick goes to the end so the pictures line up.
            Some(media) => Node::list((decorative(media.clone()), label, mark)),
            None => Node::list((mark, label)),
        };
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
                    .uses(if choice.media.is_some() {
                        Uses::SHELL_PARTS
                    } else {
                        Uses::NONE
                    })
                    .child(row),
            ))
            .into()
    }

    /// The button that opens the menu, as the variant draws it.
    fn trigger(&self) -> Node {
        let button = el("button");
        let button = match self.variant {
            SwitcherVariant::Button => button.slot("button").attr("data-variant", "outline"),
            SwitcherVariant::Card => button.slot("switcher-card").uses(Uses::SHELL_PARTS),
        };
        let button = button
            .attr("type", "button")
            .attr("id", format!("{}-trigger", self.id))
            .attr("popovertarget", self.id.clone())
            .attr("aria-haspopup", "dialog")
            .attr("title", self.current.clone());
        match self.variant {
            SwitcherVariant::Card => button
                .children((
                    self.media.clone().map(decorative),
                    el("span").slot("switcher-text").children((
                        el("span").slot("switcher-name").child(self.current.clone()),
                        self.description.clone().map(|description| {
                            el("span").slot("switcher-description").child(description)
                        }),
                    )),
                    icon(icons::CHEVRONS_UP_DOWN),
                ))
                .into(),
            SwitcherVariant::Button => button
                .children((
                    el("span").child(self.current.clone()),
                    icon(icons::CHEVRONS_UP_DOWN),
                ))
                .into(),
        }
    }
}

/// A workspace's picture, which assistive technology passes over: the name
/// beside it says which workspace it is.
fn decorative(media: Node) -> Node {
    el("span")
        .slot("switcher-media")
        .attr("aria-hidden", "true")
        .child(media)
        .into()
}

impl From<WorkspaceSwitcher> for Node {
    fn from(switcher: WorkspaceSwitcher) -> Self {
        let trigger = switcher.trigger();
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

    // Every workspace is a plain form posting the token, the
    // workspace and the page to come back to; the current one is marked.
    #[test]
    fn each_workspace_is_a_form_that_works_without_script() {
        let html = render(&Node::from(
            WorkspaceSwitcher::new("Agra")
                .workspace("agra", "Agra", true)
                .workspace("gulati", "Gulati <Ltd>", false)
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
            html.contains(r#"<input type="hidden" name="workspace" value="gulati">"#),
            "{html}"
        );
        assert!(
            html.contains(r#"<input type="hidden" name="next" value="/deals?stage=won">"#),
            "{html}"
        );
        assert_eq!(html.matches(r#"aria-current="true""#).count(), 1, "{html}");
        assert!(
            html.contains("Gulati &lt;Ltd&gt;"),
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

    // A switcher that uses no option of the card draws, byte for byte, what
    // it drew before the card existed: the outline button and the menu.
    #[test]
    fn a_switcher_with_no_card_draws_what_it_drew_before() {
        let html = render(&Node::from(
            WorkspaceSwitcher::new("Agra")
                .workspace("agra", "Agra", true)
                .workspace("gulati", "Gulati", false)
                .label("Desks")
                .csrf("tok")
                .next("/deals"),
        ));
        assert_eq!(
            html,
            concat!(
                r#"<button data-slot="button" data-variant="outline" type="button" id="ui-workspace-switcher-trigger" popovertarget="ui-workspace-switcher" aria-haspopup="dialog" title="Agra">"#,
                r#"<span>Agra</span><svg data-slot="icon" viewBox="0 0 24 24" aria-hidden="true"><path d="m7 15 5 5 5-5"/><path d="m7 9 5-5 5 5"/></svg></button>"#,
                r#"<div data-slot="popover-content" id="ui-workspace-switcher" popover="auto" role="dialog" tabindex="-1" aria-label="Desks" data-ui-place data-side="bottom" data-align="start">"#,
                r#"<div data-slot="menu-links"><form method="post" action="/workspace/switch"><input type="hidden" name="_csrf" value="tok">"#,
                r#"<input type="hidden" name="workspace" value="agra"><input type="hidden" name="next" value="/deals">"#,
                r#"<button data-slot="menu-link" type="submit" data-active aria-current="true"><svg data-slot="icon" viewBox="0 0 24 24" aria-hidden="true"><path d="M20 6 9 17l-5-5"/></svg><span>Agra</span></button></form>"#,
                r#"<form method="post" action="/workspace/switch"><input type="hidden" name="_csrf" value="tok">"#,
                r#"<input type="hidden" name="workspace" value="gulati"><input type="hidden" name="next" value="/deals">"#,
                r#"<button data-slot="menu-link" type="submit"><span>Gulati</span></button></form></div></div>"#,
            )
        );
        let empty = render(&Node::from(
            WorkspaceSwitcher::new("Choose a workspace").id("ws"),
        ));
        assert_eq!(
            empty,
            concat!(
                r#"<button data-slot="button" data-variant="outline" type="button" id="ws-trigger" popovertarget="ws" aria-haspopup="dialog" title="Choose a workspace">"#,
                r#"<span>Choose a workspace</span><svg data-slot="icon" viewBox="0 0 24 24" aria-hidden="true"><path d="m7 15 5 5 5-5"/><path d="m7 9 5-5 5 5"/></svg></button>"#,
                r#"<div data-slot="popover-content" id="ws" popover="auto" role="dialog" tabindex="-1" aria-label="Workspaces" data-ui-place data-side="bottom" data-align="start">"#,
                r#"<div data-slot="menu-links"><span data-slot="menu-link" aria-disabled="true">No workspaces yet</span></div></div>"#,
            )
        );
    }

    fn agra() -> WorkspaceSwitcher {
        WorkspaceSwitcher::new("Agra Traders")
            .workspace("agra", "Agra Traders", true)
            .workspace("gulati", "Gulati", false)
            .csrf("tok")
            .next("/deals")
    }

    /// The trigger and the menu of a rendered switcher.
    fn halves(html: &str) -> (&str, &str) {
        html.split_at(html.find(r#"<div data-slot="popover-content""#).unwrap())
    }

    // The card is a button as wide as its place: the picture (hidden from
    // assistive technology, which hears the name), the name over a second
    // line and the chevron, with the outline button's own attributes.
    #[test]
    fn the_card_draws_a_picture_two_lines_and_the_chevron() {
        let switcher = agra()
            .variant(SwitcherVariant::Card)
            .media(el("img").attr("src", "/agra.svg").attr("alt", ""))
            .description("Ludhiana · Admin <desk>");
        let node = Node::from(switcher);
        assert!(navree_ui::measure(&node).uses.has(Uses::SHELL_PARTS));
        let html = render(&node);
        let (trigger, _) = halves(&html);
        assert_eq!(
            trigger,
            concat!(
                r#"<button data-slot="switcher-card" type="button" id="ui-workspace-switcher-trigger" popovertarget="ui-workspace-switcher" aria-haspopup="dialog" title="Agra Traders">"#,
                r#"<span data-slot="switcher-media" aria-hidden="true"><img src="/agra.svg" alt=""></span>"#,
                r#"<span data-slot="switcher-text"><span data-slot="switcher-name">Agra Traders</span>"#,
                r#"<span data-slot="switcher-description">Ludhiana · Admin &lt;desk&gt;</span></span>"#,
                r#"<svg data-slot="icon" viewBox="0 0 24 24" aria-hidden="true"><path d="m7 15 5 5 5-5"/><path d="m7 9 5-5 5 5"/></svg></button>"#,
            )
        );
        // With neither a picture nor a second line, the card draws neither.
        let bare = render(&Node::from(agra().variant(SwitcherVariant::Card)));
        assert!(!bare.contains("switcher-media"), "{bare}");
        assert!(!bare.contains("switcher-description"), "{bare}");
        assert!(bare.contains(r#"<span data-slot="switcher-name">Agra Traders</span>"#));
        // The outline button draws no picture and no second line.
        let button = render(&Node::from(
            agra()
                .media(el("img").attr("src", "/agra.svg"))
                .description("Ludhiana"),
        ));
        assert_eq!(button, render(&Node::from(agra())));
    }

    // The card changes how the button is drawn, never what the menu posts:
    // the forms, their route, token, workspace and page to come back to.
    #[test]
    fn the_card_posts_what_the_button_posts() {
        let button = render(&Node::from(agra()));
        let card = render(&Node::from(
            agra()
                .variant(SwitcherVariant::Card)
                .media(el("span").child("AT"))
                .description("Ludhiana"),
        ));
        assert_eq!(halves(&button).1, halves(&card).1);
        assert!(!card.contains("data-ui-form"), "{card}");
    }

    // A workspace's picture in the menu: decorative, before the name, the
    // tick at the end; the form posts what it posted.
    #[test]
    fn a_workspace_can_have_its_picture_in_the_menu() {
        let html = render(&Node::from(
            WorkspaceSwitcher::new("Agra")
                .workspace_with("agra", "Agra", true, el("i").child("A"))
                .workspace_with("gulati", "Gulati", false, el("i").child("G"))
                .csrf("tok")
                .next("/deals"),
        ));
        let (_, menu) = halves(&html);
        assert!(
            menu.contains(concat!(
                r#"<button data-slot="menu-link" type="submit" data-active aria-current="true">"#,
                r#"<span data-slot="switcher-media" aria-hidden="true"><i>A</i></span><span>Agra</span>"#,
                r#"<svg data-slot="icon" viewBox="0 0 24 24" aria-hidden="true"><path d="M20 6 9 17l-5-5"/></svg></button>"#,
            )),
            "{menu}"
        );
        assert!(
            menu.contains(r#"<button data-slot="menu-link" type="submit"><span data-slot="switcher-media" aria-hidden="true"><i>G</i></span><span>Gulati</span></button>"#),
            "{menu}"
        );
        assert_eq!(
            menu.matches(r#"<input type="hidden" name="_csrf" value="tok"><input type="hidden" name="workspace" value="#)
                .count(),
            2,
            "{menu}"
        );
        let plain = render(&Node::from(
            WorkspaceSwitcher::new("Agra").workspace("agra", "Agra", true),
        ));
        assert!(
            !navree_ui::measure(&Node::from(WorkspaceSwitcher::new("Agra")))
                .uses
                .has(Uses::SHELL_PARTS)
        );
        assert!(!plain.contains("switcher-media"), "{plain}");
    }

    // No token, no field (an app on bearer tokens), and an empty
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
            WorkspaceSwitcher::new("Agra").workspace("agra", "Agra", true),
        ));
        assert!(!html.contains("_csrf"), "{html}");
        assert!(!html.contains(r#"name="next""#), "{html}");
    }
}
