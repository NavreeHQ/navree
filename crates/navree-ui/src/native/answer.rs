//! One route, each client in its own way.
//!
//! A handler loads its data once and answers from it: a page for a browser,
//! and for a native client a screen written for native clients with
//! [`navree_native::ui`], in place of the page read into parts. The actions
//! are the same handlers either way: a form of the screen posts where the
//! page's form posts, and what is wrong with a field comes back at the field.
//!
//! ```
//! use navree_ui::native::{Answer, ui::*};
//! use navree_ui::{Text as Words, View};
//!
//! // What the handler loaded.
//! let revenue = "₹1,24,050";
//! let response = Answer::new().each(
//!     // A browser, and the web view inside a native client.
//!     || View::new("Dashboard").body(Words::new(revenue)),
//!     // A native client.
//!     || Screen::titled("Today").body(Text::new(revenue).style(TextStyle::Display)),
//! );
//! // No native client asked here, so this is the page.
//! assert_eq!(response.headers()["content-type"], "text/html; charset=utf-8");
//! ```
//!
//! A screen function knows nothing of the request, so the layer adds what
//! only the server knows: the app's destination the screen belongs to and a
//! toast left for it (from the shell, with [`Answer::in_shell`]), the shell's
//! stamp, the session's token in the screen's posts, the shapes of the icons
//! it names, the client's vocabulary and the limits of what a client reads.

use std::sync::Arc;

use http::header::{CACHE_CONTROL, SET_COOKIE, VARY};
use http::{HeaderValue, StatusCode};
use navree_http::response::{IntoResponse, Response};
use navree_native::action::{Action, Pair};
use navree_native::node::{self as n, Node as Part};
use navree_native::{Envelope, NATIVE_HEADER, Screen};

use super::page::{self, Page};
use super::scope::{self, Asked};
use super::swap::{self, Answered, How, NodeOp};
use crate::form::CSRF_FIELD;
use crate::shell::AppShell;
use crate::view::View;

/// Whether a native client is asking: for the handler that answers a reply
/// with native parts for one and markup for the other.
///
/// ```
/// use navree_ui::native;
///
/// // Outside a native client's request, nobody is.
/// assert!(!native::asked());
/// ```
pub fn asked() -> bool {
    scope::native().is_some()
}

/// What the native side of a route says.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Said {
    /// A whole screen.
    Screen(Box<Screen>),
    /// A part of a screen: what a region's address answers, which takes the
    /// place of what the region holds.
    Part(Box<Part>),
    /// Changes to the screen in front: what a list's next rows or its changes
    /// are answered with.
    Ops(Vec<navree_native::Op>),
}

impl From<Screen> for Said {
    fn from(screen: Screen) -> Self {
        Self::Screen(Box::new(screen))
    }
}

impl From<Part> for Said {
    fn from(part: Part) -> Self {
        Self::Part(Box::new(part))
    }
}

impl From<Vec<navree_native::Op>> for Said {
    fn from(ops: Vec<navree_native::Op>) -> Self {
        Self::Ops(ops)
    }
}

/// A route's answer for each client: see the [module](crate::native).
#[derive(Clone, Copy, Debug)]
#[must_use]
pub struct Answer<'a> {
    shell: Option<&'a AppShell>,
    status: StatusCode,
}

