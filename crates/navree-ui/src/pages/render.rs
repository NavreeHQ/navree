//! The module that answers errors with pages: it decides the form an error takes for
//! the asker, and draws it.
//!
//! | The asker | The answer |
//! |---|---|
//! | A route marked `json_only`, or a client that asks for JSON alone | the JSON error body, unchanged |
//! | A route under `http.api_prefix` that is not marked `page` | the JSON error body |
//! | A browser, JavaScript on or off, and the runtime's boosted links | a page: in the shell for a signed-in user, else on its own |
//! | The runtime fetching a region | the state alone, to put where the region was |
//! | The runtime posting an action | an ops envelope with a toast, so the form stays |

use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::time::Duration;

use futures_util::FutureExt as _;

use http::header::{CACHE_CONTROL, HeaderName, HeaderValue, LOCATION, VARY};
use http::{Method, StatusCode};
use navree_http::response::{IntoResponse, Response};
use navree_http::{AppError, ErrorContext, ErrorPages, Preference, RouteKind};
use navree_kernel::BoxFuture;

use super::labels::{StateKind, StateLabels};
use super::options::PagesOptions;
use super::return_to::{ReturnTo, is_at_or_below};
use super::state::ErrorPage;
use crate::components::Toast;
use crate::html::{Href, LocalHref};
use crate::module::UiAlways;
use crate::node::{Node, el};
use crate::op::{Envelope, Op, ToastTone, UI_HEADER};
use crate::shell::Shell;

/// The request header the runtime adds to a region's fetch, naming the region.
pub const REGION_HEADER: &str = "x-navree-region";

/// The request header the runtime adds to a region's fetch and an action: the address of
/// the page the user is on, which is where to come back to after signing in.
pub const FROM_HEADER: &str = "x-navree-from";

/// The response header that says a non-`2xx` answer is a state the runtime may show in
/// the region it asked for.
pub const STATE_HEADER: &str = "x-navree-state";

/// How a shell is built for a page when the app has not registered one `Shell`
/// provider for everyone: a module contributes this to build the shell of a request,
/// with its user's name in the menu. Return `None` for a request that has no shell.
///
/// It gets the [`ErrorContext`] of the request, owned, so the future it returns can keep it.
/// It runs only for an error, and it has one second: a builder that is slower, that panics or
/// that returns `None` leaves the page on its own, with no sidebar.
///
/// ```
/// use navree_http::ErrorContext;
/// use navree_kernel::ModuleBuilder;
/// use navree_ui::{ErrorShell, NavGroup, NavItem, Shell, icons};
/// use uuid::Uuid;
///
/// async fn shell_for(user: Option<Uuid>) -> Option<Shell> {
///     user.map(|_| {
///         Shell::new("Agra").group(
///             NavGroup::plain().item(NavItem::new("Deals", "/deals").icon(icons::HANDSHAKE)),
///         )
///     })
/// }
///
/// fn register(m: &mut ModuleBuilder) {
///     m.contribute(ErrorShell::new(|request| async move { shell_for(request.user).await }));
/// }
/// # let _: fn(ErrorContext) = |_| {};
/// ```
#[derive(Clone)]
pub struct ErrorShell(ShellBuilder);

/// Builds the shell of the request it is given.
type ShellBuilder = Arc<dyn Fn(ErrorContext) -> BoxFuture<'static, Option<Shell>> + Send + Sync>;

impl ErrorShell {
    /// A builder of shells.
    pub fn new<F, Fut>(build: F) -> Self
    where
        F: Fn(ErrorContext) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Option<Shell>> + Send + 'static,
    {
        Self(Arc::new(move |request| Box::pin(build(request))))
    }
}

impl std::fmt::Debug for ErrorShell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ErrorShell")
    }
}

/// What an error is answered with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Form {
    /// The JSON error body.
    Json,
    /// A page.
    Document,
    /// The state alone, for the region the runtime named.
    Region,
    /// An ops envelope.
    Action,
}

/// The form `request` gets.
pub(crate) fn form_of(request: &ErrorContext) -> Form {
    let kind = request
        .route
        .as_ref()
        .map_or(RouteKind::Auto, |route| route.kind);
    if kind == RouteKind::Json {
        return Form::Json;
    }
    // The runtime only asks for what the app wrote for it: pages, fragments and actions.
    if request.headers.contains_key(UI_HEADER) {
        return if request.method == Method::GET || request.method == Method::HEAD {
            if request.headers.contains_key(REGION_HEADER) {
                Form::Region
            } else {
                Form::Document
            }
        } else {
            Form::Action
        };
    }
    match (kind, request.accepts()) {
        // A page asked for in JSON alone is data.
        (RouteKind::Page, Preference::Json) => Form::Json,
        (RouteKind::Page, _) => Form::Document,
        (_, Preference::Html) if !request.is_api_path() => Form::Document,
        _ => Form::Json,
    }
}

