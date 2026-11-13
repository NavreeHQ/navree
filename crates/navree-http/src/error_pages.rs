//! [`ErrorPages`]: a module's way to answer an error with something other than
//! the JSON error body, such as a page for a browser, and the request facts it
//! decides with.
//!
//! The pipeline renders every error. When a module has contributed an
//! [`ErrorPages`] ([`ModuleBuilderHttpExt::error_pages`](crate::ModuleBuilderHttpExt::error_pages)),
//! it is asked first, with an [`ErrorContext`] describing the request; an answer
//! of `None` leaves the JSON body as it is. A route that asked for JSON with
//! [`RouteDef::json_only`](crate::RouteDef::json_only) is never asked.

use std::sync::Arc;

use http::header::{self, HeaderMap, HeaderName};
use std::convert::Infallible;
use std::sync::OnceLock;

use http::{Method, Uri};
use navree_contracts::RequestId;
use navree_kernel::{AppContext, BoxFuture};
use tower::ServiceExt;
use tower::util::BoxCloneSyncService;
use uuid::Uuid;

use crate::body::Body;
use crate::extract::Request;
use crate::response::Response;
use crate::{AppError, RouteMeta, routing};

/// Answers errors in another form than JSON.
///
/// `render` returns the whole response for `error` (status and body), or `None`
/// to keep the JSON body. It runs on the error path only, never for a response
/// that succeeded. It must not panic and should not fail: when it cannot build
/// its answer it returns `None`, and the client gets the JSON body.
pub trait ErrorPages: Send + Sync + 'static {
    /// The response for `error`, or `None` for the JSON body.
    fn render<'a>(
        &'a self,
        request: &'a ErrorContext,
        error: &'a AppError,
    ) -> BoxFuture<'a, Option<Response>>;
}

/// An [`ErrorPages`] as a module contributes it.
#[derive(Clone)]
pub struct ErrorPagesHandle(pub(crate) Arc<dyn ErrorPages>);

impl ErrorPagesHandle {
    /// Wraps `pages`.
    pub fn new(pages: impl ErrorPages) -> Self {
        Self(Arc::new(pages))
    }
}

impl std::fmt::Debug for ErrorPagesHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ErrorPagesHandle")
    }
}

/// What the pipeline knows about the request that ended in an error.
#[derive(Clone, Debug)]
pub struct ErrorContext {
    /// The request's method.
    pub method: Method,
    /// The request's URI, as it arrived.
    pub uri: Uri,
    /// The headers a page can depend on: `Accept`, `Accept-Language`, `Cookie`,
    /// `Host`, `User-Agent` (a native client's web view is known by it), the
    /// `Sec-Fetch-*` and `Sec-Purpose` headers, `X-Requested-With` and every
    /// `X-Navree-*` header. Nothing else is kept: not
    /// `Authorization`, not an API key, not the body.
    pub headers: HeaderMap,
    /// The app.
    pub ctx: AppContext,
    /// The request id, when the request had one.
    pub request_id: Option<RequestId>,
    /// The route the request belongs to: the one that handled it, or, for an error
    /// raised before any route ran (a refused CSRF token, a limit, a 405), the route its
    /// path and method name (for a 405, the route of the first method the path takes).
    /// `None` when no route has that path.
    pub route: Option<Arc<RouteMeta>>,
    /// The signed-in user, when the request reached authentication and had one.
    /// A panic or a timeout cannot say, and leaves it `None`.
    pub user: Option<Uuid>,
    /// `http.api_prefix`: paths under it belong to the JSON API.
    pub api_prefix: Arc<str>,
    /// The route the request reached, filled in as it reaches one.
    pub(crate) matched: Option<Arc<OnceLock<Arc<RouteMeta>>>>,
    /// Finds a request's route without running it.
    pub(crate) finder: Option<RouteFinder>,
}