impl Default for Answer<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> Answer<'a> {
    /// An answer outside any shell: a screen with no destination to belong to.
    pub fn new() -> Self {
        Self {
            shell: None,
            status: StatusCode::OK,
        }
    }

    /// An answer in the app's shell, as this request has it: a native screen
    /// is marked with the destination its address belongs to, gets the toast
    /// the request before left for it, and carries the shell's stamp and the
    /// session's token, as a page read from the same shell would.
    pub fn in_shell(shell: &'a AppShell) -> Self {
        Self {
            shell: Some(shell),
            status: StatusCode::OK,
        }
    }

    /// The status the native side answers with, when not `200`: a screen
    /// that says the record is gone is still a `404`.
    pub fn status(mut self, status: StatusCode) -> Self {
        self.status = status;
        self
    }

    /// Answers each client in its own way: `web` for a browser and for the
    /// web view inside a native client, `native` for a native client. Only
    /// the one that is needed runs, and it runs here, so both may borrow what
    /// the handler loaded.
    pub fn each<R, S>(self, web: impl FnOnce() -> R, native: impl FnOnce() -> S) -> Response
    where
        R: IntoResponse,
        S: Into<Said>,
    {
        match scope::asked() {
            Some(Asked::Native(request)) => self.say(native().into(), &request),
            _ => web().into_response(),
        }
    }

    /// Answers a route that has only a native side. A browser, and the web
    /// view inside a native client, get a small page that says so: that the
    /// screen is in the app (by the app's name, [in its shell](Self::in_shell)),
    /// and the way to the app's home. Its status is `406`.
    pub fn native<S: Into<Said>>(self, native: impl FnOnce() -> S) -> Response {
        match scope::asked() {
            Some(Asked::Native(request)) => self.say(native().into(), &request),
            _ => not_a_page(self.shell),
        }
    }

    /// The view of an empty page of the shell, or of no shell: what holds the
    /// status, the cache rule, the cookies and the session's token a page of
    /// this request would have.
    fn frame(&self, title: &str) -> View {
        match self.shell {
            Some(shell) => shell.page(title.to_owned()).to_view(),
            None => View::new(title.to_owned()),
        }
        .status(self.status)
    }

    fn say(self, said: Said, request: &Arc<scope::Request>) -> Response {
        match said {
            Said::Screen(screen) => self.screen(*screen, request),
            Said::Part(part) => {
                let mut view = self.frame("");
                // A part is named after itself, or after the region that asked for it:
                // no client shows the name, and an answer with none reads as unfinished.
                let name = part
                    .id()
                    .map(str::to_owned)
                    .or_else(|| request.region.clone())
                    .unwrap_or_else(|| "Part".to_owned());
                let mut screen = Screen::new(name, *part);
                finish(&mut screen, view.csrf_token());
                let parts = view.native_parts();
                let page = Page {
                    screen,
                    ops: Vec::new(),
                    misses: Vec::new(),
                    dropped: Vec::new(),
                };
                let (envelope, answered) = swap::envelope(page, request);
                let answered = if answered.native {
                    Answered::parts(Vec::new())
                } else {
                    answered
                };
                let mut response = swap::respond(parts.status, parts.cache, envelope, request, "");
                swap::vary_also(&mut response, parts.vary);
                response.extensions_mut().insert(answered);
                response
            }
            Said::Ops(mut ops) => {
                let view = self.frame("");
                let token = view.csrf_token();
                let to = request.client.vocab;
                for op in &mut ops {
                    for node in nodes_of(op) {
                        finish_part(node, token);
                        // A part the client is too old for, or one deeper than it
                        // reads, can't be said as it is: the screen is fetched again.
                        let whole = std::mem::replace(node, Part::Stack(n::Stack::default()));
                        match navree_native::vocab::downgrade(whole, to) {
                            navree_native::vocab::Downgraded::Node(older)
                                if older.depth() <= navree_native::MAX_DEPTH =>
                            {
                                *node = older;
                            }
                            _ => {
                                let envelope = Envelope::ops(
                                    crate::assets::BUILD,
                                    vec![navree_native::Op::Reload],
                                );
                                return swap::respond(
                                    self.status,
                                    "no-store",
                                    envelope,
                                    request,
                                    "",
                                );
                            }
                        }
                    }
                }
                let envelope = Envelope::ops(crate::assets::BUILD, ops);
                let mut response = swap::respond(self.status, "no-store", envelope, request, "");
                response
                    .extensions_mut()
                    .insert(Answered::parts(Vec::new()));
                response
            }
        }
    }

    fn screen(self, mut screen: Screen, request: &Arc<scope::Request>) -> Response {
        // An empty page of the same shell says where the screen is in the app
        // and what the request before left for it; its body is never built.
        let mut view = self.frame(&screen.title);
        let token = view.csrf_token().map(str::to_owned);
        let read = page::of_view(&mut view, false);
        if screen.nav.is_none() {
            screen.nav = read.screen.nav;
        }
        finish(&mut screen, token.as_deref());
        let gaps = super::coverage::designed(&screen);
        let shell = view
            .note()
            .map(|note| note.shell_stamp(request.sign_in.as_deref(), &request.shell_parts))
            .unwrap_or_default();
        let parts = view.native_parts();
        let page = Page {
            screen,
            ops: read.ops,
            misses: Vec::new(),
            dropped: Vec::new(),
        };
        let (envelope, mut answered) = swap::envelope(page, request);
        if answered.native {
            answered.grade = super::Grade::Designed;
            answered.missing = gaps;
        }
        let mut response = swap::respond(parts.status, parts.cache, envelope, request, &shell);
        for cookie in parts.cookies {
            response.headers_mut().append(SET_COOKIE, cookie.clone());
        }
        swap::vary_also(&mut response, parts.vary);
        response.extensions_mut().insert(answered);
        response
    }
}