/// The module registered with `ModuleBuilderHttpExt::error_pages`.
pub(crate) struct Renderer;

impl ErrorPages for Renderer {
    fn render<'a>(
        &'a self,
        request: &'a ErrorContext,
        error: &'a AppError,
    ) -> BoxFuture<'a, Option<Response>> {
        Box::pin(render(request, error))
    }
}

async fn render(request: &ErrorContext, error: &AppError) -> Option<Response> {
    // Only an app that named `UiModule` has pages; a module that borrows the toolkit does not.
    if request.ctx.contributions::<UiAlways>().is_empty() {
        return None;
    }
    let form = form_of(request);
    if form == Form::Json {
        return None;
    }
    let options = request.ctx.try_get::<PagesOptions>().await?;
    let mut response = match form {
        Form::Json => return None,
        Form::Document => document(request, error, &options).await,
        Form::Region => region(request, error, &options),
        Form::Action => action(request, error, &options),
    };
    let headers = response.headers_mut();
    // `Retry-After`, `Allow`, `WWW-Authenticate`: the error's own headers belong to the answer.
    for (name, value) in error.headers.iter() {
        headers.insert(name.clone(), value.clone());
    }
    headers.append(VARY, HeaderValue::from_static("accept, x-navree-ui"));
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Some(response)
}

/// The error as an [`ErrorPage`], with what the request knows about where to go next.
fn page_of(request: &ErrorContext, error: &AppError, options: &PagesOptions) -> ErrorPage {
    let kind = StateKind::of(error.status, &error.code);
    let mut page = ErrorPage::new(error.status)
        .kind(kind)
        .labels(options.labels.clone())
        .home(Href::from(options.home.as_str()));
    // The error's own message, for what the person who asked can act on. A server failure's
    // words are ours: what went wrong inside never goes to the screen.
    if let Some(detail) = shown_detail(error, kind) {
        page = page.detail(detail);
    }
    if let Some(id) = &request.request_id {
        page = page.reference(id.as_str());
    }
    if let Some(seconds) = retry_after(error) {
        page = page.retry_after(seconds);
    }
    // A link repeats a `GET`. After a form posted without script, "Reload" and "Try again" would
    // be a `GET` of an address that only takes a `POST`, and a sign-in that comes back there would
    // land on a 405: such a page offers Home, and a plain sign-in.
    let repeatable = matches!(request.method, Method::GET | Method::HEAD);
    let here = repeatable.then(|| ReturnTo::of_uri(&request.uri)).flatten();
    if !repeatable {
        page = page.without_retry();
    } else if let Some(here) = &here {
        page = page.retry_to(Href::from(here.as_str()));
    }
    let back = back_to(request, &options.sign_in, here);
    page = page.sign_in(match &back {
        Some(back) => back.sign_in_href(&options.sign_in),
        None => Href::from(options.sign_in.as_str()),
    });
    if options.debug {
        page = page.debug(debug_details(request, error));
    }
    page
}

/// Where to come back to after signing in: the page the user is on. A region fetch or an
/// action names it in `X-Navree-From`; a navigation is the request itself.
fn back_to(request: &ErrorContext, sign_in: &str, here: Option<ReturnTo>) -> Option<ReturnTo> {
    let named = request
        .headers
        .get(FROM_HEADER)
        .and_then(|value| value.to_str().ok());
    match named {
        Some(from) => ReturnTo::parse_for(from, sign_in),
        None => here.and_then(|found| ReturnTo::parse_for(found.as_str(), sign_in)),
    }
}

/// The message an error may show, trimmed and short.
fn shown_detail(error: &AppError, kind: StateKind) -> Option<String> {
    if kind == StateKind::Failed {
        return None;
    }
    let text = match (&error.errors, &error.detail) {
        (Some(fields), _) if !fields.is_empty() => fields
            .values()
            .flatten()
            .cloned()
            .collect::<Vec<_>>()
            .join(" "),
        (_, Some(detail)) => detail.clone(),
        _ => return None,
    };
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    // The words the state already says are not said twice.
    if text.eq_ignore_ascii_case(&error.title) {
        return None;
    }
    Some(cut(text, 300))
}

/// `text` cut to at most `most` characters, with an ellipsis when it was longer.
fn cut(text: &str, most: usize) -> String {
    match text.char_indices().nth(most) {
        Some((at, _)) => format!("{}\u{2026}", text[..at].trim_end()),
        None => text.to_owned(),
    }
}