impl ErrorContext {
    /// A context for `method` and `uri` in the app `ctx`, with no headers, no request id, no
    /// route and no user, and `/api` as the API prefix. Set the public fields for what your
    /// test or tool knows.
    pub fn new(method: Method, uri: Uri, ctx: AppContext) -> Self {
        Self {
            method,
            uri,
            headers: HeaderMap::new(),
            ctx,
            request_id: None,
            route: None,
            user: None,
            api_prefix: Arc::from("/api"),
            matched: None,
            finder: None,
        }
    }

    /// The route this request belongs to, for an error. A route that ran has set it; for one
    /// raised before any route ran, the router is asked which route the path names, without
    /// running it. Only an error pays for this.
    pub(crate) async fn resolve_route(&self) -> Option<Arc<RouteMeta>> {
        if let Some(route) = self.matched.as_ref().and_then(|cell| cell.get()) {
            return Some(route.clone());
        }
        self.finder.as_ref()?.find(&self.method, &self.uri).await
    }

    /// The path of the request.
    pub fn path(&self) -> &str {
        self.uri.path()
    }

    /// Whether the request path is under `http.api_prefix`.
    pub fn is_api_path(&self) -> bool {
        under(self.uri.path(), &self.api_prefix)
    }

    /// What the `Accept` header prefers.
    pub fn accepts(&self) -> Preference {
        preference(&self.headers)
    }
}

/// Whether `path` is `prefix` or below it (`/api` holds `/api` and `/api/v1`, not `/apiary`).
fn under(path: &str, prefix: &str) -> bool {
    let prefix = prefix.trim_end_matches('/');
    !prefix.is_empty()
        && path
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// What a client asked for in `Accept`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preference {
    /// A page: `text/html` or `application/xhtml+xml` is listed, and not behind JSON.
    Html,
    /// Data: JSON is listed, and not behind HTML.
    Json,
    /// No preference: no header, only wildcards, a tie, or neither type listed.
    Either,
}

/// At most this many media ranges of one `Accept` header are read; the rest are ignored.
const MOST_RANGES: usize = 32;

/// What the `Accept` header of `headers` prefers.
///
/// The most specific range that matches a type sets its quality (RFC 9110
/// section 12.5.1). An explicit range (`text/html`, `application/json`) beats
/// a wildcard (`*/*`, `text/*`, `application/*`), so a browser's
/// `text/html,...,*/*;q=0.8` is a page and a script's `application/json, */*`
/// is data. When both are explicit the higher quality wins and a tie goes to the
/// one listed first. A malformed range counts for nothing; an absurdly long
/// header is read only as far as `MOST_RANGES` ranges.
pub fn preference(headers: &HeaderMap) -> Preference {
    let mut html = Rank::default();
    let mut json = Rank::default();
    let mut position = 0_usize;
    for value in headers.get_all(header::ACCEPT) {
        let Ok(text) = value.to_str() else { continue };
        for range in text.split(',').take(MOST_RANGES) {
            position += 1;
            let mut parts = range.split(';');
            let Some(media) = parts.next() else { continue };
            let media = media.trim().to_ascii_lowercase();
            let Some((kind, subtype)) = media.split_once('/') else {
                continue;
            };
            let quality = quality_of(parts);
            // Which of the two this range speaks for, and how exactly.
            let (for_html, for_json, exact) = match (kind, subtype) {
                ("text", "html") | ("application", "xhtml+xml") => (true, false, true),
                ("application", "json") => (false, true, true),
                ("application", s) if s.ends_with("+json") => (false, true, true),
                ("*", "*") => (true, true, false),
                ("text", "*") => (true, false, false),
                ("application", "*") => (false, true, false),
                _ => continue,
            };
            if for_html {
                html.add(quality, exact, position);
            }
            if for_json {
                json.add(quality, exact, position);
            }
        }
    }
    match (html.exact, json.exact) {
        (Some(h), Some(j)) => {
            if h.quality > j.quality || (h.quality == j.quality && h.position < j.position) {
                if h.quality > 0.0 {
                    Preference::Html
                } else {
                    Preference::Either
                }
            } else if j.quality > 0.0 {
                Preference::Json
            } else {
                Preference::Either
            }
        }
        (Some(h), None) if h.quality > 0.0 => Preference::Html,
        (None, Some(j)) if j.quality > 0.0 => Preference::Json,
        _ => Preference::Either,
    }
}