/// What a browser gets from a route that has only a native side: a small
/// page that says the screen is in the app, with the way to the app's home.
fn not_a_page(shell: Option<&AppShell>) -> Response {
    let mut page = crate::pages::ErrorPage::new(StatusCode::NOT_ACCEPTABLE)
        .title("This screen is in the app")
        .without_retry();
    page = match shell.map(AppShell::name_and_home) {
        Some((name, home)) => page
            .description(format!(
                "{name} shows this screen in its app. It has no page on the web."
            ))
            .home(home.clone()),
        None => page.description("This screen is shown in the app. It has no page on the web."),
    };
    let mut response = page.into_view().into_response();
    let headers = response.headers_mut();
    // The same address answers a native client with the screen.
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.append(VARY, HeaderValue::from_static(NATIVE_HEADER));
    response
}

/// The nodes an op carries.
fn nodes_of(op: &mut navree_native::Op) -> Vec<&mut Part> {
    use navree_native::Op as O;
    match op {
        O::Morph { node, .. } | O::Replace { node, .. } => vec![node],
        O::Append { nodes, .. } | O::Prepend { nodes, .. } | O::Merge { nodes, .. } => {
            nodes.iter_mut().collect()
        }
        O::Rows { rows, .. } => rows.iter_mut().flat_map(n::TableRow::nodes_mut).collect(),
        O::Row { row, .. } => row.nodes_mut().collect(),
        _ => Vec::new(),
    }
}

/// Gives a screen what its function could not know: see [`finish_part`].
fn finish(screen: &mut Screen, token: Option<&str>) {
    finish_part(&mut screen.body, token);
    for action in &mut screen.actions {
        finish_part(action, token);
    }
    if let Some(scores) = screen
        .pager
        .as_mut()
        .and_then(|pager| pager.scores.as_deref_mut())
    {
        finish_part(scores, token);
    }
}

/// Gives a part what its function could not know: the shapes of each icon it
/// names from the toolkit's set, with the system symbol that means the same,
/// and the session's token in every post it makes to this site (a form that
/// posts, a control that posts on its own) that does not carry one already.
/// A post to another site never carries the token.
fn finish_part(part: &mut Part, token: Option<&str>) {
    let mut stack = vec![part];
    while let Some(node) = stack.pop() {
        for icon in node.icons_mut() {
            draw(icon);
        }
        if let Some(token) = token {
            if let Part::Form(form) = node
                && form.method == n::Method::Post
                && swap::on_this_site(&form.action)
                && !form.hidden.iter().any(|hidden| hidden.name == CSRF_FIELD)
            {
                form.hidden.push(n::Hidden {
                    name: CSRF_FIELD.to_owned(),
                    value: token.to_owned(),
                    id: None,
                });
            }
            for action in node.actions_mut() {
                if let Action::Request {
                    method: n::Method::Post,
                    href,
                    fields,
                    ..
                } = action
                    && swap::on_this_site(href)
                    && !fields.iter().any(|pair| pair.name == CSRF_FIELD)
                {
                    fields.push(Pair {
                        name: CSRF_FIELD.to_owned(),
                        value: token.to_owned(),
                    });
                }
            }
        }
        stack.extend(node.children_mut());
    }
}

/// Gives an icon that only names itself the shapes the toolkit draws under
/// that name, and the system symbol the name stands for.
fn draw(icon: &mut n::Icon) {
    let Some(name) = icon.name.clone() else {
        return;
    };
    if icon.symbol.is_none() {
        icon.symbol = navree_native::symbols::for_name(&name);
    }
    if icon.shape().is_some() {
        return;
    }
    if let Some(glyph) = crate::icons::Icon::lookup(&name)
        && let Ok(drawn) = super::icon::icon(glyph.shapes(), glyph.view_box())
        && let Some(shape) = drawn.shape()
    {
        icon.set_shape(shape);
    }
}

/// A part with what the server knows of it: for an op built outside
/// [`Answer`].
fn finished(part: impl Into<Part>) -> Part {
    let mut part = part.into();
    finish_part(&mut part, None);
    part
}