/// `Retry-After` in whole seconds, when it is a number of seconds.
fn retry_after(error: &AppError) -> Option<u64> {
    error
        .headers
        .get(http::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .filter(|seconds| (1..=86_400).contains(seconds))
}

/// What a development build adds: everything the log has.
fn debug_details(request: &ErrorContext, error: &AppError) -> Node {
    let mut rows: Vec<(&str, String)> = vec![
        (
            "Status",
            format!("{} {}", error.status.as_u16(), error.code),
        ),
        (
            "Request",
            format!("{} {}", request.method, request.uri.path()),
        ),
    ];
    if let Some(route) = &request.route {
        rows.push((
            "Route",
            route
                .name
                .clone()
                .unwrap_or_else(|| route.full_path.clone()),
        ));
    }
    if let Some(detail) = &error.detail {
        rows.push(("Message", detail.clone()));
    }
    let mut cause = String::new();
    let mut next: Option<&(dyn std::error::Error + 'static)> = error
        .source
        .as_deref()
        .map(|source| source as &(dyn std::error::Error + 'static));
    while let Some(source) = next {
        if !cause.is_empty() {
            cause.push_str("\ncaused by: ");
        }
        cause.push_str(&source.to_string());
        next = source.source();
    }
    if !cause.is_empty() {
        rows.push(("Cause", cut(&cause, 4000)));
    }
    let mut list = el("dl");
    for (name, value) in rows {
        list = list.child((
            el("dt").child(name),
            el("dd").child(el("code").attr("dir", "ltr").child(value)),
        ));
    }
    list.into()
}

/// A page: in the shell for a signed-in user, on its own for anyone else.
async fn document(request: &ErrorContext, error: &AppError, options: &PagesOptions) -> Response {
    let page = page_of(request, error, options);
    let kind = StateKind::of(error.status, &error.code);
    // Not from the sign-in page itself, or from below it: when it answers a guest with a 401 the
    // redirect would be to the page it came from, round and round.
    if kind == StateKind::SignIn
        && options.redirect_guests
        && request.method == Method::GET
        && !is_at_or_below(request.uri.path(), &options.sign_in)
    {
        let target = back_to(request, &options.sign_in, ReturnTo::of_uri(&request.uri))
            .map_or_else(
                || Href::from(options.sign_in.as_str()),
                |back| back.sign_in_href(&options.sign_in),
            );
        if let Ok(location) = HeaderValue::from_str(target.as_str()) {
            let mut response = StatusCode::SEE_OTHER.into_response();
            response.headers_mut().insert(LOCATION, location);
            return response;
        }
    }
    // A guest, and a request for something that must work when much else does not, get the page
    // on its own.
    let wants_shell = request.user.is_some()
        && !matches!(
            kind,
            StateKind::SignIn | StateKind::RateLimited | StateKind::Unavailable
        );
    if wants_shell && let Some(shell) = shell_of(request).await {
        let app = shell.for_request(request.uri.path(), &request.headers);
        return page.in_shell(&app).into_response();
    }
    page.into_view().into_response()
}

/// How long the app's shell gets to be built. The page on its own is the answer when it takes
/// longer; the hook that calls this has three seconds in all.
const SHELL_DEADLINE: Duration = Duration::from_secs(1);

/// The shell of the app for this request, when it has one. What builds it is the app's own
/// code: one that panics, or that never answers, leaves the page on its own, and does not take
/// the answer down with it.
pub(crate) async fn shell_of(request: &ErrorContext) -> Option<Shell> {
    let attempt = AssertUnwindSafe(build_shell(request)).catch_unwind();
    match tokio::time::timeout(SHELL_DEADLINE, attempt).await {
        Ok(Ok(shell)) => shell,
        _ => None,
    }
}

async fn build_shell(request: &ErrorContext) -> Option<Shell> {
    if let Some(build) = request.ctx.contributions::<ErrorShell>().into_iter().next() {
        return (build.value.0)(request.clone()).await;
    }
    request
        .ctx
        .try_get::<Shell>()
        .await
        .map(|shell| (*shell).clone())
}

/// The state alone, for the region that asked.
fn region(request: &ErrorContext, error: &AppError, options: &PagesOptions) -> Response {
    let page = page_of(request, error, options);
    let mut response = page.into_fragment().into_response();
    // Not every answer is for the region to show: the runtime shows the ones that say so.
    if let Ok(value) = HeaderValue::from_str(&error.status.as_u16().to_string()) {
        response
            .headers_mut()
            .insert(HeaderName::from_static(STATE_HEADER), value);
    }
    response
}

/// An ops envelope for an action: a toast that says what happened, and for a guest the
/// way to sign in.
fn action(request: &ErrorContext, error: &AppError, options: &PagesOptions) -> Response {
    let kind = StateKind::of(error.status, &error.code);
    let tone = match kind {
        StateKind::SignIn | StateKind::Conflict | StateKind::Expired | StateKind::RateLimited => {
            ToastTone::Warning
        }
        _ => ToastTone::Danger,
    };
    let mut text = options.labels.words(kind).toast.to_string();
    if let Some(detail) = shown_detail(error, kind)
        && matches!(
            kind,
            StateKind::Conflict
                | StateKind::Invalid
                | StateKind::Client
                | StateKind::Forbidden
                | StateKind::Unavailable
        )
    {
        text = detail;
    }
    if let Some(seconds) = retry_after(error)
        && kind == StateKind::RateLimited
    {
        text = format!("{text} {}", options.labels.wait_text(seconds));
    }
    if kind == StateKind::Failed
        && let Some(id) = &request.request_id
    {
        text = format!("{text} {} {}.", options.labels.reference, id.as_str());
    }
    let mut ops = vec![Op::toast_with(Toast::new(tone, text))];
    if kind == StateKind::SignIn {
        let back = back_to(request, &options.sign_in, None);
        let target = back.map_or_else(
            || Href::from(options.sign_in.as_str()),
            |back| back.sign_in_href(&options.sign_in),
        );
        ops.push(match LocalHref::parse(target.as_str()) {
            Ok(local) => Op::navigate(local),
            Err(_) => Op::navigate_external(target),
        });
    }
    let mut response = Envelope::new(ops).into_response();
    *response.status_mut() = error.status;
    response
}

/// Words for tests and docs: the labels as a page would use them.
#[allow(dead_code)]
pub(crate) fn default_labels() -> StateLabels {
    StateLabels::default()
}

#[cfg(test)]
mod tests {
    use http::{HeaderMap, Method, Uri};
    use navree_http::RouteMeta;

    use super::*;

    fn context(method: Method, uri: &str, headers: &[(&str, &str)]) -> ErrorContext {
        context_in(&app(), method, uri, headers)
    }

    /// An app with nothing in it, booted once for the tests that need a context.
    fn app() -> navree_kernel::AppContext {
        static APP: std::sync::OnceLock<navree_kernel::AppContext> = std::sync::OnceLock::new();
        APP.get_or_init(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .build()
                .expect("a runtime");
            runtime.block_on(async {
                navree_kernel::Navree::app()
                    .boot()
                    .await
                    .expect("an empty app boots")
                    .ctx()
                    .clone()
            })
        })
        .clone()
    }

    fn context_in(
        ctx: &navree_kernel::AppContext,
        method: Method,
        uri: &str,
        headers: &[(&str, &str)],
    ) -> ErrorContext {
        let mut map = HeaderMap::new();
        for (name, value) in headers {
            map.append(
                HeaderName::from_bytes(name.as_bytes()).expect("a name"),
                HeaderValue::from_str(value).expect("a value"),
            );
        }
        let mut context =
            ErrorContext::new(method, uri.parse::<Uri>().expect("a uri"), ctx.clone());
        context.headers = map;
        context
    }

    fn with_route(mut request: ErrorContext, kind: RouteKind) -> ErrorContext {
        request.route = Some(Arc::new(RouteMeta {
            kind,
            ..RouteMeta::default()
        }));
        request
    }

    #[test]
    fn a_browser_navigation_gets_a_page_and_a_script_gets_json() {
        let html = "text/html,application/xhtml+xml,*/*;q=0.8";
        let page =
            |uri: &str, accept: &str| form_of(&context(Method::GET, uri, &[("accept", accept)]));
        assert_eq!(page("/deals", html), Form::Document);
        assert_eq!(page("/deals", "application/json"), Form::Json);
        assert_eq!(page("/deals", "*/*"), Form::Json);
        assert_eq!(
            page("/deals", "application/json, text/plain, */*"),
            Form::Json
        );
        assert_eq!(
            form_of(&context(Method::GET, "/deals", &[])),
            Form::Json,
            "curl asks for nothing in particular"
        );
    }

    #[test]
    fn the_api_stays_json_even_for_a_browser() {
        let html = [("accept", "text/html")];
        assert_eq!(
            form_of(&context(Method::GET, "/api/v1/deals", &html)),
            Form::Json
        );
        assert_eq!(form_of(&context(Method::GET, "/api", &html)), Form::Json);
        assert_eq!(
            form_of(&context(Method::GET, "/apiary", &html)),
            Form::Document
        );
        // A route that says it is a page is one wherever it is mounted.
        let page = with_route(context(Method::GET, "/api/pages/x", &html), RouteKind::Page);
        assert_eq!(form_of(&page), Form::Document);
    }

    #[test]
    fn a_marked_route_is_decided_by_its_mark() {
        let json = with_route(
            context(Method::GET, "/deals", &[("accept", "text/html")]),
            RouteKind::Json,
        );
        assert_eq!(form_of(&json), Form::Json);
        let json = with_route(
            context(
                Method::GET,
                "/deals",
                &[("accept", "text/html"), (UI_HEADER, "1")],
            ),
            RouteKind::Json,
        );
        assert_eq!(form_of(&json), Form::Json, "even the runtime");
        for accept in [None, Some("*/*"), Some("text/html"), Some("text/*")] {
            let headers: Vec<(&str, &str)> = accept.map(|a| ("accept", a)).into_iter().collect();
            let page = with_route(context(Method::GET, "/deals", &headers), RouteKind::Page);
            assert_eq!(form_of(&page), Form::Document, "{accept:?}");
        }
        let asked_for_data = with_route(
            context(Method::GET, "/deals", &[("accept", "application/json")]),
            RouteKind::Page,
        );
        assert_eq!(form_of(&asked_for_data), Form::Json);
    }

    #[test]
    fn the_runtime_gets_what_it_can_use() {
        let ui = [(UI_HEADER, "1")];
        assert_eq!(
            form_of(&context(Method::GET, "/deals", &ui)),
            Form::Document
        );
        assert_eq!(
            form_of(&context(Method::GET, "/api/x", &ui)),
            Form::Document
        );
        let region = [(UI_HEADER, "1"), (REGION_HEADER, "kpis")];
        assert_eq!(
            form_of(&context(Method::GET, "/frag", &region)),
            Form::Region
        );
        assert_eq!(
            form_of(&context(Method::HEAD, "/frag", &region)),
            Form::Region
        );
        for method in [Method::POST, Method::PUT, Method::PATCH, Method::DELETE] {
            assert_eq!(form_of(&context(method, "/deals", &region)), Form::Action);
        }
        assert_eq!(form_of(&context(Method::POST, "/deals", &ui)), Form::Action);
        // The region header alone is not the runtime.
        assert_eq!(
            form_of(&context(Method::GET, "/frag", &[(REGION_HEADER, "kpis")])),
            Form::Json
        );
    }

    #[test]
    fn a_plain_form_post_in_a_browser_gets_a_page() {
        let post = context(Method::POST, "/deals", &[("accept", "text/html,*/*;q=0.8")]);
        assert_eq!(form_of(&post), Form::Document);
    }

    #[test]
    fn a_long_message_is_cut_and_a_server_failure_says_nothing_of_itself() {
        let long = "x".repeat(1000);
        let error = AppError::conflict(long);
        let shown = shown_detail(&error, StateKind::Conflict).unwrap();
        assert!(shown.chars().count() <= 301, "{}", shown.len());
        assert!(shown.ends_with('\u{2026}'));
        let internal = AppError::internal("password=hunter2").with_detail("SELECT * FROM users");
        assert_eq!(shown_detail(&internal, StateKind::Failed), None);
        assert_eq!(
            shown_detail(&AppError::forbidden(), StateKind::Forbidden),
            None
        );
        assert_eq!(
            shown_detail(&AppError::conflict("Email taken"), StateKind::Conflict).as_deref(),
            Some("Email taken")
        );
        assert_eq!(cut("abcdef", 3), "abc\u{2026}");
        assert_eq!(cut("abc", 3), "abc");
        assert_eq!(cut("\u{e9}t\u{e9}", 2), "\u{e9}t\u{2026}");
        assert_eq!(cut("", 3), "");
    }

    #[test]
    fn a_wait_is_a_number_of_seconds_in_a_sane_range() {
        let with = |value: &str| {
            AppError::too_many_requests(std::time::Duration::from_secs(1)).with_header(
                http::header::RETRY_AFTER,
                HeaderValue::from_str(value).unwrap(),
            )
        };
        assert_eq!(retry_after(&with("12")), Some(12));
        assert_eq!(retry_after(&with(" 7 ")), Some(7));
        assert_eq!(retry_after(&with("0")), None);
        assert_eq!(retry_after(&with("999999999")), None);
        assert_eq!(retry_after(&with("Wed, 21 Oct 2026 07:28:00 GMT")), None);
        assert_eq!(retry_after(&AppError::not_found()), None);
    }
}