/// The quality and place of the best exact range for one type.
#[derive(Default)]
struct Rank {
    exact: Option<Found>,
}

#[derive(Clone, Copy)]
struct Found {
    quality: f32,
    position: usize,
}

impl Rank {
    fn add(&mut self, quality: f32, exact: bool, position: usize) {
        if !exact {
            return;
        }
        let better = self.exact.is_none_or(|have| {
            quality > have.quality || (quality == have.quality && position < have.position)
        });
        if better {
            self.exact = Some(Found { quality, position });
        }
    }
}

/// The `q` parameter of a media range, from 0 to 1; 1 when it has none or can't be read.
fn quality_of<'a>(parameters: impl Iterator<Item = &'a str>) -> f32 {
    for parameter in parameters {
        let Some((name, value)) = parameter.split_once('=') else {
            continue;
        };
        if name.trim().eq_ignore_ascii_case("q") {
            return value
                .trim()
                .parse::<f32>()
                .ok()
                .filter(|q| q.is_finite())
                .map_or(1.0, |q| q.clamp(0.0, 1.0));
        }
    }
    1.0
}

/// Finds the route a request names with the router's own matching, without running the route.
///
/// It is a second [`routing::Router`] with the same paths, whose services answer with the route's
/// metadata in place of its handler, so what matches here is exactly what matches there.
#[derive(Clone)]
pub(crate) struct RouteFinder {
    router: routing::Router,
}

impl RouteFinder {
    pub(crate) fn new() -> Self {
        Self {
            router: routing::Router::new(),
        }
    }

    /// Adds `meta`'s route.
    pub(crate) fn add(&mut self, method: Method, meta: &Arc<RouteMeta>) {
        let meta = meta.clone();
        // A mistake here would be the same mistake the real router already refused.
        let _ = self
            .router
            .try_route_service(method, &meta.full_path.clone(), move || {
                BoxCloneSyncService::new(tower::service_fn(move |_request: Request| {
                    let meta = meta.clone();
                    async move {
                        let mut response = Response::new(Body::empty());
                        response.extensions_mut().insert(meta);
                        Ok::<_, Infallible>(response)
                    }
                }))
            });
    }

    async fn find(&self, method: &Method, uri: &Uri) -> Option<Arc<RouteMeta>> {
        let ask = |method: &Method| {
            let mut request = Request::new(Body::empty());
            *request.method_mut() = method.clone();
            *request.uri_mut() = uri.clone();
            self.router.clone().oneshot(request)
        };
        let response = ask(method).await.ok()?;
        if let Some(meta) = response.extensions().get::<Arc<RouteMeta>>() {
            return Some(meta.clone());
        }
        // A 405 names the methods the path takes: the route of the first of them is the path's.
        let allow = response.headers().get(header::ALLOW)?.to_str().ok()?;
        for name in allow.split(',').map(str::trim) {
            if name.eq_ignore_ascii_case("OPTIONS") || name.eq_ignore_ascii_case("HEAD") {
                continue;
            }
            let Ok(other) = Method::from_bytes(name.as_bytes()) else {
                continue;
            };
            if let Ok(response) = ask(&other).await
                && let Some(meta) = response.extensions().get::<Arc<RouteMeta>>()
            {
                return Some(meta.clone());
            }
        }
        None
    }
}

impl std::fmt::Debug for RouteFinder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RouteFinder")
    }
}