fn node_op(how: How, target: impl Into<String>, nodes: Vec<Part>) -> crate::Op {
    crate::Op::Native(NodeOp {
        how,
        target: target.into(),
        nodes,
        misses: Vec::new(),
    })
}

/// Morphs the part `target` of a native client's screen into `part`, keeping
/// what the reader did: what a [`Reply`](crate::Reply) carries for a native
/// client where it carries [`Op::morph`](crate::Op::morph) for a browser.
/// Build it only when a native client [`asked`].
///
/// ```
/// use navree_ui::native::{self, ui::*};
/// use navree_ui::Reply;
///
/// let reply = Reply::ops_or_redirect("/orders/1")
///     .op(native::morph("order-total", Text::new("₹1,240").id("order-total")));
/// # let _ = reply;
/// ```
pub fn morph(target: impl Into<String>, part: impl Into<Part>) -> crate::Op {
    node_op(How::Morph, target, vec![finished(part)])
}

/// Puts `part` in the place of the part `target` of a native client's
/// screen, as new. See [`morph`].
pub fn replace(target: impl Into<String>, part: impl Into<Part>) -> crate::Op {
    node_op(How::Replace, target, vec![finished(part)])
}

/// Adds `parts` at the end of the part `target` of a native client's screen.
/// See [`morph`].
pub fn append(target: impl Into<String>, parts: impl navree_native::ui::IntoParts) -> crate::Op {
    let mut nodes = Vec::new();
    parts.push_into(&mut nodes);
    node_op(
        How::Append,
        target,
        nodes.into_iter().map(finished).collect(),
    )
}

/// Adds `parts` at the start of the part `target` of a native client's
/// screen. See [`morph`].
pub fn prepend(target: impl Into<String>, parts: impl navree_native::ui::IntoParts) -> crate::Op {
    let mut nodes = Vec::new();
    parts.push_into(&mut nodes);
    node_op(
        How::Prepend,
        target,
        nodes.into_iter().map(finished).collect(),
    )
}

#[cfg(test)]
mod tests {
    use http::header::CONTENT_TYPE;
    use navree_native::ui::{self, Button, Card, Field, Form, Icon, Stack, Text, TextStyle};
    use navree_native::{Client, FormFactor, Op, Platform};

    use super::*;
    use crate::native::Grade;
    use crate::{NavGroup, NavItem, Shell, ToastTone, icons};

    fn shop() -> Shell {
        Shell::new("Charkha Supply Co.").group(
            NavGroup::new("Store")
                .item(NavItem::new("Dashboard", "/").icon(icons::LAYOUT_DASHBOARD))
                .item(NavItem::new("Orders", "/orders").icon(icons::SHOPPING_BAG)),
        )
    }

    /// Runs `build` as the handler of a native client's request for `url`.
    fn as_native<T>(url: &str, build: impl FnOnce() -> T) -> T {
        let client = Client::new(Platform::Ios, FormFactor::Phone);
        scope::sync(
            Asked::Native(Arc::new(scope::Request::new(client, url))),
            build,
        )
    }

    async fn envelope(response: Response) -> Envelope {
        assert_eq!(
            response.headers()[CONTENT_TYPE],
            navree_native::CONTENT_TYPE,
            "not a native answer"
        );
        let bytes = navree_http::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    fn today() -> Screen {
        Screen::titled("Today").body(
            Stack::down().id("today").parts((
                Text::new("₹1,24,050").style(TextStyle::Display),
                Card::new().parts(Icon::named("shopping-bag")),
                Form::post("/orders/1/refund")
                    .id("refund")
                    .parts(Field::amount("amount", "₹")),
                Button::new("Resend").post("/orders/1/resend", [("copy", "1")]),
            )),
        )
    }

    #[tokio::test]
    async fn a_native_client_gets_the_screen_and_a_browser_the_page() {
        let both = || {
            Answer::new().each(
                || View::new("Dashboard").body(crate::Text::new("the page")),
                today,
            )
        };
        // A browser, and anything outside a native client's request: the page.
        let page = both();
        assert_eq!(page.headers()[CONTENT_TYPE], "text/html; charset=utf-8");
        let html = navree_http::body::to_bytes(page.into_body(), 1 << 20)
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&html).contains("the page"));
        // A native client: the screen that was written for it, not the page read.
        let response = as_native("/", both);
        let answered = response.extensions().get::<Answered>().cloned().unwrap();
        assert_eq!((answered.native, answered.grade), (true, Grade::Designed));
        assert!(answered.missing.is_empty(), "{:?}", answered.missing);
        let screen = envelope(response).await.screen.unwrap();
        assert_eq!(screen.title, "Today");
        assert_eq!(screen.id.as_deref(), Some("today"));
        assert!(matches!(
            screen.body.find("today").map(|body| body.children().len()),
            Some(4)
        ));
    }

    #[tokio::test]
    async fn only_the_side_that_is_asked_for_is_built() {
        let page = Answer::new().each(
            || View::new("Web"),
            || -> Screen { panic!("the screen is not built for a browser") },
        );
        assert_eq!(page.status(), StatusCode::OK);
        let response = as_native("/", || {
            Answer::new().each(
                || -> View { panic!("the page is not built for a native client") },
                today,
            )
        });
        assert_eq!(envelope(response).await.screen.unwrap().title, "Today");
    }

    #[tokio::test]
    async fn the_web_view_inside_a_native_client_gets_the_page() {
        let response = scope::sync(Asked::Embed, || {
            Answer::new().each(|| View::new("Dashboard"), today)
        });
        assert_eq!(response.headers()[CONTENT_TYPE], "text/html; charset=utf-8");
    }

    #[tokio::test]
    async fn a_route_with_only_a_native_side_gives_a_browser_a_small_page_that_says_so() {
        let text = |response: Response| async {
            let bytes = navree_http::body::to_bytes(response.into_body(), 1 << 20)
                .await
                .unwrap();
            String::from_utf8(bytes.to_vec()).unwrap()
        };
        // Outside any shell: the page says where the screen is.
        let refused = Answer::new().native(today);
        assert_eq!(refused.status(), StatusCode::NOT_ACCEPTABLE);
        assert_eq!(refused.headers()[CONTENT_TYPE], "text/html; charset=utf-8");
        assert_eq!(refused.headers()[CACHE_CONTROL], "no-store");
        assert!(
            refused
                .headers()
                .get_all(VARY)
                .iter()
                .any(|value| value == NATIVE_HEADER)
        );
        let html = text(refused).await;
        assert!(
            html.contains("<title>This screen is in the app</title>"),
            "{html}"
        );
        assert!(html.contains("This screen is shown in the app. It has no page on the web."));
        assert!(html.contains(r#"href="/""#), "the way home");
        // The screen itself is never built for a browser, and nothing of it is in the page.
        assert!(!html.contains("₹1,24,050"));
        // In the app's shell: by the app's name, with the way to the app's home.
        let shell = Shell::new("Charkha Supply Co.")
            .home("/orders")
            .for_request("/shelves", &http::HeaderMap::new());
        let refused = Answer::in_shell(&shell)
            .native(|| -> Screen { panic!("the screen is not built for a browser") });
        assert_eq!(refused.status(), StatusCode::NOT_ACCEPTABLE);
        let html = text(refused).await;
        assert!(
            html.contains(
                "Charkha Supply Co. shows this screen in its app. It has no page on the web."
            ),
            "{html}"
        );
        assert!(
            html.contains(r#"href="/orders""#),
            "the way to the app's home"
        );
        // The web view inside a native client is told the same.
        let inside = scope::sync(Asked::Embed, || Answer::new().native(today));
        assert_eq!(inside.status(), StatusCode::NOT_ACCEPTABLE);
        assert_eq!(inside.headers()[CONTENT_TYPE], "text/html; charset=utf-8");
        let response = as_native("/shelves", || {
            Answer::new().status(StatusCode::NOT_FOUND).native(today)
        });
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(envelope(response).await.screen.unwrap().title, "Today");
    }

    #[test]
    fn the_sessions_token_goes_only_with_a_post_to_this_site() {
        let mut screen = Screen::titled("Pay").body(Stack::down().parts((
            Form::post("/orders/1/refund").id("here"),
            Form::post("https://pay.example.in/checkout").id("there"),
            Form::post("//pay.example.in/checkout").id("no-scheme"),
            Button::new("Resend").post("/orders/1/resend", [("copy", "1")]),
            Button::new("Pay").post("https://pay.example.in/pay", [("order", "1")]),
        )));
        finish(&mut screen, Some("tok"));
        let carries = |id: &str| {
            let Some(Part::Form(form)) = screen.body.find(id) else {
                panic!("no form {id}");
            };
            form.hidden.iter().any(|hidden| hidden.name == CSRF_FIELD)
        };
        assert!(carries("here"));
        assert!(!carries("there"));
        assert!(!carries("no-scheme"));
        let posts: Vec<(String, bool)> = screen
            .body
            .children()
            .into_iter()
            .filter_map(|part| match part {
                Part::Button(button) => match &button.action {
                    Some(Action::Request { href, fields, .. }) => Some((
                        href.clone(),
                        fields.iter().any(|pair| pair.name == CSRF_FIELD),
                    )),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        assert_eq!(
            posts,
            [
                ("/orders/1/resend".to_owned(), true),
                ("https://pay.example.in/pay".to_owned(), false),
            ]
        );
    }

    #[test]
    fn a_browsers_page_a_shared_cache_may_keep_varies_by_the_web_view_too() {
        let page = |cache: Option<&'static str>| {
            let mut response = "<p>Dal makhani</p>".into_response();
            response.headers_mut().insert(
                CONTENT_TYPE,
                HeaderValue::from_static("text/html; charset=utf-8"),
            );
            if let Some(cache) = cache {
                response
                    .headers_mut()
                    .insert(CACHE_CONTROL, HeaderValue::from_static(cache));
            }
            swap::vary_on_native(&mut response);
            response
                .headers()
                .get_all(VARY)
                .iter()
                .filter_map(|value| value.to_str().ok())
                .collect::<Vec<_>>()
                .join(", ")
        };
        for kept in [Some("public, max-age=300"), Some("max-age=60"), None] {
            assert_eq!(
                page(kept),
                "x-navree-native, x-navree-embed, user-agent",
                "{kept:?}"
            );
        }
        for not_kept in ["no-store", "private, max-age=60", "Private"] {
            assert_eq!(page(Some(not_kept)), "x-navree-native", "{not_kept}");
        }
    }

    #[tokio::test]
    async fn an_envelope_varies_by_the_client_and_by_what_the_page_varies_by() {
        let response = as_native("/menu", || {
            View::new("Menu")
                .cache("public, max-age=300")
                .vary("Cookie")
                .body(crate::Text::new("Dal makhani"))
                .into_response()
        });
        let vary: Vec<&str> = response
            .headers()
            .get_all(VARY)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .collect();
        let vary = vary.join(", ").to_ascii_lowercase();
        for header in ["x-navree-native", "x-navree-client", "cookie"] {
            assert!(vary.contains(header), "{header}: {vary}");
        }
        // The web view's page varies by what the page varies by too.
        let response = scope::sync(Asked::Embed, || {
            View::new("Menu")
                .cache("public, max-age=300")
                .vary("Cookie")
                .body(crate::Text::new("Dal makhani"))
                .into_response()
        });
        let vary: Vec<String> = response
            .headers()
            .get_all(VARY)
            .iter()
            .filter_map(|value| value.to_str().ok().map(str::to_ascii_lowercase))
            .collect();
        assert!(
            vary.iter().any(|value| value.contains("cookie")),
            "{vary:?}"
        );
        assert!(
            vary.iter().any(|value| value.contains("user-agent")),
            "{vary:?}"
        );
    }

    #[tokio::test]
    async fn the_shell_says_where_the_screen_is_and_what_was_left_for_it() {
        let mut headers = http::HeaderMap::new();
        headers.insert(
            http::header::COOKIE,
            crate::shell::flash_cookie_toned(ToastTone::Success, "Refunded")
                .unwrap()
                .to_str()
                .unwrap()
                .split(';')
                .next()
                .unwrap()
                .parse()
                .unwrap(),
        );
        let shell = shop().for_request("/orders/7", &headers).csrf("tok");
        let response = as_native("/orders/7", || Answer::in_shell(&shell).native(today));
        // The toast is taken: the cookie that held it is cleared.
        assert!(response.headers().contains_key(SET_COOKIE));
        let sent = envelope(response).await;
        let screen = sent.screen.as_ref().unwrap();
        // The destination the address belongs to, as a page read from the shell is marked.
        assert_eq!(screen.nav.as_deref(), Some("/orders"));
        assert!(matches!(sent.ops.as_slice(), [Op::Toast(toast)] if toast.text == "Refunded"));
        // The shell's stamp is the stamp of the shell's own document for this person.
        let document = shell.native_document(None, &crate::NativeShellParts::default());
        assert_eq!(sent.shell, document.stamp);
        // The session's token goes with every post the screen makes.
        let Some(Part::Form(form)) = screen.body.find("refund") else {
            panic!("no form");
        };
        assert!(
            form.hidden
                .iter()
                .any(|hidden| hidden.name == CSRF_FIELD && hidden.value == "tok")
        );
        let posts: Vec<&Part> = screen
            .body
            .children()
            .into_iter()
            .filter(|part| matches!(part, Part::Button(_)))
            .collect();
        let [Part::Button(resend)] = posts.as_slice() else {
            panic!("no button");
        };
        assert!(
            matches!(&resend.action, Some(Action::Request { fields, .. })
            if fields.iter().any(|pair| pair.name == CSRF_FIELD && pair.value == "tok")
                && fields.iter().any(|pair| pair.name == "copy"))
        );
        // A screen that says its own destination keeps it.
        let response = as_native("/orders/7", || {
            Answer::in_shell(&shell).native(|| today().nav("/"))
        });
        assert_eq!(
            envelope(response)
                .await
                .screen
                .and_then(|screen| screen.nav)
                .as_deref(),
            Some("/"),
            "the screen's own word wins"
        );
    }

    #[tokio::test]
    async fn an_icon_a_screen_names_is_sent_with_its_shapes_and_its_symbol() {
        let response = as_native("/", || Answer::new().native(today));
        let mut envelope = envelope(response).await;
        // Said once, in the envelope's table, as every icon of an answer is.
        assert!(envelope.icons.contains_key("shopping-bag"));
        envelope.inline_icons();
        let screen = envelope.screen.unwrap();
        let icon = screen
            .body
            .children()
            .into_iter()
            .find_map(|part| match part {
                Part::Card(card) => card.children.first(),
                _ => None,
            })
            .unwrap();
        let Part::Icon(icon) = icon else {
            panic!("not an icon");
        };
        assert!(!icon.paths.is_empty());
        assert_eq!(
            icon.symbol.as_ref().map(|symbol| symbol.apple.as_str()),
            Some("bag")
        );
    }

    #[tokio::test]
    async fn a_region_is_answered_with_a_part_and_a_list_with_ops() {
        let part = as_native("/fragments/figures", || {
            Answer::new().each(
                || View::new("Figures"),
                || Part::from(Stack::down().id("figures").parts(Text::new("12 to pack"))),
            )
        });
        let answered = part.extensions().get::<Answered>().cloned().unwrap();
        assert_eq!(answered.grade, Grade::Idiom);
        let envelope_of_part = envelope(part).await;
        assert_eq!(envelope_of_part.screen.unwrap().body.id(), Some("figures"));
        let rows = as_native("/fragments/activity?since=7", || {
            Answer::new().each(
                || View::new("Activity"),
                || {
                    vec![Op::merge(
                        "activity",
                        "8",
                        ui::Edge::Start,
                        ui::Row::new("Order CS-20481 was packed").id("event-8"),
                        [],
                    )]
                },
            )
        });
        let ops = envelope(rows).await.ops;
        assert!(
            matches!(ops.as_slice(), [Op::Merge { target, nodes, since, .. }]
            if target == "activity" && nodes.len() == 1 && since == "8")
        );
    }

    #[tokio::test]
    async fn a_reply_carries_native_parts_built_with_the_same_components() {
        let tile = |left: &str| Card::new().id("shelf-7").parts(Text::new(left));
        let response = as_native("/inventory/7/restock", || {
            // What a handler gives its reply, one op after another.
            let ops = [
                replace("shelf-7", tile("27 left")),
                append("wall", (tile("a"), tile("b"))),
                crate::Op::toast(ToastTone::Success, "Restocked"),
            ];
            let crate::Op::Native(first) = &ops[0] else {
                panic!("not a native op");
            };
            assert_eq!(
                (first.how, first.target.as_str()),
                (How::Replace, "shelf-7")
            );
            // What the reply layer does with a reply's ops for a native client.
            swap::native_ops(&ops).expect("every op can be said natively")
        });
        assert!(
            matches!(&response[0], Op::Replace { target, node: Part::Card(card) }
            if target == "shelf-7" && card.children == [Part::text("27 left")])
        );
        assert!(
            matches!(&response[1], Op::Append { target, nodes, .. } if target == "wall" && nodes.len() == 2)
        );
        assert!(matches!(&response[2], Op::Toast(toast) if toast.text == "Restocked"));
    }
}