/// The request headers an [`ErrorContext`] keeps: what decides a page and
/// nothing a handler or a secret could hide in.
pub(crate) fn capture(headers: &HeaderMap) -> HeaderMap {
    let mut kept = HeaderMap::with_capacity(4);
    for (name, value) in headers {
        if kept_header(name) {
            kept.append(name.clone(), value.clone());
        }
    }
    kept
}

fn kept_header(name: &HeaderName) -> bool {
    *name == header::ACCEPT
        || *name == header::ACCEPT_LANGUAGE
        || *name == header::COOKIE
        || *name == header::HOST
        || *name == header::USER_AGENT
        || name.as_str().starts_with("sec-fetch-")
        || name.as_str() == "sec-purpose"
        || name.as_str() == "x-requested-with"
        || name.as_str().starts_with("x-navree-")
}

/// How a route's errors are shown (see [`RouteDef::page`](crate::RouteDef::page)).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum RouteKind {
    /// Not said: the `Accept` header and the request's path decide.
    #[default]
    Auto,
    /// A page: its errors are pages for any client that doesn't ask for JSON alone.
    Page,
    /// Data: its errors are always JSON.
    Json,
}

#[cfg(test)]
mod tests {
    use http::header::HeaderValue;

    use super::*;

    fn accept(value: &str) -> Preference {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::ACCEPT,
            HeaderValue::from_str(value).expect("a header"),
        );
        preference(&headers)
    }

    #[test]
    fn a_browser_navigation_is_a_page() {
        assert_eq!(
            accept("text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,*/*;q=0.8"),
            Preference::Html
        );
        assert_eq!(accept("application/xhtml+xml"), Preference::Html);
        assert_eq!(accept("TEXT/HTML"), Preference::Html);
    }

    #[test]
    fn a_script_asking_for_json_gets_json() {
        assert_eq!(accept("application/json"), Preference::Json);
        assert_eq!(
            accept("application/json, text/plain, */*"),
            Preference::Json
        );
        assert_eq!(accept("application/problem+json"), Preference::Json);
        assert_eq!(
            accept("application/vnd.api+json;q=0.9, */*"),
            Preference::Json
        );
    }

    #[test]
    fn json_listed_alone_never_gets_a_page() {
        assert_eq!(accept("application/json;q=0.1"), Preference::Json);
        assert_eq!(accept("text/html;q=0, application/json"), Preference::Json);
    }

    #[test]
    fn wildcards_and_nothing_say_nothing() {
        assert_eq!(preference(&HeaderMap::new()), Preference::Either);
        assert_eq!(accept("*/*"), Preference::Either);
        assert_eq!(accept("text/*"), Preference::Either);
        assert_eq!(accept("application/*, */*;q=0.1"), Preference::Either);
        assert_eq!(accept("image/png"), Preference::Either);
        assert_eq!(accept("text/plain"), Preference::Either);
    }

    #[test]
    fn the_higher_quality_wins_and_a_tie_goes_to_the_first() {
        assert_eq!(
            accept("text/html;q=0.1, application/json"),
            Preference::Json
        );
        assert_eq!(
            accept("text/html, application/json;q=0.5"),
            Preference::Html
        );
        assert_eq!(accept("application/json, text/html"), Preference::Json);
        assert_eq!(accept("text/html, application/json"), Preference::Html);
    }

    #[test]
    fn a_page_refused_outright_is_not_a_page() {
        assert_eq!(accept("text/html;q=0"), Preference::Either);
        assert_eq!(accept("text/html;q=0, */*"), Preference::Either);
    }

    #[test]
    fn malformed_values_are_read_as_far_as_they_make_sense() {
        for value in [
            "",
            ",,,",
            ";;;",
            "text/html;q=",
            "text/html;q=abc",
            "text/html;q=NaN",
            "text/html;q=1e999",
            "text/html;q=-1",
            "text",
            "/",
            "a/b/c",
            "text/html;;;;q=0.5",
            "\u{e9}/\u{e9}",
        ] {
            // None of these panics; what each means is covered above.
            let _ = accept(value);
        }
        assert_eq!(accept("text/html;q=abc"), Preference::Html);
        assert_eq!(accept("text/html;q=1e999"), Preference::Html);
        assert_eq!(accept("text/html;q=-1"), Preference::Either);
    }

    #[test]
    fn a_header_that_is_not_text_is_ignored() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::ACCEPT,
            HeaderValue::from_bytes(b"text/html\xff").unwrap(),
        );
        assert_eq!(preference(&headers), Preference::Either);
    }

    #[test]
    fn a_very_long_header_is_read_only_so_far() {
        let long = format!("{}text/html", "image/png,".repeat(5000));
        assert_eq!(accept(&long), Preference::Either);
        let early = format!("text/html,{}", "image/png,".repeat(5000));
        assert_eq!(accept(&early), Preference::Html);
    }

    #[test]
    fn two_headers_are_read_as_one_list() {
        let mut headers = HeaderMap::new();
        headers.append(header::ACCEPT, HeaderValue::from_static("application/json"));
        headers.append(header::ACCEPT, HeaderValue::from_static("text/html;q=0.5"));
        assert_eq!(preference(&headers), Preference::Json);
    }

    #[test]
    fn api_paths_are_the_prefix_and_below() {
        assert!(under("/api", "/api"));
        assert!(under("/api/v1/deals", "/api"));
        assert!(under("/api/v1", "/api/"));
        assert!(!under("/apiary", "/api"));
        assert!(!under("/", "/api"));
        assert!(!under("/api", ""));
    }

    /// Every request pays this when a module renders error pages: it must stay a rounding error.
    #[cfg(not(debug_assertions))]
    #[test]
    fn keeping_what_a_page_needs_costs_under_2_us() {
        use std::hint::black_box;
        use std::time::{Duration, Instant};
        let mut headers = HeaderMap::new();
        for (name, value) in [
            ("host", "app.example"),
            (
                "user-agent",
                "Mozilla/5.0 (X11; Linux x86_64) Firefox/144.0",
            ),
            ("accept", "text/html,application/xhtml+xml,*/*;q=0.8"),
            ("accept-language", "en-US,en;q=0.5"),
            ("accept-encoding", "gzip, deflate, br"),
            ("referer", "https://app.example/deals"),
            ("cookie", "navree_session=abcdef; XSRF-TOKEN=abcdef"),
            ("sec-fetch-dest", "document"),
            ("sec-fetch-mode", "navigate"),
            ("sec-fetch-site", "same-origin"),
            ("upgrade-insecure-requests", "1"),
            ("x-request-id", "01a1a48c27697468b09935ddd4286017"),
        ] {
            headers.append(
                HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_static(value),
            );
        }
        black_box(capture(&headers));
        let best = (0..2000)
            .map(|_| {
                let start = Instant::now();
                black_box(capture(black_box(&headers)));
                start.elapsed()
            })
            .min()
            .unwrap();
        println!("keeping the headers a page needs takes {best:?}");
        assert!(best < Duration::from_micros(2), "{best:?}");
    }

    #[test]
    fn only_what_decides_a_page_is_kept() {
        let mut headers = HeaderMap::new();
        for (name, value) in [
            ("accept", "text/html"),
            ("cookie", "a=b"),
            ("authorization", "Bearer secret"),
            ("x-api-key", "secret"),
            ("x-navree-ui", "1"),
            ("sec-fetch-dest", "document"),
            ("user-agent", "x"),
        ] {
            headers.append(
                HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_static(value),
            );
        }
        let kept = capture(&headers);
        let mut names: Vec<&str> = kept.keys().map(HeaderName::as_str).collect();
        names.sort_unstable();
        assert_eq!(
            names,
            [
                "accept",
                "cookie",
                "sec-fetch-dest",
                "user-agent",
                "x-navree-ui"
            ]
        );
    }
}
